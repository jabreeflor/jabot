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
