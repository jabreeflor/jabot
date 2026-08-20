//! Thread lifecycle: the overlay state machine, the run ledger, and the
//! supervisor that drives both from ACP events (#15).
//!
//! Three rules shape everything here.
//!
//! **An illegal transition is an error.** Fold, reopen, archive and delete go
//! through [`state::next_state`], and a move that is not in the research
//! transition table comes back as [`RpcError::IllegalTransition`]. A no-op
//! would leave the sidebar and the store disagreeing about whether the user's
//! work disappeared.
//!
//! **Persist, then notify.** Every resurface writes the overlay state and the
//! `inbox_events` row *before* the `inbox/resurface` notification is queued. A
//! notification that never reaches a client loses nothing; the card is already
//! on disk. The reverse order can lose a finished job to a dropped socket.
//!
//! **The process layer stays orthogonal.** [`process::AcpState`] is a separate
//! axis from [`state::ThreadState`], held in RAM and reconciled on boot. Folded
//! × running — disappeared and still working — is the product's whole premise,
//! and it is only representable because the two are not one enum.

pub mod ledger;
pub mod process;
pub mod receipt;
pub mod resurface;
pub mod state;

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use super::protocol::error::RpcError;
use super::protocol::methods::{
    FoldPolicy, InboxEventView, InboxListParams, InboxListResult, ProcessView, ReceiptView,
    ResurfaceReason, RunView, SleepingThreadView, ThreadFoldParams, ThreadOpenParams,
    ThreadRefParams, ThreadStateResult,
};
use super::store::{InboxEventRow, NewThread, RunRow, SessionReceiptRow, Store, ThreadRow};
use super::HostSession;
use ledger::RunState;
use process::{AcpState, ProcessStatus};
use receipt::SessionFingerprint;
use state::{ThreadAction, ThreadState};

/// Silence that counts as stuck. A backstop, never the completion signal —
/// `resurface.md` is explicit that "stdout went quiet" must not be primary.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// How many runs a `thread/state` reply carries. The Inbox card wants the
/// latest; the header wants recent history; nobody wants the whole ledger.
const RUN_PAGE: usize = 50;

/// Supervisor RAM: what each thread's adapter is doing right now.
///
/// Deliberately not persisted (#5, "still working is supervisor RAM,
/// reconciled on boot"). What *is* persisted is the overlay state, the ledger,
/// and the session receipt — everything needed to rebuild this after a quit.
#[derive(Debug)]
pub struct LifecycleState {
    threads: HashMap<String, ProcessStatus>,
    idle_timeout: Duration,
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            threads: HashMap::new(),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
}

impl LifecycleState {
    /// `JABOT_IDLE_TIMEOUT_MS` shortens the stuck backstop. It exists so tests
    /// can watch a real timeout fire in a second instead of ten minutes; the
    /// setting this stands in for is a user preference (#26).
    pub fn from_env() -> Self {
        let idle_timeout = std::env::var("JABOT_IDLE_TIMEOUT_MS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_IDLE_TIMEOUT);
        Self {
            idle_timeout,
            ..Self::default()
        }
    }

    fn entry(&mut self, thread_id: &str) -> &mut ProcessStatus {
        self.threads.entry(thread_id.to_string()).or_default()
    }

    fn get(&self, thread_id: &str) -> Option<&ProcessStatus> {
        self.threads.get(thread_id)
    }
}

/// What the host does with an incoming `session/request_permission`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDisposition {
    /// Surface it: notify the client and, if the thread is folded, resurface.
    Ask,
    /// Wait for Inbox auto-allowed a read. The answer goes straight back to the
    /// agent and lands in the away log; the thread does not come back.
    AutoAllow { option_id: String },
}

impl HostSession {
    // ---- client methods -----------------------------------------------

    /// New Chat. Idempotent: opening a thread that already exists returns it,
    /// so a retried request cannot start a second conversation.
    pub fn thread_open(&mut self, params: ThreadOpenParams) -> Result<ThreadStateResult, RpcError> {
        let thread_id = params
            .thread_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if self.lifecycle_thread(&thread_id)?.is_none() {
            let runtime_json = self.runtime_json_for(&params)?;
            let new = NewThread {
                id: thread_id.clone(),
                folder_id: params.folder_id.clone(),
                bot_id: params.bot_id.clone(),
                harness_id: params.harness_id.clone(),
                cwd: params.cwd.clone(),
                runtime_json,
                title: params.title.clone(),
                fold_policy: params.fold_policy.unwrap_or_default().as_str().to_string(),
            };
            self.store_or_err()?
                .insert_thread(&new)
                .map_err(store_error)?;
        }
        self.thread_state(ThreadRefParams {
            thread_id: thread_id.clone(),
        })
    }

