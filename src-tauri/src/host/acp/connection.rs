//! One ACP stdio connection = one JaBot thread's adapter subprocess.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::super::protocol::error::RpcError;
use super::super::protocol::frame::encode_frame;
use super::super::protocol::jsonrpc::{
    JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
};
use super::runtime::HarnessRuntime;
use super::spawn::{spawn_adapter, terminate_process_group};
use super::wake::AdapterWake;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(8);
const SESSION_NEW_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug)]
pub enum Inbound {
    Update(Value),
    Permission { acp_id: RequestId, params: Value },
    PromptResult(Value),
    Closed { error: Option<String> },
}

pub(crate) struct AcpConnection {
    child: Child,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    pending: Arc<Mutex<HashMap<i64, Sender<JsonRpcResponse>>>>,
    inbound_rx: Receiver<Inbound>,
    next_id: i64,
    pub session_id: Option<String>,
    pub log_path: PathBuf,
    initialized: bool,
    killed: bool,
}

impl std::fmt::Debug for AcpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpConnection")
            .field("pid", &self.child.id())
            .field("session_id", &self.session_id)
            .field("log_path", &self.log_path)
            .field("initialized", &self.initialized)
            .finish_non_exhaustive()
    }
}

impl AcpConnection {
    pub fn spawn(
        runtime: &HarnessRuntime,
        cwd: Option<&std::path::Path>,
        log_path: &std::path::Path,
        wake: Arc<AdapterWake>,
    ) -> Result<Self, RpcError> {
        let spawned = spawn_adapter(runtime, cwd, log_path).map_err(|e| match e {
            super::spawn::SpawnError::Spawn { command, source }
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                RpcError::HarnessUnavailable {
                    command,
                    install_hint: runtime.install_hint.clone(),
                }
            }
            other => RpcError::Internal(other.to_string()),
        })?;

        let stdin = Arc::new(Mutex::new(spawned.stdin));
        let pending: Arc<Mutex<HashMap<i64, Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (inbound_tx, inbound_rx) = mpsc::channel();

        let reader_stdin = Arc::clone(&stdin);
        let reader_pending = Arc::clone(&pending);
        thread::Builder::new()
            .name(format!("acp-stdio-{}", spawned.child.id()))
            .spawn(move || {
                read_loop(
                    spawned.stdout,
                    reader_stdin,
                    reader_pending,
                    inbound_tx,
                    wake,
                );
            })
            .map_err(|e| RpcError::Internal(format!("acp reader thread: {e}")))?;

        Ok(Self {
            child: spawned.child,
            stdin,
            pending,
            inbound_rx,
            next_id: 1,
            session_id: None,
            log_path: spawned.log_path,
            initialized: false,
            killed: false,
        })
    }

    pub fn try_recv(&mut self) -> Result<Inbound, TryRecvError> {
        self.inbound_rx.try_recv()
    }

    pub fn initialize(&mut self) -> Result<Value, RpcError> {
        if self.initialized {
            return Ok(json!({ "protocolVersion": 1 }));
        }
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false
                },
                "clientInfo": {
                    "name": "jabot",
                    "title": "JaBot",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
            INITIALIZE_TIMEOUT,
        )?;
        self.initialized = true;
        Ok(result)
    }

    pub fn new_session(&mut self, cwd: &str, mcp_servers: Value) -> Result<String, RpcError> {
        self.initialize()?;
        let result = self.request(
            "session/new",
            json!({
                "cwd": cwd,
                "mcpServers": mcp_servers
            }),
            SESSION_NEW_TIMEOUT,
        )?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::Internal("session/new did not return sessionId".into()))?
            .to_string();
        self.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// Fire `session/prompt` without waiting for the turn to finish. Completion
    /// arrives later as a `PromptResult` inbound event (ACP v1 returns a stop
    /// reason; the host API returns as soon as the agent has accepted the write).
    pub fn send_prompt(&mut self, session_id: &str, content: &Value) -> Result<(), RpcError> {
        let prompt = prompt_blocks(content)?;
        let id = self.next_id();
        self.write_request(
            id,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": prompt
            }),
        )
    }

    /// ACP v1 `session/cancel` is a notification.
    pub fn cancel(&mut self, session_id: &str) -> Result<(), RpcError> {
        self.write_notification("session/cancel", json!({ "sessionId": session_id }))
    }

    pub fn respond(&self, id: RequestId, result: Value) -> Result<(), RpcError> {
        let response = JsonRpcResponse::success(id, result);
        self.write_message(&JsonRpcMessage::Response(response))
    }

    pub fn kill(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        terminate_process_group(&mut self.child);
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RpcError> {
        let id = self.next_id();
        let (tx, rx) = mpsc::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            pending.insert(id, tx);
        }
        if let Err(err) = self.write_request(id, method, params) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return Err(err);
        }
        match rx.recv_timeout(timeout) {
            Ok(response) => {
                if let Some(error) = response.error {
                    Err(RpcError::Internal(format!(
                        "{method} failed: {} ({})",
                        error.message, error.code
                    )))
                } else {
                    Ok(response.result.unwrap_or(Value::Null))
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(mut pending) = self.pending.lock() {
                    pending.remove(&id);
                }
                Err(RpcError::Internal(format!(
                    "{method} timed out after {}s",
                    timeout.as_secs()
                )))
            }
            Err(RecvTimeoutError::Disconnected) => Err(RpcError::Internal(format!(
                "{method}: adapter connection closed"
            ))),
        }
    }

    fn write_request(&mut self, id: i64, method: &str, params: Value) -> Result<(), RpcError> {
        let request = JsonRpcRequest::new(RequestId::Number(id), method, Some(params));
        self.write_message(&JsonRpcMessage::Request(request))
    }

    fn write_notification(&self, method: &str, params: Value) -> Result<(), RpcError> {
        let notification = JsonRpcNotification::new(method, Some(params));
        self.write_message(&JsonRpcMessage::Notification(notification))
    }

    fn write_message(&self, message: &JsonRpcMessage) -> Result<(), RpcError> {
        let frame = encode_frame(message)?;
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|e| RpcError::Internal(e.to_string()))?;
        stdin
            .write_all(frame.as_bytes())
            .map_err(|e| RpcError::Internal(format!("acp stdin: {e}")))?;
        stdin
            .flush()
            .map_err(|e| RpcError::Internal(format!("acp stdin flush: {e}")))?;
        Ok(())
    }

    fn next_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        self.kill();
    }
}

