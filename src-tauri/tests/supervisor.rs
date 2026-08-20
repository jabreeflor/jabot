//! Session supervisor (#21): boot reconciliation, resume, keep-alive, sleep.
//!
//! Every case here restarts a host against the same data directory, or kills
//! an adapter under one, because that is the only way to make a claim about
//! durability mean anything. Decision #4 says durability is resume rather than
//! a living PID — so the tests are written the way the failure happens: the
//! process goes away, and the question is what the next one knows.
//!
//! The agent is the real `fake-acp-agent` subprocess over real ACP stdio, and
//! what the host said to it is read back out of the adapter's stderr log. A
//! test that only watched the client side could not tell `session/resume` from
//! `session/new`, which is the distinction most of this file is about.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcRequest, JsonRpcResponse, RequestId, SessionFingerprint, HOST_HELLO,
    INBOX_LIST, SESSION_PROMPT, THREAD_ARCHIVE, THREAD_FOLD, THREAD_OPEN, THREAD_RESUME,
    THREAD_STATE, THREAD_TRANSCRIPT,
};
use serde_json::{json, Value};

/// The copy `state-machine.md` specifies for a permission we quit under. It is
/// hard-coded rather than imported so that changing the sentence in the host
/// fails this test instead of silently changing what the user reads.
const WAS_WAITING_ON_YOU: &str = "the agent was waiting on you; reopen to continue";

