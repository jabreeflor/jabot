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

use super::git;
use super::permission::Withdrawal as PermissionWithdrawal;
use super::protocol::error::RpcError;
use super::protocol::methods::{
    FoldPolicy, InboxEventView, InboxListParams, InboxListResult, ProcessView, ReceiptView,
    ResurfaceReason, RunView, SleepingThreadView, ThreadFoldParams, ThreadOpenParams,
    ThreadRefParams, ThreadStateResult,
};
use super::schedule::RUN_KIND_SCHEDULE;
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
            // A code thread gets its own checkout before it gets a row (#23):
            // `cwd` *is* the worktree, and the repo stamp is read out of it, so
            // the tree has to exist by the time the INSERT runs.
            let worktree = self.provision_worktree(&thread_id, &params)?;
            let cwd = worktree
                .as_ref()
                .map(|tree| tree.path.to_string_lossy().into_owned())
                .unwrap_or_else(|| params.cwd.clone());
            let new = NewThread {
                id: thread_id.clone(),
                folder_id: params.folder_id.clone(),
                bot_id: params.bot_id.clone(),
                harness_id: params.harness_id.clone(),
                cwd: cwd.clone(),
                runtime_json,
                title: params.title.clone(),
                fold_policy: params.fold_policy.unwrap_or_default().as_str().to_string(),
                worktree_path: worktree
                    .as_ref()
                    .map(|tree| tree.path.to_string_lossy().into_owned()),
                // Where this thread works, resolved now and written with the
                // row (#16, setup-porting §19). Opening is the only moment the
                // answer is knowable without guessing. The branch comes from
                // the cwd, which is why this reads the worktree and not the
                // folder: they are different trees on different branches.
                repo: self.thread_repo_record(params.folder_id.as_deref(), &cwd),
            };
            if let Err(err) = self.store_or_err()?.insert_thread(&new) {
                // A tree whose thread does not exist is a tree nothing will
                // ever clean up, so the failed spawn takes it back out.
                if let Some(tree) = worktree.as_ref() {
                    self.discard_worktree(tree, &thread_id);
                }
                return Err(store_error(err));
            }
        }
        self.thread_state(ThreadRefParams {
            thread_id: thread_id.clone(),
        })
    }

    /// Fold: hide the thread, keep the subprocess. Never `session/close`.
    pub fn thread_fold(&mut self, params: ThreadFoldParams) -> Result<ThreadStateResult, RpcError> {
        let thread_id = params.thread_id.clone();
        let action = match params.policy {
            Some(FoldPolicy::WaitForInbox) => ThreadAction::WaitForInbox,
            _ => ThreadAction::Fold,
        };
        // Nothing is written until the fold is known to be legal. A refused
        // transition has to leave the row exactly as it was, and the policy is
        // part of the row: quietly making a thread quieter on the way out of a
        // request that came back as an error is the same silent half-move the
        // state machine exists to refuse.
        self.check_action(&thread_id, action)?;
        // The policy lands before the fold so that a permission arriving in the
        // same breath is judged by the policy the user just chose.
        if let Some(policy) = params.policy {
            self.store_or_err()?
                .set_thread_fold_policy(&thread_id, policy.as_str())
                .map_err(store_error)?;
        }
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
        // Reopening a *folded* thread finds its worktree exactly where it left
        // it. Reopening an *archived* one does not — archive removed the tree —
        // so it is put back on the branch archive saved the work onto (#23).
        self.restore_worktree(&params.thread_id);
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
        // After the adapter is gone, never before: removing the directory out
        // from under a live process is how a half-written file becomes the
        // agent's last act. Uncommitted work is committed to the thread's own
        // branch first, and a tree whose work cannot be saved is kept (#23).
        self.release_worktree(&params.thread_id, git::Release::Archived);
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
        // Delete forces where archive would give up — the user said delete, and
        // the tree is ours — but it still saves first. The `jabot/<id>` branch
        // survives a deleted thread on purpose: a branch costs nothing and is
        // the only copy of anything that was never pushed.
        self.release_worktree(&params.thread_id, git::Release::Deleted);
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
        let process = self.process_view(&row);
        // Where this work came from, when a bot sent it rather than the human
        // (#24). One indexed lookup, and it is the only place a reader of a
        // suddenly-busy standing thread can find out who asked.
        let handoff = self.handoff_view(&row.id);
        // The PRs this thread produced (#28). One indexed read, and it is the
        // thread half of a link the PR board already draws the other way.
        let pull_requests = super::pr::thread_prs(self.store_or_err()?, &row.id);
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
            worktree_path: row.worktree_path.clone(),
            repo_root: row.repo_root.clone(),
            repo: row.repo.clone(),
            forge_host: row.forge_host.clone(),
            branch: row.branch.clone(),
            host_id: row.host_id.clone(),
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
            handoff,
            pull_requests,
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
                bot_id: row.bot_id.clone(),
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

    /// One turn per session at a time. Refuse a prompt that would overlap a run
    /// that is still in flight.
    ///
    /// ACP gives us no way to tell two concurrent turns apart: the stop reason
    /// comes back on a response the host matches to the thread, not to the
    /// prompt it answers, so a second run would collect the first turn's
    /// outcome and the first run would be retired holding nothing. "A result
    /// must not be lost" is the invariant that forbids it. Queueing the second
    /// prompt instead of refusing it is #26's call, not a guess made here.
    ///
    /// A run left open by a host that quit has no adapter behind it and really
    /// is lost; it is let through so a relaunch can prompt again.
    pub(crate) fn refuse_overlapping_run(&self, thread_id: &str) -> Result<(), RpcError> {
        let Some((run_id, state)) = self.open_run(thread_id) else {
            return Ok(());
        };
        if !self.connections.contains_key(thread_id) {
            return Ok(());
        }
        Err(RpcError::RunInFlight {
            thread_id: thread_id.to_string(),
            run_id,
            state: state.as_str().to_string(),
        })
    }

    /// A prompt was accepted: open a run and stamp the session receipt.
    pub(crate) fn lifecycle_run_started(&mut self, thread_id: &str, acp_session_id: &str) {
        // Who asked for this turn. `prompt` unless a schedule fire claimed the
        // thread a moment ago (#25) — the ledger has accepted `schedule` since
        // 0001 and this is what finally writes it. Taken before the store
        // borrow below, and taken exactly once: the label belongs to this run.
        let (run_kind, trigger_json) = self.take_run_kind(thread_id);
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
        // A previous run still open here is one no adapter is going to report
        // on — a live one is refused by [`Self::refuse_overlapping_run`] before
        // we get this far, so what is left is a run whose host quit under it.
        // `lost` is exactly that: we stopped being able to find out.
        if let Ok(Some(previous)) = store.latest_run(thread_id) {
            if RunState::parse(&previous.state).map(RunState::is_open) == Ok(true) {
                let _ = store.set_run_state(
                    &previous.id,
                    RunState::Lost.as_str(),
                    Some("superseded by a new run"),
                );
            }
        }
        let run = match store.insert_run(thread_id, run_kind, trigger_json.as_deref()) {
            Ok(run) => run,
            Err(err) => {
                eprintln!("failed to open a run for {thread_id}: {err}");
                return;
            }
        };
        let _ = store.set_run_acp_session(&run.id, acp_session_id);
        let _ = store.set_run_state(&run.id, RunState::Running.as_str(), None);
        // A schedule fire has to be able to name the run it produced, and it
        // cannot look one up afterwards: a fast agent finishes the turn inside
        // `session_prompt`'s own pump (#25).
        self.note_scheduled_run(run_kind, &run.id);
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

    /// Any `session/update`. Keeps the process axis current, and ends the turn
    /// only for a v2 `state_update` that reports going idle **and** says why —
    /// the completion signal is the stop reason, never idleness on its own.
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
        // Going idle is a process fact, not an outcome. ACP carries the stop
        // reason on the `session/prompt` *response*, so a `state_update` that
        // only says "idle" has told us nothing about how the turn went —
        // ending the run on it would classify an ordinary `end_turn` as
        // `failed` before the response that says otherwise even arrives, and
        // the response would then find no open run to correct. The v1
        // completion path (`acp::handle_inbound`) ends the turn on its own.
        if reported == AcpState::Idle {
            if let Some(stop) = acp.get("stopReason").and_then(Value::as_str) {
                self.lifecycle_on_turn_end(thread_id, Some(stop));
            }
        }
    }

    /// The turn ended: close the run, and resurface if this thread was folded.
    /// An `active` thread just shows "session finished" in chat.
    ///
    /// Idempotent, because a v2 adapter reports the same ending twice — once as
    /// `state_update` with a stop reason and once as the prompt response.
    /// Whichever lands first closes the run; the other finds none open.
    pub(crate) fn lifecycle_on_turn_end(&mut self, thread_id: &str, stop_reason: Option<&str>) {
        let outcome = resurface::classify_stop(stop_reason);
        {
            let entry = self.lifecycle.entry(thread_id);
            entry.touch();
            entry.acp = AcpState::Idle;
        }
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
        // An ask the human still owes outranks the outcome. The agent may have
        // walked away from its own `request_permission`, but we cannot know
        // that, and replacing the Needs you card would drop the only pointer to
        // a question nobody answered.
        if self.pending_permission_count(thread_id) > 0 {
            return;
        }
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
            // The run stays `running` and the process stays alive on purpose:
            // stuck means "no output for a while", not "give up". `timed_out`
            // is reserved for a hard cap that actually ends a run.
            let summary = format!("no output for {}s", timeout.as_secs());
            let run_id = self
                .lifecycle
                .get(&thread_id)
                .and_then(|s| s.run_id.clone());
            // Latch only what actually landed. `resurface.md` gates the
            // backstop on `folded`, so a thread the user is still watching
            // reports `false` here — and it has to stay eligible, because the
            // only thing that clears the latch is the adapter saying
            // something, and a wedged adapter says nothing. Latching on the
            // way past would mean "went quiet on screen, then folded" never
            // produces a card: the exact failure the backstop exists for.
            let reported = match self.resurface_and_notify(
                &thread_id,
                ResurfaceReason::Stuck,
                &summary,
                run_id.as_deref(),
            ) {
                Ok(reported) => reported,
                Err(err) => {
                    eprintln!("failed to resurface {thread_id}: {err}");
                    // A store that could not take the card will not take it
                    // next tick either; latch so we log once, not per pump.
                    true
                }
            };
            self.lifecycle.entry(&thread_id).stuck_reported = reported;
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

    // ---- what the supervisor (#21) reads and writes on the process axis ----
    //
    // The axis is this module's, and it stays this module's: #21 reconciles it
    // rather than keeping a second copy, so these are the whole seam between
    // the two. Each one is a fact about a *process*, never about the overlay.

    /// A session was attached — a fresh spawn, a resume, or a load.
    pub(crate) fn lifecycle_on_attached(&mut self, thread_id: &str) {
        let entry = self.lifecycle.entry(thread_id);
        entry.connected = true;
        // Idle, not running: a restored session has context and no turn. The
        // next prompt is what makes it running.
        entry.acp = AcpState::Idle;
        entry.touch();
    }

    /// The adapter is gone and nothing is going to report on it — an idle
    /// evict, or a resume that could not find a verb the agent speaks.
    pub(crate) fn lifecycle_on_detached(&mut self, thread_id: &str) {
        let entry = self.lifecycle.entry(thread_id);
        entry.connected = false;
        entry.acp = AcpState::Unknown;
        entry.run_id = None;
    }

    /// How long since this thread's adapter last said anything.
    pub(crate) fn thread_idle_for(&self, thread_id: &str) -> Duration {
        self.lifecycle
            .get(thread_id)
            .map(|status| status.last_activity.elapsed())
            .unwrap_or_default()
    }

    /// Connected *and* mid-turn — the pair the sleep path resurfaces on.
    pub(crate) fn lifecycle_is_running(&self, thread_id: &str) -> bool {
        self.lifecycle
            .get(thread_id)
            .map(|status| status.connected && status.acp == AcpState::Running)
            .unwrap_or(false)
    }

    pub(crate) fn lifecycle_stuck_reported(&self, thread_id: &str) -> bool {
        self.lifecycle
            .get(thread_id)
            .map(|status| status.stuck_reported)
            .unwrap_or(false)
    }

    pub(crate) fn lifecycle_latch_stuck(&mut self, thread_id: &str, reported: bool) {
        self.lifecycle.entry(thread_id).stuck_reported = reported;
    }

    // ---- internals -----------------------------------------------------

    /// Where this action would move the thread, without moving it. Read-only,
    /// so a caller that writes something else alongside the transition can find
    /// out it is refused before it writes anything.
    fn check_action(
        &self,
        thread_id: &str,
        action: ThreadAction,
    ) -> Result<(ThreadState, ThreadState), RpcError> {
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
        Ok((from, to))
    }

    /// Persist the transition, or say why it cannot happen.
    fn apply_action(
        &mut self,
        thread_id: &str,
        action: ThreadAction,
    ) -> Result<ThreadRow, RpcError> {
        let (from, to) = self.check_action(thread_id, action)?;
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

    pub(crate) fn try_resurface(
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
    pub(crate) fn resurface_and_notify(
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
        // Name the card after the job, when the job is a schedule.
        //
        // A fire that lands on a folded thread resurfaces it, and #25's own
        // `schedule_card` then stands down rather than writing a second row —
        // one finished job, one card. But the card it stands down in favour of
        // is built from the *thread*, so a scheduled run came back as "Writer
        // finished" when what finished was "Morning triage". The user folded a
        // schedule; the row that brings it back should say so.
        //
        // Everything needed is already durable: `runs.trigger_json` is written
        // at run open and carries the schedule's title and ids. Read back
        // rather than kept in RAM because this resurface can happen long after
        // the dispatch — a `stuck` backstop, or a card written on the next
        // launch entirely.
        //
        // A run that is not a schedule, a trigger that will not parse, or no
        // run at all all fall through to the thread's own name, which is the
        // right answer for every prompted turn.
        let scheduled = run_id
            .and_then(|id| self.store.as_ref()?.get_run(id).ok().flatten())
            .filter(|run| run.kind == RUN_KIND_SCHEDULE)
            .and_then(|run| serde_json::from_str::<Value>(run.trigger_json.as_deref()?).ok());
        let name = scheduled
            .as_ref()
            .and_then(|trigger| trigger.get("schedule").and_then(Value::as_str))
            .unwrap_or(&row.title);
        let title = resurface::card_title(name, reason);
        let mut payload = json!({ "reason": reason.as_str() });
        // The same fields `schedule_card` would have attached had it been the
        // one to write the row, so a card means the same thing whichever path
        // produced it.
        if let Some(trigger) = scheduled.as_ref() {
            let object = payload.as_object_mut().expect("built as an object above");
            object.insert("source".into(), Value::String("schedule".into()));
            for key in ["scheduleId", "schedule", "fireId"] {
                if let Some(value) = trigger.get(key) {
                    object.insert(key.into(), value.clone());
                }
            }
        }
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
        // The card copy travels with the frame so #27 can name the thread in a
        // native notification without going back to the store. Persist first,
        // then notify — the order this whole method exists to hold.
        self.notify_inbox_resurface_card(thread_id, reason, Some(&title), Some(summary));
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
    /// as cancelled, give up on anything the user had queued, close the run,
    /// and drop the adapter.
    ///
    /// D-006 recorded that `session/close` was not sent here because the ACP
    /// layer had no close. It has one now, and `drop_adapter` sends it before
    /// it kills the group (#21).
    fn close_out(&mut self, thread_id: &str, why: &str) {
        self.withdraw_pending_permissions(thread_id, why, PermissionWithdrawal::Cancelled);
        // The user's undelivered words, before the adapter that was going to
        // receive them goes away. The queue is keyed on the thread and would
        // outlive both the adapter and this state, so a reopened thread would
        // hold every later prompt behind a message no agent will ever read —
        // and nothing drains a queue on a thread with no connection. The
        // `prompt_dropped` rows are where the words went (#14).
        self.drop_prompt_queue(thread_id, &format!("the thread was {why}"));
        // Archive is one of the two actions on a resurfaced card, and it hides
        // the thread from the Inbox for good (`state-machine.md`: archived is
        // Sidebar hidden / Inbox hidden). A badge for a thread the user has
        // just closed out has no screen left to send them to.
        if let Some(store) = self.store.as_ref() {
            if let Err(err) = store.mark_inbox_read(thread_id) {
                eprintln!("failed to clear the Inbox badge for {thread_id}: {err}");
            }
        }
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
    pub(crate) fn open_run(&self, thread_id: &str) -> Option<(String, RunState)> {
        let store = self.store.as_ref()?;
        let run = store.latest_run(thread_id).ok()??;
        let state = RunState::parse(&run.state).ok()?;
        state.is_open().then_some((run.id, state))
    }

    fn latest_open_run_id(&self, thread_id: &str) -> Option<String> {
        self.open_run(thread_id).map(|(id, _)| id)
    }

    /// The process axis for one thread, plus what #21 could do about it.
    ///
    /// `resumable` and `drift` are computed here rather than cached because
    /// they are answers about *now*: a folder deleted a second ago makes a
    /// resumable thread unresumable, and a cached `true` would send the user
    /// at a button that cannot work.
    fn process_view(&self, row: &ThreadRow) -> ProcessView {
        let status = self.lifecycle.get(&row.id);
        let readiness = self.resume_readiness(row);
        ProcessView {
            connected: status.map(|s| s.connected).unwrap_or(false),
            acp_state: status
                .map(|s| s.acp)
                .unwrap_or(AcpState::Unknown)
                .as_str()
                .to_string(),
            pending_permissions: self.pending_permission_count(&row.id),
            pid: self.adapter_pid(&row.id),
            resumable: readiness.resumable,
            drift: readiness.drift,
        }
    }

    pub(crate) fn lifecycle_thread(&self, thread_id: &str) -> Result<Option<ThreadRow>, RpcError> {
        self.store_or_err()?
            .get_thread(thread_id)
            .map_err(store_error)
    }

    pub(crate) fn store_or_err(&self) -> Result<&Store, RpcError> {
        self.store.as_ref().ok_or(RpcError::StoreUnavailable)
    }

    /// What this thread would be spawned with today — the other half of the
    /// drift check against the stored receipt.
    pub(crate) fn fingerprint_for(&self, thread: &ThreadRow) -> SessionFingerprint {
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

/// The ACP tool kind an ask is about, when the agent named one. Shared with
/// the broker (#20), which stores it on the request record so the fold policy
/// and a reopened card read the same word.
pub(crate) fn permission_kind(subject: &Value) -> Option<String> {
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
        bot_id: thread.and_then(|t| t.bot_id.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{HOST_HELLO, THREAD_FOLD, THREAD_OPEN};

    /// Wait for Inbox is the only place in this host that answers an agent
    /// *without a human*. Every gate below is the difference between "the host
    /// read a file while you were away" and "an agent did something you never
    /// approved", so each one is asserted on its own rather than through a
    /// single happy-path scenario that would still pass with any one of them
    /// removed.
    ///
    /// `tests/lifecycle.rs` drives the two ordinary outcomes end to end through
    /// a real adapter (a read is answered, an execute is not). What it cannot
    /// reach is the shape of the *options*, because the fake agent decides
    /// those — and "which of the agent's options did the host pick" is the half
    /// of this decision that grants the permission.
    fn session() -> (HostSession, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        session.handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None));
        (session, dir)
    }

    fn open(session: &mut HostSession, thread_id: &str) {
        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            THREAD_OPEN,
            Some(json!({
                "threadId": thread_id,
                "title": "Auth migration",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "harnessId": "claude",
            })),
        ));
        assert!(response.error.is_none(), "{:?}", response.error);
    }

    fn fold(session: &mut HostSession, thread_id: &str, policy: &str) {
        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(3),
            THREAD_FOLD,
            Some(json!({ "threadId": thread_id, "policy": policy })),
        ));
        assert!(response.error.is_none(), "{:?}", response.error);
    }

    /// A thread asleep under Wait for Inbox — the only configuration in which
    /// anything may be auto-allowed at all.
    fn quiet_thread(thread_id: &str) -> (HostSession, tempfile::TempDir) {
        let (mut session, dir) = session();
        open(&mut session, thread_id);
        fold(&mut session, thread_id, "wait_for_inbox");
        (session, dir)
    }

    fn read() -> Value {
        json!({ "kind": "read", "title": "Read src/auth.ts" })
    }

    /// The options an agent actually offers, in ACP's spelling: an opaque
    /// `optionId` with a `kind` that says what it means.
    fn options(kinds: &[&str]) -> Value {
        Value::Array(
            kinds
                .iter()
                .map(
                    |kind| json!({ "optionId": format!("opt-{kind}"), "name": kind, "kind": kind }),
                )
                .collect(),
        )
    }

    #[test]
    fn a_folded_read_with_an_allow_once_on_offer_is_answered_by_the_host() {
        let (session, _dir) = quiet_thread("t-read");
        let disposition = session.lifecycle_permission_policy(
            "t-read",
            &read(),
            &options(&["allow_once", "reject_once"]),
        );
        // The agent's own id for the option, not the word "allow_once": an
        // answer naming an option the agent never offered is an answer it
        // cannot act on.
        assert_eq!(
            disposition,
            PermissionDisposition::AutoAllow {
                option_id: "opt-allow_once".into()
            }
        );
    }

    /// The gate with the largest blast radius. `allow_always` is not a bigger
    /// version of `allow_once` — it is a standing grant that outlives the turn,
    /// the fold and the thread, and Wait for Inbox exists to answer *one* read
    /// while the user is away. A policy that treated any allow-shaped option as
    /// good enough would hand an agent a permanent permission for a question
    /// the user never saw.
    #[test]
    fn a_folded_read_is_never_auto_allowed_always() {
        let (session, _dir) = quiet_thread("t-always");
        for offered in [
            options(&["allow_always", "reject_once"]),
            options(&["allow_always"]),
            // Nothing to pick at all: an agent that offers only refusals.
            options(&["reject_once", "reject_always"]),
            json!([]),
            // Not an array, and an option with no id — malformed input from an
            // adapter must reach the human rather than be guessed at.
            json!({ "allow_once": true }),
            json!([{ "kind": "allow_once" }]),
        ] {
            assert_eq!(
                session.lifecycle_permission_policy("t-always", &read(), &offered),
                PermissionDisposition::Ask,
                "auto-allowed against {offered}"
            );
        }
    }

    /// Everything that is not a read reaches a human, however quiet the thread
    /// was asked to be. The list is spelled out because "read" is decided by a
    /// string an adapter sends: an unrecognised kind, a missing one, or a
    /// capitalised one must all fall to the safe side.
    #[test]
    fn only_a_read_is_ever_auto_allowed() {
        let (session, _dir) = quiet_thread("t-kinds");
        for subject in [
            json!({ "kind": "execute", "title": "Run rm -rf /" }),
            json!({ "kind": "delete", "title": "Delete src/" }),
            json!({ "kind": "edit", "title": "Edit src/auth.ts" }),
            json!({ "kind": "fetch", "title": "GET https://example.com" }),
            json!({ "kind": "Read", "title": "Read src/auth.ts" }),
            json!({ "kind": "read_and_execute", "title": "…" }),
            json!({ "title": "no kind at all" }),
            json!({ "kind": 7 }),
            Value::Null,
        ] {
            assert_eq!(
                session.lifecycle_permission_policy("t-kinds", &subject, &options(&["allow_once"])),
                PermissionDisposition::Ask,
                "auto-allowed {subject}"
            );
        }
    }

    /// Wait for Inbox is a property of a thread the user put to sleep and said
    /// "do not wake me". A thread they are looking at, or one folded under the
    /// default policy, has a human available — and asking a human who is there
    /// costs nothing, while not asking them is the whole failure.
    #[test]
    fn a_thread_the_user_is_present_for_is_never_answered_for_them() {
        let (mut session, _dir) = session();

        // Active: the user is in the thread.
        open(&mut session, "t-active");
        assert_eq!(
            session.lifecycle_permission_policy("t-active", &read(), &options(&["allow_once"])),
            PermissionDisposition::Ask
        );

        // Folded, but under the default policy: fold is visibility, not consent.
        open(&mut session, "t-plain");
        fold(&mut session, "t-plain", "default");
        assert_eq!(
            session.lifecycle_permission_policy("t-plain", &read(), &options(&["allow_once"])),
            PermissionDisposition::Ask
        );

        // A thread that is not in the store at all. The policy is read from the
        // row, so no row means no evidence of consent.
        assert_eq!(
            session.lifecycle_permission_policy("t-ghost", &read(), &options(&["allow_once"])),
            PermissionDisposition::Ask
        );

        // And a host with no store, which is the state a broken `jabot.sqlite`
        // leaves the app in — still running, still reachable by an adapter.
        let ephemeral = HostSession::ephemeral();
        assert_eq!(
            ephemeral.lifecycle_permission_policy("t-read", &read(), &options(&["allow_once"])),
            PermissionDisposition::Ask
        );
    }

    /// Waking the thread takes the standing consent with it: the user is back,
    /// so the next question is theirs to answer.
    #[test]
    fn reopening_a_quiet_thread_stops_the_host_answering_for_it() {
        let (mut session, _dir) = quiet_thread("t-woken");
        assert!(matches!(
            session.lifecycle_permission_policy("t-woken", &read(), &options(&["allow_once"])),
            PermissionDisposition::AutoAllow { .. }
        ));

        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(4),
            crate::host::protocol::THREAD_REOPEN,
            Some(json!({ "threadId": "t-woken" })),
        ));
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(
            session.lifecycle_permission_policy("t-woken", &read(), &options(&["allow_once"])),
            PermissionDisposition::Ask
        );
    }
}
