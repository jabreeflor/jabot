//! The shared session-backend contract (#299).
//!
//! Every backend the host can spawn has to pass [`exercise`] against a fresh
//! instance of itself. The checks are the things `HostSession` relies on
//! without looking at the transport: a handshake that can be repeated, tenancy
//! that says exactly who is on the process, a turn that ends on the thread
//! that sent it, a restore that is refused rather than faked when the backend
//! cannot do it, and a kill that is final.
//!
//! Two implementations run it here. [`FakeBackend`] is in-process and has no
//! subprocess at all — proof that the trait is a boundary and not a
//! description of ACP stdio. The second is the real `AcpConnection` driving a
//! four-line shell agent, so the ACP move behind the trait is checked by the
//! same code that a direct backend will be.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;

/// Run every contract check against backends from `factory`.
///
/// Each check gets its own instance: the contract says nothing about what a
/// backend does *after* a kill, so no check may inherit another's.
pub(crate) fn exercise(factory: &dyn Fn() -> Box<dyn SessionBackend>) {
    handshake_is_repeatable_and_capabilities_wait_for_it(factory());
    tenancy_says_exactly_who_is_here(factory());
    a_turn_ends_on_the_thread_that_sent_it(factory());
    an_unadvertised_restore_is_refused_not_faked(factory());
    kill_is_final_and_idempotent(factory());
}

fn handshake_is_repeatable_and_capabilities_wait_for_it(mut backend: Box<dyn SessionBackend>) {
    assert_eq!(
        backend.capabilities(),
        BackendCapabilities::default(),
        "{:?}: capabilities are all-false before the handshake",
        backend.kind()
    );
    let first = backend.handshake().expect("first handshake");
    assert!(first.is_object(), "handshake returns the agent's result");
    let second = backend.handshake().expect("second handshake");
    assert!(
        second.is_object(),
        "a second handshake answers rather than re-running"
    );
}

fn tenancy_says_exactly_who_is_here(mut backend: Box<dyn SessionBackend>) {
    let cwd = std::env::temp_dir();
    let cwd = cwd.to_string_lossy();
    assert!(backend.is_vacant(), "nothing rides a fresh process");
    assert_eq!(backend.session_for("t1"), None);
    assert!(backend.tenants().is_empty());

    let session = backend
        .create_session("t1", &cwd, json!([]), None)
        .expect("create_session");
    assert!(!session.is_empty(), "create_session returns a session id");
    assert_eq!(backend.session_for("t1").as_deref(), Some(session.as_str()));
    assert_eq!(backend.tenants(), vec!["t1".to_string()]);
    assert!(!backend.is_vacant());

    // Re-attaching replaces, never accumulates.
    backend.adopt("t1", "another");
    assert_eq!(backend.session_for("t1").as_deref(), Some("another"));
    assert_eq!(backend.tenants(), vec!["t1".to_string()]);

    assert_eq!(backend.release("t1").as_deref(), Some("another"));
    assert!(
        backend.is_vacant(),
        "the last tenant leaving vacates the process"
    );
    assert_eq!(
        backend.release("t1"),
        None,
        "releasing twice is not an error"
    );
    backend.kill();
}

fn a_turn_ends_on_the_thread_that_sent_it(mut backend: Box<dyn SessionBackend>) {
    let cwd = std::env::temp_dir();
    let cwd = cwd.to_string_lossy();
    let session = backend
        .create_session("t1", &cwd, json!([]), None)
        .expect("create_session");
    let turn = backend
        .send_prompt("t1", &session, &json!("hello"))
        .expect("send_prompt");

    let mut ended = None;
    let mut updates = 0;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && ended.is_none() {
        match backend.try_recv() {
            Ok(event) => {
                let owners = backend.route(&event);
                match event {
                    BackendEvent::Update(_) => {
                        updates += 1;
                        assert_eq!(
                            owners,
                            vec!["t1".to_string()],
                            "updates route to the tenant"
                        );
                    }
                    BackendEvent::TurnEnded {
                        turn: ended_turn, ..
                    } => {
                        assert_eq!(ended_turn, turn, "the turn that ended is the one sent");
                        assert_eq!(
                            owners,
                            vec!["t1".to_string()],
                            "the turn routes to its sender"
                        );
                        ended = Some(ended_turn);
                    }
                    BackendEvent::Interaction { .. } | BackendEvent::Extension { .. } => {
                        panic!("no interaction was requested")
                    }
                    BackendEvent::Closed { error } => panic!("process closed mid-turn: {error:?}"),
                }
            }
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
            Err(TryRecvError::Disconnected) => panic!("event channel closed mid-turn"),
        }
    }
    let ended = ended.expect("the turn ended within the deadline");
    assert!(
        updates >= 1,
        "a turn produces at least one update before it ends"
    );
    // The owner was consumed with the event: a duplicate response is unroutable.
    let again = BackendEvent::TurnEnded {
        turn: ended,
        payload: Value::Null,
    };
    assert!(
        backend.route(&again).is_empty(),
        "a turn's owner is claimed once, so a replayed response is dropped"
    );
    backend
        .cancel(&session)
        .expect("cancel after the turn is harmless");
    backend.kill();
}

