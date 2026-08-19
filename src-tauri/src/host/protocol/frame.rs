//! Newline-delimited JSON-RPC framing.
//!
//! Local Unix sockets will write one [`JsonRpcMessage`] per line. WebSocket
//! text frames carry the same JSON without the trailing newline. The in-process
//! Tauri transport uses the same message types; only the byte pipe changes.

use super::error::RpcError;
use super::jsonrpc::JsonRpcMessage;

pub fn encode_frame(message: &JsonRpcMessage) -> Result<String, RpcError> {
    let mut line = serde_json::to_string(message)
        .map_err(|e| RpcError::Internal(format!("encode frame: {e}")))?;
    line.push('\n');
    Ok(line)
}

pub fn decode_frame(line: &str) -> Result<JsonRpcMessage, RpcError> {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.is_empty() {
        return Err(RpcError::ParseError);
    }
    serde_json::from_str(line).map_err(|_| RpcError::ParseError)
}

/// Decode a buffer that may contain several NDJSON frames. Incomplete trailing
/// data is returned alongside the parsed messages so a socket reader can keep
/// it for the next read.
pub fn decode_frames(buffer: &str) -> Result<(Vec<JsonRpcMessage>, String), RpcError> {
    let mut messages = Vec::new();
    let mut consumed = 0;
    for line in buffer.split_inclusive('\n') {
        if !line.ends_with('\n') {
            break;
        }
        consumed += line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            continue;
        }
        messages.push(decode_frame(trimmed)?);
    }
    Ok((messages, buffer[consumed..].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use serde_json::json;

    #[test]
    fn ndjson_roundtrip() {
        let msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            RequestId::Number(1),
            "host/health",
            None,
        ));
        let frame = encode_frame(&msg).unwrap();
        assert!(frame.ends_with('\n'));
        assert_eq!(frame.matches('\n').count(), 1);
        let decoded = decode_frame(&frame).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn decode_frames_keeps_incomplete_tail() {
        let req = JsonRpcRequest::new(
            RequestId::Number(2),
            "host/hello",
            Some(json!({ "protocolVersion": 1 })),
        );
        let full = encode_frame(&JsonRpcMessage::Request(req.clone())).unwrap();
        let buffer = format!("{full}{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"hos");
        let (msgs, rest) = decode_frames(&buffer).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(rest.starts_with("{\"jsonrpc\""));
    }

    #[test]
    fn empty_line_is_parse_error() {
        assert!(matches!(decode_frame("\n"), Err(RpcError::ParseError)));
    }
}
