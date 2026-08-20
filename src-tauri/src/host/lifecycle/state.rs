//! The thread overlay state machine (#5, `session-lifecycle/state-machine.md`).
//!
//! This is the *UI lifecycle* only. The process layer — connected/dead crossed
//! with ACP running/idle/requires_action — is deliberately a separate enum in
//! [`super::process`]: a folded thread that is still running is the entire
//! feature, and collapsing the two axes into one enum is what makes that
//! unrepresentable.
//!
//! Every edge here is from the transition table in the research. A move that is
//! not in the table is an error, never a silent no-op — a fold that quietly did
//! nothing would leave the sidebar and the store disagreeing about whether the
//! user's work disappeared.

use std::fmt;

use crate::host::protocol::methods::ResurfaceReason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Active,
    Folded,
    Resurfaced,
    Archived,
    /// Tombstone. `threads.deleted_at` carries it, not `threads.state`, so a
    /// late adapter event still has a row to land on.
    Deleted,
}

/// What the user (or the supervisor) is trying to do to a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadAction {
    /// "Disappear until done" — keeps the current fold policy.
    Fold,
    /// Fold with `fold_policy = wait_for_inbox`. Same state, quieter policy.
    WaitForInbox,
    /// Open a sleeping or resurfaced row (or restore an archived one).
    Reopen,
    Archive,
    Delete,
    /// Supervisor-driven: the thread came back on its own.
    Resurface(ResurfaceReason),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("cannot {action} a {from} thread")]
    Illegal {
        from: &'static str,
        action: &'static str,
    },
    #[error("thread is deleted")]
    Deleted,
    #[error("unknown thread state {0}")]
    Unknown(String),
}

impl ThreadState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Folded => "folded",
            Self::Resurfaced => "resurfaced",
            Self::Archived => "archived",
            Self::Deleted => "deleted",
        }
    }

    /// Parse `threads.state`. `deleted_at` is checked by the caller, because
    /// the column never holds `deleted` — see [`ThreadState::Deleted`].
    pub fn parse(raw: &str) -> Result<Self, TransitionError> {
        match raw {
            "active" => Ok(Self::Active),
            "folded" => Ok(Self::Folded),
            "resurfaced" => Ok(Self::Resurfaced),
            "archived" => Ok(Self::Archived),
            "deleted" => Ok(Self::Deleted),
            other => Err(TransitionError::Unknown(other.to_string())),
        }
    }
}

impl fmt::Display for ThreadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ThreadAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fold => "fold",
            Self::WaitForInbox => "wait for inbox",
            Self::Reopen => "reopen",
            Self::Archive => "archive",
            Self::Delete => "delete",
            Self::Resurface(_) => "resurface",
        }
    }
}

/// Ranking used when a thread that is already resurfaced comes back again
/// *while the run is still going*.
///
/// This is about how loud the notification is, not about what happened.
/// `stuck → needs_you` is a real upgrade (the agent stopped being merely quiet
/// and now wants an answer) and `resurface.md` says the notification replaces
/// the old one. The reverse is not: nothing that arrives later should downgrade
/// a card the user is being asked to act on — unless it is an outcome, see
/// [`is_outcome`].
fn urgency(reason: ResurfaceReason) -> u8 {
    match reason {
        ResurfaceReason::Done => 0,
        ResurfaceReason::Stuck => 1,
        ResurfaceReason::Failed => 2,
        ResurfaceReason::NeedsYou => 3,
    }
}

/// Does this reason report a *closed run* — work that actually ended?
///
/// An outcome is exempt from the urgency ranking, because the Inbox is a
/// projection of the ledger and the two are not allowed to disagree.
/// `resurface.md`'s mapping table sends `idle + end_turn` to **Done**, so a
/// folded thread that went quiet, resurfaced `stuck`, and then finished has to
/// stop reading "has gone quiet" — otherwise finished work sits under Needs
/// you forever with no path back. The rule the loudness ranking encodes is
/// about replacing a live notification, not about refusing to update the card
/// once the answer is known.
fn is_outcome(reason: ResurfaceReason) -> bool {
    matches!(reason, ResurfaceReason::Done | ResurfaceReason::Failed)
}

