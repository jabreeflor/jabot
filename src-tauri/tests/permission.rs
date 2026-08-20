//! The permission broker (#20): the record, the second click, and the quit.
//!
//! `acp_adapter.rs` already proves the round trip — an agent asks, a human
//! answers, the agent is told. What is asserted here is what happens when that
//! round trip does *not* complete: the host is quit while the question is
//! outstanding, the turn is cancelled under it, or the button is pressed
//! twice. Each of those used to end with the ask simply gone.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::{
    HostSession, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, NewThread, RequestId,
    ThreadRepo, HOST_HELLO, PERMISSION_ASK, PERMISSION_PENDING, PERMISSION_REPLY, SESSION_CANCEL,
    SESSION_PROMPT,
};
use serde_json::{json, Value};

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

fn hello(session: &mut HostSession) -> String {
    let response = session.handle_request(req(1, HOST_HELLO, None));
    response.result.as_ref().expect("hello")["device"]["deviceId"]
        .as_str()
        .expect("deviceId")
        .to_string()
}

fn fake_agent() -> String {
    if let Some(path) = option_env!("CARGO_BIN_EXE_fake_acp_agent") {
        return path.to_string();
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = vec![
        manifest.join("target/debug/fake-acp-agent"),
        manifest.join("../target/debug/fake-acp-agent"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(debug_dir) = exe.parent().and_then(|p| p.parent()) {
            candidates.push(debug_dir.join("fake-acp-agent"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| manifest.join("target/debug/fake-acp-agent"))
        .to_string_lossy()
        .into_owned()
}

fn result_value(response: &JsonRpcResponse) -> &Value {
    response.result.as_ref().expect("expected result")
}

/// A stored thread whose harness is the fake agent in `mode`.
///
/// Stored rather than passed inline on the prompt, because the broker's record
/// has a foreign key to `threads`: durability is a property of a thread the
/// host actually knows about.
fn open_thread(session: &mut HostSession, dir: &std::path::Path, thread_id: &str, mode: &str) {
    let runtime = json!({ "command": fake_agent(), "args": [mode] }).to_string();
    session
        .store()
        .expect("store")
        .insert_thread(&NewThread {
            id: thread_id.into(),
            folder_id: None,
            bot_id: None,
            harness_id: "claude".into(),
            cwd: dir.to_string_lossy().into(),
            runtime_json: runtime,
            title: "Auth migration".into(),
            fold_policy: "default".into(),
            worktree_path: None,
            repo: ThreadRepo::default(),
        })
        .expect("thread row");
}

fn wait_for(
    session: &mut HostSession,
    method: &str,
    timeout: Duration,
) -> Vec<JsonRpcNotification> {
    let start = Instant::now();
    let mut found = Vec::new();
    while start.elapsed() < timeout {
        session.pump_acp();
        found.extend(session.take_outbound());
        if found.iter().any(|n| n.method == method) {
            return found;
        }
        thread::sleep(Duration::from_millis(15));
    }
    found
}

/// Drive the thread to an outstanding ask and return its `requestId`.
fn ask(session: &mut HostSession, thread_id: &str) -> String {
    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(json!({ "threadId": thread_id, "content": "rm -rf" })),
    ));
    assert!(response.error.is_none(), "{:?}", response.error);
    let outbound = wait_for(session, PERMISSION_ASK, Duration::from_secs(5));
    outbound
        .iter()
        .find(|n| n.method == PERMISSION_ASK)
        .unwrap_or_else(|| panic!("no permission/ask; saw {outbound:?}"))
        .params
        .as_ref()
        .expect("params")["requestId"]
        .as_str()
        .expect("requestId")
        .to_string()
}

fn pending(session: &mut HostSession) -> Vec<Value> {
    let response = session.handle_request(req(50, PERMISSION_PENDING, None));
    result_value(&response)["requests"]
        .as_array()
        .expect("requests")
        .clone()
}

/// Cmd-Q with a question on the screen. The adapter dies with the host and the
/// ACP request id dies with it, but the *question* is the user's, and it has to
/// still be there — and still be answerable — when they come back.
#[test]
fn an_ask_outlives_the_host_that_took_it() {
    let dir = tempfile::tempdir().unwrap();
    let request_id = {
        let mut session = HostSession::load(dir.path());
        hello(&mut session);
        open_thread(&mut session, dir.path(), "t-quit", "permission");
        let request_id = ask(&mut session, "t-quit");
        // Quit: `Drop` runs `shutdown_adapters`, which kills the adapter and
        // deliberately leaves the record pending.
        drop(session);
        request_id
    };

    let mut session = HostSession::load(dir.path());
    let device_id = hello(&mut session);
    let open = pending(&mut session);
    assert_eq!(open.len(), 1, "the ask did not survive the quit: {open:?}");
    assert_eq!(open[0]["requestId"], request_id.as_str());
    assert_eq!(open[0]["threadId"], "t-quit");
    assert_eq!(open[0]["title"], "Run ls");
    assert_eq!(open[0]["kind"], "execute");
    // The agent it belonged to is gone, and the card has to say so rather than
    // pretending a click will reach anyone.
    assert_eq!(open[0]["stale"], true);
    // The options are the agent's own, not ones the host invented.
    assert_eq!(open[0]["options"][0]["optionId"], "allow_once");

    let reply = session.handle_request(req(
        3,
        PERMISSION_REPLY,
        Some(json!({
            "requestId": request_id,
            "deviceId": device_id,
            "optionId": "allow_once"
        })),
    ));
    let answered = result_value(&reply);
    assert_eq!(answered["alreadyAnswered"], false);
    // Recorded, not delivered: there is no live ACP call left to hand it to,
    // and saying otherwise would be the one lie this surface cannot tell.
    assert_eq!(answered["delivered"], false);
    assert!(
        pending(&mut session).is_empty(),
        "an answered ask is still being asked"
    );
}

/// Two clicks. The second must not reach the agent, must not panic, and must
/// come back saying what the first one decided.
#[test]
fn answering_twice_answers_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    let device_id = hello(&mut session);
    open_thread(&mut session, dir.path(), "t-twice", "permission");
    let request_id = ask(&mut session, "t-twice");

    let reply = |session: &mut HostSession, id: i64, option: &str| {
        session.handle_request(req(
            id,
            PERMISSION_REPLY,
            Some(json!({
                "requestId": request_id,
                "deviceId": device_id,
                "optionId": option
            })),
        ))
    };

    let first = reply(&mut session, 3, "allow_once");
    assert_eq!(result_value(&first)["delivered"], true);
    assert_eq!(result_value(&first)["alreadyAnswered"], false);

    // A different option on purpose: the answer that stands is the first one,
    // not the last one to arrive.
    let second = reply(&mut session, 4, "reject_once");
    let value = result_value(&second);
    assert!(second.error.is_none(), "{:?}", second.error);
    assert_eq!(value["alreadyAnswered"], true);
    assert_eq!(value["optionId"], "allow_once");
    assert_eq!(value["delivered"], true);

    // And the agent heard it once. The fake agent narrates every outcome it
    // reads to stderr, which the host tees into the thread's log.
    let log = dir.path().join("adapter-logs").join("t-twice.stderr.log");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        session.pump_acp();
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let replies = text
            .lines()
            .filter(|line| line.starts_with("permission_reply="))
            .count();
        if replies == 1 {
            break;
        }
        assert!(
            replies < 2,
            "the agent was answered {replies} times: {text}"
        );
        assert!(Instant::now() < deadline, "the agent was never answered");
        thread::sleep(Duration::from_millis(15));
    }
}

/// The click that raced the Stop button. #10 already answers the agent
/// `cancelled`; what this pins is the *record* — the ask is resolved, so a
/// click that arrives afterwards is told what happened instead of erroring, and
/// the thread does not keep asking a question nobody can act on.
#[test]
fn a_cancelled_turn_resolves_the_ask_as_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    let device_id = hello(&mut session);
    open_thread(&mut session, dir.path(), "t-race", "permission");
    let request_id = ask(&mut session, "t-race");

    let cancel = session.handle_request(req(
        3,
        SESSION_CANCEL,
        Some(json!({ "threadId": "t-race" })),
    ));
    assert_eq!(result_value(&cancel)["cancelled"], true);
    assert!(pending(&mut session).is_empty());

    let late = session.handle_request(req(
        4,
        PERMISSION_REPLY,
        Some(json!({
            "requestId": request_id,
            "deviceId": device_id,
            "optionId": "allow_once"
        })),
    ));
    assert!(late.error.is_none(), "{:?}", late.error);
    let value = result_value(&late);
    assert_eq!(value["alreadyAnswered"], true);
    assert_eq!(value["cancelled"], true);
}