    /// Fold: hide the thread, keep the subprocess. Never `session/close`.
    pub fn thread_fold(&mut self, params: ThreadFoldParams) -> Result<ThreadStateResult, RpcError> {
        let thread_id = params.thread_id.clone();
        // The policy lands before the fold so that a permission arriving in the
        // same breath is judged by the policy the user just chose.
        if let Some(policy) = params.policy {
            self.store_or_err()?
                .set_thread_fold_policy(&thread_id, policy.as_str())
                .map_err(store_error)?;
        }
        let action = match params.policy {
            Some(FoldPolicy::WaitForInbox) => ThreadAction::WaitForInbox,
            _ => ThreadAction::Fold,
        };
        self.apply_action(&thread_id, action)?;
        // Folding work that already finished should not park it in Still
        // Sleeping waiting for an event that will never come; it resurfaces
        // immediately with whatever the last run said (state-machine.md).
        self.settle_after_fold(&thread_id);
        self.thread_state(ThreadRefParams { thread_id })
    }

    /// Open a sleeping or resurfaced row. Clears the thread's Inbox badge.
    pub fn thread_reopen(
        &mut self,
        params: ThreadRefParams,
    ) -> Result<ThreadStateResult, RpcError> {
        self.apply_action(&params.thread_id, ThreadAction::Reopen)?;
        self.store_or_err()?
            .mark_inbox_read(&params.thread_id)
            .map_err(store_error)?;
        self.thread_state(params)
    }

    /// Closed on purpose. The transcript overlay stays; the process does not.
    pub fn thread_archive(
        &mut self,
        params: ThreadRefParams,
    ) -> Result<ThreadStateResult, RpcError> {
        self.apply_action(&params.thread_id, ThreadAction::Archive)?;
        self.close_out(&params.thread_id, "archived");
        self.thread_state(params)
    }

    /// Tombstone. The row survives so a late adapter event has something to
    /// land on, and every read filters it out.
    pub fn thread_delete(
        &mut self,
        params: ThreadRefParams,
    ) -> Result<ThreadStateResult, RpcError> {
        self.apply_action(&params.thread_id, ThreadAction::Delete)?;
        self.close_out(&params.thread_id, "deleted");
        self.thread_state(params)
    }

    /// The overlay, the process axis, the ledger, and the session receipt —
    /// everything #22 needs to draw a row without a second round trip.
    pub fn thread_state(&mut self, params: ThreadRefParams) -> Result<ThreadStateResult, RpcError> {
        let row = self
            .lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        let store = self.store_or_err()?;
        let runs = store
            .list_runs(&row.id)
            .map_err(store_error)?
            .into_iter()
            .take(RUN_PAGE)
            .map(run_view)
            .collect::<Vec<_>>();
        let receipt = store
            .get_session_receipt(&row.id)
            .map_err(store_error)?
            .map(receipt_view);
        let unread = store
            .count_unread_inbox(Some(&row.id))
            .map_err(store_error)?;
        let process = self.process_view(&row.id);
        Ok(ThreadStateResult {
            thread_id: row.id.clone(),
            title: row.title.clone(),
            state: effective_state(&row).as_str().to_string(),
            fold_policy: FoldPolicy::parse(&row.fold_policy),
            resurfaced_reason: row
                .resurfaced_reason
                .as_deref()
                .and_then(ResurfaceReason::parse),
            cwd: row.cwd.clone(),
            harness_id: row.harness_id.clone(),
            folder_id: row.folder_id.clone(),
            bot_id: row.bot_id.clone(),
            acp_session_id: row.acp_session_id.clone(),
            last_stop_reason: row.last_stop_reason.clone(),
            last_error: row.last_error.clone(),
            folded_at: row.folded_at.clone(),
            resurfaced_at: row.resurfaced_at.clone(),
            archived_at: row.archived_at.clone(),
            deleted_at: row.deleted_at.clone(),
            process,
            latest_run: runs.first().cloned(),
            runs,
            receipt,
            unread,
        })
    }

