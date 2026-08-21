//! Thread ↔ PR linkage, driven through the host API (#28).
//!
//! `src/host/pr/` unit-tests the three pieces in isolation: what counts as
//! evidence, how a recorded GitHub answer maps onto a row, and which changes
//! deserve an Inbox card. This file puts a real `fake-acp-agent` subprocess and
//! a real git repository behind them, because the claim a user depends on is
//! about a live turn: an agent runs `gh pr create`, and the pull request turns
//! up on the board attached to the conversation that opened it.
//!
//! There is no GitHub credential in this environment and no egress to the API,
//! so nothing here polls. What is asserted instead is everything on *this* side
//! of the token: linkage, dedupe, the guard against linking somebody else's
//! repository, the Inbox card, and — the case every JaBot user without a `gh`
//! login is in — that a refresh which cannot reach GitHub says so and leaves
//! the board standing.

use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use jabot_lib::host::{
    HostSession, JsonRpcRequest, JsonRpcResponse, RequestId, FOLDER_REGISTER, HOST_HELLO,
    INBOX_LIST, PR_LIST, PR_REFRESH, SESSION_PROMPT, THREAD_OPEN, THREAD_STATE,
};
use serde_json::{json, Value};

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

struct Host {
    session: HostSession,
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    repo: tempfile::TempDir,
    next_id: i64,
}

impl Host {
    /// A host with one registered folder that is a real checkout with a real
    /// `origin`, because linkage is refused for a thread with no repository.
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path(), "git@github.com:jabreeflor/jabot.git");
        let mut session = HostSession::load(dir.path());
        session.handle_request(req(1, HOST_HELLO, None));
        let mut host = Self {
            session,
            dir,
            repo,
            next_id: 2,
        };
        let path = host.repo.path().to_string_lossy().into_owned();
        host.ok(FOLDER_REGISTER, json!({ "path": path }));
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

    /// A code thread in the registered folder. `execute` mode makes the agent
    /// echo the prompt back as the stdout of a shell tool call, which is
    /// exactly the shape `gh pr create` produces.
    fn open_thread(&mut self, thread_id: &str) {
        let cwd = self.repo.path().to_string_lossy().into_owned();
        self.ok(
            THREAD_OPEN,
            json!({
                "threadId": thread_id,
                "title": "Auth migration",
                "cwd": cwd,
                "harnessId": "claude",
                "runtime": { "command": fake_agent(), "args": ["execute"] }
            }),
        );
    }

    /// Prompt, then pump until the turn's tool call has been consumed.
    fn say(&mut self, thread_id: &str, text: &str) {
        let response = self.call(
            SESSION_PROMPT,
            json!({ "threadId": thread_id, "content": text }),
        );
        assert!(response.error.is_none(), "prompt failed: {response:?}");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.session.pump_acp();
            let state = self.ok(THREAD_STATE, json!({ "threadId": thread_id }));
            let running = state["latestRun"]["state"].as_str() == Some("running");
            if !running {
                return;
            }
            if Instant::now() > deadline {
                panic!("{thread_id} never finished its turn: {state}");
            }
            thread::sleep(Duration::from_millis(15));
        }
    }

    fn board(&mut self) -> Vec<Value> {
        let listed = self.ok(PR_LIST, json!({}));
        listed["pullRequests"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn inbox(&mut self) -> Value {
        self.ok(INBOX_LIST, json!({}))
    }
}

fn init_repo(dir: &std::path::Path, origin: &str) {
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git is required to build");
        assert!(
            status.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    };
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    git(&["remote", "add", "origin", origin]);
    std::fs::write(dir.join("README.md"), "# project\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "first"]);
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

/// The headline: what `gh pr create` printed becomes a row on the board, and
/// the row knows which conversation produced it.
#[test]
fn a_pr_url_in_a_shell_call_links_the_thread_that_printed_it() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.say("t-auth", "https://github.com/jabreeflor/jabot/pull/23");

    let board = host.board();
    assert_eq!(board.len(), 1, "expected one linked PR: {board:?}");
    let pr = &board[0];
    assert_eq!(pr["threadId"], "t-auth");
    assert_eq!(pr["repo"], "jabreeflor/jabot");
    assert_eq!(pr["number"], 23);
    assert_eq!(pr["provider"], "github");
    assert_eq!(pr["forgeHost"], "github.com");
    assert_eq!(pr["detectedVia"], "stdout");
    // Linked, never polled — which is the honest state of every row on a
    // machine with no `gh` login, and is why the view can say so.
    assert!(pr.get("polledAt").is_none(), "{pr}");
    // The thread half of the same link, so a thread view needs no board.
    let state = host.ok(THREAD_STATE, json!({ "threadId": "t-auth" }));
    assert_eq!(state["pullRequests"][0]["number"], 23);
}

/// `(provider, repo, number)` is the dedupe key: the same PR seen in two turns
/// — or seen twice in one — is one row, or the board grows a duplicate every
/// time the agent prints the URL again.
#[test]
fn the_same_pull_request_seen_twice_is_one_row() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.say("t-auth", "https://github.com/jabreeflor/jabot/pull/23");
    host.say(
        "t-auth",
        "still https://github.com/jabreeflor/jabot/pull/23 and \
         https://github.com/jabreeflor/jabot/pull/23",
    );
    host.say("t-auth", "github.com/jabreeflor/jabot/pull/23");

    let board = host.board();
    assert_eq!(board.len(), 1, "expected one row: {board:?}");
    assert_eq!(board[0]["number"], 23);

    // And one card, because only the first sighting was news.
    let inbox = host.inbox();
    let cards: Vec<&Value> = inbox["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "pr")
        .collect();
    assert_eq!(cards.len(), 1, "{:?}", inbox["events"]);
}

/// Opening a PR is the outcome of a coding session, so it earns an Inbox card
/// — and the card carries enough to open the pull request without a lookup.
#[test]
fn opening_a_pull_request_writes_an_inbox_card_that_names_it() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.say("t-auth", "https://github.com/jabreeflor/jabot/pull/23");

    let inbox = host.inbox();
    let card = inbox["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "pr")
        .unwrap_or_else(|| panic!("no PR card in {:?}", inbox["events"]));
    assert_eq!(card["title"], "PR #23 opened");
    assert_eq!(card["threadId"], "t-auth");
    assert_eq!(card["payload"]["event"], "opened");
    assert_eq!(card["payload"]["number"], 23);
    assert_eq!(
        card["payload"]["url"],
        "https://github.com/jabreeflor/jabot/pull/23"
    );
    // The thread never folded, so nothing resurfaced — and the card still has
    // to be counted, or nobody is ever told it exists.
    assert_eq!(inbox["events"][0]["threadState"], "active");
    assert!(
        inbox["unread"].as_i64().unwrap_or(0) >= 1,
        "a PR card must badge: {inbox}"
    );
}

/// The failure this costs the most to undo: a link is written once and never
/// re-derived, so a URL for a repository this thread has nothing to do with
/// must not become this thread's pull request.
#[test]
fn a_pull_request_in_another_repository_is_not_this_threads() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.say("t-auth", "https://github.com/somebody/else/pull/7");
    assert!(host.board().is_empty(), "{:?}", host.board());

    // A fork is the exception, and a deliberate one: `gh pr create` from a
    // fork prints the *upstream* URL, and the repository name is all the two
    // spellings share.
    host.say("t-auth", "https://github.com/upstream-org/jabot/pull/7");
    let board = host.board();
    assert_eq!(board.len(), 1);
    assert_eq!(board[0]["repo"], "upstream-org/jabot");
}

/// A compare URL is what `git push` prints on every single push. It names no
/// pull request, and linking one would attach a number that does not exist.
#[test]
fn pushing_a_branch_does_not_open_a_pull_request() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.say(
        "t-auth",
        "remote: Create a pull request for 'jabot/t-auth' on GitHub by visiting:\n\
         remote:      https://github.com/jabreeflor/jabot/compare/jabot/t-auth?expand=1",
    );
    assert!(host.board().is_empty(), "{:?}", host.board());
}

