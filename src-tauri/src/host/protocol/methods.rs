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
pub const PERMISSION_PENDING: &str = "permission/pending";
pub const PERMISSION_RESOLVED: &str = "permission/resolved";
pub const THREAD_FOLD: &str = "thread/fold";
pub const THREAD_OPEN: &str = "thread/open";
pub const THREAD_REOPEN: &str = "thread/reopen";
pub const THREAD_ARCHIVE: &str = "thread/archive";
pub const THREAD_DELETE: &str = "thread/delete";
pub const THREAD_STATE: &str = "thread/state";
pub const THREAD_TRANSCRIPT: &str = "thread/transcript";
pub const THREAD_RESUME: &str = "thread/resume";
pub const SUPERVISOR_STATUS: &str = "supervisor/status";
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
pub const CREW_THREAD: &str = "crew/thread";
pub const SCHEDULE_LIST: &str = "schedule/list";
pub const SCHEDULE_CREATE: &str = "schedule/create";
pub const SCHEDULE_UPDATE: &str = "schedule/update";
pub const SCHEDULE_REMOVE: &str = "schedule/remove";
pub const SCHEDULE_RUN: &str = "schedule/run";
pub const SYNC_RESUME_FROM: &str = "sync/resumeFrom";

pub const CLIENT_METHODS: &[&str] = &[
    HOST_HELLO,
    HOST_HEALTH,
    SESSION_PROMPT,
    SESSION_CANCEL,
    PERMISSION_REPLY,
    PERMISSION_PENDING,
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
    CREW_THREAD,
    THREAD_TRANSCRIPT,
    THREAD_RESUME,
    SUPERVISOR_STATUS,
    SCHEDULE_LIST,
    SCHEDULE_CREATE,
    SCHEDULE_UPDATE,
    SCHEDULE_REMOVE,
    SCHEDULE_RUN,
];

/// A new Inbox card exists on a thread that did **not** resurface.
///
/// `inbox/resurface` says "a folded thread came back", which is a claim about
/// the overlay. A schedule fire on an `active` standing thread produces a card
/// and moves no thread, so announcing it as a resurface would be a lie about
/// the sidebar. This is the honest half: the Inbox changed (#25).
pub const INBOX_EVENT: &str = "inbox/event";

pub const HOST_NOTIFICATIONS: &[&str] = &[
    SESSION_UPDATE,
    PERMISSION_ASK,
    PERMISSION_RESOLVED,
    INBOX_RESURFACE,
    INBOX_EVENT,
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
    /// Proof that a *paired* device is the device it says it is (#19).
    ///
    /// Absent for the local console, which is implicitly paired because it
    /// spawned the host. Required for anything else: `device.deviceId` on its
    /// own has never been enough, and this is what a second device presents
    /// instead. See [`DeviceAuth`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<DeviceAuth>,
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
    /// The subset of `methods` *this* device may call, per its role (#19's
    /// scope, surfaced for #29).
    ///
    /// A phone could hard-code the approver list, and `host/pairing/scope.rs`
    /// would still be the thing that enforces it — but then a client's idea of
    /// what it may do and the host's could drift apart silently, and the
    /// symptom would be a button that exists and always fails. The host
    /// already knows the answer; saying it is cheaper than a second copy.
    ///
    /// `default` because a host older than this field is not lying, it simply
    /// has nothing to say; a client that gets an empty list falls back to
    /// `methods`.
    #[serde(default)]
    pub scoped_methods: Vec<String>,
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
    /// The thread's *open* run, if it has one (`queued`, `running` or
    /// `needs_you`); absent once it ended, whatever it ended as.
    ///
    /// A client that mounts mid-turn cannot get this from `events`. The replay
    /// is history, and history cannot say whether the turn it stops in is
    /// still going — the last row of a live turn and the last row of a turn
    /// that died with its host look identical. So the ledger's answer travels
    /// with the replay it has to agree with, read in the same call, and a
    /// reopened thread offers Stop for work that is genuinely still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_state: Option<String>,
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

/// What became of one answer.
///
/// `delivered` is the load-bearing field and it is not always `true`: an ask
/// whose adapter died — or whose host was quit and restarted — is still
/// answerable, and the answer is still recorded, but there is no live ACP call
/// left to hand it to. Saying so is the difference between a UI that tells the
/// user the agent was told and one that tells the truth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReplyResult {
    pub request_id: String,
    pub delivered: bool,
    /// This request was already resolved before the call — a second click, or
    /// a click that raced the adapter dying. The fields below then describe
    /// what the *first* resolution decided, not what this call asked for.
    pub already_answered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    pub cancelled: bool,
}

