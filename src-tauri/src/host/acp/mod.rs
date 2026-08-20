//! ACP client + adapter subprocess supervisor (#10).
//!
//! The UI never talks to ACP stdio. This module spawns one adapter process
//! per live JaBot thread, speaks ACP v1 JSON-RPC over newline-delimited
//! stdio, and forwards `session/update` / `session/request_permission` onto
//! the host notification bus.

mod connection;
mod runtime;
mod spawn;
mod wake;

use std::path::PathBuf;

use serde_json::{json, Value};
use uuid::Uuid;

use super::protocol::error::RpcError;
use super::protocol::methods::{
    PermissionReplyParams, PermissionReplyResult, PromptParams, PromptResult, SessionCancelParams,
    SessionCancelResult,
};
use super::HostSession;
use connection::Inbound;
use runtime::{HarnessRuntime, ProbeResult};

pub(crate) use connection::AcpConnection;
pub use wake::AdapterWake;

#[derive(Debug)]
pub(crate) struct PendingPermission {
    thread_id: String,
    acp_id: super::protocol::jsonrpc::RequestId,
}

impl HostSession {
    pub fn session_prompt(&mut self, params: PromptParams) -> Result<PromptResult, RpcError> {
        let thread_id = params.thread_id.clone();
        self.ensure_connection(&params)?;
        let cwd = self.resolve_cwd(&params)?;
        let session_id = {
            let conn = self.connections.get_mut(&thread_id).expect("spawned");
            match conn.session_id.clone() {
                Some(id) => id,
                None => match conn.new_session(&cwd, json!([])) {
                    Ok(id) => id,
                    Err(err) => {
                        self.connections.remove(&thread_id);
                        return Err(err);
                    }
                },
            }
        };
        if let Some(store) = &self.store {
            let _ = store.set_thread_acp_session(&thread_id, &session_id);
        }
        if let Err(err) = self
            .connections
            .get_mut(&thread_id)
            .expect("spawned")
            .send_prompt(&session_id, &params.content)
        {
            self.connections.remove(&thread_id);
            return Err(err);
        }
        self.pump_acp();
        Ok(PromptResult {
            thread_id,
            acp_session_id: session_id,
            accepted: true,
        })
    }

    pub fn session_cancel(
        &mut self,
        params: SessionCancelParams,
    ) -> Result<SessionCancelResult, RpcError> {
        let thread_id = params.thread_id.clone();
        let Some(conn) = self.connections.get_mut(&thread_id) else {
            return Err(RpcError::Internal(format!(
                "no live adapter for thread {thread_id}"
            )));
        };
        let Some(session_id) = conn.session_id.clone() else {
            return Err(RpcError::Internal(format!(
                "adapter for {thread_id} has no ACP session"
            )));
        };
        // Order matters, and it is the reverse of the obvious one. #10:
        // "Cancellation resolves outstanding permission requests with
        // `cancelled` before `session/cancel`". An agent blocked on a
        // permission it never gets an answer to has no reason to act on the
        // cancel, so answering first is what actually unblocks the turn.
        // `cancel_pending_permissions` borrows self, so the connection is
        // re-fetched after it rather than held across the call.
        self.cancel_pending_permissions(&thread_id, "session/cancel")?;
        let Some(conn) = self.connections.get_mut(&thread_id) else {
            return Err(RpcError::Internal(format!(
                "no live adapter for thread {thread_id}"
            )));
        };
        conn.cancel(&session_id)?;
        self.pump_acp();
        Ok(SessionCancelResult {
            thread_id,
            cancelled: true,
        })
    }

