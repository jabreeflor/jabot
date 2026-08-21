//! Schedules (#25) driven through the host API, with a real ACP subprocess.
//!
//! `host/schedule/` unit-tests the cron and the catch-up ruling in isolation.
//! What cannot be checked there is the part that crosses three subsystems at
//! once: a tick turns into a `session/prompt` on a crew member's standing
//! thread (#24), which opens a run on #15's ledger with `kind = 'schedule'`,
//! whose result becomes an `inbox_event` (#5). Every case here goes in through
//! `schedule/*` and comes back out as a store row, because those are the two
//! ends a user actually experiences.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcRequest, JsonRpcResponse, RequestId, CREW_UPDATE, HOST_HELLO,
    SCHEDULE_CREATE, SCHEDULE_LIST, SCHEDULE_REMOVE, SCHEDULE_RUN, SCHEDULE_UPDATE,
};
use serde_json::{json, Value};

const INVALID_PARAMS: i64 = -32602;

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

/// The scriptable ACP agent. Same lookup `tests/lifecycle.rs` uses.
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

struct Host {
    session: HostSession,
    dir: tempfile::TempDir,
    next_id: i64,
}

impl Host {
    /// A host whose crew can actually spawn: the fake agent is registered as a
    /// tier-3 harness *before* the store opens, because `bots.harness_id` is a
    /// foreign key and the catalog is synced at load.
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("custom_harnesses")).unwrap();
        std::fs::write(
            dir.path().join("custom_harnesses/fake-acp.json"),
            json!({ "id": "fake-acp", "label": "Fake ACP", "command": fake_agent(), "args": [] })
                .to_string(),
        )
        .unwrap();
        let mut session = HostSession::load(dir.path());
        // Every pump ticks the cron, so a test does not wait out the poll.
        session.set_schedule_tick(Duration::ZERO);
        session.handle_request(req(1, HOST_HELLO, None));
        let mut host = Self {
            session,
            dir,
            next_id: 2,
        };
        host.ok(
            CREW_UPDATE,
            json!({ "botId": "writer", "harnessId": "fake-acp" }),
        );
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

    fn err(&mut self, method: &str, params: Value) -> jabot_lib::host::JsonRpcError {
        self.call(method, params).error.expect("expected an error")
    }

    fn create(&mut self, name: &str, cron: &str) -> Value {
        self.ok(
            SCHEDULE_CREATE,
            json!({
                "botId": "writer",
                "name": name,
                "cron": cron,
                "prompt": "Summarise overnight mail.",
            }),
        )
    }

    fn schedules(&mut self) -> Vec<Value> {
        self.ok(SCHEDULE_LIST, json!({}))["schedules"]
            .as_array()
            .expect("schedules")
            .clone()
    }

    fn schedule(&mut self, schedule_id: &str) -> Value {
        self.schedules()
            .into_iter()
            .find(|row| row["scheduleId"] == schedule_id)
            .expect("the schedule is still listed")
    }

    /// Backdate a schedule's claim on the clock — the only way to stand in for
    /// "the Mac was shut" without shutting one.
    fn owe_since(&mut self, schedule_id: &str, ago: chrono::Duration) {
        let due = (chrono::Utc::now() - ago).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        self.session
            .store()
            .expect("store")
            .set_schedule_due(schedule_id, Some(&due))
            .expect("backdated");
    }

    fn fires(&mut self, schedule_id: &str) -> Vec<Value> {
        self.schedule(schedule_id)["recentFires"]
            .as_array()
            .expect("recentFires")
            .clone()
    }

    /// Pump until `predicate` holds on the schedule, or give up.
    fn settle(&mut self, schedule_id: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.session.pump_acp();
            let row = self.schedule(schedule_id);
            if predicate(&row) {
                return row;
            }
            if Instant::now() > deadline {
                panic!("schedule {schedule_id} never settled; last: {row}");
            }
            thread::sleep(Duration::from_millis(15));
        }
    }

    fn runs(&self, thread_id: &str) -> Vec<jabot_lib::host::RunRow> {
        self.session
            .store()
            .expect("store")
            .list_runs(thread_id)
            .expect("runs")
    }

    fn cards(&self, thread_id: &str) -> Vec<jabot_lib::host::InboxEventRow> {
        self.session
            .store()
            .expect("store")
            .list_inbox_events(50, true)
            .expect("inbox")
            .into_iter()
            .filter(|card| card.thread_id == thread_id)
            .collect()
    }
}

fn delivered(row: &Value) -> bool {
    row["lastFire"]["state"] == "delivered"
}