/// An ask nobody has answered yet, as a client draws it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionView {
    pub request_id: String,
    pub thread_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The ACP `toolCall` (or whole request) the agent sent, verbatim.
    pub subject: Value,
    /// The ACP options the agent offered, verbatim. A client renders these and
    /// nothing else: the host never invents an option the agent did not offer.
    pub options: Value,
    pub created_at: String,
    /// No live adapter call is waiting on this one. The host that took the ask
    /// is gone, so answering records the decision and the agent never hears
    /// it — reopening the thread and prompting again is what continues the
    /// work (`state-machine.md`, and #21's boot reconciliation).
    pub stale: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPendingParams {
    /// One thread, or every thread when absent — the Inbox wants all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPendingResult {
    pub requests: Vec<PendingPermissionView>,
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
    /// Work in the folder's own checkout instead of a fresh worktree (#23).
    /// The advanced opt-out, never the default: two threads sharing the user's
    /// tree is the collision worktrees exist to prevent, so this is the New
    /// Chat toggle "work in my current folder" and nothing sets it implicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_checkout: Option<bool>,
    /// What the thread's branch starts from — a branch, tag or sha. Default is
    /// `origin/<default branch>`, never the user's possibly-dirty `HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
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
    /// The adapter's pid while one is attached. Diagnostic only — nothing
    /// durable is keyed on it, because decision #4's durability *is* resume
    /// and a pid does not survive a lid close.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Could `thread/resume` put this conversation back? True needs a stored
    /// session, a receipt that still matches, and a `cwd` that still exists.
    pub resumable: bool,
    /// Fields that have moved since the session was created, by wire name.
    /// Non-empty means the stored session is not this job any more, so the
    /// next prompt starts a new one (#15's fingerprint, #21's check).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drift: Vec<String>,
}

/// What `thread/resume` managed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeOutcome {
    /// The adapter was still there; nothing needed restoring.
    Live,
    /// ACP `session/resume` — context back, no replay.
    Resumed,
    /// ACP `session/load` — the agent replayed its history to us.
    Loaded,
    /// The receipt no longer matches; resuming would continue a different job.
    Drifted,
    /// This thread has never had an ACP session to resume.
    NoSession,
    /// The adapter speaks neither `session/resume` nor `session/load`.
    Unsupported,
    /// The directory the session was created in is gone.
    CwdMissing,
}

