//! The permission broker (#20): between an agent asking and a human answering.
//!
//! #10 already carries `session/request_permission` off the adapter and
//! `permission/reply` back to it, and #15 already decides *whether* to ask —
//! Wait for Inbox answers a read on a folded thread and never an execute
//! (decision #5). This module is the layer between those two: it owns the
//! record of what was asked, decides what a second answer means, and is the
//! only place that writes to `permission_requests`.
//!
//! Three rules shape it.
//!
//! **An ask is on disk before it is announced.** The same persist-then-notify
//! rule the Inbox follows, applied to the question instead of the result: a
//! notification nobody receives loses nothing, but a quit while a request is
//! outstanding would otherwise lose what the agent wanted to do. #21 already
//! resurfaces such a thread as `needs_you`; the row is what lets the card say
//! "Run ls" instead of "something".
//!
//! **Delivery is separate from resolution.** The ACP request id lives on a
//! live adapter call and dies with it. An answer given after that — the
//! restart case, or the click that raced the adapter's exit — resolves the
//! record and reports `delivered: false`. Nothing here replays a dead RPC.
//!
//! **Answering twice is not an error.** Two clicks, or a click and a cancelled
//! turn, resolve to whichever got there first, and the second call returns
//! that decision. The claim is a guarded `UPDATE`, so "who got there first" is
//! decided by SQLite rather than by a read the caller did a moment ago.

use serde_json::{json, Value};
use uuid::Uuid;

use super::lifecycle::{self, PermissionDisposition};
use super::protocol::error::RpcError;
use super::protocol::jsonrpc::RequestId;
use super::protocol::methods::{
    PendingPermissionView, PermissionPendingParams, PermissionPendingResult, PermissionReplyParams,
    PermissionReplyResult,
};
use super::store::{
    NewPermissionRequest, PermissionRequestRow, ASK_ANSWERED, ASK_CANCELLED, ASK_PENDING,
};
use super::HostSession;

/// `decided_by` for a request the host answered itself under Wait for Inbox,
/// or withdrew because the turn ended. Never a device id: no human chose it.
const HOST_DECIDED: &str = "host";

/// The live half of an outstanding ask: the adapter call blocked on it, plus
/// what the agent said, so a host with no store can still draw the card.
///
/// RAM only, and deliberately so — `acp_id` is meaningless to the next process
/// and everything durable about the ask is a row in `permission_requests`.
#[derive(Debug)]
pub(crate) struct PendingPermission {
    pub(crate) thread_id: String,
    pub(crate) acp_id: RequestId,
    pub(crate) title: String,
    pub(crate) kind: Option<String>,
    pub(crate) subject: Value,
    pub(crate) options: Value,
    pub(crate) created_at: String,
}

/// Why a live ask is being taken off the wire without a human answering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Withdrawal {
    /// The turn ended: the user cancelled, or the adapter died holding the
    /// ask. Nobody will ever answer it, so the record resolves `cancelled`.
    Cancelled,
    /// The *host* is going away. The question outlives us and the record stays
    /// `pending`, because `state-machine.md` promises the next launch
    /// resurfaces this thread as `needs_you` with the ask still on it.
    Abandoned,
}

impl HostSession {
    // ---- from the adapter ---------------------------------------------

    /// An agent asked to do something. Apply the policy, then either answer it
    /// or put it in front of the human.
    pub(crate) fn open_permission_request(
        &mut self,
        thread_id: &str,
        acp_id: RequestId,
        params: &Value,
    ) {
        let subject = params
            .get("subject")
            .cloned()
            .or_else(|| params.get("toolCall").cloned())
            .unwrap_or_else(|| params.clone());
        let options = params.get("options").cloned().unwrap_or_else(|| json!([]));
        self.persist_transcript_event(thread_id, "session/request_permission", params);

        let request_id = Uuid::new_v4().to_string();
        let pending = PendingPermission {
            thread_id: thread_id.to_string(),
            acp_id,
            title: subject_title(&subject),
            kind: lifecycle::permission_kind(&subject),
            subject,
            options,
            created_at: super::store::now_utc(),
        };
        // Written before either branch acts, so the ledger holds every ask the
        // host ever took — including the ones it answered itself. What the
        // host decided while the user was away is worth recording for exactly
        // the same reason as what it asked them.
        self.record_permission_request(&request_id, &pending);

        // Wait for Inbox is a host-side permission policy on a folded thread
        // (#5, #15): reads are answered here, everything else reaches a human.
        match self.lifecycle_permission_policy(thread_id, &pending.subject, &pending.options) {
            PermissionDisposition::AutoAllow { option_id } => {
                let delivered = self.answer_agent(thread_id, pending.acp_id, selected(&option_id));
                self.resolve_permission_record(
                    &request_id,
                    ASK_ANSWERED,
                    HOST_DECIDED,
                    Some(&option_id),
                    delivered,
                );
                self.lifecycle_record_auto_allow(thread_id, &pending.subject, &option_id);
            }
            PermissionDisposition::Ask => {
                let subject = pending.subject.clone();
                let options = pending.options.clone();
                self.pending_permissions.insert(request_id.clone(), pending);
                self.notify_permission_ask(thread_id, &request_id, subject.clone(), options);
                self.lifecycle_on_permission_pending(thread_id, &subject);
            }
        }
    }

