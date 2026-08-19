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
pub const INBOX_RESURFACE: &str = "inbox/resurface";
pub const SYNC_RESUME_FROM: &str = "sync/resumeFrom";

pub const CLIENT_METHODS: &[&str] = &[
    HOST_HELLO,
    HOST_HEALTH,
    SESSION_PROMPT,
    SESSION_CANCEL,
    PERMISSION_REPLY,
    THREAD_FOLD,
    SYNC_RESUME_FROM,
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
    pub accepted: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadFoldParams {
    pub thread_id: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResurfaceReason {
    Done,
    Failed,
    NeedsYou,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxResurfaceParams {
    pub host_id: String,
    pub thread_id: String,
    pub seq: u64,
    pub reason: ResurfaceReason,
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