const SUPERVISOR_STATUS: &str = "supervisor/status";

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
        let mut host = Self {
            session: HostSession::load(dir.path()),
            dir,
            next_id: 2,
        };
        host.arm();
        host
    }

    /// Probe on every pump and never idle-evict, unless a case asks for it.
    /// Both are the settings #26 owns; a test that waited for the shipping
    /// values would wait a second and two minutes respectively.
    fn arm(&mut self) {
        self.session.set_probe_interval(Duration::ZERO);
        self.session.set_idle_evict_after(Duration::ZERO);
        self.session.handle_request(req(1, HOST_HELLO, None));
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

    /// Quit and relaunch against the same data dir. The `TempDir` outlives the
    /// session, so the store is reopened rather than recreated — and dropping
    /// the old session kills its adapters, which is what Cmd-Q does.
    fn restart(&mut self) {
        let path = self.dir.path().to_path_buf();
        drop(std::mem::replace(
            &mut self.session,
            HostSession::ephemeral(),
        ));
        self.session = HostSession::load(&path);
        self.arm();
    }

    /// What the host actually said to the adapter. Each spawn truncates the
    /// file, so this is the current process's side of the conversation.
    fn adapter_log(&self, thread_id: &str) -> String {
        let path = self
            .dir
            .path()
            .join("adapter-logs")
            .join(format!("{thread_id}.stderr.log"));
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn inbox(&mut self) -> Value {
        self.ok(INBOX_LIST, json!({}))
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

// ---- boot reconciliation ------------------------------------------------

#[test]
fn quitting_on_an_unanswered_permission_says_so_on_the_next_launch() {
    let mut host = Host::start();
    host.open_thread("t-perm", Some("permission"));
    host.ok(THREAD_FOLD, json!({ "threadId": "t-perm" }));
    host.prompt("t-perm");
    let blocked = host.settle("t-perm", |s| s["state"] == "resurfaced");
    assert_eq!(blocked["latestRun"]["state"], "needs_you");
    // Before the quit the card is about the request itself.
    assert_eq!(host.inbox()["events"][0]["summary"], "Run ls");

    host.restart();

    // The run cannot continue: the process that asked is gone and the RPC it
    // was blocked on died with it. `lost` is the ledger's word for that.
    let state = host.state("t-perm");
    assert_eq!(state["latestRun"]["state"], "lost");
    assert_eq!(state["latestRun"]["error"], WAS_WAITING_ON_YOU);
    assert_eq!(state["lastError"], WAS_WAITING_ON_YOU);
    // The process axis is honest about knowing nothing until a resume.
    assert_eq!(state["process"]["connected"], false);
    assert_eq!(state["process"]["acpState"], "unknown");

    // And the card the user sees now says why, rather than still describing a
    // live request (`state-machine.md`).
    let inbox = host.inbox();
    assert_eq!(kinds(&inbox), vec!["needs_you"]);
    assert_eq!(inbox["events"][0]["summary"], WAS_WAITING_ON_YOU);
    assert_eq!(inbox["unread"], 1);

    let notes = host.session.boot_notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].thread_id, "t-perm");
    assert_eq!(notes[0].was, "needs_you");
    assert_eq!(notes[0].now, "lost");
}

#[test]
fn a_run_that_was_still_going_comes_back_as_interrupted_not_failed() {
    let mut host = Host::start();
    host.open_thread("t-hang", Some("hang"));
    host.prompt("t-hang");
    host.settle("t-hang", |s| s["latestRun"]["state"] == "running");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-hang" }));
    assert_eq!(host.state("t-hang")["state"], "folded");

    host.restart();

    // Stuck, not failed: `keep-alive.md`'s restart row. Failed would invite a
    // retry of work we have no evidence went wrong; the ask is to reopen.
    let state = host.state("t-hang");
    assert_eq!(state["state"], "resurfaced");
    assert_eq!(state["resurfacedReason"], "stuck");
    assert_eq!(state["latestRun"]["state"], "lost");
    assert_eq!(state["latestRun"]["error"], "interrupted by restart");

    let inbox = host.inbox();
    assert_eq!(kinds(&inbox), vec!["stuck"]);
    assert_eq!(inbox["events"][0]["title"], "Auth migration has gone quiet");
    assert_eq!(inbox["events"][0]["summary"], "interrupted by restart");
}

#[test]
fn the_boot_pass_runs_once_however_often_the_host_is_relaunched() {
    let mut host = Host::start();
    host.open_thread("t-twice", Some("hang"));
    host.prompt("t-twice");
    host.settle("t-twice", |s| s["latestRun"]["state"] == "running");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-twice" }));

    host.restart();
    assert_eq!(host.session.boot_notes().len(), 1);
    assert_eq!(kinds(&host.inbox()).len(), 1);

    // The second launch finds a ledger with nothing open. A pass that closed
    // runs it had already closed would stack a duplicate card on every launch
    // for the rest of the thread's life.
    host.restart();
    assert!(host.session.boot_notes().is_empty());
    assert_eq!(kinds(&host.inbox()).len(), 1);
    assert_eq!(host.state("t-twice")["latestRun"]["state"], "lost");
}

#[test]
fn work_that_finished_before_the_quit_is_left_alone() {
    let mut host = Host::start();
    host.open_thread("t-done", None);
    host.prompt("t-done");
    host.settle("t-done", |s| s["latestRun"]["state"] == "succeeded");

    host.restart();

    // Nothing was in flight, so the boot pass has nothing to say — and must
    // not manufacture a card for a thread that simply finished.
    assert!(host.session.boot_notes().is_empty());
    assert_eq!(host.state("t-done")["latestRun"]["state"], "succeeded");
    assert!(kinds(&host.inbox()).is_empty());
}

// ---- resume -------------------------------------------------------------

#[test]
fn a_prompt_after_a_restart_resumes_rather_than_orphaning_the_conversation() {
    let mut host = Host::start();
    host.open_thread("t-resume", Some("resumable"));
    host.prompt("t-resume");
    let first = host.settle("t-resume", |s| s["latestRun"]["state"] == "succeeded");
    assert_eq!(first["acpSessionId"], "sess-fake-1");
    assert!(host.adapter_log("t-resume").contains("session_new="));

    host.restart();
    // With no adapter there is nothing to reattach to, but the receipt still
    // matches, so the thread advertises that it can be put back.
    let cold = host.state("t-resume");
    assert_eq!(cold["process"]["connected"], false);
    assert_eq!(cold["process"]["resumable"], true);
    assert!(cold["process"]["drift"].as_array().is_none());

    host.prompt("t-resume");
    host.settle("t-resume", |s| s["latestRun"]["seq"] == 2);

    // The claim: this is the same conversation, not a new one wearing its
    // title. Only the adapter's own side of the wire can prove it.
    let log = host.adapter_log("t-resume");
    assert!(log.contains("session_resume="), "{log}");
    assert!(
        !log.contains("session_new="),
        "a resumed thread must never be handed session/new: {log}"
    );
    assert!(log.contains("\"sessionId\":\"sess-fake-1\""), "{log}");
    assert_eq!(host.state("t-resume")["acpSessionId"], "sess-fake-1");
}

#[test]
fn thread_resume_reattaches_and_says_which_verb_it_used() {
    let mut host = Host::start();
    host.open_thread("t-explicit", Some("resumable"));
    host.prompt("t-explicit");
    host.settle("t-explicit", |s| s["latestRun"]["state"] == "succeeded");
    host.restart();

    let resumed = host.ok(THREAD_RESUME, json!({ "threadId": "t-explicit" }));
    assert_eq!(resumed["outcome"], "resumed");
    assert_eq!(resumed["resumed"], true);
    assert_eq!(resumed["acpSessionId"], "sess-fake-1");
    // A restored session has context and no turn: idle, not running.
    assert_eq!(resumed["state"]["process"]["connected"], true);
    assert_eq!(resumed["state"]["process"]["acpState"], "idle");

    // Asking again while it is attached is not a reason to tear the process
    // down and build it back: reopening a folded thread must not interrupt it.
    let again = host.ok(THREAD_RESUME, json!({ "threadId": "t-explicit" }));
    assert_eq!(again["outcome"], "live");
    assert_eq!(again["resumed"], true);
}

#[test]
fn a_changed_permission_mode_is_drift_and_drift_refuses_the_resume() {
    let mut host = Host::start();
    host.open_thread("t-drift", Some("resumable"));
    host.prompt("t-drift");
    host.settle("t-drift", |s| s["latestRun"]["state"] == "succeeded");

    // Wait for Inbox is a different permission mode, and the receipt #15 wrote
    // was stamped under the old one.
    host.ok(
        THREAD_FOLD,
        json!({ "threadId": "t-drift", "policy": "wait_for_inbox" }),
    );
    host.restart();

    let state = host.state("t-drift");
    assert_eq!(state["process"]["resumable"], false);
    assert_eq!(state["process"]["drift"], json!(["permissionMode"]));

    let refused = host.ok(THREAD_RESUME, json!({ "threadId": "t-drift" }));
    assert_eq!(refused["outcome"], "drifted");
    assert_eq!(refused["resumed"], false);
    assert_eq!(refused["drift"], json!(["permissionMode"]));
    assert!(refused["detail"]
        .as_str()
        .unwrap()
        .contains("permissionMode"));
    // Refused means refused: nothing was spawned to hold a session it must
    // not have been given.
    assert_eq!(host.session.live_adapter_count(), 0);
}

#[test]
fn a_resume_into_a_folder_that_is_gone_is_refused_and_says_so() {
    let mut host = Host::start();
    let cwd = host.dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).unwrap();
    host.ok(
        THREAD_OPEN,
        json!({
            "threadId": "t-gone",
            "title": "Auth migration",
            "cwd": cwd.to_string_lossy(),
            "harnessId": "claude",
            "runtime": { "command": fake_agent(), "args": ["resumable"] }
        }),
    );
    host.prompt("t-gone");
    host.settle("t-gone", |s| s["latestRun"]["state"] == "succeeded");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-gone" }));
    host.restart();

    std::fs::remove_dir_all(&cwd).unwrap();

    let refused = host.ok(THREAD_RESUME, json!({ "threadId": "t-gone" }));
    assert_eq!(refused["outcome"], "cwd_missing");
    assert_eq!(refused["resumed"], false);
    assert!(refused["detail"].as_str().unwrap().contains("checkout"));
    // `keep-alive.md`: refuse and resurface failed ("folder missing"). Silently
    // resuming somewhere else is how an agent is told to edit files that are
    // not there.
    assert_eq!(refused["state"]["resurfacedReason"], "failed");
    assert_eq!(kinds(&host.inbox())[0], "failed");
    assert_eq!(host.session.live_adapter_count(), 0);
}

