//! Gemini CLI harness (#219): catalog id, spawn, persist, cancel, empty reply.
//!
//! The real `gemini --acp` binary is not required. These cases pin the host
//! contract for the `gemini` card — a missing CLI never looks like success,
//! a real turn is stored on the thread that opened it, cancel stays honest,
//! and an empty `end_turn` is `empty_response`.

mod common;
use common::fake_agent;

use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcNotification, JsonRpcRequest, NewThread, RequestId, ThreadRepo, HOST_HELLO,
    SESSION_CANCEL, SESSION_PROMPT, SESSION_UPDATE, THREAD_OPEN, THREAD_STATE, THREAD_TRANSCRIPT,
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

fn settle_state(session: &mut HostSession, thread_id: &str, needle: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        session.pump_acp();
        let state = session
            .handle_request(req(
                90,
                THREAD_STATE,
                Some(json!({ "threadId": thread_id })),
            ))
            .result
            .expect("thread/state");
        if state.to_string().contains(needle) {
            return state;
        }
        if Instant::now() > deadline {
            panic!("thread {thread_id} never reached {needle}: {state}");
        }
        thread::sleep(Duration::from_millis(15));
    }
}

fn open_gemini(session: &mut HostSession, thread_id: &str, cwd: &str, mode: Option<&str>) -> Value {
    let mut args = Vec::new();
    if let Some(mode) = mode {
        args.push(mode);
    }
    session
        .handle_request(req(
            2,
            THREAD_OPEN,
            Some(json!({
                "threadId": thread_id,
                "title": "Gemini thread",
                "cwd": cwd,
                "harnessId": "gemini",
                "runtime": { "command": fake_agent(), "args": args }
            })),
        ))
        .result
        .unwrap_or_else(|| panic!("thread/open gemini"))
}

#[test]
fn gemini_prompt_persists_a_reply_on_the_opened_thread() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("worktree");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);

    let opened = open_gemini(&mut session, "t-gemini", &cwd.to_string_lossy(), None);
    assert_eq!(opened["harnessId"], "gemini");
    assert_eq!(opened["cwd"], cwd.to_string_lossy().as_ref());

    let response = session.handle_request(req(
        3,
        SESSION_PROMPT,
        Some(json!({ "threadId": "t-gemini", "content": "hi from the worktree" })),
    ));
    let value = response.result.expect("prompt accepted");
    assert_eq!(value["accepted"], true);

    let outbound = wait_for_update(&mut session, "hello from fake-acp", Duration::from_secs(3));
    assert!(
        outbound.iter().any(|n| n.method == SESSION_UPDATE),
        "no streamed chunk: {outbound:?}"
    );

    let transcript = session
        .handle_request(req(
            4,
            THREAD_TRANSCRIPT,
            Some(json!({ "threadId": "t-gemini" })),
        ))
        .result
        .expect("transcript");
    assert!(
        transcript.to_string().contains("hello from fake-acp"),
        "reply was not persisted: {transcript}"
    );
    let thread = session
        .store()
        .unwrap()
        .get_thread("t-gemini")
        .unwrap()
        .unwrap();
    assert_eq!(thread.harness_id, "gemini");
    assert_eq!(thread.cwd, cwd.to_string_lossy());
    assert_eq!(thread.acp_session_id.as_deref(), Some("sess-fake-1"));
}

#[test]
fn gemini_missing_binary_is_an_error_not_a_success() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    session
        .store()
        .unwrap()
        .insert_thread(&NewThread {
            id: "t-missing".into(),
            folder_id: None,
            bot_id: Some("bot-recruiter".into()),
            harness_id: "gemini".into(),
            cwd: dir.path().to_string_lossy().into(),
            runtime_json: json!({
                "command": "jabot-gemini-not-on-path",
                "args": ["--acp"],
                "installHint": "Install Gemini CLI (`npm i -g @google/gemini-cli`)."
            })
            .to_string(),
            title: "Missing Gemini".into(),
            fold_policy: "default".into(),
            worktree_path: None,
            repo: ThreadRepo::default(),
        })
        .unwrap();

    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(json!({ "threadId": "t-missing", "content": "hi" })),
    ));
    let error = response.error.expect("startup must fail");
    assert_eq!(error.code, -32004);
    assert_eq!(
        error.data.as_ref().unwrap()["command"],
        "jabot-gemini-not-on-path"
    );
    assert!(
        error.data.as_ref().unwrap()["installHint"]
            .as_str()
            .unwrap()
            .contains("gemini-cli"),
        "{error:?}"
    );
}

#[test]
fn gemini_cancel_does_not_report_success() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    open_gemini(
        &mut session,
        "t-cancel",
        &dir.path().to_string_lossy(),
        Some("cancellable"),
    );
    session
        .handle_request(req(
            3,
            SESSION_PROMPT,
            Some(json!({ "threadId": "t-cancel", "content": "hi" })),
        ))
        .result
        .expect("prompt");
    let _ = wait_for_update(&mut session, "hello from fake-acp", Duration::from_secs(3));

    let cancel = session.handle_request(req(
        4,
        SESSION_CANCEL,
        Some(json!({ "threadId": "t-cancel" })),
    ));
    assert_eq!(cancel.result.expect("cancel")["cancelled"], true);
    assert_eq!(session.live_adapter_count(), 1);
}

#[test]
fn gemini_empty_reply_is_a_failed_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    open_gemini(
        &mut session,
        "t-empty",
        &dir.path().to_string_lossy(),
        Some("empty-reply"),
    );
    session
        .handle_request(req(
            3,
            SESSION_PROMPT,
            Some(json!({ "threadId": "t-empty", "content": "hi" })),
        ))
        .result
        .expect("prompt");

    let state = settle_state(&mut session, "t-empty", "empty_response");
    assert_eq!(state["lastStopReason"], "empty_response");
    assert_ne!(state["latestRun"]["state"], "succeeded");
}
