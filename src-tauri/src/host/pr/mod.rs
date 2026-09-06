//! Pull Requests: the board, and the link between a thread and what it opened (#28).
//!
//! Three jobs, and they are deliberately separate because they fail
//! separately.
//!
//! **Linkage** ([`detect`]) watches ACP traffic for the URL `gh pr create`
//! prints, and asks `gh` at turn end when a turn merely *suggested* a pull
//! request. It needs no credential and works on a machine that has never been
//! logged in — a PR the host cannot poll is still a PR the host knows this
//! thread opened.
//!
//! **The poll** ([`github`]) asks GitHub what those pull requests look like
//! now: title, state, diffstat, checks, review decision. It is the only part
//! that needs auth, it is a subprocess and a network round trip, and it is on
//! its own method (`pr/refresh`) precisely so that the board (`pr/list`) is a
//! store read that cannot fail.
//!
//! **The cards** ([`card`]) turn a *change* between two polls into an Inbox
//! row. Transitions, never states: a PR that has been red since lunch is not
//! news every fifteen seconds.
//!
//! **The person's own board** ([`HostSession::pr_mine`]) is the one place the
//! linkage premise is deliberately dropped: it asks GitHub what the signed-in
//! user has open anywhere, so the view can show every pull request they are
//! waiting on rather than only the ones a session here happened to open. A PR
//! that is both comes back carrying its thread.
//!
//! What ties them together is the row. `thread_prs.thread_id` is NOT NULL, so
//! every PR on this board was opened by a session on this Mac and "Reopen
//! thread" always has somewhere to go; `(provider, repo, number)` is the dedupe
//! key, so the same PR seen from stdout and from `gh pr view` is one row.
//!
//! Auth is the user's own `gh` login (#16). The token is never read into this
//! process, never persisted, and never logged: `gh api graphql` makes the
//! request with its own credential, which is one fewer place a token can leak
//! from than `gh auth token` would be.

pub mod card;
pub mod detect;
pub mod github;
pub mod workspace;

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::protocol::error::RpcError;
use super::protocol::methods::{
    GithubPullRequestView, PrCheckView, PrListParams, PrListResult, PrMineParams, PrMineResult,
    PrRefreshParams, PrRefreshResult, PrUnavailable, PullRequestView,
};
use super::repo::gh::DEFAULT_HOST;
use super::store::{
    NewThreadPr, Store, ThreadPrRow, ThreadRow, VIA_GH_PR_VIEW, VIA_HEAD_LIST, VIA_STDOUT,
};
use super::HostSession;
use detect::PrLink;
use github::{MinePr, PrAnswer, PrKey};

/// `thread_prs.provider`. One forge for MVP (`folders-and-auth.md`); the column
/// exists so a second one is a value rather than a schema change.
const PROVIDER_GITHUB: &str = "github";

impl HostSession {
    /// The board: every linked pull request, newest activity first.
    ///
    /// A store read and nothing else. No network, no subprocess, no `gh` — a
    /// user with no GitHub login still gets their PR list, showing what the
    /// linkage knows and saying (through `polledAt: null`) that GitHub has
    /// never been asked.
    pub fn pr_list(&mut self, params: PrListParams) -> Result<PrListResult, RpcError> {
        let store = self.store_or_err()?;
        let rows = match params.thread_id.as_deref() {
            Some(thread_id) => store.list_prs_for_thread(thread_id).map_err(internal)?,
            None => store.list_prs().map_err(internal)?,
        };
        Ok(PrListResult {
            pull_requests: views(store, rows)?,
        })
    }

    /// Ask GitHub about the linked pull requests and write down what changed.
    ///
    /// Never an error frame for "GitHub was not reachable". A client polls this
    /// every fifteen seconds while checks are moving, and a poll that throws
    /// because `gh` is not installed is a poll that takes the board down with
    /// it. The reason travels in `unavailable`, in the three-fact shape
    /// `github/status` established, and the rows come back regardless.
    pub fn pr_refresh(&mut self, params: PrRefreshParams) -> Result<PrRefreshResult, RpcError> {
        self.refresh_linked(params.thread_id.as_deref())
    }

