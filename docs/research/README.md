# JaBot Research

Research needed before we open build issues. Each topic has its own folder with a
`brief.md` (questions to answer) and `findings.md` (answers). Deep dives are more
`.md` files in the same folder. When a topic's questions are answered, we turn
its "What this unblocks" list into GitHub issues with dependencies flagged.

**Numbered topics #1–#7 plus cross-cutting setup-porting have findings in
(August 2026).** Headline decisions are below. The three product forks
(host process, fold/run/Inbox, what is a bot) are **settled** in
[`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md). How those
relate to setup-porting is in [tensions](#tensions-with-setup-porting).

## Topics, in dependency order

| # | Topic | Findings | Headline |
|---|-------|----------|----------|
| 1 | [harness-integration](harness-integration/brief.md) | [findings](harness-integration/findings.md) | Speak **ACP**. Do not PTY-wrap TUIs. |
| — | [setup-porting](setup-porting/brief.md) | [findings](setup-porting/findings.md) | Prior art from OpenClaw, Hermes Agent, and Buzz. Cross-cuts #1–#7: daemon split, crew scopes, Inbox vs runs, catalog, pairing. |
| 2 | [session-lifecycle](session-lifecycle/brief.md) | [findings](session-lifecycle/findings.md) | User-space supervisor keeps the ACP process while folded work is in flight; idle / sleep / crash **resume** from `sessionId` + `cwd`. Not launchd. |
| 3 | [app-shell](app-shell/brief.md) | [findings](app-shell/findings.md) | macOS-only **Tauri 2 + React 19**; Rust host owns ACP subprocesses. Hide-to-Dock, no PTY for MVP. |
| 4 | [bot-crew](bot-crew/brief.md) | [findings](bot-crew/findings.md) | **Every bot is an ACP harness session** (Buzz-style catalog + per-bot harness). Chief hands off to worker threads. Findings' two-runtime split is superseded by [#6](../decisions/issues-4-6.md). |
| 5 | [git-and-prs](git-and-prs/brief.md) | [findings](git-and-prs/findings.md) | Folder = one repo. **Host-owned worktree** per thread as ACP `cwd`. Reuse `gh`; GitHub-only; poll GraphQL. |
| 6 | [data-and-persistence](data-and-persistence/brief.md) | [findings](data-and-persistence/findings.md) | **SQLite WAL** as source of truth; ACP overlay (not harness-log mirrors); secrets in the OS keychain. |
| 7 | [remote-and-mobile](remote-and-mobile/brief.md) | [findings](remote-and-mobile/findings.md) | **Logical client/host split** from day one. JaBot-owned host protocol. Pairing + thin mobile Inbox are MVP2. |

## Locked stack (what to scaffold)

```
WKWebView (React 19 + Vite)
        │  host API (Tauri IPC now; Unix socket / WebSocket later)
        ▼
Rust host  —  SQLite, crew, Inbox overlay, supervisor, MCP catalog
        │  ACP stdio
        ▼
claude-agent-acp / codex-acp / pi-acp / custom
```

- **Shell:** Tauri 2, macOS-only, Developer ID + notarize (not App Store).
- **Harness:** ACP v1 client in the host. One adapter subprocess per live thread.
- **Threads:** UI overlay `active → folded → resurfaced → archived` plus a `runs` table. Inbox is a projection of run events. Wait for Inbox is a permission policy, not a fifth state.
- **Code isolation:** `git worktree` per concurrent **code** thread; path is ACP `cwd`.
- **Crew:** every bot (Chief, Code, Inbox Mgr, templates) is an ACP harness session. Buzz three-tier catalog; per-bot `harness_id`; custom JSON. Isolation is credentials + memory, not a second runtime.
- **Store:** one SQLite file, single-writer host; Keychain for tokens.
- **Remote:** same host API over LAN/Tailscale later. No JaBot relay. No ACP-over-HTTP (still draft).

## Settled forks (was: physical host)

[app-shell](app-shell/process-architecture.md) and
[session-lifecycle](session-lifecycle/keep-alive.md) wanted the host **in-process**
with the Tauri binary (hide window ≠ quit; Cmd-Q resumes from disk).
[remote-and-mobile](remote-and-mobile/architecture.md) wanted a **separate host
daemon in MVP1**. [setup-porting](setup-porting/findings.md) wanted a **durable
host daemon** even after the UI quits.

**Settled in [#4](../decisions/issues-4-6.md):** logical split in MVP1 (webview
talks only to a host API). Host stays in the Tauri binary; hide-to-Dock.
Extract `jabot-host` when a second client exists. Do not install launchd.
Window close ≠ host quit (hide). Cmd-Q does quit; resume from disk.

The UI never owns ACP stdio. The host API is socket-shaped so it can detach.

## Tensions with setup-porting

[#2–#7 findings](.) were written before
[setup-porting](setup-porting/findings.md) landed. The three forks are now
settled in [`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md):

| Topic | #2–#7 findings | setup-porting | Settled |
|---|---|---|---|
| Host process | In-process Tauri host + hide-to-Dock; sidecar later | Durable host **daemon** from day one; UI quit ≠ host quit | **#4:** in-process + hide-to-Dock. Quit resumes. Sidecar when a second client exists. Folded work must not die on *window close*. |
| Fold / Inbox | Thread overlay; Inbox is a query on `threads.state` | **Three stores**: fold, run, Inbox projection | **#5:** overlay **and** a `runs` table. Inbox is persist-then-notify from run events. Still sleeping = folded threads. |
| What is a bot? | Code = ACP; others = prompt + MCP | Isolated **scope**; never share auth/session stores | **#6:** every bot is an ACP harness (Buzz catalog). Scope = persona + tools + memory + per-bot harness. Shared OAuth grants with allowlists; no shared harness session stores. |

Aligned already: ACP as the harness seam, persist-then-notify, never auto-allow
execute because a thread is folded, SQLite + OS keychain, pairing without a
JaBot account/relay.

## File map

```
docs/decisions/
  issues-4-6.md                      ← settled forks (#4 host, #5 fold/run/Inbox, #6 bot=harness)
docs/research/
  README.md                          ← you are here
  harness-integration/
    brief.md  findings.md  acp.md  adapter-design.md
    claude-code.md  codex.md  pi.md  buzz.md
  setup-porting/
    brief.md  findings.md  openclaw.md  hermes.md  buzz.md
  session-lifecycle/
    brief.md  findings.md  state-machine.md  keep-alive.md  resurface.md
  app-shell/
    brief.md  findings.md  electron-vs-tauri.md  ui-stack.md  process-architecture.md
  bot-crew/
    brief.md  findings.md  bot-model.md  routing.md  mcp-and-tools.md
    templates-memory-schedules.md
  git-and-prs/
    brief.md  findings.md  worktrees.md  folders-and-auth.md  pr-linkage.md
  data-and-persistence/
    brief.md  findings.md  store.md  schema.md  secrets-and-sync.md
  remote-and-mobile/
    brief.md  findings.md  architecture.md  protocol-and-reach.md
    pairing-security-mobile.md
```

## Product source of truth

`prototypes/jabot-classic.html` is the MVP prototype. Concepts it defines:
Chief of Staff bot, crew of template bots, harness selection at new-chat time
(Claude Code / Codex / Pi / Custom), folders of code threads, Disappearing
Threads (fold away → Inbox), Pull Requests view, right-click actions
(Wait for Inbox / Archive / Delete).
