//! The session backend boundary (#299).
//!
//! Everything the host asks of a running agent goes through
//! [`SessionBackend`]: start or restore a session, send a prompt, cancel,
//! answer an interaction, drain events, and know whether the process is still
//! there. `HostSession` holds `Box<dyn SessionBackend>` and never names a
//! transport. Today the only implementation is the ACP adapter in
//! `host/acp/`, and [`select`] always answers [`BackendKind::Acp`] — which is
//! the point of landing the seam first: the ACP path moved behind it with no
//! change on the wire, and a direct provider backend arrives later as one
//! more `impl` plus one more variant here, gated on the capability evidence
//! in `docs/decisions/issue-299.md`.
//!
//! What is deliberately *not* abstracted: the event vocabulary. A backend
//! emits [`BackendEvent::Update`] carrying an ACP `session/update`-shaped
//! payload, because that payload is already the host's normalized event
//! model — persisted by `persist_transcript_event`, replayed by one reducer,
//! read by the lifecycle, the preview, and the PR watch. A native backend
//! translates its own events into that shape at its boundary, the way T3 Code
//! translates native events into its common model, rather than teaching every
//! consumer a second vocabulary.
//!
//! Capabilities are explicit and absent means **no**. A backend that does not
//! advertise `resume` is never sent a resume; a future `fork` or config-option
//! verb is refused before any conversation or workspace state changes. The
//! flags here grow with the operations the host actually dispatches; none is
//! declared ahead of a caller.

#[cfg(test)]
pub(crate) mod contract;

use std::path::Path;
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;

use serde_json::Value;

use super::acp::{AcpConnection, AdapterWake, HarnessRuntime};
use super::protocol::error::RpcError;
use super::protocol::jsonrpc::RequestId;

/// Which integration is behind a session.
///
/// One variant per implementation that exists. A direct provider backend adds
/// its variant in the same change that adds its `impl SessionBackend`, so the
/// enum never advertises a backend the host cannot spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// An ACP adapter subprocess speaking JSON-RPC over stdio (`host/acp/`).
    Acp,
}

impl BackendKind {
    /// The stable identifier, for diagnostics and for the row that will pin a
    /// thread to its backend once one is persisted.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Acp => "acp",
        }
    }
}

/// What a backend said it can do, learned from its handshake.
///
/// Every field is an operation the host dispatches conditionally. Guessing
/// yes buys a `session/resume` that comes back method-not-found on a thread
/// the user was told had been restored (`session-lifecycle/keep-alive.md`),
/// so `Default` is all-false and a backend has to opt in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// Restore a session's context without replaying its history.
    pub resume: bool,
    /// Restore a session by replaying its history as update events.
    pub load: bool,
    /// Free the agent's own resources for a session before the process goes.
    pub close: bool,
}

/// The native id of a request the agent is blocked on, handed back verbatim
/// with the answer. A JSON-RPC id for every backend that speaks JSON-RPC,
/// which is all of them so far; opaque to everything outside the backend.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractionId(pub RequestId);

/// The native id of a turn in flight. ACP v1 puts no `sessionId` on the
/// prompt *response*, so this is the only thing that says whose turn ended on
/// a process serving several threads; a backend records it against the thread
/// at send time and hands it back on [`BackendEvent::TurnEnded`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TurnId(pub RequestId);

/// What a backend hands the pump.
#[derive(Debug)]
pub enum BackendEvent {
    /// A `session/update`-shaped payload, already flattened to the shape every
    /// consumer reads (`sessionUpdate` and `content` at the top level).
    Update(Value),
    /// The agent is blocked waiting for a permission answer
    /// (`session/request_permission`).
    Interaction { id: InteractionId, params: Value },
    /// A blocking request that is not a permission: one of the extension
    /// methods `acp/extensions` names (`cursor/ask_question`,
    /// `cursor/create_plan`, #298). Carried raw so the host — not the
    /// reader thread — decides whether it can be drawn, and still holds the
    /// id to refuse it if not.
    Extension {
        id: InteractionId,
        method: String,
        params: Value,
    },
    /// The turn identified by `turn` is over. `payload` is the prompt
    /// response: a stop reason, or an `error` when the agent refused.
    TurnEnded { turn: TurnId, payload: Value },
    /// The process is gone. Fans out to every thread riding it.
    Closed { error: Option<String> },
}

/// How a restore attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restored {
    /// Context is back; no history was replayed.
    Resumed,
    /// History was replayed as [`BackendEvent::Update`]s, which are sitting in
    /// the backend's queue by the time this returns. The caller decides
    /// whether its own transcript already holds them.
    Loaded,
    /// The backend speaks no restoring verb, or the one it speaks refused.
    Unsupported(Option<String>),
}

/// One live agent process, carrying one or more JaBot threads.
///
/// Object-safe on purpose: `HostSession` owns these as `Box<dyn
/// SessionBackend>`, keyed by `HostSession::connection_key`. Tenancy — which
/// thread owns which native session on this process — lives on the backend
/// because it is the backend's own routing table, and it is the same table
/// whatever protocol the process speaks.
pub trait SessionBackend: std::fmt::Debug + Send {
    fn kind(&self) -> BackendKind;

