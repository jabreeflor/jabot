//! JSON-RPC 2.0 error codes for the JaBot host protocol.

use serde::Serialize;
use serde_json::Value;

/// Standard and application JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const PROTOCOL_MISMATCH: i64 = -32000;
pub const UNIMPLEMENTED: i64 = -32001;
pub const HELLO_REQUIRED: i64 = -32002;
pub const UNPAIRED_DEVICE: i64 = -32003;
pub const HARNESS_UNAVAILABLE: i64 = -32004;
pub const ILLEGAL_TRANSITION: i64 = -32005;
pub const THREAD_NOT_FOUND: i64 = -32006;
pub const STORE_UNAVAILABLE: i64 = -32007;
pub const RUN_IN_FLIGHT: i64 = -32008;
pub const FOLDER_EXISTS: i64 = -32009;
pub const CHIEF_REQUIRED: i64 = -32010;
pub const WORKTREE_FAILED: i64 = -32011;
pub const CWD_MISSING: i64 = -32012;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Parse error")]
    ParseError,
    #[error("Invalid Request")]
    InvalidRequest,
    #[error("Method not found")]
    MethodNotFound,
    #[error("Invalid params: {0}")]
    InvalidParams(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Method not implemented: {0}")]
    Unimplemented(&'static str),
    #[error("Unsupported protocol version {requested} (host supports {supported})")]
    ProtocolMismatch { requested: u32, supported: u32 },
    #[error("host/hello is required before this method")]
    HelloRequired,
    #[error("Unpaired device")]
    UnpairedDevice,
    #[error("Harness unavailable: {command}")]
    HarnessUnavailable {
        command: String,
        install_hint: Option<String>,
    },
    /// The state machine refused the move. Never a silent no-op: a fold that
    /// quietly did nothing would leave the sidebar and the store disagreeing
    /// about whether the user's work disappeared (#15).
    #[error("cannot {action} a {from} thread")]
    IllegalTransition {
        thread_id: String,
        from: String,
        action: String,
    },
    #[error("no such thread: {0}")]
    ThreadNotFound(String),
    #[error("this host has no store; lifecycle state cannot be persisted")]
    StoreUnavailable,
    /// A second prompt while the thread's run is still in flight. ACP runs one
    /// turn per session at a time, and the outcome that comes back names no
    /// prompt — so a second run would be handed the first turn's stop reason
    /// and the first run would end holding nothing (#15).
    #[error("thread {thread_id} already has a run in flight")]
    RunInFlight {
        thread_id: String,
        run_id: String,
        state: String,
    },
    /// This directory — or the checkout it belongs to — is already a folder.
    /// An error rather than a silent second row, and it carries the id of the
    /// folder that already exists so the UI can select it instead (#16).
    #[error("{path} is already registered")]
    FolderExists { folder_id: String, path: String },
    /// Chief is the one crew seat the product assumes exists — the Inbox, the
    /// handoff tools and every "ask Chief" path are written against it, and
    /// `bots_one_chief` means a deleted seat cannot simply be re-added. Every
    /// other bot is the user's to remove (#6, #17).
    #[error("Chief cannot be removed")]
    ChiefRequired { bot_id: String },
    /// The thread's worktree could not be created (#23). A refusal rather than
    /// a quiet fall back to the folder's own checkout: the fallback is two
    /// agents and a human editing one directory, which is the failure this
    /// costs the most to debug. New Chat holds the draft and offers "work in
    /// my current folder" as the deliberate way through.
    #[error("could not create a worktree for {thread_id}: {detail}")]
    WorktreeFailed {
        thread_id: String,
        path: Option<String>,
        detail: String,
    },
    /// The thread's working directory is not there any more — an unmounted
    /// volume, a moved checkout, a worktree removed under a folded thread —
    /// so there is nothing to prompt in. `keep-alive.md` says refuse: an
    /// adapter spawned anyway inherits JaBot's own working directory, and the
    /// `session/new` that follows would overwrite the receipt still pointing
    /// at the real conversation (#21).
    #[error("{cwd} is gone; {thread_id} cannot run until it is back")]
    CwdMissing { thread_id: String, cwd: String },
}

impl RpcError {
    pub fn code(&self) -> i64 {
        match self {
            Self::ParseError => PARSE_ERROR,
            Self::InvalidRequest => INVALID_REQUEST,
            Self::MethodNotFound => METHOD_NOT_FOUND,
            Self::InvalidParams(_) => INVALID_PARAMS,
            Self::Internal(_) => INTERNAL_ERROR,
            Self::ProtocolMismatch { .. } => PROTOCOL_MISMATCH,
            Self::Unimplemented(_) => UNIMPLEMENTED,
            Self::HelloRequired => HELLO_REQUIRED,
            Self::UnpairedDevice => UNPAIRED_DEVICE,
            Self::HarnessUnavailable { .. } => HARNESS_UNAVAILABLE,
            Self::IllegalTransition { .. } => ILLEGAL_TRANSITION,
            Self::ThreadNotFound(_) => THREAD_NOT_FOUND,
            Self::StoreUnavailable => STORE_UNAVAILABLE,
            Self::RunInFlight { .. } => RUN_IN_FLIGHT,
            Self::FolderExists { .. } => FOLDER_EXISTS,
            Self::ChiefRequired { .. } => CHIEF_REQUIRED,
            Self::WorktreeFailed { .. } => WORKTREE_FAILED,
            Self::CwdMissing { .. } => CWD_MISSING,
        }
    }

    pub fn data(&self) -> Option<Value> {
        match self {
            Self::Unimplemented(method) => Some(serde_json::json!({ "method": method })),
            Self::ProtocolMismatch {
                requested,
                supported,
            } => Some(serde_json::json!({
                "requested": requested,
                "supported": supported,
            })),
            Self::InvalidParams(detail) => Some(serde_json::json!({ "detail": detail })),
            Self::Internal(detail) => Some(serde_json::json!({ "detail": detail })),
            Self::HarnessUnavailable {
                command,
                install_hint,
            } => Some(serde_json::json!({
                "command": command,
                "installHint": install_hint,
            })),
            Self::IllegalTransition {
                thread_id,
                from,
                action,
            } => Some(serde_json::json!({
                "threadId": thread_id,
                "from": from,
                "action": action,
            })),
            Self::ThreadNotFound(thread_id) => Some(serde_json::json!({
                "threadId": thread_id,
            })),
            Self::RunInFlight {
                thread_id,
                run_id,
                state,
            } => Some(serde_json::json!({
                "threadId": thread_id,
                "runId": run_id,
                "runState": state,
            })),
            Self::FolderExists { folder_id, path } => Some(serde_json::json!({
                "folderId": folder_id,
                "path": path,
            })),
            Self::ChiefRequired { bot_id } => Some(serde_json::json!({ "botId": bot_id })),
            Self::WorktreeFailed {
                thread_id,
                path,
                detail,
            } => Some(serde_json::json!({
                "threadId": thread_id,
                "path": path,
                "detail": detail,
            })),
            Self::CwdMissing { thread_id, cwd } => Some(serde_json::json!({
                "threadId": thread_id,
                "cwd": cwd,
            })),
            _ => None,
        }
    }

    pub fn into_rpc(self) -> super::jsonrpc::JsonRpcError {
        super::jsonrpc::JsonRpcError {
            code: self.code(),
            message: self.to_string(),
            data: self.data(),
        }
    }
}

impl Serialize for RpcError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.clone_rpc().serialize(serializer)
    }
}

impl RpcError {
    fn clone_rpc(&self) -> super::jsonrpc::JsonRpcError {
        super::jsonrpc::JsonRpcError {
            code: self.code(),
            message: self.to_string(),
            data: self.data(),
        }
    }
}
