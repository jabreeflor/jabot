//! In-process host session: identity, JSON-RPC router, outbound notifications.
//!
//! The UI never talks to ACP stdio. It sends JSON-RPC frames through this
//! session — Tauri IPC now, Unix socket later, same messages.

mod acp;
mod chief;
mod crew;
mod git;
mod harness;
mod identity;
mod lifecycle;
mod log;
mod permission;
mod procgroup;
mod protocol;
mod repo;
mod router;
mod schedule;
mod seq;
mod store;
mod supervisor;
mod tools;
mod transcript;

#[allow(unused_imports)]
pub use acp::AdapterWake;
#[allow(unused_imports)]
pub use crew::{is_known_tool, BOT_COLORS, HOST_TOOLS};
#[allow(unused_imports)]
pub use git::{Release, ThreadWorktree};
#[allow(unused_imports)]
pub use harness::{catalog::HarnessDescriptor, doctor::ProbeHost, resolve_command};
#[allow(unused_imports)]
pub use identity::{DeviceRecord, HostIdentity};
#[allow(unused_imports)]
pub use lifecycle::{
    ledger::RunState,
    process::AcpState,
    receipt::{drift, DriftField, SessionFingerprint},
    state::ThreadState,
};
#[allow(unused_imports)]
pub use protocol::{
    decode_frame, decode_frames, encode_frame, BotTemplateView, BotView, ChiefInvokeParams,
    ChiefInvokeResult, ChiefToolView, ChiefToolsResult, CrewCreateParams, CrewHostToolView,
    CrewListResult, CrewRefParams, CrewRemoveResult, CrewUpdateParams, DeviceInfo, DeviceRole,
    Envelope, FolderForgetResult, FolderListResult, FolderOriginView, FolderThreadView, FolderView,
    GithubStatusResult, HarnessCardView, HarnessDoctorResult, HarnessListResult, HarnessStatus,
    HarnessTier, HealthResult, HelloParams, HelloResult, JsonRpcError, JsonRpcMessage,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, PendingPermissionView,
    PermissionPendingParams, PermissionPendingResult, PermissionReplyParams, PermissionReplyResult,
    PromptMode, QueuedPromptView, RequestId, ResumeOutcome, ResurfaceReason, RpcError,
    ScheduleCreateParams, ScheduleFireView, ScheduleListParams, ScheduleListResult,
    ScheduleRefParams, ScheduleRemoveResult, ScheduleRunResult, ScheduleUpdateParams, ScheduleView,
    StoreStatus, SupervisorStatusResult, ThreadResumeResult, ThreadStateResult,
    ThreadTranscriptParams, ThreadTranscriptResult, ToolCardView, ToolConnectResult,
    ToolConnectionStatus, ToolDisconnectResult, ToolListResult, ToolRefParams, ToolTransport,
    TranscriptEventView, CHIEF_INVOKE, CHIEF_TOOLS, CLIENT_METHODS, CREW_CREATE, CREW_LIST,
    CREW_REMOVE, CREW_THREAD, CREW_UPDATE, FOLDER_FORGET, FOLDER_LIST, FOLDER_REGISTER,
    FOLDER_UPDATE, GITHUB_STATUS, HARNESS_DOCTOR, HARNESS_LIST, HOST_HEALTH, HOST_HELLO,
    HOST_NOTIFICATIONS, INBOX_LIST, INBOX_RESURFACE, JSONRPC_VERSION, PERMISSION_ASK,
    PERMISSION_PENDING, PERMISSION_REPLY, PERMISSION_RESOLVED, PROTOCOL_VERSION, SCHEDULE_CREATE,
    SCHEDULE_LIST, SCHEDULE_REMOVE, SCHEDULE_RUN, SCHEDULE_UPDATE, SESSION_CANCEL, SESSION_PROMPT,
    SESSION_UPDATE, SUPERVISOR_STATUS, THREAD_ARCHIVE, THREAD_DELETE, THREAD_FOLD, THREAD_OPEN,
    THREAD_REOPEN, THREAD_RESUME, THREAD_STATE, THREAD_TRANSCRIPT, TOOLS_CONNECT, TOOLS_DISCONNECT,
    TOOLS_LIST,
};
#[allow(unused_imports)]
pub use repo::{gh::GhAuth, git::RepoProbe, origin::Origin};
#[allow(unused_imports)]
pub use schedule::Scheduler;
#[allow(unused_imports)]
pub use store::{NewFolder, NewThread, Secrets, Store, StoreError, ThreadRepo, ThreadRow};
#[allow(unused_imports)]
pub use supervisor::{ResumeReadiness, Supervisor, DEFAULT_SLEEP_GAP};
#[allow(unused_imports)]
pub use tools::catalog::CATALOG as TOOL_CATALOG;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use identity::HostIdentity as Identity;
use lifecycle::LifecycleState;
use log::EventLog;
use protocol::methods::{
    InboxResurfaceParams, LoggedEvent, PermissionAskParams, PermissionResolvedParams,
    ResumeFromParams, ResumeFromResult, SessionUpdateParams,
};
use schedule::Scheduler as SchedulerState;
use seq::SeqStore;
use supervisor::Supervisor as SupervisorState;

