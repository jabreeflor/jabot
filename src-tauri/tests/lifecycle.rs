//! Thread state machine + run ledger (#15) driven through the host API.
//!
//! Every case here goes in as a JSON-RPC request and comes back out as a store
//! row or a notification, because that is the only surface #22 and #26 will
//! have. Where a case needs an agent it gets the real `fake-acp-agent`
//! subprocess over real ACP stdio, not a stub.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId, HOST_HELLO,
    INBOX_LIST, INBOX_RESURFACE, PERMISSION_ASK, PERMISSION_REPLY, SESSION_PROMPT, THREAD_ARCHIVE,
    THREAD_DELETE, THREAD_FOLD, THREAD_OPEN, THREAD_REOPEN, THREAD_STATE,
};
use serde_json::{json, Value};

const ILLEGAL_TRANSITION: i64 = -32005;
const THREAD_NOT_FOUND: i64 = -32006;
const RUN_IN_FLIGHT: i64 = -32008;

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

struct Host {
    session: HostSession,
    dir: tempfile::TempDir,
    next_id: i64,
}

impl Host {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        session.handle_request(req(1, HOST_HELLO, None));
        Self {
            session,
            dir,
            next_id: 2,
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

    fn err(&mut self, method: &str, params: Value) -> jabot_lib::host::JsonRpcError {
        self.call(method, params).error.expect("expected an error")
    }

    /// A thread whose runtime is the fake agent, so `session/prompt` spawns
    /// something that really speaks ACP.
    fn open_thread(&mut self, thread_id: &str, mode: Option<&str>) -> Value {
        let mut args: Vec<String> = Vec::new();
        if let Some(mode) = mode {
            args.push(mode.to_string());
        }
        self.ok(
            THREAD_OPEN,
            json!({
                "threadId": thread_id,
                "title": "Auth migration",
                "cwd": self.dir.path().to_string_lossy(),
                "harnessId": "claude",
                "runtime": { "command": fake_agent(), "args": args }
            }),
        )
    }

    fn prompt(&mut self, thread_id: &str) -> Value {
        self.ok(
            SESSION_PROMPT,
            json!({ "threadId": thread_id, "content": "hi" }),
        )
    }

    fn state(&mut self, thread_id: &str) -> Value {
        self.ok(THREAD_STATE, json!({ "threadId": thread_id }))
    }

    /// Pump until `predicate` holds on the thread's state, or give up.
    fn settle(&mut self, thread_id: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
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

    /// Quit and relaunch against the same data dir — the `TempDir` stays alive
    /// so the store is reopened, not recreated.
    fn restart(&mut self) {
        let path = self.dir.path().to_path_buf();
        drop(std::mem::replace(
            &mut self.session,
            HostSession::ephemeral(),
        ));
        self.session = HostSession::load(&path);
        self.session.handle_request(req(1, HOST_HELLO, None));
    }

    fn drain(&mut self) -> Vec<JsonRpcNotification> {
        self.session.pump_acp();
        self.session.take_outbound()
    }
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

fn kinds(inbox: &Value) -> Vec<String> {
    inbox["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn open_fold_reopen_walks_the_overlay_and_persists_it() {
    let mut host = Host::start();
    let opened = host.open_thread("t-walk", None);
    assert_eq!(opened["state"], "active");
    assert_eq!(opened["foldPolicy"], "default");

    let folded = host.ok(
        THREAD_FOLD,
        json!({ "threadId": "t-walk", "policy": "wait_for_inbox" }),
    );
    assert_eq!(folded["state"], "folded");
    assert_eq!(folded["foldPolicy"], "wait_for_inbox");
    assert!(folded["foldedAt"].is_string());

    // Still Sleeping is a projection of the thread row, not an event: folding
    // must not manufacture an Inbox card.
    let inbox = host.ok(INBOX_LIST, json!({}));
    assert!(kinds(&inbox).is_empty());
    assert_eq!(inbox["sleeping"][0]["threadId"], "t-walk");
    assert_eq!(inbox["sleeping"][0]["foldPolicy"], "wait_for_inbox");

    let reopened = host.ok(THREAD_REOPEN, json!({ "threadId": "t-walk" }));
    assert_eq!(reopened["state"], "active");

    // The overlay is on disk, not in the supervisor's head.
    let store = host.session.store().unwrap();
    assert_eq!(store.get_thread("t-walk").unwrap().unwrap().state, "active");
    assert_eq!(
        store.get_thread("t-walk").unwrap().unwrap().fold_policy,
        "wait_for_inbox"
    );
}

#[test]
fn an_illegal_transition_is_an_error_not_a_silent_no_op() {
    let mut host = Host::start();
    host.open_thread("t-illegal", None);
    host.ok(THREAD_FOLD, json!({ "threadId": "t-illegal" }));

    // Folding a folded thread is not in the table.
    let err = host.err(THREAD_FOLD, json!({ "threadId": "t-illegal" }));
    assert_eq!(err.code, ILLEGAL_TRANSITION);
    assert_eq!(err.data.as_ref().unwrap()["from"], "folded");
    assert_eq!(err.data.as_ref().unwrap()["action"], "fold");
    // And it left the thread where it was, rather than half-moving it.
    assert_eq!(host.state("t-illegal")["state"], "folded");

    // The policy is part of the row: a refused fold must not quietly make the
    // thread quieter on its way back out as an error.
    let with_policy = host.err(
        THREAD_FOLD,
        json!({ "threadId": "t-illegal", "policy": "wait_for_inbox" }),
    );
    assert_eq!(with_policy.code, ILLEGAL_TRANSITION);
    assert_eq!(host.state("t-illegal")["foldPolicy"], "default");

    host.ok(THREAD_DELETE, json!({ "threadId": "t-illegal" }));
    assert_eq!(host.state("t-illegal")["state"], "deleted");
    // Deleted has no outbound edges at all.
    for method in [THREAD_FOLD, THREAD_REOPEN, THREAD_ARCHIVE, THREAD_DELETE] {
        let err = host.err(method, json!({ "threadId": "t-illegal" }));
        assert_eq!(err.code, ILLEGAL_TRANSITION, "{method}");
    }

    let missing = host.err(THREAD_FOLD, json!({ "threadId": "t-never-existed" }));
    assert_eq!(missing.code, THREAD_NOT_FOUND);
}

#[test]
fn a_prompt_opens_a_run_and_end_turn_closes_it() {
    let mut host = Host::start();
    host.open_thread("t-run", None);
    let accepted = host.prompt("t-run");
    assert_eq!(accepted["acpSessionId"], "sess-fake-1");

    let state = host.settle("t-run", |s| s["latestRun"]["state"] == "succeeded");
    let run = &state["latestRun"];
    assert_eq!(run["seq"], 1);
    assert_eq!(run["kind"], "prompt");
    // The run is stamped with the ACP session it executed on, so a later
    // resume that mints a new session id cannot rewrite this one's history.
    assert_eq!(run["acpSessionId"], "sess-fake-1");
    assert!(run["startedAt"].is_string());
    assert!(run["endedAt"].is_string());
    assert_eq!(state["lastStopReason"], "end_turn");
    // Active threads do not resurface; finishing shows in chat, not the Inbox.
    assert_eq!(state["state"], "active");
    assert!(kinds(&host.ok(INBOX_LIST, json!({}))).is_empty());

    // A second prompt is a second run on the same session (#5).
    host.prompt("t-run");
    let second = host.settle("t-run", |s| s["latestRun"]["seq"] == 2);
    assert_eq!(second["runs"].as_array().unwrap().len(), 2);
    assert_eq!(second["latestRun"]["acpSessionId"], "sess-fake-1");
}

#[test]
fn a_failed_stop_reason_is_failed_not_done() {
    let mut host = Host::start();
    host.open_thread("t-fail", Some("fail"));
    host.prompt("t-fail");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-fail" }));

    let state = host.settle("t-fail", |s| s["state"] == "resurfaced");
    assert_eq!(state["resurfacedReason"], "failed");
    assert_eq!(state["latestRun"]["state"], "failed");
    assert_eq!(state["lastStopReason"], "max_tokens");
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["failed"]);
}

#[test]
fn folding_work_that_already_finished_resurfaces_it_instead_of_parking_it() {
    let mut host = Host::start();
    host.open_thread("t-late", None);
    host.prompt("t-late");
    host.settle("t-late", |s| s["latestRun"]["state"] == "succeeded");

    // The agent stopped before the user folded. Still Sleeping would be a
    // parking lot for finished work, so the card comes back immediately.
    let folded = host.ok(THREAD_FOLD, json!({ "threadId": "t-late" }));
    assert_eq!(folded["state"], "resurfaced");
    assert_eq!(folded["resurfacedReason"], "done");

    let inbox = host.ok(INBOX_LIST, json!({}));
    assert_eq!(kinds(&inbox), vec!["done"]);
    assert_eq!(inbox["events"][0]["title"], "Auth migration finished");
    assert_eq!(inbox["events"][0]["runId"], folded["latestRun"]["id"]);
    assert!(inbox["sleeping"].as_array().unwrap().is_empty());
}

#[test]
fn an_outstanding_permission_on_a_folded_thread_is_needs_you() {
    let mut host = Host::start();
    host.open_thread("t-perm", Some("permission"));
    host.ok(THREAD_FOLD, json!({ "threadId": "t-perm" }));
    host.prompt("t-perm");

    let state = host.settle("t-perm", |s| s["state"] == "resurfaced");
    assert_eq!(state["resurfacedReason"], "needs_you");
    // The run is paused, not finished: the turn resumes when we answer.
    assert_eq!(state["latestRun"]["state"], "needs_you");
    assert_eq!(state["process"]["acpState"], "requires_action");
    assert_eq!(state["process"]["pendingPermissions"], 1);
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["needs_you"]);

    // Answering puts the same run back to work and lets it finish.
    let ask = host
        .drain()
        .into_iter()
        .find(|n| n.method == PERMISSION_ASK)
        .expect("permission/ask");
    let request_id = ask.params.unwrap()["requestId"]
        .as_str()
        .unwrap()
        .to_string();
    let device_id = host.session.identity().local_device.device_id.clone();
    host.ok(
        PERMISSION_REPLY,
        json!({
            "requestId": request_id,
            "deviceId": device_id,
            "optionId": "allow_once"
        }),
    );
    let finished = host.settle("t-perm", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(finished["latestRun"]["seq"], 1, "same run, not a new one");
}

#[test]
fn wait_for_inbox_answers_a_read_itself_and_leaves_the_thread_asleep() {
    let mut host = Host::start();
    host.open_thread("t-read", Some("read-permission"));
    host.ok(
        THREAD_FOLD,
        json!({ "threadId": "t-read", "policy": "wait_for_inbox" }),
    );
    host.prompt("t-read");

    // The host answered the read, so the agent got to finish without the user.
    let state = host.settle("t-read", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(state["state"], "resurfaced");
    assert_eq!(
        state["resurfacedReason"], "done",
        "a read must not turn into a judgment call"
    );

    let inbox = host.ok(INBOX_LIST, json!({}));
    let events = inbox["events"].as_array().unwrap();
    let away = events
        .iter()
        .find(|e| e["kind"] == "judgment_call")
        .expect("the away log records what we allowed");
    assert_eq!(away["title"], "Allowed Read src/auth.ts");
    assert_eq!(away["payload"]["reviewable"], false);
    // Recorded, but never badged: it is a receipt, not something still owed.
    assert!(away["readAt"].is_string());
    assert_eq!(inbox["unread"], 1, "only the done card is unread");
}

#[test]
fn wait_for_inbox_still_asks_before_an_execute() {
    let mut host = Host::start();
    host.open_thread("t-exec", Some("permission"));
    host.ok(
        THREAD_FOLD,
        json!({ "threadId": "t-exec", "policy": "wait_for_inbox" }),
    );
    host.prompt("t-exec");

    let state = host.settle("t-exec", |s| s["state"] == "resurfaced");
    // Locked policy: folding never auto-allows execute, whatever the quietness
    // setting says. An unanswered execute is a judgment call for the human.
    assert_eq!(state["resurfacedReason"], "needs_you");
    assert_eq!(state["process"]["pendingPermissions"], 1);
    assert!(host.drain().iter().any(|n| n.method == PERMISSION_ASK));
}

#[test]
fn silence_while_running_resurfaces_stuck_and_keeps_the_process() {
    let mut host = Host::start();
    host.session.set_idle_timeout(Duration::from_millis(50));
    host.open_thread("t-stuck", Some("hang"));
    host.ok(THREAD_FOLD, json!({ "threadId": "t-stuck" }));
    host.prompt("t-stuck");

    let state = host.settle("t-stuck", |s| s["state"] == "resurfaced");
    assert_eq!(state["resurfacedReason"], "stuck");
    // Stuck is not failed: the run is still open and the adapter is still up,
    // so the user can wait, reopen, or cancel.
    assert_eq!(state["latestRun"]["state"], "running");
    assert_eq!(state["process"]["connected"], true);
    assert_eq!(host.session.live_adapter_count(), 1);
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["stuck"]);

    // And it fires once, not on every pump for as long as the agent is quiet.
    for _ in 0..5 {
        host.drain();
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        kinds(&host.ok(INBOX_LIST, json!({}))).len(),
        1,
        "the backstop must not re-notify while it stays quiet"
    );
}

#[test]
fn a_session_receipt_is_written_and_survives_the_process() {
    let mut host = Host::start();
    host.open_thread("t-receipt", None);
    host.prompt("t-receipt");
    let state = host.settle("t-receipt", |s| s["receipt"].is_object());

    let receipt = &state["receipt"];
    assert_eq!(receipt["acpSessionId"], "sess-fake-1");
    assert_eq!(receipt["harnessId"], "claude");
    assert_eq!(receipt["permissionMode"], "default");
    let fingerprint = receipt["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(fingerprint.len(), 16);

    // The bug to avoid is Buzz's in-memory session map: nothing about the
    // receipt may depend on this host still being alive.
    let cwd = host.dir.path().to_string_lossy().into_owned();
    host.restart();
    let stored = host
        .session
        .store()
        .unwrap()
        .get_session_receipt("t-receipt")
        .unwrap()
        .expect("the receipt outlives the host that wrote it");
    assert_eq!(stored.fingerprint, fingerprint);
    assert_eq!(stored.cwd, cwd);
    assert_eq!(stored.acp_session_id, "sess-fake-1");
}

#[test]
fn the_inbox_card_lands_before_the_notification_and_outlives_a_dropped_one() {
    let mut host = Host::start();
    host.open_thread("t-order", None);
    host.prompt("t-order");
    host.settle("t-order", |s| s["latestRun"]["state"] == "succeeded");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-order" }));

    // Persist-then-notify: by the time the notification exists, the card it
    // announces is already readable.
    let notifications = host.drain();
    let resurfaced = notifications
        .iter()
        .find(|n| n.method == INBOX_RESURFACE)
        .expect("inbox/resurface");
    assert_eq!(resurfaced.params.as_ref().unwrap()["reason"], "done");
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["done"]);

    // Now throw the notification away, the way a dead socket would, and quit.
    // A failed notification must never lose a result.
    host.restart();
    let inbox = host.ok(INBOX_LIST, json!({}));
    assert_eq!(kinds(&inbox), vec!["done"]);
    assert_eq!(inbox["events"][0]["threadState"], "resurfaced");
    assert_eq!(inbox["unread"], 1);
    assert_eq!(host.state("t-order")["resurfacedReason"], "done");
}

#[test]
fn a_resurface_that_cannot_be_persisted_notifies_nobody() {
    // An ephemeral host has no store, so the card has nowhere to land. The
    // rule is that the notification is downstream of the write: no write, no
    // notification, and no client told about a result that does not exist.
    let mut session = HostSession::ephemeral();
    session.handle_request(req(1, HOST_HELLO, None));
    let response = session.handle_request(req(2, THREAD_FOLD, Some(json!({ "threadId": "t-x" }))));
    assert_eq!(response.error.unwrap().code, -32007);
    assert!(session
        .take_outbound()
        .iter()
        .all(|n| n.method != INBOX_RESURFACE));
}

#[test]
fn archive_ends_the_work_and_delete_tombstones_the_thread() {
    let mut host = Host::start();
    host.open_thread("t-archive", Some("hang"));
    host.prompt("t-archive");
    host.settle("t-archive", |s| s["latestRun"]["state"] == "running");

    let archived = host.ok(THREAD_ARCHIVE, json!({ "threadId": "t-archive" }));
    assert_eq!(archived["state"], "archived");
    // Archiving is a decision to stop: the open run is closed and the adapter
    // goes with it, rather than being left to stream into nothing.
    assert_eq!(archived["latestRun"]["state"], "cancelled");
    assert_eq!(host.session.live_adapter_count(), 0);

    host.open_thread("t-gone", None);
    host.prompt("t-gone");
    host.settle("t-gone", |s| s["latestRun"]["state"] == "succeeded");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-gone" }));
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["done"]);

    let deleted = host.ok(THREAD_DELETE, json!({ "threadId": "t-gone" }));
    assert_eq!(deleted["state"], "deleted");
    assert!(deleted["deletedAt"].is_string());
    // The row survives so a late adapter event has somewhere to land, but the
    // Inbox stops showing it.
    assert!(kinds(&host.ok(INBOX_LIST, json!({}))).is_empty());
    assert!(host
        .session
        .store()
        .unwrap()
        .get_thread("t-gone")
        .unwrap()
        .is_some());
}

#[test]
fn reopening_a_resurfaced_thread_clears_its_badge() {
    let mut host = Host::start();
    host.open_thread("t-badge", None);
    host.prompt("t-badge");
    host.settle("t-badge", |s| s["latestRun"]["state"] == "succeeded");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-badge" }));
    assert_eq!(host.ok(INBOX_LIST, json!({}))["unread"], 1);

    let reopened = host.ok(THREAD_REOPEN, json!({ "threadId": "t-badge" }));
    assert_eq!(reopened["state"], "active");
    assert_eq!(reopened["unread"], 0);
    assert_eq!(host.ok(INBOX_LIST, json!({}))["unread"], 0);
    // The card stays in the Inbox history; only the badge clears.
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["done"]);
}

#[test]
fn archiving_a_resurfaced_thread_clears_its_badge() {
    let mut host = Host::start();
    host.open_thread("t-closed", None);
    host.prompt("t-closed");
    host.settle("t-closed", |s| s["latestRun"]["state"] == "succeeded");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-closed" }));
    assert_eq!(host.ok(INBOX_LIST, json!({}))["unread"], 1);

    // Archive is the other action offered on a resurfaced card, and it takes
    // the thread out of the Inbox for good. A badge that outlives it points at
    // a row no screen shows any more, and no click can ever clear it.
    let archived = host.ok(THREAD_ARCHIVE, json!({ "threadId": "t-closed" }));
    assert_eq!(archived["state"], "archived");
    assert_eq!(archived["unread"], 0);
    assert_eq!(host.ok(INBOX_LIST, json!({}))["unread"], 0);
}

#[test]
fn going_quiet_on_screen_still_resurfaces_stuck_once_it_is_folded() {
    let mut host = Host::start();
    host.session.set_idle_timeout(Duration::from_millis(50));
    host.open_thread("t-watched", Some("hang"));
    host.prompt("t-watched");

    // Watched, so nothing resurfaces: an `active` thread shows its own silence.
    for _ in 0..5 {
        host.drain();
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(host.state("t-watched")["state"], "active");
    assert!(kinds(&host.ok(INBOX_LIST, json!({}))).is_empty());

    // Now the user gets bored and folds it. The agent is still wedged, so it
    // will never say anything that could re-arm the backstop — the card has to
    // come from the fold, not from a signal that is never coming.
    let folded = host.ok(THREAD_FOLD, json!({ "threadId": "t-watched" }));
    assert_eq!(
        folded["state"], "folded",
        "a running run is not settled yet"
    );
    let state = host.settle("t-watched", |s| s["state"] == "resurfaced");
    assert_eq!(state["resurfacedReason"], "stuck");
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["stuck"]);
}

#[test]
fn a_thread_that_went_quiet_and_then_finished_reads_as_done() {
    let mut host = Host::start();
    host.session.set_idle_timeout(Duration::from_millis(50));
    host.open_thread("t-slow", Some("late-end"));
    host.ok(THREAD_FOLD, json!({ "threadId": "t-slow" }));
    host.prompt("t-slow");

    let stuck = host.settle("t-slow", |s| s["resurfacedReason"] == "stuck");
    assert_eq!(stuck["latestRun"]["state"], "running");

    // The agent was slow, not wedged. Once the turn actually ends the Inbox
    // has to agree with the ledger: finished work belongs in Done, not under
    // "has gone quiet" in Needs you.
    let done = host.settle("t-slow", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(done["resurfacedReason"], "done");
    assert_eq!(done["lastStopReason"], "end_turn");
    // And one row, not a stale stuck card beside its own answer.
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["done"]);
    assert_eq!(host.ok(INBOX_LIST, json!({}))["unread"], 1);
}

#[test]
fn reporting_idle_without_a_stop_reason_is_not_a_failure() {
    let mut host = Host::start();
    host.open_thread("t-idle", Some("v2-idle"));
    host.ok(THREAD_FOLD, json!({ "threadId": "t-idle" }));
    host.prompt("t-idle");

    // The adapter said "idle" before it said how the turn went. Idleness is a
    // process fact; the outcome rides on the prompt response, and reading the
    // first as the second reports a successful turn as a failure.
    let state = host.settle("t-idle", |s| s["state"] == "resurfaced");
    assert_eq!(state["resurfacedReason"], "done");
    assert_eq!(state["latestRun"]["state"], "succeeded");
    assert_eq!(state["lastStopReason"], "end_turn");
    assert_eq!(kinds(&host.ok(INBOX_LIST, json!({}))), vec!["done"]);
}

#[test]
fn a_second_prompt_cannot_retire_a_run_that_is_still_in_flight() {
    let mut host = Host::start();
    host.open_thread("t-overlap", Some("permission"));
    host.ok(THREAD_FOLD, json!({ "threadId": "t-overlap" }));
    host.prompt("t-overlap");
    let blocked = host.settle("t-overlap", |s| s["state"] == "resurfaced");
    assert_eq!(blocked["latestRun"]["state"], "needs_you");

    // The first turn is alive and will report. Opening a second run here would
    // hand it the first turn's stop reason and retire the run that did the
    // work — "a result must not be lost" forbids it.
    let refused = host.err(
        SESSION_PROMPT,
        json!({ "threadId": "t-overlap", "content": "and also" }),
    );
    assert_eq!(refused.code, RUN_IN_FLIGHT);
    assert_eq!(refused.data.as_ref().unwrap()["runState"], "needs_you");
    assert_eq!(
        host.state("t-overlap")["runs"].as_array().unwrap().len(),
        1,
        "the refused prompt must not have opened a run"
    );

    // Answering the outstanding ask lets the original run finish as itself.
    let ask = host
        .drain()
        .into_iter()
        .find(|n| n.method == PERMISSION_ASK)
        .expect("permission/ask");
    let request_id = ask.params.unwrap()["requestId"]
        .as_str()
        .unwrap()
        .to_string();
    let device_id = host.session.identity().local_device.device_id.clone();
    host.ok(
        PERMISSION_REPLY,
        json!({
            "requestId": request_id,
            "deviceId": device_id,
            "optionId": "allow_once"
        }),
    );
    let finished = host.settle("t-overlap", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(finished["latestRun"]["seq"], 1);
    assert_eq!(finished["resurfacedReason"], "done");
}
