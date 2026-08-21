//! The tick: what a schedule owes, and what its result becomes.
//!
//! Two passes, deliberately independent, both idempotent.
//!
//! [`HostSession::dispatch_due_schedules`] turns *time* into *work*. It claims
//! an occurrence and opens a run, and it is the only thing that writes
//! `schedule_fires`.
//!
//! [`HostSession::deliver_finished_fires`] turns *work* into a *card*. It reads
//! the run ledger and nothing else — which is what makes it survive a restart:
//! a fire dispatched by a host that then quit has its run closed as `lost` by
//! #21's boot pass, and the next launch's delivery pass writes the card that
//! says so. Nothing is held in RAM between the two, because a schedule whose
//! result depended on the process that started it would lose exactly the runs a
//! user most needs to hear about.

use serde_json::json;

use super::super::lifecycle::ledger::RunState;
use super::super::lifecycle::resurface;
use super::super::protocol::error::RpcError;
use super::super::protocol::methods::{PromptMode, PromptParams, ResurfaceReason};
use super::super::store::{
    BotRow, NewScheduleFire, ScheduleFireRow, ScheduleRow, FIRE_DELIVERED, FIRE_DISPATCHED,
    FIRE_FAILED, FIRE_SKIPPED,
};
use super::super::HostSession;
use super::{
    catchup, from_stamp, to_stamp, CatchUp, CronSpec, PendingRun, RUN_KIND_PROMPT,
    RUN_KIND_SCHEDULE,
};

impl HostSession {
    /// Called from the ACP pump. Rate-limited to the tick interval.
    pub(crate) fn schedule_tick(&mut self) {
        // A dispatch prompts, `session_prompt` pumps, and the pump comes back
        // here. Without this the fire's own prompt would re-enter the delivery
        // pass and rule on the row that is still being written.
        if self.schedules.dispatching {
            return;
        }
        if self.schedules.last_tick.elapsed() < self.schedules.tick_interval {
            return;
        }
        self.schedules.last_tick = std::time::Instant::now();
        // Delivery first: a fire whose run ended is a card the user is already
        // owed, and dispatching before delivering would let a fast schedule
        // start its next run while the last one's result was still unreported.
        self.deliver_finished_fires();
        self.dispatch_due_schedules();
    }

    /// The half of the tick that belongs to boot, run from #21's pass.
    ///
    /// Only delivery. Dispatch stays on the pump, which is at most one tick
    /// away: `HostSession::load` runs on the app's startup path, and spawning
    /// an adapter subprocess there would put a schedule's job in front of the
    /// window opening. What genuinely is boot work is the other half — every
    /// run a stopped host left open has just been closed as `lost`, and those
    /// are precisely the fires whose card nobody has written.
    pub(crate) fn reconcile_schedule_fires(&mut self) {
        self.deliver_finished_fires();
    }

    // ---- time → work -----------------------------------------------------

    fn dispatch_due_schedules(&mut self) {
        let now = chrono::Local::now();
        let due = match self.store.as_ref() {
            Some(store) => store.list_due_schedules(&to_stamp(now)).unwrap_or_default(),
            None => return,
        };
        for schedule in due {
            self.rule_on(&schedule, now);
        }
    }

    /// Decide what one due schedule gets, and claim it.
    fn rule_on(&mut self, schedule: &ScheduleRow, now: chrono::DateTime<chrono::Local>) {
        let spec = match CronSpec::parse(&schedule.cron) {
            Ok(spec) => spec,
            Err(err) => {
                // A row whose cron no longer parses cannot be scheduled, and
                // guessing an interval for it would be worse than stopping.
                // Park the clock and leave the record saying why.
                eprintln!("schedule {}: {err}", schedule.id);
                if let Some(store) = self.store.as_ref() {
                    let _ = store.set_schedule_due(&schedule.id, None);
                }
                return;
            }
        };
        let Some(due) = schedule.next_run_at.as_deref().and_then(from_stamp) else {
            return;
        };
        let Some(plan) = catchup::plan(&spec, due, now, CatchUp::parse(&schedule.catch_up)) else {
            return;
        };
        let claimed = {
            let Some(store) = self.store.as_ref() else {
                return;
            };
            let new = NewScheduleFire {
                schedule_id: schedule.id.clone(),
                due_at: to_stamp(plan.due),
                state: if plan.fire.is_some() {
                    FIRE_DISPATCHED.to_string()
                } else {
                    FIRE_SKIPPED.to_string()
                },
                caught_up: plan.caught_up,
                skipped_count: plan.skipped,
                detail: plan.detail.clone(),
            };
            match store.claim_fire(&new, plan.next.map(to_stamp).as_deref()) {
                // Somebody else took this occurrence. Not an error: that is the
                // uniqueness constraint doing the job it exists for.
                Ok(None) => return,
                Ok(Some(fire)) => fire,
                Err(err) => {
                    eprintln!("schedule {}: could not claim a fire: {err}", schedule.id);
                    return;
                }
            }
        };
        if plan.fire.is_none() {
            return;
        }
        self.dispatch(schedule, &claimed);
    }

