//! The session supervisor: keep-alive, resume, crash and sleep recovery (#21).
//!
//! Decision #4 says durability is **resume, not a living PID** — Cmd-Q, a
//! crash, a lid close and a reboot are one event as far as this module is
//! concerned, because all four end with the same thing: a durable ledger, a
//! session receipt, and no process. Everything here follows from that.
//!
//! **Reconcile against the ledger, never against RAM.** The process axis
//! ([`super::lifecycle::process`]) is deliberately not persisted, so after a
//! restart it holds nothing at all. What survives is `runs`, `threads` and
//! `session_receipts`, and [`boot`] rebuilds the world from those alone. A
//! supervisor that trusted its own memory would come up believing every thread
//! it had never heard of was fine.
//!
//! **A pid that stopped is not a session.** [`keepalive`] reaps live adapters
//! rather than waiting for EOF, because an adapter that forked something
//! holding its stdout can die without ever closing the pipe. It also watches
//! for the wall time the monotonic clock did not count ([`clock`]) — the only
//! way to see, without a macOS API, that the machine was asleep.
//!
//! **Resuming the wrong job is worse than not resuming.** [`resume`] checks
//! #15's compatibility fingerprint before it hands a session back: a harness,
//! model, cwd, tool or permission-mode change means the conversation on disk
//! is not the one that would be spawned now, and the honest answer is to say
//! so rather than to continue something else under the same title.

mod boot;
mod clock;
mod keepalive;
mod resume;

use std::time::{Duration, Instant};

use super::harness::catalog;
use super::protocol::error::RpcError;
use super::protocol::methods::{
    BootNoteView, LiveAdapterView, SupervisorStatusResult, ThreadRefParams, ThreadResumeResult,
};
use super::store::now_utc;
use super::HostSession;
use clock::SleepDetector;

pub use clock::DEFAULT_SLEEP_GAP;
pub use resume::ResumeReadiness;

/// How often the keep-alive probe runs. It costs one `waitpid` per live
/// adapter, so this is cheap; the pump calls it far more often than this and
/// the interval is what keeps it from being a busy loop.
const DEFAULT_PROBE_INTERVAL: Duration = Duration::from_millis(1_000);

/// How long an adapter with nothing to do, on a thread nobody is watching,
/// stays warm before it is closed.
///
/// `keep-alive.md`'s idle-eviction section: keep it warm a couple of minutes
/// in case the user reopens, then `session/close` and drop the subprocess —
/// because Buzz never did and pinned a Claude process tree per session for the
/// life of the app. Safe only because resume exists; that is why it lives in
/// this module and not in #15.
const DEFAULT_IDLE_EVICT: Duration = Duration::from_secs(120);

/// Supervisor RAM. Everything durable is in the store; this is what one
/// process knows about its own lifetime.
#[derive(Debug)]
pub struct Supervisor {
    clock: SleepDetector,
    booted_at: String,
    boot_notes: Vec<BootNoteView>,
    last_probe: Instant,
    probe_interval: Duration,
    idle_evict_after: Duration,
    sleeps_observed: u64,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self {
            clock: SleepDetector::new(DEFAULT_SLEEP_GAP),
            booted_at: now_utc(),
            boot_notes: Vec::new(),
            // Deliberately far enough in the past that the first pump probes
            // immediately: an adapter inherited from a previous run of this
            // process does not exist, but a spawn that failed halfway does.
            last_probe: Instant::now() - DEFAULT_PROBE_INTERVAL,
            probe_interval: DEFAULT_PROBE_INTERVAL,
            idle_evict_after: DEFAULT_IDLE_EVICT,
            sleeps_observed: 0,
        }
    }
}

impl Supervisor {
    /// Three env knobs, all of them stand-ins for settings #26 owns. They
    /// exist because the alternative is a test that waits two real minutes for
    /// an idle evict, and a sleep that cannot be exercised at all.
    ///
    /// - `JABOT_SUPERVISOR_PROBE_MS` — keep-alive probe interval.
    /// - `JABOT_IDLE_EVICT_MS` — idle grace; `0` turns eviction off.
    /// - `JABOT_SLEEP_GAP_MS` — unaccounted wall time that counts as a sleep.
    pub fn from_env() -> Self {
        let read = |key: &str| std::env::var(key).ok();
        let probe_interval = millis(read("JABOT_SUPERVISOR_PROBE_MS").as_deref())
            .filter(|d| !d.is_zero())
            .unwrap_or(DEFAULT_PROBE_INTERVAL);
        Self {
            clock: SleepDetector::new(
                millis(read("JABOT_SLEEP_GAP_MS").as_deref())
                    .filter(|d| !d.is_zero())
                    .unwrap_or(DEFAULT_SLEEP_GAP),
            ),
            probe_interval,
            last_probe: Instant::now() - probe_interval,
            // Zero is meaningful here and nowhere else: "never evict" is a
            // real policy for someone who wants every folded thread warm.
            idle_evict_after: millis(read("JABOT_IDLE_EVICT_MS").as_deref())
                .unwrap_or(DEFAULT_IDLE_EVICT),
            ..Self::default()
        }
    }
}