#[test]
fn an_adapter_that_can_only_load_does_not_replay_a_transcript_we_already_have() {
    let mut host = Host::start();
    host.open_thread("t-load", Some("loadable"));
    host.prompt("t-load");
    host.settle("t-load", |s| s["latestRun"]["state"] == "succeeded");
    host.restart();

    let resumed = host.ok(THREAD_RESUME, json!({ "threadId": "t-load" }));
    assert_eq!(resumed["outcome"], "loaded");
    let log = host.adapter_log("t-load");
    assert!(log.contains("session_load="), "{log}");

    // The agent replayed two messages at us and we kept our own transcript.
    // Persisting the replay would show the user every message twice.
    let transcript = host.ok(THREAD_TRANSCRIPT, json!({ "threadId": "t-load" }));
    let text = transcript.to_string();
    assert!(!text.contains("replayed one"), "{text}");
    assert!(text.contains("hello from fake-acp"), "{text}");
}

#[test]
fn a_thread_with_no_transcript_of_its_own_does_want_the_replay() {
    // The case `store.md` step 3 describes: a session on disk whose overlay
    // transcript is missing. Built through the store because the only way to
    // get a session id is to have run a turn, and running one writes the very
    // rows this case is about not having.
    let mut host = Host::start();
    let cwd = host.dir.path().to_string_lossy().into_owned();
    host.ok(
        THREAD_OPEN,
        json!({
            "threadId": "t-empty",
            "title": "Recovered",
            "cwd": cwd,
            "harnessId": "claude",
            "runtime": { "command": fake_agent(), "args": ["loadable"] }
        }),
    );
    let store = host.session.store().expect("store");
    store
        .set_thread_acp_session("t-empty", "sess-fake-1")
        .unwrap();
    let fingerprint = SessionFingerprint::new("claude", None, cwd.clone(), Vec::new(), "default");
    store
        .upsert_session_receipt(
            "t-empty",
            "sess-fake-1",
            None,
            "claude",
            None,
            &cwd,
            &fingerprint.tools_json(),
            "default",
            &fingerprint.digest(),
        )
        .unwrap();

    let loaded = host.ok(THREAD_RESUME, json!({ "threadId": "t-empty" }));
    assert_eq!(loaded["outcome"], "loaded");
    let transcript = host.ok(THREAD_TRANSCRIPT, json!({ "threadId": "t-empty" }));
    let text = transcript.to_string();
    assert!(text.contains("replayed one"), "{text}");
    assert!(text.contains("replayed two"), "{text}");
}

