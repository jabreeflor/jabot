//! Cursor's blocking extensions as questions and plans (#298).
//!
//! `cursor_adapter.rs` proves the round trip once. What is asserted here is
//! every way that round trip does *not* go smoothly: an answer that does not
//! fit, a second click, a cancelled turn, an agent that stopped waiting, an
//! adapter that died holding the question, a host that was quit, an
//! extension nobody renders, and a payload nobody can draw. Each used to end
//! in a synthetic `cancelled` the user never saw.

mod common;
use common::fake_agent;

use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{THREAD_OPEN, THREAD_STATE, THREAD_TRANSCRIPT};
use jabot_lib::{
    HostSession, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, NewThread, RequestId,
    ThreadRepo, HOST_HELLO, INTERACTION_ASK, INTERACTION_PENDING, INTERACTION_REPLY,
    INTERACTION_RESOLVED, PERMISSION_PENDING, PERMISSION_REPLY, SESSION_CANCEL, SESSION_PROMPT,
};
use serde_json::{json, Value};

const INVALID_PARAMS: i64 = -32602;

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

struct Host {
    session: HostSession,
    dir: tempfile::TempDir,
    device_id: String,
    next_id: i64,
    /// Every notification the host has pushed so far. Kept, not drained: an
    /// ask and its resolution can land in the same pump, and a wait for the
    /// second must not lose it to the wait for the first.
    inbox: Vec<JsonRpcNotification>,
}

impl Host {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self::on(HostSession::load(dir.path()), dir)
    }

    fn on(mut session: HostSession, dir: tempfile::TempDir) -> Self {
        let hello = session.handle_request(req(1, HOST_HELLO, None));
        let device_id = hello.result.expect("hello")["device"]["deviceId"]
            .as_str()
            .expect("deviceId")
            .to_string();
        Self {
            session,
            dir,
            device_id,
            next_id: 2,
            inbox: Vec::new(),
        }
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

    /// A thread the store knows about, so the ledger row can be written and
    /// a second host can find it.
    fn open_on(&mut self, thread_id: &str, mode: &str) {
        let runtime = json!({ "command": fake_agent(), "args": [mode] });
        self.ok(
            THREAD_OPEN,
            json!({
                "threadId": thread_id,
                "title": "Cursor thread",
                "cwd": self.dir.path().to_string_lossy(),
                "harnessId": "cursor",
                "runtime": runtime
            }),
        );
    }

    fn prompt(&mut self, thread_id: &str) {
        let response = self.call(
            SESSION_PROMPT,
            json!({ "threadId": thread_id, "content": "go" }),
        );
        assert!(response.error.is_none(), "{:?}", response.error);
    }

    fn state(&mut self, thread_id: &str) -> Value {
        self.ok(THREAD_STATE, json!({ "threadId": thread_id }))
    }

    fn transcript(&mut self, thread_id: &str) -> Value {
        self.ok(THREAD_TRANSCRIPT, json!({ "threadId": thread_id }))
    }

    fn interactions(&mut self, thread_id: &str) -> Vec<Value> {
        self.ok(INTERACTION_PENDING, json!({ "threadId": thread_id }))["requests"]
            .as_array()
            .expect("requests")
            .clone()
    }

    fn permissions(&mut self, thread_id: &str) -> Vec<Value> {
        self.ok(PERMISSION_PENDING, json!({ "threadId": thread_id }))["requests"]
            .as_array()
            .expect("requests")
            .clone()
    }

    /// Pump until a notification with this method has gone out for this
    /// thread, and return its params. Searches everything seen so far,
    /// oldest first.
    fn wait_for(&mut self, method: &str, thread_id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            self.session.pump_acp();
            self.inbox.extend(self.session.take_outbound());
            if let Some(found) = self.inbox.iter().find(|n| {
                n.method == method
                    && n.params
                        .as_ref()
                        .is_some_and(|params| params["threadId"] == thread_id)
            }) {
                return found.params.clone().expect("params");
            }
            assert!(
                Instant::now() < deadline,
                "no {method} went out; saw {:?}",
                self.inbox
            );
            thread::sleep(Duration::from_millis(15));
        }
    }

    fn settle(&mut self, thread_id: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            self.session.pump_acp();
            self.inbox.extend(self.session.take_outbound());
            let state = self.state(thread_id);
            if predicate(&state) {
                return state;
            }
            assert!(
                Instant::now() < deadline,
                "thread {thread_id} never settled; last state: {state}"
            );
            thread::sleep(Duration::from_millis(15));
        }
    }

    /// Prompt, and come back with the ask the agent sent.
    fn ask(&mut self, thread_id: &str, mode: &str) -> Value {
        self.open_on(thread_id, mode);
        self.prompt(thread_id);
        self.wait_for(INTERACTION_ASK, thread_id)
    }

    fn reply(&mut self, request_id: &str, outcome: &str, extra: Value) -> JsonRpcResponse {
        let mut params = json!({
            "requestId": request_id,
            "deviceId": self.device_id,
            "outcome": outcome,
        });
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                params[key] = value.clone();
            }
        }
        self.call(INTERACTION_REPLY, params)
    }

    fn resolved_events(&mut self, thread_id: &str) -> Vec<Value> {
        self.transcript(thread_id)["events"]
            .as_array()
            .expect("events")
            .iter()
            .filter(|event| event["method"] == INTERACTION_RESOLVED)
            .cloned()
            .collect()
    }
}

