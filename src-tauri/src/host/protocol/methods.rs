//! Typed host protocol methods, params, and results.
//!
//! Method names follow the research envelope in
//! `docs/research/remote-and-mobile/protocol-and-reach.md`. Handlers for
//! session/prompt and friends land in later issues; the types and router
//! slots exist now so those issues do not rewrite the wire.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

pub const HOST_HELLO: &str = "host/hello";
pub const HOST_HEALTH: &str = "host/health";
pub const SESSION_PROMPT: &str = "session/prompt";
pub const SESSION_CANCEL: &str = "session/cancel";
pub const SESSION_UPDATE: &str = "session/update";
pub const PERMISSION_ASK: &str = "permission/ask";
pub const PERMISSION_REPLY: &str = "permission/reply";
pub const PERMISSION_RESOLVED: &str = "permission/resolved";
pub const THREAD_FOLD: &str = "thread/fold";
pub const THREAD_OPEN: &str = "thread/open";
pub const THREAD_REOPEN: &str = "thread/reopen";
pub const THREAD_ARCHIVE: &str = "thread/archive";
pub const THREAD_DELETE: &str = "thread/delete";
pub const THREAD_STATE: &str = "thread/state";
pub const THREAD_TRANSCRIPT: &str = "thread/transcript";
pub const INBOX_RESURFACE: &str = "inbox/resurface";
pub const HARNESS_LIST: &str = "harness/list";
pub const HARNESS_DOCTOR: &str = "harness/doctor";
pub const INBOX_LIST: &str = "inbox/list";
pub const TOOLS_LIST: &str = "tools/list";
pub const TOOLS_CONNECT: &str = "tools/connect";
pub const TOOLS_DISCONNECT: &str = "tools/disconnect";
pub const FOLDER_LIST: &str = "folder/list";
pub const FOLDER_REGISTER: &str = "folder/register";
pub const FOLDER_UPDATE: &str = "folder/update";
pub const FOLDER_FORGET: &str = "folder/forget";
pub const GITHUB_STATUS: &str = "github/status";
pub const CREW_LIST: &str = "crew/list";
pub const CREW_CREATE: &str = "crew/create";
pub const CREW_UPDATE: &str = "crew/update";
pub const CREW_REMOVE: &str = "crew/remove";
pub const SYNC_RESUME_FROM: &str = "sync/resumeFrom";

pub const CLIENT_METHODS: &[&str] = &[
    HOST_HELLO,
    HOST_HEALTH,
    SESSION_PROMPT,
    SESSION_CANCEL,
    PERMISSION_REPLY,
    THREAD_FOLD,
    THREAD_OPEN,
    THREAD_REOPEN,
    THREAD_ARCHIVE,
    THREAD_DELETE,
    THREAD_STATE,
    INBOX_LIST,
    SYNC_RESUME_FROM,
    HARNESS_LIST,
    HARNESS_DOCTOR,
    TOOLS_LIST,
    TOOLS_CONNECT,
    TOOLS_DISCONNECT,
    FOLDER_LIST,
    FOLDER_REGISTER,
    FOLDER_UPDATE,
    FOLDER_FORGET,
    GITHUB_STATUS,
    CREW_LIST,
    CREW_CREATE,
    CREW_UPDATE,
    CREW_REMOVE,
    THREAD_TRANSCRIPT,
];