#[test]
fn a_schedule_is_created_armed_and_listed() {
    let mut host = Host::start();
    let created = host.create("Morning triage", "0 9 * * *");
    assert_eq!(created["botId"], "writer");
    assert_eq!(created["botName"], "Writer");
    assert_eq!(created["catchUp"], "once");
    assert_eq!(created["enabled"], true);
    // Armed from now, never from the past: a schedule made at 10am does not
    // owe this morning's 9am.
    let next = created["nextRunAt"].as_str().expect("armed");
    assert!(
        next > chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            .as_str(),
        "next run {next} should be ahead of now"
    );
    assert!(created["lastFire"].is_null());

    let listed = host.schedules();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["scheduleId"], created["scheduleId"]);
}

#[test]
fn a_cron_that_would_never_run_is_refused_before_the_row_exists() {
    let mut host = Host::start();
    let err = host.err(
        SCHEDULE_CREATE,
        json!({ "botId": "writer", "name": "Nightly", "cron": "0 99 * * *", "prompt": "go" }),
    );
    assert_eq!(err.code, INVALID_PARAMS);
    assert!(err.message.contains("hour"), "{}", err.message);
    // Refused means refused: nothing was written.
    assert!(host.schedules().is_empty());

    // …and so is a bot that does not exist, for the same reason: a job with
    // nobody to run it would fire forever into an error.
    let err = host.err(
        SCHEDULE_CREATE,
        json!({ "botId": "nobody", "name": "Nightly", "cron": "0 9 * * *", "prompt": "go" }),
    );
    assert_eq!(err.code, INVALID_PARAMS);
}

/// The spine of the issue: a fire is a run of kind `schedule`, and its result
/// is a card in the Inbox.
#[test]
fn a_fire_runs_as_the_bot_and_delivers_its_result_to_the_inbox() {
    let mut host = Host::start();
    let created = host.create("Morning triage", "0 9 * * *");
    let schedule_id = created["scheduleId"].as_str().unwrap().to_string();

    let fire = host.ok(SCHEDULE_RUN, json!({ "scheduleId": schedule_id }))["fire"].clone();
    let thread_id = fire["threadId"].as_str().expect("a thread").to_string();
    // Decision #6: the job runs on the bot's standing thread (#24), not on a
    // thread the scheduler invented.
    assert_eq!(thread_id, "bot-writer");

    let settled = host.settle(&schedule_id, delivered);

    // #15's ledger, with the kind the schema has accepted since 0001 and
    // nothing had written until now.
    let runs = host.runs(&thread_id);
    let scheduled: Vec<_> = runs.iter().filter(|run| run.kind == "schedule").collect();
    assert_eq!(scheduled.len(), 1, "one fire, one run: {runs:?}");
    let trigger = scheduled[0]
        .trigger_json
        .as_deref()
        .expect("a schedule run says which schedule");
    assert!(trigger.contains(&schedule_id), "{trigger}");
    assert_eq!(scheduled[0].state, "succeeded");

    // …and decision #5's projection: the Inbox card, with the schedule's name
    // on it rather than the thread's.
    let cards = host.cards(&thread_id);
    let card = cards
        .iter()
        .find(|card| card.run_id.as_deref() == Some(scheduled[0].id.as_str()))
        .expect("a card for the run");
    assert_eq!(card.kind, "done");
    assert!(card.title.contains("Morning triage"), "{}", card.title);
    let payload = card.payload_json.as_deref().expect("payload");
    assert!(payload.contains("\"source\":\"schedule\""), "{payload}");
    assert!(payload.contains(&schedule_id), "{payload}");

    // Run now is its own occurrence: it must not have consumed 9am.
    assert_eq!(settled["nextRunAt"], created["nextRunAt"]);
}

/// The failure the issue names: a week of missed dailies must not become a
/// week of runs on launch.
#[test]
fn a_backlog_collapses_to_one_run_and_says_how_many_it_dropped() {
    let mut host = Host::start();
    // Hourly, so the most recent missed occurrence is always fresh enough to
    // be worth running — a daily's would be up to a day stale, which is a
    // different (and separately tested) branch.
    let created = host.create("Hourly sweep", "0 * * * *");
    let schedule_id = created["scheduleId"].as_str().unwrap().to_string();
    host.owe_since(&schedule_id, chrono::Duration::days(6));

    let settled = host.settle(&schedule_id, delivered);
    let fires = host.fires(&schedule_id);
    assert_eq!(
        fires.len(),
        1,
        "six days of hourlies is one fire: {fires:?}"
    );
    assert_eq!(fires[0]["caughtUp"], true);
    let skipped = fires[0]["skippedCount"].as_i64().expect("a count");
    assert!(
        skipped > 100,
        "six days of hourlies is {skipped} occurrences"
    );

    let runs = host.runs("bot-writer");
    assert_eq!(
        runs.iter().filter(|run| run.kind == "schedule").count(),
        1,
        "one run, not one per missed hour: {runs:?}"
    );
    // The clock is ahead of now again, so the next tick does nothing.
    let next = settled["nextRunAt"].as_str().expect("re-armed");
    assert!(
        next > chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            .as_str()
    );
}

