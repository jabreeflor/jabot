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
mod pairing;
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
pub use chief::{MCP_SERVER_NAME, MCP_VERSION};
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
pub use protocol::methods::{
    client_methods, DeviceAuth, DeviceListResult, DeviceRefParams, DeviceRevokeResult,
    PairedDeviceView, PairingCancelResult, PairingClaimParams, PairingClaimResult,
    PairingConfirmParams, PairingConfirmResult, PairingDevice, PairingOfferView, PairingQr,
    PairingRefParams, PairingSide, PairingStartParams, PairingStartResult, PairingStatusResult,
    DEVICE_LIST, DEVICE_REVOKE, PAIRING_CANCEL, PAIRING_CLAIM, PAIRING_CONFIRM, PAIRING_METHODS,
    PAIRING_START, PAIRING_STATUS,
};
#[allow(unused_imports)]
pub use protocol::{
    decode_frame, decode_frames, encode_frame, BotTemplateView, BotView, CrewCreateParams,
    CrewHostToolView, CrewListResult, CrewRefParams, CrewRemoveResult, CrewUpdateParams,
    DeviceInfo, DeviceRole, Envelope, FolderForgetResult, FolderListResult, FolderOriginView,
    FolderThreadView, FolderView, GithubStatusResult, HandoffView, HarnessCardView,
    HarnessDoctorResult, HarnessListResult, HarnessStatus, HarnessTier, HealthResult, HelloParams,
    HelloResult, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, PendingPermissionView, PermissionPendingParams, PermissionPendingResult,
    PermissionReplyParams, PermissionReplyResult, PromptMode, QueuedPromptView, RequestId,
    ResumeOutcome, ResurfaceReason, RpcError, ScheduleCreateParams, ScheduleFireView,
    ScheduleListResult, ScheduleRefParams, ScheduleRemoveResult, ScheduleRunResult,
    ScheduleUpdateParams, ScheduleView, StoreStatus, SupervisorStatusResult, ThreadResumeResult,
    ThreadStateResult, ThreadTranscriptParams, ThreadTranscriptResult, ToolCardView,
    ToolConnectResult, ToolConnectionStatus, ToolDisconnectResult, ToolListResult, ToolRefParams,
    ToolTransport, TranscriptEventView, CLIENT_METHODS, CREW_CREATE, CREW_LIST, CREW_REMOVE,
    CREW_THREAD, CREW_UPDATE, FOLDER_FORGET, FOLDER_LIST, FOLDER_REGISTER, FOLDER_UPDATE,
    GITHUB_STATUS, HARNESS_DOCTOR, HARNESS_LIST, HOST_HEALTH, HOST_HELLO, HOST_NOTIFICATIONS,
    INBOX_LIST, INBOX_RESURFACE, JSONRPC_VERSION, PERMISSION_ASK, PERMISSION_PENDING,
    PERMISSION_REPLY, PERMISSION_RESOLVED, PROTOCOL_VERSION, SESSION_CANCEL, SESSION_PROMPT,
    SESSION_UPDATE, SUPERVISOR_STATUS, THREAD_ARCHIVE, THREAD_DELETE, THREAD_FOLD, THREAD_OPEN,
    THREAD_REOPEN, THREAD_RESUME, THREAD_STATE, THREAD_TRANSCRIPT, TOOLS_CONNECT, TOOLS_DISCONNECT,
    TOOLS_LIST,
};
#[allow(unused_imports)]
pub use protocol::{
    INBOX_EVENT, SCHEDULE_CREATE, SCHEDULE_LIST, SCHEDULE_REMOVE, SCHEDULE_RUN, SCHEDULE_UPDATE,
};
pub use repo::{gh::GhAuth, git::RepoProbe, origin::Origin};
/// Schedules (#25). The cron and the catch-up policy are exported so tests and
/// a future settings surface can reason about them without a live host.
#[allow(unused_imports)]
pub use schedule::{CatchUp, CronError, CronSpec, RUN_KIND_SCHEDULE, STALE_AFTER};
#[allow(unused_imports)]
pub use store::{
    schema_head, InboxEventRow, NewFolder, NewThread, RunRow, ScheduleFireRow, ScheduleRow,
    Secrets, Store, StoreError, ThreadRepo, ThreadRow,
};
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
    InboxEventParams, InboxResurfaceParams, LoggedEvent, PermissionAskParams,
    PermissionResolvedParams, ResumeFromParams, ResumeFromResult, SessionUpdateParams,
};
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
    /// Pairing offers that are on somebody's screen right now (#19). RAM by
    /// design: a QR photographed off a monitor must be worthless the moment
    /// the host restarts, and the durable half of pairing is the grant, not
    /// the invitation. See `host/pairing/offer.rs`.
    pairing: pairing::PairingState,
    /// Who is on the other end, as the *host* understands them — the local
    /// console, or a paired device with the role its row carries. Set by
    /// `host/hello`; never taken from a later request.
    connected_device: Option<DeviceInfo>,
    /// One loopback MCP server per thread whose bot carries Chief's host
    /// tools (#24). Not persisted, and rightly so: it is a socket, and the
    /// port a dead process was listening on is worth nothing to this one.
    chief_bridges: HashMap<String, chief::Bridge>,
    /// True while a host tool call is being answered. A handoff prompts
    /// another thread, prompting pumps, and the pump comes back here — the
    /// guard is what keeps that from being recursion.
    chief_dispatching: bool,
    /// The device each *connection* said hello as (#29).
    ///
    /// Everything else on this struct is host state that a second client
    /// should share — one store, one set of adapters, one broker. The device
    /// binding is the one thing that must not be shared: `connected_device`
    /// decides what the caller may do, so a phone and a console reading the
    /// same binding is a phone with the console's authority. Requests are
    /// dispatched through [`HostSession::handle_request_on`], which swaps this
    /// connection's binding in and stashes it back out again.
    connection_devices: HashMap<String, DeviceInfo>,
    /// The in-process cron (#25). RAM: the poll clock and the label for the
    /// run a fire is about to open. Everything durable is in `schedules` and
    /// `schedule_fires`, because decision #4 stops this process every time the
    /// user quits and a schedule has to survive that.
    schedules: schedule::ScheduleState,
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
            pairing: pairing::PairingState::default(),
            connected_device: None,
            chief_bridges: HashMap::new(),
            chief_dispatching: false,
            connection_devices: HashMap::new(),
            schedules: schedule::ScheduleState::from_env(),
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
        self.handle_request_on(LOCAL_CONNECTION, request)
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
                self.connected_device = Some(self.identity.local_device_info());
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
                self.connected_device = Some(self.identity.local_device_info());
            }
            // Not this host's own console. Since #19 that is no longer
            // automatically a stranger — it may be a device the two humans
            // paired — but it is a stranger until it proves it, and every way
            // of failing to prove it comes back as the same
            // `UnpairedDevice` this arm has always returned.
            Some(id) => {
                let device = self.authenticate_paired_device(id, params.auth.as_ref())?;
                self.connected_device_id = Some(device.device_id.clone());
                self.connected_device = Some(device);
            }
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

    /// A new Inbox card on a thread that did not move (#25).
    ///
    /// `inbox/resurface` is a claim about the overlay — a folded thread came
    /// back. A schedule firing on an `active` standing thread produces a card
    /// and moves nothing, so it needs its own word rather than a resurface
    /// that would be a lie about the sidebar.
    pub fn notify_inbox_event(
        &mut self,
        thread_id: &str,
        kind: &str,
        title: &str,
        summary: &str,
    ) -> u64 {
        let seq = self.seq.next(thread_id);
        let params = InboxEventParams {
            host_id: self.identity.host_id.clone(),
            thread_id: thread_id.to_string(),
            seq,
            kind: kind.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
        };
        self.push_logged(thread_id, protocol::INBOX_EVENT, params);
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
        let methods = protocol::methods::client_methods();
        // The role the *host* bound to this connection, never the one the
        // client asked for in `hello`.
        let role = self
            .connected_device
            .as_ref()
            .map(|device| device.role)
            .unwrap_or(self.identity.local_device.role);
        let scoped_methods = methods
            .iter()
            .filter(|method| pairing::scope::allows(role, method))
            .cloned()
            .collect();
        HelloResult {
            protocol_version: PROTOCOL_VERSION,
            host_id: self.identity.host_id.clone(),
            host_name: self.identity.host_name.clone(),
            host_mode: HOST_MODE.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
            device: self
                .connected_device
                .clone()
                .unwrap_or_else(|| self.identity.local_device_info()),
            methods,
            scoped_methods,
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

/// The connection id a colocated client uses when nobody names one.
///
/// [`HostSession::handle_request`] is the Tauri path: one webview, one host,
/// one binding, and it keeps working untouched. A transport that can carry
/// several clients at once — the Unix socket `jabot-hostd --listen` opens —
/// gives each accepted connection its own id instead.
pub const LOCAL_CONNECTION: &str = "local";

/// Serving more than one client from one host (#29).
///
/// Decision #4 called the host API "socket-shaped" so a second device would be
/// packaging rather than a rewrite. A second device is what #29 is, and the
/// only thing the API turned out to be missing is *whose* connection a request
/// arrived on: `hello` binds a device, and until now there was exactly one
/// binding for the whole process.
///
/// The fix is deliberately not a second, phone-shaped API. The frames are the
/// same frames, the router is the same router, and the scope check in
/// `host/pairing/scope.rs` still reads the role off the `paired_devices` row —
/// it just reads it for the connection that is asking.
impl HostSession {
    /// Dispatch a request that arrived on `connection`.
    ///
    /// The binding is swapped in before the router runs and stashed back
    /// afterwards, so a hello on one connection cannot re-role another. Calls
    /// are serialized by the caller's lock; this is state, not concurrency.
    pub fn handle_request_on(
        &mut self,
        connection: &str,
        request: JsonRpcRequest,
    ) -> JsonRpcResponse {
        self.enter_connection(connection);
        let response = router::dispatch(self, request);
        self.leave_connection(connection);
        response
    }

    /// Forget a connection that hung up.
    ///
    /// A device is "connected" only while a socket it said hello on is open,
    /// which is what makes `device/list`'s `connected` column true rather than
    /// a memory of the last time somebody called.
    pub fn drop_connection(&mut self, connection: &str) {
        self.connection_devices.remove(connection);
        // The two scratch fields are whatever the last dispatch left behind,
        // and that may be the device that just hung up. Clearing them keeps
        // `device_is_connected` from reporting a socket that is closed; the
        // next dispatch fills them in again from the map.
        self.connected_device = None;
        self.connected_device_id = None;
    }

    /// Whether this device is on the other end of *some* live connection.
    ///
    /// Not the same question as "is it the caller": the desktop asking
    /// `device/list` wants to know whether the phone is up, and the phone is
    /// by definition not the connection that asked.
    pub(crate) fn device_is_connected(&self, device_id: &str) -> bool {
        self.connected_device_id.as_deref() == Some(device_id)
            || self
                .connection_devices
                .values()
                .any(|device| device.device_id == device_id)
    }

    fn enter_connection(&mut self, connection: &str) {
        let device = self.connection_devices.get(connection).cloned();
        self.connected_device_id = device.as_ref().map(|d| d.device_id.clone());
        self.connected_device = device;
    }

    fn leave_connection(&mut self, connection: &str) {
        match self.connected_device.clone() {
            Some(device) => {
                self.connection_devices
                    .insert(connection.to_string(), device);
            }
            None => {
                self.connection_devices.remove(connection);
            }
        }
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
        // The head of the migration list rather than a number copied here:
        // two issues landing migrations at once should not both have to edit
        // this line, and what hello promises is "the schema you have".
        assert_eq!(value["store"]["schemaVersion"], schema_head());
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

/// Two clients, one host (#29).
///
/// The socket in `jabot-hostd --listen` is what makes this reachable, and
/// `tests/e2e/mobile-inbox.test.ts` drives it for real. These are the
/// in-process statements of the same rule: a connection's device binding is
/// its own, and it does not outlive the connection.
#[cfg(test)]
mod connection_tests {
    use super::protocol::error::HELLO_REQUIRED;
    use super::protocol::jsonrpc::RequestId;
    use super::protocol::{HOST_HELLO, SYNC_RESUME_FROM};
    use super::*;
    use serde_json::json;

    fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(RequestId::Number(id), method, params)
    }

    /// A method behind `require_hello` that needs no store, so what is being
    /// asserted is the binding rather than SQLite.
    fn resume_from(session: &mut HostSession, connection: &str) -> JsonRpcResponse {
        session.handle_request_on(
            connection,
            req(
                2,
                SYNC_RESUME_FROM,
                Some(json!({ "threadId": "t1", "seq": 0 })),
            ),
        )
    }

    /// The property the approver role rests on. If one hello bound the whole
    /// process, a phone connecting would inherit — or hand out — the console's
    /// authority, and `host/pairing/scope.rs` would be checking the wrong row.
    #[test]
    fn a_hello_on_one_connection_does_not_speak_for_another() {
        let mut session = HostSession::ephemeral();
        let hello = session.handle_request_on("desktop", req(1, HOST_HELLO, None));
        assert!(hello.error.is_none());

        // The other connection has said nothing, and is treated as such.
        let stranger = resume_from(&mut session, "phone");
        assert_eq!(stranger.error.expect("error").code, HELLO_REQUIRED);
        // While the one that did say hello is unaffected by the refusal.
        assert!(resume_from(&mut session, "desktop").error.is_none());
    }

    #[test]
    fn hanging_up_forgets_the_device() {
        let mut session = HostSession::ephemeral();
        let hello = session.handle_request_on("phone", req(1, HOST_HELLO, None));
        let device_id = hello.result.expect("hello")["device"]["deviceId"]
            .as_str()
            .expect("deviceId")
            .to_string();
        assert!(session.device_is_connected(&device_id));

        session.drop_connection("phone");
        // `device/list` must not keep claiming a socket that is closed, and a
        // reconnect on the same id must say hello again.
        assert!(!session.device_is_connected(&device_id));
        assert_eq!(
            resume_from(&mut session, "phone")
                .error
                .expect("error")
                .code,
            HELLO_REQUIRED
        );
    }

    /// The default path — Tauri IPC, one webview — is unchanged: it is just a
    /// connection whose id nobody had to choose.
    #[test]
    fn the_colocated_client_is_an_ordinary_connection() {
        let mut session = HostSession::ephemeral();
        assert!(session
            .handle_request(req(1, HOST_HELLO, None))
            .error
            .is_none());
        assert!(resume_from(&mut session, LOCAL_CONNECTION).error.is_none());
    }

    /// `host/hello` says what *this* device may call, so a client does not have
    /// to keep its own copy of the role's allowlist in sync by hand (#19).
    #[test]
    fn hello_says_what_this_device_may_call() {
        let mut session = HostSession::ephemeral();
        let hello = session.handle_request(req(1, HOST_HELLO, None));
        let value = hello.result.expect("hello");
        let scoped: Vec<&str> = value["scopedMethods"]
            .as_array()
            .expect("scopedMethods")
            .iter()
            .map(|m| m.as_str().expect("method"))
            .collect();
        // The console is `full`, so its scope is everything it was told about.
        assert_eq!(scoped.len(), value["methods"].as_array().unwrap().len());
        assert!(scoped.contains(&protocol::SESSION_PROMPT));
        // And the narrow list is a strict subset of it, whichever role asks.
        for method in pairing::scope::APPROVER_METHODS {
            assert!(scoped.contains(method), "{method}");
        }
    }
}