pub const HOST_NOTIFICATIONS: &[&str] = &[
    SESSION_UPDATE,
    PERMISSION_ASK,
    PERMISSION_RESOLVED,
    INBOX_RESURFACE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
    Full,
    Approver,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub device_id: String,
    pub name: String,
    pub role: DeviceRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HelloDevice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<DeviceRole>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HelloParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<HelloDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HelloResult {
    pub protocol_version: u32,
    pub host_id: String,
    pub host_name: String,
    pub host_mode: String,
    pub version: String,
    pub platform: String,
    pub device: DeviceInfo,
    pub methods: Vec<String>,
    pub notifications: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_error: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    pub version: String,
    pub platform: String,
    pub host_mode: String,
    pub host_id: String,
    pub protocol_version: u32,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_error: Option<String>,
}

/// Snapshot of the adapter command used to spawn an ACP subprocess.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpec {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptParams {
    pub thread_id: String,
    pub content: Value,
    /// What to do when a turn is already in flight on this thread (#14).
    /// Omitted is [`PromptMode::Reject`], which is #15's contract: a client
    /// that has not been taught about the queue cannot silently create one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<PromptMode>,
    /// Used when the thread is not yet in the store (tests / first prompt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptResult {
    pub thread_id: String,
    pub acp_session_id: String,
    /// The agent has the prompt. False means it is only queued — see `queued`.
    pub accepted: bool,
    /// Held for the turn in flight rather than sent (`mode: queue|interrupt`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    /// 1 = next out. Only meaningful while `queued`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<usize>,
}

/// Steer vs redispatch: what a prompt does to a thread that is already busy.
///
/// ACP has no mid-turn steering primitive — `session/prompt` is one turn per
/// session and the stop reason comes back on the response — so the two honest
/// answers are *wait for the turn* and *end the turn first*. Both keep the
/// #15 invariant that no run collects another run's outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptMode {
    /// Refuse with `RUN_IN_FLIGHT` (#15). The default, so an older client
    /// cannot queue by accident.
    #[default]
    Reject,
    /// Hold it and send it when the turn in flight ends.
    Queue,
    /// Cancel the turn in flight, then send this one when the cancelled turn
    /// reports back. Buzz's "if the adapter lacks steer, cancel and redispatch".
    Interrupt,
}

impl PromptMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Queue => "queue",
            Self::Interrupt => "interrupt",
        }
    }
}

/// Hydrate a reopened thread from our own store, never from harness JSONL
/// (#14, store.md "transcript ownership").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTranscriptParams {
    pub thread_id: String,
    /// Exclusive: only rows with a greater `seq`. A client that already holds
    /// part of the transcript asks for the rest instead of the whole thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<i64>,
    /// Newest-N window. The reply is still in `seq` order; `truncated` says
    /// whether anything older was left behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// One `transcript_events` row. `payload` is the ACP notification as it
/// arrived — the renderer runs the same mapper over a replay as over the live
/// stream, which is the only way the two can agree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEventView {
    pub seq: i64,
    pub method: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTranscriptResult {
    pub thread_id: String,
    /// The highest `seq` on disk for this thread, whether or not it is in
    /// `events`. A live client applies notifications above this and drops the
    /// ones at or below it, which is what makes hydrate-while-streaming safe.
    pub head_seq: i64,
    pub events: Vec<TranscriptEventView>,
    /// Older rows exist that `limit` left out.
    pub truncated: bool,
    /// Prompts held for the turn in flight, oldest first. Supervisor RAM, so
    /// this is empty after a restart — the same answer the run ledger gives.
    pub queued: Vec<QueuedPromptView>,
}

/// A prompt the user has sent that the agent has not been given yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPromptView {
    pub position: usize,
    /// The prompt content as the client sent it.
    pub content: Value,
    pub queued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelResult {
    pub thread_id: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReplyResult {
    pub request_id: String,
    pub delivered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub thread_id: String,
}

/// `threads.fold_policy`. "Wait for Inbox" is a permission policy on a folded
/// thread — auto-allow reads, still ask for execute and delete — not a fifth
/// overlay state (#5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoldPolicy {
    #[default]
    Default,
    WaitForInbox,
}

impl FoldPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::WaitForInbox => "wait_for_inbox",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "wait_for_inbox" => Self::WaitForInbox,
            _ => Self::Default,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadFoldParams {
    pub thread_id: String,
    /// Omitted keeps whatever policy the thread already has: "Disappear until
    /// done" and "Wait for Inbox" are the same fold with different quietness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<FoldPolicy>,
}

/// Every lifecycle method that only needs to name a thread.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRefParams {
    pub thread_id: String,
}