#[test]
fn an_adapter_that_can_do_neither_says_so_instead_of_pretending() {
    let mut host = Host::start();
    // The default fake agent advertises neither resume nor load.
    host.open_thread("t-plain", None);
    host.prompt("t-plain");
    host.settle("t-plain", |s| s["latestRun"]["state"] == "succeeded");
    host.restart();

    let answer = host.ok(THREAD_RESUME, json!({ "threadId": "t-plain" }));
    assert_eq!(answer["outcome"], "unsupported");
    assert_eq!(answer["resumed"], false);
    // A process holding no session looks connected and can answer nothing, so
    // it is not kept.
    assert_eq!(answer["state"]["process"]["connected"], false);
    assert_eq!(host.session.live_adapter_count(), 0);

    // The user is not blocked by it: prompting starts an honest new session.
    host.prompt("t-plain");
    host.settle("t-plain", |s| s["latestRun"]["seq"] == 2);
    assert!(host.adapter_log("t-plain").contains("session_new="));
}

#[test]
fn a_thread_that_never_ran_has_nothing_to_resume() {
    let mut host = Host::start();
    host.open_thread("t-fresh", None);
    let answer = host.ok(THREAD_RESUME, json!({ "threadId": "t-fresh" }));
    assert_eq!(answer["outcome"], "no_session");
    assert_eq!(answer["resumed"], false);
    assert_eq!(answer["state"]["process"]["resumable"], false);
}

// ---- keep-alive ---------------------------------------------------------