/// The state every user without a `gh` login is in. A poll that cannot reach
/// GitHub is not an error frame — the client polls this every fifteen seconds
/// — and it must not cost the user their board.
#[test]
fn a_refresh_with_no_github_cli_says_so_and_keeps_the_board() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.say("t-auth", "https://github.com/jabreeflor/jabot/pull/23");

    let refreshed = host.ok(PR_REFRESH, json!({}));
    let board = refreshed["pullRequests"].as_array().unwrap();
    assert_eq!(board.len(), 1, "the board survives a failed refresh");

    // On a machine that *does* have `gh` this reaches the network and the
    // assertion below would be wrong, so it is conditional on the environment
    // rather than on a mock: what must always hold is that the call succeeds
    // and the row is still there.
    let unavailable = refreshed["unavailable"].as_array().unwrap();
    if jabot_lib::host::resolve_command("gh").is_none() {
        assert_eq!(unavailable.len(), 1, "{refreshed}");
        assert_eq!(unavailable[0]["reason"], "gh_missing");
        assert_eq!(unavailable[0]["host"], "github.com");
        assert_eq!(unavailable[0]["remedy"], "brew install gh");
        assert_eq!(refreshed["checked"], 0);
        assert_eq!(refreshed["cards"], 0);
    }
}

/// Narrowing to one thread is what a thread view asks for, and it must not
/// answer with somebody else's pull requests.
#[test]
fn the_board_can_be_narrowed_to_one_thread() {
    let mut host = Host::start();
    host.open_thread("t-auth");
    host.open_thread("t-sidebar");
    host.say("t-auth", "https://github.com/jabreeflor/jabot/pull/23");
    host.say("t-sidebar", "https://github.com/jabreeflor/jabot/pull/24");

    assert_eq!(host.board().len(), 2);
    let mine = host.ok(PR_LIST, json!({ "threadId": "t-sidebar" }));
    let mine = mine["pullRequests"].as_array().unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["number"], 24);
    assert_eq!(mine[0]["threadId"], "t-sidebar");
}
