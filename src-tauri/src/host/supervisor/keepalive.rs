//! Keep-alive: what the supervisor does between events.
//!
//! Three jobs, all of them polls, all of them cheap enough to run on the ACP
//! pump: reap adapters that died, notice that the machine was asleep, and
//! close sessions nobody is using.
//!
//! The reaping is the one that is not obvious. The connection layer learns an
//! adapter is gone from EOF on its stdout — but an adapter that forked
//! something inheriting that pipe can exit without ever closing it, and then
//! the reader thread blocks forever on a pipe nobody will write to while JaBot
//! goes on reporting a live session. `waitpid` cannot be fooled that way, so
//! the pid is what the supervisor believes.

use std::time::Duration;

use super::super::lifecycle::process::AcpState;
use super::super::protocol::methods::ResurfaceReason;
use super::super::HostSession;

impl HostSession {
    /// Called from the ACP pump. Rate-limited to the probe interval, so the
    /// 50ms pump in `jabot-hostd` does not turn into 20 `waitpid`s a second
    /// per adapter.
    pub(crate) fn supervisor_tick(&mut self) {
        if self.supervisor.last_probe.elapsed() < self.supervisor.probe_interval {
            return;
        }
        self.supervisor.last_probe = std::time::Instant::now();
        if let Some(gap) = self.supervisor.clock.observe() {
            self.wake_from_sleep(gap);
            return;
        }
        self.reap_dead_adapters();
        self.drain_stranded_queues();
        self.release_unacknowledged_cancels();
        self.evict_idle_adapters();
    }

    /// Stop holding a user's words behind a cancel the adapter ignored.
    ///
    /// `session/cancel` is an ACP *notification*: there is no reply, and the
    /// only acknowledgement is the prompt response that ends the turn. An
    /// adapter that ignores it therefore keeps `AcpState::Running` and an open
    /// run, which makes the thread invisible to both branches of
    /// [`Self::drain_stranded_queues`] — the first wants an idle adapter with
    /// no open run, the second wants no adapter at all. So anything the user
    /// typed after pressing stop sits in `prompt_queue` behind a turn that
    /// will never end, showing "N messages waiting", until the adapter
    /// eventually dies and they are reported dropped — possibly hours later.
    ///
    /// What this deliberately does **not** do is end the run or synthesize a
    /// stop reason. The host cannot know the turn ended, and saying it did
    /// would be exactly the invention that was refused when this was first
    /// considered. The run stays open, the adapter stays running, and the
    /// `stuck` backstop in `lifecycle_tick` still owns the run itself.
    ///
    /// The only thing corrected here is the lie of omission: the user is told
    /// their queued prompts are not going to be delivered, at the point that
    /// becomes true, instead of finding out whenever the adapter happens to
    /// close.
    fn release_unacknowledged_cancels(&mut self) {
        let grace = self.supervisor.cancel_grace;
        // Zero means "wait as long as it takes", which is a real choice.
        if grace.is_zero() {
            return;
        }
        let overdue: Vec<String> = self
            .cancel_requested
            .iter()
            .filter(|(_, requested)| requested.elapsed() >= grace)
            .map(|(thread_id, _)| thread_id.clone())
            .collect();
        for thread_id in overdue {
            // Whatever happens below, this thread has had its grace. Clearing
            // first means a thread whose queue is already empty stops being
            // scanned every tick, and a later cancel starts a fresh stopwatch.
            self.cancel_requested.remove(&thread_id);
            if self.queue_depth(&thread_id) == 0 {
                continue;
            }
            self.drop_prompt_queue(&thread_id, "the agent never acknowledged the stop");
        }
    }

    /// The machine was asleep for `gap`, and is not any more.
    ///
    /// Also the entry point a macOS `NSWorkspace.didWakeNotification` observer
    /// would call: the poll in [`Self::supervisor_tick`] infers the same event
    /// from the clocks, so the behaviour does not depend on an API that only
    /// exists on one platform.
    ///
    /// `keep-alive.md`'s crash-and-sleep table: ping the adapters, resume the
    /// dead ones, and resurface anything that was running as `stuck` — "we
    /// cannot prove the tool finished". A tool that was half-way through a
    /// `git push` when the lid shut is exactly the case where claiming success
    /// or failure would both be inventions.
    pub fn wake_from_sleep(&mut self, gap: Duration) {
        self.supervisor.sleeps_observed += 1;
        // Order matters: reap first, so a thread whose adapter did not survive
        // gets the `failed` card the crash path writes, rather than a `stuck`
        // card about a process that is not there.
        self.reap_dead_adapters();
        let survivors: Vec<String> = self
            .connections
            .keys()
            .filter(|thread_id| self.lifecycle_is_running(thread_id))
            .filter(|thread_id| self.pending_permission_count(thread_id) == 0)
            .cloned()
            .collect();
        let summary = format!("the Mac slept for {}s while it was working", gap.as_secs());
        for thread_id in survivors {
            // Same latch the idle backstop uses: report the silence once, not
            // on every tick for as long as it lasts.
            if self.lifecycle_stuck_reported(&thread_id) {
                continue;
            }
            let run_id = self.open_run(&thread_id).map(|(id, _)| id);
            let reported = match self.resurface_and_notify(
                &thread_id,
                ResurfaceReason::Stuck,
                &summary,
                run_id.as_deref(),
            ) {
                Ok(reported) => reported,
                Err(err) => {
                    eprintln!("failed to resurface {thread_id} after a sleep: {err}");
                    true
                }
            };
            self.lifecycle_latch_stuck(&thread_id, reported);
        }
    }

