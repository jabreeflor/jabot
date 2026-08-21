//! What happens to an occurrence whose time passed while JaBot was closed.
//!
//! Decision #4 is what makes this a question at all: the host lives inside the
//! Tauri binary, so Cmd-Q, a lid close and a reboot all stop the clock. A
//! schedule that was owed 9am on Tuesday is still owed it on Thursday, and the
//! host has to rule on that before it dispatches anything.
//!
//! **The ruling is: catch up once, never replay.** At most one run comes out of
//! any outage, for the most recent occurrence, and every earlier one is dropped
//! with a count. Firing a week of missed dailies at once on launch is the
//! failure this exists to prevent — it is a stampede of agents against a user
//! who just opened their laptop, each one acting on a day that is over.
//!
//! Three qualifications, each one a real case:
//!
//! - **Too old is not caught up.** An occurrence more than [`STALE_AFTER`] late
//!   is not run at all. A standup summary from three days ago is not worth
//!   producing, and the honest answer is to say it was missed.
//! - **`skip` means skip.** A schedule whose job is worthless late (a nightly
//!   deploy check, a message that says "good morning") can be set to run
//!   nothing it missed; it simply advances to the next future occurrence.
//! - **Nothing is silent.** Either way one `schedule_fires` row records the
//!   decision, carrying how many occurrences were dropped, so "JaBot was shut
//!   for a week" is legible in the UI without seven rows and seven runs.

use chrono::{DateTime, Duration, Local};

use super::cron::CronSpec;

/// How late an occurrence may be and still be worth running. Long enough to
/// cover a lunch, a meeting, or a laptop shut overnight; short enough that
/// yesterday's job does not run today under a name that says "morning".
pub const STALE_AFTER: Duration = Duration::hours(12);

/// How late a fire has to be before it is reported as a catch-up rather than an
/// ordinary tick. Above the poll interval by a wide margin, so a busy host is
/// never accused of having missed something it ran on time.
const LATE_GRACE: Duration = Duration::seconds(60);

/// How many occurrences the scan will walk before it stops counting.
///
/// A per-second schedule and a month-long outage is 2.6 million occurrences,
/// and counting them exactly buys nothing: the decision is the same at 500 as
/// at 2.6 million. The cap keeps a pathological spec from turning a launch into
/// a hang.
const MAX_SCAN: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchUp {
    /// Run the most recent missed occurrence; drop the rest.
    Once,
    /// Run none of them.
    Skip,
}

impl CatchUp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => super::super::store::CATCH_UP_ONCE,
            Self::Skip => super::super::store::CATCH_UP_SKIP,
        }
    }

    /// Unknown values read as `Once`: a row written by a future version should
    /// still run the user's job, and running one late is a smaller mistake than
    /// silently running none.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "skip" => Self::Skip,
            _ => Self::Once,
        }
    }
}

/// What the tick should do about one due schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The occurrence to run, or `None` when this outage produces no run.
    pub fire: Option<DateTime<Local>>,
    /// The occurrence the row is *about*, whether or not it runs. Always the
    /// most recent one that came due — a skipped fire still has to say which
    /// occurrence it declined.
    pub due: DateTime<Local>,
    /// Occurrences dropped in favour of (or alongside) this one.
    pub skipped: i64,
    /// The schedule's next claim on the clock. `None` for a spec with no
    /// future occurrence at all, which parks it.
    pub next: Option<DateTime<Local>>,
    /// True when this occurrence was already in the past when it was ruled on.
    pub caught_up: bool,
    /// One line, for the fire row and the Inbox card.
    pub detail: Option<String>,
}

