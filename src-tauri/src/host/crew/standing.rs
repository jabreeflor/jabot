//! A bot's standing thread: the one conversation it always has (#24).
//!
//! Decision #6 splits the crew in two. **Code** owns many folder threads, one
//! per checkout. **Everyone else** has one standing thread that lives as long
//! as the bot does: extra tasks append to it, a long job folds away to the
//! Inbox and comes back to the same place, and its `cwd` is the bot's memory
//! directory rather than a repository — a worker has no repo unless it asks
//! for one through `spawn_code_session`.
//!
//! D-009 left this unbuilt: #17 shipped the crew, the memory directories and
//! `memoryDir` on the wire, and said plainly that nothing yet opened a thread
//! in one. This is that opener, and Chief's `handoff_to_bot` is its first
//! caller — a handoff has to land *somewhere*, and this is the somewhere.
//!
//! Three properties the rest of #24 leans on.
//!
//! **The id is derived, not minted.** `bot-<bot id>` means "one standing
//! thread" is enforced by the primary key rather than by a lookup that can
//! race, and it makes the call idempotent for free: `thread/open` already
//! returns an existing thread instead of starting a second one, so two
//! handoffs arriving together cannot produce two threads.
//!
//! **No worktree, stated rather than inferred.** `use_checkout` is set even
//! though a thread with no folder would not get a tree anyway. The day someone
//! gives a worker a folder, this line is what keeps the promise.
//!
//! **The workspace exists before the thread does.** The memory directory is
//! the `cwd` the adapter is spawned in, and an adapter cannot start in a
//! directory that is not there.

use super::super::protocol::error::RpcError;
use super::super::protocol::methods::{CrewRefParams, ThreadOpenParams, ThreadStateResult};
use super::super::store::BotRow;
use super::super::HostSession;

/// Prefix for the derived thread id. Namespaced so a standing thread cannot
/// collide with a `thread/open` from New Chat, which mints UUIDs.
pub const STANDING_PREFIX: &str = "bot-";

/// The one thread id this bot's standing conversation can ever have.
pub fn thread_id_for(bot_id: &str) -> String {
    format!("{STANDING_PREFIX}{bot_id}")
}

impl HostSession {
    /// Open (or return) a bot's standing thread.
    ///
    /// Idempotent by construction — the id is derived from the bot — so this
    /// is also the "get" for a thread that may not exist yet, which is what
    /// makes it safe to call from a handoff, from the crew grid, and from a
    /// resumed session without three different answers.
    pub fn crew_thread(&mut self, params: CrewRefParams) -> Result<ThreadStateResult, RpcError> {
        let row = self.bot_row(&params.bot_id)?;
        self.open_standing_thread(&row)
    }

    /// The same thing, from a [`BotRow`] the caller already has.
    pub(crate) fn open_standing_thread(
        &mut self,
        bot: &BotRow,
    ) -> Result<ThreadStateResult, RpcError> {
        // Materialise the workspace first: it is the cwd, and #17 makes the
        // files a projection of the record, so this also refreshes the persona
        // the session is about to read.
        self.ensure_memory(bot);
        let cwd = self
            .memory_dir(&bot.id)
            .map(|dir| dir.display().to_string());
        let Some(cwd) = cwd else {
            // An ephemeral host has no data directory, so a worker has nowhere
            // to work. Naming a temp directory instead would give the bot a
            // memory that vanishes, which is worse than saying so.
            return Err(RpcError::Internal(format!(
                "this host has no data directory, so {} has no workspace to run in",
                bot.name
            )));
        };
        self.thread_open(ThreadOpenParams {
            thread_id: Some(thread_id_for(&bot.id)),
            title: bot.name.clone(),
            cwd,
            harness_id: bot.harness_id.clone(),
            runtime: None,
            folder_id: None,
            bot_id: Some(bot.id.clone()),
            fold_policy: None,
            // Never a worktree (decision #6). A worker's thread has no repo to
            // isolate, and this says so out loud rather than relying on the
            // absence of a folder to imply it.
            use_checkout: Some(true),
            base_ref: None,
        })
    }

    pub(crate) fn bot_row(&self, bot_id: &str) -> Result<BotRow, RpcError> {
        let id = bot_id.trim();
        if id.is_empty() {
            return Err(RpcError::InvalidParams("botId is required".into()));
        }
        self.store_or_err()?
            .get_bot(id)
            .map_err(|err| RpcError::Internal(err.to_string()))?
            .ok_or_else(|| RpcError::InvalidParams(format!("no such bot: {id}")))
    }
}