    /// The Inbox: resurfaced cards from `inbox_events`, plus Still Sleeping
    /// projected straight off `threads.state = folded` (#5).
    pub fn inbox_list(&mut self, params: InboxListParams) -> Result<InboxListResult, RpcError> {
        let limit = params.limit.unwrap_or(100).clamp(1, 500);
        let include_dismissed = params.include_dismissed.unwrap_or(false);
        let store = self.store_or_err()?;
        let rows = store
            .list_inbox_events(limit, include_dismissed)
            .map_err(store_error)?;
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let thread = store.get_thread(&row.thread_id).map_err(store_error)?;
            events.push(inbox_event_view(row, thread.as_ref()));
        }
        let folded = store
            .list_threads_by_state(ThreadState::Folded.as_str())
            .map_err(store_error)?;
        let mut sleeping = Vec::with_capacity(folded.len());
        for row in folded {
            let run_state = store
                .latest_run(&row.id)
                .map_err(store_error)?
                .map(|run| run.state);
            sleeping.push(SleepingThreadView {
                thread_id: row.id.clone(),
                title: row.title.clone(),
                fold_policy: FoldPolicy::parse(&row.fold_policy),
                folded_at: row.folded_at.clone(),
                run_state,
                acp_state: self.acp_state(&row.id).as_str().to_string(),
            });
        }
        let unread = self
            .store_or_err()?
            .count_unread_inbox(None)
            .map_err(store_error)?;
        Ok(InboxListResult {
            events,
            sleeping,
            unread,
        })
    }

    // ---- supervisor hooks, driven by the ACP layer (#10) ---------------

    /// A prompt was accepted: open a run and stamp the session receipt.
    pub(crate) fn lifecycle_run_started(&mut self, thread_id: &str, acp_session_id: &str) {
        {
            let entry = self.lifecycle.entry(thread_id);
            entry.connected = true;
            entry.acp = AcpState::Running;
            entry.touch();
        }
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Ok(Some(thread)) = store.get_thread(thread_id) else {
            return;
        };
        // A previous run still open when a new prompt arrives will never report
        // its own outcome — `lost` is exactly that: we stopped being able to
        // find out.
        if let Ok(Some(previous)) = store.latest_run(thread_id) {
            if RunState::parse(&previous.state).map(RunState::is_open) == Ok(true) {
                let _ = store.set_run_state(
                    &previous.id,
                    RunState::Lost.as_str(),
                    Some("superseded by a new run"),
                );
            }
        }
        let run = match store.insert_run(thread_id, "prompt", None) {
            Ok(run) => run,
            Err(err) => {
                eprintln!("failed to open a run for {thread_id}: {err}");
                return;
            }
        };
        let _ = store.set_run_acp_session(&run.id, acp_session_id);
        let _ = store.set_run_state(&run.id, RunState::Running.as_str(), None);
        let fingerprint = self.fingerprint_for(&thread);
        if let Err(err) = self.store_or_err().and_then(|store| {
            store
                .upsert_session_receipt(
                    thread_id,
                    acp_session_id,
                    thread.native_session_ref.as_deref(),
                    &fingerprint.harness_id,
                    fingerprint.model.as_deref(),
                    &fingerprint.cwd,
                    &fingerprint.tools_json(),
                    &fingerprint.permission_mode,
                    &fingerprint.digest(),
                )
                .map_err(store_error)
        }) {
            eprintln!("failed to write a session receipt for {thread_id}: {err}");
        }
        self.lifecycle.entry(thread_id).run_id = Some(run.id);
    }

    /// Any `session/update`. Also the completion signal, when the update is a
    /// v2 `state_update` going idle or the v1 prompt result the ACP layer
    /// normalises into one.
    pub(crate) fn lifecycle_on_update(&mut self, thread_id: &str, acp: &Value) {
        self.lifecycle.entry(thread_id).touch();
        if acp.get("sessionUpdate").and_then(Value::as_str) != Some("state_update") {
            // Output means work. Do not overwrite `requires_action` with it:
            // an agent can narrate while it waits for an answer.
            if self.pending_permission_count(thread_id) == 0 {
                self.lifecycle.entry(thread_id).acp = AcpState::Running;
            }
            return;
        }
        let reported = acp
            .get("sessionState")
            .or_else(|| acp.get("state"))
            .and_then(Value::as_str)
            .map(AcpState::parse)
            .unwrap_or(AcpState::Unknown);
        self.lifecycle.entry(thread_id).acp = reported;
        if reported == AcpState::Idle {
            let stop = acp
                .get("stopReason")
                .and_then(Value::as_str)
                .map(str::to_string);
            self.lifecycle_on_turn_end(thread_id, stop.as_deref());
        }
    }

    /// Idle plus a stop reason: close the run, and resurface if this thread was
    /// folded. An `active` thread just shows "session finished" in chat.
    pub(crate) fn lifecycle_on_turn_end(&mut self, thread_id: &str, stop_reason: Option<&str>) {
        let outcome = resurface::classify_stop(stop_reason);
        self.lifecycle.entry(thread_id).acp = AcpState::Idle;
        let target = match outcome {
            resurface::StopOutcome::Done => RunState::Succeeded,
            resurface::StopOutcome::Failed => RunState::Failed,
            resurface::StopOutcome::Cancelled => RunState::Cancelled,
        };
        let error = (target == RunState::Failed).then(|| {
            stop_reason
                .map(|r| format!("stopped: {r}"))
                .unwrap_or_else(|| "adapter returned no stop reason".into())
        });
        let run_id = self.close_run(thread_id, target, error.as_deref());
        if let Some(store) = self.store.as_ref() {
            let _ = store.set_thread_stop(thread_id, stop_reason, error.as_deref());
        }
        self.lifecycle.entry(thread_id).run_id = None;
        let Some(reason) = outcome.resurface_reason() else {
            // A cancel the user asked for gets a quiet row, not a card.
            return;
        };
        let summary = match reason {
            ResurfaceReason::Done => "finished".to_string(),
            _ => error.unwrap_or_else(|| "the run did not finish".into()),
        };
        self.try_resurface(thread_id, reason, &summary, run_id.as_deref());
    }

    /// Wait for Inbox, applied by the host rather than the harness: auto-allow
    /// reads on a folded thread, ask for everything else.
    ///
    /// Locked policy — never auto-allow execute or delete because a thread is
    /// folded, and never auto-pick an answer to a question. An unanswered
    /// execute is a judgment call for the human, not a decision for us.
    pub(crate) fn lifecycle_permission_policy(
        &self,
        thread_id: &str,
        subject: &Value,
        options: &Value,
    ) -> PermissionDisposition {
        let Some(store) = self.store.as_ref() else {
            return PermissionDisposition::Ask;
        };
        let Ok(Some(thread)) = store.get_thread(thread_id) else {
            return PermissionDisposition::Ask;
        };
        if thread.state != ThreadState::Folded.as_str()
            || FoldPolicy::parse(&thread.fold_policy) != FoldPolicy::WaitForInbox
        {
            return PermissionDisposition::Ask;
        }
        if permission_kind(subject).as_deref() != Some("read") {
            return PermissionDisposition::Ask;
        }
        match allow_once_option(options) {
            Some(option_id) => PermissionDisposition::AutoAllow { option_id },
            // No allow-once on offer means we cannot answer without guessing
            // which of the agent's options is the harmless one.
            None => PermissionDisposition::Ask,
        }
    }

    /// Away-log entry for a permission the host answered while the user was
    /// gone. Recorded, but read on arrival: it is a receipt, not an ask.
    pub(crate) fn lifecycle_record_auto_allow(
        &mut self,
        thread_id: &str,
        subject: &Value,
        option_id: &str,
    ) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let title = subject
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("a read")
            .to_string();
        let payload = json!({
            "reviewable": false,
            "auto": true,
            "optionId": option_id,
            "subject": subject,
        });
        let event = store.insert_inbox_event(
            thread_id,
            self.lifecycle
                .get(thread_id)
                .and_then(|s| s.run_id.as_deref()),
            "judgment_call",
            &format!("Allowed {title}"),
            "auto-allowed by Wait for Inbox",
            Some(&payload.to_string()),
        );
        match event {
            Ok(event) => {
                let _ = store.mark_inbox_event_read(&event.id);
            }
            Err(err) => eprintln!("failed to record an away-log entry for {thread_id}: {err}"),
        }
    }

    /// An outstanding `request_permission` means Needs you — the run pauses and
    /// a folded thread comes back with the ask.
    pub(crate) fn lifecycle_on_permission_pending(&mut self, thread_id: &str, subject: &Value) {
        {
            let entry = self.lifecycle.entry(thread_id);
            entry.acp = AcpState::RequiresAction;
            entry.touch();
        }
        let run_id = self.pause_run(thread_id);
        let summary = subject
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("waiting on your answer")
            .to_string();
        self.try_resurface(
            thread_id,
            ResurfaceReason::NeedsYou,
            &summary,
            run_id.as_deref(),
        );
    }

    /// The human answered. A selected option puts the same run back to work.
    pub(crate) fn lifecycle_on_permission_answered(&mut self, thread_id: &str, cancelled: bool) {
        {
            let entry = self.lifecycle.entry(thread_id);
            entry.touch();
            entry.acp = if cancelled {
                AcpState::Idle
            } else {
                AcpState::Running
            };
        }
        if cancelled {
            return;
        }
        let (Some(store), Some(run_id)) = (
            self.store.as_ref(),
            self.lifecycle
                .get(thread_id)
                .and_then(|s| s.run_id.clone())
                .or_else(|| self.latest_open_run_id(thread_id)),
        ) else {
            return;
        };
        if let Ok(Some(run)) = store.get_run(&run_id) {
            if RunState::parse(&run.state) == Ok(RunState::NeedsYou) {
                let _ = store.set_run_state(&run_id, RunState::Running.as_str(), None);
            }
        }
        self.lifecycle.entry(thread_id).run_id = Some(run_id);
    }

    /// The adapter died. A crash while running is a failure; a crash while
    /// blocked on a permission loses the answer we were waiting to give.
    pub(crate) fn lifecycle_on_adapter_closed(&mut self, thread_id: &str, error: Option<&str>) {
        let was = {
            let entry = self.lifecycle.entry(thread_id);
            entry.connected = false;
            let was = entry.acp;
            entry.acp = AcpState::Unknown;
            was
        };
        let open = self.open_run(thread_id);
        let Some((run_id, run_state)) = open else {
            return;
        };
        let target = match run_state {
            RunState::Running => RunState::Failed,
            _ => RunState::Lost,
        };
        let detail = error.unwrap_or("adapter exited");
        if let Some(store) = self.store.as_ref() {
            if ledger::advance(run_state, target).is_ok() {
                let _ = store.set_run_state(&run_id, target.as_str(), Some(detail));
            }
            let _ = store.set_thread_stop(thread_id, Some("adapter_exit"), Some(detail));
        }
        self.lifecycle.entry(thread_id).run_id = None;
        // A thread already resurfaced as Needs you keeps that card: the ask is
        // still the thing the human has to deal with.
        if was != AcpState::RequiresAction {
            self.try_resurface(thread_id, ResurfaceReason::Failed, detail, Some(&run_id));
        }
    }

    /// Idle-timeout backstop. Run every pump cycle; costs a clock read per
    /// live thread.
    pub(crate) fn lifecycle_tick(&mut self) {
        let timeout = self.lifecycle.idle_timeout;
        let stalled: Vec<String> = self
            .lifecycle
            .threads
            .iter()
            .filter(|(_, status)| {
                status.connected
                    && status.acp == AcpState::Running
                    && !status.stuck_reported
                    && status.last_activity.elapsed() >= timeout
            })
            .map(|(id, _)| id.clone())
            .collect();
        for thread_id in stalled {
            // Blocked on a permission is Needs you, not stuck — that thread is
            // waiting on us, not on itself.
            if self.pending_permission_count(&thread_id) > 0 {
                continue;
            }
            self.lifecycle.entry(&thread_id).stuck_reported = true;
            // The run stays `running` and the process stays alive on purpose:
            // stuck means "no output for a while", not "give up". `timed_out`
            // is reserved for a hard cap that actually ends a run.
            let summary = format!("no output for {}s", timeout.as_secs());
            let run_id = self
                .lifecycle
                .get(&thread_id)
                .and_then(|s| s.run_id.clone());
            self.try_resurface(
                &thread_id,
                ResurfaceReason::Stuck,
                &summary,
                run_id.as_deref(),
            );
        }
    }

    /// The stuck backstop's threshold. `resurface.md` starts it at ten minutes
    /// and says to make it a setting; this is that setting's entry point, and
    /// what a test uses to watch a real timeout fire in milliseconds.
    pub fn set_idle_timeout(&mut self, timeout: Duration) {
        self.lifecycle.idle_timeout = timeout;
    }

    pub(crate) fn acp_state(&self, thread_id: &str) -> AcpState {
        self.lifecycle
            .get(thread_id)
            .map(|status| status.acp)
            .unwrap_or(AcpState::Unknown)
    }

    // ---- internals -----------------------------------------------------

    /// Persist the transition, or say why it cannot happen.
    fn apply_action(
        &mut self,
        thread_id: &str,
        action: ThreadAction,
    ) -> Result<ThreadRow, RpcError> {
        let row = self
            .lifecycle_thread(thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(thread_id.to_string()))?;
        let from = effective_state(&row);
        let current_reason = row
            .resurfaced_reason
            .as_deref()
            .and_then(ResurfaceReason::parse);
        let to = state::next_state(from, action, current_reason).map_err(|_| {
            RpcError::IllegalTransition {
                thread_id: thread_id.to_string(),
                from: from.as_str().to_string(),
                action: action.as_str().to_string(),
            }
        })?;
        let store = self.store_or_err()?;
        let updated = if to == ThreadState::Deleted {
            store.tombstone_thread(thread_id)
        } else {
            let reason = match action {
                ThreadAction::Resurface(reason) => Some(reason.as_str()),
                _ => None,
            };
            store.transition_thread(thread_id, from.as_str(), to.as_str(), reason)
        }
        .map_err(store_error)?;
        Ok(updated)
    }

    fn try_resurface(
        &mut self,
        thread_id: &str,
        reason: ResurfaceReason,
        summary: &str,
        run_id: Option<&str>,
    ) {
        match self.resurface_and_notify(thread_id, reason, summary, run_id) {
            Ok(_) => {}
            Err(err) => eprintln!("failed to resurface {thread_id}: {err}"),
        }
    }

    /// Persist, then notify — in that order and never the other one.
    ///
    /// The overlay state and the Inbox card land together in one transaction;
    /// `inbox/resurface` is queued only after that returns. A notification that
    /// never reaches a client therefore loses nothing, and a store failure
    /// tells nobody a thread came back when it did not.
    ///
    /// Returns `false` when the thread is not somewhere a resurface applies —
    /// an `active` thread finishing a turn is the common case, not an error.
    fn resurface_and_notify(
        &mut self,
        thread_id: &str,
        reason: ResurfaceReason,
        summary: &str,
        run_id: Option<&str>,
    ) -> Result<bool, RpcError> {
        // A host with no store has no overlay to resurface into — the ACP
        // tests run that way. Not applicable, not a failure to report.
        if self.store.is_none() {
            return Ok(false);
        }
        let Some(row) = self.lifecycle_thread(thread_id)? else {
            return Ok(false);
        };
        let from = effective_state(&row);
        let current_reason = row
            .resurfaced_reason
            .as_deref()
            .and_then(ResurfaceReason::parse);
        if state::next_state(from, ThreadAction::Resurface(reason), current_reason).is_err() {
            return Ok(false);
        }
        let title = resurface::card_title(&row.title, reason);
        let payload = json!({ "reason": reason.as_str() });
        self.store_or_err()?
            .resurface_thread(
                thread_id,
                from.as_str(),
                reason.as_str(),
                resurface::inbox_kind(reason),
                &title,
                summary,
                Some(&payload.to_string()),
                run_id,
            )
            .map_err(store_error)?;
        self.notify_inbox_resurface(thread_id, reason);
        Ok(true)
    }

    /// Folding something that already stopped: resurface it now rather than
    /// parking finished work in Still Sleeping.
    fn settle_after_fold(&mut self, thread_id: &str) {
        if self.pending_permission_count(thread_id) > 0 {
            self.try_resurface(
                thread_id,
                ResurfaceReason::NeedsYou,
                "waiting on your answer",
                self.lifecycle
                    .get(thread_id)
                    .and_then(|s| s.run_id.clone())
                    .as_deref(),
            );
            return;
        }
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Ok(Some(run)) = store.latest_run(thread_id) else {
            return;
        };
        let Ok(run_state) = RunState::parse(&run.state) else {
            return;
        };
        let reason = match run_state {
            RunState::Succeeded => ResurfaceReason::Done,
            RunState::Failed | RunState::TimedOut | RunState::Lost => ResurfaceReason::Failed,
            // Queued, running and needs_you are still in flight; cancelled is
            // the quiet stop that deliberately does not celebrate.
            _ => return,
        };
        let summary = run
            .error
            .clone()
            .unwrap_or_else(|| "finished before you folded it".into());
        self.try_resurface(thread_id, reason, &summary, Some(&run.id));
    }

    /// Archive and delete both stop the work: answer outstanding permissions
    /// as cancelled, close the run, and drop the adapter.
    ///
    /// `session/close` is not sent because the ACP layer does not speak it yet
    /// (#21 owns resume and close); killing the process group is what we have,
    /// and it is what quit already does.
    fn close_out(&mut self, thread_id: &str, why: &str) {
        let _ = self.cancel_pending_permissions(thread_id, why);
        if let Some((run_id, run_state)) = self.open_run(thread_id) {
            if let Some(store) = self.store.as_ref() {
                if ledger::advance(run_state, RunState::Cancelled).is_ok() {
                    let _ = store.set_run_state(&run_id, RunState::Cancelled.as_str(), Some(why));
                }
            }
        }
        self.drop_adapter(thread_id);
        self.lifecycle.threads.remove(thread_id);
    }

    fn close_run(
        &mut self,
        thread_id: &str,
        target: RunState,
        error: Option<&str>,
    ) -> Option<String> {
        let (run_id, run_state) = self.open_run(thread_id)?;
        let store = self.store.as_ref()?;
        // An agent that ends a turn while we still had it marked as blocked
        // resumed without us; walk the ledger through `running` rather than
        // inventing an edge out of `needs_you`.
        let from = if run_state == RunState::NeedsYou && target != RunState::Cancelled {
            match store.set_run_state(&run_id, RunState::Running.as_str(), None) {
                Ok(_) => RunState::Running,
                Err(_) => run_state,
            }
        } else {
            run_state
        };
        match ledger::advance(from, target) {
            Ok(_) => {
                let _ = store.set_run_state(&run_id, target.as_str(), error);
            }
            Err(err) => eprintln!("refusing an illegal run transition on {thread_id}: {err}"),
        }
        Some(run_id)
    }

    fn pause_run(&mut self, thread_id: &str) -> Option<String> {
        let (run_id, run_state) = self.open_run(thread_id)?;
        let store = self.store.as_ref()?;
        if ledger::advance(run_state, RunState::NeedsYou).is_ok() {
            let _ = store.set_run_state(&run_id, RunState::NeedsYou.as_str(), None);
        }
        Some(run_id)
    }

    /// The thread's live run, if it has one, as (id, state).
    fn open_run(&self, thread_id: &str) -> Option<(String, RunState)> {
        let store = self.store.as_ref()?;
        let run = store.latest_run(thread_id).ok()??;
        let state = RunState::parse(&run.state).ok()?;
        state.is_open().then_some((run.id, state))
    }

    fn latest_open_run_id(&self, thread_id: &str) -> Option<String> {
        self.open_run(thread_id).map(|(id, _)| id)
    }

    fn process_view(&self, thread_id: &str) -> ProcessView {
        let status = self.lifecycle.get(thread_id);
        ProcessView {
            connected: status.map(|s| s.connected).unwrap_or(false),
            acp_state: status
                .map(|s| s.acp)
                .unwrap_or(AcpState::Unknown)
                .as_str()
                .to_string(),
            pending_permissions: self.pending_permission_count(thread_id),
        }
    }

    fn lifecycle_thread(&self, thread_id: &str) -> Result<Option<ThreadRow>, RpcError> {
        self.store_or_err()?
            .get_thread(thread_id)
            .map_err(store_error)
    }

    fn store_or_err(&self) -> Result<&Store, RpcError> {
        self.store.as_ref().ok_or(RpcError::StoreUnavailable)
    }

    /// What this thread would be spawned with today — the other half of the
    /// drift check against the stored receipt.
    fn fingerprint_for(&self, thread: &ThreadRow) -> SessionFingerprint {
        let model = serde_json::from_str::<Value>(&thread.runtime_json)
            .ok()
            .and_then(|runtime| {
                runtime
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        let tools = thread
            .bot_id
            .as_deref()
            .and_then(|bot_id| self.store.as_ref()?.get_bot(bot_id).ok().flatten())
            .and_then(|bot| serde_json::from_str::<Vec<String>>(&bot.tools_json).ok())
            .unwrap_or_default();
        SessionFingerprint::new(
            thread.harness_id.clone(),
            model,
            thread.cwd.clone(),
            tools,
            // Today JaBot's permission mode *is* the fold policy; #20 adds the
            // rest of the modes and this starts carrying more than two values.
            thread.fold_policy.clone(),
        )
    }

    fn runtime_json_for(&self, params: &ThreadOpenParams) -> Result<String, RpcError> {
        if let Some(spec) = &params.runtime {
            let mut runtime = json!({ "command": spec.command });
            if let Some(args) = &spec.args {
                runtime["args"] = json!(args);
            }
            if let Some(env) = &spec.env {
                runtime["env"] = json!(env);
            }
            if let Some(hint) = &spec.install_hint {
                runtime["installHint"] = json!(hint);
            }
            return Ok(runtime.to_string());
        }
        // The catalog knows which of a card's candidate commands this machine
        // actually has — the difference between `claude-agent-acp` and the
        // older `claude-code-acp` — so a thread snapshots what would really
        // spawn rather than the first name in the table (#13).
        if let Some(spec) = self.catalog_runtime_spec(&params.harness_id) {
            let mut runtime = json!({ "command": spec.command });
            if let Some(args) = &spec.args {
                runtime["args"] = json!(args);
            }
            if let Some(env) = &spec.env {
                runtime["env"] = json!(env);
            }
            if let Some(hint) = &spec.install_hint {
                runtime["installHint"] = json!(hint);
            }
            return Ok(runtime.to_string());
        }
        let row = self
            .store_or_err()?
            .get_harness(&params.harness_id)
            .map_err(store_error)?
            .ok_or_else(|| {
                RpcError::InvalidParams(format!(
                    "unknown harness {} and no runtime override",
                    params.harness_id
                ))
            })?;
        Ok(json!({
            "command": row.command,
            "args": serde_json::from_str::<Value>(&row.args_json).unwrap_or_else(|_| json!([])),
            "env": serde_json::from_str::<Value>(&row.env_json).unwrap_or_else(|_| json!({})),
            "installHint": row.install_hint,
        })
        .to_string())
    }
}

/// `deleted` lives in `deleted_at`, not in `threads.state`, so the tombstone
/// survives whatever state the thread was in when it was deleted.
fn effective_state(row: &ThreadRow) -> ThreadState {
    if row.deleted_at.is_some() {
        return ThreadState::Deleted;
    }
    ThreadState::parse(&row.state).unwrap_or(ThreadState::Active)
}

fn permission_kind(subject: &Value) -> Option<String> {
    subject
        .get("kind")
        .or_else(|| subject.get("toolCall").and_then(|tc| tc.get("kind")))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn allow_once_option(options: &Value) -> Option<String> {
    let options = options.as_array()?;
    options
        .iter()
        .find(|option| {
            option.get("kind").and_then(Value::as_str) == Some("allow_once")
                || option.get("optionId").and_then(Value::as_str) == Some("allow_once")
        })
        .and_then(|option| option.get("optionId").and_then(Value::as_str))
        .map(str::to_string)
}

fn run_view(row: RunRow) -> RunView {
    RunView {
        id: row.id,
        seq: row.seq,
        kind: row.kind,
        state: row.state,
        error: row.error,
        acp_session_id: row.acp_session_id,
        started_at: row.started_at,
        ended_at: row.ended_at,
        created_at: row.created_at,
    }
}

fn receipt_view(row: SessionReceiptRow) -> ReceiptView {
    ReceiptView {
        acp_session_id: row.acp_session_id,
        native_session_ref: row.native_session_ref,
        harness_id: row.harness_id,
        model: row.model,
        cwd: row.cwd,
        tools: serde_json::from_str(&row.tools_json).unwrap_or_default(),
        permission_mode: row.permission_mode,
        fingerprint: row.fingerprint,
        updated_at: row.updated_at,
    }
}

fn inbox_event_view(row: InboxEventRow, thread: Option<&ThreadRow>) -> InboxEventView {
    InboxEventView {
        id: row.id,
        thread_id: row.thread_id,
        thread_title: thread.map(|t| t.title.clone()).unwrap_or_default(),
        thread_state: thread
            .map(|t| effective_state(t).as_str().to_string())
            .unwrap_or_else(|| ThreadState::Deleted.as_str().to_string()),
        kind: row.kind,
        title: row.title,
        summary: row.summary,
        run_id: row.run_id,
        payload: row
            .payload_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        created_at: row.created_at,
        read_at: row.read_at,
        dismissed_at: row.dismissed_at,
    }
}

fn store_error(err: super::store::StoreError) -> RpcError {
    match err {
        super::store::StoreError::NotFound(id) => RpcError::ThreadNotFound(id),
        other => RpcError::Internal(other.to_string()),
    }
}