/// New Chat: the edge into the state machine. Idempotent — opening a thread
/// that already exists returns it rather than starting a second one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadOpenParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub title: String,
    pub cwd: String,
    pub harness_id: String,
    /// Snapshot of `{ command, args, env }` for this thread (#6). Without it
    /// the harness catalog row is used as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold_policy: Option<FoldPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub seq: i64,
    pub kind: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub created_at: String,
}

/// The receipt #21 compares against on resume. `fingerprint` is the cheap
/// equality check; the fields beside it say what drifted when it fails.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptView {
    pub acp_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_session_ref: Option<String>,
    pub harness_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub cwd: String,
    pub tools: Vec<String>,
    pub permission_mode: String,
    pub fingerprint: String,
    pub updated_at: String,
}

/// The process axis, reported next to the overlay state and never folded into
/// it: a folded thread that is still `running` is the whole feature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessView {
    pub connected: bool,
    pub acp_state: String,
    pub pending_permissions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStateResult {
    pub thread_id: String,
    pub title: String,
    pub state: String,
    pub fold_policy: FoldPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resurfaced_reason: Option<ResurfaceReason>,
    pub cwd: String,
    /// The spawn record (#16, setup-porting §19): where this thread works,
    /// stamped when it was opened and never re-derived. It outlives the folder
    /// it was copied from, which is the point — a thread whose folder has been
    /// forgotten still knows its checkout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    /// `owner/name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Which machine opened it. One host in MVP1; recorded so a second one
    /// never has to guess (remote-and-mobile).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    pub harness_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folded_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resurfaced_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    pub process: ProcessView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<RunView>,
    pub runs: Vec<RunView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ReceiptView>,
    pub unread: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_dismissed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxEventView {
    pub id: String,
    pub thread_id: String,
    pub thread_title: String,
    pub thread_state: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_at: Option<String>,
}

/// Still Sleeping is a projection of `threads.state = folded`, not an event —
/// folding writes no card, because the thread row already says it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SleepingThreadView {
    pub thread_id: String,
    pub title: String,
    pub fold_policy: FoldPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folded_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_state: Option<String>,
    pub acp_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxListResult {
    pub events: Vec<InboxEventView>,
    pub sleeping: Vec<SleepingThreadView>,
    pub unread: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReplyParams {
    pub request_id: String,
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResumeFromParams {
    pub thread_id: String,
    /// Exclusive: replay notifications with `seq` greater than this value.
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoggedEvent {
    pub seq: u64,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResumeFromResult {
    pub thread_id: String,
    pub head_seq: u64,
    pub events: Vec<LoggedEvent>,
}

/// Envelope fields present on every host → client notification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateParams {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
    /// Opaque ACP `session/update` payload (ACP v1 `session/update` params).
    pub acp: Value,
    /// Where this event landed in `transcript_events`, when it landed at all.
    /// `seq` above orders notifications; this one orders the durable log, and
    /// a client hydrating from `thread/transcript` needs the second to know
    /// which live events it has already replayed. `None` on a host with no
    /// store, which is also a host with nothing to hydrate from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_seq: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAskParams {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
    pub request_id: String,
    pub subject: Value,
    pub options: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResolvedParams {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
    pub request_id: String,
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
}

/// Why a folded thread came back. `failed` and `stuck` are distinct on purpose:
/// a failure wants a retry, a stall wants patience or a cancel, and the process
/// behind a `stuck` card is deliberately still alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResurfaceReason {
    Done,
    Failed,
    Stuck,
    NeedsYou,
}

impl ResurfaceReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Stuck => "stuck",
            Self::NeedsYou => "needs_you",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "stuck" => Some(Self::Stuck),
            "needs_you" => Some(Self::NeedsYou),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxResurfaceParams {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
    pub reason: ResurfaceReason,
}

/// Which tier of the catalog a card came from (#13). The UI shows all three
/// the same way; the tier says who may edit it and whether its id is reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessTier {
    Shipped,
    Preset,
    Custom,
}

/// How many JaBot chats one adapter process may carry.
///
/// Hermes wants one long-lived process per profile with chats multiplexed as
/// ACP sessions; Claude and Codex get a process per thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionScope {
    Thread,
    Profile,
}

/// Why a harness is not ready. Each variant is a different fix, which is the
/// entire reason the Doctor exists: "not installed" sends the user to the
/// wrong page five times out of six.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessStatus {
    Ready,
    /// The vendor product is not installed at all.
    CliMissing,
    /// The product is here; the thing that speaks ACP is not.
    AdapterMissing,
    /// The adapter is here but answers an older ACP than the host speaks.
    AdapterOutdated,
    /// Installed and configured, but nobody is signed in.
    LoggedOut,
    /// Installed and signed in, but not set up (no provider, no model).
    InvalidConfig,
    /// The adapter is a bridge to a daemon that is not running.
    DaemonNotRunning,
    /// The probe could not be run. Deliberately not a failure: an unanswered
    /// question must not read as a diagnosis.
    Unknown,
}

/// A catalog row as a New Chat / crew-editor card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCardView {
    pub id: String,
    pub label: String,
    pub blurb: String,
    /// Accent colour token, e.g. `var(--h-claude)`.
    pub accent: String,
    pub tier: HarnessTier,
    pub command: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    pub session_scope: SessionScope,
    /// Reserved ids cannot be shadowed by a user file.
    pub reserved: bool,
}