/// Rule on a schedule whose `due` has arrived. `None` when it has not.
pub fn plan(
    spec: &CronSpec,
    due: DateTime<Local>,
    now: DateTime<Local>,
    policy: CatchUp,
) -> Option<Plan> {
    if due > now {
        return None;
    }
    let (latest, occurrences, next) = collapse(spec, due, now);
    let skipped = i64::from(occurrences - 1);
    let late = now.signed_duration_since(latest);
    let caught_up = skipped > 0 || late >= LATE_GRACE;

    if policy == CatchUp::Skip && caught_up {
        return Some(Plan {
            fire: None,
            due: latest,
            skipped: i64::from(occurrences),
            next,
            caught_up,
            detail: Some(missed_line(occurrences, "this schedule does not catch up")),
        });
    }
    if late > STALE_AFTER {
        return Some(Plan {
            fire: None,
            due: latest,
            skipped: i64::from(occurrences),
            next,
            caught_up,
            detail: Some(missed_line(
                occurrences,
                &format!("the most recent one was {} late", human(late)),
            )),
        });
    }
    Some(Plan {
        fire: Some(latest),
        due: latest,
        skipped,
        next,
        caught_up,
        detail: caught_up.then(|| {
            if skipped > 0 {
                format!(
                    "caught up on the {} run; {} earlier {} skipped",
                    human_ago(late),
                    skipped,
                    if skipped == 1 { "one was" } else { "were" }
                )
            } else {
                format!("caught up on a run that was {} late", human(late))
            }
        }),
    })
}

/// The most recent occurrence at or before `now`, how many came due, and the
/// first one still ahead.
///
/// The newest occurrence is found by walking *backwards* from now, which costs
/// a step per day rather than a step per occurrence — the count is the only
/// part that has to walk forward, and it is the only part that can be capped
/// without changing what runs.
fn collapse(
    spec: &CronSpec,
    due: DateTime<Local>,
    now: DateTime<Local>,
) -> (DateTime<Local>, u32, Option<DateTime<Local>>) {
    let latest = spec
        .prev_at_or_before(now)
        // A `due` the current spec would not produce means the user edited the
        // cron; the occurrence that was owed is still the one to run.
        .filter(|found| *found >= due)
        .unwrap_or(due);
    let next = spec.next_after(now);
    let mut occurrences = 1;
    let mut cursor = due;
    while occurrences < MAX_SCAN {
        match spec.next_after(cursor) {
            Some(candidate) if candidate <= now => {
                occurrences += 1;
                cursor = candidate;
            }
            _ => break,
        }
    }
    (latest, occurrences, next)
}

fn missed_line(occurrences: u32, why: &str) -> String {
    if occurrences == 1 {
        format!("1 run was missed while JaBot was closed; {why}")
    } else {
        format!("{occurrences} runs were missed while JaBot was closed; {why}")
    }
}

