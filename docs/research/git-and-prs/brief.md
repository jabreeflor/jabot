# Git & Pull Requests

Code threads live in folders (projects), run against real repos, and feed a
Pull Requests view (needs review / checks running / recently merged).

Mostly independent; light dependency on
[session-lifecycle](../session-lifecycle/brief.md) (a folded thread that opens
a PR must link the two).

**Findings (2026-08):** questions below are answered in
[findings.md](findings.md). Deep dives: [worktrees.md](worktrees.md),
[folders-and-auth.md](folders-and-auth.md), [pr-linkage.md](pr-linkage.md).
Headline: JaBot-owned worktrees as ACP cwd; reuse `gh` for GitHub-only MVP;
poll GraphQL; detect PRs from execute stdout + `gh pr view`.

## Questions to answer

1. **Isolation per thread** — do concurrent threads on one repo each get a git
   worktree? Who creates/cleans them? What do Claude Code / Codex do natively
   about worktrees, and do we manage it ourselves instead?
2. **Folders = repos?** — is a sidebar folder exactly one repo/directory, or a
   grouping? How does a user register one?
3. **PR data source** — GitHub only for MVP? `gh` CLI vs GraphQL API vs
   Octokit. Polling vs webhooks for check status ("checks running" needs
   live-ish updates).
4. **Auth** — reuse the user's existing `gh` login vs our own OAuth app.
5. **Thread ↔ PR linkage** — when a session opens PR #23, how do we know, so
   the Inbox card can say "PR #23 opened" and deep-link it?

## What this blocks (future issues)

- Folder/repo registration UI
- Worktree manager
- Pull Requests view (real data)
- Inbox cards that link to PRs
