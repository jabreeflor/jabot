//! OpenCode harness adapter (#220): success, startup/auth failure, cancel,
//! empty responses. Uses the fake ACP agent as the `opencode` runtime so the
//! catalog id is real and the wire is the same one a live `opencode acp` uses.

mod common;
use common::fake_agent;

use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{THREAD_OPEN, THREAD_STATE};
use jabot_lib::{
    HostSession, JsonRpcNotification, JsonRpcRequest, RequestId, HOST_HELLO, SESSION_CANCEL,
    SESSION_PROMPT, SESSION_UPDATE,
};
use serde_json::{json, Value};

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

fn hello(session: &mut HostSession) {
    session
        .handle_request(req(1, HOST_HELLO, None))
        .result
        .expect("hello");
}

fn hosted() -> (tempfile::TempDir, HostSession) {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    (dir, session)
}

fn open_opencode(session: &mut HostSession, cwd: &str, thread_id: &str, mode: Option<&str>) {
    let mut args = Vec::new();
    if let Some(mode) = mode {
        args.push(json!(mode));
    }
    let response = session.handle_request(req(
        2,
        THREAD_OPEN,
        Some(json!({
            "threadId": thread_id,
            "title": "OpenCode",
            "cwd": cwd,
            "harnessId": "opencode",
            "model": "anthropic/claude-sonnet-4-5",
            "runtime": {
                "command": fake_agent(),
                "args": args,
                "model": "anthropic/claude-sonnet-4-5"
            }
        })),
    ));
    assert!(
        response.error.is_none(),
        "thread/open failed: {:?}",
        response.error
    );
}

fn wait_for_state(
    session: &mut HostSession,
    thread_id: &str,
    pred: impl Fn(&Value) -> bool,
    timeout: Duration,
) -> Value {
    let start = Instant::now();
    let mut last = json!({});
    while start.elapsed() < timeout {
        session.pump_acp();
        let _ = session.take_outbound();
        let response =
            session.handle_request(req(9, THREAD_STATE, Some(json!({ "threadId": thread_id }))));
        if let Some(state) = response.result {
            last = state;
            if pred(&last) {
                return last;
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
    panic!("timed out waiting for {thread_id} state: {last}");
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

/// The first `session/update` is the host's echo of the prompt
/// (`user_message_chunk`). Wait for a particular later chunk.
fn wait_for_update(
    session: &mut HostSession,
    needle: &str,
    timeout: Duration,
) -> Vec<JsonRpcNotification> {
    let start = Instant::now();
    let mut found = Vec::new();
    while start.elapsed() < timeout {
        session.pump_acp();
        found.extend(session.take_outbound());
        if found.iter().any(|n| {
            n.method == SESSION_UPDATE
                && n.params
                    .as_ref()
                    .map(|p| p["acp"].to_string().contains(needle))
                    .unwrap_or(false)
        }) {
            return found;
        }
        thread::sleep(Duration::from_millis(15));
    }
    found
}

#[test]
fn catalog_lists_opencode_as_a_shipped_card() {
    let mut session = HostSession::ephemeral();
    hello(&mut session);
    let listed = session
        .handle_request(req(2, "harness/list", None))
        .result
        .expect("harness/list");
    let card = listed["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|card| card["id"] == "opencode")
        .expect("opencode card");
    assert_eq!(card["tier"], "shipped");
    assert_eq!(card["command"], "opencode");
    assert_eq!(card["args"], json!(["acp"]));
    assert_eq!(card["supportsModels"], true);
    assert!(card["declaredCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "resume"));
    assert!(card["accountIsolation"]
        .as_str()
        .unwrap()
        .contains("auth.json"));
}

#[test]
fn a_real_prompt_persists_a_reply_on_opencode() {
    let (dir, mut session) = hosted();
    let cwd = dir.path().to_string_lossy().into_owned();
    open_opencode(&mut session, &cwd, "t-ok", None);
    let response = session.handle_request(req(
        3,
        SESSION_PROMPT,
        Some(json!({
            "threadId": "t-ok",
            "content": "hi"
        })),
    ));
    assert_eq!(response.result.as_ref().unwrap()["accepted"], true);

    let outbound = wait_for_update(&mut session, "hello from fake-acp", Duration::from_secs(3));
    assert!(
        outbound.iter().any(|n| {
            n.method == SESSION_UPDATE
                && n.params
                    .as_ref()
                    .map(|p| p["acp"].to_string().contains("hello from fake-acp"))
                    .unwrap_or(false)
        }),
        "expected a persisted agent chunk, got {outbound:?}"
    );
    let state = wait_for_state(
        &mut session,
        "t-ok",
        |s| s["latestRun"]["state"] == "succeeded",
        Duration::from_secs(3),
    );
    assert_eq!(state["harnessId"], "opencode");
    assert_eq!(state["cwd"], cwd);
    assert_eq!(state["lastStopReason"], "end_turn");
}

#[test]
fn startup_auth_failure_is_an_error_not_success() {
    let (dir, mut session) = hosted();
    let cwd = dir.path().to_string_lossy().into_owned();
    open_opencode(&mut session, &cwd, "t-auth", Some("auth-fail"));
    let response = session.handle_request(req(
        3,
        SESSION_PROMPT,
        Some(json!({
            "threadId": "t-auth",
            "content": "hi"
        })),
    ));
    let error = response.error.expect("auth failure must surface");
    assert!(
        error.message.contains("Authentication") || error.message.contains("auth"),
        "{}",
        error.message
    );
}

#[test]
fn cancellation_ends_the_turn() {
    let (dir, mut session) = hosted();
    let cwd = dir.path().to_string_lossy().into_owned();
    open_opencode(&mut session, &cwd, "t-cancel", Some("cancellable"));
    session
        .handle_request(req(
            3,
            SESSION_PROMPT,
            Some(json!({ "threadId": "t-cancel", "content": "hi" })),
        ))
        .result
        .expect("prompt");
    let _ = wait_for(&mut session, SESSION_UPDATE, Duration::from_secs(3));
    let cancel = session.handle_request(req(
        4,
        SESSION_CANCEL,
        Some(json!({ "threadId": "t-cancel" })),
    ));
    assert_eq!(cancel.result.as_ref().unwrap()["cancelled"], true);
    let state = wait_for_state(
        &mut session,
        "t-cancel",
        |s| s["latestRun"]["state"] == "cancelled" || s["lastStopReason"] == "cancelled",
        Duration::from_secs(3),
    );
    assert_ne!(state["latestRun"]["state"], "succeeded");
}

#[test]
fn an_empty_reply_is_not_a_silent_success() {
    let (dir, mut session) = hosted();
    let cwd = dir.path().to_string_lossy().into_owned();
    open_opencode(&mut session, &cwd, "t-empty", Some("empty-reply"));
    session
        .handle_request(req(
            3,
            SESSION_PROMPT,
            Some(json!({ "threadId": "t-empty", "content": "hi" })),
        ))
        .result
        .expect("prompt");
    let state = wait_for_state(
        &mut session,
        "t-empty",
        |s| s["latestRun"]["state"] == "failed",
        Duration::from_secs(3),
    );
    assert_eq!(state["lastStopReason"], "empty_response");
    assert!(
        state["latestRun"]["error"]
            .as_str()
            .unwrap_or("")
            .contains("empty_response"),
        "{}",
        state["latestRun"]["error"]
    );
}