/// Process placement. In-process until a second client exists (#4).
pub const HOST_MODE: &str = "in-process";

#[derive(Debug)]
pub struct HostSession {
    identity: Identity,
    connected_device_id: Option<String>,
    seq: SeqStore,
    events: EventLog,
    outbound: VecDeque<JsonRpcNotification>,
    store: Option<Store>,
    secrets: Secrets,
    store_error: Option<String>,
    connections: HashMap<String, acp::AcpConnection>,
    /// Live asks, by the `requestId` the client answers with. The durable half
    /// is `permission_requests`; this holds the adapter call blocked on each
    /// one, which is the part that cannot outlive the process (#20).
    pending_permissions: HashMap<String, permission::PendingPermission>,
    wake: Arc<acp::AdapterWake>,
    log_dir: PathBuf,
    /// Tier-3 harness JSON. `None` on an ephemeral host: with no data
    /// directory there is nowhere for a user file to have been put.
    custom_harness_dir: Option<PathBuf>,
    /// The app data directory, for anything that is neither SQLite nor a
    /// secret: OAuth client registrations, MCP browser profiles (#18).
    data_dir: Option<PathBuf>,
    /// OAuth flows waiting on a browser, keyed by provider. Not persisted: a
    /// consent window does not survive a quit, and pretending otherwise would
    /// leave a chip saying "connecting" after a restart.
    connect_flows: HashMap<String, tools::ConnectFlow>,
    /// Which thread holds each JaBot-owned MCP profile directory, by catalog
    /// id. A `--user-data-dir` is a lock — one Playwright process at a time —
    /// so this is a lease, not a cache (#18).
    mcp_profiles: HashMap<String, String>,
    /// Prompts held for a turn already in flight, oldest first (#14). RAM, in
    /// the same spirit as `connections`: a queued prompt has not been said to
    /// the agent, so nothing durable should claim it has.
    prompt_queue: HashMap<String, VecDeque<transcript::queue::QueuedPrompt>>,
    lifecycle: LifecycleState,
    /// Keep-alive, resume, and what this launch found on the ledger (#21).
    supervisor: SupervisorState,
    /// The in-process cron (#25). Decision #4 rules out a daemon, so this is
    /// the only clock a schedule has — and why a missed slot is reconciled at
    /// launch rather than replayed.
    scheduler: SchedulerState,
    /// The loopback MCP server that carries Chief's host tools into its ACP
    /// session (#24). Started on the first session that has a host-tool chip,
    /// not at boot: a host nobody has prompted should not be listening.
    chief: Option<chief::ChiefEndpoint>,
    /// Whether a host tool is running right now. A tool can prompt, prompting
    /// pumps, and pumping drains the tool queue — this is what stops a call
    /// from being answered inside another one.
    chief_draining: bool,
}

impl HostSession {
    pub fn ephemeral() -> Self {
        Self::with_identity(Identity::generate())
    }

