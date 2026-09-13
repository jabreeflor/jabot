//! One ACP stdio connection = one adapter subprocess, carrying one or more
//! JaBot threads.
//!
//! Usually one. A harness the catalog marks `SessionScope::Profile` — Hermes,
//! OpenClaw — wants a single long-lived process per profile with JaBot's chats
//! multiplexed onto it as ACP sessions (`setup-porting/hermes.md` §5), so the
//! supervisor keys its pool by `HarnessDescriptor::profile_key` and several
//! threads can land here.
//!
//! That makes routing this module's job, and there are two directions. Going
//! out is easy: every ACP request names its `sessionId`. Coming back is not.
//! `session/update` and `session/request_permission` carry a `sessionId` and
//! are routed through [`SessionBackend::route`], but the ACP v1 prompt
//! *response* carries only the JSON-RPC request id — so the id `send_prompt`
//! allocated is recorded against its thread, and that map is the only thing
//! that can say whose turn just ended on a shared process.
//!
//! This is the ACP implementation of the host's [`SessionBackend`] boundary
//! (#299). The host never names `AcpConnection` outside `spawn`; everything
//! it asks goes through the trait.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::super::backend::{
    BackendCapabilities, BackendEvent, BackendKind, InteractionId, Restored, SessionBackend, TurnId,
};
use super::super::harness::doctor::advertised_models;
use super::super::procgroup::GroupedChild;
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

pub(crate) struct AcpConnection {
    child: GroupedChild,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    pending: Arc<Mutex<HashMap<i64, Sender<JsonRpcResponse>>>>,
    inbound_rx: Receiver<BackendEvent>,
    next_id: i64,
    /// thread id → the ACP session it owns on this process.
    sessions: HashMap<String, String>,
    /// The reverse, for routing an inbound `sessionId` back to a thread.
    by_session: HashMap<String, String>,
    /// `session/prompt` request ids in flight, against the thread that sent
    /// each. See the module docs: the prompt response names no session.
    prompt_owners: HashMap<TurnId, String>,
    log_path: PathBuf,
    initialized: bool,
    killed: bool,
    capabilities: BackendCapabilities,
    /// Model ids the last `session/new` advertised, when it did.
    advertised_models: Vec<String>,
}

impl std::fmt::Debug for AcpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpConnection")
            .field("kind", &self.kind().as_str())
            .field("pid", &self.child.id())
            .field("sessions", &self.sessions)
            .field("log_path", &self.log_path)
            .field("initialized", &self.initialized)
            .finish_non_exhaustive()
    }
}

