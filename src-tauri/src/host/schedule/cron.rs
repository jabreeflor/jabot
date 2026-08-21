//! A cron spec, and the next time it comes round.
//!
//! Written here rather than pulled in, for two reasons. The dependency list in
//! `Cargo.toml` is deliberately short — this is one screen of arithmetic and a
//! bitmask per field — and none of the crates answer the question this module
//! actually exists for, which is *"what did this schedule owe while the Mac was
//! shut?"*. That needs the occurrences between two instants, not just the next
//! one, and it needs them without walking a month of minutes.
//!
//! **Local time, not UTC.** "Every weekday at 9" has to keep meaning 9am after
//! the clocks change, so matching happens in the machine's local zone. Two
//! places where that is not free, both handled below: a local time that does
//! not exist (the hour spring-forward skips) is not an occurrence, and a local
//! time that happens twice (autumn) fires on the first of the two — the
//! alternative is firing the same 1:30am job twice in one night.
//!
//! **The grammar is the usual one.** Five fields `min hour dom mon dow`, or six
//! with a leading seconds field, or one of the `@daily` shorthands. `*`, `a-b`,
//! `*/n`, `a-b/n` and comma lists, with the classic Vixie rule for the two
//! day fields: when *both* day-of-month and day-of-week are restricted, a day
//! matching **either** is a match, because `0 0 13 * FRI` has to be able to
//! mean "the 13th, and every Friday".

use std::fmt;

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Timelike};

/// How far ahead [`CronSpec::next_after`] will look before giving up.
///
/// Four years covers every leap-day schedule (`0 0 29 2 *`) with a year to
/// spare. A spec that matches nothing inside that window matches nothing.
const MAX_DAYS_AHEAD: i64 = 366 * 4;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CronError {
    #[error("{0}")]
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> CronError {
    CronError::Invalid(message.into())
}

/// One cron field as a set of allowed values, plus whether it was written `*`.
///
/// The `*` flag is not cosmetic: the day-of-month / day-of-week rule turns on
/// it, and a field of `1-31` is not the same statement as `*` even though the
/// two sets are equal.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    allowed: Vec<bool>,
    wildcard: bool,
}

impl Field {
    fn parse(raw: &str, min: u32, max: u32, name: &str) -> Result<Self, CronError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(invalid(format!("the {name} field is empty")));
        }
        let mut allowed = vec![false; (max - min + 1) as usize];
        let wildcard = raw == "*";
        for part in raw.split(',') {
            let (spec, step) = match part.split_once('/') {
                Some((spec, step)) => {
                    let step: u32 = step.parse().map_err(|_| {
                        invalid(format!("{part} is not a step the {name} field takes"))
                    })?;
                    if step == 0 {
                        return Err(invalid(format!("a step of 0 in the {name} field")));
                    }
                    (spec, step)
                }
                None => (part, 1),
            };
            let (from, to) = if spec == "*" {
                (min, max)
            } else if let Some((from, to)) = spec.split_once('-') {
                (value(from, min, max, name)?, value(to, min, max, name)?)
            } else {
                let single = value(spec, min, max, name)?;
                // `5/15` means "from 5, every 15" — a bare value with a step is
                // an open range, not one value.
                if step > 1 {
                    (single, max)
                } else {
                    (single, single)
                }
            };
            if from > to {
                return Err(invalid(format!(
                    "{spec} runs backwards in the {name} field"
                )));
            }
            let mut at = from;
            while at <= to {
                allowed[(at - min) as usize] = true;
                at += step;
            }
        }
        Ok(Self { allowed, wildcard })
    }

    fn contains(&self, value: u32, min: u32) -> bool {
        value
            .checked_sub(min)
            .and_then(|index| self.allowed.get(index as usize).copied())
            .unwrap_or(false)
    }
}

