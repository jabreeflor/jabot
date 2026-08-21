//! Which pull-request changes are worth an Inbox card, and what it says.
//!
//! The Inbox is the place JaBot is allowed to interrupt someone, so the rule
//! that matters here is **transitions, not states**. A poll runs every fifteen
//! seconds while checks are moving; a PR that has been red since lunch is not
//! news fifteen seconds later. Every decision below compares the row as it was
//! against the row as it is, and a change that did not happen produces nothing.
//!
//! Three events clear that bar (`pr-linkage.md`: "Inbox copy can then be
//! deterministic"):
//!
//! | Event | Why the human is being told |
//! |---|---|
//! | opened | The session produced a pull request. This is the outcome. |
//! | checks failed | Green when the session ended, red now. Nobody is watching. |
//! | changes requested | A reviewer has asked for work. That is a new job. |
//!
//! Deliberately *not* cards: a merge (the user did that, and usually did it in
//! the browser they are looking at), an approval (good news that is not a task
//! — it shows on the row), a PR closed, and checks going green again (the
//! absence of a red card is that news).

use super::super::store::{ThreadPrRow, CHECKS_FAILING, STATUS_MERGED};

/// What happened to a pull request that the Inbox should say out loud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrEvent {
    Opened,
    ChecksFailed,
    ChangesRequested,
}

impl PrEvent {
    /// The `payload_json` discriminator, so a client can tell three `pr` cards
    /// apart without parsing the copy.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::ChecksFailed => "checks_failed",
            Self::ChangesRequested => "changes_requested",
        }
    }
}

/// GitHub's `reviewDecision`, lowercased on the way into the row.
const CHANGES_REQUESTED: &str = "changes_requested";

/// The card for a PR the host has just linked for the first time.
///
/// Separate from [`transition`] because there is no "before" to compare with:
/// the evidence that a session opened a PR is itself the event.
pub fn opened(row: &ThreadPrRow) -> Card {
    Card {
        event: PrEvent::Opened,
        title: format!("PR #{} opened", row.number),
        summary: format!("{} · {}", row.repo, describe(row)),
    }
}

/// What one poll changed, if anything the human needs.
///
/// At most one card per poll per PR, and the order is the order of urgency: a
/// PR that went red *and* had changes requested in the same fifteen seconds is
/// one interruption, about the thing that is on fire.
pub fn transition(before: &ThreadPrRow, after: &ThreadPrRow) -> Option<Card> {
    // A merged PR is finished. Checks that go red on the merge commit, or a
    // review left after the fact, are not work anybody is being asked to do.
    if after.status == STATUS_MERGED {
        return None;
    }
    if after.check_state.as_deref() == Some(CHECKS_FAILING)
        && before.check_state.as_deref() != Some(CHECKS_FAILING)
    {
        return Some(Card {
            event: PrEvent::ChecksFailed,
            title: format!("PR #{} · checks failed", after.number),
            summary: format!("{} · {}", after.repo, failing_summary(after)),
        });
    }
    if after.review_state.as_deref() == Some(CHANGES_REQUESTED)
        && before.review_state.as_deref() != Some(CHANGES_REQUESTED)
    {
        return Some(Card {
            event: PrEvent::ChangesRequested,
            title: format!("PR #{} · changes requested", after.number),
            summary: format!("{} · {}", after.repo, after.title),
        });
    }
    None
}

/// One Inbox card, decided but not yet written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub event: PrEvent,
    pub title: String,
    pub summary: String,
}

/// The card names the checks that are red, because "checks failed" without
/// them sends the user to GitHub to find out what this card already knew.
fn failing_summary(row: &ThreadPrRow) -> String {
    let failing = failing_labels(row);
    match failing.len() {
        0 => row.title.clone(),
        1 => format!("{} failed", failing[0]),
        _ => format!("{} failed", failing.join(", ")),
    }
}

fn failing_labels(row: &ThreadPrRow) -> Vec<String> {
    let parsed: Vec<super::github::CheckView> =
        serde_json::from_str(&row.checks_json).unwrap_or_default();
    parsed
        .into_iter()
        .filter(|check| check.state == CHECKS_FAILING)
        .map(|check| check.label)
        .collect()
}

