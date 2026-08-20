//! The transcript overlay (#14) driven through the host API.
//!
//! `src/host/transcript/` unit-tests the read and the queue against a store.
//! This file puts a real `fake-acp-agent` subprocess behind them, because the
//! claims that matter are about a live turn: that what was streamed is what is
//! on disk, that a relaunched host can replay a conversation it did not
//! witness, and that a prompt held for a busy thread really does go out when
//! the turn ends.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcRequest, JsonRpcResponse, RequestId, HOST_HELLO, SESSION_PROMPT,
    THREAD_OPEN, THREAD_STATE, THREAD_TRANSCRIPT,
};
use serde_json::{json, Value};

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

    fn open_thread(&mut self, thread_id: &str, mode: Option<&str>) {
        let args: Vec<String> = mode.into_iter().map(str::to_string).collect();
        self.ok(
            THREAD_OPEN,
            json!({
                "threadId": thread_id,
                "title": "Auth migration",
                "cwd": self.dir.path().to_string_lossy(),
                "harnessId": "claude",
                "runtime": { "command": fake_agent(), "args": args }
            }),
        );
    }

    fn prompt(&mut self, thread_id: &str, text: &str, mode: Option<&str>) -> JsonRpcResponse {
        let mut params = json!({ "threadId": thread_id, "content": text });
        if let Some(mode) = mode {
            params["mode"] = json!(mode);
        }
        self.call(SESSION_PROMPT, params)
    }

    fn transcript(&mut self, thread_id: &str) -> Value {
        self.ok(THREAD_TRANSCRIPT, json!({ "threadId": thread_id }))
    }

    /// Pump the adapter until `predicate` holds on the replayed transcript.
    fn settle(&mut self, thread_id: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.session.pump_acp();
            let transcript = self.transcript(thread_id);
            if predicate(&transcript) {
                return transcript;
            }
            if Instant::now() > deadline {
                panic!("{thread_id} never settled; last transcript: {transcript}");
            }
            thread::sleep(Duration::from_millis(15));
        }
    }

    /// Quit and relaunch on the same data directory.
    fn restart(&mut self) {
        let path = self.dir.path().to_path_buf();
        drop(std::mem::replace(
            &mut self.session,
            HostSession::ephemeral(),
        ));
        self.session = HostSession::load(&path);
        self.session.handle_request(req(1, HOST_HELLO, None));
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

fn kinds(transcript: &Value) -> Vec<String> {
    transcript["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| {
            event["payload"]["sessionUpdate"]
                .as_str()
                .unwrap_or("?")
                .into()
        })
        .collect()
}

/// The headline claim: a thread reopened after a quit replays from *our*
/// store. Nothing here reads `~/.claude`, and the relaunched host never saw
/// the turn it is replaying.
#[test]
fn a_relaunched_host_replays_the_conversation_from_sqlite() {
    let mut host = Host::start();
    host.open_thread("t-hydrate", Some("tools"));
    host.prompt("t-hydrate", "fix the auth guard", None);
    host.settle("t-hydrate", |t| {
        kinds(t).iter().any(|kind| kind == "state_update")
    });

    host.restart();

    let replay = host.transcript("t-hydrate");
    let kinds = kinds(&replay);
    // The user's half of the conversation is in there too: a transcript that
    // replays only the agent is a transcript of half a conversation.
    assert_eq!(
        kinds.first().map(String::as_str),
        Some("user_message_chunk")
    );
    assert_eq!(
        replay["events"][0]["payload"]["content"]["text"],
        "fix the auth guard"
    );
    for expected in [
        "agent_message_chunk",
        "plan",
        "tool_call",
        "tool_call_update",
    ] {
        assert!(
            kinds.iter().any(|kind| kind == expected),
            "{expected} missing from the replay: {kinds:?}"
        );
    }
    assert_eq!(
        replay["headSeq"],
        replay["events"].as_array().unwrap().len()
    );
    // A restart holds no queue: it was RAM, and it says so rather than
    // pretending a prompt is still on its way.
    assert_eq!(replay["queued"].as_array().unwrap().len(), 0);
}

