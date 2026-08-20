//! ACP adapter supervisor (#10): spawn, prompt, permission, cancel, kill.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::{
    HostSession, JsonRpcNotification, JsonRpcRequest, NewThread, RequestId, ThreadRepo, HOST_HELLO,
    PERMISSION_ASK, PERMISSION_REPLY, SESSION_CANCEL, SESSION_PROMPT, SESSION_UPDATE,
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

fn prompt_params(thread_id: &str, content: &str, mode: Option<&str>) -> Value {
    let mut args = Vec::new();
    if let Some(mode) = mode {
        args.push(mode);
    }
    json!({
        "threadId": thread_id,
        "content": content,
        "cwd": std::env::temp_dir().to_string_lossy(),
        "runtime": {
            "command": fake_agent(),
            "args": args
        }
    })
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

/// Poll the adapter's teed stderr until it has read the cancel, then return it
/// split into lines. The file is written by the child process, so it lags the
/// host's write by a scheduling hop rather than appearing atomically.
fn wait_for_log(path: &std::path::Path, timeout: Duration) -> Vec<String> {
    let start = Instant::now();
    loop {
        let lines: Vec<String> = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        if lines.iter().any(|line| line == "cancelled") || start.elapsed() >= timeout {
            return lines;
        }
        thread::sleep(Duration::from_millis(15));
    }
}

/// Like [`wait_for`], but for one particular `session/update` — several now
/// arrive on a thread and the interesting one is rarely the first.
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

fn result_value(response: &jabot_lib::JsonRpcResponse) -> &Value {
    response.result.as_ref().expect("expected result")
}

#[test]
fn prompt_streams_session_update() {
    let mut session = HostSession::ephemeral();
    hello(&mut session);
    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(prompt_params("t-echo", "hi", None)),
    ));
    let value = result_value(&response);
    assert_eq!(value["accepted"], true);
    assert_eq!(value["acpSessionId"], "sess-fake-1");

    // The first `session/update` on a thread is now the host's own echo of the
    // prompt (#14 writes the user's words into the transcript), so this looks
    // for the agent's chunk rather than for whichever update arrives first.
    let outbound = wait_for_update(&mut session, "hello from fake-acp", Duration::from_secs(3));
    let update = outbound
        .iter()
        .find(|n| {
            n.method == SESSION_UPDATE
                && n.params.as_ref().unwrap()["acp"]
                    .to_string()
                    .contains("hello from fake-acp")
        })
        .expect("the agent's message chunk");
    assert_eq!(
        update.params.as_ref().unwrap()["acp"]["sessionUpdate"],
        "agent_message_chunk"
    );
    assert_eq!(session.live_adapter_count(), 1);
}

#[test]
fn missing_binary_returns_install_hint() {
    let mut session = HostSession::ephemeral();
    hello(&mut session);
    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(json!({
            "threadId": "t-missing",
            "content": "hi",
            "runtime": {
                "command": "jabot-definitely-not-on-path-xyz",
                "installHint": "npm i -g jabot-definitely-not-on-path-xyz"
            }
        })),
    ));
    let error = response.error.expect("harness unavailable");
    assert_eq!(error.code, -32004);
    assert_eq!(
        error.data.as_ref().unwrap()["command"],
        "jabot-definitely-not-on-path-xyz"
    );
    assert_eq!(
        error.data.as_ref().unwrap()["installHint"],
        "npm i -g jabot-definitely-not-on-path-xyz"
    );
}

#[test]
fn permission_roundtrip() {
    let mut session = HostSession::ephemeral();
    let device_id = hello(&mut session);
    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(prompt_params("t-perm", "rm -rf", Some("permission"))),
    ));
    assert!(response.error.is_none(), "{:?}", response.error);

    let outbound = wait_for(&mut session, PERMISSION_ASK, Duration::from_secs(3));
    let ask = outbound
        .iter()
        .find(|n| n.method == PERMISSION_ASK)
        .expect("permission/ask");
    let request_id = ask.params.as_ref().unwrap()["requestId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(ask.params.as_ref().unwrap()["subject"]["kind"], "execute");

    let reply = session.handle_request(req(
        3,
        PERMISSION_REPLY,
        Some(json!({
            "requestId": request_id,
            "deviceId": device_id,
            "optionId": "allow_once"
        })),
    ));
    assert_eq!(result_value(&reply)["delivered"], true);

    let after = wait_for(&mut session, SESSION_UPDATE, Duration::from_secs(3));
    let allowed = after.iter().any(|n| {
        n.method == SESSION_UPDATE
            && n.params
                .as_ref()
                .unwrap()
                .get("acp")
                .map(|a| a.to_string().contains("allowed"))
                .unwrap_or(false)
    });
    assert!(allowed, "expected allowed chunk, got {after:?}");
}

