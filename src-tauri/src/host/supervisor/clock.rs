//! Detecting the time the monotonic clock did not count.
//!
//! macOS `Instant` is `mach_absolute_time` and Linux's is `CLOCK_MONOTONIC`;
//! neither ticks while the machine is suspended. That is usually a feature —
//! it is why the stuck backstop does not accuse an agent of going quiet
//! because the user shut the lid — but it also means the supervisor cannot see
//! a sleep at all if it only ever reads one clock.
//!
//! The wall clock does count that time. So the gap between how much wall time
//! passed and how much monotonic time passed *is* the suspend, to within the
//! resolution of the poll. No macOS API is required, which matters because the
//! same code has to run under `cargo test` on Linux.
//!
//! A wall clock that is merely wrong — NTP stepping it, the user changing the
//! date — produces the same signal. That is acceptable: what the host does
//! with a detected sleep is re-probe live adapters and resurface threads that
//! were mid-turn, and doing that after a clock step costs one Inbox card and
//! nothing else. The reverse mistake (missing a real sleep) leaves a dead
//! session reported as live.

use std::time::{Duration, Instant, SystemTime};

/// Default suspend that counts. Short enough to catch a lid closed over lunch,
/// long enough that ordinary scheduler jitter and a slow SQLite write never
/// look like sleep.
pub const DEFAULT_SLEEP_GAP: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct SleepDetector {
    wall: SystemTime,
    mono: Instant,
    threshold: Duration,
}

impl SleepDetector {
    pub fn new(threshold: Duration) -> Self {
        Self {
            wall: SystemTime::now(),
            mono: Instant::now(),
            threshold,
        }
    }

    pub fn threshold(&self) -> Duration {
        self.threshold
    }

    /// Sample both clocks. `Some(gap)` when the wall clock ran ahead of the
    /// monotonic one by more than the threshold — the machine was asleep.
    pub fn observe(&mut self) -> Option<Duration> {
        self.observe_at(SystemTime::now(), Instant::now())
    }

    /// The same, with both clocks injected, so a test can suspend a laptop.
    pub fn observe_at(&mut self, wall: SystemTime, mono: Instant) -> Option<Duration> {
        let wall_elapsed = wall.duration_since(self.wall).unwrap_or_default();
        let mono_elapsed = mono.saturating_duration_since(self.mono);
        self.wall = wall;
        self.mono = mono;
        let gap = wall_elapsed.saturating_sub(mono_elapsed);
        (gap >= self.threshold).then_some(gap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_polling_is_not_a_sleep() {
        let mut detector = SleepDetector::new(Duration::from_secs(30));
        let base_wall = SystemTime::now();
        let base_mono = Instant::now();
        detector.wall = base_wall;
        detector.mono = base_mono;

        // Both clocks advanced by the same second: the host was simply running.
        let gap = detector.observe_at(
            base_wall + Duration::from_secs(1),
            base_mono + Duration::from_secs(1),
        );
        assert_eq!(gap, None);
    }

    #[test]
    fn wall_time_the_monotonic_clock_missed_is_the_sleep() {
        let mut detector = SleepDetector::new(Duration::from_secs(30));
        let base_wall = SystemTime::now();
        let base_mono = Instant::now();
        detector.wall = base_wall;
        detector.mono = base_mono;

        // The lid was shut for an hour: an hour of wall time, and the ~40ms of
        // monotonic time the poll loop actually spent awake around it.
        let gap = detector
            .observe_at(
                base_wall + Duration::from_secs(3600),
                base_mono + Duration::from_millis(40),
            )
            .expect("an hour of unaccounted wall time is a sleep");
        assert!(gap >= Duration::from_secs(3599), "{gap:?}");
    }

    #[test]
    fn a_long_but_awake_stretch_is_not_a_sleep() {
        // The case that separates this from "has it been a while?": the host
        // was busy for an hour and never suspended. Both clocks agree, so
        // nothing is resurfaced and no adapter is re-probed.
        let mut detector = SleepDetector::new(Duration::from_secs(30));
        let base_wall = SystemTime::now();
        let base_mono = Instant::now();
        detector.wall = base_wall;
        detector.mono = base_mono;
        assert_eq!(
            detector.observe_at(
                base_wall + Duration::from_secs(3600),
                base_mono + Duration::from_secs(3600),
            ),
            None
        );
    }

    #[test]
    fn each_observation_measures_from_the_last_one() {
        let mut detector = SleepDetector::new(Duration::from_secs(30));
        let base_wall = SystemTime::now();
        let base_mono = Instant::now();
        detector.wall = base_wall;
        detector.mono = base_mono;

        detector.observe_at(
            base_wall + Duration::from_secs(3600),
            base_mono + Duration::from_millis(40),
        );
        // The next poll must not re-report the same hour. A supervisor that
        // latched the baseline would resurface every live thread as stuck on
        // every tick for as long as the app stayed open.
        assert_eq!(
            detector.observe_at(
                base_wall + Duration::from_secs(3601),
                base_mono + Duration::from_millis(1040),
            ),
            None
        );
    }
}