impl ResumeOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Resumed => "resumed",
            Self::Loaded => "loaded",
            Self::Drifted => "drifted",
            Self::NoSession => "no_session",
            Self::Unsupported => "unsupported",
            Self::CwdMissing => "cwd_missing",
        }
    }

    /// Is there a usable session on the other end when this comes back?
    pub fn is_attached(self) -> bool {
        matches!(self, Self::Live | Self::Resumed | Self::Loaded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResumeResult {
    pub thread_id: String,
    /// True only when a conversation is attached — `outcome` says which way.
    pub resumed: bool,
    pub outcome: ResumeOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drift: Vec<String>,
    /// One sentence a client can show. Present whenever the outcome is not a
    /// plain success, because "we could not resume" without a reason is the
    /// answer that sends a user to the wrong fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The thread as it stands afterwards, so a resume is one round trip.
    pub state: ThreadStateResult,
}

/// One adapter the supervisor is currently holding open.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveAdapterView {
    pub thread_id: String,
    pub pid: u32,
    pub harness_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_session_id: Option<String>,
    pub acp_state: String,
    /// Milliseconds since the adapter last said anything.
    pub idle_ms: u64,
    pub pending_permissions: usize,
    /// Which adapter processes may be shared (#13). Two live threads with the
    /// same key are two threads that could have been one process; today they
    /// are not, and this is what says so out loud.
    pub profile_key: String,
}

/// What the boot pass did to one run left open by a host that stopped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BootNoteView {
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The run state the previous host left behind.
    pub was: String,
    /// What it was moved to. Always terminal: nothing is reporting on it.
    pub now: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resurfaced_as: Option<ResurfaceReason>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorStatusResult {
    pub host_id: String,
    /// When this host process came up. Every `boot` note below belongs to it.
    pub booted_at: String,
    pub live_adapters: Vec<LiveAdapterView>,
    /// The reconciliation this launch performed. RAM, and rightly so: it
    /// describes what *this* process found, and the durable half of it is
    /// already in `runs` and `inbox_events`.
    pub boot: Vec<BootNoteView>,
    /// Grace before an idle adapter on a thread nobody is watching is closed.
    /// Zero means eviction is off.
    pub idle_evict_after_ms: u64,
    /// Unaccounted wall time that counts as a machine sleep.
    pub sleep_gap_threshold_ms: u64,
    /// Sleeps this host has noticed since it started.
    pub sleeps_observed: u64,
}

/// Where a thread's work came from, when a bot sent it rather than the human
/// (#24).
///
/// Chief routes by *handing off*: the receiving agent gets a prompt, and
/// without this the human reading that thread tomorrow has no way to tell
/// whether they asked for it or Chief did. `dispatched` is the honest half —
/// the handoff was recorded even if no agent could be started to hear it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HandoffView {
    pub handoff_id: String,
    /// `handoff` (a task on a crew member's standing thread) or `code_session`
    /// (a fresh coding thread in a registered folder).
    pub kind: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_bot_id: Option<String>,
    /// Resolved for display. `None` once the sending bot has been removed —
    /// the trail survives the crew member (#17 detaches rather than deletes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_bot_name: Option<String>,
    /// Whether the task actually reached an agent.
    pub dispatched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: String,
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
    /// The host-owned worktree this thread works in (#23), when it has one.
    /// Absent for every thread that is not a code thread — a worker's standing
    /// thread, a folder that is not a checkout — and absent again once the
    /// thread has been archived or deleted and its tree collected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
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
    /// The most recent handoff onto this thread (#24). Absent for every thread
    /// the human started themselves, which is most of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff: Option<HandoffView>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxEventParams {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
    /// `inbox_events.kind` — `done`, `failed`, and the rest of the closed list.
    pub kind: String,
    pub title: String,
    pub summary: String,
}

// ---- Schedules (#25) --------------------------------------------------
//
// A schedule belongs to a bot and runs on that bot's standing thread. The
// cron string is evaluated in the Mac's *local* time; every timestamp on the
// wire is UTC, like every other timestamp this host emits.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCreateParams {
    pub bot_id: String,
    pub name: String,
    /// 5-field cron, 6 with a leading seconds field, or an `@daily` shorthand.
    pub cron: String,
    /// What the bot is asked to do. Sent as an ordinary prompt on its thread.
    pub prompt: String,
    /// Defaults to on: a schedule nobody enabled is a schedule nobody wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `once` (default) or `skip` — what happens to occurrences missed while
    /// JaBot was closed. Nothing replays a backlog; see `host/schedule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catch_up: Option<String>,
}

impl ScheduleCreateParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.bot_id, "botId")?;
        require_non_empty(&self.name, "name")?;
        require_non_empty(&self.cron, "cron")?;
        require_non_empty(&self.prompt, "prompt")?;
        Ok(())
    }
}

/// Every field but the id optional: absent means "leave it", so the editor can
/// send only what the user touched and a toggle is not a full rewrite.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleUpdateParams {
    pub schedule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catch_up: Option<String>,
}

impl ScheduleUpdateParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.schedule_id, "scheduleId")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRefParams {
    pub schedule_id: String,
}

impl ScheduleRefParams {
    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        require_non_empty(&self.schedule_id, "scheduleId")
    }
}