/// Every event that was streamed to the client is an event on disk, in the
/// same order, with the same `seq`. That pairing is what lets a client hydrate
/// and stream at once without rendering anything twice.
#[test]
fn streamed_events_and_stored_rows_are_the_same_events() {
    let mut host = Host::start();
    host.open_thread("t-pair", Some("tools"));
    host.prompt("t-pair", "hi", None);
    let replay = host.settle("t-pair", |t| {
        kinds(t).iter().any(|kind| kind == "state_update")
    });

    let streamed: Vec<(i64, String)> = host
        .session
        .take_outbound()
        .iter()
        .filter(|n| n.method == "session/update")
        .map(|n| {
            let params = n.params.clone().unwrap();
            (
                params["transcriptSeq"].as_i64().expect("transcriptSeq"),
                params["acp"]["sessionUpdate"]
                    .as_str()
                    .unwrap_or("?")
                    .into(),
            )
        })
        .collect();
    let stored: Vec<(i64, String)> = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        // Every row but a permission request was also streamed. The turn-end
        // row is labelled `session/prompt` because that is the ACP message it
        // was synthesized from, and it still went out as a `session/update`.
        .filter(|e| e["method"] != "session/request_permission")
        .map(|e| {
            (
                e["seq"].as_i64().unwrap(),
                e["payload"]["sessionUpdate"].as_str().unwrap_or("?").into(),
            )
        })
        .collect();

    assert!(!streamed.is_empty());
    assert_eq!(streamed, stored);
}

/// Steer vs redispatch, end to end: the second prompt waits for the turn in
/// flight and is sent the moment it ends — in the order the user typed it.
#[test]
fn a_queued_prompt_is_sent_when_the_turn_in_flight_ends() {
    let mut host = Host::start();
    // `late-end` holds the turn open for long enough that the follow-up really
    // does arrive mid-flight, rather than racing a turn that already finished.
    host.open_thread("t-queue", Some("late-end"));
    host.prompt("t-queue", "first", None);

    let queued = host.prompt("t-queue", "second", Some("queue"));
    let result = queued.result.expect("queued rather than refused");
    assert_eq!(result["queued"], true);
    assert_eq!(result["accepted"], false);
    assert_eq!(result["queuePosition"], 1);

    // The queue is visible while it waits, so a client can draw it.
    let waiting = host.transcript("t-queue");
    assert_eq!(waiting["queued"][0]["content"], "second");

    let settled = host.settle("t-queue", |t| {
        t["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["payload"]["sessionUpdate"] == "user_message_chunk")
            .count()
            == 2
    });
    let said: Vec<&str> = settled["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["payload"]["sessionUpdate"] == "user_message_chunk")
        .map(|e| e["payload"]["content"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(said, vec!["first", "second"]);
    assert!(settled["queued"].as_array().unwrap().is_empty());

    // Two turns, two runs — never one run collecting both outcomes (#15).
    let state = host.ok(THREAD_STATE, json!({ "threadId": "t-queue" }));
    assert_eq!(state["runs"].as_array().unwrap().len(), 2);
}

/// A prompt that is only queued must not be written down as one the agent was
/// given. The transcript is the record of what was *said* to the harness.
#[test]
fn a_queued_prompt_is_not_in_the_transcript_until_it_is_sent() {
    let mut host = Host::start();
    host.open_thread("t-unsent", Some("hang"));
    host.prompt("t-unsent", "first", None);
    host.prompt("t-unsent", "second", Some("queue"));

    let transcript = host.transcript("t-unsent");
    let said: Vec<&str> = transcript["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["payload"]["sessionUpdate"] == "user_message_chunk")
        .map(|e| e["payload"]["content"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(said, vec!["first"]);
    assert_eq!(transcript["queued"][0]["content"], "second");
}

/// The other half of steer-vs-redispatch: interrupt cancels the turn in flight
/// and the follow-up goes out on the back of the cancellation, not before it.
/// Sending it immediately would be the overlapping run #15 forbids.
#[test]
fn interrupt_cancels_the_turn_and_then_sends_the_follow_up() {
    let mut host = Host::start();
    host.open_thread("t-interrupt", Some("cancellable"));
    host.prompt("t-interrupt", "the wrong thing", None);
    host.session.pump_acp();

    let interrupted = host.prompt("t-interrupt", "no, this instead", Some("interrupt"));
    let result = interrupted.result.expect("queued behind the cancel");
    assert_eq!(result["queued"], true);

    let settled = host.settle("t-interrupt", |t| {
        t["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["payload"]["sessionUpdate"] == "user_message_chunk")
            .count()
            == 2
    });
    let said: Vec<&str> = settled["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["payload"]["sessionUpdate"] == "user_message_chunk")
        .map(|e| e["payload"]["content"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(said, vec!["the wrong thing", "no, this instead"]);

    // The interrupted turn is a cancelled run, not a lost one: the host asked
    // for it to stop and the agent said it had.
    let state = host.ok(THREAD_STATE, json!({ "threadId": "t-interrupt" }));
    let runs = state["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let first = runs
        .iter()
        .find(|run| run["seq"] == 1)
        .expect("the interrupted run");
    assert_eq!(first["state"], "cancelled");
}