#[test]
fn cancel_keeps_process_alive() {
    let mut session = HostSession::ephemeral();
    hello(&mut session);
    session
        .handle_request(req(
            2,
            SESSION_PROMPT,
            Some(prompt_params("t-cancel", "hi", None)),
        ))
        .result
        .expect("prompt");
    let _ = wait_for(&mut session, SESSION_UPDATE, Duration::from_secs(3));
    assert_eq!(session.live_adapter_count(), 1);

    let cancel = session.handle_request(req(
        3,
        SESSION_CANCEL,
        Some(json!({ "threadId": "t-cancel" })),
    ));
    assert_eq!(result_value(&cancel)["cancelled"], true);
    assert_eq!(session.live_adapter_count(), 1);
}

/// #10's ordering claim, in-process: a permission still outstanding when the
/// user cancels is answered `cancelled` *before* `session/cancel` reaches the
/// agent. An agent that sees the turn torn down while it is still blocked on a
/// request it never got an answer to is the hang this ordering exists to
/// prevent. The fake agent narrates both events to stderr as it reads them and
/// the host tees that into the thread log, so the log is a faithful record of
/// the order the host wrote to the adapter's stdin — which `live_adapter_count`
/// or the outbound notifications alone cannot show.
#[test]
fn cancel_answers_pending_permission_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(prompt_params("t-order", "rm -rf", Some("permission"))),
    ));
    assert!(response.error.is_none(), "{:?}", response.error);
    let outbound = wait_for(&mut session, PERMISSION_ASK, Duration::from_secs(5));
    assert!(
        outbound.iter().any(|n| n.method == PERMISSION_ASK),
        "adapter never asked for permission: {outbound:?}"
    );

    let cancel = session.handle_request(req(
        3,
        SESSION_CANCEL,
        Some(json!({ "threadId": "t-order" })),
    ));
    assert_eq!(result_value(&cancel)["cancelled"], true);

    let log = dir.path().join("adapter-logs").join("t-order.stderr.log");
    let lines = wait_for_log(&log, Duration::from_secs(5));
    let reply = lines
        .iter()
        .position(|line| line.starts_with("permission_reply="))
        .unwrap_or_else(|| panic!("adapter never read a permission outcome: {lines:?}"));
    let cancelled = lines
        .iter()
        .position(|line| line == "cancelled")
        .unwrap_or_else(|| panic!("adapter never read session/cancel: {lines:?}"));
    assert!(
        reply < cancelled,
        "session/cancel reached the agent before the pending permission was answered: {lines:?}"
    );
}

#[test]
fn persists_acp_session_id_on_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    hello(&mut session);
    let runtime = json!({
        "command": fake_agent(),
        "args": []
    })
    .to_string();
    session
        .store()
        .unwrap()
        .insert_thread(&NewThread {
            id: "t-store".into(),
            folder_id: None,
            bot_id: Some("code".into()),
            harness_id: "claude".into(),
            cwd: dir.path().to_string_lossy().into(),
            runtime_json: runtime,
            title: "Stored thread".into(),
            fold_policy: "default".into(),
            repo: ThreadRepo::default(),
        })
        .unwrap();

    let response = session.handle_request(req(
        2,
        SESSION_PROMPT,
        Some(json!({
            "threadId": "t-store",
            "content": "hi"
        })),
    ));
    assert!(response.error.is_none(), "{:?}", response.error);
    let thread = session
        .store()
        .unwrap()
        .get_thread("t-store")
        .unwrap()
        .unwrap();
    assert_eq!(thread.acp_session_id.as_deref(), Some("sess-fake-1"));
}

#[test]
fn shutdown_kills_adapter() {
    let mut session = HostSession::ephemeral();
    hello(&mut session);
    session
        .handle_request(req(
            2,
            SESSION_PROMPT,
            Some(prompt_params("t-kill", "hi", Some("grandchild"))),
        ))
        .result
        .expect("prompt");
    assert_eq!(session.live_adapter_count(), 1);
    session.shutdown_adapters();
    assert_eq!(session.live_adapter_count(), 0);
}