    /// Take every live ask on a thread off the wire.
    ///
    /// The agent is answered `cancelled` either way — one blocked on a request
    /// it will never get an answer to has no reason to act on the cancel that
    /// follows (#10's ordering claim). What differs is the *record*: see
    /// [`Withdrawal`].
    pub(crate) fn withdraw_pending_permissions(
        &mut self,
        thread_id: &str,
        reason: &str,
        how: Withdrawal,
    ) {
        let ids: Vec<String> = self
            .pending_permissions
            .iter()
            .filter(|(_, pending)| pending.thread_id == thread_id)
            .map(|(id, _)| id.clone())
            .collect();
        for request_id in ids {
            let Some(pending) = self.pending_permissions.remove(&request_id) else {
                continue;
            };
            let delivered = self.answer_agent(thread_id, pending.acp_id, cancelled_outcome());
            if !delivered {
                eprintln!(
                    "could not tell {thread_id}'s agent that {request_id} was withdrawn ({reason})"
                );
            }
            if how == Withdrawal::Abandoned {
                continue;
            }
            self.resolve_permission_record(
                &request_id,
                ASK_CANCELLED,
                HOST_DECIDED,
                None,
                delivered,
            );
            let device = self
                .connected_device_id
                .clone()
                .unwrap_or_else(|| HOST_DECIDED.into());
            self.notify_permission_resolved(thread_id, &request_id, &device, None, Some(true));
        }
    }

    /// Outstanding `session/request_permission` calls on a thread. The
    /// lifecycle layer asks because a blocked thread is Needs you, never stuck.
    pub(crate) fn pending_permission_count(&self, thread_id: &str) -> usize {
        self.pending_permissions
            .values()
            .filter(|pending| pending.thread_id == thread_id)
            .count()
    }

    // ---- client methods -----------------------------------------------

    /// Answer an ask. Idempotent, and honest about whether the agent heard it.
    pub fn permission_reply(
        &mut self,
        params: PermissionReplyParams,
    ) -> Result<PermissionReplyResult, RpcError> {
        let request_id = params.request_id.clone();
        let record = self.permission_record(&request_id);
        // Already decided: the second click, or one that raced the turn being
        // cancelled. Answered before anything is removed or sent, so a repeat
        // is a read — it cannot take a live ask off the wire without
        // answering it, and it cannot tell the agent twice.
        if let Some(row) = record.as_ref().filter(|row| row.state != ASK_PENDING) {
            return Ok(resolved_result(row));
        }
        // Removing the live entry *is* the claim on the delivery half: the
        // host handles one request at a time, so exactly one caller can find
        // it there, and a second click therefore cannot reach `respond` twice.
        let live = self.pending_permissions.remove(&request_id);
        // Neither half has heard of this id: a card from another host, a
        // thread that was deleted, or a typo. There is nothing to be
        // idempotent about, so this stays the error it has always been.
        let Some(thread_id) = live
            .as_ref()
            .map(|pending| pending.thread_id.clone())
            .or_else(|| record.as_ref().map(|row| row.thread_id.clone()))
        else {
            return Err(RpcError::InvalidParams(format!(
                "unknown permission requestId {request_id}"
            )));
        };

        let cancelled = params.cancelled.unwrap_or(false);
        let outcome = match (&params.option_id, cancelled) {
            (Some(option_id), false) => selected(option_id),
            _ => cancelled_outcome(),
        };
        let delivered = match live {
            Some(pending) => self.answer_agent(&thread_id, pending.acp_id, outcome),
            // The ask outlived the adapter that could have been told: a quit
            // and a restart, or a crash between the card and the click. The
            // decision is still recorded — that is what makes an ask taken
            // while the app was closed answerable when it reopens.
            None => false,
        };
        let claimed = self.resolve_permission_record(
            &request_id,
            if cancelled {
                ASK_CANCELLED
            } else {
                ASK_ANSWERED
            },
            &params.device_id,
            params.option_id.as_deref(),
            delivered,
        );
        // Lost the claim to a resolution that landed between the read above
        // and this write. Report what actually stands rather than what this
        // call intended.
        if !claimed {
            if let Some(row) = self
                .permission_record(&request_id)
                .filter(|row| row.state != ASK_PENDING)
            {
                return Ok(resolved_result(&row));
            }
        }
        self.notify_permission_resolved(
            &thread_id,
            &request_id,
            &params.device_id,
            params.option_id.clone(),
            params.cancelled,
        );
        // Only when the agent heard it. Putting a run back to `running`
        // because of an answer no process is acting on would be the ledger
        // asserting work that is not happening — the thing #21's boot pass
        // exists to stop.
        if delivered {
            self.lifecycle_on_permission_answered(&thread_id, cancelled);
        }
        Ok(PermissionReplyResult {
            request_id,
            delivered,
            already_answered: false,
            option_id: params.option_id,
            cancelled,
        })
    }