fn an_unadvertised_restore_is_refused_not_faked(mut backend: Box<dyn SessionBackend>) {
    let cwd = std::env::temp_dir();
    let cwd = cwd.to_string_lossy();
    backend.handshake().expect("handshake");
    let caps = backend.capabilities();
    if caps.resume || caps.load {
        // This backend can restore; the refusal path is not its contract.
        backend.kill();
        return;
    }
    let restored = backend
        .restore_session("t1", "sess-from-a-previous-life", &cwd, json!([]))
        .expect("restore is a refusal, not an error");
    assert_eq!(restored, Restored::Unsupported(None));
    assert_eq!(
        backend.session_for("t1"),
        None,
        "a refused restore adopts nothing"
    );
    backend.kill();
}

fn kill_is_final_and_idempotent(mut backend: Box<dyn SessionBackend>) {
    assert!(backend.is_alive(), "a fresh backend is alive");
    backend.kill();
    let deadline = Instant::now() + Duration::from_secs(5);
    while backend.is_alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!backend.is_alive(), "kill stops the process");
    backend.kill();
    assert!(!backend.is_alive(), "a second kill changes nothing");
}

/// A backend with no process behind it.
///
/// Answers the handshake with whatever capabilities it was built with, hands
/// back one update and a turn end for every prompt, and routes exactly the
/// way the ACP connection does. Everything the contract needs and nothing
/// else, so a failure here is a failure of the trait's shape.
#[derive(Debug)]
pub(crate) struct FakeBackend {
    advertised: BackendCapabilities,
    learned: BackendCapabilities,
    handshakes: u32,
    sessions: HashMap<String, String>,
    by_session: HashMap<String, String>,
    turn_owners: HashMap<TurnId, String>,
    next_id: i64,
    queue: VecDeque<BackendEvent>,
    alive: bool,
    log_path: PathBuf,
}

impl FakeBackend {
    pub(crate) fn new(advertised: BackendCapabilities) -> Self {
        Self {
            advertised,
            learned: BackendCapabilities::default(),
            handshakes: 0,
            sessions: HashMap::new(),
            by_session: HashMap::new(),
            turn_owners: HashMap::new(),
            next_id: 1,
            queue: VecDeque::new(),
            alive: true,
            log_path: std::env::temp_dir().join("fake-backend.log"),
        }
    }

    fn owner_of(&self, params: &Value) -> Vec<String> {
        match params.get("sessionId").and_then(Value::as_str) {
            Some(session) => self.by_session.get(session).cloned().into_iter().collect(),
            None if self.sessions.len() == 1 => self.tenants(),
            None => Vec::new(),
        }
    }
}

