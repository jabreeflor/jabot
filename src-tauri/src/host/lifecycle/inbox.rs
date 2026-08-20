//! The Inbox projection: `inbox_events` and folded threads as cards (#22).
//!
//! Decision #5 fixes the classification and this module does not invent a
//! parallel one. **Still sleeping** is `threads.state = folded` and nothing
//! else — folding writes no card, because the thread row already says it.
//! **Needs you / Done** (and failed, stuck, lost) are `inbox_events` rows, read
//! back with the latest run beside them.
//!
//! What this file adds on top of the raw rows is the *join the host owns*: who
//! the card is from, where the work happened, and what the ledger last said.
//! The renderer could not do that join itself without a second source of truth
//! — it would have to guess a bot's face from a crew list that may not have
//! answered yet, and guess a run's state from the card's own copy.
//!
//! Two things are load-bearing enough to say out loud:
//!
//! **`failed` and `stuck` stay apart.** They are different asks (`resurface.rs`
//! says why) and they arrive here as different `kind`s. Nothing in this module
//! folds one into the other, and `run_state` is what lets a `stuck` card admit
//! the process behind it is still running.
//!
//! **Archived work leaves the Inbox.** `state-machine.md` gives `archived` as
//! "Sidebar hidden / Inbox hidden", and `count_unread_inbox` already refuses to
//! badge it. Listing the card anyway left Archive — one of the two buttons on
//! every resurfaced card — looking like it had done nothing.

use crate::host::protocol::methods::{FoldPolicy, InboxEventView, SleepingThreadView};
use crate::host::store::{BotRow, InboxEventRow, ThreadRow};

use super::state::ThreadState;

/// Whether a card's thread has been closed out. Deleted threads never reach
/// here — the store's own query drops them — so this is about archive.
///
/// A card whose thread row is missing entirely is *not* hidden: it is a row we
/// can no longer explain, and dropping it silently would lose a result. It
/// renders with the deleted state, which is the honest label for it.
pub fn hidden_from_inbox(thread: Option<&ThreadRow>) -> bool {
    thread.is_some_and(|row| super::effective_state(row) == ThreadState::Archived)
}

/// One `inbox_events` row, with everything its card draws.
pub fn event_view(
    row: InboxEventRow,
    thread: Option<&ThreadRow>,
    bot: Option<&BotRow>,
    run_state: Option<String>,
) -> InboxEventView {
    InboxEventView {
        id: row.id,
        thread_id: row.thread_id,
        thread_title: thread.map(|t| t.title.clone()).unwrap_or_default(),
        thread_state: thread
            .map(|t| super::effective_state(t).as_str().to_string())
            .unwrap_or_else(|| ThreadState::Deleted.as_str().to_string()),
        kind: row.kind,
        title: row.title,
        summary: row.summary,
        run_id: row.run_id,
        payload: row
            .payload_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        created_at: row.created_at,
        read_at: row.read_at,
        dismissed_at: row.dismissed_at,
        // The bot is resolved from the *thread*, not from the event: an event
        // has no owner of its own, and a thread that belongs to no bot is a
        // code session, which draws the other avatar.
        bot_id: thread.and_then(|t| t.bot_id.clone()),
        bot_name: bot.map(|b| b.name.clone()),
        bot_color: bot.map(|b| b.color.clone()),
        repo: thread.and_then(|t| t.repo.clone()),
        branch: thread.and_then(|t| t.branch.clone()),
        run_state,
        last_error: thread.and_then(|t| t.last_error.clone()),
        folded_at: thread.and_then(|t| t.folded_at.clone()),
        resurfaced_at: thread.and_then(|t| t.resurfaced_at.clone()),
    }
}