/// A tier-3 file that did not make it into the catalog, and why. Surfaced
/// rather than logged: a user who wrote the file is the only one who can fix it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogIssue {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessListResult {
    pub harnesses: Vec<HarnessCardView>,
    pub issues: Vec<CatalogIssue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessDoctorParams {
    /// Probe one card instead of the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_id: Option<String>,
    /// Also spawn each ready adapter and run the ACP handshake. The only way
    /// to learn the protocol version it actually speaks, and the only way to
    /// find out it is outdated before a user's first prompt does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessReport {
    pub id: String,
    pub label: String,
    pub tier: HarnessTier,
    pub status: HarnessStatus,
    pub ready: bool,
    /// One sentence naming what was found, in the user's terms.
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
    /// The absolute path that resolved, when one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessDoctorResult {
    pub reports: Vec<HarnessReport>,
    pub issues: Vec<CatalogIssue>,
    /// The PATH the probes searched. "It works in my terminal" is a PATH the
    /// app never inherited, and this is how the user can see the difference.
    pub path: Vec<String>,
}

/// How a catalog tool reaches its provider (#18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTransport {
    /// Remote MCP over streamable HTTP, with a host-minted bearer.
    Http,
    /// A local MCP subprocess the harness spawns.
    Stdio,
    /// Not MCP: the harness's own `execute`. Terminal, and only Terminal.
    HarnessExecute,
}

/// What the bot editor's chip says. Each value is a different next action,
/// which is why "not working" is not one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConnectionStatus {
    /// Usable in a session right now.
    Connected,
    /// Needs a provider grant and has none. The chip offers Connect.
    NeedsAuth,
    /// A consent window is open. `authorizeUrl` is the page to show.
    Connecting,
    /// The last attempt failed; `detail` is the provider's own words.
    Error,
    /// A local MCP server whose command is not installed on this machine.
    Missing,
}

/// A catalog entry as a chip in the bot editor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCardView {
    pub id: String,
    pub label: String,
    pub blurb: String,
    pub transport: ToolTransport,
    /// False for Terminal: allowlisting it can never produce an MCP server.
    pub mcp: bool,
    /// The grant this tool draws on. Several tools share one — Gmail,
    /// Calendar and Drive are one Google login.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_label: Option<String>,
    pub scopes: Vec<String>,
    pub status: ToolConnectionStatus,
    /// One sentence for the chip: which account, or what went wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Only while a consent window is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorize_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    pub docs_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolListResult {
    pub tools: Vec<ToolCardView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRefParams {
    pub tool_id: String,
}

