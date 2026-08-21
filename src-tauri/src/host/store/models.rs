//! Row types for the host SQLite store. UUIDs and timestamps are text.

use serde::{Deserialize, Serialize};

/// One registered local directory (#16) — not a group of repos, and not a
/// remote. `repo_root` is `None` for a directory git does not claim: such a
/// folder still runs threads, it just has no PR surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub id: String,
    pub name: String,
    pub path: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub repo_root: Option<String>,
    pub origin_url: Option<String>,
    pub forge_host: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
    /// Run once in a fresh worktree before the agent starts (#23 consumes it).
    pub setup_command: Option<String>,
    /// Gitignored files a worktree needs copied in — `.env` and friends.
    pub files_to_copy_json: String,
}

impl FolderRow {
    /// The files-to-copy list as a list. Stored as JSON because SQLite has no
    /// arrays; a column that will not parse reads as "nothing to copy" rather
    /// than failing a spawn, since the worst case is a worktree missing its
    /// `.env` and the alternative is a thread that cannot be opened at all.
    pub fn files_to_copy(&self) -> Vec<String> {
        serde_json::from_str(&self.files_to_copy_json).unwrap_or_default()
    }
}

/// What `folder/register` writes. The git columns come from probing the
/// directory once, at registration, rather than on every read: a sidebar that
/// shells out to git per render is a sidebar that stutters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewFolder {
    pub name: String,
    pub path: String,
    pub sort_order: i64,
    pub repo_root: Option<String>,
    pub origin_url: Option<String>,
    pub forge_host: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
    pub setup_command: Option<String>,
    pub files_to_copy_json: String,
}