#[test]
fn an_adapter_that_dies_without_closing_its_stdout_is_still_dead() {
    let mut host = Host::start();
    host.open_thread("t-orphan", Some("orphan-stdout"));
    host.prompt("t-orphan");

    // The adapter exits, but a grandchild it forked holds the same stdout, so
    // the read loop never sees EOF. Only reaping the pid can tell us. Without
    // the keep-alive probe this thread reports a live session forever.
    let state = host.settle("t-orphan", |s| s["process"]["connected"] == false);
    assert_eq!(state["latestRun"]["state"], "failed");
    assert_eq!(state["latestRun"]["error"], "the adapter process exited");
    assert_eq!(host.session.live_adapter_count(), 0);
}

#[test]
fn an_idle_adapter_nobody_is_watching_is_closed_and_can_come_back() {
    let mut host = Host::start();
    host.open_thread("t-evict", Some("resumable"));
    host.prompt("t-evict");
    host.settle("t-evict", |s| s["latestRun"]["state"] == "succeeded");
    // Folding finished work resurfaces it, so this thread is no longer one
    // the user is looking at — and its run is over.
    host.ok(THREAD_FOLD, json!({ "threadId": "t-evict" }));
    assert_eq!(host.session.live_adapter_count(), 1);

    host.session.set_idle_evict_after(Duration::from_millis(1));
    thread::sleep(Duration::from_millis(5));
    let evicted = host.settle("t-evict", |s| s["process"]["connected"] == false);

    // Closed, not merely killed: Buzz never sent `session/close` and pinned a
    // process tree per session for the life of the app.
    assert!(
        host.adapter_log("t-evict").contains("session_close="),
        "{}",
        host.adapter_log("t-evict")
    );
    assert_eq!(host.session.live_adapter_count(), 0);
    // Eviction is only safe because the conversation can come back.
    assert_eq!(evicted["process"]["resumable"], true);
    assert_eq!(evicted["latestRun"]["state"], "succeeded");
}

#[test]
fn folded_work_that_is_still_running_is_never_evicted() {
    let mut host = Host::start();
    host.open_thread("t-busy", Some("hang"));
    host.prompt("t-busy");
    host.settle("t-busy", |s| s["latestRun"]["state"] == "running");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-busy" }));

    // Disappeared and still working is the product's whole premise. An evict
    // here would kill the turn the user folded specifically to keep.
    host.session.set_idle_evict_after(Duration::from_millis(1));
    thread::sleep(Duration::from_millis(5));
    for _ in 0..5 {
        host.session.pump_acp();
    }
    assert_eq!(host.session.live_adapter_count(), 1);
    assert_eq!(host.state("t-busy")["process"]["connected"], true);
}

#[test]
fn a_sleep_while_the_agent_was_working_resurfaces_it_stuck_and_keeps_the_process() {
    let mut host = Host::start();
    host.open_thread("t-sleep", Some("hang"));
    host.prompt("t-sleep");
    host.settle("t-sleep", |s| s["latestRun"]["state"] == "running");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-sleep" }));

    // The lid was shut for an hour. The monotonic clock did not count it, so
    // the idle backstop never fires — this is the only signal there is.
    host.session.wake_from_sleep(Duration::from_secs(3600));

    let state = host.state("t-sleep");
    assert_eq!(state["state"], "resurfaced");
    assert_eq!(state["resurfacedReason"], "stuck");
    let inbox = host.inbox();
    assert_eq!(kinds(&inbox), vec!["stuck"]);
    assert!(inbox["events"][0]["summary"]
        .as_str()
        .unwrap()
        .contains("slept"));
    // We cannot prove the tool finished, so the run keeps running and the
    // process stays: the user can wait, reopen, or cancel.
    assert_eq!(state["latestRun"]["state"], "running");
    assert_eq!(host.session.live_adapter_count(), 1);

    // And it is said once. A second wake with nothing new to report must not
    // stack another card.
    host.session.wake_from_sleep(Duration::from_secs(3600));
    assert_eq!(kinds(&host.inbox()).len(), 1);
}