    /// The body of `pr/refresh`, shared with [`HostSession::pr_tick`].
    ///
    /// One code path for the RPC and the tick on purpose: the cards are
    /// written here, and two paths would mean two chances for one of them to
    /// stop writing them.
    fn refresh_linked(&mut self, thread_id: Option<&str>) -> Result<PrRefreshResult, RpcError> {
        let params = PrRefreshParams {
            thread_id: thread_id.map(str::to_string),
        };
        // Discovery first, so a thread the caller just asked about can gain a
        // PR and have it polled in the same call rather than the next one.
        if let Some(thread_id) = params.thread_id.as_deref() {
            self.discover_pr_for_thread(thread_id);
        }

        let store = self.store_or_err()?;
        let rows = match params.thread_id.as_deref() {
            Some(thread_id) => store.list_prs_for_thread(thread_id).map_err(internal)?,
            None => store.list_prs().map_err(internal)?,
        };

        let mut checked = 0i64;
        let mut updated = 0i64;
        let mut unavailable: Vec<PrUnavailable> = Vec::new();
        let mut cards: Vec<(String, card::Card, ThreadPrRow)> = Vec::new();

        for (host, batch) in batches(&rows) {
            let keys: Vec<PrKey> = batch.iter().filter_map(key_for).collect();
            if keys.len() != batch.len() {
                // A row whose `repo` is not `owner/name` cannot be asked about.
                // It stays on the board — it is still a real link — and is
                // simply never polled.
                continue;
            }
            let query = github::build_query(&keys);
            let body = match github::fetch(&host, &query) {
                Ok(body) => body,
                Err(err) => {
                    unavailable.push(PrUnavailable {
                        remedy: err.remedy(&host),
                        host: host.clone(),
                        reason: err.reason().to_string(),
                        detail: err.detail(),
                    });
                    continue;
                }
            };
            for (row, answer) in batch.iter().zip(github::parse_response(&body, &keys)) {
                checked += 1;
                let PrAnswer::Found(snapshot) = answer else {
                    // A repository the token cannot see any more. Deliberately
                    // not an unlink: a revoked grant must not delete the board.
                    continue;
                };
                let store = self.store_or_err()?;
                let (before, after) = match store.apply_pr_snapshot(&row.id, &snapshot) {
                    Ok(pair) => pair,
                    Err(err) => {
                        eprintln!("failed to store PR {} #{}: {err}", row.repo, row.number);
                        continue;
                    }
                };
                if changed(&before, &after) {
                    updated += 1;
                }
                if let Some(card) = card::transition(&before, &after) {
                    cards.push((after.thread_id.clone(), card, after));
                }
            }
        }

        // Cards after every write, so a failure halfway through a batch cannot
        // announce a state the store does not hold.
        let card_count = cards.len() as i64;
        for (thread_id, card, row) in cards {
            self.write_pr_card(&thread_id, &card, &row);
        }

        let store = self.store_or_err()?;
        let rows = match params.thread_id.as_deref() {
            Some(thread_id) => store.list_prs_for_thread(thread_id).map_err(internal)?,
            None => store.list_prs().map_err(internal)?,
        };
        Ok(PrRefreshResult {
            pull_requests: views(store, rows)?,
            checked,
            updated,
            cards: card_count,
            unavailable,
        })
    }

    /// Every open pull request the signed-in user wrote — here or anywhere.
    ///
    /// The board's other two methods are about *linkage*: what a session on
    /// this Mac opened. This one is about the person. It needs a GitHub login
    /// to answer at all, which is why the view that draws it also offers the
    /// sign-in, and it holds the same promise `pr/refresh` does — an
    /// unreachable GitHub is a `unavailable` field and never an error frame,
    /// because this is polled on a timer beside the rest of the board.
    ///
    /// A PR that is *also* linked comes back carrying its thread, so the row
    /// keeps its "Reopen thread" button and the renderer can fold the two
    /// lists together on `(provider, repo, number)` without a second lookup.
    pub fn pr_mine(&mut self, params: PrMineParams) -> Result<PrMineResult, RpcError> {
        let host = params
            .host
            .clone()
            .unwrap_or_else(|| DEFAULT_HOST.to_string());
        let answer = github::fetch(&host, &github::mine_query()).and_then(|body| {
            // A body that parses but names no viewer is a failed *request*,
            // not an empty board — see `parse_mine`.
            github::parse_mine(&body)
        });
        let answer = match answer {
            Ok(answer) => answer,
            Err(err) => {
                return Ok(PrMineResult {
                    account: None,
                    pull_requests: Vec::new(),
                    unavailable: Some(PrUnavailable {
                        remedy: err.remedy(&host),
                        host,
                        reason: err.reason().to_string(),
                        detail: err.detail(),
                    }),
                })
            }
        };
        let store = self.store_or_err()?;
        let mut pull_requests = Vec::with_capacity(answer.pull_requests.len());
        for pr in answer.pull_requests {
            pull_requests.push(mine_view(store, &host, pr)?);
        }
        Ok(PrMineResult {
            account: answer.login,
            pull_requests,
            unavailable: None,
        })
    }

    // ---- linkage, driven by the ACP layer ------------------------------

    /// One `session/update`. Cheap: string scanning over a payload the host is
    /// already holding, and a store write only when a PR URL is actually found.
    pub(crate) fn pr_observe_update(&mut self, thread_id: &str, acp: &Value) {
        let links = self
            .pr_watch
            .entry(thread_id.to_string())
            .or_default()
            .observe(acp);
        for link in links {
            self.link_pr(thread_id, &link, VIA_STDOUT);
        }
    }