    /// Open identity + SQLite under the app data directory. Store failures are
    /// surfaced on hello/health (`storeError`) instead of crashing the app.
    pub fn load(data_dir: &Path) -> Self {
        let identity = match Identity::load_or_create(&data_dir.join("host-identity.json")) {
            Ok(identity) => identity,
            Err(err) => {
                eprintln!(
                    "failed to persist host identity at {}: {err}",
                    data_dir.display()
                );
                Identity::generate()
            }
        };
        let mut session = Self::with_identity(identity)
            .with_store_at(&data_dir.join("jabot.sqlite"))
            .with_log_dir(data_dir.join("adapter-logs"));
        session.custom_harness_dir = Some(data_dir.join("custom_harnesses"));
        session.data_dir = Some(data_dir.to_path_buf());
        // Custom harnesses become rows now rather than on first list: New Chat
        // may open a thread on one before anything asks for the catalog, and
        // `threads.harness_id` is a foreign key.
        session.sync_harness_catalog();
        // Before anything can ask. A client that says hello and immediately
        // lists the Inbox has to see the same answer as one that connects an
        // hour later — and until this runs, the ledger still claims runs are
        // in flight for a process that no longer exists (#21).
        session.reconcile_boot();
        // After the ledger, because the sweep asks the store which threads
        // still claim a tree — and before anything can open a new one, so a
        // directory left by the last launch is collected rather than colliding
        // with the thread that reuses its path (#23).
        session.sweep_worktrees();
        // And last, because it reads the ledger the boot pass has just closed
        // out: a schedule whose run was lost with the host gets its card here,
        // and a slot that came round while the app was shut is caught up once
        // rather than replayed (#25). Not a second boot path — this is the
        // same reconcile the pump runs, taken once before anything can ask.
        session.reconcile_schedules();
        session
    }

    fn with_log_dir(mut self, log_dir: PathBuf) -> Self {
        self.log_dir = log_dir;
        self
    }

    pub fn with_identity(identity: Identity) -> Self {
        let log_dir = std::env::temp_dir()
            .join("jabot-adapter-logs")
            .join(&identity.host_id);
        Self {
            identity,
            connected_device_id: None,
            seq: SeqStore::default(),
            events: EventLog::default(),
            outbound: VecDeque::new(),
            store: None,
            secrets: Secrets::memory(),
            store_error: None,
            connections: HashMap::new(),
            pending_permissions: HashMap::new(),
            wake: acp::AdapterWake::new(),
            log_dir,
            custom_harness_dir: None,
            data_dir: None,
            connect_flows: HashMap::new(),
            mcp_profiles: HashMap::new(),
            prompt_queue: HashMap::new(),
            lifecycle: LifecycleState::from_env(),
            supervisor: SupervisorState::from_env(),
            scheduler: SchedulerState::from_env(),
            chief: None,
            chief_draining: false,
        }
    }

    fn with_store_at(mut self, sqlite_path: &Path) -> Self {
        match Store::open(sqlite_path) {
            Ok(store) => {
                self.store = Some(store);
                self.secrets = Secrets::platform();
                self.store_error = None;
            }
            Err(err) => {
                eprintln!(
                    "failed to open sqlite store at {}: {err}",
                    sqlite_path.display()
                );
                self.store = None;
                self.secrets = Secrets::platform();
                self.store_error = Some(err.to_string());
            }
        }
        self
    }

    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    pub fn checkpoint_store(&mut self) {
        if let Some(store) = &self.store {
            if let Err(err) = store.checkpoint() {
                eprintln!("failed to checkpoint sqlite store: {err}");
            }
        }
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        router::dispatch(self, request)
    }

    pub fn take_outbound(&mut self) -> Vec<JsonRpcNotification> {
        self.outbound.drain(..).collect()
    }

    pub fn require_hello(&self) -> Result<(), RpcError> {
        if self.connected_device_id.is_some() {
            Ok(())
        } else {
            Err(RpcError::HelloRequired)
        }
    }

    pub fn hello(&mut self, params: HelloParams) -> Result<HelloResult, RpcError> {
        let requested = params.protocol_version.unwrap_or(PROTOCOL_VERSION);
        if requested != PROTOCOL_VERSION {
            return Err(RpcError::ProtocolMismatch {
                requested,
                supported: PROTOCOL_VERSION,
            });
        }

        let device_id = params
            .device
            .as_ref()
            .and_then(|d| d.device_id.as_deref())
            .map(str::trim)
            .filter(|id| !id.is_empty());

        match device_id {
            None => {
                self.connected_device_id = Some(self.identity.local_device.device_id.clone());
            }
            Some(id) if id == self.identity.local_device.device_id => {
                if let Some(device) = params.device.as_ref() {
                    if let Some(name) = device.name.as_ref() {
                        if !name.trim().is_empty() {
                            self.identity.local_device.name = name.clone();
                        }
                    }
                }
                self.connected_device_id = Some(id.to_string());
            }
            Some(_) => return Err(RpcError::UnpairedDevice),
        }

        Ok(self.hello_result())
    }

    pub fn health(&self) -> HealthResult {
        HealthResult {
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
            host_mode: HOST_MODE.to_string(),
            host_id: self.identity.host_id.clone(),
            protocol_version: PROTOCOL_VERSION,
            connected: self.connected_device_id.is_some(),
            device_id: self.connected_device_id.clone(),
            store: self.store_status(),
            store_error: self.store_error.clone(),
        }
    }

