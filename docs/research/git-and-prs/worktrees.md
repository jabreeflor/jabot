# Worktrees

How JaBot should isolate concurrent code threads on one repo. Complements
[findings.md](findings.md) Q1.

## Decision

**JaBot owns the worktree.** For any code thread that can run in parallel
with another thread (or with the user) on the same folder, the host runs
`git worktree add`, then `session/new` with that absolute path as `cwd`.
Do not pass Claude `--worktree` or Codex `--worktree` through the ACP
adapter. Do not ask the agent to create the worktree.

ACP already requires an absolute `cwd`
([adapter-design.md](../harness-integration/adapter-design.md)). Isolation
is a host concern, same as "JaBot does not PTY-wrap."

## Why worktrees, not shared checkout or extra clones

Git lets one repository have a main working tree plus linked working trees.
Each linked tree has its own files, index, and `HEAD`, and shares objects
and remotes ([git-worktree](https://git-scm.com/docs/git-worktree)).

| Approach | Verdict for JaBot |
|---|---|
| All threads share the registered folder | Collisions. Two agents edit the same files; `git checkout` fights the user. Prototype folders already show **two** jabot-app threads at once (Auth migration running, Sidebar overflow done). |
| `git clone` per thread | Slow, doubles disk for `.git`, remotes drift. Worktrees exist to avoid this. |
| Harness-native `--worktree` | Different roots, branch rules, and cleanup per product. ACP adapters will not expose those flags cleanly. Pi has none. |
| Host-owned worktree + ACP `cwd` | One manager, every harness, matches Conductor / Cursor / Codex desktop. |

Git hard rule, called out by both Codex and Conductor: **a branch can be
checked out in only one worktree**. Give each thread its own branch
(`jabot/<short-id>`). Never reuse `main` as the thread checkout.

## What Claude Code does natively

Docs: [code.claude.com/docs/en/worktrees](https://code.claude.com/docs/en/worktrees).

- `claude --worktree <name>` (or `-w`) creates
  `.claude/worktrees/<name>/` on a new branch `worktree-<name>`, defaulting
  to the repo default branch (`worktree.baseRef: "fresh"`).
- Desktop app: **every new session gets a worktree automatically**.
- Subagents: `isolation: worktree` in frontmatter.
- Cleanup: interactive exit prompts; unnamed + clean → remove tree **and
  branch**. Subagent trees are swept after `cleanupPeriodDays` if they hold
  no work. Live trees are `git worktree lock`ed.
- Resume is cwd-scoped (already noted in
  [claude-code.md](../harness-integration/claude-code.md)): lookup includes
  git worktrees; resume must use the **same cwd**.
- `.worktreeinclude` copies gitignored files (`.env`, …).
- Can branch from a PR: `claude --worktree "#1234"` fetches
  `pull/<n>/head` (GitHub) or `merge-requests/<n>/head` (GitLab).

**Do not use this as JaBot's manager.** It writes under the user's repo
(must be gitignored), names branches `worktree-*`, and the ACP spawn is
`claude-agent-acp` / `claude-code-acp` with a cwd we already chose — adding
`--worktree` would create a *second* tree and ignore our cwd.

If a user already has Claude-created trees, leave them alone. JaBot trees
live elsewhere.

## What Codex does natively

Two layers:

1. **ChatGPT desktop app** —
   [developers.openai.com/codex/app/worktrees](https://developers.openai.com/codex/app/worktrees).
   Managed trees under `$CODEX_HOME/worktrees`, default **detached HEAD**
   (so many trees can share a starting commit without branch clashes).
   Handoff moves a chat between Local and Worktree. Auto-keeps ~15 managed
   trees; permanent trees are not auto-deleted. `.worktreeinclude` for
   ignored files. Setup scripts in `.codex`.
2. **CLI** — [openai/codex#21435](https://github.com/openai/codex/pull/21435)
   adds `--worktree`, `codex worktree list|path|remove|prune`, TUI
   `/worktree`. Sibling paths, Codex ownership metadata, refuses to remove
   trees it does not own. Older secondary write-ups still say "no flag";
   treat the PR as the CLI direction, the app docs as shipping.

ACP path is `codex-acp` with `thread/start { cwd, … }`. Same rule: **we
set cwd**; we do not invoke Codex's worktree CLI.

## Cursor and Conductor (prior art)

**Cursor** ([docs](https://cursor.com/docs/configuration/worktrees)): Agents
Window / `/worktree` creates an isolated checkout; `.cursor/worktrees.json`
runs setup (`npm ci`, copy `$ROOT_WORKTREE_PATH/.env`); machine-wide cap
(default 25) and interval cleanup. `/apply-worktree` / `/delete-worktree`.

**Conductor** ([git-worktrees](https://www.conductor.build/docs/concepts/git-worktrees)):
the product JaBot most resembles. Workspace = worktree + branch + chat +
diff + checks + PR. Trees under `~/conductor/workspaces/<repo>/<name>`.
Setup scripts + files-to-copy. Archive after merge. Isolation is **not** a
security boundary.

Steal Conductor's shape: folder = repo root; thread = workspace; PR view
lists workspaces that opened PRs.

## Pi and Custom

Pi has no worktree flag. Harness research already says sandbox at the host
(OS, worktree) rather than trusting Pi
([pi.md](../harness-integration/pi.md)). Custom ACP binaries only see
`cwd`. Host worktrees cover both.

## Lifecycle JaBot should implement

```text
register folder (repoRoot)
        │
new code thread
        │
git worktree add --lock --reason "jabot <threadId>" \
    -b jabot/<short>  <jabotWorktreeRoot>/<folderId>/<threadId>  <baseRef>
        │
optional: copy .worktreeinclude / folder "files to copy"
optional: run folder setup script (npm ci, …)  — like Cursor/Conductor
        │
ACP session/new(cwd = worktree path)
        │
… agent commits, pushes, gh pr create …
        │
on Delete, or Archive after merged:
    git worktree unlock
    git worktree remove [--force if user confirmed]
    git worktree prune
    optionally delete local branch if fully merged
```

**Create**

- Path: host-owned, **outside** the user's checkout — e.g.
  `~/.jabot/worktrees/<folderId>/<threadId>`. Same idea as Codex
  `$CODEX_HOME/worktrees` and Conductor `~/conductor/workspaces/…`. Do not
  drop trees into `.claude/worktrees` or the repo root.
- Branch: `jabot/<short-id>` from `origin/<default>` (fetch if stale), not
  from a dirty user `HEAD`, unless the New Chat modal says "from current
  branch".
- `git worktree add --lock` while the session process is alive (Claude does
  this; it prevents a prune/remove race).
- Parse `git worktree list --porcelain -z` for recovery after crash.

**Opt-out: "use this checkout"**

One thread at a time may use `repoRoot` as cwd — the user's own dirty tree.
Default for **New thread in folder** while another thread is active:
worktree. Default when nothing else is running: still worktree. The
prototype is a multi-thread UI; sharing the main checkout is the footgun
Conductor exists to remove. Offer "work in my current folder" as an
advanced toggle, not the default.

**Clean**

| Event | Action |
|---|---|
| Thread Delete | Unlock + `remove --force` after confirm if dirty/unpushed. |
| PR merged + user Archives | Remove worktree; leave the merged branch deletion to `gh pr merge -d` or a later sweep. |
| Session still folded / sleeping | **Keep** the worktree. Resume needs the same cwd. |
| Crash / leftover directory | `git worktree prune`; list porcelain; offer Repair in folder settings. |
| Disk cap (later) | Cursor-style max count. Not MVP. Never auto-delete a tree with unpushed commits. |

`git worktree remove` refuses dirty trees unless `--force`. The main
worktree cannot be removed. If the user `rm -rf`'d the directory, `prune`
drops stale `$GIT_DIR/worktrees` metadata.

Do not delete the user's `main` checkout. Do not `git clean` their registered
folder as part of thread cleanup.

## Setup (gitignore is the real pain)

A new worktree is a **tracked-files** checkout. `node_modules`, `.env`,
and local DBs are missing. Every mature product copies a small ignored
set and runs a setup script:

- Claude / Codex / Conductor: `.worktreeinclude`
- Cursor: `.cursor/worktrees.json` + `$ROOT_WORKTREE_PATH`
- Conductor: Files to copy + setup scripts + `CONDUCTOR_PORT`

MVP: folder setting "copy these paths from repoRoot" (default suggest
`.env`, `.env.local` if present) + optional setup command. Honor
`.worktreeinclude` in the repo if it exists so we stay compatible with
Claude/Codex/Conductor checkouts of the same project.

Do not symlink `node_modules` from the main tree (Cursor explicitly warns).

## What we explicitly defer

- Best-of-N / extra worktrees per prompt (Cursor `/best-of-n`).
- Codex-style detached HEAD + Handoff into the user's IDE checkout.
- Letting Claude `EnterWorktree` move cwd out from under us — if the
  adapter surfaces it, deny or re-bind our stored cwd.
- Non-git VCS (Claude WorktreeCreate hooks). Folders that are not git
  repos: still a cwd, no worktree, no PR view for that folder.