    /// The turn ended. If anything in it suggested a pull request we never saw
    /// a URL for — a truncated `gh pr create`, a PR opened through an MCP
    /// server, an agent that only *said* it had — ask `gh` now.
    ///
    /// Gated on suspicion rather than run on every turn end: this is a
    /// subprocess with a five-second deadline on the pump thread, and paying it
    /// for every turn of every chat would make a fast agent's turn feel slow
    /// for a question whose answer is almost always "no PR".
    pub(crate) fn pr_on_turn_end(&mut self, thread_id: &str) {
        let suspected = self
            .pr_watch
            .get(thread_id)
            .map(|watch| watch.suspected)
            .unwrap_or(false);
        if suspected {
            self.discover_pr_for_thread(thread_id);
        }
        // Per-turn state: the execute ids and the suspicion belong to the turn
        // that raised them, and carrying either into the next one would link a
        // PR to whichever conversation happened to run next.
        if let Some(watch) = self.pr_watch.get_mut(thread_id) {
            watch.reset();
        }
    }

    /// Drop a thread's linkage watch. Called when its adapter goes.
    pub(crate) fn pr_forget_thread(&mut self, thread_id: &str) {
        self.pr_watch.remove(thread_id);
    }

    /// Steps 2 and 3 of `pr-linkage.md`: ask `gh` which PR this thread's branch
    /// belongs to.
    ///
    /// `gh pr view` with no argument answers for the branch checked out in the
    /// directory, which is the authoritative answer and the one that catches a
    /// PR opened in the browser. When that fails — a fork whose head `gh`
    /// spells `owner:branch`, a detached HEAD — the branch is matched against
    /// the repository's open PRs instead.
    fn discover_pr_for_thread(&mut self, thread_id: &str) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Ok(Some(thread)) = store.get_thread(thread_id) else {
            return;
        };
        // Nothing to ask about: a worker's standing thread has no repo
        // (decision #6), and a folder that is not a checkout has no PRs.
        if thread.repo.is_none() {
            return;
        }
        let dir = std::path::PathBuf::from(&thread.cwd);
        if !dir.is_dir() {
            return;
        }
        if let Ok(Some(viewed)) = github::pr_for_cwd(&dir) {
            if let Some(link) = link_from_url(&viewed.url, &thread) {
                self.link_pr(thread_id, &link, VIA_GH_PR_VIEW);
                return;
            }
        }
        let Some(branch) = head_branch(&dir).or_else(|| thread.branch.clone()) else {
            return;
        };
        let Some(repo) = thread.repo.clone() else {
            return;
        };
        let host = thread
            .forge_host
            .clone()
            .unwrap_or_else(|| DEFAULT_HOST.to_string());
        if let Ok(Some(found)) = github::pr_for_branch(&host, &repo, &branch) {
            if let Some(link) = link_from_url(&found.url, &thread) {
                self.link_pr(thread_id, &link, VIA_HEAD_LIST);
            }
        }
    }

    /// Write the link, and announce a genuinely new pull request.
    ///
    /// The guard is what stops an agent that ran `gh pr view --repo
    /// somebody/else` from attaching a stranger's PR to this conversation for
    /// good: a link is accepted only for the thread's own repository, or for a
    /// repository of the same name under a different owner, which is what a
    /// fork's `gh pr create` prints.
    fn link_pr(&mut self, thread_id: &str, link: &PrLink, via: &str) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Ok(Some(thread)) = store.get_thread(thread_id) else {
            return;
        };
        if !belongs_to(link, &thread) {
            return;
        }
        let new = NewThreadPr {
            thread_id: thread_id.to_string(),
            provider: PROVIDER_GITHUB.to_string(),
            forge_host: Some(link.host.clone()),
            repo: link.slug(),
            number: link.number,
            url: link.url.clone(),
            detected_via: via.to_string(),
        };
        let (row, created) = match store.link_pr(&new) {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("failed to link {} #{}: {err}", new.repo, new.number);
                return;
            }
        };
        if created {
            let card = card::opened(&row);
            self.write_pr_card(thread_id, &card, &row);
        }
    }

    /// Persist the card, then tell the client — the same order decision #5
    /// gives every other Inbox row, for the same reason: a notification that
    /// fails must not be how a result is lost.
    ///
    /// `inbox/event` and not `inbox/resurface`: a PR card moves no thread. The
    /// session that opened it is usually finished and archived by the time its
    /// checks go red, and claiming it came back would be a lie about the
    /// sidebar (#25 made the same distinction for schedule fires).
    fn write_pr_card(&mut self, thread_id: &str, card: &card::Card, row: &ThreadPrRow) {
        let payload = json!({
            "event": card.event.as_str(),
            "prId": row.id,
            "provider": row.provider,
            "repo": row.repo,
            "number": row.number,
            "url": row.url,
        });
        let payload = serde_json::to_string(&payload).ok();
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if let Err(err) = store.insert_inbox_event(
            thread_id,
            None,
            "pr",
            &card.title,
            &card.summary,
            payload.as_deref(),
        ) {
            eprintln!("failed to write a PR card for {thread_id}: {err}");
            return;
        }
        self.notify_inbox_event(thread_id, "pr", &card.title, &card.summary);
    }
}

/// Whether a link found in this thread's output is plausibly this thread's PR.
///
/// A thread with no repo stamped on it accepts nothing: it is a worker's
/// standing thread, and decision #6 says workers have no checkout.
fn belongs_to(link: &PrLink, thread: &ThreadRow) -> bool {
    let Some(repo) = thread.repo.as_deref() else {
        return false;
    };
    if repo.eq_ignore_ascii_case(&link.slug()) {
        return true;
    }
    // A fork: `gh pr create` from `me/jabot` prints the upstream URL, and the
    // repository name is the only thing the two spellings share.
    repo.rsplit('/')
        .next()
        .map(|name| name.eq_ignore_ascii_case(&link.name))
        .unwrap_or(false)
}