/// The same backlog, for a user who said a late run is worse than none.
#[test]
fn skip_runs_nothing_it_missed_but_still_records_the_outage() {
    let mut host = Host::start();
    let created = host.create("Hourly sweep", "0 * * * *");
    let schedule_id = created["scheduleId"].as_str().unwrap().to_string();
    host.ok(
        SCHEDULE_UPDATE,
        json!({ "scheduleId": schedule_id, "catchUp": "skip" }),
    );
    host.owe_since(&schedule_id, chrono::Duration::days(6));

    host.settle(&schedule_id, |row| !row["lastFire"].is_null());
    let fires = host.fires(&schedule_id);
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0]["state"], "skipped");
    assert!(fires[0]["skippedCount"].as_i64().unwrap() > 100);
    // Nothing ran, and the user can still find out that something was missed.
    assert!(fires[0]["detail"]
        .as_str()
        .unwrap()
        .contains("does not catch up"));
    assert!(host
        .runs("bot-writer")
        .iter()
        .all(|run| run.kind != "schedule"));
}

#[test]
fn ticking_twice_does_not_fire_the_same_occurrence_twice() {
    let mut host = Host::start();
    let created = host.create("Hourly sweep", "0 * * * *");
    let schedule_id = created["scheduleId"].as_str().unwrap().to_string();
    host.owe_since(&schedule_id, chrono::Duration::hours(2));

    host.settle(&schedule_id, delivered);
    // Ten more pumps, each one a tick: the occurrence is already claimed, and
    // the uniqueness constraint is what says so.
    for _ in 0..10 {
        host.session.pump_acp();
    }
    assert_eq!(host.fires(&schedule_id).len(), 1);
    assert_eq!(
        host.runs("bot-writer")
            .iter()
            .filter(|run| run.kind == "schedule")
            .count(),
        1
    );
}

#[test]
fn disabling_a_schedule_parks_it_and_re_enabling_does_not_hand_it_the_backlog() {
    let mut host = Host::start();
    let created = host.create("Hourly sweep", "0 * * * *");
    let schedule_id = created["scheduleId"].as_str().unwrap().to_string();

    let off = host.ok(
        SCHEDULE_UPDATE,
        json!({ "scheduleId": schedule_id, "enabled": false }),
    );
    assert_eq!(off["enabled"], false);
    assert!(
        off["nextRunAt"].is_null(),
        "a disabled schedule owes nothing"
    );

    host.owe_since(&schedule_id, chrono::Duration::days(3));
    let on = host.ok(
        SCHEDULE_UPDATE,
        json!({ "scheduleId": schedule_id, "enabled": true }),
    );
    // Switching it off was the user saying they did not want those runs, so
    // switching it back on re-arms from now rather than from the outage.
    let next = on["nextRunAt"].as_str().expect("re-armed");
    assert!(
        next > chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            .as_str()
    );
    for _ in 0..5 {
        host.session.pump_acp();
    }
    assert!(host.fires(&schedule_id).is_empty());
}

#[test]
fn removing_a_schedule_stops_it_and_removing_a_ghost_is_an_error() {
    let mut host = Host::start();
    let created = host.create("Hourly sweep", "0 * * * *");
    let schedule_id = created["scheduleId"].as_str().unwrap().to_string();
    host.owe_since(&schedule_id, chrono::Duration::hours(2));

    let removed = host.ok(SCHEDULE_REMOVE, json!({ "scheduleId": schedule_id }));
    assert_eq!(removed["removed"], true);
    assert!(host.schedules().is_empty());
    for _ in 0..5 {
        host.session.pump_acp();
    }
    assert!(host
        .runs("bot-writer")
        .iter()
        .all(|run| run.kind != "schedule"));

    let err = host.err(SCHEDULE_REMOVE, json!({ "scheduleId": schedule_id }));
    assert_eq!(err.code, INVALID_PARAMS);
    // The temp dir has to outlive the assertions above.
    drop(host.dir);
}