    pub fn permission_reply(
        &mut self,
        params: PermissionReplyParams,
    ) -> Result<PermissionReplyResult, RpcError> {
        let pending = self
            .pending_permissions
            .remove(&params.request_id)
            .ok_or_else(|| {
                RpcError::InvalidParams(format!(
                    "unknown permission requestId {}",
                    params.request_id
                ))
            })?;
        let outcome = if params.cancelled.unwrap_or(false) {
            json!({ "outcome": { "outcome": "cancelled" } })
        } else {
            json!({
                "outcome": {
                    "outcome": "selected",
                    "optionId": params.option_id
                }
            })
        };
        let Some(conn) = self.connections.get_mut(&pending.thread_id) else {
            return Err(RpcError::Internal(format!(
                "no live adapter for thread {}",
                pending.thread_id
            )));
        };
        conn.respond(pending.acp_id, outcome)?;
        let device_id = params.device_id.clone();
        self.notify_permission_resolved(
            &pending.thread_id,
            &params.request_id,
            &device_id,
            params.option_id.clone(),
            params.cancelled,
        );
        Ok(PermissionReplyResult {
            request_id: params.request_id,
            delivered: true,
        })
    }

    pub fn pump_acp(&mut self) {
        let thread_ids: Vec<String> = self.connections.keys().cloned().collect();
        for thread_id in thread_ids {
            let mut events = Vec::new();
            if let Some(conn) = self.connections.get_mut(&thread_id) {
                while let Ok(event) = conn.try_recv() {
                    events.push(event);
                }
            }
            for event in events {
                self.handle_inbound(&thread_id, event);
            }
        }
    }

    pub fn shutdown_adapters(&mut self) {
        let ids: Vec<String> = self.connections.keys().cloned().collect();
        for id in &ids {
            let _ = self.cancel_pending_permissions(id, "host shutdown");
        }
        for (_, mut conn) in self.connections.drain() {
            conn.kill();
        }
    }

    pub fn adapter_wake(&self) -> std::sync::Arc<AdapterWake> {
        std::sync::Arc::clone(&self.wake)
    }

    pub fn live_adapter_count(&self) -> usize {
        self.connections.len()
    }

    fn ensure_connection(&mut self, params: &PromptParams) -> Result<(), RpcError> {
        if self.connections.contains_key(&params.thread_id) {
            return Ok(());
        }
        let runtime = self.resolve_runtime(params)?;
        match runtime.probe() {
            ProbeResult::Missing { command, hint } => {
                return Err(RpcError::HarnessUnavailable {
                    command,
                    install_hint: hint,
                });
            }
            ProbeResult::Installed(_) => {}
        }
        let log_path = self.adapter_log_path(&params.thread_id);
        let cwd = self.resolve_cwd(params)?;
        let cwd_path = PathBuf::from(&cwd);
        let conn = AcpConnection::spawn(
            &runtime,
            Some(cwd_path.as_path()),
            &log_path,
            std::sync::Arc::clone(&self.wake),
        )?;
        self.connections.insert(params.thread_id.clone(), conn);
        Ok(())
    }

    fn resolve_runtime(&self, params: &PromptParams) -> Result<HarnessRuntime, RpcError> {
        if let Some(store) = &self.store {
            if let Ok(Some(thread)) = store.get_thread(&params.thread_id) {
                return HarnessRuntime::from_runtime_json(&thread.harness_id, &thread.runtime_json)
                    .map_err(RpcError::InvalidParams);
            }
        }
        if let Some(spec) = &params.runtime {
            let id = params
                .harness_id
                .clone()
                .unwrap_or_else(|| "custom".to_string());
            return HarnessRuntime::from_spec(id, spec).map_err(RpcError::InvalidParams);
        }
        if let (Some(store), Some(harness_id)) = (&self.store, params.harness_id.as_deref()) {
            if let Ok(Some(row)) = store.get_harness(harness_id) {
                return HarnessRuntime::from_harness(&row).map_err(RpcError::InvalidParams);
            }
        }
        Err(RpcError::InvalidParams(format!(
            "no runtime for thread {}",
            params.thread_id
        )))
    }