fn link_from_url(url: &str, thread: &ThreadRow) -> Option<PrLink> {
    detect::scan(url)
        .into_iter()
        .find(|link| belongs_to(link, thread))
}

/// The branch checked out in a directory right now.
///
/// Not `threads.branch`: that is what the thread was *given* at spawn, and an
/// agent that rebased onto a new branch, or #23's rescue branch after a
/// detached HEAD, has moved on from it. Empty output means a detached HEAD,
/// which is not a branch anything can have opened a PR from.
fn head_branch(dir: &Path) -> Option<String> {
    super::git::worktree::head_branch(dir)
}

fn key_for(row: &ThreadPrRow) -> Option<PrKey> {
    let (owner, name) = row.repo.rsplit_once('/')?;
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(PrKey {
        owner: owner.to_string(),
        name: name.to_string(),
        number: row.number,
    })
}

/// Group the board by forge host, capped at one document's worth each.
///
/// A user with more linked PRs than [`github::BATCH`] gets the rest next tick.
/// The cap is GraphQL's node budget, not ours: one enormous document is how a
/// personal app trips a secondary rate limit.
fn batches(rows: &[ThreadPrRow]) -> Vec<(String, Vec<ThreadPrRow>)> {
    let mut by_host: Vec<(String, Vec<ThreadPrRow>)> = Vec::new();
    for row in rows {
        if row.provider != PROVIDER_GITHUB {
            continue;
        }
        let host = row
            .forge_host
            .clone()
            .unwrap_or_else(|| DEFAULT_HOST.to_string());
        match by_host.iter_mut().find(|(known, _)| known == &host) {
            Some((_, batch)) => batch.push(row.clone()),
            None => by_host.push((host, vec![row.clone()])),
        }
    }
    for (_, batch) in by_host.iter_mut() {
        // Least recently polled first, so a board longer than one batch still
        // refreshes every row eventually rather than the same 25 forever.
        batch.sort_by(|a, b| a.polled_at.cmp(&b.polled_at));
        batch.truncate(github::BATCH);
    }
    by_host
}

/// Did this poll learn anything? Compares GitHub's half only — `polled_at` and
/// `updated_at` move on every poll by definition and would make every refresh
/// report every row as changed.
fn changed(before: &ThreadPrRow, after: &ThreadPrRow) -> bool {
    before.title != after.title
        || before.status != after.status
        || before.check_state != after.check_state
        || before.review_state != after.review_state
        || before.additions != after.additions
        || before.deletions != after.deletions
        || before.changed_files != after.changed_files
        || before.checks_json != after.checks_json
        || before.head_ref != after.head_ref
        || before.base_ref != after.base_ref
}

fn views(store: &Store, rows: Vec<ThreadPrRow>) -> Result<Vec<PullRequestView>, RpcError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let thread = store.get_thread(&row.thread_id).map_err(internal)?;
        out.push(view(row, thread.as_ref()));
    }
    Ok(out)
}

pub(crate) fn view(row: ThreadPrRow, thread: Option<&ThreadRow>) -> PullRequestView {
    let checks: Vec<github::CheckView> = serde_json::from_str(&row.checks_json).unwrap_or_default();
    PullRequestView {
        thread_title: thread.map(|t| t.title.clone()).unwrap_or_default(),
        thread_state: thread
            .map(|t| t.state.clone())
            .unwrap_or_else(|| "deleted".to_string()),
        // GitHub's clock, then the linkage's, then the row's. Never the row's
        // `updated_at` alone: a poll that found nothing new moves it, and the
        // card's "38m ago" must not move with it.
        updated_at: row
            .pr_updated_at
            .clone()
            .or_else(|| row.detected_at.clone())
            .unwrap_or_else(|| row.created_at.clone()),
        id: row.id,
        thread_id: row.thread_id,
        provider: row.provider,
        forge_host: row.forge_host,
        repo: row.repo,
        number: row.number,
        url: row.url,
        title: row.title,
        status: row.status,
        check_state: row.check_state,
        review_state: row.review_state,
        head_ref: row.head_ref,
        base_ref: row.base_ref,
        additions: row.additions,
        deletions: row.deletions,
        changed_files: row.changed_files,
        checks: checks
            .into_iter()
            .map(|check| PrCheckView {
                label: check.label,
                state: check.state,
            })
            .collect(),
        detected_via: row.detected_via,
        polled_at: row.polled_at,
    }
}