fn describe(row: &ThreadPrRow) -> String {
    if row.title.is_empty() {
        // The first sighting is a URL and a number; the title arrives with the
        // first poll, which on a machine with no `gh` login never comes.
        row.url.clone()
    } else {
        row.title.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::store::{CHECKS_PASSING, CHECKS_RUNNING, STATUS_OPEN};

    fn row() -> ThreadPrRow {
        ThreadPrRow {
            id: "pr-1".into(),
            thread_id: "t-auth".into(),
            provider: "github".into(),
            forge_host: Some("github.com".into()),
            repo: "jabreeflor/jabot".into(),
            number: 23,
            url: "https://github.com/jabreeflor/jabot/pull/23".into(),
            title: "Migrate auth to sessions".into(),
            status: STATUS_OPEN.into(),
            check_state: Some(CHECKS_PASSING.into()),
            review_state: None,
            head_ref: Some("jabot/t-auth".into()),
            base_ref: Some("main".into()),
            additions: 214,
            deletions: 96,
            changed_files: 3,
            checks_json: "[]".into(),
            pr_updated_at: None,
            detected_via: Some("stdout".into()),
            detected_at: None,
            polled_at: None,
            created_at: "2026-08-21T09:00:00Z".into(),
            updated_at: "2026-08-21T09:00:00Z".into(),
        }
    }

    #[test]
    fn opening_a_pr_is_a_card_that_names_it() {
        let card = opened(&row());
        assert_eq!(card.event, PrEvent::Opened);
        assert_eq!(card.title, "PR #23 opened");
        assert!(card.summary.contains("jabreeflor/jabot"));
        assert!(card.summary.contains("Migrate auth to sessions"));
    }

    /// The property the whole module exists for: a red PR is one card, not one
    /// card every fifteen seconds for the rest of the afternoon.
    #[test]
    fn a_pr_that_is_still_red_is_not_news_again() {
        let green = row();
        let mut red = row();
        red.check_state = Some(CHECKS_FAILING.into());
        red.checks_json = r#"[{"label":"tests","state":"failing"}]"#.into();

        let first = transition(&green, &red).expect("green to red is a card");
        assert_eq!(first.event, PrEvent::ChecksFailed);
        assert_eq!(first.title, "PR #23 · checks failed");
        assert!(first.summary.contains("tests failed"), "{}", first.summary);

        assert_eq!(transition(&red, &red), None, "still red is not new");
        // And going green again is not an interruption either.
        assert_eq!(transition(&red, &green), None);
    }

    #[test]
    fn a_review_asking_for_work_is_a_card_and_an_approval_is_not() {
        let before = row();
        let mut changes = row();
        changes.review_state = Some("changes_requested".into());
        let card = transition(&before, &changes).expect("changes requested is a card");
        assert_eq!(card.event, PrEvent::ChangesRequested);
        assert_eq!(card.title, "PR #23 · changes requested");

        let mut approved = row();
        approved.review_state = Some("approved".into());
        assert_eq!(transition(&before, &approved), None);
        // And a second poll reporting the same request says nothing more.
        assert_eq!(transition(&changes, &changes), None);
    }

    /// Red and reviewed in the same poll is one interruption, about the fire.
    #[test]
    fn one_card_per_poll_and_it_is_the_urgent_one() {
        let before = row();
        let mut both = row();
        both.check_state = Some(CHECKS_FAILING.into());
        both.review_state = Some("changes_requested".into());
        assert_eq!(
            transition(&before, &both).map(|card| card.event),
            Some(PrEvent::ChecksFailed)
        );
    }

    #[test]
    fn a_merged_pr_stops_producing_cards() {
        let mut before = row();
        before.status = STATUS_MERGED.into();
        let mut after = before.clone();
        after.check_state = Some(CHECKS_FAILING.into());
        assert_eq!(transition(&before, &after), None);
    }

    /// Checks that have merely started running are not a failure.
    #[test]
    fn running_checks_are_not_failing_checks() {
        let before = row();
        let mut running = row();
        running.check_state = Some(CHECKS_RUNNING.into());
        assert_eq!(transition(&before, &running), None);
    }
}