impl SessionBackend for FakeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Acp
    }

    fn handshake(&mut self) -> Result<Value, RpcError> {
        self.handshakes += 1;
        self.learned = self.advertised;
        Ok(json!({ "protocolVersion": 1, "handshakes": self.handshakes }))
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.learned
    }

    fn create_session(
        &mut self,
        thread_id: &str,
        _cwd: &str,
        _mcp_servers: Value,
        _model: Option<&str>,
    ) -> Result<String, RpcError> {
        self.handshake()?;
        let session = format!("fake-{}", self.next_id);
        self.next_id += 1;
        self.adopt(thread_id, &session);
        Ok(session)
    }

    fn apply_model(&mut self, _session_id: &str, _model_id: &str) -> Result<(), RpcError> {
        Ok(())
    }

    fn take_advertised_models(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn restore_session(
        &mut self,
        thread_id: &str,
        session_id: &str,
        _cwd: &str,
        _mcp_servers: Value,
    ) -> Result<Restored, RpcError> {
        self.handshake()?;
        if self.learned.resume {
            self.adopt(thread_id, session_id);
            return Ok(Restored::Resumed);
        }
        if self.learned.load {
            self.adopt(thread_id, session_id);
            self.queue.push_back(BackendEvent::Update(json!({
                "sessionId": session_id,
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "replayed" }
            })));
            return Ok(Restored::Loaded);
        }
        Ok(Restored::Unsupported(None))
    }

    fn close_session(&mut self, _session_id: &str) -> Result<(), RpcError> {
        Ok(())
    }

    fn send_prompt(
        &mut self,
        thread_id: &str,
        session_id: &str,
        _content: &Value,
    ) -> Result<TurnId, RpcError> {
        let turn = TurnId(RequestId::Number(self.next_id));
        self.next_id += 1;
        self.turn_owners.insert(turn.clone(), thread_id.to_string());
        self.queue.push_back(BackendEvent::Update(json!({
            "sessionId": session_id,
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "ok" }
        })));
        self.queue.push_back(BackendEvent::TurnEnded {
            turn: turn.clone(),
            payload: json!({ "stopReason": "end_turn" }),
        });
        Ok(turn)
    }

    fn cancel(&mut self, _session_id: &str) -> Result<(), RpcError> {
        Ok(())
    }

    fn respond(&self, _id: InteractionId, _outcome: Value) -> Result<(), RpcError> {
        Ok(())
    }

    fn respond_error(
        &self,
        _id: InteractionId,
        _code: i64,
        _message: &str,
    ) -> Result<(), RpcError> {
        Ok(())
    }

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

    fn release(&mut self, thread_id: &str) -> Option<String> {
        self.turn_owners.retain(|_, owner| owner != thread_id);
        let session = self.sessions.remove(thread_id)?;
        self.by_session.remove(&session);
        Some(session)
    }

    fn is_vacant(&self) -> bool {
        self.sessions.is_empty()
    }

    fn route(&mut self, event: &BackendEvent) -> Vec<String> {
        match event {
            BackendEvent::Closed { .. } => self.tenants(),
            BackendEvent::TurnEnded { turn, .. } => {
                self.turn_owners.remove(turn).into_iter().collect()
            }
            BackendEvent::Update(params)
            | BackendEvent::Interaction { params, .. }
            | BackendEvent::Extension { params, .. } => self.owner_of(params),
        }
    }

    fn try_recv(&mut self) -> Result<BackendEvent, TryRecvError> {
        if !self.alive {
            return Err(TryRecvError::Disconnected);
        }
        self.queue.pop_front().ok_or(TryRecvError::Empty)
    }

    fn is_alive(&mut self) -> bool {
        self.alive
    }

    fn pid(&self) -> u32 {
        0
    }

    fn log_path(&self) -> &Path {
        &self.log_path
    }

    fn kill(&mut self) {
        self.alive = false;
    }
}

#[test]
fn the_in_process_fake_passes_the_contract() {
    exercise(&|| Box::new(FakeBackend::new(BackendCapabilities::default())));
}

#[test]
fn a_fake_that_can_restore_takes_the_restoring_branches() {
    let cwd = std::env::temp_dir();
    let cwd = cwd.to_string_lossy();

    let mut resumes = FakeBackend::new(BackendCapabilities {
        resume: true,
        ..BackendCapabilities::default()
    });
    assert_eq!(
        resumes
            .restore_session("t1", "old", &cwd, json!([]))
            .unwrap(),
        Restored::Resumed
    );
    assert_eq!(resumes.session_for("t1").as_deref(), Some("old"));

    let mut loads = FakeBackend::new(BackendCapabilities {
        load: true,
        ..BackendCapabilities::default()
    });
    assert_eq!(
        loads.restore_session("t1", "old", &cwd, json!([])).unwrap(),
        Restored::Loaded
    );
    // The replay is queued by the time the restore answers, which is what
    // lets the supervisor settle it as a unit.
    assert!(matches!(loads.try_recv(), Ok(BackendEvent::Update(_))));
}

/// The smallest ACP agent that can hold up its end of the contract: answers
/// `initialize` and `session/new`, streams one chunk and a stop reason for
/// every prompt, and method-not-founds everything else — which is also what
/// makes it advertise no capabilities, so the refusal branch is exercised.
#[cfg(unix)]
const SH_AGENT: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"sh-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sh-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}\n'
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
    *'"method":"session/cancel"'*) ;;
    *)
      if [ -n "$id" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id"
      fi ;;
  esac
done
"#;

#[cfg(unix)]
#[test]
fn the_acp_connection_passes_the_contract() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("sh-agent.stderr.log");
    let runtime = HarnessRuntime {
        id: "sh-acp".into(),
        command: "sh".into(),
        args: vec!["-c".into(), SH_AGENT.into()],
        env: std::collections::BTreeMap::new(),
        install_hint: None,
        model: None,
    };
    let wake = AdapterWake::new();
    exercise(&|| {
        spawn(
            select(&runtime),
            &runtime,
            None,
            &log_path,
            Arc::clone(&wake),
        )
        .expect("spawn the shell agent")
    });
}