/// One of the viewer's PRs as the wire shape, with whatever this Mac knows
/// about it attached.
///
/// The store lookup is by `(provider, repo, number)` — the same identity
/// `link_pr` dedupes on — so a PR the user opened from a JaBot session is one
/// row on the board rather than two, and keeps the thread it came from.
fn mine_view(store: &Store, host: &str, pr: MinePr) -> Result<GithubPullRequestView, RpcError> {
    let linked = store
        .get_pr(PROVIDER_GITHUB, &pr.repo, pr.number)
        .map_err(internal)?;
    let thread = match linked.as_ref() {
        Some(row) => store.get_thread(&row.thread_id).map_err(internal)?,
        None => None,
    };
    let checks: Vec<github::CheckView> =
        serde_json::from_str(&pr.snapshot.checks_json).unwrap_or_default();
    Ok(GithubPullRequestView {
        id: format!("{PROVIDER_GITHUB}:{}#{}", pr.repo, pr.number),
        linked_id: linked.as_ref().map(|row| row.id.clone()),
        provider: PROVIDER_GITHUB.to_string(),
        forge_host: host.to_string(),
        number: pr.number,
        // GitHub's own URL when it gave one; the canonical one otherwise, so a
        // row is never a dead link.
        url: pr
            .snapshot
            .url
            .clone()
            .unwrap_or_else(|| format!("https://{host}/{}/pull/{}", pr.repo, pr.number)),
        title: pr.snapshot.title,
        status: pr.snapshot.status,
        check_state: pr.snapshot.check_state,
        review_state: pr.snapshot.review_state,
        head_ref: pr.snapshot.head_ref,
        base_ref: pr.snapshot.base_ref,
        additions: pr.snapshot.additions,
        deletions: pr.snapshot.deletions,
        changed_files: pr.snapshot.changed_files,
        checks: checks
            .into_iter()
            .map(|check| PrCheckView {
                label: check.label,
                state: check.state,
            })
            .collect(),
        // GitHub just told us; there is no older clock to prefer here.
        updated_at: pr
            .snapshot
            .pr_updated_at
            .unwrap_or_else(|| now_iso(linked.as_ref())),
        // Only ever set together, and only for a PR this Mac opened.
        thread_id: linked.as_ref().map(|row| row.thread_id.clone()),
        thread_title: thread.as_ref().map(|t| t.title.clone()),
        thread_state: thread.as_ref().map(|t| t.state.clone()),
        repo: pr.repo,
    })
}

/// The fallback timestamp for a PR GitHub answered about without an
/// `updatedAt`. The linked row's clock if there is one, so the card does not
/// jump to the epoch; otherwise the row's own creation, which is the only
/// other honest thing to say.
fn now_iso(linked: Option<&ThreadPrRow>) -> String {
    linked
        .and_then(|row| {
            row.pr_updated_at
                .clone()
                .or_else(|| row.detected_at.clone())
        })
        .unwrap_or_default()
}

fn internal(err: super::store::StoreError) -> RpcError {
    match err {
        super::store::StoreError::NotFound(id) => RpcError::ThreadNotFound(id),
        other => RpcError::Internal(other.to_string()),
    }
}

/// Every PR the host is holding for a thread, as the wire shape. Used by
/// `thread/state` so a thread can name its own pull request without the client
/// scanning the whole board.
pub(crate) fn thread_prs(store: &Store, thread_id: &str) -> Vec<PullRequestView> {
    let Ok(rows) = store.list_prs_for_thread(thread_id) else {
        return Vec::new();
    };
    let thread = store.get_thread(thread_id).ok().flatten();
    rows.into_iter()
        .map(|row| view(row, thread.as_ref()))
        .collect()
}

/// Per-thread linkage state, keyed by thread id. RAM, like the prompt queue.
pub(crate) type PrWatchMap = HashMap<String, detect::PrWatch>;

/// The PR poll's clock, on the host rather than in a renderer (#28, #21).
///
/// `usePullRequests` owned the whole poll: a `setInterval` in the renderer,
/// armed only while a webview was alive and running. Since `card::transition`
/// only writes an Inbox `pr` card when a *refresh* observes a change, no
/// "checks failed" and no "changes requested" card could ever be written while
/// the app sat in the Dock with its timers throttled — and `jabot-hostd` runs
/// the same pump with no renderer at all, so a paired phone got no PR polling
/// and no PR cards whatsoever.
///
/// RAM, like the cron's clock beside it: nothing here needs to survive a quit,
/// because the first pump after a launch polls immediately.
#[derive(Debug)]
pub(crate) struct PrPoll {
    /// `JABOT_PR_POLL_MS=0` turns the background poll off entirely, the way
    /// `JABOT_IDLE_EVICT_MS=0` turns idle eviction off. Two callers want it:
    /// a test that is asserting what an explicit `pr/refresh` does and cannot
    /// have a tick racing it, and a user who would rather their machine never
    /// shelled out to `gh` unless they were looking.
    enabled: bool,
    last_poll: Instant,
    /// Checks in flight is the one case where seconds matter (`pr-linkage.md`).
    pending_interval: Duration,
    idle_interval: Duration,
    /// Set when a poll came back `unavailable` — no `gh`, or no login. Without
    /// it a machine that has never signed in would shell out every minute
    /// forever to be told the same thing.
    backoff_until: Option<Instant>,
}