impl AcpConnection {
    pub fn spawn(
        runtime: &HarnessRuntime,
        cwd: Option<&Path>,
        log_path: &Path,
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
            sessions: HashMap::new(),
            by_session: HashMap::new(),
            prompt_owners: HashMap::new(),
            log_path: spawned.log_path,
            initialized: false,
            killed: false,
            capabilities: BackendCapabilities::default(),
            advertised_models: Vec::new(),
        })
    }

    fn initialize(&mut self) -> Result<Value, RpcError> {
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
        // Cursor (and any adapter that advertises a method) expects
        // `authenticate` before `session/new`. Skip when the list is empty so
        // Claude/Codex/Pi keep the handshake they already pass.
        self.authenticate_if_needed(&result)?;
        self.initialized = true;
        self.capabilities = parse_capabilities(&result);
        Ok(result)
    }

    fn authenticate_if_needed(&mut self, init: &Value) -> Result<(), RpcError> {
        let methods = match init.get("authMethods").and_then(Value::as_array) {
            Some(methods) if !methods.is_empty() => methods,
            _ => return Ok(()),
        };
        let method_id = methods
            .iter()
            .find_map(|method| {
                method
                    .get("id")
                    .or_else(|| method.get("methodId"))
                    .and_then(Value::as_str)
            })
            .ok_or_else(|| {
                RpcError::Internal("adapter advertised authMethods without an id".into())
            })?;
        self.request(
            "authenticate",
            json!({ "methodId": method_id }),
            INITIALIZE_TIMEOUT,
        )?;
        Ok(())
    }

    /// Best-effort live model switch. Tries the documented ACP verbs in order.
    fn apply_model_verbs(&mut self, session_id: &str, model_id: &str) -> Result<(), RpcError> {
        let attempts = [
            (
                "session/set_model",
                json!({ "sessionId": session_id, "modelId": model_id }),
            ),
            (
                "session/set_config",
                json!({
                    "sessionId": session_id,
                    "configId": "model",
                    "value": model_id
                }),
            ),
            (
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": "model",
                    "value": model_id
                }),
            ),
        ];
        let mut last_err = None;
        for (method, params) in attempts {
            match self.request(method, params, Duration::from_secs(2)) {
                Ok(_) => return Ok(()),
                Err(err) => last_err = Some(err),
            }
        }
        Err(last_err
            .unwrap_or_else(|| RpcError::Internal("no model apply method succeeded".into())))
    }

    /// ACP `session/resume`: hand the agent back a session it already has.
    ///
    /// Restores context **without** replaying history, which is what makes it
    /// the right verb for a thread whose transcript we already hold. The same
    /// absolute `cwd` and the same MCP list go back out: resume is a
    /// continuation of one job, and a session that comes back pointed at a
    /// different directory or holding different tools is a different job
    /// (`keep-alive.md`, "Resume recipe").
    fn resume_session(
        &mut self,
        thread_id: &str,
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
        self.adopt(thread_id, session_id);
        Ok(())
    }

    /// ACP `session/load`: the agent replays the whole conversation at us.
    ///
    /// The replay arrives as `session/update` notifications *before* this
    /// returns, so the caller decides what happens to them — a thread with no
    /// transcript of its own wants them, and a thread that has one would get
    /// every message twice (`keep-alive.md` step 4).
    fn load_session(
        &mut self,
        thread_id: &str,
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
        self.adopt(thread_id, session_id);
        Ok(())
    }

    /// The thread a `sessionId`-bearing payload belongs to.
    ///
    /// One tenant is the overwhelmingly common case and an adapter that omits
    /// `sessionId` is within its rights on a process serving a single session,
    /// so that is answered without the field. With two, a payload that names
    /// nothing is genuinely unroutable.
    fn owner_of_session(&self, params: &Value) -> Vec<String> {
        match params.get("sessionId").and_then(Value::as_str) {
            Some(session_id) => self
                .by_session
                .get(session_id)
                .cloned()
                .into_iter()
                .collect(),
            None if self.sessions.len() == 1 => self.tenants(),
            None => Vec::new(),
        }
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

impl SessionBackend for AcpConnection {
    fn kind(&self) -> BackendKind {
        BackendKind::Acp
    }

    fn handshake(&mut self) -> Result<Value, RpcError> {
        self.initialize()
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities
    }

    fn create_session(
        &mut self,
        thread_id: &str,
        cwd: &str,
        mcp_servers: Value,
        model: Option<&str>,
    ) -> Result<String, RpcError> {
        self.initialize()?;
        let mut params = json!({
            "cwd": cwd,
            "mcpServers": mcp_servers
        });
        if let Some(model) = model {
            params["model"] = json!(model);
            // Some Claude adapter builds read the pin from `_meta` rather than
            // the ACP `model` field. Both go out; neither invents an id.
            params["_meta"] = json!({ "claudeCode": { "options": { "model": model } } });
        }
        let result = self.request("session/new", params, SESSION_NEW_TIMEOUT)?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::Internal("session/new did not return sessionId".into()))?
            .to_string();
        self.adopt(thread_id, &session_id);
        if let Some(models) = advertised_models(&result) {
            self.advertised_models = models;
        }
        if let Some(model) = model {
            // Claude: `session/set_model`. OpenCode: `session/set_config`.
            // Older builds answer method-not-found and keep the spawn pin.
            let _ = self.apply_model_verbs(&session_id, model);
        }
        Ok(session_id)
    }

    fn apply_model(&mut self, session_id: &str, model_id: &str) -> Result<(), RpcError> {
        self.apply_model_verbs(session_id, model_id)
    }

    fn take_advertised_models(&mut self) -> Vec<String> {
        std::mem::take(&mut self.advertised_models)
    }

    /// Resume first, then load, then — only then — tell the caller neither
    /// worked. The order is the research's own (`keep-alive.md`): resume
    /// restores context without a replay, which is right for a thread whose
    /// transcript the host already holds; load is the fallback for an adapter
    /// that only speaks that. A verb that was advertised and then refused
    /// falls through rather than giving up, because the adapter still has the
    /// conversation.
    fn restore_session(
        &mut self,
        thread_id: &str,
        session_id: &str,
        cwd: &str,
        mcp_servers: Value,
    ) -> Result<Restored, RpcError> {
        // Capabilities are only knowable after the handshake, and the
        // handshake failing is the "install hint" case, not a resume case.
        self.initialize()?;
        let capabilities = self.capabilities;
        if capabilities.resume {
            match self.resume_session(thread_id, session_id, cwd, mcp_servers.clone()) {
                Ok(()) => return Ok(Restored::Resumed),
                Err(err) => eprintln!("session/resume for {thread_id} failed: {err}"),
            }
        }
        if capabilities.load {
            return match self.load_session(thread_id, session_id, cwd, mcp_servers) {
                Ok(()) => Ok(Restored::Loaded),
                Err(err) => Ok(Restored::Unsupported(Some(err.to_string()))),
            };
        }
        Ok(Restored::Unsupported(None))
    }

    /// ACP `session/close`. Frees the agent's own resources before we drop the
    /// process; skipped, not faked, when the adapter never advertised it.
    fn close_session(&mut self, session_id: &str) -> Result<(), RpcError> {
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
    /// arrives later as a `TurnEnded` event (ACP v1 returns a stop reason; the
    /// host API returns as soon as the agent has accepted the write).
    fn send_prompt(
        &mut self,
        thread_id: &str,
        session_id: &str,
        content: &Value,
    ) -> Result<TurnId, RpcError> {
        let prompt = prompt_blocks(content)?;
        let id = self.next_id();
        let turn = TurnId(RequestId::Number(id));
        // Before the write, not after: the response can be read by the reader
        // thread and queued the instant the bytes land, and an owner recorded
        // afterwards would be a race a shared process loses by misrouting a
        // turn to a neighbour chat.
        self.prompt_owners
            .insert(turn.clone(), thread_id.to_string());
        let sent = self.write_request(
            id,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": prompt
            }),
        );
        if let Err(err) = sent {
            self.prompt_owners.remove(&turn);
            return Err(err);
        }
        Ok(turn)
    }

    /// ACP v1 `session/cancel` is a notification.
    fn cancel(&mut self, session_id: &str) -> Result<(), RpcError> {
        self.write_notification("session/cancel", json!({ "sessionId": session_id }))
    }

    fn respond(&self, id: InteractionId, result: Value) -> Result<(), RpcError> {
        let response = JsonRpcResponse::success(id.0, result);
        self.write_message(&JsonRpcMessage::Response(response))
    }

    /// Refuse a request the agent made. `-32601` is what a request nobody
    /// implements gets, and the one answer that lets Cursor fall back to
    /// what it can do without us (#298).
    fn respond_error(&self, id: InteractionId, code: i64, message: &str) -> Result<(), RpcError> {
        let error = JsonRpcError {
            code,
            message: message.to_string(),
            data: None,
        };
        let response = JsonRpcResponse::failure(id.0, error);
        self.write_message(&JsonRpcMessage::Response(response))
    }

    // ---- tenancy ---------------------------------------------------------

    /// Both directions, because both are asked: the outbound path needs a
    /// session for a thread, and every inbound event needs a thread for a
    /// session. A thread that re-attaches (resume after a drop, or a load that
    /// followed a refused resume) replaces its old entry rather than adding
    /// one, so the reverse map cannot accumulate a stale session pointing at a
    /// thread that has moved on.
    fn adopt(&mut self, thread_id: &str, session_id: &str) {
        if let Some(previous) = self
            .sessions
            .insert(thread_id.to_string(), session_id.to_string())
        {
            if previous != session_id {
                self.by_session.remove(&previous);
            }
        }
        self.by_session
            .insert(session_id.to_string(), thread_id.to_string());
    }

    fn session_for(&self, thread_id: &str) -> Option<String> {
        self.sessions.get(thread_id).cloned()
    }

    fn tenants(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    /// Its outstanding prompt goes too: a response that arrives for a thread
    /// that has left must not be handed to whoever is still here.
    fn release(&mut self, thread_id: &str) -> Option<String> {
        self.prompt_owners
            .retain(|_, owner| owner.as_str() != thread_id);
        let session_id = self.sessions.remove(thread_id)?;
        self.by_session.remove(&session_id);
        Some(session_id)
    }

    /// A connection with no tenants is not necessarily new — `ensure_connection`
    /// spawns before `session/new` runs — so this is asked only where a thread
    /// was just released.
    fn is_vacant(&self) -> bool {
        self.sessions.is_empty()
    }

    /// `Closed` is everybody's: the process is gone and every thread on it has
    /// lost its adapter. The rest name one thread, or none — and none means
    /// *drop it*, not "give it to whoever". Misattributing a neighbour's tool
    /// call to this chat would write it into the wrong transcript permanently,
    /// which is worse than losing an event that already has nowhere to go.
    fn route(&mut self, event: &BackendEvent) -> Vec<String> {
        match event {
            BackendEvent::Closed { .. } => self.tenants(),
            BackendEvent::TurnEnded { turn, .. } => {
                self.prompt_owners.remove(turn).into_iter().collect()
            }
            BackendEvent::Update(params) => self.owner_of_session(params),
            BackendEvent::Interaction { params, .. } => self.owner_of_session(params),
            // Cursor's extensions carry no `sessionId`, which is fine on the
            // one-session-per-process it is catalogued as; on a shared process
            // this is unroutable, and the pump answers it rather than hanging
            // the turn.
            BackendEvent::Extension { params, .. } => self.owner_of_session(params),
        }
    }

    // ---- liveness --------------------------------------------------------

    fn try_recv(&mut self) -> Result<BackendEvent, TryRecvError> {
        self.inbound_rx.try_recv()
    }

    /// The supervisor cannot ask the reader thread: EOF on stdout is what
    /// tells it a child is gone, and a child that forked something holding the
    /// same stdout leaves that pipe open after it exits. So the read loop
    /// blocks on a pipe nobody will ever write to while the adapter itself is
    /// a corpse — which is a session JaBot would keep reporting as live
    /// forever. Reaping the pid is the only answer that cannot lie.
    fn is_alive(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn log_path(&self) -> &Path {
        &self.log_path
    }

    fn kill(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        terminate_process_group(&mut self.child);
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
fn parse_capabilities(result: &Value) -> BackendCapabilities {
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
    BackendCapabilities {
        load: flag(agent, "loadSession"),
        resume: flag(session, "resume"),
        close: flag(session, "close"),
    }
}

pub(crate) fn prompt_blocks(content: &Value) -> Result<Value, RpcError> {
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
    inbound: Sender<BackendEvent>,
    wake: Arc<AdapterWake>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = inbound.send(BackendEvent::Closed {
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
                let _ = inbound.send(BackendEvent::Closed {
                    error: Some(format!("adapter stdout: {err}")),
                });
                wake.ping();
                break;
            }
        }
    }
}

/// Hoist the spec's `session/update` shape onto the one the host reads.
///
/// ACP sends `{ sessionId, update: { sessionUpdate, ... } }`. Everything
/// downstream — `has_reply`, the preview, the lifecycle, the renderer's
/// reducer — reads `sessionUpdate` and `content` off the top level, which is
/// also what the fake agent used to send. A real Claude Code turn therefore
/// streamed its text into a shape nobody looked at and ended as
/// `empty_response` with the reply sitting in the transcript rows. Flattening
/// here, at the wire, is the one place every consumer shares. An already-flat
/// payload (no `update` object) passes through untouched; a top-level key
/// wins over the same key inside `update`, so `sessionId` stays the routing
/// key.
fn flatten_session_update(params: Value) -> Value {
    let Value::Object(mut outer) = params else {
        return params;
    };
    let Some(Value::Object(inner)) = outer.remove("update") else {
        return Value::Object(outer);
    };
    if !inner.contains_key("sessionUpdate") {
        // Not the spec envelope — some other field that happens to be named
        // `update`. Put it back and leave the payload alone.
        outer.insert("update".to_string(), Value::Object(inner));
        return Value::Object(outer);
    }
    for (key, value) in inner {
        outer.entry(key).or_insert(value);
    }
    Value::Object(outer)
}

fn dispatch_message(
    message: JsonRpcMessage,
    stdin: &Arc<Mutex<std::process::ChildStdin>>,
    pending: &Arc<Mutex<HashMap<i64, Sender<JsonRpcResponse>>>>,
    inbound: &Sender<BackendEvent>,
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
                let _ = inbound.send(BackendEvent::TurnEnded {
                    turn: TurnId(RequestId::Number(id)),
                    payload,
                });
                wake.ping();
            }
        }
        JsonRpcMessage::Notification(notification) => {
            if notification.method == "session/update" {
                let params = flatten_session_update(notification.params.unwrap_or(Value::Null));
                let _ = inbound.send(BackendEvent::Update(params));
                wake.ping();
            }
        }
        JsonRpcMessage::Request(request) => {
            if request.method == "session/request_permission" {
                let _ = inbound.send(BackendEvent::Interaction {
                    id: InteractionId(request.id),
                    params: request.params.unwrap_or(Value::Null),
                });
                wake.ping();
            } else if super::extensions::is_supported(&request.method) {
                // A blocking extension the host renders (#298). Raw here: the
                // host reads it, and refuses it with `-32601` if it cannot.
                let _ = inbound.send(BackendEvent::Extension {
                    id: InteractionId(request.id),
                    method: request.method,
                    params: request.params.unwrap_or(Value::Null),
                });
                wake.ping();
            } else {
                // Everything else an agent asks of us — `cursor/update_todos`,
                // whatever the next extension is — is refused at the wire.
                // Method-not-found is the JSON-RPC answer for exactly this,
                // and the one a well-behaved agent treats as "the client
                // cannot", so a turn never hangs on a request nobody rendered
                // and no support is claimed for one nobody implemented.
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
    fn spec_shaped_session_update_is_hoisted() {
        // The shape claude-agent-acp actually sends: the discriminator and
        // the content live under `update`. Before this hoist a real reply
        // ended the turn as `empty_response`.
        let flat = flatten_session_update(json!({
            "sessionId": "s1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "Hi!" },
                "messageId": "m1"
            }
        }));
        assert_eq!(flat["sessionId"], "s1");
        assert_eq!(flat["sessionUpdate"], "agent_message_chunk");
        assert_eq!(flat["content"]["text"], "Hi!");
        assert_eq!(flat["messageId"], "m1");
        assert!(flat.get("update").is_none());
    }

    #[test]
    fn flat_session_update_passes_through() {
        let already = json!({
            "sessionId": "s1",
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "hello from fake-acp" }
        });
        assert_eq!(flatten_session_update(already.clone()), already);
        // An `update` that is not the envelope is not touched either.
        let other = json!({ "sessionId": "s1", "sessionUpdate": "x", "update": { "n": 1 } });
        assert_eq!(flatten_session_update(other.clone()), other);
        assert_eq!(flatten_session_update(Value::Null), Value::Null);
    }

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
            BackendCapabilities {
                load: true,
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
        assert!(!hoisted.load);
    }

    #[test]
    fn an_unadvertised_capability_is_no() {
        // The failure this rules out is a `session/resume` that comes back
        // "method not found" on a thread the user was told had been restored.
        let silent = parse_capabilities(&json!({ "protocolVersion": 1 }));
        assert_eq!(silent, BackendCapabilities::default());
        let junk = parse_capabilities(&json!("not even an object"));
        assert_eq!(junk, BackendCapabilities::default());
        // A non-boolean is not a yes either.
        let lying = parse_capabilities(&json!({
            "agentCapabilities": { "loadSession": "yes" }
        }));
        assert!(!lying.load);
    }

    /// The reader thread's split of an agent's requests (#298): permissions
    /// and the rendered extensions reach the host; everything else is refused
    /// at the wire, so no support is ever claimed for a method nobody drew.
    #[test]
    fn only_rendered_extensions_are_carried_to_the_host() {
        assert!(super::super::extensions::is_supported(
            "cursor/ask_question"
        ));
        assert!(super::super::extensions::is_supported("cursor/create_plan"));
        assert!(!super::super::extensions::is_supported(
            "cursor/update_todos"
        ));
        assert!(!super::super::extensions::is_supported(
            "session/request_permission"
        ));
    }
}