fn plan_answer() -> Value {
    json!({ "answers": [{ "questionId": "q1", "selectedOptionIds": ["plan"] }] })
}

#[test]
fn a_question_is_surfaced_answered_once_and_recorded() {
    let mut host = Host::start();
    let ask = host.ask("t-q", "cursor-ask");
    assert_eq!(ask["ask"], "question");
    assert_eq!(ask["method"], "cursor/ask_question");
    assert_eq!(ask["title"], "Need input");
    assert_eq!(ask["request"]["questions"][0]["id"], "q1");
    assert_eq!(ask["request"]["questions"][0]["options"][1]["id"], "plan");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();

    // Its own list, and not the permission one.
    let pending = host.interactions("t-q");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["requestId"], request_id);
    assert_eq!(pending[0]["stale"], false);
    assert!(host.permissions("t-q").is_empty());
    // The question itself is in the transcript, labelled by the method.
    let transcript = host.transcript("t-q");
    assert!(transcript["events"]
        .as_array()
        .expect("events")
        .iter()
        .any(|e| e["method"] == "cursor/ask_question" && e["payload"]["requestId"] == request_id));

    let first = host.reply(&request_id, "answered", plan_answer());
    let first = first.result.expect("answered");
    assert_eq!(first["delivered"], true);
    assert_eq!(first["alreadyAnswered"], false);
    assert_eq!(first["outcome"], "answered");
    assert_eq!(first["state"], "answered");

    // The agent heard exactly the option chosen, and the turn finished.
    let state = host.settle("t-q", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(state["latestRun"]["state"], "succeeded");
    let text = host.transcript("t-q").to_string();
    assert!(
        text.contains("selectedOptionIds") && text.contains("plan"),
        "answer never reached the agent: {text}"
    );
    assert!(host.interactions("t-q").is_empty());

    // The second click is a read.
    let second = host.reply(&request_id, "answered", plan_answer());
    let second = second.result.expect("already answered is not an error");
    assert_eq!(second["alreadyAnswered"], true);
    assert_eq!(second["outcome"], "answered");
    assert_eq!(host.resolved_events("t-q").len(), 1);
    let resolved = &host.resolved_events("t-q")[0]["payload"];
    assert_eq!(resolved["outcome"], "answered");
    assert_eq!(resolved["answers"][0]["selectedOptionIds"][0], "plan");
    assert_eq!(resolved["delivered"], true);
}