/// The two cadences `pr-linkage.md` gives, and the same pair the renderer used.
const DEFAULT_PR_POLL: Duration = Duration::from_secs(15);
const DEFAULT_PR_POLL_IDLE: Duration = Duration::from_secs(60);
/// Long enough that a machine with no `gh` is not paying for a subprocess on
/// any human timescale, short enough that signing in during a session starts
/// working without a relaunch.
const PR_UNAVAILABLE_BACKOFF: Duration = Duration::from_secs(300);

impl Default for PrPoll {
    fn default() -> Self {
        Self {
            enabled: true,
            // In the past, so the first pump after a launch polls rather than
            // waiting out an interval of silence — which is exactly the window
            // a relaunch after a crash lands in.
            last_poll: Instant::now() - DEFAULT_PR_POLL_IDLE,
            pending_interval: DEFAULT_PR_POLL,
            idle_interval: DEFAULT_PR_POLL_IDLE,
            backoff_until: None,
        }
    }
}

impl PrPoll {
    /// `JABOT_PR_POLL_MS` and `JABOT_PR_POLL_IDLE_MS`, stand-ins for the
    /// settings #26 owns and the only way a test watches a card land in
    /// milliseconds rather than a minute. Read the way `Supervisor::from_env`
    /// reads its own three.
    pub fn from_env() -> Self {
        let raw = std::env::var("JABOT_PR_POLL_MS").ok();
        let pending_interval = poll_interval(raw.as_deref(), DEFAULT_PR_POLL);
        let idle_interval = poll_interval(
            std::env::var("JABOT_PR_POLL_IDLE_MS").ok().as_deref(),
            DEFAULT_PR_POLL_IDLE,
        );
        Self {
            enabled: !poll_disabled(raw.as_deref()),
            last_poll: Instant::now() - idle_interval.max(pending_interval),
            pending_interval,
            idle_interval,
            backoff_until: None,
        }
    }
}

