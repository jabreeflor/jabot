# Keep-alive vs checkpoint

Locked from [adapter-design.md](../harness-integration/adapter-design.md):

- ACP session is the conversation; the **process** is the live agent loop.
- Folded + still working ⇒ keep the subprocess (Buzz-style supervisor).
- Folded + idle / laptop sleep ⇒ process may die; resume from
  `sessionId` + `cwd`.
- App restart: reconnect with `session/resume` if the adapter process is
  gone. Do not require the original PID.

This file is the how.

## Decision

```text
Fold (work in flight)
  → supervisor holds AcpConnection (stdio subprocess)
  → NSProcessInfo beginActivity(.userInitiatedAllowingIdleSystemSleep)
    so App Nap does not freeze the agent while the lid is open
  → do not session/close

Fold (already idle) or laptop actually sleeps / app quit / crash
  → persist overlay (sessionId, cwd, nativeSessionRef, awayLog)
  → process may freeze or die; that is OK
  → on wake / relaunch: if PID + stdio still healthy, reuse;
    else spawn adapter, initialize, session/resume (or session/load
    if we have no local transcript)
```

Do **not** checkpoint-and-kill a running turn just because the UI folded.
The agent is mid-tool-loop; Claude/Codex/Pi resume restores **history**,
not an in-flight bash. Killing mid-turn is a cancel.

Do **not** install a `launchd` KeepAlive agent in MVP. Launchd will not
keep a coding agent running through lid-close (the machine sleeps), it
will restart crashed jobs in a tight loop without our policy, and it
fights a clean quit (KeepAlive races). User-space supervisor in the
JaBot host is enough. Detached daemon is an [app-shell](../app-shell/brief.md)
question, not a session-lifecycle one.

## What each harness actually resumes

### ACP (all three cards)

| Verb | When |
|---|---|
| `session/new` | First message. Persist returned `sessionId`. |
| `session/prompt` | User send. v1 completion = RPC return + `stopReason`. v2 = prompt returns on **accept**; completion is idle `state_update`. |
| `session/cancel` | User stop / Delete. Then idle/`cancelled`. Reply `cancelled` to every outstanding `request_permission`. |
| `session/resume` | Process died or new adapter spawn. Restores context **without** replaying history. Capability `sessionCapabilities.resume` (v1); required on the v2 session surface. Pass the same `cwd` (absolute) and MCP list. |
| `session/load` | We need the agent to **replay** history into our renderer (no local transcript yet). Capability `loadSession`. v2 folded this into `session/resume` + `replayFrom: { type: "start" }`. Speak v1; if the adapter only has load, use load. |
| `session/close` | Archive / Delete / idle-evict. Cancels work then frees adapter-side resources. Capability `sessionCapabilities.close`. Buzz historically leaked process trees because it never closed — do not copy that. |
| `session/list` / `session/delete` | Discover / wipe harness history. Delete after close when advertised. |