fn prompt_blocks(content: &Value) -> Result<Value, RpcError> {
    if content.is_null() {
        return Err(RpcError::InvalidParams("content is required".into()));
    }
    if let Some(text) = content.as_str() {
        return Ok(json!([{ "type": "text", "text": text }]));
    }
    if content.is_array() {
        return Ok(content.clone());
    }
    if content.get("type").and_then(Value::as_str) == Some("text") {
        return Ok(json!([content]));
    }
    if let Some(text) = content.get("text").and_then(Value::as_str) {
        return Ok(json!([{ "type": "text", "text": text }]));
    }
    Ok(json!([{ "type": "text", "text": content.to_string() }]))
}

fn read_loop(
    stdout: std::process::ChildStdout,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    pending: Arc<Mutex<HashMap<i64, Sender<JsonRpcResponse>>>>,
    inbound: Sender<Inbound>,
    wake: Arc<AdapterWake>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = inbound.send(Inbound::Closed {
                    error: Some("adapter stdout closed".into()),
                });
                wake.ping();
                break;
            }
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\n', '\r']);
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<JsonRpcMessage>(trimmed) {
                    Ok(message) => {
                        dispatch_message(message, &stdin, &pending, &inbound, &wake);
                    }
                    Err(err) => {
                        eprintln!("acp stdout parse error: {err}: {trimmed}");
                    }
                }
            }
            Err(err) => {
                let _ = inbound.send(Inbound::Closed {
                    error: Some(format!("adapter stdout: {err}")),
                });
                wake.ping();
                break;
            }
        }
    }
}

fn dispatch_message(
    message: JsonRpcMessage,
    stdin: &Arc<Mutex<std::process::ChildStdin>>,
    pending: &Arc<Mutex<HashMap<i64, Sender<JsonRpcResponse>>>>,
    inbound: &Sender<Inbound>,
    wake: &AdapterWake,
) {
    match message {
        JsonRpcMessage::Response(response) => {
            let id = match &response.id {
                RequestId::Number(n) => *n,
                _ => {
                    eprintln!("acp response with non-numeric id");
                    return;
                }
            };
            let sender = pending.lock().ok().and_then(|mut map| map.remove(&id));
            if let Some(sender) = sender {
                let _ = sender.send(response);
            } else {
                // Prompt responses arrive after send_prompt returned.
                let payload = response.result.clone().unwrap_or_else(|| {
                    json!({
                        "error": response.error.as_ref().map(|e| e.message.clone())
                    })
                });
                let _ = inbound.send(Inbound::PromptResult(payload));
                wake.ping();
            }
        }
        JsonRpcMessage::Notification(notification) => {
            if notification.method == "session/update" {
                let params = notification.params.unwrap_or(Value::Null);
                let _ = inbound.send(Inbound::Update(params));
                wake.ping();
            }
        }
        JsonRpcMessage::Request(request) => {
            if request.method == "session/request_permission" {
                let _ = inbound.send(Inbound::Permission {
                    acp_id: request.id,
                    params: request.params.unwrap_or(Value::Null),
                });
                wake.ping();
            } else {
                let error = JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                };
                let response = JsonRpcResponse::failure(request.id, error);
                if let Ok(frame) = encode_frame(&JsonRpcMessage::Response(response)) {
                    if let Ok(mut out) = stdin.lock() {
                        let _ = out.write_all(frame.as_bytes());
                        let _ = out.flush();
                    }
                }
            }
        }
    }
}