/// One occurrence of a schedule, as the UI reads it.
///
/// `dueAt` and `firedAt` are separate on purpose: they are the same to within a
/// tick on a machine that was awake and hours apart on one that was not, and
/// the difference is the only place the catch-up decision is visible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleFireView {
    pub fire_id: String,
    pub schedule_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub due_at: String,
    pub fired_at: String,
    /// `dispatched`, `skipped`, `failed` or `delivered`.
    pub state: String,
    /// The occurrence was already in the past when the host ruled on it.
    pub caught_up: bool,
    /// Occurrences dropped in favour of (or alongside) this one.
    pub skipped_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleView {
    pub schedule_id: String,
    pub bot_id: String,
    /// Resolved for display. The bot always exists — removing it removes the
    /// schedule — so this is never a dangling name.
    pub bot_name: String,
    pub name: String,
    pub cron: String,
    pub prompt: String,
    pub enabled: bool,
    pub catch_up: String,
    /// `None` for a disabled schedule: it owes nothing, and a stale due time
    /// would make re-enabling it look like an outage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    /// The bot's standing thread, once a fire has opened one. `None` for a
    /// schedule that has never run: #24 derives the id, but a thread that does
    /// not exist yet is not one the UI can open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// The most recent occurrence, so the list can say what happened without a
    /// second round trip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fire: Option<ScheduleFireView>,
    pub recent_fires: Vec<ScheduleFireView>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleListResult {
    pub schedules: Vec<ScheduleView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRemoveResult {
    pub schedule_id: String,
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRunResult {
    pub schedule_id: String,
    pub fire: ScheduleFireView,
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

// ---- Device pairing (#19) --------------------------------------------
//
// The handshake in `host/pairing/`. Method names, field names and the
// derivations behind `mac` / `sas` are the contract a phone client written in
// another language implements, so they are documented on the types rather than
// only in the module.

pub const PAIRING_START: &str = "pairing/start";
pub const PAIRING_CLAIM: &str = "pairing/claim";
pub const PAIRING_CONFIRM: &str = "pairing/confirm";
pub const PAIRING_CANCEL: &str = "pairing/cancel";
pub const PAIRING_STATUS: &str = "pairing/status";
pub const DEVICE_LIST: &str = "device/list";
pub const DEVICE_REVOKE: &str = "device/revoke";

/// The methods added by #19, appended to [`CLIENT_METHODS`] by
/// [`client_methods`] so the two waves that both touch this file cannot lose
/// each other's entries.
pub const PAIRING_METHODS: &[&str] = &[
    PAIRING_START,
    PAIRING_CLAIM,
    PAIRING_CONFIRM,
    PAIRING_CANCEL,
    PAIRING_STATUS,
    DEVICE_LIST,
    DEVICE_REVOKE,
];

/// Every method a client may call, in one list — what `host/hello` advertises.
pub fn client_methods() -> Vec<String> {
    CLIENT_METHODS
        .iter()
        .chain(PAIRING_METHODS.iter())
        .map(|method| (*method).to_string())
        .collect()
}

impl DeviceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Approver => "approver",
        }
    }

    /// Parse a stored role. `None` for anything else — a row whose role does
    /// not parse is not treated as `full`, it is treated as unusable, which is
    /// the only safe direction for a value that decides what a device may do.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "full" => Some(Self::Full),
            "approver" => Some(Self::Approver),
            _ => None,
        }
    }
}

/// Proof that a connecting device holds the token its pairing derived.
///
/// `mac = HMAC-SHA256(deviceToken, H["jabot/hello/v1", hostId, deviceId,
/// protocolVersion, counter])`, hex, where `H` is the length-framed transcript
/// hash described on [`PairingClaimParams`]. `counter` must be strictly
/// greater than the last one this host accepted for the device, which is what
/// stops a captured proof from being replayed on a wire with no
/// confidentiality of its own.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuth {
    pub counter: u64,
    pub mac: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingStartParams {
    /// Seconds the offer stays scannable. Clamped by the host; the caller is
    /// asking, not deciding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
}

/// What the host draws. `secret` and `code` are returned exactly once, here —
/// no list method ever hands them back, so a client that loses them starts a
/// new offer instead of re-reading a live capability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingStartResult {
    pub pairing_id: String,
    pub host_id: String,
    pub host_name: String,
    pub host_fingerprint: String,
    pub host_nonce: String,
    /// The QR channel credential: 256 bits, base64url.
    pub secret: String,
    /// The typed channel credential for a host with no screen: eight Crockford
    /// characters. Low entropy on purpose — it is a code a human reads aloud —
    /// which is why the offer expires, is single-use, and stops answering
    /// after three wrong tries.
    pub code: String,
    pub expires_at: String,
    /// The exact string to put in the QR: the fields above as compact JSON.
    pub qr_payload: String,
}

/// The device's half of the QR, as it appears inside `qrPayload`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingQr {
    pub v: u32,
    pub host_id: String,
    pub host_name: String,
    pub host_fingerprint: String,
    pub pairing_id: String,
    pub host_nonce: String,
    pub secret: String,
    /// Where to reach this host. Empty in MVP1 — the only client is colocated,
    /// and publishing an address the host does not listen on would be a lie.
    pub addrs: Vec<String>,
}

/// Who is claiming. `fingerprint` is a commitment to the device's own
/// long-term key material — the device never sends the material itself, and
/// the host never needs it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingDevice {
    pub device_id: String,
    pub name: String,
    pub fingerprint: String,
    pub nonce: String,
}

