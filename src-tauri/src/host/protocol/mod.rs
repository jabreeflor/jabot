//! JSON-RPC 2.0 host protocol.
//!
//! Socket-shaped: these messages are the wire. Tauri IPC is one transport;
//! a Unix socket / WebSocket later is the same frames
//! ([#4](docs/decisions/issues-4-6.md), research in
//! `docs/research/remote-and-mobile/protocol-and-reach.md`).

pub mod error;
pub mod frame;
pub mod jsonrpc;
pub mod methods;

pub use error::RpcError;
pub use frame::{decode_frame, decode_frames, encode_frame};
pub use jsonrpc::{
    JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
    JSONRPC_VERSION,
};
pub use methods::*;
