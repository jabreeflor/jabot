//! Host-owned SQLite store (WAL, single writer) and secrets vault glue.
//!
//! The renderer never opens this file. The host process is the only writer
//! (`docs/research/data-and-persistence/store.md`).

mod catalog;
mod error;
mod handoff;
mod migrate;
mod models;
mod overlay;
mod pairing;
mod permission;
mod pr;
mod schedule;
mod secrets;
mod seed;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Row};

pub use error::StoreError;
/// `handoffs.kind`, so #24's two dispatch tools and the SQL check constraint
/// cannot spell the same two words differently.
pub use handoff::{KIND_CODE_SESSION as HANDOFF_CODE_SESSION, KIND_HANDOFF};
/// The schema version a freshly opened store lands on (`store::migrate`).
/// Exported so callers assert against the migrations that exist rather than
/// against a number copied into a test.
pub use migrate::head as schema_head;
pub use models::*;
/// `permission_requests.state`, so the broker and the SQL cannot spell the
/// same three words differently (#20).
pub use permission::{
    ANSWERED as ASK_ANSWERED, CANCELLED as ASK_CANCELLED, PENDING as ASK_PENDING,
};
/// `thread_prs.status`, `.check_state` and `.detected_via` (#28), so the poll,
/// the wire and the SQL cannot spell the same words differently.
pub use pr::{
    CHECKS_FAILING, CHECKS_PASSING, CHECKS_RUNNING, STATUS_CLOSED, STATUS_DRAFT, STATUS_MERGED,
    STATUS_OPEN, VIA_GH_PR_VIEW, VIA_HEAD_LIST, VIA_STDOUT,
};
/// `schedules.catch_up` and `schedule_fires.state` (#25), so the tick, the
/// wire and the SQL check constraints cannot spell the same words differently.
pub use schedule::{
    CATCH_UP_ONCE, CATCH_UP_SKIP, FIRE_DELIVERED, FIRE_DISPATCHED, FIRE_FAILED, FIRE_SKIPPED,
};
pub use secrets::{Secrets, SecretsBackend};

