//! Cursor Agent CLI adapter (#222): success, auth failure, cancel, empty.
//!
//! The catalog launches `agent acp` without `--force`. These cases drive the
//! real supervisor against `fake-acp-agent` in Cursor-shaped modes so a
//! startup failure or an empty turn cannot be mistaken for a successful reply.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcRequest, JsonRpcResponse, RequestId, HOST_HELLO, SESSION_CANCEL,
    SESSION_PROMPT, THREAD_OPEN, THREAD_STATE, THREAD_TRANSCRIPT,
};
use serde_json::{json, Value};

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

fn fake_agent() -> String {
    if let Some(path) = option_env!("CARGO_BIN_EXE_fake_acp_agent") {
        return path.to_string();
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("target/debug/fake-acp-agent")
        .to_string_lossy()
        .into_owned()
}

struct Host {
    session: HostSession,
    dir: tempfile::TempDir,
    next_id: i64,
}

impl Host {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut host = Self {
            session: HostSession::load(dir.path()),
            dir,
            next_id: 2,
        };
        host.session.handle_request(req(1, HOST_HELLO, None));
        host
    }

    fn call(&mut self, method: &str, params: Value) -> JsonRpcResponse {
        let id = self.next_id;
        self.next_id += 1;
        self.session.handle_request(req(id, method, Some(params)))
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let response = self.call(method, params);
        assert!(
            response.error.is_none(),
            "{method} failed: {:?}",
            response.error
        );
        response.result.expect("result")
    }

    fn open_on(&mut self, thread_id: &str, mode: &str) -> Value {
        self.ok(
            THREAD_OPEN,
            json!({
                "threadId": thread_id,
                "title": "Cursor thread",
                "cwd": self.dir.path().to_string_lossy(),
                "harnessId": "cursor",
                "runtime": { "command": fake_agent(), "args": [mode] }
            }),
        )
    }

    fn prompt(&mut self, thread_id: &str) -> JsonRpcResponse {
        self.call(
            SESSION_PROMPT,
            json!({ "threadId": thread_id, "content": "hi" }),
        )
    }

    fn state(&mut self, thread_id: &str) -> Value {
        self.ok(THREAD_STATE, json!({ "threadId": thread_id }))
    }

    fn settle(&mut self, thread_id: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            self.session.pump_acp();
            let state = self.state(thread_id);
            if predicate(&state) {
                return state;
            }
            if Instant::now() > deadline {
                panic!("thread {thread_id} never settled; last state: {state}");
            }
            thread::sleep(Duration::from_millis(15));
        }
    }
}

#[test]
fn a_real_prompt_persists_a_reply_in_the_thread_worktree() {
    let mut host = Host::start();
    let cwd = host.dir.path().to_string_lossy().into_owned();
    let opened = host.open_on("t-cursor-ok", "cursor-login");
    assert_eq!(opened["harnessId"], "cursor");
    assert_eq!(opened["cwd"], cwd);

    let accepted = host.prompt("t-cursor-ok");
    assert!(accepted.error.is_none(), "{:?}", accepted.error);
    let state = host.settle("t-cursor-ok", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(state["cwd"], cwd);
    assert_eq!(state["latestRun"]["state"], "succeeded");

    let transcript = host.ok(THREAD_TRANSCRIPT, json!({ "threadId": "t-cursor-ok" }));
    let events = transcript["events"].as_array().expect("events");
    let said = events
        .iter()
        .any(|event| event["payload"].to_string().contains("hello from fake-acp"));
    assert!(said, "persisted transcript missing the reply: {transcript}");
}

#[test]
fn startup_auth_failure_is_an_error_not_a_successful_turn() {
    let mut host = Host::start();
    host.open_on("t-cursor-auth", "cursor-auth-fail");
    let response = host.prompt("t-cursor-auth");
    assert!(
        response.error.is_some(),
        "auth failure must not look like success: {:?}",
        response.result
    );
    let state = host.state("t-cursor-auth");
    assert_ne!(state["latestRun"]["state"], "succeeded");
}

#[test]
fn cancellation_ends_the_turn_without_a_success() {
    let mut host = Host::start();
    host.open_on("t-cursor-cancel", "cancellable");
    host.prompt("t-cursor-cancel");
    host.settle("t-cursor-cancel", |s| s["latestRun"]["state"] == "running");
    let cancelled = host.ok(SESSION_CANCEL, json!({ "threadId": "t-cursor-cancel" }));
    assert_eq!(cancelled["cancelled"], true);
    let state = host.settle("t-cursor-cancel", |s| {
        matches!(
            s["latestRun"]["state"].as_str(),
            Some("cancelled") | Some("failed")
        )
    });
    assert_ne!(state["latestRun"]["state"], "succeeded");
}

#[test]
fn an_empty_response_fails_instead_of_reporting_success() {
    let mut host = Host::start();
    host.open_on("t-cursor-empty", "empty-reply");
    host.prompt("t-cursor-empty");
    let state = host.settle("t-cursor-empty", |s| s["latestRun"]["state"] == "failed");
    assert_eq!(state["lastStopReason"], "empty_response");
    assert!(
        state["latestRun"]["error"]
            .as_str()
            .unwrap_or("")
            .contains("empty_response"),
        "{}",
        state["latestRun"]
    );
}

#[test]
fn a_blocking_cursor_question_is_declined_and_the_turn_finishes() {
    let mut host = Host::start();
    host.open_on("t-cursor-ask", "cursor-ask");
    let accepted = host.prompt("t-cursor-ask");
    assert!(accepted.error.is_none(), "{:?}", accepted.error);
    let state = host.settle("t-cursor-ask", |s| {
        matches!(
            s["latestRun"]["state"].as_str(),
            Some("succeeded") | Some("failed")
        )
    });
    assert!(
        state["latestRun"]["state"] != "running",
        "cursor/ask_question left the turn hung: {state}"
    );
}