    /// The protocol handshake. Idempotent: a second call answers from what
    /// the first learned. Returns the agent's raw handshake result so the
    /// Doctor can read a protocol version out of it.
    fn handshake(&mut self) -> Result<Value, RpcError>;

    /// Only meaningful after [`Self::handshake`]; all-false before it.
    fn capabilities(&self) -> BackendCapabilities;

    /// Mint a new session for `thread_id` and adopt it. `mcp_servers` fixes
    /// the tool surface for the life of the session; `model` is a request the
    /// backend may or may not be able to honour.
    fn create_session(
        &mut self,
        thread_id: &str,
        cwd: &str,
        mcp_servers: Value,
        model: Option<&str>,
    ) -> Result<String, RpcError>;

    /// Best-effort live model switch on `session_id` (#296).
    fn apply_model(&mut self, session_id: &str, model_id: &str) -> Result<(), RpcError>;

    /// Model ids the last `create_session` advertised, if any. Taking them
    /// clears the cache so a later caller does not persist a stale list.
    fn take_advertised_models(&mut self) -> Vec<String>;

    /// Hand the agent back a session it already has, by whichever restoring
    /// verb it advertised, and adopt it on success. Never mints a new
    /// session: `Unsupported` is the caller's cue to do that honestly.
    fn restore_session(
        &mut self,
        thread_id: &str,
        session_id: &str,
        cwd: &str,
        mcp_servers: Value,
    ) -> Result<Restored, RpcError>;

    /// Free the agent's resources for one session. A no-op, not a fake, when
    /// the backend never advertised `close`.
    fn close_session(&mut self, session_id: &str) -> Result<(), RpcError>;

    /// Fire a prompt without waiting for the turn. Completion arrives later as
    /// [`BackendEvent::TurnEnded`] carrying the returned id.
    fn send_prompt(
        &mut self,
        thread_id: &str,
        session_id: &str,
        content: &Value,
    ) -> Result<TurnId, RpcError>;

    /// Ask the agent to stop the turn on `session_id`. Fire-and-forget: the
    /// acknowledgement, if any, is the turn ending.
    fn cancel(&mut self, session_id: &str) -> Result<(), RpcError>;

    /// Answer an interaction the agent is blocked on.
    fn respond(&self, id: InteractionId, outcome: Value) -> Result<(), RpcError>;

    /// Refuse a request the agent made. `-32601` is what a request nobody
    /// implements gets, and the one answer that lets Cursor fall back to
    /// what it can do without us (#298).
    fn respond_error(&self, id: InteractionId, code: i64, message: &str) -> Result<(), RpcError>;

    // ---- tenancy ---------------------------------------------------------

    /// Record that `thread_id` owns `session_id` on this process, replacing
    /// any earlier entry for the thread.
    fn adopt(&mut self, thread_id: &str, session_id: &str);
    /// The session this thread owns here, if it has one yet.
    fn session_for(&self, thread_id: &str) -> Option<String>;
    /// Every thread riding this process.
    fn tenants(&self) -> Vec<String>;
    /// Forget a thread, returning the session it held so the caller can close
    /// it. Anything in flight for the thread is forgotten with it.
    fn release(&mut self, thread_id: &str) -> Option<String>;
    /// Whether anything is still riding this process.
    fn is_vacant(&self) -> bool;
    /// Who an event belongs to. Empty means *drop it*, not "give it to
    /// whoever"; `Closed` is everybody's.
    fn route(&mut self, event: &BackendEvent) -> Vec<String>;

    // ---- liveness --------------------------------------------------------

    fn try_recv(&mut self) -> Result<BackendEvent, TryRecvError>;
    /// Is the process still running? Answered by reaping the pid, not by the
    /// state of a pipe a grandchild may be holding open.
    fn is_alive(&mut self) -> bool;
    /// Diagnostic only: nothing durable is keyed on a pid (decision #4).
    fn pid(&self) -> u32;
    /// Where the process's stderr is going.
    fn log_path(&self) -> &Path;
    /// Terminate the process tree. Idempotent.
    fn kill(&mut self);
}

/// Which backend a runtime should be started on.
///
/// The selection policy. There is one answer today, and having the question
/// asked in exactly one place is what keeps it that way: when a direct
/// backend exists, this is where the card's evidence-backed capability claim
/// meets the thread, and a thread that already has a backend is never
/// re-selected (#299: existing threads stay pinned).
pub(crate) fn select(_runtime: &HarnessRuntime) -> BackendKind {
    BackendKind::Acp
}

/// Start a process for `kind` and hand it back behind the trait.
///
/// The one constructor every caller uses — the prompt path, the supervisor's
/// restore, the Doctor's throwaway handshake — so that no caller names a
/// concrete backend type.
pub(crate) fn spawn(
    kind: BackendKind,
    runtime: &HarnessRuntime,
    cwd: Option<&Path>,
    log_path: &Path,
    wake: Arc<AdapterWake>,
) -> Result<Box<dyn SessionBackend>, RpcError> {
    match kind {
        BackendKind::Acp => Ok(Box::new(AcpConnection::spawn(
            runtime, cwd, log_path, wake,
        )?)),
    }
}