fn human(late: Duration) -> String {
    let secs = late.num_seconds().max(0);
    match secs {
        0..=90 => format!("{secs}s"),
        91..=5_400 => format!("{}m", secs / 60),
        5_401..=172_800 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

fn human_ago(late: Duration) -> String {
    format!("{} old", human(late))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, mi, 0)
            .single()
            .expect("a real local time")
    }

    fn daily_nine() -> CronSpec {
        CronSpec::parse("0 9 * * *").unwrap()
    }

    #[test]
    fn a_schedule_that_is_not_due_yet_has_no_plan() {
        let spec = daily_nine();
        assert_eq!(
            plan(
                &spec,
                at(2026, 3, 5, 9, 0),
                at(2026, 3, 4, 18, 0),
                CatchUp::Once
            ),
            None
        );
    }

    #[test]
    fn an_on_time_tick_is_not_a_catch_up() {
        let spec = daily_nine();
        let plan = plan(
            &spec,
            at(2026, 3, 4, 9, 0),
            // The poll landed a couple of seconds after the minute, as it does.
            at(2026, 3, 4, 9, 0) + Duration::seconds(2),
            CatchUp::Once,
        )
        .expect("due");
        assert_eq!(plan.fire, Some(at(2026, 3, 4, 9, 0)));
        assert_eq!(plan.skipped, 0);
        assert!(!plan.caught_up);
        assert_eq!(plan.detail, None);
        assert_eq!(plan.next, Some(at(2026, 3, 5, 9, 0)));
    }

    /// The headline requirement: a week of missed dailies is one run.
    #[test]
    fn a_week_of_missed_dailies_produces_exactly_one_run() {
        let spec = daily_nine();
        // Owed Tuesday 9am; the Mac came back the following Monday at 10.
        let plan = plan(
            &spec,
            at(2026, 3, 3, 9, 0),
            at(2026, 3, 9, 10, 0),
            CatchUp::Once,
        )
        .expect("due");
        // Only the most recent occurrence runs — today's 9am, not Tuesday's.
        assert_eq!(plan.fire, Some(at(2026, 3, 9, 9, 0)));
        assert_eq!(plan.skipped, 6);
        assert!(plan.caught_up);
        assert_eq!(plan.next, Some(at(2026, 3, 10, 9, 0)));
        assert!(plan.detail.unwrap().contains("6 earlier"));
    }

    #[test]
    fn an_occurrence_older_than_the_stale_window_is_not_run_at_all() {
        let spec = daily_nine();
        // Back at 8am, so the most recent 9am is yesterday's — 23 hours late.
        let plan = plan(
            &spec,
            at(2026, 3, 3, 9, 0),
            at(2026, 3, 9, 8, 0),
            CatchUp::Once,
        )
        .expect("due");
        assert_eq!(plan.fire, None);
        assert!(plan.caught_up);
        // Every one of them is accounted for, including the one not run.
        assert_eq!(plan.skipped, 6);
        assert_eq!(plan.next, Some(at(2026, 3, 9, 9, 0)));
        assert!(plan.detail.unwrap().contains("missed"));
    }

    #[test]
    fn skip_runs_nothing_it_missed_but_still_says_how_much() {
        let spec = daily_nine();
        let plan = plan(
            &spec,
            at(2026, 3, 3, 9, 0),
            at(2026, 3, 9, 10, 0),
            CatchUp::Skip,
        )
        .expect("due");
        assert_eq!(plan.fire, None);
        assert_eq!(plan.skipped, 7);
        assert_eq!(plan.next, Some(at(2026, 3, 10, 9, 0)));
        assert!(plan.detail.unwrap().contains("does not catch up"));
    }

    /// `skip` is about occurrences that were *missed*. A tick that lands on
    /// time still runs, or the policy would mean "never run".
    #[test]
    fn skip_still_runs_an_occurrence_that_is_due_right_now() {
        let spec = daily_nine();
        let plan = plan(
            &spec,
            at(2026, 3, 4, 9, 0),
            at(2026, 3, 4, 9, 0) + Duration::seconds(1),
            CatchUp::Skip,
        )
        .expect("due");
        assert_eq!(plan.fire, Some(at(2026, 3, 4, 9, 0)));
    }

    /// A per-second spec and a long outage must not be counted one at a time.
    #[test]
    fn a_pathological_backlog_is_capped_rather_than_walked() {
        let spec = CronSpec::parse("* * * * * *").unwrap();
        let due = at(2026, 3, 1, 0, 0);
        let now = at(2026, 3, 9, 0, 0);
        let plan = plan(&spec, due, now, CatchUp::Once).expect("due");
        assert_eq!(plan.skipped, i64::from(MAX_SCAN - 1));
        // Capped counting must not cost correctness: the run is still the
        // newest occurrence, and the next claim is still ahead of now.
        assert!(plan.fire.expect("a run") <= now);
        assert!(plan.next.expect("a next") > now);
    }

    #[test]
    fn an_unknown_policy_string_still_runs_the_job() {
        assert_eq!(CatchUp::parse("once"), CatchUp::Once);
        assert_eq!(CatchUp::parse("skip"), CatchUp::Skip);
        assert_eq!(CatchUp::parse("whenever"), CatchUp::Once);
        assert_eq!(CatchUp::Skip.as_str(), "skip");
    }
}