const MIN_SQLITE: (u32, u32, u32) = (3, 51, 3);
const UNCLEAN_SUFFIX: &str = ".unclean";
/// `secret_refs.kind` for an OAuth token bundle. One row per provider grant.
const TOOL_GRANT_KIND: &str = "mcp_oauth";

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(&path)?;
        apply_pragmas(&conn)?;
        verify_sqlite_version()?;
        let marker = unclean_marker(&path);
        if marker.exists() {
            integrity_check(&conn)?;
        }
        std::fs::write(&marker, b"open")?;
        migrate::migrate(&mut conn)?;
        seed::seed(&conn)?;
        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i32, StoreError> {
        migrate::schema_version(&self.conn)
    }

    pub fn journal_mode(&self) -> Result<String, StoreError> {
        let mode: String = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        Ok(mode.to_ascii_lowercase())
    }

    pub fn status(&self, secrets: &Secrets) -> Result<StoreStatus, StoreError> {
        let harness_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM harnesses", [], |row| row.get(0))?;
        let bot_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM bots", [], |row| row.get(0))?;
        Ok(StoreStatus {
            path: self.path.display().to_string(),
            schema_version: self.schema_version()?,
            sqlite_version: rusqlite::version().to_string(),
            journal_mode: self.journal_mode()?,
            secrets_backend: secrets.backend().as_str().to_string(),
            harness_count,
            bot_count,
        })
    }

    pub fn checkpoint(&self) -> Result<(), StoreError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let marker = unclean_marker(&self.path);
        if marker.exists() {
            std::fs::remove_file(marker)?;
        }
        Ok(())
    }

    pub fn list_folders(&self) -> Result<Vec<FolderRow>, StoreError> {
        catalog::list_folders(&self.conn)
    }

    pub fn insert_folder(&self, new: &NewFolder) -> Result<FolderRow, StoreError> {
        catalog::insert_folder(&self.conn, new)
    }

    pub fn find_folder_by_path(&self, path: &str) -> Result<Option<FolderRow>, StoreError> {
        catalog::find_folder_by_path(&self.conn, path)
    }

    pub fn find_folder_by_repo_root(
        &self,
        repo_root: &str,
    ) -> Result<Option<FolderRow>, StoreError> {
        catalog::find_folder_by_repo_root(&self.conn, repo_root)
    }

    pub fn next_folder_sort_order(&self) -> Result<i64, StoreError> {
        catalog::next_folder_sort_order(&self.conn)
    }

    pub fn update_folder(&self, id: &str, patch: &FolderPatch) -> Result<FolderRow, StoreError> {
        catalog::update_folder(&self.conn, id, patch)
    }

    /// Forget the folder, keep the directory — see [`catalog::delete_folder`].
    /// Returns how many live threads lost their folder.
    pub fn delete_folder(&self, id: &str) -> Result<usize, StoreError> {
        catalog::delete_folder(&self.conn, id)
    }

    pub fn list_harnesses(&self) -> Result<Vec<HarnessRow>, StoreError> {
        catalog::list_harnesses(&self.conn)
    }

    pub fn get_harness(&self, id: &str) -> Result<Option<HarnessRow>, StoreError> {
        catalog::get_harness(&self.conn, id)
    }

    pub fn upsert_custom_harness(
        &self,
        id: &str,
        label: &str,
        command: &str,
        args: &[String],
        env: &std::collections::BTreeMap<String, String>,
        install_hint: Option<&str>,
    ) -> Result<HarnessRow, StoreError> {
        catalog::upsert_custom_harness(&self.conn, id, label, command, args, env, install_hint)
    }

    pub fn get_folder(&self, id: &str) -> Result<Option<FolderRow>, StoreError> {
        catalog::get_folder(&self.conn, id)
    }

    pub fn list_bots(&self) -> Result<Vec<BotRow>, StoreError> {
        catalog::list_bots(&self.conn)
    }

    pub fn get_bot(&self, id: &str) -> Result<Option<BotRow>, StoreError> {
        catalog::get_bot(&self.conn, id)
    }

    pub fn insert_bot(&self, new: &NewBot) -> Result<BotRow, StoreError> {
        catalog::insert_bot(&self.conn, new)
    }

    pub fn next_bot_sort_order(&self) -> Result<i64, StoreError> {
        catalog::next_bot_sort_order(&self.conn)
    }

    pub fn update_bot(&self, id: &str, patch: &BotPatch) -> Result<BotRow, StoreError> {
        catalog::update_bot(&self.conn, id, patch)
    }

    /// Remove a bot, keep its threads — see [`catalog::delete_bot`]. Returns
    /// how many live threads lost their owner.
    pub fn delete_bot(&self, id: &str) -> Result<usize, StoreError> {
        catalog::delete_bot(&self.conn, id)
    }

    pub fn insert_thread(&self, new: &NewThread) -> Result<ThreadRow, StoreError> {
        overlay::insert_thread(&self.conn, new)
    }

    pub fn get_thread(&self, id: &str) -> Result<Option<ThreadRow>, StoreError> {
        overlay::get_thread(&self.conn, id)
    }

    pub fn set_thread_acp_session(
        &self,
        id: &str,
        acp_session_id: &str,
    ) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_acp_session(&self.conn, id, acp_session_id)
    }

    /// Threads that still claim a host-owned worktree — see
    /// [`overlay::list_worktree_threads`].
    pub fn list_worktree_threads(&self) -> Result<Vec<ThreadRow>, StoreError> {
        overlay::list_worktree_threads(&self.conn)
    }

    /// Say whether the thread's worktree currently exists — see
    /// [`overlay::set_thread_worktree`].
    pub fn set_thread_worktree(
        &self,
        id: &str,
        path: Option<&str>,
    ) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_worktree(&self.conn, id, path)
    }

    /// Record the branch a thread's saved work is really on — see
    /// [`overlay::set_thread_branch`].
    pub fn set_thread_branch(&self, id: &str, branch: &str) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_branch(&self.conn, id, branch)
    }

    pub fn list_threads_by_state(&self, state: &str) -> Result<Vec<ThreadRow>, StoreError> {
        overlay::list_threads_by_state(&self.conn, state)
    }

    /// Every live thread one bot is working on — see
    /// [`overlay::list_bot_threads`].
    pub fn list_bot_threads(&self, bot_id: &str) -> Result<Vec<ThreadRow>, StoreError> {
        overlay::list_bot_threads(&self.conn, bot_id)
    }

    /// The sidebar's rows for one folder — see [`overlay::list_folder_threads`].
    pub fn list_folder_threads(&self, folder_id: &str) -> Result<Vec<ThreadRow>, StoreError> {
        overlay::list_folder_threads(&self.conn, folder_id)
    }

    pub fn set_thread_state(&self, id: &str, state: &str) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_state(&self.conn, id, state)
    }

    pub fn insert_run(
        &self,
        thread_id: &str,
        kind: &str,
        trigger_json: Option<&str>,
    ) -> Result<RunRow, StoreError> {
        overlay::insert_run(&self.conn, thread_id, kind, trigger_json)
    }

    pub fn set_run_state(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<RunRow, StoreError> {
        overlay::set_run_state(&self.conn, id, state, error)
    }

    pub fn append_transcript(
        &self,
        thread_id: &str,
        acp_method: &str,
        payload_json: &str,
    ) -> Result<TranscriptEventRow, StoreError> {
        overlay::append_transcript(&self.conn, thread_id, acp_method, payload_json)
    }

    pub fn transcript_after(
        &self,
        thread_id: &str,
        seq: i64,
    ) -> Result<Vec<TranscriptEventRow>, StoreError> {
        overlay::transcript_after(&self.conn, thread_id, seq)
    }

    pub fn transcript_head(&self, thread_id: &str) -> Result<i64, StoreError> {
        overlay::transcript_head(&self.conn, thread_id)
    }

    pub fn insert_inbox_event(
        &self,
        thread_id: &str,
        run_id: Option<&str>,
        kind: &str,
        title: &str,
        summary: &str,
        payload_json: Option<&str>,
    ) -> Result<InboxEventRow, StoreError> {
        overlay::insert_inbox_event(
            &self.conn,
            thread_id,
            run_id,
            kind,
            title,
            summary,
            payload_json,
        )
    }

    /// Guarded overlay transition — see [`overlay::transition_thread`].
    pub fn transition_thread(
        &self,
        id: &str,
        from: &str,
        to: &str,
        reason: Option<&str>,
    ) -> Result<ThreadRow, StoreError> {
        overlay::transition_thread(&self.conn, id, from, to, reason)
    }

    /// Atomic resurface — see [`overlay::resurface_thread`].
    #[allow(clippy::too_many_arguments)]
    pub fn resurface_thread(
        &self,
        id: &str,
        from: &str,
        reason: &str,
        kind: &str,
        title: &str,
        summary: &str,
        payload_json: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<(ThreadRow, InboxEventRow), StoreError> {
        overlay::resurface_thread(
            &self.conn,
            id,
            from,
            reason,
            kind,
            title,
            summary,
            payload_json,
            run_id,
        )
    }

    pub fn tombstone_thread(&self, id: &str) -> Result<ThreadRow, StoreError> {
        overlay::tombstone_thread(&self.conn, id)
    }

    pub fn set_thread_fold_policy(
        &self,
        id: &str,
        fold_policy: &str,
    ) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_fold_policy(&self.conn, id, fold_policy)
    }

    pub fn set_thread_stop(
        &self,
        id: &str,
        stop_reason: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        overlay::set_thread_stop(&self.conn, id, stop_reason, error)
    }

    pub fn set_run_acp_session(&self, id: &str, acp_session_id: &str) -> Result<(), StoreError> {
        overlay::set_run_acp_session(&self.conn, id, acp_session_id)
    }

    pub fn get_run(&self, id: &str) -> Result<Option<RunRow>, StoreError> {
        overlay::get_run(&self.conn, id)
    }

    pub fn list_runs(&self, thread_id: &str) -> Result<Vec<RunRow>, StoreError> {
        overlay::list_runs(&self.conn, thread_id)
    }

    pub fn latest_run(&self, thread_id: &str) -> Result<Option<RunRow>, StoreError> {
        overlay::latest_run(&self.conn, thread_id)
    }

    /// Runs a stopped host left open, for boot reconciliation (#21).
    pub fn list_open_runs(&self) -> Result<Vec<RunRow>, StoreError> {
        overlay::list_open_runs(&self.conn)
    }

    /// The handoff trail — see [`handoff`] (#24).
    pub fn insert_handoff(&self, new: &NewHandoff) -> Result<HandoffRow, StoreError> {
        handoff::insert_handoff(&self.conn, new)
    }

    /// Record whether the dispatched prompt actually reached an agent.
    pub fn set_handoff_dispatched(
        &self,
        id: &str,
        dispatched: bool,
        detail: Option<&str>,
    ) -> Result<(), StoreError> {
        handoff::set_handoff_dispatched(&self.conn, id, dispatched, detail)
    }

    /// Where this thread's work came from, if a bot sent it.
    pub fn latest_handoff_to(&self, thread_id: &str) -> Result<Option<HandoffRow>, StoreError> {
        handoff::latest_handoff_to(&self.conn, thread_id)
    }

    /// Every handoff onto a thread, newest first.
    pub fn list_handoffs_to(&self, thread_id: &str) -> Result<Vec<HandoffRow>, StoreError> {
        handoff::list_handoffs_to(&self.conn, thread_id)
    }

    /// Thread ↔ PR linkage and the poll's cache — see [`pr`] (#28).
    ///
    /// `link_pr` also reports whether the row is new, because a first sighting
    /// is worth an Inbox card and the fourth detection of the same PR is not.
    pub fn link_pr(&self, new: &NewThreadPr) -> Result<(ThreadPrRow, bool), StoreError> {
        pr::link_pr(&self.conn, new)
    }

    /// Replace GitHub's half of a linked row, handing back before *and* after
    /// so the caller can see what actually changed.
    pub fn apply_pr_snapshot(
        &self,
        id: &str,
        snapshot: &PrSnapshot,
    ) -> Result<(ThreadPrRow, ThreadPrRow), StoreError> {
        pr::apply_snapshot(&self.conn, id, snapshot)
    }

    pub fn get_pr(
        &self,
        provider: &str,
        repo: &str,
        number: i64,
    ) -> Result<Option<ThreadPrRow>, StoreError> {
        pr::get_pr(&self.conn, provider, repo, number)
    }

    /// Every linked PR whose thread still exists.
    pub fn list_prs(&self) -> Result<Vec<ThreadPrRow>, StoreError> {
        pr::list_prs(&self.conn)
    }

    pub fn list_prs_for_thread(&self, thread_id: &str) -> Result<Vec<ThreadPrRow>, StoreError> {
        pr::list_prs_for_thread(&self.conn, thread_id)
    }

    /// Schedules and their fires — see [`schedule`] (#25).
    pub fn insert_schedule(&self, new: &NewSchedule) -> Result<ScheduleRow, StoreError> {
        schedule::insert_schedule(&self.conn, new)
    }

    pub fn get_schedule(&self, id: &str) -> Result<Option<ScheduleRow>, StoreError> {
        schedule::get_schedule(&self.conn, id)
    }

    pub fn list_schedules(&self) -> Result<Vec<ScheduleRow>, StoreError> {
        schedule::list_schedules(&self.conn)
    }

    /// Enabled schedules whose due time has arrived — what the tick walks.
    pub fn list_due_schedules(&self, now: &str) -> Result<Vec<ScheduleRow>, StoreError> {
        schedule::list_due_schedules(&self.conn, now)
    }

    pub fn update_schedule(
        &self,
        id: &str,
        patch: &SchedulePatch,
    ) -> Result<ScheduleRow, StoreError> {
        schedule::update_schedule(&self.conn, id, patch)
    }

    /// Move (or park) a schedule's claim on the clock.
    pub fn set_schedule_due(&self, id: &str, next_run_at: Option<&str>) -> Result<(), StoreError> {
        schedule::set_schedule_due(&self.conn, id, next_run_at)
    }

    /// Which thread the schedule's last fire landed on.
    pub fn set_schedule_thread(&self, id: &str, thread_id: &str) -> Result<(), StoreError> {
        schedule::set_schedule_thread(&self.conn, id, thread_id)
    }

    pub fn delete_schedule(&self, id: &str) -> Result<usize, StoreError> {
        schedule::delete_schedule(&self.conn, id)
    }

    /// Take one occurrence exactly once — see [`schedule::claim_fire`].
    pub fn claim_fire(
        &self,
        new: &NewScheduleFire,
        next_run_at: Option<&str>,
    ) -> Result<Option<ScheduleFireRow>, StoreError> {
        schedule::claim_fire(&self.conn, new, next_run_at)
    }

    pub fn get_fire(&self, id: &str) -> Result<Option<ScheduleFireRow>, StoreError> {
        schedule::get_fire(&self.conn, id)
    }

    pub fn set_fire_target(
        &self,
        id: &str,
        thread_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<(), StoreError> {
        schedule::set_fire_target(&self.conn, id, thread_id, run_id)
    }

    pub fn set_fire_state(
        &self,
        id: &str,
        state: &str,
        detail: Option<&str>,
        delivered: bool,
    ) -> Result<(), StoreError> {
        schedule::set_fire_state(&self.conn, id, state, detail, delivered)
    }

    /// Fires whose run has not been reported on yet.
    pub fn list_undelivered_fires(&self) -> Result<Vec<ScheduleFireRow>, StoreError> {
        schedule::list_undelivered_fires(&self.conn)
    }

    pub fn latest_fire(&self, schedule_id: &str) -> Result<Option<ScheduleFireRow>, StoreError> {
        schedule::latest_fire(&self.conn, schedule_id)
    }

    pub fn list_fires(
        &self,
        schedule_id: &str,
        limit: i64,
    ) -> Result<Vec<ScheduleFireRow>, StoreError> {
        schedule::list_fires(&self.conn, schedule_id, limit)
    }

    /// Whether a run already produced an Inbox card (#15's resurface path).
    pub fn run_has_inbox_event(&self, run_id: &str) -> Result<bool, StoreError> {
        schedule::run_has_inbox_event(&self.conn, run_id)
    }

    /// The permission broker's ledger — see [`permission`] (#20).
    pub fn insert_permission_request(
        &self,
        new: &NewPermissionRequest,
    ) -> Result<PermissionRequestRow, StoreError> {
        permission::insert_permission_request(&self.conn, new)
    }

    pub fn get_permission_request(
        &self,
        id: &str,
    ) -> Result<Option<PermissionRequestRow>, StoreError> {
        permission::get_permission_request(&self.conn, id)
    }

    /// Asks nobody has answered, on threads that can still be answered on.
    pub fn list_open_permission_requests(
        &self,
        thread_id: Option<&str>,
    ) -> Result<Vec<PermissionRequestRow>, StoreError> {
        permission::list_open_permission_requests(&self.conn, thread_id)
    }

    /// Every ask ever taken on a thread, answered ones included.
    pub fn list_permission_requests(
        &self,
        thread_id: &str,
    ) -> Result<Vec<PermissionRequestRow>, StoreError> {
        permission::list_permission_requests(&self.conn, thread_id)
    }

    /// Claim an outstanding request. `false` means it was already resolved.
    pub fn resolve_permission_request(
        &self,
        id: &str,
        state: &str,
        decided_by: &str,
        option_id: Option<&str>,
        delivered: bool,
    ) -> Result<bool, StoreError> {
        permission::resolve_permission_request(
            &self.conn, id, state, decided_by, option_id, delivered,
        )
    }

    /// Update the newest undismissed card of a kind on a thread, and unread it.
    pub fn restate_inbox_event(
        &self,
        thread_id: &str,
        kind: &str,
        summary: &str,
    ) -> Result<bool, StoreError> {
        overlay::restate_inbox_event(&self.conn, thread_id, kind, summary)
    }

    pub fn list_inbox_events(
        &self,
        limit: i64,
        include_dismissed: bool,
    ) -> Result<Vec<InboxEventRow>, StoreError> {
        overlay::list_inbox_events(&self.conn, limit, include_dismissed)
    }

    pub fn count_unread_inbox(&self, thread_id: Option<&str>) -> Result<i64, StoreError> {
        overlay::count_unread_inbox(&self.conn, thread_id)
    }

    /// Unread cards per bot — the red dot on a crew blob. Same predicate as
    /// [`Store::count_unread_inbox`], so the dot and the badge cannot disagree.
    pub fn count_unread_inbox_by_bot(
        &self,
    ) -> Result<std::collections::HashMap<String, i64>, StoreError> {
        overlay::count_unread_inbox_by_bot(&self.conn)
    }

    pub fn mark_inbox_event_read(&self, id: &str) -> Result<(), StoreError> {
        overlay::mark_inbox_event_read(&self.conn, id)
    }

    pub fn mark_inbox_kind_read(&self, thread_id: &str, kind: &str) -> Result<bool, StoreError> {
        overlay::mark_inbox_kind_read(&self.conn, thread_id, kind)
    }

    pub fn mark_inbox_read(&self, thread_id: &str) -> Result<usize, StoreError> {
        overlay::mark_inbox_read(&self.conn, thread_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_session_receipt(
        &self,
        thread_id: &str,
        acp_session_id: &str,
        native_session_ref: Option<&str>,
        harness_id: &str,
        model: Option<&str>,
        cwd: &str,
        tools_json: &str,
        permission_mode: &str,
        fingerprint: &str,
    ) -> Result<SessionReceiptRow, StoreError> {
        overlay::upsert_session_receipt(
            &self.conn,
            thread_id,
            acp_session_id,
            native_session_ref,
            harness_id,
            model,
            cwd,
            tools_json,
            permission_mode,
            fingerprint,
        )
    }

    pub fn get_session_receipt(
        &self,
        thread_id: &str,
    ) -> Result<Option<SessionReceiptRow>, StoreError> {
        overlay::get_session_receipt(&self.conn, thread_id)
    }

    /// Store secret bytes in the vault; SQLite keeps only the pointer.
    pub fn put_secret(
        &self,
        secrets: &mut Secrets,
        kind: &str,
        label: &str,
        secret: &str,
        bot_id: Option<&str>,
    ) -> Result<SecretRefRow, StoreError> {
        if secret.is_empty() {
            return Err(StoreError::invalid("secret bytes must be non-empty"));
        }
        if matches!(secrets.backend(), SecretsBackend::Unavailable) {
            return Err(StoreError::SecretsUnavailable);
        }
        let row = secrets::insert_secret_ref(&self.conn, kind, label, bot_id)?;
        if let Err(err) = secrets.put(&row.account, secret) {
            let _ = secrets::delete_secret_ref(&self.conn, &row.id);
            return Err(err);
        }
        Ok(row)
    }

    pub fn get_secret(&self, secrets: &Secrets, id: &str) -> Result<String, StoreError> {
        let row = secrets::get_secret_ref(&self.conn, id)?
            .ok_or_else(|| StoreError::NotFound(id.into()))?;
        secrets.get(&row.account)
    }

    pub fn delete_secret(&self, secrets: &mut Secrets, id: &str) -> Result<(), StoreError> {
        let Some(row) = secrets::delete_secret_ref(&self.conn, id)? else {
            return Err(StoreError::NotFound(id.into()));
        };
        secrets.delete(&row.account)
    }

    pub fn list_secret_refs(&self) -> Result<Vec<SecretRefRow>, StoreError> {
        secrets::list_secret_refs(&self.conn)
    }

    /// Record a provider grant: tokens into the vault, everything else into
    /// `tool_connections` (#18).
    ///
    /// Any grant this provider already had is revoked first — vault item and
    /// pointer both — so a re-consent replaces the tokens instead of leaving
    /// an orphaned keychain entry nobody can reach or delete.
    #[allow(clippy::too_many_arguments)]
    pub fn put_tool_grant(
        &self,
        secrets: &mut Secrets,
        provider: &str,
        account: Option<&str>,
        scopes: &[String],
        client_id: Option<&str>,
        expires_at: Option<&str>,
        bundle_json: &str,
    ) -> Result<ToolConnectionRow, StoreError> {
        self.revoke_tool_secret(secrets, provider)?;
        let row = self.put_secret(secrets, TOOL_GRANT_KIND, provider, bundle_json, None)?;
        let scopes_json = serde_json::to_string(scopes)?;
        secrets::upsert_tool_connection(
            &self.conn,
            provider,
            "connected",
            account,
            &scopes_json,
            Some(&row.id),
            client_id,
            expires_at,
            None,
        )
    }

    /// The stored token bundle, or `None` when this provider has no grant.
    ///
    /// A pointer with no bytes behind it (keychain item deleted out from under
    /// us) is not an error worth failing a prompt over — it is a grant that
    /// needs re-authorising, and reads as `None`.
    pub fn get_tool_grant(
        &self,
        secrets: &Secrets,
        provider: &str,
    ) -> Result<Option<String>, StoreError> {
        let Some(row) = secrets::get_tool_connection(&self.conn, provider)? else {
            return Ok(None);
        };
        let Some(secret_ref_id) = row.secret_ref_id else {
            return Ok(None);
        };
        match self.get_secret(secrets, &secret_ref_id) {
            Ok(bundle) => Ok(Some(bundle)),
            Err(StoreError::SecretNotFound(_)) | Err(StoreError::NotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Forget a grant entirely: vault bytes, pointer, and connection row.
    pub fn delete_tool_grant(
        &self,
        secrets: &mut Secrets,
        provider: &str,
    ) -> Result<bool, StoreError> {
        let had_secret = self.revoke_tool_secret(secrets, provider)?;
        let had_row = secrets::delete_tool_connection(&self.conn, provider)?.is_some();
        Ok(had_secret || had_row)
    }

    /// Keep the row, drop the tokens: what a failed refresh leaves behind, so
    /// the chip can say "needs auth" instead of silently pretending to work.
    pub fn expire_tool_grant(
        &self,
        secrets: &mut Secrets,
        provider: &str,
        reason: &str,
    ) -> Result<(), StoreError> {
        self.revoke_tool_secret(secrets, provider)?;
        let existing = secrets::get_tool_connection(&self.conn, provider)?;
        secrets::upsert_tool_connection(
            &self.conn,
            provider,
            "needs_auth",
            existing.as_ref().and_then(|row| row.account.as_deref()),
            existing
                .as_ref()
                .map(|row| row.scopes_json.as_str())
                .unwrap_or("[]"),
            None,
            existing.as_ref().and_then(|row| row.client_id.as_deref()),
            None,
            Some(reason),
        )?;
        Ok(())
    }

    /// Remember that a connect attempt failed. The message is shown on the
    /// chip, so it has to be the provider's own words, not a stack trace.
    pub fn fail_tool_connection(
        &self,
        provider: &str,
        message: &str,
    ) -> Result<ToolConnectionRow, StoreError> {
        let existing = secrets::get_tool_connection(&self.conn, provider)?;
        secrets::upsert_tool_connection(
            &self.conn,
            provider,
            "error",
            existing.as_ref().and_then(|row| row.account.as_deref()),
            existing
                .as_ref()
                .map(|row| row.scopes_json.as_str())
                .unwrap_or("[]"),
            existing
                .as_ref()
                .and_then(|row| row.secret_ref_id.as_deref()),
            existing.as_ref().and_then(|row| row.client_id.as_deref()),
            existing.as_ref().and_then(|row| row.expires_at.as_deref()),
            Some(message),
        )
    }

    /// Stamp a refreshed access token onto an existing grant.
    pub fn refresh_tool_grant(
        &self,
        secrets: &mut Secrets,
        provider: &str,
        expires_at: Option<&str>,
        bundle_json: &str,
    ) -> Result<(), StoreError> {
        let existing = secrets::get_tool_connection(&self.conn, provider)?;
        let account = existing.as_ref().and_then(|row| row.account.clone());
        let scopes_json = existing
            .as_ref()
            .map(|row| row.scopes_json.clone())
            .unwrap_or_else(|| "[]".into());
        let client_id = existing.as_ref().and_then(|row| row.client_id.clone());
        self.revoke_tool_secret(secrets, provider)?;
        let row = self.put_secret(secrets, TOOL_GRANT_KIND, provider, bundle_json, None)?;
        secrets::upsert_tool_connection(
            &self.conn,
            provider,
            "connected",
            account.as_deref(),
            &scopes_json,
            Some(&row.id),
            client_id.as_deref(),
            expires_at,
            None,
        )?;
        Ok(())
    }

    pub fn get_tool_connection(
        &self,
        provider: &str,
    ) -> Result<Option<ToolConnectionRow>, StoreError> {
        secrets::get_tool_connection(&self.conn, provider)
    }

    pub fn list_tool_connections(&self) -> Result<Vec<ToolConnectionRow>, StoreError> {
        secrets::list_tool_connections(&self.conn)
    }

    fn revoke_tool_secret(
        &self,
        secrets: &mut Secrets,
        provider: &str,
    ) -> Result<bool, StoreError> {
        let Some(row) = secrets::get_tool_connection(&self.conn, provider)? else {
            return Ok(false);
        };
        let Some(secret_ref_id) = row.secret_ref_id else {
            return Ok(false);
        };
        match self.delete_secret(secrets, &secret_ref_id) {
            Ok(()) => Ok(true),
            // The pointer outliving its bytes is the state we are trying to
            // reach anyway; only a real vault failure is worth reporting.
            Err(StoreError::NotFound(_)) | Err(StoreError::SecretNotFound(_)) => Ok(false),
            Err(err) => Err(err),
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(err) = self.checkpoint() {
            eprintln!(
                "failed to checkpoint sqlite store at {}: {err}",
                self.path.display()
            );
        }
    }
}

fn apply_pragmas(conn: &Connection) -> Result<(), StoreError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", "5000")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}

fn verify_sqlite_version() -> Result<(), StoreError> {
    let found = rusqlite::version();
    let Some(parsed) = parse_sqlite_version(found) else {
        return Err(StoreError::SqliteTooOld {
            found: found.to_string(),
        });
    };
    if parsed < MIN_SQLITE {
        return Err(StoreError::SqliteTooOld {
            found: found.to_string(),
        });
    }
    Ok(())
}

fn parse_sqlite_version(raw: &str) -> Option<(u32, u32, u32)> {
    let mut parts = raw.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let patch: u32 = patch_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn integrity_check(conn: &Connection) -> Result<(), StoreError> {
    let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(StoreError::Integrity(result));
    }
    Ok(())
}

fn unclean_marker(db_path: &Path) -> PathBuf {
    let mut marker = db_path.as_os_str().to_os_string();
    marker.push(UNCLEAN_SUFFIX);
    PathBuf::from(marker)
}

pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn secret_account(id: &str) -> String {
    format!("jabot.secret.{id}")
}

pub fn validate_runtime_json(raw: &str) -> Result<(), StoreError> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let obj = value
        .as_object()
        .ok_or_else(|| StoreError::invalid("runtime_json must be an object"))?;
    let command = obj
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim();
    if command.is_empty() {
        return Err(StoreError::invalid("runtime_json.command is required"));
    }
    if let Some(env) = obj.get("env") {
        let env = env
            .as_object()
            .ok_or_else(|| StoreError::invalid("runtime_json.env must be an object"))?;
        for key in env.keys() {
            if env_key_looks_secret(key) {
                return Err(StoreError::invalid(format!(
                    "runtime_json.env must not contain secret key {key}"
                )));
            }
        }
    }
    Ok(())
}

/// Shared with the harness catalog (#13): a credential belongs in the keychain,
/// so neither a thread's `runtime_json` nor a catalog file may carry one.
pub(crate) fn env_key_looks_secret(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("API_KEY")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("ACCESS_KEY")
        || upper.ends_with("_TOKEN")
        || upper == "TOKEN"
}

pub(crate) fn map_folder(row: &Row<'_>) -> rusqlite::Result<FolderRow> {
    Ok(FolderRow {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        repo_root: row.get(6)?,
        origin_url: row.get(7)?,
        forge_host: row.get(8)?,
        repo_owner: row.get(9)?,
        repo_name: row.get(10)?,
        default_branch: row.get(11)?,
        setup_command: row.get(12)?,
        files_to_copy_json: row.get(13)?,
    })
}

pub(crate) fn map_harness(row: &Row<'_>) -> rusqlite::Result<HarnessRow> {
    Ok(HarnessRow {
        id: row.get(0)?,
        label: row.get(1)?,
        command: row.get(2)?,
        args_json: row.get(3)?,
        env_json: row.get(4)?,
        install_hint: row.get(5)?,
        is_builtin: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub(crate) fn map_bot(row: &Row<'_>) -> rusqlite::Result<BotRow> {
    Ok(BotRow {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        instructions: row.get(3)?,
        tools_json: row.get(4)?,
        harness_id: row.get(5)?,
        is_chief: row.get::<_, i64>(6)? != 0,
        template_id: row.get(7)?,
        host_id: row.get(8)?,
        sort_order: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        image: row.get(12)?,
    })
}

pub(crate) fn map_thread(row: &Row<'_>) -> rusqlite::Result<ThreadRow> {
    Ok(ThreadRow {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        bot_id: row.get(2)?,
        harness_id: row.get(3)?,
        acp_session_id: row.get(4)?,
        native_session_ref: row.get(5)?,
        cwd: row.get(6)?,
        runtime_json: row.get(7)?,
        title: row.get(8)?,
        state: row.get(9)?,
        fold_policy: row.get(10)?,
        last_stop_reason: row.get(11)?,
        last_error: row.get(12)?,
        preview: row.get(13)?,
        worktree_path: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        folded_at: row.get(17)?,
        resurfaced_at: row.get(18)?,
        archived_at: row.get(19)?,
        deleted_at: row.get(20)?,
        resurfaced_reason: row.get(21)?,
        repo_root: row.get(22)?,
        repo: row.get(23)?,
        forge_host: row.get(24)?,
        branch: row.get(25)?,
        host_id: row.get(26)?,
    })
}

pub(crate) fn map_handoff(row: &Row<'_>) -> rusqlite::Result<HandoffRow> {
    Ok(HandoffRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        to_thread_id: row.get(2)?,
        to_bot_id: row.get(3)?,
        from_thread_id: row.get(4)?,
        from_bot_id: row.get(5)?,
        task: row.get(6)?,
        context: row.get(7)?,
        dispatched: row.get::<_, i64>(8)? != 0,
        detail: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub(crate) fn map_thread_pr(row: &Row<'_>) -> rusqlite::Result<ThreadPrRow> {
    Ok(ThreadPrRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        provider: row.get(2)?,
        forge_host: row.get(3)?,
        repo: row.get(4)?,
        number: row.get(5)?,
        url: row.get(6)?,
        title: row.get(7)?,
        status: row.get(8)?,
        check_state: row.get(9)?,
        review_state: row.get(10)?,
        head_ref: row.get(11)?,
        base_ref: row.get(12)?,
        additions: row.get(13)?,
        deletions: row.get(14)?,
        changed_files: row.get(15)?,
        checks_json: row.get(16)?,
        pr_updated_at: row.get(17)?,
        detected_via: row.get(18)?,
        detected_at: row.get(19)?,
        polled_at: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
}

pub(crate) fn map_schedule(row: &Row<'_>) -> rusqlite::Result<ScheduleRow> {
    Ok(ScheduleRow {
        id: row.get(0)?,
        bot_id: row.get(1)?,
        title: row.get(2)?,
        cron: row.get(3)?,
        prompt: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
        catch_up: row.get(6)?,
        last_run_at: row.get(7)?,
        next_run_at: row.get(8)?,
        last_thread_id: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub(crate) fn map_schedule_fire(row: &Row<'_>) -> rusqlite::Result<ScheduleFireRow> {
    Ok(ScheduleFireRow {
        id: row.get(0)?,
        schedule_id: row.get(1)?,
        thread_id: row.get(2)?,
        run_id: row.get(3)?,
        due_at: row.get(4)?,
        fired_at: row.get(5)?,
        state: row.get(6)?,
        caught_up: row.get::<_, i64>(7)? != 0,
        skipped_count: row.get(8)?,
        detail: row.get(9)?,
        delivered_at: row.get(10)?,
    })
}

pub(crate) fn map_run(row: &Row<'_>) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        seq: row.get(2)?,
        kind: row.get(3)?,
        state: row.get(4)?,
        trigger_json: row.get(5)?,
        error: row.get(6)?,
        started_at: row.get(7)?,
        ended_at: row.get(8)?,
        created_at: row.get(9)?,
        acp_session_id: row.get(10)?,
    })
}

pub(crate) fn map_transcript(row: &Row<'_>) -> rusqlite::Result<TranscriptEventRow> {
    Ok(TranscriptEventRow {
        thread_id: row.get(0)?,
        seq: row.get(1)?,
        acp_method: row.get(2)?,
        payload_json: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub(crate) fn map_inbox_event(row: &Row<'_>) -> rusqlite::Result<InboxEventRow> {
    Ok(InboxEventRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        run_id: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        summary: row.get(5)?,
        payload_json: row.get(6)?,
        created_at: row.get(7)?,
        read_at: row.get(8)?,
        dismissed_at: row.get(9)?,
    })
}

pub(crate) fn map_permission_request(row: &Row<'_>) -> rusqlite::Result<PermissionRequestRow> {
    Ok(PermissionRequestRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        run_id: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        subject_json: row.get(5)?,
        options_json: row.get(6)?,
        state: row.get(7)?,
        decided_by: row.get(8)?,
        option_id: row.get(9)?,
        delivered: row.get(10)?,
        created_at: row.get(11)?,
        resolved_at: row.get(12)?,
    })
}

pub(crate) fn map_receipt(row: &Row<'_>) -> rusqlite::Result<SessionReceiptRow> {
    Ok(SessionReceiptRow {
        thread_id: row.get(0)?,
        acp_session_id: row.get(1)?,
        native_session_ref: row.get(2)?,
        harness_id: row.get(3)?,
        model: row.get(4)?,
        cwd: row.get(5)?,
        tools_json: row.get(6)?,
        permission_mode: row.get(7)?,
        fingerprint: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

pub(crate) fn map_secret_ref(row: &Row<'_>) -> rusqlite::Result<SecretRefRow> {
    Ok(SecretRefRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        label: row.get(2)?,
        account: row.get(3)?,
        bot_id: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub(crate) fn map_tool_connection(row: &Row<'_>) -> rusqlite::Result<ToolConnectionRow> {
    Ok(ToolConnectionRow {
        provider: row.get(0)?,
        status: row.get(1)?,
        account: row.get(2)?,
        scopes_json: row.get(3)?,
        secret_ref_id: row.get(4)?,
        client_id: row.get(5)?,
        expires_at: row.get(6)?,
        last_error: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub(crate) fn map_paired_device(row: &Row<'_>) -> rusqlite::Result<PairedDeviceRow> {
    Ok(PairedDeviceRow {
        device_id: row.get(0)?,
        name: row.get(1)?,
        role: row.get(2)?,
        fingerprint: row.get(3)?,
        token_ref: row.get(4)?,
        auth_counter: row.get(5)?,
        paired_via: row.get(6)?,
        sas: row.get(7)?,
        created_at: row.get(8)?,
        last_seen_at: row.get(9)?,
        revoked_at: row.get(10)?,
    })
}

/// Paired devices — see [`pairing`] and `migrations/0008_pairing.sql` (#19).
impl Store {
    /// Admit a device, or re-issue the grant of one that was revoked.
    pub fn upsert_paired_device(
        &self,
        new: &NewPairedDevice,
    ) -> Result<PairedDeviceRow, StoreError> {
        pairing::upsert_paired_device(&self.conn, new)
    }

    /// The row as it stands, tombstone included. Read on every call a paired
    /// device makes, so a revoke lands on the next request rather than the
    /// next reconnect.
    pub fn get_paired_device(
        &self,
        device_id: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        pairing::get_paired_device(&self.conn, device_id)
    }

    pub fn list_paired_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        pairing::list_paired_devices(&self.conn)
    }

    /// Cut a device off. `false` means it was already revoked or never paired.
    pub fn revoke_paired_device(&self, device_id: &str) -> Result<bool, StoreError> {
        pairing::revoke_paired_device(&self.conn, device_id)
    }

    /// Accept a `host/hello` proof counter strictly greater than the last one.
    /// `false` is a replay, or a device revoked in the meantime.
    pub fn bump_device_auth_counter(
        &self,
        device_id: &str,
        counter: i64,
    ) -> Result<bool, StoreError> {
        pairing::bump_device_auth_counter(&self.conn, device_id, counter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("jabot.sqlite")).unwrap();
        (store, dir)
    }

    fn sample_runtime() -> String {
        json!({
            "command": "claude-agent-acp",
            "args": [],
            "env": { "ACP_DEBUG": "1" }
        })
        .to_string()
    }

    fn sample_thread(id: &str) -> NewThread {
        NewThread {
            id: id.into(),
            folder_id: None,
            bot_id: Some("code".into()),
            harness_id: "claude".into(),
            cwd: "/tmp/repo".into(),
            runtime_json: sample_runtime(),
            title: "Auth migration".into(),
            fold_policy: "default".into(),
            worktree_path: None,
            repo: ThreadRepo::default(),
        }
    }

    fn sample_folder(name: &str, path: &str) -> NewFolder {
        NewFolder {
            name: name.into(),
            path: path.into(),
            files_to_copy_json: "[]".into(),
            ..NewFolder::default()
        }
    }

    #[test]
    fn catalog_getters_roundtrip() {
        let (store, _dir) = open_store();
        assert!(store.path().ends_with("jabot.sqlite"));
        assert_eq!(
            store.get_harness("claude").unwrap().unwrap().command,
            "claude-agent-acp"
        );
        assert!(store.get_bot("chief").unwrap().unwrap().is_chief);
        let folder = store
            .insert_folder(&sample_folder("App", "/repos/app"))
            .unwrap();
        assert_eq!(
            store.get_folder(&folder.id).unwrap().unwrap().path,
            "/repos/app"
        );
        assert_eq!(store.list_folders().unwrap().len(), 1);
        store.insert_thread(&sample_thread("t-get")).unwrap();
        assert_eq!(
            store.get_thread("t-get").unwrap().unwrap().title,
            "Auth migration"
        );
    }

    #[test]
    fn bundled_sqlite_meets_wal_fix_version() {
        let parsed = parse_sqlite_version(rusqlite::version()).unwrap();
        assert!(parsed >= MIN_SQLITE, "sqlite {}", rusqlite::version());
    }

    #[test]
    fn open_uses_wal_and_seeds_catalog() {
        let (store, _dir) = open_store();
        assert_eq!(store.journal_mode().unwrap(), "wal");
        // Derived, not written down twice: whoever lands the next migration
        // should not have to edit this line to keep it true.
        assert_eq!(store.schema_version().unwrap(), migrate::head());
        // Both compiled-in tiers land as rows, because `threads.harness_id`
        // is a foreign key and a preset has to be nameable by a thread (#13).
        let harnesses = store.list_harnesses().unwrap();
        let ids: Vec<_> = harnesses.iter().map(|h| h.id.as_str()).collect();
        assert!(
            ids.contains(&"claude") && ids.contains(&"hermes"),
            "{ids:?}"
        );
        assert!(harnesses.iter().all(|h| h.is_builtin));
        let hermes = harnesses.iter().find(|h| h.id == "hermes").unwrap();
        assert!(
            hermes.env_json.contains("HERMES_ACP_SKIP_CONFIGURED_MCP"),
            "the preset's env floor has to survive into the row: {}",
            hermes.env_json
        );
        let bots = store.list_bots().unwrap();
        assert_eq!(bots.len(), 6);
        assert_eq!(bots[0].id, "chief");
        assert!(bots[0].is_chief);
        assert_eq!(bots[0].harness_id, "claude");
    }

    #[test]
    fn reopen_does_not_recreate_deleted_bot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jabot.sqlite");
        {
            let store = Store::open(&path).unwrap();
            store
                .conn
                .execute("DELETE FROM bots WHERE id = 'writer'", [])
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let ids: Vec<_> = store
            .list_bots()
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert!(!ids.contains(&"writer".to_string()));
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn foreign_keys_and_unique_constraints() {
        let (store, _dir) = open_store();
        let err = store
            .insert_thread(&NewThread {
                harness_id: "nope".into(),
                ..sample_thread("t1")
            })
            .unwrap_err();
        assert!(matches!(err, StoreError::Invalid(_)), "{err}");

        store
            .insert_folder(&sample_folder("App", "/repos/app"))
            .unwrap();
        let dup = store
            .insert_folder(&sample_folder("App 2", "/repos/app"))
            .unwrap_err();
        assert!(matches!(dup, StoreError::Invalid(_)), "{dup}");

        store
            .conn
            .execute(
                "INSERT INTO bots (id, name, color, instructions, tools_json, harness_id, is_chief, sort_order, created_at, updated_at)
                 VALUES ('chief2', 'Other', 'b-teal', '', '[]', 'claude', 1, 9, 't', 't')",
                [],
            )
            .unwrap_err();
    }

    #[test]
    fn runtime_json_rejects_secret_env() {
        let (store, _dir) = open_store();
        let mut new = sample_thread("t-secret");
        new.runtime_json = json!({
            "command": "claude-agent-acp",
            "env": { "ANTHROPIC_API_KEY": "sk-ant-secret" }
        })
        .to_string();
        let err = store.insert_thread(&new).unwrap_err();
        assert!(err.to_string().contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn thread_run_transcript_inbox_overlay() {
        let (store, _dir) = open_store();
        store.insert_thread(&sample_thread("t1")).unwrap();
        store.set_thread_state("t1", "folded").unwrap();
        let sleeping = store.list_threads_by_state("folded").unwrap();
        assert_eq!(sleeping.len(), 1);
        assert!(sleeping[0].folded_at.is_some());

        let run = store.insert_run("t1", "prompt", None).unwrap();
        assert_eq!(run.seq, 1);
        store.set_run_state(&run.id, "running", None).unwrap();
        let done = store.set_run_state(&run.id, "succeeded", None).unwrap();
        assert!(done.ended_at.is_some());

        store
            .append_transcript(
                "t1",
                "session/update",
                &json!({ "sessionUpdate": "agent_message_chunk" }).to_string(),
            )
            .unwrap();
        store
            .append_transcript(
                "t1",
                "session/update",
                &json!({ "sessionUpdate": "agent_message_chunk", "content": "more" }).to_string(),
            )
            .unwrap();
        let replay = store.transcript_after("t1", 1).unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].seq, 2);

        let event = store
            .insert_inbox_event(
                "t1",
                Some(&run.id),
                "done",
                "Auth migration",
                "PR ready",
                None,
            )
            .unwrap();
        assert_eq!(event.kind, "done");
        assert!(event.read_at.is_none());
    }

    #[test]
    fn a_guarded_transition_refuses_a_stale_from_state() {
        let (store, _dir) = open_store();
        store.insert_thread(&sample_thread("t-guard")).unwrap();
        store
            .transition_thread("t-guard", "active", "folded", None)
            .unwrap();

        // A second caller that still believes the thread is active must lose,
        // rather than dragging it back out of the state it already reached.
        let stale = store
            .transition_thread("t-guard", "active", "archived", None)
            .unwrap_err();
        assert!(matches!(stale, StoreError::NotFound(_)), "{stale}");
        assert_eq!(
            store.get_thread("t-guard").unwrap().unwrap().state,
            "folded"
        );
    }

    #[test]
    fn a_resurface_writes_the_state_and_the_card_or_neither() {
        let (store, _dir) = open_store();
        store.insert_thread(&sample_thread("t-atomic")).unwrap();
        store
            .transition_thread("t-atomic", "active", "folded", None)
            .unwrap();

        // A run id that does not exist trips the foreign key *after* the state
        // has already been updated inside the transaction. If the two writes
        // were not one unit, the thread would be left claiming it resurfaced
        // with no Inbox card to open.
        let err = store
            .resurface_thread(
                "t-atomic",
                "folded",
                "done",
                "done",
                "Auth migration finished",
                "finished",
                None,
                Some("no-such-run"),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::Invalid(_)), "{err}");
        assert_eq!(
            store.get_thread("t-atomic").unwrap().unwrap().state,
            "folded"
        );
        assert!(store.list_inbox_events(10, true).unwrap().is_empty());

        let run = store.insert_run("t-atomic", "prompt", None).unwrap();
        let (thread, event) = store
            .resurface_thread(
                "t-atomic",
                "folded",
                "done",
                "done",
                "Auth migration finished",
                "finished",
                None,
                Some(&run.id),
            )
            .unwrap();
        assert_eq!(thread.state, "resurfaced");
        assert_eq!(thread.resurfaced_reason.as_deref(), Some("done"));
        assert!(thread.resurfaced_at.is_some());
        assert_eq!(event.kind, "done");
        assert_eq!(store.count_unread_inbox(None).unwrap(), 1);
        store.mark_inbox_read("t-atomic").unwrap();
        assert_eq!(store.count_unread_inbox(None).unwrap(), 0);
    }

    /// The red dot on a crew blob (#22, #24).
    ///
    /// Grouped by `threads.bot_id`, on the same predicate the sidebar badge
    /// counts with — a dot that disagreed with the number beside the Inbox
    /// would be worse than no dot.
    #[test]
    fn counts_what_is_waiting_on_each_bot_and_nothing_else() {
        let (store, _dir) = open_store();
        for (thread, bot) in [("t-writer", "writer"), ("t-code", "code")] {
            store
                .insert_thread(&NewThread {
                    bot_id: Some(bot.into()),
                    ..sample_thread(thread)
                })
                .unwrap();
        }
        // A thread nobody owns: it badges the sidebar and belongs to no blob.
        store
            .insert_thread(&NewThread {
                bot_id: None,
                ..sample_thread("t-loose")
            })
            .unwrap();

        for thread in ["t-writer", "t-loose"] {
            store.set_thread_state(thread, "folded").unwrap();
            store
                .resurface_thread(
                    thread, "folded", "done", "done", "Finished", "finished", None, None,
                )
                .unwrap();
        }

        let counts = store.count_unread_inbox_by_bot().unwrap();
        assert_eq!(counts.get("writer").copied(), Some(1));
        // A bot with nothing waiting is absent rather than zero, and the
        // ownerless thread's card is nobody's dot — though it is still in the
        // sidebar's own count, which is the number beside the Inbox.
        assert_eq!(counts.get("code"), None);
        assert_eq!(counts.len(), 1);
        assert_eq!(store.count_unread_inbox(None).unwrap(), 2);

        // Reading it is what clears the dot, and it clears only that bot's.
        store.mark_inbox_read("t-writer").unwrap();
        assert!(store.count_unread_inbox_by_bot().unwrap().is_empty());
        assert_eq!(store.count_unread_inbox(None).unwrap(), 1);
    }

    #[test]
    fn a_run_that_resumes_drops_the_end_time_it_never_earned() {
        let (store, _dir) = open_store();
        store.insert_thread(&sample_thread("t-resume")).unwrap();
        let run = store.insert_run("t-resume", "prompt", None).unwrap();
        store.set_run_state(&run.id, "running", None).unwrap();
        store.set_run_acp_session(&run.id, "sess-1").unwrap();

        // needs_you stamps an end time because the run stopped producing...
        let paused = store.set_run_state(&run.id, "needs_you", None).unwrap();
        assert!(paused.ended_at.is_some());
        // ...but answering the permission puts the same run back to work, and
        // a run that is running has not ended.
        let resumed = store.set_run_state(&run.id, "running", None).unwrap();
        assert!(resumed.ended_at.is_none());
        assert_eq!(resumed.acp_session_id.as_deref(), Some("sess-1"));
        assert_eq!(store.latest_run("t-resume").unwrap().unwrap().id, run.id);
    }

    #[test]
    fn a_session_receipt_is_one_row_per_thread() {
        let (store, _dir) = open_store();
        store.insert_thread(&sample_thread("t-receipt")).unwrap();
        store
            .upsert_session_receipt(
                "t-receipt",
                "sess-1",
                None,
                "claude",
                Some("sonnet"),
                "/repos/app",
                r#"["github"]"#,
                "default",
                "aaaa0000aaaa0000",
            )
            .unwrap();
        // A re-spawn replaces it: the live session is the only one a resume
        // can attach to, so a stale receipt would point at nothing.
        let respawned = store
            .upsert_session_receipt(
                "t-receipt",
                "sess-2",
                Some("claude-uuid"),
                "claude",
                Some("sonnet"),
                "/repos/app",
                r#"["github"]"#,
                "wait_for_inbox",
                "bbbb1111bbbb1111",
            )
            .unwrap();
        assert_eq!(respawned.acp_session_id, "sess-2");
        assert_eq!(respawned.permission_mode, "wait_for_inbox");
        assert_eq!(respawned.native_session_ref.as_deref(), Some("claude-uuid"));
        // The upsert keeps `created_at` and moves `updated_at`. Asserting the
        // two are equal only held while both writes landed in the same
        // millisecond, which stopped being true under a loaded test run.
        assert!(respawned.updated_at >= respawned.created_at);
        assert_eq!(
            store
                .get_session_receipt("t-receipt")
                .unwrap()
                .unwrap()
                .fingerprint,
            "bbbb1111bbbb1111"
        );
    }

    #[test]
    fn secrets_live_in_vault_not_sqlite() {
        let (store, _dir) = open_store();
        let mut secrets = Secrets::memory();
        let token = "ya29.gmail-refresh-token";
        let row = store
            .put_secret(&mut secrets, "gmail", "Gmail", token, Some("inboxm"))
            .unwrap();
        assert_eq!(row.account, secret_account(&row.id));
        assert_eq!(store.get_secret(&secrets, &row.id).unwrap(), token);

        let dump: String = store
            .conn
            .query_row("SELECT quote(id) || quote(kind) || quote(label) || quote(account) || COALESCE(quote(bot_id),'') FROM secret_refs WHERE id = ?1", [&row.id], |r| r.get(0))
            .unwrap();
        assert!(!dump.contains(token), "sqlite leaked secret: {dump}");

        let all_sql: String = {
            let mut stmt = store
                .conn
                .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
                .unwrap();
            stmt.query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(!all_sql.to_lowercase().contains("ciphertext"));

        store.delete_secret(&mut secrets, &row.id).unwrap();
        assert!(store.get_secret(&secrets, &row.id).is_err());
        assert!(store.list_secret_refs().unwrap().is_empty());
    }

    /// The same guarantee as `secrets_live_in_vault_not_sqlite`, extended to
    /// tool credentials (#18) — and checked against the database *file*, not
    /// just the rows, so a token that leaked into an index or a WAL frame
    /// would fail this too.
    #[test]
    fn tool_tokens_live_in_vault_not_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jabot.sqlite");
        let store = Store::open(&path).unwrap();
        let mut secrets = Secrets::memory();
        let access = "ya29.tool-access-token";
        let refresh = "1//tool-refresh-token";
        let bundle = format!(
            r#"{{"accessToken":"{access}","refreshToken":"{refresh}","tokenType":"Bearer"}}"#
        );

        let row = store
            .put_tool_grant(
                &mut secrets,
                "google",
                Some("you@example.com"),
                &["gmail.compose".to_string()],
                Some("client-1"),
                None,
                &bundle,
            )
            .unwrap();
        assert_eq!(row.status, "connected");
        assert_eq!(
            store.get_tool_grant(&secrets, "google").unwrap().as_deref(),
            Some(bundle.as_str())
        );

        store.checkpoint().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        for secret in [access, refresh] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "sqlite file contains {secret}"
            );
        }
        // The account label is not a secret, and the chip needs it — so it is
        // in the file, which is what makes the assertion above meaningful.
        assert!(bytes
            .windows("you@example.com".len())
            .any(|window| window == b"you@example.com"));

        // A re-consent replaces the grant rather than orphaning a vault item.
        store
            .put_tool_grant(
                &mut secrets,
                "google",
                Some("you@example.com"),
                &[],
                Some("client-1"),
                None,
                r#"{"accessToken":"second"}"#,
            )
            .unwrap();
        assert_eq!(store.list_secret_refs().unwrap().len(), 1);

        assert!(store.delete_tool_grant(&mut secrets, "google").unwrap());
        assert!(store.get_tool_grant(&secrets, "google").unwrap().is_none());
        assert!(store.list_secret_refs().unwrap().is_empty());
        assert!(store.get_tool_connection("google").unwrap().is_none());
    }

    /// A failed refresh must not leave a grant that looks alive.
    #[test]
    fn expiring_a_grant_keeps_the_row_and_drops_the_tokens() {
        let (store, _dir) = open_store();
        let mut secrets = Secrets::memory();
        store
            .put_tool_grant(
                &mut secrets,
                "notion",
                None,
                &[],
                None,
                None,
                r#"{"accessToken":"a"}"#,
            )
            .unwrap();

        store
            .expire_tool_grant(&mut secrets, "notion", "the refresh token was revoked")
            .unwrap();

        let row = store.get_tool_connection("notion").unwrap().unwrap();
        assert_eq!(row.status, "needs_auth");
        assert_eq!(
            row.last_error.as_deref(),
            Some("the refresh token was revoked")
        );
        assert!(row.secret_ref_id.is_none());
        assert!(store.get_tool_grant(&secrets, "notion").unwrap().is_none());
        assert!(store.list_secret_refs().unwrap().is_empty());
    }

    #[test]
    fn unavailable_secrets_fail_closed() {
        let (store, _dir) = open_store();
        let mut secrets = Secrets::Unavailable;
        let err = store
            .put_secret(&mut secrets, "gmail", "Gmail", "tok", None)
            .unwrap_err();
        assert!(matches!(err, StoreError::SecretsUnavailable));
        assert!(store.list_secret_refs().unwrap().is_empty());
    }

    #[test]
    fn checkpoint_clears_unclean_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jabot.sqlite");
        let store = Store::open(&path).unwrap();
        assert!(unclean_marker(&path).exists());
        store.checkpoint().unwrap();
        assert!(!unclean_marker(&path).exists());
    }
}
