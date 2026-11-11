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

use super::backend::InteractionId;
use super::lifecycle::{self, PermissionDisposition};
use super::protocol::error::RpcError;
use super::protocol::methods::{
    AskKind, InteractionOutcome, PendingPermissionView, PermissionPendingParams,
    PermissionPendingResult, PermissionReplyParams, PermissionReplyResult,
};
use super::store::{
    NewPermissionRequest, PermissionRequestRow, ASK_ANSWERED, ASK_CANCELLED, ASK_PENDING,
};
use super::HostSession;

/// `decided_by` for a request the host answered itself under Wait for Inbox,
/// or withdrew because the turn ended. Never a device id: no human chose it.
pub(crate) const HOST_DECIDED: &str = "host";

/// The ACP method a permission arrives on; the `method` a permission row keeps.
pub(crate) const REQUEST_PERMISSION: &str = "session/request_permission";

/// The live half of an outstanding ask: the adapter call blocked on it, plus
/// what the agent said, so a host with no store can still draw the card.
///
/// RAM only, and deliberately so — `interaction` is meaningless to the next
/// process and everything durable about the ask is a row in
/// `permission_requests`.
///
/// Since #298 this is also the live half of a question or plan: same map, same
/// row, tagged by `ask`. For those, `subject` is the typed request and
/// `options` is empty — what they offer is inside the request, not beside it.
#[derive(Debug)]
pub(crate) struct PendingPermission {
    pub(crate) thread_id: String,
    /// The backend's own id for the blocked request, handed back verbatim
    /// with the answer (#299).
    pub(crate) interaction: InteractionId,
    pub(crate) title: String,
    pub(crate) kind: Option<String>,
    pub(crate) subject: Value,
    pub(crate) options: Value,
    pub(crate) created_at: String,
    /// Which sort of ask: decides which pending list it appears in and which
    /// reply method may settle it.
    pub(crate) ask: AskKind,
    /// The ACP method it arrived on. Kept for the record; nothing dispatches
    /// on it.
    pub(crate) method: String,
}

/// Why a live ask is being taken off the wire without a human answering it.
///
/// The agent is answered `cancelled` in every case. What differs is the
/// *record*, and since #298 it differs by kind: a permission decision is worth
/// recording after the fact, so #20's rules for it are untouched; a question
/// or plan whose asker is gone is closed as such, because Cursor will not ask
/// again and an answer recorded later could never be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Withdrawal {
    /// The turn ended on purpose: the user cancelled, or the lifecycle closed
    /// the thread out. Nobody will ever answer, so the record resolves
    /// `cancelled` — every kind.
    Cancelled,
    /// The *host* is going away. A permission stays `pending`, because
    /// `state-machine.md` promises the next launch resurfaces this thread as
    /// `needs_you` with the ask still on it. A question or plan becomes
    /// `unavailable`.
    Abandoned,
    /// The adapter died holding the ask. A permission resolves `cancelled`,
    /// as it always has; a question or plan is `unavailable`.
    Lost,
    /// The turn ended with the ask still open: the agent stopped waiting. A
    /// permission resolves `cancelled`; a question or plan is `expired`.
    Expired,
}

impl HostSession {
    // ---- from the adapter ---------------------------------------------

    /// An agent asked to do something. Apply the policy, then either answer it
    /// or put it in front of the human.
    pub(crate) fn open_permission_request(
        &mut self,
        thread_id: &str,
        interaction: InteractionId,
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
            interaction,
            title: subject_title(&subject),
            kind: lifecycle::permission_kind(&subject),
            subject,
            options,
            created_at: super::store::now_utc(),
            ask: AskKind::Permission,
            method: REQUEST_PERMISSION.to_string(),
        };
        // Written before either branch acts, so the ledger holds every ask the
        // host ever took — including the ones it answered itself. What the
        // host decided while the user was away is worth recording for exactly
        // the same reason as what it asked them.
        self.record_permission_request(&request_id, &pending);