fn value(raw: &str, min: u32, max: u32, name: &str) -> Result<u32, CronError> {
    let parsed: u32 = raw
        .trim()
        .parse()
        .map_err(|_| invalid(format!("{raw} is not a number the {name} field takes")))?;
    // Sunday is both 0 and 7 in every cron anyone has ever written.
    let parsed = if name == "day-of-week" && parsed == 7 {
        0
    } else {
        parsed
    };
    if parsed < min || parsed > max {
        return Err(invalid(format!(
            "{raw} is outside {min}-{max} in the {name} field"
        )));
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSpec {
    raw: String,
    second: Field,
    minute: Field,
    hour: Field,
    day_of_month: Field,
    month: Field,
    day_of_week: Field,
}

impl fmt::Display for CronSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl CronSpec {
    pub fn parse(raw: &str) -> Result<Self, CronError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(invalid("a schedule needs a cron expression"));
        }
        let expanded = match trimmed.to_ascii_lowercase().as_str() {
            "@yearly" | "@annually" => "0 0 1 1 *".to_string(),
            "@monthly" => "0 0 1 * *".to_string(),
            "@weekly" => "0 0 * * 0".to_string(),
            "@daily" | "@midnight" => "0 0 * * *".to_string(),
            "@hourly" => "0 * * * *".to_string(),
            other if other.starts_with('@') => {
                return Err(invalid(format!(
                    "{trimmed} is not a shorthand this host knows"
                )))
            }
            _ => trimmed.to_string(),
        };
        let fields: Vec<&str> = expanded.split_whitespace().collect();
        // Six fields is the Quartz/robfig form with seconds in front. The UI
        // only ever writes five; the sixth exists so a spec that needs to run
        // oftener than once a minute can say so rather than being rounded up.
        let (second, rest) = match fields.len() {
            5 => ("0", &fields[..]),
            6 => (fields[0], &fields[1..]),
            n => {
                return Err(invalid(format!(
                    "a cron expression has 5 fields (or 6 with seconds); this one has {n}"
                )))
            }
        };
        Ok(Self {
            raw: trimmed.to_string(),
            second: Field::parse(second, 0, 59, "second")?,
            minute: Field::parse(rest[0], 0, 59, "minute")?,
            hour: Field::parse(rest[1], 0, 23, "hour")?,
            day_of_month: Field::parse(rest[2], 1, 31, "day-of-month")?,
            month: Field::parse(rest[3], 1, 12, "month")?,
            day_of_week: Field::parse(rest[4], 0, 6, "day-of-week")?,
        })
    }

    /// The classic Vixie day rule. Restricting both day fields is a union, not
    /// an intersection; restricting one is an ordinary match.
    fn matches_date(&self, date: NaiveDate) -> bool {
        if !self.month.contains(date.month(), 1) {
            return false;
        }
        let dom = self.day_of_month.contains(date.day(), 1);
        let dow = self
            .day_of_week
            .contains(date.weekday().num_days_from_sunday(), 0);
        match (self.day_of_month.wildcard, self.day_of_week.wildcard) {
            (true, true) => true,
            (false, true) => dom,
            (true, false) => dow,
            (false, false) => dom || dow,
        }
    }

    /// The first occurrence strictly after `after`.
    ///
    /// `None` only when nothing matches inside [`MAX_DAYS_AHEAD`] — a spec like
    /// `0 0 30 2 *`, which is a date that does not exist.
    pub fn next_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        // Strictly after, and on a whole second: an occurrence is a second, so
        // starting from the same second would return the one we just ran.
        let start = (after + Duration::seconds(1))
            .with_nanosecond(0)
            .unwrap_or(after);
        let mut date = start.date_naive();
        let mut floor = (start.hour(), start.minute(), start.second());
        for _ in 0..MAX_DAYS_AHEAD {
            if self.matches_date(date) {
                if let Some(found) = self.first_time_on(date, floor) {
                    return Some(found);
                }
            }
            date = date.succ_opt()?;
            floor = (0, 0, 0);
        }
        None
    }

    /// The most recent occurrence at or before `before`.
    ///
    /// The mirror of [`Self::next_after`], and the reason it exists is
    /// [`super::catchup`]: collapsing an outage means running the *newest*
    /// missed occurrence, and finding that by walking forward from the oldest
    /// one costs a step per occurrence. Walking backwards from now costs a step
    /// per day, whatever the spec.
    pub fn prev_at_or_before(&self, before: DateTime<Local>) -> Option<DateTime<Local>> {
        let start = before.with_nanosecond(0).unwrap_or(before);
        let mut date = start.date_naive();
        let mut ceiling = (start.hour(), start.minute(), start.second());
        for _ in 0..MAX_DAYS_AHEAD {
            if self.matches_date(date) {
                if let Some(found) = self.last_time_on(date, ceiling) {
                    return Some(found);
                }
            }
            date = date.pred_opt()?;
            ceiling = (23, 59, 59);
        }
        None
    }

    /// The last matching local time on `date` at or before `ceiling`.
    fn last_time_on(&self, date: NaiveDate, ceiling: (u32, u32, u32)) -> Option<DateTime<Local>> {
        for hour in (0..=ceiling.0).rev() {
            if !self.hour.contains(hour, 0) {
                continue;
            }
            let minute_ceiling = if hour == ceiling.0 { ceiling.1 } else { 59 };
            for minute in (0..=minute_ceiling).rev() {
                if !self.minute.contains(minute, 0) {
                    continue;
                }
                let second_ceiling = if hour == ceiling.0 && minute == ceiling.1 {
                    ceiling.2
                } else {
                    59
                };
                for second in (0..=second_ceiling).rev() {
                    if !self.second.contains(second, 0) {
                        continue;
                    }
                    let Some(naive) = date.and_hms_opt(hour, minute, second) else {
                        continue;
                    };
                    match Local.from_local_datetime(&naive) {
                        LocalResult::Single(at) => return Some(at),
                        LocalResult::Ambiguous(first, _) => return Some(first),
                        LocalResult::None => continue,
                    }
                }
            }
        }
        None
    }

    /// The first matching local time on `date` at or after `floor`.
    fn first_time_on(&self, date: NaiveDate, floor: (u32, u32, u32)) -> Option<DateTime<Local>> {
        for hour in floor.0..24 {
            if !self.hour.contains(hour, 0) {
                continue;
            }
            let minute_floor = if hour == floor.0 { floor.1 } else { 0 };
            for minute in minute_floor..60 {
                if !self.minute.contains(minute, 0) {
                    continue;
                }
                let second_floor = if hour == floor.0 && minute == floor.1 {
                    floor.2
                } else {
                    0
                };
                for second in second_floor..60 {
                    if !self.second.contains(second, 0) {
                        continue;
                    }
                    let Some(naive) = date.and_hms_opt(hour, minute, second) else {
                        continue;
                    };
                    match Local.from_local_datetime(&naive) {
                        LocalResult::Single(at) => return Some(at),
                        // Autumn: this local time happens twice. Fire on the
                        // first, so a 1:30am job runs once on the night the
                        // clocks go back rather than twice.
                        LocalResult::Ambiguous(first, _) => return Some(first),
                        // Spring: this local time does not exist. It is not an
                        // occurrence — skipping is the only honest answer, and
                        // the catch-up policy is what covers a daily job whose
                        // 2:30am was skipped once a year.
                        LocalResult::None => continue,
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, mi, s)
            .single()
            .expect("a real local time")
    }

    fn next(spec: &str, from: DateTime<Local>) -> DateTime<Local> {
        CronSpec::parse(spec)
            .expect("valid spec")
            .next_after(from)
            .expect("an occurrence")
    }

    #[test]
    fn a_daily_job_lands_on_the_next_day_once_today_has_passed() {
        let spec = "0 9 * * *";
        // Before 9: today.
        assert_eq!(
            next(spec, at(2026, 3, 4, 8, 59, 59)),
            at(2026, 3, 4, 9, 0, 0)
        );
        // On 9 exactly: strictly after, so tomorrow — otherwise the tick that
        // ran the job would immediately find it due again.
        assert_eq!(next(spec, at(2026, 3, 4, 9, 0, 0)), at(2026, 3, 5, 9, 0, 0));
    }

    #[test]
    fn shorthands_expand_to_the_same_thing_as_writing_them_out() {
        let from = at(2026, 3, 4, 13, 22, 5);
        assert_eq!(next("@daily", from), next("0 0 * * *", from));
        assert_eq!(next("@hourly", from), next("0 * * * *", from));
        assert_eq!(next("@weekly", from), next("0 0 * * 0", from));
        assert!(CronSpec::parse("@fortnightly").is_err());
    }

    #[test]
    fn steps_ranges_and_lists_all_parse() {
        let from = at(2026, 3, 4, 0, 0, 0);
        assert_eq!(next("*/15 * * * *", from), at(2026, 3, 4, 0, 15, 0));
        assert_eq!(next("0 9-17 * * *", from), at(2026, 3, 4, 9, 0, 0));
        assert_eq!(next("0 0,12 * * *", from), at(2026, 3, 4, 12, 0, 0));
        // A bare value with a step is an open range: from 10, every 20.
        assert_eq!(next("10/20 * * * *", from), at(2026, 3, 4, 0, 10, 0));
    }

    #[test]
    fn weekdays_are_matched_by_name_of_number_not_by_position() {
        // 2026-03-04 is a Wednesday; the next Friday is the 6th.
        let from = at(2026, 3, 4, 12, 0, 0);
        assert_eq!(next("0 8 * * 5", from), at(2026, 3, 6, 8, 0, 0));
        // Sunday is 0 and 7 alike.
        assert_eq!(next("0 8 * * 7", from), next("0 8 * * 0", from));
    }

    /// The rule that separates a real cron from a plausible one.
    #[test]
    fn restricting_both_day_fields_is_a_union() {
        // "the 13th, or any Friday" — 2026-03-06 is a Friday, before the 13th.
        let from = at(2026, 3, 4, 12, 0, 0);
        assert_eq!(next("0 0 13 * 5", from), at(2026, 3, 6, 0, 0, 0));
        // With day-of-week wild, only the 13th matches.
        assert_eq!(next("0 0 13 * *", from), at(2026, 3, 13, 0, 0, 0));
    }

    #[test]
    fn a_seconds_field_is_accepted_and_a_five_field_spec_means_second_zero() {
        let from = at(2026, 3, 4, 12, 0, 0);
        assert_eq!(next("*/2 * * * * *", from), at(2026, 3, 4, 12, 0, 2));
        // Five fields never fire off the top of the minute.
        assert_eq!(next("* * * * *", from), at(2026, 3, 4, 12, 1, 0));
    }

    #[test]
    fn a_date_that_never_happens_returns_nothing_rather_than_looping() {
        let spec = CronSpec::parse("0 0 30 2 *").unwrap();
        assert_eq!(spec.next_after(at(2026, 3, 4, 12, 0, 0)), None);
        // …but a leap day is found, four years out at worst.
        let leap = CronSpec::parse("0 0 29 2 *").unwrap();
        assert!(leap.next_after(at(2026, 3, 4, 12, 0, 0)).is_some());
    }

    #[test]
    fn nonsense_is_refused_with_a_reason_a_user_can_act_on() {
        for bad in [
            "",
            "* * *",
            "* * * * * * *",
            "61 * * * *",
            "* 25 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "10-5 * * * *",
            "*/0 * * * *",
            "abc * * * *",
        ] {
            assert!(CronSpec::parse(bad).is_err(), "{bad} should not parse");
        }
        let err = CronSpec::parse("61 * * * *").unwrap_err();
        assert!(err.to_string().contains("minute"), "{err}");
    }

    #[test]
    fn walking_backwards_finds_the_same_occurrences_as_walking_forwards() {
        let spec = CronSpec::parse("0 9 * * 1-5").unwrap();
        // 2026-03-07 is a Saturday, so the most recent weekday 9am is Friday's.
        assert_eq!(
            spec.prev_at_or_before(at(2026, 3, 7, 12, 0, 0)),
            Some(at(2026, 3, 6, 9, 0, 0))
        );
        // At the occurrence itself, "at or before" includes it — the forward
        // walk is strict and this one is not, which is what makes a fire that
        // is due *now* findable from now.
        assert_eq!(
            spec.prev_at_or_before(at(2026, 3, 6, 9, 0, 0)),
            Some(at(2026, 3, 6, 9, 0, 0))
        );
        let daily = CronSpec::parse("0 9 * * *").unwrap();
        let noon = at(2026, 3, 4, 12, 0, 0);
        let previous = daily.prev_at_or_before(noon).unwrap();
        assert_eq!(daily.next_after(previous), Some(at(2026, 3, 5, 9, 0, 0)));
    }

    #[test]
    fn the_raw_text_survives_a_round_trip_for_display() {
        let spec = CronSpec::parse(" 0 9 * * 1-5 ").unwrap();
        assert_eq!(spec.to_string(), "0 9 * * 1-5");
    }
}