    pub fn resume_from(&self, params: ResumeFromParams) -> ResumeFromResult {
        ResumeFromResult {
            thread_id: params.thread_id.clone(),
            head_seq: self.seq.head(&params.thread_id),
            events: self.events.after(&params.thread_id, params.seq),
        }
    }

    pub fn notify_session_update(&mut self, thread_id: &str, acp: Value) -> u64 {
        self.notify_session_update_at(thread_id, acp, None)
    }

    /// The same notification, stamped with the `transcript_events` row this
    /// event landed in (#14). A client hydrating from `thread/transcript`
    /// compares that seq against the head it was given to know whether a live
    /// event is one it has already replayed; the envelope `seq` cannot answer
    /// that, because it counts permission and resurface events too.
    pub fn notify_session_update_at(
        &mut self,
        thread_id: &str,
        acp: Value,
        transcript_seq: Option<i64>,
    ) -> u64 {
        let seq = self.seq.next(thread_id);
        let params = SessionUpdateParams {
            host_id: self.identity.host_id.clone(),
            thread_id: thread_id.to_string(),
            seq,
            acp,
            transcript_seq,
        };
        self.push_logged(thread_id, protocol::SESSION_UPDATE, params);
        seq
    }

    pub fn notify_permission_ask(
        &mut self,
        thread_id: &str,
        request_id: &str,
        subject: Value,
        options: Value,
    ) -> u64 {
        let seq = self.seq.next(thread_id);
        let params = PermissionAskParams {
            host_id: self.identity.host_id.clone(),
            thread_id: thread_id.to_string(),
            seq,
            request_id: request_id.to_string(),
            subject,
            options,
        };
        self.push_logged(thread_id, protocol::PERMISSION_ASK, params);
        seq
    }

    pub fn notify_permission_resolved(
        &mut self,
        thread_id: &str,
        request_id: &str,
        device_id: &str,
        option_id: Option<String>,
        cancelled: Option<bool>,
    ) -> u64 {
        let seq = self.seq.next(thread_id);
        let params = PermissionResolvedParams {
            host_id: self.identity.host_id.clone(),
            thread_id: thread_id.to_string(),
            seq,
            request_id: request_id.to_string(),
            device_id: device_id.to_string(),
            option_id,
            cancelled,
        };
        self.push_logged(thread_id, protocol::PERMISSION_RESOLVED, params);
        seq
    }

    pub fn notify_inbox_resurface(&mut self, thread_id: &str, reason: ResurfaceReason) -> u64 {
        let seq = self.seq.next(thread_id);
        let params = InboxResurfaceParams {
            host_id: self.identity.host_id.clone(),
            thread_id: thread_id.to_string(),
            seq,
            reason,
        };
        self.push_logged(thread_id, protocol::INBOX_RESURFACE, params);
        seq
    }

    fn push_logged<T: serde::Serialize>(&mut self, thread_id: &str, method: &str, params: T) {
        let params = serde_json::to_value(params).expect("notification params serialize");
        self.events.push(
            thread_id,
            LoggedEvent {
                seq: params
                    .get("seq")
                    .and_then(Value::as_u64)
                    .expect("envelope seq"),
                method: method.to_string(),
                params: params.clone(),
            },
        );
        self.outbound
            .push_back(JsonRpcNotification::new(method, Some(params)));
    }

    fn hello_result(&self) -> HelloResult {
        HelloResult {
            protocol_version: PROTOCOL_VERSION,
            host_id: self.identity.host_id.clone(),
            host_name: self.identity.host_name.clone(),
            host_mode: HOST_MODE.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
            device: self.identity.local_device_info(),
            methods: CLIENT_METHODS.iter().map(|m| (*m).to_string()).collect(),
            notifications: HOST_NOTIFICATIONS
                .iter()
                .map(|m| (*m).to_string())
                .collect(),
            store: self.store_status(),
            store_error: self.store_error.clone(),
        }
    }

    fn store_status(&self) -> Option<StoreStatus> {
        let store = self.store.as_ref()?;
        store.status(&self.secrets).ok().map(|status| StoreStatus {
            path: status.path,
            schema_version: status.schema_version,
            sqlite_version: status.sqlite_version,
            journal_mode: status.journal_mode,
            secrets_backend: status.secrets_backend,
            harness_count: status.harness_count,
            bot_count: status.bot_count,
        })
    }
}

impl Drop for HostSession {
    fn drop(&mut self) {
        self.shutdown_adapters();
    }
}

