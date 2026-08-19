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