    /// Put the schedule's prompt on its bot's standing thread.
    fn dispatch(&mut self, schedule: &ScheduleRow, fire: &ScheduleFireRow) {
        self.schedules.dispatching = true;
        self.dispatch_inner(schedule, fire);
        self.schedules.dispatching = false;
    }

    fn dispatch_inner(&mut self, schedule: &ScheduleRow, fire: &ScheduleFireRow) {
        let bot = match self.store.as_ref().and_then(|store| {
            store
                .get_bot(&schedule.bot_id)
                .ok()
                .flatten()
                .map(|bot| Box::new(bot) as Box<BotRow>)
        }) {
            Some(bot) => *bot,
            None => {
                // `bots.id` is a foreign key with `ON DELETE CASCADE`, so this
                // is a store that lost a row rather than an ordinary removal.
                self.settle_fire(fire, FIRE_FAILED, "the bot this schedule runs as is gone");
                return;
            }
        };
        let thread = match self.open_standing_thread(&bot) {
            Ok(thread) => thread,
            Err(err) => {
                // No workspace, no harness, no thread. There is nowhere to put
                // an Inbox card either — the card needs a thread — so the fire
                // row is the only record, which is what `schedule/list` shows.
                self.settle_fire(fire, FIRE_FAILED, &format!("{err}"));
                return;
            }
        };
        let thread_id = thread.thread_id.clone();
        // The CronJob rule, and the one that keeps a fast schedule from piling
        // up: a bot still working on its last task does not get a second one.
        // Queueing would be worse than skipping — a nightly job that overran
        // would come back to N copies of itself, all acting on a day that has
        // moved on.
        if self.open_run(&thread_id).is_some() || self.queue_depth(&thread_id) > 0 {
            if let Some(store) = self.store.as_ref() {
                let _ = store.set_fire_target(&fire.id, Some(&thread_id), None);
            }
            self.settle_fire(
                fire,
                FIRE_SKIPPED,
                &format!("{} was still working on its last task", bot.name),
            );
            return;
        }
        if let Some(store) = self.store.as_ref() {
            let _ = store.set_fire_target(&fire.id, Some(&thread_id), None);
            // Display only, so the settings row can link to the conversation
            // without a second lookup; the durable link is on the fire.
            let _ = store.set_schedule_thread(&schedule.id, &thread_id);
        }
        // The next run opened on this thread is this fire's. Set before the
        // prompt, cleared after, so nothing else can inherit the label.
        self.schedules.claimed_run = None;
        self.schedules.pending = Some(PendingRun {
            thread_id: thread_id.clone(),
            trigger_json: json!({
                "scheduleId": schedule.id,
                "schedule": schedule.title,
                "fireId": fire.id,
                "dueAt": fire.due_at,
                "caughtUp": fire.caught_up,
            })
            .to_string(),
        });
        let sent = self.session_prompt(PromptParams {
            thread_id: thread_id.clone(),
            content: serde_json::Value::String(prompt_text(&schedule.title, &schedule.prompt)),
            // `Reject` rather than `Queue`: the busy check above is the policy,
            // and a queue would smuggle the pile-up back in through the door
            // the check just closed.
            mode: Some(PromptMode::Reject),
            cwd: None,
            harness_id: None,
            runtime: None,
        });
        self.schedules.pending = None;
        match sent {
            Ok(_) => {
                let run_id = self.schedules.claimed_run.take();
                if let Some(store) = self.store.as_ref() {
                    let _ = store.set_fire_target(&fire.id, Some(&thread_id), run_id.as_deref());
                }
                if run_id.is_none() {
                    // Accepted but no run on the ledger: nothing will ever
                    // report on this, so close it now rather than leave the
                    // delivery pass waiting on a run that does not exist.
                    self.settle_fire(fire, FIRE_FAILED, "the prompt opened no run");
                }
            }
            Err(err) => {
                let detail = err.to_string();
                self.settle_fire(fire, FIRE_FAILED, &detail);
                // A dispatch that never reached an agent is exactly the thing a
                // user must not have to go looking for.
                self.schedule_card(
                    &thread_id,
                    None,
                    ResurfaceReason::Failed,
                    schedule,
                    &fire.id,
                    &detail,
                );
            }
        }
    }