#[cfg(test)]
mod tests {
    use super::protocol::error::{
        HARNESS_UNAVAILABLE, HELLO_REQUIRED, INVALID_PARAMS, METHOD_NOT_FOUND, PROTOCOL_MISMATCH,
        UNPAIRED_DEVICE,
    };
    use super::protocol::jsonrpc::RequestId;
    use super::protocol::{
        INBOX_RESURFACE, PERMISSION_REPLY, SESSION_PROMPT, SESSION_UPDATE, SYNC_RESUME_FROM,
    };
    use super::*;
    use serde_json::json;

    fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(RequestId::Number(id), method, params)
    }

    fn result_value(response: &JsonRpcResponse) -> &Value {
        response.result.as_ref().expect("expected result")
    }

    #[test]
    fn hello_binds_local_device() {
        let mut session = HostSession::ephemeral();
        let response = session.handle_request(req(1, HOST_HELLO, Some(json!({}))));
        let value = result_value(&response);
        assert_eq!(value["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(value["hostMode"], HOST_MODE);
        assert_eq!(
            value["device"]["deviceId"],
            session.identity.local_device.device_id
        );
        assert_eq!(value["methods"][0], HOST_HELLO);
        assert!(session.connected_device_id.is_some());
    }

    #[test]
    fn hello_rejects_unknown_device() {
        let mut session = HostSession::ephemeral();
        let response = session.handle_request(req(
            1,
            HOST_HELLO,
            Some(json!({
                "protocolVersion": 1,
                "device": { "deviceId": "phone-not-paired-yet" }
            })),
        ));
        let error = response.error.expect("unpaired");
        assert_eq!(error.code, UNPAIRED_DEVICE);
        assert!(session.connected_device_id.is_none());
    }

    #[test]
    fn hello_rejects_other_protocol_version() {
        let mut session = HostSession::ephemeral();
        let response =
            session.handle_request(req(1, HOST_HELLO, Some(json!({ "protocolVersion": 99 }))));
        let error = response.error.expect("mismatch");
        assert_eq!(error.code, PROTOCOL_MISMATCH);
    }

    #[test]
    fn prompt_requires_hello_then_validates() {
        let mut session = HostSession::ephemeral();
        let missing = session.handle_request(req(
            1,
            SESSION_PROMPT,
            Some(json!({ "threadId": "t1", "content": "hi" })),
        ));
        assert_eq!(missing.error.unwrap().code, HELLO_REQUIRED);

        session
            .handle_request(req(2, HOST_HELLO, None))
            .result
            .expect("hello");

        let bad = session.handle_request(req(
            3,
            SESSION_PROMPT,
            Some(json!({ "threadId": "", "content": "hi" })),
        ));
        assert_eq!(bad.error.unwrap().code, INVALID_PARAMS);

        let no_runtime = session.handle_request(req(
            4,
            SESSION_PROMPT,
            Some(json!({ "threadId": "t1", "content": "hi" })),
        ));
        assert_eq!(no_runtime.error.unwrap().code, INVALID_PARAMS);
    }

    #[test]
    fn missing_harness_is_unavailable_not_a_crash() {
        let mut session = HostSession::ephemeral();
        session
            .handle_request(req(1, HOST_HELLO, None))
            .result
            .expect("hello");
        let response = session.handle_request(req(
            2,
            SESSION_PROMPT,
            Some(json!({
                "threadId": "t-missing",
                "content": "hi",
                "runtime": {
                    "command": "jabot-definitely-not-on-path-xyz",
                    "installHint": "brew install nope"
                }
            })),
        ));
        let err = response.error.unwrap();
        assert_eq!(err.code, HARNESS_UNAVAILABLE);
        assert_eq!(err.data.unwrap()["installHint"], "brew install nope");
    }

    #[test]
    fn unknown_method() {
        let mut session = HostSession::ephemeral();
        let response = session.handle_request(req(1, "nope/nope", None));
        assert_eq!(response.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn session_update_envelope_and_resume_from() {
        let mut session = HostSession::ephemeral();
        session.handle_request(req(1, HOST_HELLO, None));

        let seq1 = session.notify_session_update(
            "thread-a",
            json!({ "sessionUpdate": "agent_message_chunk" }),
        );
        let seq2 = session.notify_inbox_resurface("thread-a", ResurfaceReason::NeedsYou);
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        let outbound = session.take_outbound();
        assert_eq!(outbound.len(), 2);
        assert_eq!(outbound[0].method, SESSION_UPDATE);
        assert_eq!(
            outbound[0].params.as_ref().unwrap()["hostId"],
            session.identity.host_id
        );
        assert_eq!(outbound[0].params.as_ref().unwrap()["seq"], 1);

        let replay = session.handle_request(req(
            2,
            SYNC_RESUME_FROM,
            Some(json!({ "threadId": "thread-a", "seq": 1 })),
        ));
        let value = result_value(&replay);
        assert_eq!(value["headSeq"], 2);
        assert_eq!(value["events"].as_array().unwrap().len(), 1);
        assert_eq!(value["events"][0]["method"], INBOX_RESURFACE);
        assert_eq!(value["events"][0]["params"]["reason"], "needs_you");
    }

    #[test]
    fn permission_reply_shape_is_enforced() {
        let mut session = HostSession::ephemeral();
        session.handle_request(req(1, HOST_HELLO, None));
        let response = session.handle_request(req(
            2,
            PERMISSION_REPLY,
            Some(json!({
                "requestId": "p1",
                "deviceId": session.identity.local_device.device_id,
            })),
        ));
        assert_eq!(response.error.unwrap().code, INVALID_PARAMS);
    }

    #[test]
    fn hello_with_local_device_id_reconnects() {
        let mut session = HostSession::ephemeral();
        let device_id = session.identity.local_device.device_id.clone();
        let response = session.handle_request(req(
            1,
            HOST_HELLO,
            Some(json!({
                "protocolVersion": 1,
                "device": { "deviceId": device_id, "name": "JaBot" }
            })),
        ));
        assert!(response.error.is_none());
        assert_eq!(result_value(&response)["device"]["name"], "JaBot");
    }

    #[test]
    fn health_before_hello_is_not_connected() {
        let mut session = HostSession::ephemeral();
        let response = session.handle_request(req(1, HOST_HEALTH, None));
        let value = result_value(&response);
        assert_eq!(value["connected"], false);
        assert_eq!(value["hostMode"], "in-process");
        assert!(value["hostId"].as_str().unwrap().len() > 8);
    }

    #[test]
    fn permission_notifications_carry_device_and_seq() {
        let mut session = HostSession::ephemeral();
        session.handle_request(req(1, HOST_HELLO, None));
        let seq = session.notify_permission_ask(
            "t1",
            "perm-1",
            json!({ "type": "command", "command": "ls" }),
            json!([{ "optionId": "allow_once" }]),
        );
        session.notify_permission_resolved(
            "t1",
            "perm-1",
            &session.identity.local_device.device_id.clone(),
            Some("allow_once".into()),
            None,
        );
        let outbound = session.take_outbound();
        assert_eq!(seq, 1);
        assert_eq!(outbound[0].method, super::protocol::PERMISSION_ASK);
        assert_eq!(outbound[1].method, super::protocol::PERMISSION_RESOLVED);
        assert_eq!(outbound[1].params.as_ref().unwrap()["seq"], 2);
        assert_eq!(
            outbound[1].params.as_ref().unwrap()["deviceId"],
            session.identity.local_device.device_id
        );
    }

    #[test]
    fn ndjson_hello_frame_roundtrip_through_session() {
        let mut session = HostSession::ephemeral();
        let request = req(9, HOST_HELLO, Some(json!({ "protocolVersion": 1 })));
        let frame = encode_frame(&JsonRpcMessage::Request(request.clone())).unwrap();
        let JsonRpcMessage::Request(decoded) = decode_frame(&frame).unwrap() else {
            panic!("expected request");
        };
        let response = session.handle_request(decoded);
        let out = encode_frame(&JsonRpcMessage::Response(response)).unwrap();
        assert!(out.contains("\"hostId\""));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn load_opens_sqlite_and_hello_reports_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        assert!(session.store().is_some());
        assert!(session.store_error.is_none());
        let response = session.handle_request(req(1, HOST_HELLO, None));
        let value = result_value(&response);
        assert_eq!(value["store"]["journalMode"], "wal");
        assert_eq!(value["store"]["schemaVersion"], 7);
        assert_eq!(value["store"]["botCount"], 6);
        // Three shipped cards plus the two presets, all seeded as rows so a
        // thread can name any of them (#13).
        assert_eq!(value["store"]["harnessCount"], 5);
        let backend = value["store"]["secretsBackend"].as_str().unwrap();
        assert!(
            backend == "keychain" || backend == "unavailable",
            "unexpected secrets backend {backend}"
        );
    }
}
