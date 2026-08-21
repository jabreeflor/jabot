//! The process layer: connected/dead crossed with the harness's own ACP state.
//!
//! Kept orthogonal to [`super::state::ThreadState`] on purpose. ACP already has
//! a process state and JaBot does not duplicate it; the overlay is a *UI*
//! lifecycle laid on top. The supervisor's whole job lives in the product of
//! the two axes — `folded × running` is "disappeared and still working",
//! `folded × requires_action` is "must still deliver".
//!
//! This is supervisor RAM, reconciled on boot, not a durable enum (#5). After a
//! cold start every thread is [`AcpState::Unknown`] until a resume says
//! otherwise.

use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpState {
    Running,
    Idle,
    RequiresAction,
    Unknown,
}

impl AcpState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Idle => "idle",
            Self::RequiresAction => "requires_action",
            Self::Unknown => "unknown",
        }
    }

    /// `state_update` (ACP v2) reports this directly; v1 adapters never send it
    /// and the host infers the same values from prompt traffic instead.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "running" | "working" => Self::Running,
            "idle" => Self::Idle,
            "requires_action" | "requiresAction" => Self::RequiresAction,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessStatus {
    pub connected: bool,
    pub acp: AcpState,
    /// Last time the adapter said anything at all. The stuck backstop measures
    /// silence from here, so any streamed chunk of a long tool call resets it.
    pub last_activity: Instant,
    /// The run this process is currently working through, if any.
    pub run_id: Option<String>,
    /// Set once per silence so the backstop resurfaces a thread once, not every
    /// tick for as long as it stays quiet.
    pub stuck_reported: bool,
}

impl Default for ProcessStatus {
    fn default() -> Self {
        Self {
            connected: false,
            acp: AcpState::Unknown,
            last_activity: Instant::now(),
            run_id: None,
            stuck_reported: false,
        }
    }
}

impl ProcessStatus {
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
        self.stuck_reported = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_state_parses_what_adapters_actually_send() {
        assert_eq!(AcpState::parse("idle"), AcpState::Idle);
        assert_eq!(AcpState::parse("requiresAction"), AcpState::RequiresAction);
        assert_eq!(AcpState::parse("requires_action"), AcpState::RequiresAction);
        // A v1 adapter that sends nothing we recognise must not be reported as
        // idle — unknown is the honest answer and it blocks the done path.
        assert_eq!(AcpState::parse("thinking-really-hard"), AcpState::Unknown);
    }

    #[test]
    fn activity_clears_a_stuck_report() {
        let mut status = ProcessStatus {
            stuck_reported: true,
            ..ProcessStatus::default()
        };
        status.touch();
        assert!(!status.stuck_reported);
    }
}
