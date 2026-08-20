//! Row types for the host SQLite store. UUIDs and timestamps are text.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub id: String,
    pub name: String,
    pub path: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
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