/// The transition table. `current_reason` is only consulted for a re-resurface.
pub fn next_state(
    from: ThreadState,
    action: ThreadAction,
    current_reason: Option<ResurfaceReason>,
) -> Result<ThreadState, TransitionError> {
    use ThreadAction::*;
    use ThreadState::*;

    let illegal = || {
        Err(TransitionError::Illegal {
            from: from.as_str(),
            action: action.as_str(),
        })
    };

    if from == Deleted {
        return Err(TransitionError::Deleted);
    }
    if action == Delete {
        return Ok(Deleted);
    }

    match (from, action) {
        (Active, Fold | WaitForInbox) => Ok(Folded),
        (Active, Archive) => Ok(Archived),
        // Reopening a thread that is already on screen is a UI focus, not a
        // state change; treating it as legal would hide a caller's confusion.
        (Active, _) => illegal(),

        (Folded, Reopen) => Ok(Active),
        // Explicit Archive on a sleeping row is the user giving up. Allowed;
        // an *automatic* folded → archived is not, and nothing emits it.
        (Folded, Archive) => Ok(Archived),
        (Folded, Resurface(_)) => Ok(Resurfaced),
        (Folded, _) => illegal(),

        (Resurfaced, Reopen) => Ok(Active),
        (Resurfaced, Archive) => Ok(Archived),
        // You cannot re-sleep a card that already came back: fold again from
        // `active` after reopening it.
        (Resurfaced, Resurface(next)) => match current_reason {
            // The card already says this. A second identical resurface has
            // nothing new to tell anyone and would only stack a duplicate row.
            Some(current) if current == next => illegal(),
            // The run ended. Say what happened, even over a louder card.
            _ if is_outcome(next) => Ok(Resurfaced),
            Some(current) if urgency(next) <= urgency(current) => illegal(),
            _ => Ok(Resurfaced),
        },
        (Resurfaced, _) => illegal(),

        // Restore is in the table but has no MVP UI; the edge exists so #21 can
        // resume an archived thread without inventing a state.
        (Archived, Reopen) => Ok(Active),
        (Archived, _) => illegal(),

        (Deleted, _) => Err(TransitionError::Deleted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ThreadAction::*;
    use ThreadState::*;

    fn ok(from: ThreadState, action: ThreadAction) -> ThreadState {
        next_state(from, action, None).expect("legal transition")
    }

    #[test]
    fn fold_reopen_resurface_archive_are_the_legal_spine() {
        assert_eq!(ok(Active, Fold), Folded);
        assert_eq!(ok(Active, WaitForInbox), Folded);
        assert_eq!(ok(Active, Archive), Archived);
        assert_eq!(ok(Folded, Reopen), Active);
        assert_eq!(ok(Folded, Archive), Archived);
        assert_eq!(ok(Folded, Resurface(ResurfaceReason::Done)), Resurfaced);
        assert_eq!(ok(Resurfaced, Reopen), Active);
        assert_eq!(ok(Resurfaced, Archive), Archived);
        assert_eq!(ok(Archived, Reopen), Active);
    }

    #[test]
    fn every_non_deleted_state_can_be_deleted() {
        for state in [Active, Folded, Resurfaced, Archived] {
            assert_eq!(ok(state, Delete), Deleted, "{state}");
        }
    }

    /// The four the research names explicitly as "illegal / do not invent".
    #[test]
    fn named_illegal_edges_are_errors_not_no_ops() {
        assert!(matches!(
            next_state(Resurfaced, Fold, Some(ResurfaceReason::Done)),
            Err(TransitionError::Illegal { .. })
        ));
        assert!(matches!(
            next_state(Archived, Fold, None),
            Err(TransitionError::Illegal { .. })
        ));
        assert!(matches!(
            next_state(Active, Resurface(ResurfaceReason::Done), None),
            Err(TransitionError::Illegal { .. })
        ));
        for action in [Fold, Reopen, Archive, Delete] {
            assert_eq!(
                next_state(Deleted, action, None),
                Err(TransitionError::Deleted),
                "deleted has no outbound edges"
            );
        }
    }

    #[test]
    fn a_second_resurface_only_lands_if_it_is_more_urgent() {
        // Quiet-then-blocked: the card upgrades and the notification replaces.
        assert_eq!(
            next_state(
                Resurfaced,
                Resurface(ResurfaceReason::NeedsYou),
                Some(ResurfaceReason::Stuck)
            ),
            Ok(Resurfaced)
        );
        // Blocked-then-quiet must not downgrade an ask the user still owes.
        assert!(next_state(
            Resurfaced,
            Resurface(ResurfaceReason::Stuck),
            Some(ResurfaceReason::NeedsYou)
        )
        .is_err());
        assert!(next_state(
            Resurfaced,
            Resurface(ResurfaceReason::Done),
            Some(ResurfaceReason::Done)
        )
        .is_err());
    }

    /// The loudness ranking must not outlive the run it was ranking. A thread
    /// that went quiet and then finished is Done — `resurface.md`'s own mapping
    /// table says so — and leaving it under "has gone quiet" strands finished
    /// work in the Needs you tab.
    #[test]
    fn an_outcome_replaces_the_card_that_was_only_a_status() {
        for current in [ResurfaceReason::Stuck, ResurfaceReason::NeedsYou] {
            for outcome in [ResurfaceReason::Done, ResurfaceReason::Failed] {
                assert_eq!(
                    next_state(Resurfaced, Resurface(outcome), Some(current)),
                    Ok(Resurfaced),
                    "{current:?} → {outcome:?}"
                );
            }
        }
        // A finished run that finishes again is still nothing new, and going
        // quiet is not an outcome: it cannot displace an ask the user owes.
        assert!(next_state(
            Resurfaced,
            Resurface(ResurfaceReason::Failed),
            Some(ResurfaceReason::Failed)
        )
        .is_err());
        assert!(next_state(
            Resurfaced,
            Resurface(ResurfaceReason::Stuck),
            Some(ResurfaceReason::NeedsYou)
        )
        .is_err());
    }

    #[test]
    fn state_strings_round_trip_the_store_column() {
        for state in [Active, Folded, Resurfaced, Archived, Deleted] {
            assert_eq!(ThreadState::parse(state.as_str()), Ok(state));
        }
        assert!(matches!(
            ThreadState::parse("sleeping"),
            Err(TransitionError::Unknown(_))
        ));
    }
}