#[test]
fn an_answer_that_does_not_fit_is_refused_and_the_question_stays_open() {
    let mut host = Host::start();
    let ask = host.ask("t-bad-answer", "cursor-ask");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();

    let cases = [
        (
            "an option that was never offered",
            "answered",
            json!({ "answers": [{ "questionId": "q1", "selectedOptionIds": ["yolo"] }] }),
        ),
        (
            "two picks for a single choice",
            "answered",
            json!({ "answers": [{ "questionId": "q1", "selectedOptionIds": ["plan", "agent"] }] }),
        ),
        (
            "a question that was never asked",
            "answered",
            json!({ "answers": [{ "questionId": "q9", "selectedOptionIds": ["plan"] }] }),
        ),
        ("no answers at all", "answered", json!({})),
        ("a plan verb on a question", "accepted", json!({})),
        ("the host's own state", "expired", json!({})),
    ];
    for (why, outcome, extra) in cases {
        let response = host.reply(&request_id, outcome, extra);
        let error = response
            .error
            .unwrap_or_else(|| panic!("{why} must be refused"));
        assert_eq!(error.code, INVALID_PARAMS, "{why}: {error:?}");
        // Still there, still answerable, nothing sent.
        assert_eq!(host.interactions("t-bad-answer").len(), 1, "{why}");
        assert_eq!(
            host.state("t-bad-answer")["latestRun"]["state"],
            "needs_you"
        );
    }

    // A permission reply cannot settle it either.
    let cross = host.call(
        PERMISSION_REPLY,
        json!({ "requestId": request_id, "deviceId": host.device_id, "optionId": "plan" }),
    );
    assert_eq!(cross.error.expect("refused").code, INVALID_PARAMS);
    assert_eq!(host.interactions("t-bad-answer").len(), 1);

    // And then the real answer goes through.
    let answered = host.reply(&request_id, "answered", plan_answer());
    assert!(answered.error.is_none(), "{:?}", answered.error);
    host.settle("t-bad-answer", |s| s["latestRun"]["state"] == "succeeded");
}

#[test]
fn a_question_can_be_skipped_and_the_agent_is_told_why() {
    let mut host = Host::start();
    let ask = host.ask("t-skip", "cursor-ask");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();
    let skipped = host
        .reply(&request_id, "skipped", json!({ "reason": "not now" }))
        .result
        .expect("skipped");
    assert_eq!(skipped["outcome"], "skipped");
    assert_eq!(skipped["state"], "answered");
    host.settle("t-skip", |s| s["latestRun"]["state"] == "succeeded");
    let text = host.transcript("t-skip").to_string();
    assert!(
        text.contains("skipped") && text.contains("not now"),
        "{text}"
    );
}

#[test]
fn a_plan_is_accepted_or_rejected_in_cursors_own_words() {
    let mut host = Host::start();
    let ask = host.ask("t-plan-ok", "cursor-plan");
    assert_eq!(ask["ask"], "plan");
    assert_eq!(ask["title"], "Auth migration");
    assert_eq!(
        ask["request"]["todos"][1]["content"],
        "Migrate the middleware"
    );
    assert!(ask["request"]["plan"]
        .as_str()
        .unwrap()
        .starts_with("## Steps"));
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();
    // Not a permission: the permission list is empty and the permission
    // reply refuses it.
    assert!(host.permissions("t-plan-ok").is_empty());
    let accepted = host
        .reply(&request_id, "accepted", json!({}))
        .result
        .expect("accepted");
    assert_eq!(accepted["outcome"], "accepted");
    assert_eq!(accepted["delivered"], true);
    host.settle("t-plan-ok", |s| s["latestRun"]["state"] == "succeeded");
    // The fake agent streams what it was told back as `outcome=<json>`.
    let text = host.transcript("t-plan-ok").to_string();
    assert!(
        text.contains("outcome=") && text.contains("accepted"),
        "the agent never heard the accept: {text}"
    );

    let ask = host.ask("t-plan-no", "cursor-plan");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();
    let rejected = host
        .reply(&request_id, "rejected", json!({ "reason": "too broad" }))
        .result
        .expect("rejected");
    assert_eq!(rejected["outcome"], "rejected");
    host.settle("t-plan-no", |s| s["latestRun"]["state"] == "succeeded");
    let text = host.transcript("t-plan-no").to_string();
    assert!(
        text.contains("rejected") && text.contains("too broad"),
        "{text}"
    );
    // A question verb on a plan is refused.
    let ask = host.ask("t-plan-x", "cursor-plan");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();
    let wrong = host.reply(&request_id, "answered", plan_answer());
    assert_eq!(wrong.error.expect("refused").code, INVALID_PARAMS);
    assert_eq!(host.interactions("t-plan-x").len(), 1);
}