        // Wait for Inbox is a host-side permission policy on a folded thread
        // (#5, #15): reads are answered here, everything else reaches a human.
        // Permissions only — a question or plan never comes through here, so
        // nothing can auto-answer one (#298).
        match self.lifecycle_permission_policy(thread_id, &pending.subject, &pending.options) {
            PermissionDisposition::AutoAllow { option_id } => {
                let delivered =
                    self.answer_agent(thread_id, pending.interaction, selected(&option_id));
                self.resolve_permission_record(
                    &request_id,
                    ASK_ANSWERED,
                    HOST_DECIDED,
                    Some(&option_id),
                    delivered,
                    None,
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
            let ask = pending.ask;
            let delivered = self.answer_agent(thread_id, pending.interaction, cancelled_outcome());
            if !delivered {
                eprintln!(
                    "could not tell {thread_id}'s agent that {request_id} was withdrawn ({reason})"
                );
            }
            if ask != AskKind::Permission {
                let outcome = match how {
                    Withdrawal::Cancelled => InteractionOutcome::Cancelled,
                    Withdrawal::Abandoned | Withdrawal::Lost => InteractionOutcome::Unavailable,
                    Withdrawal::Expired => InteractionOutcome::Expired,
                };
                self.settle_interaction(
                    thread_id,
                    &request_id,
                    ask,
                    outcome,
                    None,
                    Some(reason.to_string()),
                    HOST_DECIDED,
                    delivered,
                );
                continue;
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
                None,
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
        // Who answered is the host's fact, not the caller's claim (#29). The
        // record and the `permission/resolved` broadcast are how every other
        // client learns that the phone took this one, and a client that could
        // write any id into that field could answer as the console it is
        // deliberately not.
        let device_id = self.answering_device(&params.device_id)?;
        let record = self.permission_record(&request_id);
        // A question or plan has its own reply method, and this one must not
        // be able to reach it: `permission/reply` carries an option id, and
        // there is no option on a plan that means "accepted" (#298).
        let is_permission = |ask: AskKind| ask == AskKind::Permission;
        let named_kind = self
            .pending_permissions
            .get(&request_id)
            .map(|pending| pending.ask)
            .or_else(|| record.as_ref().and_then(|row| AskKind::parse(&row.ask)));
        if named_kind.is_some_and(|ask| !is_permission(ask)) {
            return Err(RpcError::InvalidParams(format!(
                "{request_id} is not a permission request; answer it with interaction/reply"
            )));
        }
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
            Some(pending) => self.answer_agent(&thread_id, pending.interaction, outcome),
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
            &device_id,
            params.option_id.as_deref(),
            delivered,
            None,
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
            &device_id,
            params.option_id.clone(),
            params.cancelled,
        );
        // The card that brought the user here is answered, so it stops
        // asking. An ask on a folded thread resurfaces it and writes an
        // unread `needs_you` row; nothing on this path ever cleared that row,
        // so the moment the ask was answered the Inbox re-expanded the same
        // question as a stale card with no buttons, still counted in the
        // badge, until the turn happened to end.
        //
        // Cleared whether or not the answer was `delivered`. `delivered` is
        // about whether a process heard it, and the run state below rightly
        // turns on that. The card is about whether the *user* still has a
        // question waiting, and they do not — they just answered it. An
        // adapter that died before hearing is a different fact, and the
        // end-of-adapter paths own saying so.
        //
        // Narrow on purpose: only this thread's newest unread `needs_you`,
        // not the blanket `mark_inbox_read` that reopening a thread uses.
        // Anything else this thread surfaced is still owed.
        if let Some(store) = self.store.as_ref() {
            if let Err(err) = store.mark_inbox_kind_read(&thread_id, "needs_you") {
                eprintln!("could not clear the needs_you card for {thread_id}: {err}");
            }
        }
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

    /// The device this answer will be attributed to.
    ///
    /// A connection is bound to a device by `host/hello`, and since #29 that
    /// binding is per connection, so the host always knows which client is
    /// speaking. A caller that names itself correctly is fine; one that names
    /// somebody else is refused rather than quietly corrected, because the
    /// only reason to send a different id is to be recorded as a device you
    /// are not.
    pub(crate) fn answering_device(&self, claimed: &str) -> Result<String, RpcError> {
        let Some(connected) = self.connected_device_id.as_deref() else {
            // `require_hello` runs before this in the router; belt and braces
            // for anyone calling the handler directly.
            return Err(RpcError::HelloRequired);
        };
        let claimed = claimed.trim();
        if !claimed.is_empty() && claimed != connected {
            return Err(RpcError::InvalidParams(
                "deviceId must be the device this connection said hello as".to_string(),
            ));
        }
        Ok(connected.to_string())
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
        // Permissions only. A question or plan is listed by
        // `interaction/pending`, and never here — a client that draws this
        // list as "things to allow" must not be handed a plan to allow (#298).
        let mut requests: Vec<PendingPermissionView> = self
            .pending_permissions
            .iter()
            .filter(|(_, pending)| pending.ask == AskKind::Permission)
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
                if row.ask != AskKind::Permission.as_str()
                    || self.pending_permissions.contains_key(&row.id)
                {
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

    pub(crate) fn record_permission_request(&self, request_id: &str, pending: &PendingPermission) {
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
            ask: pending.ask.as_str().to_string(),
            method: Some(pending.method.clone()),
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

    pub(crate) fn permission_record(&self, request_id: &str) -> Option<PermissionRequestRow> {
        self.store
            .as_ref()?
            .get_permission_request(request_id)
            .unwrap_or_else(|err| {
                eprintln!("could not read permission request {request_id}: {err}");
                None
            })
    }

    /// `true` when this call is the one that resolved the row.
    pub(crate) fn resolve_permission_record(
        &self,
        request_id: &str,
        state: &str,
        decided_by: &str,
        option_id: Option<&str>,
        delivered: bool,
        answer_json: Option<&str>,
    ) -> bool {
        let Some(store) = self.store.as_ref() else {
            // Nothing durable to claim; the live entry was the claim.
            return true;
        };
        match store.resolve_permission_request(
            request_id,
            state,
            decided_by,
            option_id,
            delivered,
            answer_json,
        ) {
            Ok(claimed) => claimed,
            Err(err) => {
                eprintln!("could not resolve permission request {request_id}: {err}");
                true
            }
        }
    }

    /// Hand an outcome to the adapter. `false` when there is no adapter left —
    /// which is a fact about the world, not a failure of the call.
    pub(crate) fn answer_agent(
        &self,
        thread_id: &str,
        interaction: InteractionId,
        outcome: Value,
    ) -> bool {
        let Some(conn) = self.conn(thread_id) else {
            return false;
        };
        match conn.respond(interaction, outcome) {
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

/// The one answer every blocking request accepts as "nobody chose": ACP's
/// permission outcome and Cursor's extension outcomes spell it the same way.
pub(crate) fn cancelled_outcome() -> Value {
    json!({ "outcome": { "outcome": "cancelled" } })
}

#[cfg(test)]
mod tests {
    use super::super::protocol::error::{HELLO_REQUIRED, INVALID_PARAMS};
    use super::super::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use super::super::protocol::HOST_HELLO;
    use super::super::HostSession;

    fn hello(session: &mut HostSession) -> String {
        let response =
            session.handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None));
        response.result.expect("hello")["device"]["deviceId"]
            .as_str()
            .expect("deviceId")
            .to_string()
    }

    /// Who answered is what `permission/resolved` broadcasts and what the
    /// record keeps. A client that could put any id in that field could be
    /// recorded as a device it is not — which is the whole point of a phone
    /// being a *different* device from the Mac (#29).
    #[test]
    fn an_answer_is_attributed_to_the_connection_that_gave_it() {
        let mut session = HostSession::ephemeral();
        let device_id = hello(&mut session);

        assert_eq!(session.answering_device(&device_id).unwrap(), device_id);
        // Omitting it is fine: the host knows.
        assert_eq!(session.answering_device("").unwrap(), device_id);
        // Claiming to be somebody else is not.
        let err = session
            .answering_device("some-other-device")
            .expect_err("a foreign device id must be refused");
        assert_eq!(err.code(), INVALID_PARAMS);
    }

    #[test]
    fn nobody_answers_before_saying_hello() {
        let session = HostSession::ephemeral();
        let err = session.answering_device("").expect_err("no hello yet");
        assert_eq!(err.code(), HELLO_REQUIRED);
    }
}