    fn resolve_cwd(&self, params: &PromptParams) -> Result<String, RpcError> {
        if let Some(store) = &self.store {
            if let Ok(Some(thread)) = store.get_thread(&params.thread_id) {
                return absolute_cwd(&thread.cwd);
            }
        }
        if let Some(cwd) = params.cwd.as_deref() {
            return absolute_cwd(cwd);
        }
        let fallback = std::env::current_dir().map_err(|e| RpcError::Internal(e.to_string()))?;
        Ok(fallback.to_string_lossy().into_owned())
    }

    fn adapter_log_path(&self, thread_id: &str) -> PathBuf {
        self.log_dir.join(format!("{thread_id}.stderr.log"))
    }

    fn handle_inbound(&mut self, thread_id: &str, event: Inbound) {
        match event {
            Inbound::Update(acp) => {
                self.persist_transcript(thread_id, "session/update", &acp);
                self.notify_session_update(thread_id, acp);
            }
            Inbound::Permission { acp_id, params } => {
                let request_id = Uuid::new_v4().to_string();
                self.pending_permissions.insert(
                    request_id.clone(),
                    PendingPermission {
                        thread_id: thread_id.to_string(),
                        acp_id,
                    },
                );
                let subject = params
                    .get("subject")
                    .cloned()
                    .or_else(|| params.get("toolCall").cloned())
                    .unwrap_or_else(|| params.clone());
                let options = params.get("options").cloned().unwrap_or_else(|| json!([]));
                self.persist_transcript(thread_id, "session/request_permission", &params);
                self.notify_permission_ask(thread_id, &request_id, subject, options);
            }
            Inbound::PromptResult(result) => {
                let acp = json!({
                    "sessionUpdate": "state_update",
                    "sessionState": "idle",
                    "stopReason": result.get("stopReason").cloned().unwrap_or(Value::Null),
                    "result": result,
                });
                self.persist_transcript(thread_id, "session/prompt", &acp);
                self.notify_session_update(thread_id, acp);
            }
            Inbound::Closed { error } => {
                if let Some(err) = error {
                    eprintln!("adapter for {thread_id} closed: {err}");
                }
                let _ = self.cancel_pending_permissions(thread_id, "adapter closed");
                self.connections.remove(thread_id);
            }
        }
    }

    fn persist_transcript(&self, thread_id: &str, method: &str, payload: &Value) {
        let Some(store) = &self.store else {
            return;
        };
        let Ok(json) = serde_json::to_string(payload) else {
            return;
        };
        if let Err(err) = store.append_transcript(thread_id, method, &json) {
            eprintln!("failed to persist transcript for {thread_id}: {err}");
        }
    }

    fn cancel_pending_permissions(
        &mut self,
        thread_id: &str,
        _reason: &str,
    ) -> Result<(), RpcError> {
        let ids: Vec<String> = self
            .pending_permissions
            .iter()
            .filter(|(_, pending)| pending.thread_id == thread_id)
            .map(|(id, _)| id.clone())
            .collect();
        for request_id in ids {
            if let Some(pending) = self.pending_permissions.remove(&request_id) {
                if let Some(conn) = self.connections.get_mut(thread_id) {
                    let _ = conn.respond(
                        pending.acp_id,
                        json!({ "outcome": { "outcome": "cancelled" } }),
                    );
                }
                let device = self
                    .connected_device_id
                    .clone()
                    .unwrap_or_else(|| "host".into());
                self.notify_permission_resolved(thread_id, &request_id, &device, None, Some(true));
            }
        }
        Ok(())
    }
}

fn absolute_cwd(cwd: &str) -> Result<String, RpcError> {
    let path = PathBuf::from(cwd);
    if path.is_absolute() {
        return Ok(path.to_string_lossy().into_owned());
    }
    let joined = std::env::current_dir()
        .map_err(|e| RpcError::Internal(e.to_string()))?
        .join(path);
    Ok(joined.to_string_lossy().into_owned())
}
