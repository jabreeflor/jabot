//! JSON-RPC 2.0 message types. Framing-agnostic: the same structs ride
//! Tauri IPC today and newline-delimited sockets later.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
    Null,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: method.into(),
            params,
        }
    }

    pub fn validate(&self) -> Result<(), super::error::RpcError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(super::error::RpcError::InvalidRequest);
        }
        if self.method.is_empty() {
            return Err(super::error::RpcError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: RequestId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    pub fn from_rpc_error(id: RequestId, error: super::error::RpcError) -> Self {
        Self::failure(id, error.into_rpc())
    }
}

/// One JSON-RPC 2.0 message. Untagged so NDJSON frames deserialize without
/// a JaBot wrapper — a Unix socket later sends these lines as-is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_roundtrip() {
        let req = JsonRpcRequest::new(
            RequestId::Number(1),
            "host/hello",
            Some(json!({ "protocolVersion": 1 })),
        );
        let encoded = serde_json::to_value(&req).unwrap();
        assert_eq!(encoded["jsonrpc"], "2.0");
        assert_eq!(encoded["id"], 1);
        assert_eq!(encoded["method"], "host/hello");
        let decoded: JsonRpcRequest = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn notification_has_no_id() {
        let n = JsonRpcNotification::new("session/update", Some(json!({ "seq": 1 })));
        let encoded = serde_json::to_value(&n).unwrap();
        assert!(encoded.get("id").is_none());
        let msg: JsonRpcMessage = serde_json::from_value(encoded).unwrap();
        assert!(matches!(msg, JsonRpcMessage::Notification(_)));
    }

    #[test]
    fn success_response_omits_error() {
        let r = JsonRpcResponse::success(RequestId::Number(7), json!({ "ok": true }));
        let encoded = serde_json::to_value(&r).unwrap();
        assert!(encoded.get("error").is_none());
        assert_eq!(encoded["result"]["ok"], true);
    }

    #[test]
    fn null_id_roundtrip() {
        let r = JsonRpcResponse::failure(
            RequestId::Null,
            JsonRpcError {
                code: -32600,
                message: "Invalid Request".into(),
                data: None,
            },
        );
        let encoded = serde_json::to_string(&r).unwrap();
        assert!(encoded.contains("\"id\":null"));
        let decoded: JsonRpcResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.id, RequestId::Null);
    }
}