/// An id from nowhere is still an error. Idempotence is about a request the
/// host took, not about accepting anything a client sends.
#[test]
fn an_unknown_request_id_is_refused() {
    let mut session = HostSession::ephemeral();
    let device_id = hello(&mut session);
    let response = session.handle_request(req(
        2,
        PERMISSION_REPLY,
        Some(json!({
            "requestId": "req-nobody-asked",
            "deviceId": device_id,
            "optionId": "allow_once"
        })),
    ));
    assert_eq!(response.error.expect("refusal").code, -32602);
}

/// Wait for Inbox answers a read itself (#5/#15). The broker's ledger records
/// that it did — an away-log of what the host decided on the user's behalf is
/// worth exactly as much as a record of what it asked them.
#[test]
fn wait_for_inbox_records_the_answer_it_gave() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    open_thread(&mut session, dir.path(), "t-read", "read-permission");
    session
        .store()
        .expect("store")
        .transition_thread("t-read", "active", "folded", None)
        .expect("fold");
    session
        .store()
        .expect("store")
        .set_thread_fold_policy("t-read", "wait_for_inbox")
        .expect("policy");

    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(json!({ "threadId": "t-read", "content": "read it" })),
    ));
    assert!(response.error.is_none(), "{:?}", response.error);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        session.pump_acp();
        let _ = session.take_outbound();
        let store = session.store().expect("store");
        let rows = store.list_permission_requests("t-read").expect("ledger");
        if let Some(row) = rows.first() {
            assert_eq!(row.state, "answered");
            // Not a device: no human chose this.
            assert_eq!(row.decided_by.as_deref(), Some("host"));
            assert_eq!(row.option_id.as_deref(), Some("allow_once"));
            assert_eq!(row.kind.as_deref(), Some("read"));
            assert!(row.delivered, "the agent was never told");
            // And the human was never asked, so nothing is outstanding.
            assert!(pending(&mut session).is_empty());
            return;
        }
        assert!(Instant::now() < deadline, "the read was never auto-allowed");
        thread::sleep(Duration::from_millis(15));
    }
}