/// Claim an offer, presenting the out-of-band credential and a proof.
///
/// The derivations, once, for a client in another language. `H[a, b, …]` is
/// SHA-256 over each field written as a 4-byte big-endian length followed by
/// its UTF-8 bytes — the framing is what stops one field absorbing another's
/// characters.
///
/// ```text
/// transcript = hex(H["jabot/pairing/v1", hostId, hostFingerprint, hostNonce,
///                    pairingId, deviceId, deviceFingerprint, deviceNonce, via])
/// key        = secret (via = "qr")  |  normalized code (via = "code")
/// bind(d)    = HMAC-SHA256(key, H[d, transcript])
/// mac        = hex(bind("jabot/pairing/claim/v1"))
/// hostMac    = hex(bind("jabot/pairing/host/v1"))
/// confirmMac = hex(bind("jabot/pairing/confirm/v1"))
/// sas        = eight decimal digits of bind("jabot/pairing/sas/v1"), "NNNN-NNNN"
/// token      = base64url(bind("jabot/pairing/device-token/v1"))
/// ```
///
/// The safety number therefore depends on both fingerprints and both nonces.
/// A client MUST derive its own rather than display one the host sent it: a
/// number only one side computed proves nothing about the other side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingClaimParams {
    pub pairing_id: String,
    /// Present when the QR was scanned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    /// Present when the code was typed. Case, spaces and dashes are ignored,
    /// and `I`/`L`/`O` fold onto `1`/`1`/`0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub device: PairingDevice,
    pub mac: String,
}

/// The host's answer, including its own proof of holding the same credential.
///
/// Deliberately *not* the safety number: the claiming device derives that
/// itself. What comes back is what the device cannot compute — that the party
/// on the other end of the wire also knows the out-of-band secret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingClaimResult {
    pub pairing_id: String,
    pub host_id: String,
    pub host_name: String,
    pub host_fingerprint: String,
    pub host_nonce: String,
    pub host_mac: String,
    pub via: String,
    pub expires_at: String,
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PairingSide {
    /// The machine that can run `rm -rf`. Requires a `full` device to have
    /// said hello, and is where the role is chosen.
    Host,
    /// The device that scanned. Proves itself with `confirmMac`.
    Device,
}

/// "I am looking at this safety number and it matches."
///
/// Both sides send the number they derived. The host refuses to pair unless
/// the two agree with each other *and* with its own — which is what makes a
/// man in the middle who substituted key material fail rather than merely look
/// suspicious.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingConfirmParams {
    pub pairing_id: String,
    pub side: PairingSide,
    pub sas: String,
    /// Host side only. The scope this device is granted; `approver` if unsaid.
    /// A device-side value is ignored — a client never chooses its own grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<DeviceRole>,
    /// Host side only: rename the device as it is admitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Device side only: `confirmMac` from [`PairingClaimParams`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingConfirmResult {
    pub pairing_id: String,
    /// `awaiting_device` | `awaiting_host` | `paired`.
    pub state: String,
    /// Present once both sides have confirmed and the row is on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingRefParams {
    pub pairing_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingCancelResult {
    pub pairing_id: String,
    pub cancelled: bool,
}

/// One live offer as the host operator sees it. No credentials, ever.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingOfferView {
    pub pairing_id: String,
    pub state: String,
    pub expires_at: String,
    pub attempts: u32,
    pub host_confirmed: bool,
    pub device_confirmed: bool,
    /// The safety number to put on the host's screen, once a device has
    /// claimed and the host has something to compare against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sas: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<PairingDevice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingStatusResult {
    pub offers: Vec<PairingOfferView>,
}

/// A device on the revoke list. `revokedAt` set means it is refused; the row
/// is kept so the list can answer "was this phone ever paired, and when did we
/// cut it off".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceView {
    pub device_id: String,
    pub name: String,
    pub role: DeviceRole,
    pub fingerprint: String,
    pub paired_via: String,
    /// The safety number the two humans compared when this device was let in.
    pub sas: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
    /// The colocated device that spawned this host. Implicitly paired, and it
    /// cannot be revoked — that would lock the desktop out of its own host.
    pub local: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListResult {
    pub devices: Vec<PairedDeviceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRefParams {
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRevokeResult {
    pub device_id: String,
    /// `false` when it was already revoked — the caller's intent still holds.
    pub revoked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}