One ACP connection can hold several sessions. MVP: **one adapter process
per live thread** is simpler to kill and matches how `claude-agent-acp`
materializes a CLI tree per session. Idle-evict closed sessions so we do
not pin GB of Claude trees the way [Buzz #2961](https://github.com/block/buzz/issues/2961)
does.

### Claude Code

Sessions are JSONL under `~/.claude/projects/…`. Conversation only — not
the filesystem ([SDK sessions](https://code.claude.com/docs/en/agent-sdk/sessions)).

| Mechanism | Use |
|---|---|
| SDK `resume: "<uuid>"` / CLI `claude --resume <id\|name>` | Specific thread. **This is our path.** Capture `session_id` from init/`result`. |
| SDK `continue: true` / `claude --continue` | Most recent session **in this cwd**. Unsafe once JaBot has many threads. |
| `forkSession: true` | Later; not fold. |
| `persistSession: false` | Never for JaBot threads we might fold. |

Resume must use the original `cwd` (lookup was cwd-scoped before CLI
v2.1.223; still the right invariant). Push `--name` / SDK `title` so the
user's `claude --resume` picker matches our sidebar.

`claude -p` is one process per turn. The ACP adapter / Agent SDK process
**is** the runtime. Folded working threads keep that process.

Native completion: `result` message (`subtype` success/error, `is_error`,
`permission_denials`). Map through the adapter to idle + stop reason.

### Codex

Sessions are JSONL rollouts under `~/.codex/sessions/YYYY/MM/DD/`.

| Mechanism | Use |
|---|---|
| App-server `thread/resume` `{ thread_id }` | Official rich-client resume. Same `cwd` / approval overrides allowed. |
| `codex resume <id>` / `--last` | Interactive TUI. Do not wrap; we are not a TUI. `--last` is cwd-scoped unless `--all`. |
| `codex exec resume <id\|--last>` | Headless follow-up. Completes and exits. Fine for a one-shot, not for a folded chat that must answer permissions. |
| `turn/completed` | Native "this turn ended." `status`: `completed` \| `interrupted` \| `failed`. |

ACP card still goes through `codex-acp`. Store Codex `thread.id` in
`nativeSessionRef` so we can `thread/resume` if the adapter mapping is
lost.

`thread/archive` is Codex's own analog of our Archive. Do not call it on
fold — that moves files to an archived directory. Our Archive may call it
later as an overlay, not MVP.

### Pi

Sessions auto-save to
`~/.pi/agent/sessions/--<cwd-with-slashes-as-hyphens>--/<timestamp>_<uuid>.jsonl`.

| Mechanism | Use |
|---|---|
| RPC `get_state` → `sessionId` + `sessionFile` | Persist both. |
| RPC `switch_session` `{ sessionPath }` | Resume after process death. |
| `pi -c` / `pi --session <path\|id>` | TUI/CLI equivalents. |
| `pi --no-session` | Never for folded threads. |

Completion: **`agent_settled`**, not `agent_end`. `agent_end` fires per
low-level loop and may be followed by retry / compaction / queued
follow-up (`willRetry`). Inbox "done" = `agent_settled`. The `pi-acp`
adapter must translate that to ACP idle; verify at implement time
(uncertainty: adapter quality).

## Supervisor (Buzz-shaped, JaBot-owned)

Buzz desktop: tracks PIDs, logs, readiness; `buzz-acp` respawns a crashed
agent and persists channel→session bindings in a JSON sidecar
([PR #2633](https://github.com/block/buzz/pull/2633)). Heartbeats stay
ephemeral. The UI never talks to Claude/Codex/Pi directly.

Copy that seam:

```text
JaBot UI  ←overlay events→  Host supervisor  ←ACP stdio→  adapter
                                │
                                ├─ spawn / initialize / new or resume
                                ├─ hold connections for folded+running
                                ├─ permission RPC (even when UI folded)
                                ├─ idle-evict + session/close
                                └─ persist session map (not just RAM)
```

MVP: supervisor **is** the host process (Electron/Tauri main). Closing
the window hides UI; quitting the app drops processes and relies on
resume. That matches **Conductor local** workspaces: "If you close the
app or shut down your Mac, those sessions end"
([cloud FAQ](https://www.conductor.build/docs/cloud/faq)). Conductor only
keeps agents alive after lid-close by moving them to **cloud sandboxes**.
We are not building that for MVP.

Optional later (app-shell): a user-level daemon so the window can quit
and threads keep running on this Mac. Still not launchd KeepAlive — a
single JaBot host started on login (`KeepAlive: false`, `RunAtLoad` if
we ever want it) that **we** supervise.

### Idle eviction

Buzz never evicts idle ACP sessions; each Claude session pins a process
tree. We must:

1. On `idle` + `end_turn` and `uiState != active`: eligible after a
   short grace (e.g. keep warm 2 minutes in case the user reopens).
2. Then `session/close` + drop the subprocess. Overlay stays `folded`
   only if we have not yet resurfaced; normally we already resurfaced
   `done` and can close.
3. Cap concurrent live processes (hard ceiling). Folded+running never
   evicts until stuck/failed/cancel.

### Crash of the adapter

Supervisor sees EOF / non-zero exit.

- If `acpState` was `running` or `requires_action`: resurface `failed`
  (or `needs_you` if we know a permission was outstanding — copy:
  "the agent quit while waiting on you"). Do not silent-respawn a
  mid-turn process; the user should see it.
- If `idle`: respawn on next prompt / on reopen via `session/resume`.
  No Inbox noise.

Uncertainty: some adapters crash-loop on bad env. Bound respawn (once)
and then `failed`.

## Crash and sleep {#crash-and-sleep}

| Event | Live process | Overlay | What we do |
|---|---|---|---|
| App Nap (lid open, window backgrounded) | At risk of throttling | Intact | `beginActivity(.userInitiatedAllowingIdleSystemSleep)` while any thread is `running`. End the activity when none are. Do **not** use `.userInitiated` (prevents idle **system** sleep) — the user asked the laptop to sleep, honor it. |
| Lid close / system sleep | Frozen; may be killed | Intact | On `didWake`: ping stdio (ACP initialize already done — send a cheap no-op or rely on next `session/update`). If dead → `session/resume`. If the thread was `running`, resurface `stuck` (we cannot know if the tool finished). |
| App restart | Dead | Disk | Spawn adapter, `session/resume` with stored `sessionId` + `cwd`. If `uiState == folded` and last `acpState == running`, resurface `stuck` ("interrupted by restart"). If last was `idle`, stay sleeping or already-done. |
| Machine reboot | Dead | Disk | Same as app restart. |
| Adapter crash | Dead | Disk | See above. |

`NSUserNotification` is deprecated and crashes on newer macOS. Process
lifetime APIs we want are `NSProcessInfo` activities, not notifications.

`caffeinate` / preventing system sleep for a 40-minute migration: only
if we add an explicit "keep Mac awake while this thread runs" toggle.
Default off. The prototype's 40-minute auth migration assumes the laptop
stays open; we should say that in the fold toast, not fight Energy Saver.

## Resume recipe (implement this)

1. Spawn the same `runtime` snapshot (command/args/env).
2. `initialize`. If handshake fails → thread `failed`, show install hint.
3. If adapter advertises `resume`: `session/resume { sessionId, cwd, mcpServers }`.
4. Else if `loadSession`: `session/load` (replay into renderer **only** if
   our overlay transcript is empty; otherwise resume-without-replay and
   keep our JSON).
5. Else: native fallback — Claude `resume: id`, Codex `thread/resume`,
   Pi `switch_session`. Last resort. Should be rare if we shipped adapters.
6. Do not `session/new`. That orphans the conversation.

cwd mismatch: refuse and resurface `failed` ("folder missing"). Do not
silently `session/new` in a different directory.

## What we explicitly defer

- File checkpointing / revert of the working tree (Claude has a separate
  file-checkpointing API; Codex rollback; git-and-prs owns worktrees).
- Conductor-style cloud sandboxes so lid-close does not stop the agent.
- launchd Login Item for a detached host (app-shell).
- Sharing one adapter process across many sessions (memory win, kill
  complexity). Revisit if process count becomes a problem; start with
  1:1 and idle-evict.