/// Milliseconds, or nothing. Anything that is not a plain integer is *not* a
/// zero: `JABOT_IDLE_EVICT_MS=off` has to fall back to the default rather than
/// silently switch eviction off, because those two readings are opposites.
fn millis(raw: Option<&str>) -> Option<Duration> {
    raw?.trim().parse::<u64>().ok().map(Duration::from_millis)
}

impl HostSession {
    /// Put a thread's ACP session back. See [`resume`] for the recipe.
    pub fn thread_resume(
        &mut self,
        params: ThreadRefParams,
    ) -> Result<ThreadResumeResult, RpcError> {
        self.resume_thread(params)
    }

    /// What the supervisor is holding and what it found at boot.
    pub fn supervisor_status(&mut self) -> Result<SupervisorStatusResult, RpcError> {
        let descriptors = catalog::compiled_in();
        let mut thread_ids: Vec<String> = self.connections.keys().cloned().collect();
        thread_ids.sort();
        let mut live_adapters = Vec::with_capacity(thread_ids.len());
        for thread_id in thread_ids {
            let harness_id = self
                .store
                .as_ref()
                .and_then(|store| store.get_thread(&thread_id).ok().flatten())
                .map(|thread| thread.harness_id)
                .unwrap_or_else(|| "custom".to_string());
            let profile_key = descriptors
                .iter()
                .find(|descriptor| descriptor.id == harness_id)
                .map(|descriptor| descriptor.profile_key(&thread_id))
                .unwrap_or_else(|| format!("{harness_id}:{thread_id}"));
            let pending_permissions = self.pending_permission_count(&thread_id);
            let idle_ms = self.thread_idle_for(&thread_id).as_millis() as u64;
            let acp_state = self.acp_state(&thread_id).as_str().to_string();
            let Some(conn) = self.connections.get(&thread_id) else {
                continue;
            };
            live_adapters.push(LiveAdapterView {
                pid: conn.pid(),
                acp_session_id: conn.session_id.clone(),
                thread_id,
                harness_id,
                acp_state,
                idle_ms,
                pending_permissions,
                profile_key,
            });
        }
        Ok(SupervisorStatusResult {
            host_id: self.identity().host_id.clone(),
            booted_at: self.supervisor.booted_at.clone(),
            live_adapters,
            boot: self.supervisor.boot_notes.clone(),
            idle_evict_after_ms: self.supervisor.idle_evict_after.as_millis() as u64,
            sleep_gap_threshold_ms: self.supervisor.clock.threshold().as_millis() as u64,
            sleeps_observed: self.supervisor.sleeps_observed,
        })
    }

    /// What the boot pass did, for the tests and for `supervisor/status`.
    pub fn boot_notes(&self) -> &[BootNoteView] {
        &self.supervisor.boot_notes
    }

    /// How often the keep-alive probe may run. `ZERO` means every pump — the
    /// only way a test watches a dead adapter be reaped in milliseconds
    /// instead of a second. The setting this stands in for is #26's.
    pub fn set_probe_interval(&mut self, interval: Duration) {
        self.supervisor.probe_interval = interval;
        self.supervisor.last_probe = Instant::now() - interval;
    }

    /// How long an idle adapter on a thread nobody is watching stays warm.
    /// `ZERO` turns eviction off entirely, which is a real user preference and
    /// is also how every other test keeps its adapter.
    pub fn set_idle_evict_after(&mut self, grace: Duration) {
        self.supervisor.idle_evict_after = grace;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_that_is_not_a_number_falls_back_instead_of_meaning_zero() {
        // `off` and `0` are opposites for eviction: one has to mean the
        // default and the other has to mean never. Parsing junk as zero would
        // turn a typo into "keep every adapter forever".
        assert_eq!(millis(Some("250")), Some(Duration::from_millis(250)));
        assert_eq!(millis(Some(" 250 ")), Some(Duration::from_millis(250)));
        assert_eq!(millis(Some("0")), Some(Duration::ZERO));
        assert_eq!(millis(Some("off")), None);
        assert_eq!(millis(Some("")), None);
        assert_eq!(millis(Some("-1")), None);
        assert_eq!(millis(None), None);
    }

    #[test]
    fn the_first_pump_probes_rather_than_waiting_out_the_interval() {
        // An adapter can die between a spawn and the first tick. Starting the
        // clock at "now" would hide that for a whole interval.
        let supervisor = Supervisor::default();
        assert!(supervisor.last_probe.elapsed() >= supervisor.probe_interval);
    }
}