/// One folded thread, as a Still Sleeping row.
pub fn sleeping_view(
    row: &ThreadRow,
    bot: Option<&BotRow>,
    run_state: Option<String>,
    acp_state: &str,
) -> SleepingThreadView {
    SleepingThreadView {
        thread_id: row.id.clone(),
        title: row.title.clone(),
        fold_policy: FoldPolicy::parse(&row.fold_policy),
        folded_at: row.folded_at.clone(),
        run_state,
        acp_state: acp_state.to_string(),
        bot_id: row.bot_id.clone(),
        bot_name: bot.map(|b| b.name.clone()),
        bot_color: bot.map(|b| b.color.clone()),
        repo: row.repo.clone(),
        branch: row.branch.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(state: &str) -> ThreadRow {
        ThreadRow {
            id: "t-1".into(),
            folder_id: Some("f-1".into()),
            bot_id: None,
            harness_id: "claude".into(),
            acp_session_id: None,
            native_session_ref: None,
            cwd: "/tmp".into(),
            runtime_json: "{}".into(),
            title: "Auth migration".into(),
            state: state.into(),
            fold_policy: "default".into(),
            last_stop_reason: None,
            last_error: None,
            preview: None,
            worktree_path: None,
            created_at: "2026-08-20T10:00:00Z".into(),
            updated_at: "2026-08-20T10:00:00Z".into(),
            folded_at: Some("2026-08-20T10:05:00Z".into()),
            resurfaced_at: None,
            archived_at: None,
            deleted_at: None,
            resurfaced_reason: None,
            repo_root: Some("/tmp".into()),
            repo: Some("jabreeflor/jabot".into()),
            forge_host: Some("github.com".into()),
            branch: Some("jabot/t-1".into()),
            host_id: None,
        }
    }

    fn bot() -> BotRow {
        BotRow {
            id: "inboxm".into(),
            name: "Inbox Mgr".into(),
            color: "b-purple".into(),
            instructions: String::new(),
            tools_json: "[]".into(),
            harness_id: "claude".into(),
            is_chief: false,
            template_id: None,
            host_id: None,
            sort_order: 1,
            created_at: "2026-08-20T10:00:00Z".into(),
            updated_at: "2026-08-20T10:00:00Z".into(),
        }
    }

    fn event(kind: &str) -> InboxEventRow {
        InboxEventRow {
            id: format!("e-{kind}"),
            thread_id: "t-1".into(),
            run_id: Some("r-1".into()),
            kind: kind.into(),
            title: "Auth migration failed".into(),
            summary: "the adapter refused".into(),
            payload_json: Some(r#"{"reason":"failed"}"#.into()),
            created_at: "2026-08-20T10:06:00Z".into(),
            read_at: None,
            dismissed_at: None,
        }
    }

    /// Archive is one of the two buttons on a resurfaced card. If the card
    /// survived the click the button would look broken.
    #[test]
    fn archived_work_leaves_the_inbox_and_nothing_else_does() {
        assert!(hidden_from_inbox(Some(&thread("archived"))));
        assert!(!hidden_from_inbox(Some(&thread("resurfaced"))));
        assert!(!hidden_from_inbox(Some(&thread("folded"))));
        assert!(!hidden_from_inbox(Some(&thread("active"))));
        // A card we can no longer explain still gets shown, not swallowed.
        assert!(!hidden_from_inbox(None));
    }

    /// #15 keeps these two reasons apart deliberately; the projection must not
    /// quietly re-merge them on the way to a card.
    #[test]
    fn failed_and_stuck_reach_the_card_as_different_kinds() {
        let failed = event_view(event("failed"), Some(&thread("resurfaced")), None, None);
        let stuck = event_view(
            event("stuck"),
            Some(&thread("resurfaced")),
            None,
            Some("running".into()),
        );

        assert_eq!(failed.kind, "failed");
        assert_eq!(stuck.kind, "stuck");
        // And the difference is legible: a stuck thread's process is still
        // going, which is the fact that makes "wait" a sane answer to it.
        assert_eq!(stuck.run_state.as_deref(), Some("running"));
        assert_eq!(failed.run_state, None);
    }

    #[test]
    fn a_bots_card_carries_the_bot_and_a_code_threads_does_not() {
        let mut owned = thread("resurfaced");
        owned.bot_id = Some("inboxm".into());
        let bot = bot();

        let from_bot = event_view(event("needs_you"), Some(&owned), Some(&bot), None);
        assert_eq!(from_bot.bot_id.as_deref(), Some("inboxm"));
        assert_eq!(from_bot.bot_name.as_deref(), Some("Inbox Mgr"));
        assert_eq!(from_bot.bot_color.as_deref(), Some("b-purple"));

        let from_code = event_view(event("done"), Some(&thread("resurfaced")), None, None);
        assert_eq!(from_code.bot_id, None);
        assert_eq!(from_code.bot_name, None);
        // The code session says where it worked instead.
        assert_eq!(from_code.repo.as_deref(), Some("jabreeflor/jabot"));
        assert_eq!(from_code.branch.as_deref(), Some("jabot/t-1"));
    }

    /// A card whose thread row has gone reports the tombstone rather than
    /// inventing an active thread the user could be sent to.
    #[test]
    fn a_card_without_a_thread_says_so() {
        let orphan = event_view(event("done"), None, None, None);
        assert_eq!(orphan.thread_state, "deleted");
        assert_eq!(orphan.thread_title, "");
        assert_eq!(orphan.bot_id, None);
    }

    #[test]
    fn a_sleeping_row_carries_the_face_asleep_behind_it() {
        let mut owned = thread("folded");
        owned.bot_id = Some("inboxm".into());

        let view = sleeping_view(&owned, Some(&bot()), Some("running".into()), "running");

        assert_eq!(view.thread_id, "t-1");
        assert_eq!(view.bot_name.as_deref(), Some("Inbox Mgr"));
        assert_eq!(view.folded_at.as_deref(), Some("2026-08-20T10:05:00Z"));
        // Folded and still working is the product's whole premise (#5); the row
        // has to be able to say both at once.
        assert_eq!(view.run_state.as_deref(), Some("running"));
        assert_eq!(view.acp_state, "running");
    }
}