/// A field-by-field patch: `None` leaves the column alone, which is what lets
/// a rename and a setup-script edit be the same method.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderPatch {
    pub name: Option<String>,
    pub setup_command: Option<Option<String>>,
    pub files_to_copy_json: Option<String>,
    /// A re-probe: origin and default branch as git answers today.
    pub repo: Option<FolderRepoPatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderRepoPatch {
    pub repo_root: Option<String>,
    pub origin_url: Option<String>,
    pub forge_host: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessRow {
    pub id: String,
    pub label: String,
    pub command: String,
    pub args_json: String,
    pub env_json: String,
    pub install_hint: Option<String>,
    pub is_builtin: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BotRow {
    pub id: String,
    pub name: String,
    pub color: String,
    pub instructions: String,
    pub tools_json: String,
    pub harness_id: String,
    pub is_chief: bool,
    pub template_id: Option<String>,
    pub host_id: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// What `crew/create` writes. `is_chief` is absent on purpose: the seat is
/// seeded once and `bots_one_chief` allows exactly one, so "add a bot" can
/// never be the thing that mints a second Chief.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewBot {
    pub name: String,
    pub color: String,
    pub instructions: String,
    pub tools_json: String,
    pub harness_id: String,
    /// Provenance only. The template's fields were copied at create time and
    /// nothing reads back through this id (#17) — editing the bot later must
    /// not be affected by the pack it came from.
    pub template_id: Option<String>,
    pub sort_order: i64,
}

/// A field-by-field patch: `None` leaves the column alone. There is no
/// `is_chief` and no `template_id` — one is a seat, the other is history.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BotPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub instructions: Option<String>,
    pub tools_json: Option<String>,
    pub harness_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRow {
    pub id: String,
    pub folder_id: Option<String>,
    pub bot_id: Option<String>,
    pub harness_id: String,
    pub acp_session_id: Option<String>,
    pub native_session_ref: Option<String>,
    pub cwd: String,
    pub runtime_json: String,
    pub title: String,
    pub state: String,
    pub fold_policy: String,
    pub last_stop_reason: Option<String>,
    pub last_error: Option<String>,
    pub preview: Option<String>,
    pub worktree_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub folded_at: Option<String>,
    pub resurfaced_at: Option<String>,
    pub archived_at: Option<String>,
    pub deleted_at: Option<String>,
    /// Why the thread came back. `failed` and `stuck` are deliberately
    /// distinct: one needs a retry, the other needs patience or a cancel.
    pub resurfaced_reason: Option<String>,
    /// Where the thread lives, stamped at spawn and never re-derived
    /// (setup-porting §19). These outlive the folder they were copied from.
    pub repo_root: Option<String>,
    /// `owner/name`, as `gh` spells it.
    pub repo: Option<String>,
    pub forge_host: Option<String>,
    pub branch: Option<String>,
    pub host_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewThread {
    pub id: String,
    pub folder_id: Option<String>,
    pub bot_id: Option<String>,
    pub harness_id: String,
    pub cwd: String,
    pub runtime_json: String,
    pub title: String,
    pub fold_policy: String,
    /// The host-owned git worktree this thread works in (#23), or `None` for a
    /// thread that has no repo to isolate — a worker's standing thread, a
    /// folder that is not a checkout, or the advanced "use my own checkout".
    /// Written with the row, never by a follow-up UPDATE: a tree that exists
    /// for even one crash-width moment without a row is a tree nothing will
    /// ever clean up.
    #[serde(default)]
    pub worktree_path: Option<String>,
    /// Where this thread works, resolved once at spawn (setup-porting §19).
    #[serde(default)]
    pub repo: ThreadRepo,
}

/// The repo half of a spawn record. Every field is optional because a thread
/// with no folder — a bot's standing thread in its memory directory — is a
/// thread with no repo, and that is a legal answer rather than a missing one.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRepo {
    pub repo_root: Option<String>,
    /// `owner/name`.
    pub repo: Option<String>,
    pub forge_host: Option<String>,
    pub branch: Option<String>,
    pub host_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunRow {
    pub id: String,
    pub thread_id: String,
    pub seq: i64,
    pub kind: String,
    pub state: String,
    pub trigger_json: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
    /// The ACP session this run executed on. Sequential runs share one until a
    /// resume mints a new session id.
    pub acp_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEventRow {
    pub thread_id: String,
    pub seq: i64,
    pub acp_method: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxEventRow {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub payload_json: Option<String>,
    pub created_at: String,
    pub read_at: Option<String>,
    pub dismissed_at: Option<String>,
}

/// One `session/request_permission`, as the broker recorded it (#20).
///
/// The ACP request id it arrived on is deliberately *not* here: it belongs to
/// a live adapter call and is meaningless to the next process. What is here is
/// everything a human needs in order to be asked the same question again after
/// a quit — and `delivered`, which says whether the agent ever heard the
/// answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestRow {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub kind: Option<String>,
    pub title: String,
    pub subject_json: String,
    pub options_json: String,
    pub state: String,
    pub decided_by: Option<String>,
    pub option_id: Option<String>,
    pub delivered: bool,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewPermissionRequest {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub kind: Option<String>,
    pub title: String,
    pub subject_json: String,
    pub options_json: String,
}

/// What a session was spawned with, so a later resume can tell whether the
/// world moved under it (#21). Persisted, not held in RAM — an in-memory map
/// is exactly the drift bug this table exists to avoid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionReceiptRow {
    pub thread_id: String,
    pub acp_session_id: String,
    pub native_session_ref: Option<String>,
    pub harness_id: String,
    pub model: Option<String>,
    pub cwd: String,
    pub tools_json: String,
    pub permission_mode: String,
    pub fingerprint: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SecretRefRow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub account: String,
    pub bot_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The non-secret half of a provider grant (#18). One row per provider —
/// Gmail, Calendar and Drive are three catalog entries drawing on one Google
/// grant — so the chips can show connected / needs auth without opening the
/// vault. `secret_ref_id` is the pointer to the keychain item holding the
/// tokens; nothing here is a credential.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConnectionRow {
    pub provider: String,
    pub status: String,
    pub account: Option<String>,
    pub scopes_json: String,
    pub secret_ref_id: Option<String>,
    pub client_id: Option<String>,
    pub expires_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    pub path: String,
    pub schema_version: i32,
    pub sqlite_version: String,
    pub journal_mode: String,
    pub secrets_backend: String,
    pub harness_count: i64,
    pub bot_count: i64,
}

/// One dispatch Chief made: a task put on another crew member's thread (#24).
///
/// The row hangs off the *receiving* thread, because that is where the
/// question gets asked — a standing thread that is suddenly busy has to be
/// able to say who asked. `dispatched` is the honest half: the record that the
/// work was handed over survives even when no agent could be started to hear
/// it, and `detail` says why.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HandoffRow {
    pub id: String,
    pub kind: String,
    pub to_thread_id: String,
    pub to_bot_id: Option<String>,
    pub from_thread_id: Option<String>,
    pub from_bot_id: Option<String>,
    pub task: String,
    pub context: Option<String>,
    pub dispatched: bool,
    pub detail: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewHandoff {
    pub kind: String,
    pub to_thread_id: String,
    pub to_bot_id: Option<String>,
    pub from_thread_id: Option<String>,
    pub from_bot_id: Option<String>,
    pub task: String,
    pub context: Option<String>,
    pub dispatched: bool,
    pub detail: Option<String>,
}

/// A device this host has been paired with (#19).
///
/// `token_ref` names a vault account; the token itself is never in SQLite and
/// never in this struct. `revoked_at` is the tombstone that makes a revoke
/// survive a restart — see `migrations/0008_pairing.sql`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceRow {
    pub device_id: String,
    pub name: String,
    pub role: String,
    pub fingerprint: String,
    pub token_ref: String,
    pub auth_counter: i64,
    pub paired_via: String,
    pub sas: String,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl PairedDeviceRow {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewPairedDevice {
    pub device_id: String,
    pub name: String,
    pub role: String,
    pub fingerprint: String,
    pub token_ref: String,
    pub paired_via: String,
    pub sas: String,
}

/// A recurring job (#25).
///
/// The columns are `0001_init.sql`'s, plus `catch_up` from `0009`. Field names
/// mirror the columns rather than the wire — `title`, not `name` — so that a
/// reader of this file and a reader of the schema are looking at the same
/// thing; the rename happens once, in the view.
///
/// `next_run_at` is durable on purpose. The host only runs while JaBot does
/// (decision #4), so the gap between this column and the wall clock at the next
/// launch *is* the backlog — there is no other record of what the schedule owed
/// while the Mac was shut.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRow {
    pub id: String,
    pub bot_id: String,
    pub title: String,
    pub cron: String,
    pub prompt: String,
    pub enabled: bool,
    /// `once` or `skip` — see [`crate::host::CatchUp`].
    pub catch_up: String,
    pub last_run_at: Option<String>,
    /// `None` only while a schedule is disabled and has no claim on the clock.
    pub next_run_at: Option<String>,
    /// The bot's standing thread, as of the last fire.
    pub last_thread_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewSchedule {
    pub bot_id: String,
    pub title: String,
    pub cron: String,
    pub prompt: String,
    pub enabled: bool,
    pub catch_up: String,
    pub next_run_at: Option<String>,
}

/// Every field optional: absent means "leave it", which is what lets the UI
/// send only what the user touched.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePatch {
    pub title: Option<String>,
    pub cron: Option<String>,
    pub prompt: Option<String>,
    pub enabled: Option<bool>,
    pub catch_up: Option<String>,
}

/// One occurrence of a schedule, recorded before it is dispatched.
///
/// `due_at` is the occurrence; `fired_at` is when the host got to it. They are
/// the same to within a tick on a machine that was awake, and hours apart on
/// one that was not — which is the whole of what `caught_up` reports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleFireRow {
    pub id: String,
    pub schedule_id: String,
    pub thread_id: Option<String>,
    pub run_id: Option<String>,
    pub due_at: String,
    pub fired_at: String,
    pub state: String,
    pub caught_up: bool,
    /// Occurrences dropped in favour of this one. Non-zero only after an outage.
    pub skipped_count: i64,
    pub detail: Option<String>,
    pub delivered_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewScheduleFire {
    pub schedule_id: String,
    pub due_at: String,
    pub state: String,
    pub caught_up: bool,
    pub skipped_count: i64,
    pub detail: Option<String>,
}