#[test]
fn archive_closes_the_session_before_it_kills_the_process() {
    let mut host = Host::start();
    host.open_thread("t-archive", Some("resumable"));
    host.prompt("t-archive");
    host.settle("t-archive", |s| s["latestRun"]["state"] == "succeeded");

    host.ok(THREAD_ARCHIVE, json!({ "threadId": "t-archive" }));

    // D-006 left archive killing the process group because the ACP layer had
    // no close. It has one now, and close comes first: killing the group frees
    // our resources, `session/close` frees the agent's.
    let log = host.adapter_log("t-archive");
    assert!(log.contains("session_close="), "{log}");
    assert_eq!(host.session.live_adapter_count(), 0);
}

#[test]
fn supervisor_status_reports_what_is_live_and_what_boot_found() {
    let mut host = Host::start();
    host.open_thread("t-status", Some("hang"));
    host.prompt("t-status");
    host.settle("t-status", |s| s["latestRun"]["state"] == "running");

    let status = host.ok(SUPERVISOR_STATUS, json!({}));
    let adapters = status["liveAdapters"].as_array().unwrap();
    assert_eq!(adapters.len(), 1);
    assert_eq!(adapters[0]["threadId"], "t-status");
    assert_eq!(adapters[0]["acpSessionId"], "sess-fake-1");
    assert!(adapters[0]["pid"].as_u64().unwrap() > 0);
    // Thread-scoped harnesses key their profile on the thread, which is what
    // says out loud that these two could never have shared a process (#13).
    assert_eq!(adapters[0]["profileKey"], "claude:t-status");
    assert!(status["boot"].as_array().unwrap().is_empty());

    host.ok(THREAD_FOLD, json!({ "threadId": "t-status" }));
    host.restart();
    let after = host.ok(SUPERVISOR_STATUS, json!({}));
    assert!(after["liveAdapters"].as_array().unwrap().is_empty());
    assert_eq!(after["boot"][0]["threadId"], "t-status");
    assert_eq!(after["boot"][0]["resurfacedAs"], "stuck");
    assert_eq!(after["sleepsObserved"], 0);
}

#[test]
fn a_prompt_queued_behind_a_v2_turn_is_still_delivered() {
    let mut host = Host::start();
    host.open_thread("t-stranded", Some("v2-cancel"));
    host.prompt("t-stranded");
    host.settle("t-stranded", |s| s["latestRun"]["state"] == "running");

    // Interrupt: cancel the turn, then send this instead. The agent ends the
    // turn the ACP v2 way — an idle `state_update` with a stop reason, and no
    // `session/prompt` response at all — so #14's drain, which hangs off that
    // response, never runs. Without the supervisor's reconciliation the user's
    // message would sit in the queue for the life of the app.
    let queued = host.ok(
        SESSION_PROMPT,
        json!({
            "threadId": "t-stranded",
            "content": "no, this instead",
            "mode": "interrupt"
        }),
    );
    assert_eq!(queued["queued"], true);

    let state = host.settle("t-stranded", |s| s["latestRun"]["seq"] == 2);
    assert_eq!(state["latestRun"]["state"], "running");
    let transcript = host.ok(THREAD_TRANSCRIPT, json!({ "threadId": "t-stranded" }));
    assert!(
        transcript.to_string().contains("no, this instead"),
        "the follow-up never reached the agent: {transcript}"
    );
    assert!(transcript["queued"].as_array().unwrap().is_empty());
}

#[test]
fn an_adapter_that_cannot_hand_its_session_back_is_never_evicted() {
    let mut host = Host::start();
    // The default fake agent advertises neither resume nor load.
    host.open_thread("t-keep", None);
    host.prompt("t-keep");
    host.settle("t-keep", |s| s["latestRun"]["state"] == "succeeded");
    host.ok(THREAD_FOLD, json!({ "threadId": "t-keep" }));

    host.session.set_idle_evict_after(Duration::from_millis(1));
    thread::sleep(Duration::from_millis(5));
    for _ in 0..5 {
        host.session.pump_acp();
    }

    // Evicting here would trade a few megabytes for the agent's entire
    // context, and the next prompt would quietly continue in a new session.
    assert_eq!(host.session.live_adapter_count(), 1);
    assert_eq!(host.state("t-keep")["process"]["connected"], true);
}