#[test]
fn cancelling_the_turn_cancels_the_question_and_a_late_answer_is_a_read() {
    let mut host = Host::start();
    let ask = host.ask("t-cancel", "cursor-ask");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();

    let cancelled = host.ok(SESSION_CANCEL, json!({ "threadId": "t-cancel" }));
    assert_eq!(cancelled["cancelled"], true);
    let resolved = host.wait_for(INTERACTION_RESOLVED, "t-cancel");
    assert_eq!(resolved["requestId"], request_id);
    assert_eq!(resolved["outcome"], "cancelled");
    assert_eq!(resolved["deviceId"], "host");
    assert!(host.interactions("t-cancel").is_empty());

    let late = host
        .reply(&request_id, "answered", plan_answer())
        .result
        .expect("a late answer is a read, not an error");
    assert_eq!(late["alreadyAnswered"], true);
    assert_eq!(late["outcome"], "cancelled");
    assert_eq!(late["state"], "cancelled");
    assert_eq!(host.resolved_events("t-cancel").len(), 1);
}

#[test]
fn a_question_the_agent_stopped_waiting_on_expires() {
    let mut host = Host::start();
    let ask = host.ask("t-expire", "cursor-ask-expire");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();
    let resolved = host.wait_for(INTERACTION_RESOLVED, "t-expire");
    assert_eq!(resolved["requestId"], request_id);
    assert_eq!(resolved["outcome"], "expired");
    assert!(host.interactions("t-expire").is_empty());
    let late = host
        .reply(&request_id, "answered", plan_answer())
        .result
        .expect("read");
    assert_eq!(late["alreadyAnswered"], true);
    assert_eq!(late["state"], "expired");
    // The turn itself finished normally; the ask did not hold it.
    let state = host.settle("t-expire", |s| s["latestRun"]["state"] != "running");
    assert_ne!(state["latestRun"]["state"], "needs_you", "{state}");
}

#[test]
fn a_question_whose_adapter_died_is_unavailable() {
    let mut host = Host::start();
    let ask = host.ask("t-exit", "cursor-ask-exit");
    let request_id = ask["requestId"].as_str().expect("requestId").to_string();
    let resolved = host.wait_for(INTERACTION_RESOLVED, "t-exit");
    assert_eq!(resolved["requestId"], request_id);
    assert_eq!(resolved["outcome"], "unavailable");
    assert_eq!(resolved["delivered"], false);
    assert!(host.interactions("t-exit").is_empty());
    let late = host
        .reply(&request_id, "accepted", json!({}))
        .result
        .expect("read");
    assert_eq!(late["alreadyAnswered"], true);
    assert_eq!(late["state"], "unavailable");
}

/// Cmd-Q with a question on the screen. A permission comes back answerable
/// (#20); a question does not — Cursor will not ask again, and the next
/// launch must not offer buttons that reach nobody.
#[test]
fn a_question_does_not_outlive_the_host_as_answerable() {
    question_left_open_by_a_host_that_went_away(true);
}