/// `tools/connect` returns as soon as the flow is running, not when the user
/// has finished signing in: the host answers on one thread and consent takes
/// as long as a human takes. Poll `tools/list` for `authorizeUrl` and for the
/// outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConnectResult {
    pub tool_id: String,
    pub provider: String,
    pub status: ToolConnectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorize_url: Option<String>,
    pub redirect_uri: String,
    /// The other chips this grant covers, so the UI can say so before the user
    /// wonders why Calendar lit up when they connected Gmail.
    pub affects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDisconnectResult {
    pub tool_id: String,
    pub provider: String,
    pub disconnected: bool,
    /// Every chip that lost its grant. Disconnecting Gmail disconnects
    /// Calendar and Drive, because there was only ever one Google login.
    pub affects: Vec<String>,
}

/// A folder's `origin`, split the way `gh` splits it (#16). Absent when the
/// directory has no remote, or a remote no forge claims — both of which are
/// folders that still run threads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderOriginView {
    pub url: String,
    /// `github.com`, a GHES hostname, `gitlab.com`. Never assumed.
    pub host: String,
    pub owner: String,
    pub name: String,
    /// `owner/name` — one spelling for `gh --repo`, `thread_prs.repo`, and the
    /// PR view, so they cannot disagree about what this repository is called.
    pub repo: String,
}

/// A sidebar row under a folder. The fields are exactly what the list needs;
/// the transcript and the run ledger are a `thread/state` away.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderThreadView {
    pub thread_id: String,
    pub folder_id: Option<String>,
    pub bot_id: Option<String>,
    pub harness_id: String,
    pub title: String,
    pub state: String,
    pub fold_policy: FoldPolicy,
    /// The latest run's state, or `None` for a thread that has never run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// One registered directory (#16). A folder is a repo, not a group of them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    pub folder_id: String,
    /// Ours to display and to rename. The directory keeps its own name.
    pub name: String,
    /// The absolute directory the user registered.
    pub path: String,
    /// What a thread in this folder starts in: the repository root when there
    /// is one, else the registered path. Resolved here so the renderer does not
    /// re-derive the rule, and so #23 has one thing to replace with a worktree.
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    /// False for a directory git does not claim. Legal: threads run, the PR
    /// view skips it, and the sidebar says so.
    pub is_git: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<FolderOriginView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// Optional per-folder setup for a fresh worktree (#23 runs it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_command: Option<String>,
    /// Gitignored files a fresh worktree needs — `.env` and friends (#23).
    pub files_to_copy: Vec<String>,
    pub sort_order: i64,
    /// Active and resurfaced threads only. A folded thread is not listed: that
    /// is the promise fold makes.
    pub threads: Vec<FolderThreadView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderListResult {
    pub folders: Vec<FolderView>,
}

/// Register a directory. The host probes git once, here, and writes the answer
/// down; nothing later re-derives it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderRegisterParams {
    /// Absolute, or `~`-relative. The host canonicalises it.
    pub path: String,
    /// Defaults to the directory's basename, and stays editable after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_to_copy: Option<Vec<String>>,
}

/// A patch. An omitted field is left alone; an empty `setupCommand` clears it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderUpdateParams {
    pub folder_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_to_copy: Option<Vec<String>>,
    /// Ask git again: a remote added or re-pointed since registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderRefParams {
    pub folder_id: String,
}

/// Forgetting a folder removes the sidebar row, never the directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderForgetResult {
    pub folder_id: String,
    pub forgotten: bool,
    /// Threads that lost their folder and kept everything else — their cwd and
    /// their repo were stamped on them at spawn.
    pub detached_threads: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatusParams {
    /// Defaults to `github.com`. GHES folders pass their `origin` host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// Whether the host can act as the user on GitHub, and as whom.
///
/// There is no token in this result and there never will be: MVP auth is the
/// user's own `gh` login, read on demand by the host (#16). `installed` and
/// `authenticated` are separate because they have different remedies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatusResult {
    pub installed: bool,
    pub authenticated: bool,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
    /// Where `gh` resolved from, so "it works in my terminal" is comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gh_path: Option<String>,
}

