# PR data and thread linkage

How the Pull Requests view gets data, how checks stay live-ish, and how
Inbox learns "PR #23 opened". Complements [findings.md](findings.md) Q3
and Q5.

## Decision

- **Forge:** GitHub.com / GHES for MVP ([folders-and-auth.md](folders-and-auth.md)).
- **Host queries:** Octokit GraphQL (or `gh api graphql`) with the user's
  `gh` token. One query per refresh: title, number, url, isDraft, state,
  additions/deletions, `statusCheckRollup`, head/base.
- **Do not** drive the UI by scraping `gh pr list` human output.
- **Do** let agents keep using `gh pr create` / `git push`; we observe.
- **Updates:** poll. Webhooks need a public HTTPS URL GitHub can POST to;
  `localhost` is rejected
  ([docs](https://docs.github.com/en/webhooks/testing-and-troubleshooting-webhooks/troubleshooting-webhooks)).
- **Linkage:** persist `{host, owner, repo, number, url, threadId}` as
  soon as we have high-confidence evidence. Inbox and the PR view read
  that store, then refresh from the API.

## `gh` vs GraphQL vs Octokit

All three talk to the same GitHub API. The split is **who calls** and
**how often**.

| Tool | Role |
|---|---|
| **`gh` CLI (agent)** | Create, push, checkout, merge from inside the worktree. Already on PATH for this user. `gh pr create` prints the URL ([manual](https://cli.github.com/manual/gh_pr_create)). |
| **`gh pr list/view/checks --json`** | Excellent for one-shot scripts and for **linkage from cwd**. JSON fields include `number`, `url`, `isDraft`, `additions`, `deletions`, `statusCheckRollup`, `state` ([list](https://cli.github.com/manual/gh_pr_list), [view](https://cli.github.com/manual/gh_pr_view)). `gh pr checks` adds `bucket`: pass/fail/pending/skipping/cancel, exit 8 if pending, `--watch --interval 10` ([checks](https://cli.github.com/manual/gh_pr_checks)). |
| **Octokit GraphQL in the host** | PR view refresh. One nested query replaces list + per-PR checks + diffstat. Official client: [octokit.js](https://github.com/octokit/octokit.js). Same token as `gh`. |
| **`gh api graphql`** | Fine as the first implementation (no extra dep). Switch to Octokit when we want typed queries and ETag/conditional REST. |

`gh` internally already uses GraphQL for `pr checks` (`statusCheckRollup`
on the last commit). We should too.

Do not spawn `gh pr checks --watch` as a long-lived child per PR — that is
a hidden poller we don't control, and it fights app-lifecycle. Poll from
the host.

### Fields the prototype needs

GraphQL `PullRequest` already has `additions`, `deletions`,
`changedFiles`, `isDraft`, `url`, `statusCheckRollup`
([reference](https://docs.github.com/en/graphql/reference/pulls)).
`StatusCheckRollup.state` plus `contexts` map to "48 tests · lint · build"
and "2 of 3 checks done".

```graphql
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number title url isDraft state
      additions deletions changedFiles
      headRefName baseRefName
      statusCheckRollup { state contexts(first: 20) { nodes { __typename } } }
    }
  }
}
```

List page: query each **linked** PR (dozens, not thousands), or
`repository.pullRequests(states: OPEN, first: 30)` **filtered in the
client** to `number ∈ linked set`. Never show unlinked coworker PRs.

Merge button: `gh pr merge <n> --repo owner/repo` or GraphQL
`mergePullRequest`. Prefer the API from the UI so failures are structured.
Honor merge queues (`gh pr merge` enables auto-merge when required
checks are pending — [manual](https://cli.github.com/manual/gh_pr_merge)).

View diff (MVP): `gh pr view <n> --web` or open `url`. In-app diff is a
later renderer (ACP already streams file diffs for the *thread*; the PR
diff is the GitHub one after push).

Reopen thread: session-lifecycle `resume` with stored `acpSessionId` +
**worktree cwd** (not `repoRoot`).

## Polling vs webhooks for "checks running"

GitHub's REST guidance is "subscribe to webhooks instead of polling"
([best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)).
That is aimed at **server** integrations. A personal desktop app cannot
be a webhook target:

- Payload URL cannot be `localhost` / `127.0.0.1`.
- Production GitHub Apps that want webhooks need a reachable HTTPS
  server. Smee/ngrok are for development, not for shipping JaBot.
- `gh webhook forward` is a dev tunnel, not a product architecture.

**Poll**, but cheaply:

| When | Interval |
|---|---|
| PR view focused and any linked PR has pending checks | 10–15s (matches `gh pr checks --watch` default) |
| App focused, no pending checks | 60s |
| App backgrounded, pending checks (for Inbox/badge) | 30–60s |
| App backgrounded, all green/merged | pause or 5–15 min |
| Laptop sleep | stop; resume on wake |

Use authenticated **conditional** REST (`If-None-Match` / ETag) when
hitting REST; `304` does not consume the primary rate limit. GraphQL has
no ETag story — keep the query small (linked PRs only). Authenticated
user budget is ~5,000 REST req/hour and ~5,000 GraphQL points/hour
([REST](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api),
[GraphQL](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api)).
A 15s poll of 10 PRs in one GraphQL document is fine.

Respect `x-ratelimit-remaining` / `x-ratelimit-reset`. Do not parallel-
stampede (GitHub asks for serial-ish traffic to avoid secondary limits).

Webhook-worthy events if we ever have a host server (remote-and-mobile):
`pull_request`, `check_suite`, `check_run`, `workflow_run`. Until then,
do not register repo webhooks as the user — noisy, 20-per-event cap, and
orgs disable OAuth-created hooks when the app is restricted.

## How we know the session opened PR #23

ACP does not have a "pull request opened" event. Linkage is an overlay,
like Inbox. Confidence, high to low:

### 1. Parse ACP `execute` stdout (primary, live)

Harness adapter already maps `tool_call_update` `kind: execute` to the
bash toolblock, with streamed text
([acp.md](../harness-integration/acp.md),
[ACP v1 tool-calls](https://agentclientprotocol.com/protocol/v1/tool-calls)).
Watch completed execute content (and chunks) for:

- `https://github.com/<owner>/<repo>/pull/<n>` (and `http://`, `.git`
  hosts, GHES host from the folder)
- `github.com/<owner>/<repo>/pull/<n>`
- GitHub compare URLs are **not** a PR yet; ignore until `/pull/<n>`

`gh pr create` success output **is that URL** (last line in the manual's
examples). Newer `gh` may grow `--json number,url` on create; parse JSON
if present, URL regex otherwise. Do not wait for `--json` to ship
everywhere.

Also match `gh pr create` in the **command** text so we know to run
fallback (2) even if stdout was truncated.

Terminal-embedded execute (`content.type: "terminal"`) still ends with
text we can scan, or we skip to fallback (2) at turn idle.

### 2. Post-turn / idle: `gh pr view` in the session cwd (authoritative)

On ACP idle / stop reason, from the **worktree cwd**:

```bash
gh pr view --json number,url,title,state,isDraft,headRefName
```

No argument ⇒ PR for the **current branch**
([gh pr view](https://cli.github.com/manual/gh_pr_view)). This catches
PRs opened in the GitHub UI, via the `hub` CLI, or via an MCP GitHub
server whose stdout we failed to parse.

If that fails: `git branch --show-current` then
`gh pr list --head "$BRANCH" --json number,url --limit 1`.

### 3. Git remote + GraphQL `headRefName` (reconciliation)

If `gh` is confused (forks, `:owner:branch` heads): resolve
`owner/repo` from `origin`, query
`repository.pullRequests(headRefName: $branch, states: OPEN)`. GitHub
also exposes `ref.associatedPullRequests`.

`git status -sb` / `@{upstream}` only prove a **branch was pushed**, not
that a PR exists. Do not treat "pushed to origin" as linkage.

### 4. Agent chat text (display only)

"Opened PR #23" in a bot bubble is **not** enough to write the store.
Agents hallucinate numbers. Use it to trigger fallback (2), then confirm.

### 5. GitHub webhooks / events feed

Skip for linkage. Same public-URL problem. `GET /repos/…/events` is
another poll with worse signal.

### What to persist

```text
threadId
pr { host, owner, repo, number, url }
detectedAt, detectedVia     # stdout | gh-pr-view | head-list
```

Inbox copy can then be deterministic: `PR #${number} opened`. Button
opens `url` (or in-app PR row). Session-lifecycle owns *when* the card
appears; this topic owns *the PR fields on the card*.

If the session opens a **second** PR (stacked / follow-up): keep a list,
surface the latest on the Inbox card, show all on the PR view. Rare;
don't overbuild.

Drafts: `gh pr create --draft` still prints a `/pull/n` URL. Prototype
has a Drafts tab / DRAFT pill — `isDraft` from the API, not a separate
detector.

## Mapping prototype sections

| Prototype | API |
|---|---|
| Needs Review | linked + `state: OPEN` + not draft + checks not pending (or reviewDecision ≠ APPROVED — keep it simple: open + checks green) |
| Checks Running | `statusCheckRollup.state` in `PENDING` / `EXPECTED`, or any context pending |
| Recently Merged | `state: MERGED` (or `gh pr list --state merged`) for linked PRs |
| Drafts tab | `isDraft` |
| Open · N | count of linked OPEN |

"from folded session" is session-lifecycle state, not GitHub.

## What we explicitly defer

- In-app merge conflict editor.
- Review comments / requested changes as a full GitHub review UI.
- Auto-merge policies beyond what `gh pr merge --auto` already does.
- Creating the PR from a JaBot button (Conductor Cmd-Shift-P). Nice;
  agents already create PRs. Host-side `createPullRequest` can wait.
- GitHub Projects / assignees on the card.
