//! Boot reconciliation: what a host finds when it comes up after a stop.
//!
//! D-006 left this open, and it is the part of #21 that decision #4 turns on.
//! Quit, crash, lid close and reboot all end the same way — no adapter, and a
//! `runs` table still claiming work is in flight. Until something walks that
//! table, every thread reports `acpState: unknown` with a run that says
//! `running`, which is a ledger asserting a process that does not exist.
//!
//! The pass reconciles **against the ledger, never against RAM**. There is no
//! RAM to reconcile against: [`super::super::lifecycle::LifecycleState`] is
//! empty at this point by design (#5, "still working is supervisor RAM,
//! reconciled on boot"). What it reads is `runs`, and what it writes is a
//! terminal run state plus, where the thread is somewhere a card applies, an
//! Inbox row.
//!
//! Every open run becomes `lost`. Not `failed`: `failed` says the work went
//! wrong and invites a retry, and we do not know that it did. `lost` is the
//! ledger's word for "we stopped being able to find out", which is exactly
//! what happens when the host holding the only connection goes away.
//!
//! What the *card* says is a separate question, and the two research files
//! answer it differently on purpose:
//!
//! - A run left in `needs_you` resurfaces as **needs you** —
//!   `state-machine.md`: "If we quit with an outstanding permission, next
//!   launch resurfaces as `needs_you` with copy 'the agent was waiting on you;
//!   reopen to continue' rather than replaying a dead RPC."
//! - A run left `running` resurfaces as **stuck** — `keep-alive.md`'s crash
//!   and sleep table: "If `uiState == folded` and last `acpState == running`,
//!   resurface `stuck` ('interrupted by restart')." Stuck rather than failed
//!   because the conversation is resumable and the ask of the user is to
//!   reopen, not to retry.

use super::super::lifecycle::ledger::{self, RunState};
use super::super::protocol::methods::{BootNoteView, ResurfaceReason};
use super::super::store::RunRow;
use super::super::HostSession;

/// The copy `state-machine.md` specifies, verbatim. It is the only thing that
/// tells a user why an answer they gave is not being acted on.
pub(crate) const WAS_WAITING_ON_YOU: &str = "the agent was waiting on you; reopen to continue";
const INTERRUPTED: &str = "interrupted by restart";
const NEVER_STARTED: &str = "the host stopped before this run started";

/// How one abandoned run is dealt with.
struct Verdict {
    detail: &'static str,
    /// `None` for a run that never began: there is nothing to tell the user
    /// about work that produced nothing and asked for nothing.
    reason: Option<ResurfaceReason>,
}

fn verdict(state: RunState) -> Option<Verdict> {
    match state {
        RunState::NeedsYou => Some(Verdict {
            detail: WAS_WAITING_ON_YOU,
            reason: Some(ResurfaceReason::NeedsYou),
        }),
        RunState::Running => Some(Verdict {
            detail: INTERRUPTED,
            reason: Some(ResurfaceReason::Stuck),
        }),
        RunState::Queued => Some(Verdict {
            detail: NEVER_STARTED,
            reason: None,
        }),
        // Terminal states are already reconciled; nothing to do.
        _ => None,
    }
}

impl HostSession {
    /// Walk every run a previous host left open. Idempotent: a second call
    /// finds nothing, because the first one closed them all.
    ///
    /// Failures are logged rather than propagated. A store that cannot take
    /// one row should cost the user that row, not the launch.
    pub(crate) fn reconcile_boot(&mut self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let open = match store.list_open_runs() {
            Ok(open) => open,
            Err(err) => {
                eprintln!("boot reconciliation could not read the run ledger: {err}");
                return;
            }
        };
        // Ordered thread, then newest run first: the newest is the one the
        // thread's card is about, and older strays are closed silently.
        let mut seen_thread: Option<String> = None;
        for run in open {
            let latest = seen_thread.as_deref() != Some(run.thread_id.as_str());
            seen_thread = Some(run.thread_id.clone());
            self.reconcile_run(&run, latest);
        }
    }

    fn reconcile_run(&mut self, run: &RunRow, latest: bool) {
        let Ok(state) = RunState::parse(&run.state) else {
            return;
        };
        let Some(verdict) = verdict(state) else {
            return;
        };
        let detail = verdict.detail;
        if let Some(store) = self.store.as_ref() {
            if ledger::advance(state, RunState::Lost).is_ok() {
                if let Err(err) =
                    store.set_run_state(&run.id, RunState::Lost.as_str(), Some(detail))
                {
                    eprintln!("failed to close abandoned run {}: {err}", run.id);
                    return;
                }
            }
        }
        if !latest {
            return;
        }
        // The thread's own record of how it stopped, for the header line on a
        // thread the user simply reopens without going through the Inbox.
        if let Some(store) = self.store.as_ref() {
            let _ = store.set_thread_stop(&run.thread_id, Some("host_stopped"), Some(detail));
        }
        let mut note = BootNoteView {
            thread_id: run.thread_id.clone(),
            run_id: Some(run.id.clone()),
            was: state.as_str().to_string(),
            now: RunState::Lost.as_str().to_string(),
            resurfaced_as: None,
            detail: detail.to_string(),
        };
        if let Some(reason) = verdict.reason {
            if self.announce(&run.thread_id, reason, detail, &run.id) {
                note.resurfaced_as = Some(reason);
            }
        }
        self.supervisor.boot_notes.push(note);
    }

    /// Put the card in front of the user, one way or another.
    ///
    /// The straightforward case is a folded thread: it resurfaces and a new
    /// Inbox row lands. The case that actually happens with an unanswered
    /// permission is the awkward one — the thread resurfaced `needs_you` the
    /// moment the agent asked, *before* the quit, so the transition is a no-op
    /// and its card still carries the old subject line ("Run ls") with no hint
    /// that the process behind it is gone. Restating that row is what makes
    /// `state-machine.md`'s promise true; a second card for the same
    /// unanswered question would be noise, and leaving the old one would be a
    /// card that lies about a live request.
    fn announce(
        &mut self,
        thread_id: &str,
        reason: ResurfaceReason,
        detail: &str,
        run_id: &str,
    ) -> bool {
        match self.resurface_and_notify(thread_id, reason, detail, Some(run_id)) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(err) => {
                eprintln!("failed to resurface {thread_id} at boot: {err}");
                return false;
            }
        }
        let Some(store) = self.store.as_ref() else {
            return false;
        };
        let restated = store.restate_inbox_event(
            thread_id,
            super::super::lifecycle::resurface::inbox_kind(reason),
            detail,
        );
        match restated {
            Ok(true) => {
                self.notify_inbox_resurface(thread_id, reason);
                true
            }
            Ok(false) => false,
            Err(err) => {
                eprintln!("failed to restate the Inbox card for {thread_id}: {err}");
                false
            }
        }
    }
}