/// A `bots` row as the crew grid and the bot editor see it (#17).
///
/// `tools` is the parsed allowlist rather than the stored JSON text, because
/// every reader wants the list and none of them wants to parse it twice. The
/// editor **is** this record: what it saves is what the next spawn resolves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BotView {
    pub bot_id: String,
    pub name: String,
    /// A colour token from [`crate::host::BOT_COLORS`] — the gradient is the
    /// bot's identity in the UI, so the host keeps the vocabulary closed.
    pub color: String,
    /// Persona / system prompt. Also mirrored to `instructions.md` in the
    /// bot's memory directory, where the session can read it.
    pub instructions: String,
    /// MCP catalog ids, plus host-tool ids for Chief (#6, #18).
    pub tools: Vec<String>,
    pub harness_id: String,
    /// Exactly one bot has this, it is seeded, and it cannot be removed.
    pub is_chief: bool,
    /// Which template's fields were copied when this bot was added. History,
    /// not a link: the template is never read again (#17).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    /// The bot's own directory: `instructions.md`, `MEMORY.md`, and the cwd a
    /// worker's standing thread runs in. `None` on a host with no data
    /// directory — an ephemeral host has nowhere to put one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_dir: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A shipped template pack: a bot record without an identity (#6).
///
/// Adding one **copies** these fields into a new row. There is no live link
/// back, which is why the editor can change anything afterwards without the
/// pack having a say.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BotTemplateView {
    pub template_id: String,
    pub name: String,
    pub color: String,
    pub instructions: String,
    pub tools: Vec<String>,
    pub harness_id: String,
}

/// One of Chief's host tools (#6). Not MCP, not in the `tools/list` catalog,
/// and not offered to other bots — but the crew grid still has to name it
/// rather than print `handoff_to_bot` at the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrewHostToolView {
    pub id: String,
    pub label: String,
    pub blurb: String,
}

/// Everything the Crew view draws in one answer. The templates and host tools
/// are compiled in and tiny; sending them with the crew means the editor and
/// the grid can never disagree about what a template contains.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrewListResult {
    pub bots: Vec<BotView>,
    pub templates: Vec<BotTemplateView>,
    pub host_tools: Vec<CrewHostToolView>,
}

/// Add a bot. Every field is optional so that "add from template" is one
/// call — the template supplies whatever the caller did not — but the result
/// is a snapshot either way.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrewCreateParams {
    /// A shipped pack to copy the unspecified fields from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_id: Option<String>,
}

/// A patch. An omitted field is left alone; `instructions: ""` really does
/// clear the persona, which is a thing a user may want.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrewUpdateParams {
    pub bot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrewRefParams {
    pub bot_id: String,
}

/// Removing a bot takes the row, never the directory — its markdown memory
/// outlives it, the same way forgetting a folder leaves the checkout alone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrewRemoveResult {
    pub bot_id: String,
    pub removed: bool,
    /// Threads that lost their bot and kept everything else — their cwd,
    /// harness and runtime were stamped on them at spawn.
    pub detached_threads: usize,
    /// The directory left behind, so the UI can say where the notes went.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_dir: Option<String>,
}

impl ToolRefParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.tool_id, "toolId")
    }
}

impl CrewCreateParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        // A blank name with no template is the one shape that cannot be
        // resolved into a bot; everything else has a default to fall back on.
        match (&self.name, &self.template_id) {
            (None, None) => Err(super::error::RpcError::InvalidParams(
                "name or templateId is required".into(),
            )),
            (Some(name), _) => require_non_empty(name, "name"),
            _ => Ok(()),
        }
    }
}

impl CrewUpdateParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.bot_id, "botId")?;
        if let Some(name) = &self.name {
            require_non_empty(name, "name")?;
        }
        Ok(())
    }
}

impl CrewRefParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.bot_id, "botId")
    }
}

impl FolderRegisterParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.path, "path")
    }
}

impl FolderUpdateParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.folder_id, "folderId")?;
        if let Some(name) = &self.name {
            require_non_empty(name, "name")?;
        }
        Ok(())
    }
}

impl FolderRefParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.folder_id, "folderId")
    }
}

impl PromptParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.thread_id, "threadId")
    }
}

impl SessionCancelParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.thread_id, "threadId")
    }
}