    // ---- work → card -----------------------------------------------------

    fn deliver_finished_fires(&mut self) {
        let open = match self.store.as_ref() {
            Some(store) => store.list_undelivered_fires().unwrap_or_default(),
            None => return,
        };
        for fire in open {
            self.deliver_fire(&fire);
        }
    }

    fn deliver_fire(&mut self, fire: &ScheduleFireRow) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Some(run_id) = fire.run_id.clone() else {
            // Dispatched with no run to watch. Older rows and interrupted
            // dispatches both land here; either way nothing is coming.
            self.settle_fire(fire, FIRE_FAILED, "no run was recorded for this fire");
            return;
        };
        let Ok(Some(run)) = store.get_run(&run_id) else {
            self.settle_fire(fire, FIRE_FAILED, "the run for this fire is gone");
            return;
        };
        let Ok(state) = RunState::parse(&run.state) else {
            return;
        };
        if !state.is_terminal() {
            return;
        }
        let Ok(Some(schedule)) = store.get_schedule(&fire.schedule_id) else {
            // The user deleted the schedule while its last job was running.
            // The run's own card (if the thread was folded) still stands; there
            // is no schedule left to write one for.
            self.settle_fire(fire, FIRE_DELIVERED, "the schedule was removed");
            return;
        };
        let reason = match state {
            RunState::Succeeded => ResurfaceReason::Done,
            RunState::Cancelled => {
                // A cancel the user asked for gets a quiet row, not a card —
                // the same rule #15 applies to an ordinary turn.
                self.settle_fire(fire, FIRE_DELIVERED, "cancelled");
                return;
            }
            _ => ResurfaceReason::Failed,
        };
        let summary = match reason {
            ResurfaceReason::Done => run
                .ended_at
                .as_deref()
                .map(|_| "finished".to_string())
                .unwrap_or_else(|| "finished".to_string()),
            _ => run
                .error
                .clone()
                .unwrap_or_else(|| format!("the run ended {}", run.state)),
        };
        let thread_id = fire
            .thread_id
            .clone()
            .unwrap_or_else(|| run.thread_id.clone());
        self.schedule_card(
            &thread_id,
            Some(&run_id),
            reason,
            &schedule,
            &fire.id,
            &summary,
        );
        self.settle_fire(fire, FIRE_DELIVERED, &summary);
    }

    /// One card per fire, in the Inbox.
    ///
    /// A schedule fire on a **folded** thread has already produced a card: #15
    /// resurfaces the thread when its run ends and writes the row itself. Two
    /// cards for one finished job would be noise, so the existing one stands
    /// and this adds nothing — which is also why the check is on `run_id` and
    /// not on the thread.
    fn schedule_card(
        &mut self,
        thread_id: &str,
        run_id: Option<&str>,
        reason: ResurfaceReason,
        schedule: &ScheduleRow,
        fire_id: &str,
        summary: &str,
    ) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if let Some(run_id) = run_id {
            if store.run_has_inbox_event(run_id).unwrap_or(false) {
                return;
            }
        }
        let title = resurface::card_title(&schedule.title, reason);
        let payload = json!({
            "source": "schedule",
            "scheduleId": schedule.id,
            "schedule": schedule.title,
            "fireId": fire_id,
            "botId": schedule.bot_id,
        });
        let kind = resurface::inbox_kind(reason);
        match store.insert_inbox_event(
            thread_id,
            run_id,
            kind,
            &title,
            summary,
            Some(&payload.to_string()),
        ) {
            // Persist, then notify — the order decision #5 insists on. A
            // notification nobody receives loses nothing; a notification
            // without the row would be a card that does not exist.
            Ok(_) => {
                self.notify_inbox_event(thread_id, kind, &title, summary);
            }
            Err(err) => eprintln!("schedule {}: could not write a card: {err}", schedule.id),
        }
    }

    /// Close a fire out. Best effort: the work happened whether or not this
    /// write lands, and losing the note must not make the tick retry the job.
    fn settle_fire(&mut self, fire: &ScheduleFireRow, state: &str, detail: &str) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if let Err(err) = store.set_fire_state(&fire.id, state, Some(detail), true) {
            eprintln!(
                "schedule fire {}: could not record the outcome: {err}",
                fire.id
            );
        }
    }

    // ---- the seam into #15's ledger --------------------------------------

    /// What kind of run the prompt about to be sent on this thread opens.
    ///
    /// `lifecycle_run_started` calls this instead of hard-coding `prompt`. A
    /// schedule fire claims the label for exactly one run on exactly one
    /// thread, so a human typing into another chat in the same instant still
    /// gets an ordinary `prompt` run.
    pub(crate) fn take_run_kind(&mut self, thread_id: &str) -> (&'static str, Option<String>) {
        let claimed = self
            .schedules
            .pending
            .as_ref()
            .is_some_and(|pending| pending.thread_id == thread_id);
        if !claimed {
            return (RUN_KIND_PROMPT, None);
        }
        let pending = self.schedules.pending.take().expect("checked just above");
        (RUN_KIND_SCHEDULE, Some(pending.trigger_json))
    }

    /// Remember the run a schedule label produced.
    ///
    /// Reading it back off the ledger a moment later will not do: a fast agent
    /// can finish the whole turn inside `session_prompt`'s own pump, so by the
    /// time the call returns there is no *open* run to find and the fire would
    /// look like a dispatch that opened nothing.
    pub(crate) fn note_scheduled_run(&mut self, kind: &str, run_id: &str) {
        if kind == RUN_KIND_SCHEDULE {
            self.schedules.claimed_run = Some(run_id.to_string());
        }
    }

    /// Fire a schedule now, outside its cron. The Run now button.
    ///
    /// Deliberately *not* a shortcut into [`Self::rule_on`]: a manual run is
    /// its own occurrence, stamped with the moment the user asked, and it must
    /// not consume or move the schedule's next due time. Pressing it at 8:59
    /// does not cancel 9am.
    pub(crate) fn fire_now(&mut self, schedule: &ScheduleRow) -> Result<ScheduleFireRow, RpcError> {
        let now = chrono::Local::now();
        let store = self.store_or_err()?;
        let new = NewScheduleFire {
            schedule_id: schedule.id.clone(),
            due_at: to_stamp(now),
            state: FIRE_DISPATCHED.to_string(),
            caught_up: false,
            skipped_count: 0,
            detail: Some("run by hand".to_string()),
        };
        let fire = store
            .claim_fire(&new, schedule.next_run_at.as_deref())
            .map_err(|err| RpcError::Internal(err.to_string()))?
            .ok_or_else(|| {
                RpcError::InvalidParams("this schedule already has a run for right now".into())
            })?;
        self.dispatch(schedule, &fire);
        self.store_or_err()?
            .get_fire(&fire.id)
            .map_err(|err| RpcError::Internal(err.to_string()))?
            .ok_or_else(|| RpcError::Internal("the fire vanished as it was recorded".into()))
    }
}

/// What the agent actually reads.
///
/// Named as a schedule on purpose, the same way a handoff is: the bot on the
/// other end is continuing its own standing conversation and has no idea a
/// clock is involved. "This is the 9am job" is the difference between a reply
/// that stands on its own and one that answers a question nobody asked.
fn prompt_text(name: &str, prompt: &str) -> String {
    format!("Scheduled job: {}.\n\n{}\n", name.trim(), prompt.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_says_it_came_from_a_schedule() {
        let text = prompt_text("  Morning triage  ", "  Summarise overnight mail.  ");
        assert!(text.starts_with("Scheduled job: Morning triage."));
        assert!(text.contains("Summarise overnight mail."));
        // No leading or trailing whitespace smuggled through from the record.
        assert!(!text.contains("  Morning"));
    }
}
