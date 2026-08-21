//! The run ledger (decision #5).
//!
//! A thread is a conversation; a run is one turn of work inside it. One thread
//! has many sequential runs on the same ACP session — another prompt, a
//! schedule fire, a Chief re-dispatch — which is why "what is this thread
//! doing" is a question about its latest run, not about the thread row.
//!
//! `needs_you` is the one non-terminal stop: the run has stopped producing but
//! the turn is not over, and answering the permission puts it back to
//! `running`. Treating it as terminal would make an answered prompt look like a
//! finished job.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Lost,
    NeedsYou,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    #[error("run cannot go from {from} to {to}")]
    Illegal { from: RunState, to: RunState },
    #[error("unknown run state {0}")]
    Unknown(String),
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Lost => "lost",
            Self::NeedsYou => "needs_you",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, LedgerError> {
        match raw {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "timed_out" => Ok(Self::TimedOut),
            "lost" => Ok(Self::Lost),
            "needs_you" => Ok(Self::NeedsYou),
            other => Err(LedgerError::Unknown(other.to_string())),
        }
    }

    /// Terminal states have no outbound edges. `needs_you` is not one of them.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut | Self::Lost
        )
    }

    /// Is this run still the thread's live work?
    pub fn is_open(self) -> bool {
        !self.is_terminal()
    }
}

impl fmt::Display for RunState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn advance(from: RunState, to: RunState) -> Result<RunState, LedgerError> {
    use RunState::*;
    let legal = match (from, to) {
        // Queued work can start, be cancelled before it starts, or be lost
        // with the host that was going to dispatch it.
        (Queued, Running | Cancelled | Lost) => true,
        (Running, Succeeded | Failed | Cancelled | TimedOut | Lost | NeedsYou) => true,
        // Answered, or abandoned while blocked.
        (NeedsYou, Running | Cancelled | Failed | TimedOut | Lost) => true,
        _ => false,
    };
    if legal {
        Ok(to)
    } else {
        Err(LedgerError::Illegal { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::RunState::*;
    use super::*;

    #[test]
    fn a_run_starts_queued_and_ends_once() {
        assert_eq!(advance(Queued, Running), Ok(Running));
        assert_eq!(advance(Running, Succeeded), Ok(Succeeded));
        for terminal in [Succeeded, Failed, Cancelled, TimedOut, Lost] {
            assert!(terminal.is_terminal(), "{terminal}");
            assert!(
                advance(terminal, Running).is_err(),
                "{terminal} must have no outbound edges"
            );
        }
    }

    #[test]
    fn needs_you_is_a_pause_not_an_end() {
        assert!(!NeedsYou.is_terminal());
        // The permission gets answered and the same run carries on — this is
        // the edge that makes needs_you a pause rather than a sixth ending.
        assert_eq!(advance(Running, NeedsYou), Ok(NeedsYou));
        assert_eq!(advance(NeedsYou, Running), Ok(Running));
        assert_eq!(advance(NeedsYou, Cancelled), Ok(Cancelled));
        // But it cannot succeed straight out of being blocked; the agent has
        // to run again first.
        assert!(advance(NeedsYou, Succeeded).is_err());
    }

    #[test]
    fn queued_work_cannot_skip_running() {
        assert!(advance(Queued, Succeeded).is_err());
        assert!(advance(Queued, NeedsYou).is_err());
        assert_eq!(advance(Queued, Lost), Ok(Lost));
    }

    #[test]
    fn state_strings_round_trip_the_store_column() {
        for state in [
            Queued, Running, Succeeded, Failed, Cancelled, TimedOut, Lost, NeedsYou,
        ] {
            assert_eq!(RunState::parse(state.as_str()), Ok(state));
        }
        assert!(matches!(
            RunState::parse("done"),
            Err(LedgerError::Unknown(_))
        ));
    }
}
