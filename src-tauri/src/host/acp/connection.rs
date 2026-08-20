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
/// Resume restores context; it does not run a turn, so it should answer as
/// fast as `session/new`. `session/load` shares the budget because a replay
/// arrives as notifications while the response is still outstanding.
const SESSION_RESUME_TIMEOUT: Duration = Duration::from_secs(8);
/// Closing is best-effort cleanup on a path that is about to kill the process
/// group anyway (archive, delete, idle-evict). An adapter that will not answer
/// in a second does not get to hold up the user's Archive.
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

/// What the agent said it can do, read once out of the `initialize` result.
///
/// Absent means **no**. Every one of these is a capability an adapter has to
/// opt into (`session-lifecycle/keep-alive.md`), and guessing yes buys a
/// `session/resume` that comes back `-32601` on a thread the user was told had
/// been restored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AgentCapabilities {
    /// ACP `agentCapabilities.loadSession` — replay history into the client.
    pub load_session: bool,
    /// `sessionCapabilities.resume` — restore context *without* a replay.
    pub resume: bool,
    /// `sessionCapabilities.close` — free adapter-side resources. Buzz never
    /// sent it and leaked process trees; that is the bug this flag exists for.
    pub close: bool,
}

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
    capabilities: AgentCapabilities,
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
            capabilities: AgentCapabilities::default(),
        })
    }

    pub fn try_recv(&mut self) -> Result<Inbound, TryRecvError> {
        self.inbound_rx.try_recv()
    }

    pub fn capabilities(&self) -> AgentCapabilities {
        self.capabilities
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Is the adapter process still running?
    ///
    /// The supervisor cannot ask the reader thread: EOF on stdout is what
    /// tells it a child is gone, and a child that forked something holding the
    /// same stdout leaves that pipe open after it exits. So the read loop
    /// blocks on a pipe nobody will ever write to while the adapter itself is
    /// a corpse — which is a session JaBot would keep reporting as live
    /// forever. Reaping the pid is the only answer that cannot lie.
    pub fn is_alive(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
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
        self.capabilities = parse_capabilities(&result);
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

    /// ACP `session/resume`: hand the agent back a session it already has.
    ///
    /// Restores context **without** replaying history, which is what makes it
    /// the right verb for a thread whose transcript we already hold. The same
    /// absolute `cwd` and the same MCP list go back out: resume is a
    /// continuation of one job, and a session that comes back pointed at a
    /// different directory or holding different tools is a different job
    /// (`keep-alive.md`, "Resume recipe").
    pub fn resume_session(
        &mut self,
        session_id: &str,
        cwd: &str,
        mcp_servers: Value,
    ) -> Result<(), RpcError> {
        self.initialize()?;
        self.request(
            "session/resume",
            json!({
                "sessionId": session_id,
                "cwd": cwd,
                "mcpServers": mcp_servers
            }),
            SESSION_RESUME_TIMEOUT,
        )?;
        self.session_id = Some(session_id.to_string());
        Ok(())
    }

    /// ACP `session/load`: the agent replays the whole conversation at us.
    ///
    /// The replay arrives as `session/update` notifications *before* this
    /// returns, so the caller decides what happens to them — a thread with no
    /// transcript of its own wants them, and a thread that has one would get
    /// every message twice (`keep-alive.md` step 4).
    pub fn load_session(
        &mut self,
        session_id: &str,
        cwd: &str,
        mcp_servers: Value,
    ) -> Result<(), RpcError> {
        self.initialize()?;
        self.request(
            "session/load",
            json!({
                "sessionId": session_id,
                "cwd": cwd,
                "mcpServers": mcp_servers
            }),
            SESSION_RESUME_TIMEOUT,
        )?;
        self.session_id = Some(session_id.to_string());
        Ok(())
    }

    /// ACP `session/close`. Frees the agent's own resources before we drop the
    /// process; skipped, not faked, when the adapter never advertised it.
    pub fn close_session(&mut self, session_id: &str) -> Result<(), RpcError> {
        if !self.capabilities.close {
            return Ok(());
        }
        self.request(
            "session/close",
            json!({ "sessionId": session_id }),
            SESSION_CLOSE_TIMEOUT,
        )?;
        Ok(())
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

/// Read the capability flags out of an `initialize` result.
///
/// Two shapes are accepted for `sessionCapabilities` because two exist in the
/// wild: ACP nests it under `agentCapabilities`, and adapters written against
/// the v2 session surface put it at the top level. Reading only one of them
/// would silently downgrade half the adapters to "cannot resume".
fn parse_capabilities(result: &Value) -> AgentCapabilities {
    let agent = result.get("agentCapabilities");
    let session = agent
        .and_then(|caps| caps.get("sessionCapabilities"))
        .or_else(|| result.get("sessionCapabilities"));
    let flag = |source: Option<&Value>, key: &str| {
        source
            .and_then(|value| value.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    AgentCapabilities {
        load_session: flag(agent, "loadSession"),
        resume: flag(session, "resume"),
        close: flag(session, "close"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_read_from_both_shapes_agents_use() {
        // ACP nests session capabilities under `agentCapabilities`.
        let nested = parse_capabilities(&json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": true,
                "sessionCapabilities": { "resume": true, "close": false }
            }
        }));
        assert_eq!(
            nested,
            AgentCapabilities {
                load_session: true,
                resume: true,
                close: false
            }
        );

        // Adapters written against the v2 session surface hoist it.
        let hoisted = parse_capabilities(&json!({
            "protocolVersion": 1,
            "sessionCapabilities": { "resume": true, "close": true }
        }));
        assert!(hoisted.resume && hoisted.close);
        assert!(!hoisted.load_session);
    }

    #[test]
    fn an_unadvertised_capability_is_no() {
        // The failure this rules out is a `session/resume` that comes back
        // "method not found" on a thread the user was told had been restored.
        let silent = parse_capabilities(&json!({ "protocolVersion": 1 }));
        assert_eq!(silent, AgentCapabilities::default());
        let junk = parse_capabilities(&json!("not even an object"));
        assert_eq!(junk, AgentCapabilities::default());
        // A non-boolean is not a yes either.
        let lying = parse_capabilities(&json!({
            "agentCapabilities": { "loadSession": "yes" }
        }));
        assert!(!lying.load_session);
    }
}