/// The same, when the host never got to say goodbye: a crash, a kill, a
/// daemon stopped mid-ask. The row was still `pending` on disk, and the boot
/// pass is what closes it before any client can be handed the card.
#[test]
fn a_question_left_by_a_crashed_host_is_closed_at_boot() {
    question_left_open_by_a_host_that_went_away(false);
}

fn question_left_open_by_a_host_that_went_away(clean_quit: bool) {
    let dir = tempfile::tempdir().unwrap();
    let first = HostSession::load(dir.path());
    let runtime = json!({ "command": fake_agent(), "args": ["cursor-ask"] }).to_string();
    first
        .store()
        .expect("store")
        .insert_thread(&NewThread {
            id: "t-quit".into(),
            folder_id: None,
            bot_id: None,
            harness_id: "cursor".into(),
            cwd: dir.path().to_string_lossy().into(),
            runtime_json: runtime,
            title: "Cursor thread".into(),
            fold_policy: "default".into(),
            worktree_path: None,
            repo: ThreadRepo::default(),
        })
        .expect("thread row");
    let mut host = Host::on(first, dir);
    host.prompt("t-quit");
    let request_id = host.wait_for(INTERACTION_ASK, "t-quit")["requestId"]
        .as_str()
        .expect("requestId")
        .to_string();
    assert_eq!(host.interactions("t-quit").len(), 1);

    if clean_quit {
        host.session.shutdown_adapters();
    }
    // Dropping the session without a shutdown is the crash: the adapter is
    // killed with it and the row is left exactly as it was.
    let Host { session, dir, .. } = host;
    drop(session);

    let mut next = Host::on(HostSession::load(dir.path()), dir);
    assert!(
        next.interactions("t-quit").is_empty(),
        "a dead question must not come back answerable"
    );
    let rows = next
        .session
        .store()
        .expect("store")
        .list_permission_requests("t-quit")
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, request_id);
    assert_eq!(rows[0].state, "unavailable");
    assert_eq!(rows[0].ask, "question");
    assert_eq!(rows[0].method.as_deref(), Some("cursor/ask_question"));
    // The transcript says so, so a reopened thread draws a closed card.
    let resolved = next.resolved_events("t-quit");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["payload"]["outcome"], "unavailable");
    // A late answer is a read of what stands — `unavailable` — and never a
    // second answer. (`delivered` here is about the *withdrawal*: the agent
    // was told `cancelled` before the host killed it, which is true.)
    let late = next
        .reply(&request_id, "answered", plan_answer())
        .result
        .expect("read");
    assert_eq!(late["alreadyAnswered"], true);
    assert_eq!(late["outcome"], "unavailable");
    assert_eq!(late["state"], "unavailable");
    assert_eq!(next.resolved_events("t-quit").len(), 1);
}

#[test]
fn an_extension_nobody_renders_is_refused_and_the_turn_finishes() {
    let mut host = Host::start();
    host.open_on("t-todos", "cursor-todos");
    host.prompt("t-todos");
    let state = host.settle("t-todos", |s| {
        matches!(
            s["latestRun"]["state"].as_str(),
            Some("succeeded") | Some("failed")
        )
    });
    assert_ne!(state["latestRun"]["state"], "running");
    let text = host.transcript("t-todos").to_string();
    assert!(text.contains("refused=-32601"), "{text}");
    assert!(host.interactions("t-todos").is_empty());
}

#[test]
fn a_question_that_cannot_be_drawn_is_refused_not_rendered() {
    let mut host = Host::start();
    host.open_on("t-bad", "cursor-ask-bad");
    host.prompt("t-bad");
    let state = host.settle("t-bad", |s| {
        matches!(
            s["latestRun"]["state"].as_str(),
            Some("succeeded") | Some("failed")
        )
    });
    assert_ne!(state["latestRun"]["state"], "running");
    let text = host.transcript("t-bad").to_string();
    assert!(text.contains("refused=-32601"), "{text}");
    // Nothing was drawn and nothing was recorded as asked.
    assert!(host.interactions("t-bad").is_empty());
    assert!(!text.contains("interaction/ask"));
}