    /// Every ask still waiting on a human, oldest first — the live ones and
    /// the ones a previous host left behind, in one list.
    ///
    /// Not served from the store alone: an ephemeral host (no SQLite) still
    /// brokers permissions, and its live asks are as real as any row.
    pub fn permission_pending(
        &self,
        params: PermissionPendingParams,
    ) -> Result<PermissionPendingResult, RpcError> {
        let wanted = params.thread_id.as_deref();
        let mut requests: Vec<PendingPermissionView> = self
            .pending_permissions
            .iter()
            .filter(|(_, pending)| wanted.is_none_or(|id| pending.thread_id == id))
            .map(|(request_id, pending)| PendingPermissionView {
                request_id: request_id.clone(),
                thread_id: pending.thread_id.clone(),
                title: pending.title.clone(),
                kind: pending.kind.clone(),
                subject: pending.subject.clone(),
                options: pending.options.clone(),
                created_at: pending.created_at.clone(),
                stale: false,
            })
            .collect();
        if let Some(store) = self.store.as_ref() {
            let rows = store
                .list_open_permission_requests(wanted)
                .map_err(|err| RpcError::Internal(err.to_string()))?;
            for row in rows {
                if self.pending_permissions.contains_key(&row.id) {
                    continue;
                }
                requests.push(stale_view(row));
            }
        }
        // Oldest first, so a thread that accumulated two asks before a quit
        // reads in the order the agent made them.
        requests.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.request_id.cmp(&b.request_id))
        });
        Ok(PermissionPendingResult { requests })
    }

    // ---- the record ---------------------------------------------------

    fn record_permission_request(&self, request_id: &str, pending: &PendingPermission) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let run_id = self.open_run(&pending.thread_id).map(|(id, _)| id);
        let new = NewPermissionRequest {
            id: request_id.to_string(),
            thread_id: pending.thread_id.clone(),
            run_id,
            kind: pending.kind.clone(),
            title: pending.title.clone(),
            subject_json: pending.subject.to_string(),
            options_json: pending.options.to_string(),
        };
        // A thread with no row — an ephemeral prompt, or a test driving the
        // adapter directly — cannot have a request row either (the foreign key
        // says so). That must not cost the user the *ask*: the live half
        // stands on its own and the card still appears. What is lost is only
        // the durability, and only for a thread nothing else persists anyway.
        if let Err(err) = store.insert_permission_request(&new) {
            eprintln!("could not record permission request {request_id}: {err}");
        }
    }

    fn permission_record(&self, request_id: &str) -> Option<PermissionRequestRow> {
        self.store
            .as_ref()?
            .get_permission_request(request_id)
            .unwrap_or_else(|err| {
                eprintln!("could not read permission request {request_id}: {err}");
                None
            })
    }

    /// `true` when this call is the one that resolved the row.
    fn resolve_permission_record(
        &self,
        request_id: &str,
        state: &str,
        decided_by: &str,
        option_id: Option<&str>,
        delivered: bool,
    ) -> bool {
        let Some(store) = self.store.as_ref() else {
            // Nothing durable to claim; the live entry was the claim.
            return true;
        };
        match store.resolve_permission_request(request_id, state, decided_by, option_id, delivered)
        {
            Ok(claimed) => claimed,
            Err(err) => {
                eprintln!("could not resolve permission request {request_id}: {err}");
                true
            }
        }
    }

    /// Hand an outcome to the adapter. `false` when there is no adapter left —
    /// which is a fact about the world, not a failure of the call.
    fn answer_agent(&self, thread_id: &str, acp_id: RequestId, outcome: Value) -> bool {
        let Some(conn) = self.connections.get(thread_id) else {
            return false;
        };
        match conn.respond(acp_id, outcome) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("could not answer {thread_id}'s permission request: {err}");
                false
            }
        }
    }
}

fn resolved_result(row: &PermissionRequestRow) -> PermissionReplyResult {
    PermissionReplyResult {
        request_id: row.id.clone(),
        delivered: row.delivered,
        already_answered: true,
        option_id: row.option_id.clone(),
        cancelled: row.state == ASK_CANCELLED,
    }
}

fn stale_view(row: PermissionRequestRow) -> PendingPermissionView {
    PendingPermissionView {
        request_id: row.id,
        thread_id: row.thread_id,
        title: row.title,
        kind: row.kind,
        // A row that will not parse is a row we wrote badly; it is still an
        // ask that happened, so it comes back as a string rather than taking
        // the whole list down with it.
        subject: serde_json::from_str(&row.subject_json).unwrap_or(Value::String(row.subject_json)),
        options: serde_json::from_str(&row.options_json).unwrap_or_else(|_| json!([])),
        created_at: row.created_at,
        stale: true,
    }
}

/// What a card says when the agent gave it no title.
fn subject_title(subject: &Value) -> String {
    subject
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "waiting on your answer".to_string())
}

fn selected(option_id: &str) -> Value {
    json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
}

fn cancelled_outcome() -> Value {
    json!({ "outcome": { "outcome": "cancelled" } })
}
