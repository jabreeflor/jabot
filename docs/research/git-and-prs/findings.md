# Git & Pull Requests — Findings

Researched August 2026 against Git, Claude Code, Codex, Cursor, Conductor,
GitHub CLI/API, and the JaBot prototype. This file answers the five questions
in [`brief.md`](brief.md). Deep dives live in sibling files.

Locked from [harness-integration](../harness-integration/findings.md):
each ACP session has a **cwd**; JaBot does **not** PTY-wrap.

**Recommendation in one sentence:** Treat a sidebar folder as one local git
repo, have JaBot create a host-owned worktree per concurrent code thread and
pass that path as ACP `cwd`, reuse the user's `gh` login for GitHub-only MVP,
and detect "PR #23 opened" by watching ACP `execute` stdout plus a post-turn
`gh pr view` in that cwd — poll GraphQL for checks; skip webhooks until we
have a server.

| Question | Short answer | Detail |
|---|---|---|
| 1. Isolation per thread | Yes: one git worktree per concurrent thread on a repo. JaBot creates and cleans them. Do not rely on Claude/Codex native `--worktree`. | [worktrees.md](worktrees.md) |
| 2. Folders = repos? | Yes for MVP: a folder is one registered local directory (almost always a git repo). Not a grouping of many repos. | [folders-and-auth.md](folders-and-auth.md) |
| 3. PR data source | GitHub only. Octokit GraphQL (token from `gh`) for the PR view. Poll, don't webhook. | [pr-linkage.md](pr-linkage.md) |
| 4. Auth | Reuse `gh auth login` / `gh auth token`. Do not ship a JaBot GitHub App for MVP. | [folders-and-auth.md](folders-and-auth.md#auth) |
| 5. Thread ↔ PR linkage | Layered: parse ACP execute stdout, then `gh pr view` in the session cwd, then `head` branch lookup. Persist `{owner, repo, number, url}` on the thread. | [pr-linkage.md](pr-linkage.md) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Folder/repo registration UI** — pick a local path; store `folderId`,
   display name, `repoRoot`, `origin` (`owner/repo` or unknown), optional
   setup script / files-to-copy. See [folders-and-auth.md](folders-and-auth.md).
2. **Worktree manager** — `git worktree add -b jabot/<thread> …`, lock while
   the session is live, remove on Delete / post-merge Archive, prune stale
   metadata. Pass the worktree path as ACP `session/new` `cwd`. See
   [worktrees.md](worktrees.md).
3. **Pull Requests view (real data)** — GraphQL list of PRs **JaBot linked**,
   not every PR in the repo. Tabs map to `OPEN` / `MERGED` / `isDraft`.
   "Checks running" from `statusCheckRollup`. Merge via API. View diff can
   be `gh pr view --web` for MVP.
4. **Inbox cards that link to PRs** — session-lifecycle resurface payload
   includes the stored PR link so the copy can say "PR #23 opened" and the
   primary button is "Open PR #23".

## Prototype note

`prototypes/jabot-classic.html` already sketches the product:

- Sidebar folders `JABOT-APP` / `GLOBNET-SYNC`, each with "New thread in …".
- Pull Requests view: **Needs Review**, **Checks Running**, **Recently
  Merged**; actions **Merge**, **View diff**, **Reopen thread**.
- Subtitle: "Opened by your coding sessions" — the list is **session-owned
  PRs**, not a clone of github.com/pulls.
- Inbox: `jabot-app coding session · … · PR #23 opened` with **Open PR #23**
  and **Reopen thread**.

Do not populate the PR view from `gh pr list` of the whole repo. That would
mix Dependabot and coworker PRs into a view the prototype presents as
session output.

## Sources

Primary docs, not secondary blogs, unless noted:

- Git: [git-scm.com/docs/git-worktree](https://git-scm.com/docs/git-worktree)
- Claude Code: [code.claude.com/docs/en/worktrees](https://code.claude.com/docs/en/worktrees)
- Codex: [developers.openai.com/codex/app/worktrees](https://developers.openai.com/codex/app/worktrees),
  [codex PR #21435](https://github.com/openai/codex/pull/21435) (CLI `--worktree`)
- Cursor: [cursor.com/docs/configuration/worktrees](https://cursor.com/docs/configuration/worktrees)
- Conductor: [conductor.build git-worktrees](https://www.conductor.build/docs/concepts/git-worktrees),
  [review-and-merge](https://www.conductor.build/docs/guides/review-and-merge)
- GitHub CLI: [gh pr list](https://cli.github.com/manual/gh_pr_list),
  [gh pr view](https://cli.github.com/manual/gh_pr_view),
  [gh pr checks](https://cli.github.com/manual/gh_pr_checks),
  [gh pr create](https://cli.github.com/manual/gh_pr_create),
  [gh pr merge](https://cli.github.com/manual/gh_pr_merge),
  [gh auth token](https://cli.github.com/manual/gh_auth_token)
- GitHub API: [GraphQL PullRequest](https://docs.github.com/en/graphql/reference/pulls),
  [REST best practices (avoid polling / use ETags)](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api),
  [webhooks: localhost not supported](https://docs.github.com/en/webhooks/testing-and-troubleshooting-webhooks/troubleshooting-webhooks),
  [Octokit](https://github.com/octokit/octokit.js)
- ACP: [v1 tool-calls](https://agentclientprotocol.com/protocol/v1/tool-calls)
  (`kind: execute` stdout)
- GitLab later: [glab](https://docs.gitlab.com/cli/)