/// One env knob, parsed. Its own function so the rule can be tested without
/// a process-wide `set_var`, which is racy under a parallel test runner.
///
/// Nonsense and absence fall back to the shipped cadence. A zero does too —
/// it is read by [`poll_disabled`] instead, because "every zero milliseconds"
/// is not a cadence anybody wants.
fn poll_interval(raw: Option<&str>, fallback: Duration) -> Duration {
    raw.and_then(|raw| raw.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .filter(|d| !d.is_zero())
        .unwrap_or(fallback)
}

/// An explicit `0` on `JABOT_PR_POLL_MS`, which turns the background poll off
/// — the same reading `JABOT_IDLE_EVICT_MS=0` gets. Nonsense does not: a
/// mistyped value should leave the shipped behaviour alone rather than
/// silently disable a thing the user never asked to disable.
fn poll_disabled(raw: Option<&str>) -> bool {
    raw.map(str::trim).and_then(|raw| raw.parse::<u64>().ok()) == Some(0)
}

/// How often to ask, given what is on the board.
///
/// `None` means never: a board with no linked rows has nothing to ask about,
/// and the point of checking is that a machine which has never opened a pull
/// request never spawns `gh` at all.
pub(crate) fn pr_cadence(rows: &[ThreadPrRow], poll: &PrPoll) -> Option<Duration> {
    if !poll.enabled || rows.is_empty() {
        return None;
    }
    // The same rule the renderer used: something is running, so seconds
    // matter. A row with no checks configured is not a row that is waiting.
    let running = rows
        .iter()
        .any(|row| row.check_state.as_deref() == Some("running"));
    Some(if running {
        poll.pending_interval
    } else {
        poll.idle_interval
    })
}

impl HostSession {
    /// Called from the ACP pump, beside the cron. Rate-limited inside.
    ///
    /// Its own call site rather than a line inside `supervisor_tick`, because
    /// that early-returns on the sleep path — and a machine that has just woken
    /// is precisely the one whose board is most out of date.
    pub(crate) fn pr_tick(&mut self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let rows = match store.list_prs() {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("pr poll: could not read the board: {err}");
                return;
            }
        };
        let Some(interval) = pr_cadence(&rows, &self.pr_poll) else {
            return;
        };
        let now = Instant::now();
        if self.pr_poll.backoff_until.is_some_and(|until| now < until) {
            return;
        }
        if now.duration_since(self.pr_poll.last_poll) < interval {
            return;
        }
        self.pr_poll.last_poll = now;
        match self.refresh_linked(None) {
            // `unavailable` is not an error — a board is still a board without
            // a GitHub login — but it is the answer that means asking again in
            // fifteen seconds buys nothing.
            Ok(result) if !result.unavailable.is_empty() => {
                self.pr_poll.backoff_until = Some(Instant::now() + PR_UNAVAILABLE_BACKOFF);
            }
            Ok(_) => self.pr_poll.backoff_until = None,
            Err(err) => eprintln!("pr poll: {err:?}"),
        }
    }

    /// A machine that has just woken has the most out-of-date board in the
    /// house, so the next pump polls rather than waiting out an interval that
    /// was measured across the sleep. The backoff is cleared with it: a `gh`
    /// that was not logged in before the lid closed may well be now.
    pub(crate) fn pr_poll_now(&mut self) {
        self.pr_poll.last_poll = Instant::now()
            - self
                .pr_poll
                .idle_interval
                .max(self.pr_poll.pending_interval);
        self.pr_poll.backoff_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(repo: Option<&str>) -> ThreadRow {
        ThreadRow {
            id: "t-auth".into(),
            folder_id: Some("f".into()),
            bot_id: Some("code".into()),
            harness_id: "claude".into(),
            acp_session_id: None,
            native_session_ref: None,
            cwd: "/tmp/x".into(),
            runtime_json: "{}".into(),
            title: "Auth migration".into(),
            state: "active".into(),
            fold_policy: "default".into(),
            last_stop_reason: None,
            last_error: None,
            preview: None,
            worktree_path: None,
            created_at: "2026-08-21T09:00:00Z".into(),
            updated_at: "2026-08-21T09:00:00Z".into(),
            folded_at: None,
            resurfaced_at: None,
            archived_at: None,
            deleted_at: None,
            resurfaced_reason: None,
            repo_root: Some("/tmp/x".into()),
            repo: repo.map(str::to_string),
            forge_host: Some("github.com".into()),
            branch: Some("jabot/t-auth".into()),
            host_id: None,
        }
    }

    fn link(slug: &str, number: i64) -> PrLink {
        let (owner, name) = slug.split_once('/').unwrap();
        PrLink {
            host: "github.com".into(),
            owner: owner.into(),
            name: name.into(),
            number,
            url: format!("https://github.com/{slug}/pull/{number}"),
        }
    }

    /// The guard that stops `gh pr view --repo somebody/else` from attaching a
    /// stranger's PR to this conversation for good.
    #[test]
    fn a_link_is_only_accepted_for_the_threads_own_repository() {
        let code = thread(Some("jabreeflor/jabot"));
        assert!(belongs_to(&link("jabreeflor/jabot", 23), &code));
        // A fork opens against upstream; the repository name is what they share.
        assert!(belongs_to(&link("upstream-org/jabot", 23), &code));
        assert!(!belongs_to(&link("somebody/else", 1), &code));
        // A worker's standing thread has no repo and can open nothing.
        assert!(!belongs_to(&link("jabreeflor/jabot", 23), &thread(None)));
    }

    #[test]
    fn the_board_is_grouped_by_forge_host() {
        let mut rows = vec![pr_row("github.com", 1), pr_row("git.corp.example.com", 2)];
        rows.push(pr_row("github.com", 3));
        let batched = batches(&rows);
        assert_eq!(batched.len(), 2);
        let github = batched
            .iter()
            .find(|(host, _)| host == "github.com")
            .expect("github batch");
        assert_eq!(github.1.len(), 2);
    }

    /// A board longer than one document must not poll the same 25 rows for
    /// ever; least-recently-polled goes first.
    #[test]
    fn a_batch_is_capped_and_takes_the_stalest_rows_first() {
        let mut rows: Vec<ThreadPrRow> = (0..github::BATCH as i64 + 5)
            .map(|n| {
                let mut row = pr_row("github.com", n + 1);
                row.polled_at = Some(format!("2026-08-21T10:{:02}:00Z", n));
                row
            })
            .collect();
        let stalest = rows.len() - 1;
        rows[stalest].polled_at = None;
        let batched = batches(&rows);
        assert_eq!(batched[0].1.len(), github::BATCH);
        assert_eq!(
            batched[0].1[0].number, rows[stalest].number,
            "a row never polled is the stalest of all"
        );
    }

    #[test]
    fn a_poll_that_learned_nothing_reports_nothing_changed() {
        let before = pr_row("github.com", 1);
        let mut after = before.clone();
        // Only the bookkeeping moved, which every poll does by definition.
        after.polled_at = Some("2026-08-21T11:00:00Z".into());
        after.updated_at = "2026-08-21T11:00:00Z".into();
        assert!(!changed(&before, &after));
        after.check_state = Some("failing".into());
        assert!(changed(&before, &after));
    }

    /// The card's timestamp is GitHub's, never ours — otherwise every poll
    /// would redate every row to "just now".
    #[test]
    fn the_view_dates_a_row_by_github_and_not_by_the_last_poll() {
        let mut row = pr_row("github.com", 1);
        row.pr_updated_at = Some("2026-08-21T09:14:02Z".into());
        row.updated_at = "2026-08-21T11:59:59Z".into();
        let view = view(row, Some(&thread(Some("jabreeflor/jabot"))));
        assert_eq!(view.updated_at, "2026-08-21T09:14:02Z");
        assert_eq!(view.thread_title, "Auth migration");
        assert_eq!(view.thread_state, "active");
    }

    #[test]
    fn a_row_carries_its_checks_line_to_the_wire() {
        let mut row = pr_row("github.com", 1);
        row.checks_json = r#"[{"label":"tests","state":"failing"}]"#.into();
        let view = view(row, None);
        assert_eq!(view.checks.len(), 1);
        assert_eq!(view.checks[0].label, "tests");
        assert_eq!(view.checks[0].state, "failing");
        // No thread row left: the board says so rather than inventing a title.
        assert_eq!(view.thread_state, "deleted");
    }

    fn pr_row(host: &str, number: i64) -> ThreadPrRow {
        ThreadPrRow {
            id: format!("pr-{host}-{number}"),
            thread_id: "t-auth".into(),
            provider: PROVIDER_GITHUB.into(),
            forge_host: Some(host.into()),
            repo: "jabreeflor/jabot".into(),
            number,
            url: format!("https://{host}/jabreeflor/jabot/pull/{number}"),
            title: "Migrate auth to sessions".into(),
            status: "open".into(),
            check_state: Some("passing".into()),
            review_state: None,
            head_ref: Some("jabot/t-auth".into()),
            base_ref: Some("main".into()),
            additions: 1,
            deletions: 1,
            changed_files: 1,
            checks_json: "[]".into(),
            pr_updated_at: None,
            detected_via: Some(VIA_STDOUT.into()),
            detected_at: Some("2026-08-21T09:00:00Z".into()),
            polled_at: None,
            created_at: "2026-08-21T09:00:00Z".into(),
            updated_at: "2026-08-21T09:00:00Z".into(),
        }
    }

    /// The poll's clock (#28), now that it is the host's rather than a
    /// `setInterval` in a renderer that may not exist.
    mod poll {
        use super::*;

        fn poll() -> PrPoll {
            PrPoll {
                enabled: true,
                last_poll: Instant::now(),
                pending_interval: Duration::from_secs(15),
                idle_interval: Duration::from_secs(60),
                backoff_until: None,
            }
        }

        /// The load-bearing one. A machine that has never opened a pull
        /// request must never spawn `gh` — not every fifteen seconds, not
        /// every minute, not at all — and "no rows" is how that is known
        /// without asking anybody.
        #[test]
        fn an_empty_board_is_never_polled() {
            assert_eq!(pr_cadence(&[], &poll()), None);
        }

        /// The same rule the renderer used: checks in flight is the one case
        /// where seconds matter.
        #[test]
        fn a_running_check_earns_the_fast_cadence() {
            let mut row = pr_row("github.com", 23);
            row.check_state = Some("running".into());
            assert_eq!(pr_cadence(&[row], &poll()), Some(Duration::from_secs(15)));
        }

        #[test]
        fn a_board_with_nothing_moving_is_polled_once_a_minute() {
            // `passing`, and a PR with no checks configured at all — which is
            // not the same as checks that have not started.
            let mut none = pr_row("github.com", 24);
            none.check_state = None;
            assert_eq!(
                pr_cadence(&[pr_row("github.com", 23), none], &poll()),
                Some(Duration::from_secs(60))
            );
        }

        /// One running row among many is enough: the fast cadence is about
        /// whether *anything* on the board is moving.
        #[test]
        fn one_running_row_speeds_up_the_whole_board() {
            let mut running = pr_row("github.com", 25);
            running.check_state = Some("running".into());
            let rows = vec![pr_row("github.com", 23), running, pr_row("github.com", 24)];
            assert_eq!(pr_cadence(&rows, &poll()), Some(Duration::from_secs(15)));
        }

        /// The env knobs are what let an e2e case watch a card land in
        /// milliseconds rather than a minute. Parsed through a function
        /// rather than read from the process, so the rule can be pinned
        /// without a `set_var` race under a parallel runner.
        #[test]
        fn a_knob_overrides_the_shipped_cadence() {
            assert_eq!(
                poll_interval(Some("200"), DEFAULT_PR_POLL),
                Duration::from_millis(200)
            );
            assert_eq!(
                poll_interval(Some(" 900 "), DEFAULT_PR_POLL_IDLE),
                Duration::from_millis(900)
            );
        }

        #[test]
        fn a_zero_or_a_nonsense_value_falls_back() {
            for raw in [Some("0"), Some("soon"), Some(""), None] {
                assert_eq!(
                    poll_interval(raw, DEFAULT_PR_POLL),
                    DEFAULT_PR_POLL,
                    "{raw:?}"
                );
            }
        }

        /// An explicit zero turns the poll off, the way `JABOT_IDLE_EVICT_MS`
        /// turns idle eviction off. A mistyped value must not: silently
        /// disabling something the user never asked to disable is worse than
        /// ignoring the typo.
        #[test]
        fn an_explicit_zero_turns_the_poll_off_and_a_typo_does_not() {
            assert!(poll_disabled(Some("0")));
            assert!(poll_disabled(Some(" 0 ")));
            for raw in [Some("soon"), Some(""), Some("200"), None] {
                assert!(!poll_disabled(raw), "{raw:?}");
            }

            let off = PrPoll {
                enabled: false,
                ..poll()
            };
            let mut running = pr_row("github.com", 23);
            running.check_state = Some("running".into());
            assert_eq!(pr_cadence(&[running], &off), None);
        }

        /// The first pump after a launch polls rather than waiting an interval
        /// out — which is exactly the window a relaunch after a crash lands in.
        #[test]
        fn the_clock_starts_already_expired() {
            let fresh = PrPoll::default();
            assert!(fresh.last_poll.elapsed() >= DEFAULT_PR_POLL_IDLE);
        }
    }
}