impl ThreadFoldParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.thread_id, "threadId")
    }
}

impl ThreadRefParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.thread_id, "threadId")
    }
}

impl ThreadOpenParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.title, "title")?;
        require_non_empty(&self.cwd, "cwd")?;
        require_non_empty(&self.harness_id, "harnessId")?;
        if let Some(id) = &self.thread_id {
            require_non_empty(id, "threadId")?;
        }
        Ok(())
    }
}

impl PermissionReplyParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.request_id, "requestId")?;
        require_non_empty(&self.device_id, "deviceId")?;
        let cancelled = self.cancelled.unwrap_or(false);
        match (self.option_id.as_deref(), cancelled) {
            (Some(id), false) if !id.is_empty() => Ok(()),
            (None, true) => Ok(()),
            (Some(id), true) if !id.is_empty() => Err(super::error::RpcError::InvalidParams(
                "optionId and cancelled are mutually exclusive".into(),
            )),
            _ => Err(super::error::RpcError::InvalidParams(
                "permission/reply requires optionId or cancelled: true".into(),
            )),
        }
    }
}

impl ThreadTranscriptParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.thread_id, "threadId")
    }
}

impl ResumeFromParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.thread_id, "threadId")
    }
}

pub fn require_non_empty(value: &str, field: &str) -> Result<(), super::error::RpcError> {
    if value.trim().is_empty() {
        Err(super::error::RpcError::InvalidParams(format!(
            "{field} must be non-empty"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hello_params_camel_case() {
        let params: HelloParams = serde_json::from_value(json!({
            "protocolVersion": 1,
            "device": { "deviceId": "dev-1", "name": "Phone", "role": "approver" }
        }))
        .unwrap();
        assert_eq!(params.protocol_version, Some(1));
        assert_eq!(params.device.unwrap().role, Some(DeviceRole::Approver));
    }

    #[test]
    fn resurface_reason_snake_case() {
        let encoded = serde_json::to_value(ResurfaceReason::NeedsYou).unwrap();
        assert_eq!(encoded, json!("needs_you"));
    }

    #[test]
    fn permission_reply_requires_decision() {
        let params = PermissionReplyParams {
            request_id: "r1".into(),
            device_id: "d1".into(),
            option_id: None,
            cancelled: None,
        };
        assert!(params.validate().is_err());
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use serde_json::json;

    /// Every host → client notification must carry the same envelope
    /// (`hostId`, `threadId`, `seq`) so a reconnecting client can order and
    /// attribute events without knowing the method. `Envelope` is that
    /// contract; this test is what keeps the concrete params structs honest
    /// against it.
    #[test]
    fn every_notification_params_carries_the_envelope() {
        let session_update = serde_json::to_value(SessionUpdateParams {
            host_id: "host-1".into(),
            thread_id: "thread-1".into(),
            seq: 7,
            acp: json!({ "sessionUpdate": "agent_message_chunk" }),
            transcript_seq: Some(3),
        })
        .unwrap();

        let permission_ask = serde_json::to_value(PermissionAskParams {
            host_id: "host-1".into(),
            thread_id: "thread-1".into(),
            seq: 8,
            request_id: "perm-1".into(),
            subject: json!({}),
            options: json!([]),
        })
        .unwrap();

        let inbox_resurface = serde_json::to_value(InboxResurfaceParams {
            host_id: "host-1".into(),
            thread_id: "thread-1".into(),
            seq: 9,
            reason: ResurfaceReason::NeedsYou,
        })
        .unwrap();

        for (label, params) in [
            ("session/update", session_update),
            ("permission/ask", permission_ask),
            ("inbox/resurface", inbox_resurface),
        ] {
            let envelope: Envelope = serde_json::from_value(params)
                .unwrap_or_else(|e| panic!("{label} is missing envelope fields: {e}"));
            assert_eq!(envelope.host_id, "host-1", "{label}");
            assert_eq!(envelope.thread_id, "thread-1", "{label}");
            assert!(envelope.seq > 0, "{label}");
        }
    }
}