    /// Prompts the user typed behind a turn that has since ended.
    ///
    /// #14 drains the queue when a `session/prompt` **response** arrives,
    /// which is the ACP v1 completion signal. A v2 adapter reports the end of
    /// a turn as an idle `state_update` and may never send that response at
    /// all, and the interrupt path has a narrower version of the same problem:
    /// `session/cancel` pumps before the follow-up is enqueued, so a fast
    /// adapter can end the turn in the window between the two. Either way the
    /// user's next message sits in a queue nothing will ever drain, and the
    /// chat waits forever on a message no agent will ever read.
    ///
    /// The supervisor is the right place to catch it because the condition is
    /// a *reconciliation*: the queue claims work is pending, the ledger says
    /// nothing is running, and the adapter says it is idle. All three have to
    /// agree before anything is sent, so a turn genuinely in flight — which
    /// keeps its run open — is never overtaken.
    ///
    /// A queue on a thread with **no** adapter is the same failure and cannot
    /// be fixed by sending: `drain_prompt_queue` needs a live session, so
    /// nothing would ever empty it and every later prompt would be held behind
    /// it. Those are dropped rather than delivered, which is what every other
    /// end-of-adapter path already does.
    fn drain_stranded_queues(&mut self) {
        let stranded: Vec<String> = self
            .connections
            .keys()
            .filter(|thread_id| self.queue_depth(thread_id) > 0)
            .filter(|thread_id| self.acp_state(thread_id) == AcpState::Idle)
            .filter(|thread_id| self.open_run(thread_id).is_none())
            .filter(|thread_id| self.pending_permission_count(thread_id) == 0)
            .cloned()
            .collect();
        for thread_id in stranded {
            self.drain_prompt_queue(&thread_id);
        }
        let orphaned: Vec<String> = self
            .queued_thread_ids()
            .into_iter()
            .filter(|thread_id| !self.connections.contains_key(thread_id))
            // An open run without an adapter is a *lost* run, and the boot and
            // reap paths own it; touching the queue here would race them.
            .filter(|thread_id| self.open_run(thread_id).is_none())
            .collect();
        for thread_id in orphaned {
            self.drop_prompt_queue(&thread_id, "the adapter is no longer running");
        }
    }

    /// Adapters whose process is gone, whatever their stdout says.
    fn reap_dead_adapters(&mut self) {
        let dead: Vec<String> = self
            .connections
            .iter_mut()
            .filter_map(|(thread_id, conn)| (!conn.is_alive()).then(|| thread_id.clone()))
            .collect();
        for thread_id in dead {
            self.on_adapter_gone(&thread_id, Some("the adapter process exited"));
        }
    }

    /// Close sessions that are doing nothing for a thread nobody is watching.
    ///
    /// Every gate here is a way of getting this wrong:
    ///
    /// - **an open run** — folded and still working never evicts, at any age;
    ///   that thread is the product's whole premise.
    /// - **a pending permission** — the agent is blocked on a question, and
    ///   dropping the process is answering it with silence.
    /// - **a queued prompt** — the user has already typed the next turn.
    /// - **`active`** — the thread is on screen; a composer whose adapter
    ///   vanished under it is not "idle-evicted", it is broken.
    /// - **`acpState` not idle** — anything else means we do not know.
    ///
    /// And one gate that is not about the thread at all: the session has to be
    /// *restorable*. Eviction is only safe because resume exists, so an
    /// adapter that speaks neither `session/resume` nor `session/load` keeps
    /// its process — closing it would trade a little memory for the agent's
    /// entire context, and the next prompt would silently continue in a new
    /// session. Holding a process is the honest cost of an adapter that cannot
    /// hand its conversation back.
    fn evict_idle_adapters(&mut self) {
        let grace = self.supervisor.idle_evict_after;
        if grace.is_zero() {
            return;
        }
        let candidates: Vec<String> = self
            .connections
            .keys()
            .filter(|thread_id| self.acp_state(thread_id) == AcpState::Idle)
            .filter(|thread_id| self.thread_idle_for(thread_id) >= grace)
            .filter(|thread_id| self.pending_permission_count(thread_id) == 0)
            .filter(|thread_id| self.open_run(thread_id).is_none())
            .filter(|thread_id| self.queue_depth(thread_id) == 0)
            .filter(|thread_id| !self.thread_is_active(thread_id))
            .cloned()
            .collect();
        for thread_id in candidates {
            if !self.can_come_back(&thread_id) {
                continue;
            }
            // `drop_adapter` sends `session/close` first where the adapter
            // advertised it, which is the half Buzz never did.
            self.drop_adapter(&thread_id);
            self.lifecycle_on_detached(&thread_id);
        }
    }

    /// Would a resume put this exact conversation back? Both halves have to
    /// hold: the adapter has to speak a restoring verb, and #15's receipt has
    /// to still describe the job.
    fn can_come_back(&self, thread_id: &str) -> bool {
        let restorable = self
            .connections
            .get(thread_id)
            .map(|conn| conn.capabilities())
            .map(|caps| caps.resume || caps.load_session)
            .unwrap_or(false);
        if !restorable {
            return false;
        }
        self.lifecycle_thread(thread_id)
            .ok()
            .flatten()
            .map(|thread| self.resume_readiness(&thread).resumable)
            .unwrap_or(false)
    }

    fn thread_is_active(&self, thread_id: &str) -> bool {
        self.store
            .as_ref()
            .and_then(|store| store.get_thread(thread_id).ok().flatten())
            .map(|thread| thread.deleted_at.is_none() && thread.state == "active")
            // A thread with no row is a bare ACP test session; treat it as
            // watched rather than evict something nobody can resume.
            .unwrap_or(true)
    }
}
