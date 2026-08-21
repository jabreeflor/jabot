//! Schedules: recurring jobs that fire into the Inbox (#25).
//!
//! Decision #4 keeps the host inside the Tauri binary — no launchd, no daemon
//! — so the cron here is in-process and runs on the same pump everything else
//! rides. That single fact is what makes this module mostly about *absence*:
//! the interesting code is not "run the job", it is what the host owes a
//! schedule whose 9am happened while the Mac was shut. [`catchup`] rules on
//! that; the rule is **catch up once, never replay**.
//!
//! ```text
//! pump_acp ──► schedule_tick ──┬─► deliver_finished_fires   (runs that ended → Inbox)
//!                              └─► dispatch_due_schedules   (occurrences owed → runs)
//! ```
//!
//! **A fire runs as a bot, on that bot's standing thread.** Decision #6 gives
//! every non-Code crew member exactly one conversation, and #24 built the
//! opener. A schedule reuses it rather than inventing a second kind of thread:
//! the persona, the tool allowlist, the memory directory and the ACP session
//! are the bot's, so a scheduled job is the same agent the user talks to,
//! doing a thing on a timer. There is no second path, and there is no
//! schedule-owned thread.
//!
//! **A fire is a run, and a run's result is an Inbox card.** #15's ledger
//! already accepts `kind = 'schedule'`, so a fire is not a new concept in the
//! store — it is a row in `runs` with a trigger that says which schedule and
//! which occurrence. When that run reaches a terminal state, [`deliver`] turns
//! it into the `inbox_event` decision #5 asks for, which is what makes the
//! Inbox "cards the human should see" rather than "chats the human folded".
//!
//! **The clock is durable and the ledger decides, not RAM.** `next_due_at` is a
//! column, and every occurrence is claimed with a `UNIQUE (schedule_id,
//! due_at)` insert in the same transaction that advances it. Two ticks, a boot
//! pass overlapping a live tick, and a user pressing Run now on a schedule that
//! is already due all converge on one row and one run.

mod api;
mod catchup;
mod cron;
mod fire;

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, SecondsFormat, Utc};

pub use catchup::{CatchUp, STALE_AFTER};
pub use cron::{CronError, CronSpec};

/// How often the tick runs. Cron granularity is a second at finest, so a
/// one-second poll can never be late by more than one occurrence; the interval
/// exists to keep a 50ms pump from doing a SQLite query twenty times a second.
const DEFAULT_TICK: Duration = Duration::from_millis(1_000);

/// `runs.kind` for a schedule fire. The schema's check constraint has accepted
/// it since 0001; this is the first thing to write it.
pub const RUN_KIND_SCHEDULE: &str = "schedule";

/// The run kind a prompt gets when nothing has claimed it.
pub const RUN_KIND_PROMPT: &str = "prompt";

/// A dispatch in flight: the next run opened on this thread belongs to a
/// schedule, and this is what says so.
///
/// RAM, and only for the width of one `session/prompt` call — the durable
/// record is the `schedule_fires` row, which is written first. Keeping it
/// keyed by thread means a fire that somehow fails to start cannot mislabel
/// a human's prompt on a different thread.
#[derive(Debug, Clone)]
struct PendingRun {
    thread_id: String,
    trigger_json: String,
}

/// Supervisor RAM for the cron. Everything durable is in `schedules` and
/// `schedule_fires`.
#[derive(Debug)]
pub struct ScheduleState {
    last_tick: Instant,
    tick_interval: Duration,
    pending: Option<PendingRun>,
    /// True while a fire is being dispatched. Prompting pumps, and the pump
    /// ticks the cron — the guard is what keeps a fire's own `session/prompt`
    /// from re-entering the delivery pass and ruling on the half-written row
    /// that started it. Chief's `chief_dispatching` exists for the same
    /// reason and for the same reentrancy (#24).
    dispatching: bool,
    /// The run the last claimed label produced. Written by #15 as the run
    /// opens, read by the dispatch that set the label — see
    /// [`super::HostSession::note_scheduled_run`].
    claimed_run: Option<String>,
}

impl Default for ScheduleState {
    fn default() -> Self {
        Self {
            // In the past, so the first pump after a launch rules on the
            // backlog immediately rather than after a tick of silence.
            last_tick: Instant::now() - DEFAULT_TICK,
            tick_interval: DEFAULT_TICK,
            pending: None,
            dispatching: false,
            claimed_run: None,
        }
    }
}

impl ScheduleState {
    /// `JABOT_SCHEDULE_TICK_MS` — how often the cron is polled. A stand-in for
    /// the setting #26 owns, and the only way a test watches a fire land in
    /// milliseconds instead of a second.
    pub fn from_env() -> Self {
        let interval = std::env::var("JABOT_SCHEDULE_TICK_MS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(Duration::from_millis)
            .filter(|d| !d.is_zero())
            .unwrap_or(DEFAULT_TICK);
        Self {
            last_tick: Instant::now() - interval,
            tick_interval: interval,
            pending: None,
            dispatching: false,
            claimed_run: None,
        }
    }
}

/// Store timestamps are RFC3339 in UTC (`store::now_utc`); cron is evaluated in
/// local time. These two functions are the whole of the conversion, and they
/// are here so no caller has to remember which side of the wire it is on.
pub(crate) fn to_stamp(at: DateTime<Local>) -> String {
    at.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn from_stamp(raw: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|at| at.with_timezone(&Local))
}

impl super::HostSession {
    /// How often the cron is polled. `ZERO` means every pump — the only way a
    /// test watches a fire land in milliseconds instead of a second. The
    /// setting this stands in for is #26's, the same as the supervisor's.
    pub fn set_schedule_tick(&mut self, interval: Duration) {
        self.schedules.tick_interval = interval;
        self.schedules.last_tick = Instant::now() - interval;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_survives_the_round_trip_through_the_store() {
        let now = Local::now();
        let stamp = to_stamp(now);
        // Milliseconds is what `store::now_utc` writes, so that is the
        // resolution a round trip can promise.
        let back = from_stamp(&stamp).expect("parses");
        assert!(
            (back.timestamp_millis() - now.timestamp_millis()).abs() <= 1,
            "{back} != {now}"
        );
        assert!(stamp.ends_with('Z'), "stored UTC, not local: {stamp}");
    }

    #[test]
    fn a_stamp_the_store_could_not_have_written_is_not_guessed_at() {
        assert!(from_stamp("").is_none());
        assert!(from_stamp("tomorrow").is_none());
    }
}
