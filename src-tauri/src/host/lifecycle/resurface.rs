//! When a folded thread comes back, and as what (`session-lifecycle/resurface.md`).
//!
//! The prototype conflated "it failed" with "it went quiet"; they are different
//! asks of the human. A `failed` card wants a retry or a reopen. A `stuck` card
//! wants patience or a cancel, and the process is deliberately still alive
//! behind it. They get distinct [`ResurfaceReason`]s and distinct copy.
//!
//! Detection prefers the ACP signal — idle plus a stop reason — over silence.
//! The idle timeout in [`super::LifecycleState`] is a backstop, not the primary
//! completion signal.

use crate::host::protocol::methods::ResurfaceReason;

/// How a turn ended, before the overlay decides whether anyone should be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    Done,
    Failed,
    /// The user stopped it. Not a celebration and not a failure — no banner.
    Cancelled,
}

/// ACP v1 `StopReason`, plus the native equivalents adapters may pass through.
///
/// `end_turn` is the only success. Everything else that is not an explicit
/// cancel is terminal-with-a-problem: `max_tokens` and `max_turn_requests` ran
/// out of room, `refusal` declined. An unknown or custom `_reason` is treated
/// as failed rather than done — a stop we cannot classify is not evidence the
/// work succeeded, and the raw string is kept on `threads.last_stop_reason` so
/// a human can see what the adapter actually said.
pub fn classify_stop(reason: Option<&str>) -> StopOutcome {
    match reason {
        Some("end_turn") => StopOutcome::Done,
        Some("cancelled") | Some("canceled") => StopOutcome::Cancelled,
        _ => StopOutcome::Failed,
    }
}

impl StopOutcome {
    /// The reason a *folded* thread resurfaces with. `None` means "do not
    /// resurface": a cancel the user asked for gets a quiet row, not a card.
    pub fn resurface_reason(self) -> Option<ResurfaceReason> {
        match self {
            Self::Done => Some(ResurfaceReason::Done),
            Self::Failed => Some(ResurfaceReason::Failed),
            Self::Cancelled => None,
        }
    }
}

/// Card copy. Short, and about the reason rather than the transcript.
pub fn card_title(title: &str, reason: ResurfaceReason) -> String {
    match reason {
        ResurfaceReason::Done => format!("{title} finished"),
        ResurfaceReason::Failed => format!("{title} failed"),
        ResurfaceReason::Stuck => format!("{title} has gone quiet"),
        ResurfaceReason::NeedsYou => format!("{title} needs a call"),
    }
}

/// `inbox_events.kind` for a resurface. The three reasons the schema already
/// names map straight through; `stuck` is its own kind, not a flavour of
/// failed, because the Inbox has to be able to say "still running" on it.
pub fn inbox_kind(reason: ResurfaceReason) -> &'static str {
    match reason {
        ResurfaceReason::Done => "done",
        ResurfaceReason::Failed => "failed",
        ResurfaceReason::Stuck => "stuck",
        ResurfaceReason::NeedsYou => "needs_you",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_turn_is_the_only_success() {
        assert_eq!(classify_stop(Some("end_turn")), StopOutcome::Done);
        for reason in ["max_tokens", "max_turn_requests", "refusal", "_weird"] {
            assert_eq!(classify_stop(Some(reason)), StopOutcome::Failed, "{reason}");
        }
        // A v1 adapter that returns without a stop reason has told us nothing
        // good; guessing "done" would report success we cannot see.
        assert_eq!(classify_stop(None), StopOutcome::Failed);
    }

    #[test]
    fn a_user_cancel_does_not_resurface() {
        assert_eq!(classify_stop(Some("cancelled")), StopOutcome::Cancelled);
        assert_eq!(StopOutcome::Cancelled.resurface_reason(), None);
        assert_eq!(
            StopOutcome::Done.resurface_reason(),
            Some(ResurfaceReason::Done)
        );
    }

    /// Failed and stuck must stay separable all the way to the card.
    #[test]
    fn failed_and_stuck_are_not_the_same_card() {
        assert_ne!(
            inbox_kind(ResurfaceReason::Failed),
            inbox_kind(ResurfaceReason::Stuck)
        );
        assert_eq!(
            card_title("Auth migration", ResurfaceReason::Stuck),
            "Auth migration has gone quiet"
        );
        assert_eq!(
            card_title("Auth migration", ResurfaceReason::Failed),
            "Auth migration failed"
        );
    }
}
