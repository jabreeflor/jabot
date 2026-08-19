# Adapter design (JaBot)

Concrete shape for the harness layer, given [findings.md](findings.md).
This is research, not a build spec — enough to open the blocked issues.

## Decision

```
JaBot UI  ←normalized events→  Host / session supervisor  ←ACP stdio→  adapter process
                                                                  │
                    ┌───────────────┬────────────────┬────────────┴────────┐
                    ▼               ▼                ▼                     ▼
            claude-agent-acp   codex-acp          pi-acp            user command
```

- **One client implementation** (ACP TypeScript or Rust SDK).
- **Shipped commands** for the three New Chat cards.
- **Custom** = user-supplied `command` + `args` + `env` that must speak ACP.
- Native SDKs (`query()`, `codex app-server`, `pi --mode rpc`) are
  documented fallbacks, not the UI's dialect.

Do not scrape TUIs. Do not invent a fourth JSON event format unless ACP is
missing a field we cannot live without — then extend via ACP `_meta`, not a
parallel bus.

## Adapter interface (host-side)

Logical trait. Names are illustrative.

```text
HarnessRuntime
  id, label, command, args, env
  probe() -> installed | missing(hint)
  spawn(cwd, extraEnv) -> AcpConnection

AcpConnection
  initialize()
  newSession(cwd, mcpServers) -> sessionId
  resumeSession(sessionId, cwd)
  loadSession(sessionId, cwd)          # replay into our renderer
  prompt(sessionId, content)
  cancel(sessionId)
  close(sessionId)
  onUpdate(handler)                    # session/update
  onPermission(handler) -> reply
  kill()                               # SIGTERM the subprocess
```

JaBot thread row stores:

```text
threadId          our uuid
harnessId         "claude" | "codex" | "pi" | custom id
acpSessionId      opaque string from session/new
nativeSessionRef  optional overlay (Claude uuid, Codex thread id, Pi JSONL path)
cwd
runtime           { command, args, env } snapshot so Custom is reproducible
state             active | folded | …  (session-lifecycle owns this)
```

`nativeSessionRef` exists so we can resume if the ACP adapter is swapped
or loses the mapping.

## Event model (what the chat renderer consumes)

Normalize **to ACP v1 `session/update`**, then to prototype kinds:

| ACP update | Prototype | Notes |
|---|---|---|
| user message chunk | `me` bubble | Also used on `session/load` replay |
| agent message chunk | `bot` bubble | Markdown |
| `tool_call_update` kind `read` | `▸ read` | title + path |
| `tool_call_update` kind `edit` | `▸ edit` / `▸ write` | attach diff content when present |
| `tool_call_update` kind `execute` | `▸ bash` | stream output; status running/ok/fail |
| plan | optional checklist / "step 3/7" in the header | prototype `status: running · step 3/7` |
| `session_info_update` / usage | footer / cost later | not MVP |
| idle `state_update` + stop reason | `sys` "Session finished" | Inbox resurface |
| `requires_action` | permission modal + Inbox "needs you" | |
| error / failed tool | toolblock fail + optional `sys` | |

Completion detection (feeds session-lifecycle):

1. Prefer ACP idle + stop reason (`end_turn`, `cancelled`, error).
2. Codex native: `turn/completed`.
3. Pi native: `agent_settled` (not `agent_end`).
4. Claude native: `result` message (`is_error`, `permission_denials`).

Do not use "stdout went quiet" as the primary signal. Idle-timeout is a
backstop only.

Errors must be structured (failed tool, auth, sandbox, cancelled). The
renderer already has `.ok` / `.run` classes; add a fail class.

## Permissions

One UI for all harnesses. ACP options become buttons:

| `kind` | Button |
|---|---|
| `allow_once` | Allow |
| `allow_always` | Always allow (this session / this pattern — we remember) |
| `reject_once` | Deny |
| `reject_always` | Never (session policy) |

`subject.type === "command"`: show the exact command + cwd, not just the
tool name. `tool_call`: show title + description; link to the pending
toolblock (`pending` = awaiting approval).

While a request is outstanding:

- Thread status = needs you / judgment call.
- Folded threads **must still deliver** the prompt (notification + Inbox
  card). ACP does not queue this for us; the host keeps the connection.
- On `session/cancel` or user Delete: reply `cancelled` to every pending
  request, then close.

Policy presets (later, not protocol):

- **Ask** — default. Every unmatched tool prompts.
- **Accept edits** — map to Claude `acceptEdits` / Codex workspace-write
  + auto file approvals if we have a native transport; via ACP, auto-select
  `allow_once` for `kind: edit` only.
- **Wait for Inbox** — still prompt for execute/delete; auto-allow reads.
  Unanswered execute while folded = resurface as judgment call, do not
  invent an answer.

Pi may not prompt unless we use `pi-acp` or a Pi extension. Document that
the Pi card is less gated until that adapter is verified.

## Custom harness

Minimal contract (copy Buzz, tighten the prototype copy):

**The binary must speak ACP over stdio.** Command + args + env. Not "any
TUI."

Suggested config (New Chat → Custom, or Settings):

```json
{
  "id": "amp",
  "label": "Amp",
  "command": "amp-acp",
  "args": [],
  "env": {},
  "installHint": "npm i -g @sourcegraph/amp-acp"
}
```

Probe: spawn, send `initialize`, expect a protocol version. If it fails,
show `installHint`. Do not attempt ANSI parsing as a fallback.

Optional later: "raw PTY" escape hatch as a fourth runtime type, rendered
as a terminal view, **not** as JaBot bubbles. That is app-shell, not
adapter.

## Session identity (brief Q7)

| Harness | Create | Name | Resume | Kill |
|---|---|---|---|---|
| **ACP (all)** | `session/new` → `sessionId` | session info / config options if advertised | `session/resume` or `session/load` | `session/close` + SIGTERM |
| Claude native | `query()`; read `session_id` | CLI `--name` | `resume: id` (same cwd) | drop process |
| Codex native | `thread/start` → `thread.id` | `thread.name` | `thread/resume` | `turn/interrupt` then drop app-server **or** keep server, archive thread |
| Pi native | RPC start; `get_state.sessionId` + `sessionFile` | `set_session_name` / `--name` | `switch_session` to JSONL | `abort` + process exit |

JaBot names are ours (thread title). Push through to the harness when a
name RPC exists so the user's `claude` / `codex resume` / `pi -r` lists
stay recognizable.

**Keep-alive vs checkpoint** (handoff to session-lifecycle):

- ACP session is the conversation; the **process** is the live agent loop.
- Folded + still working ⇒ keep the subprocess (Buzz-style supervisor).
- Folded + idle / laptop sleep ⇒ process may die; resume from
  `sessionId` + cwd. Claude and Codex persist on disk; Pi JSONL does too.
- App restart: reconnect with `session/resume` if the adapter process is
  gone. Do not require the original PID.

## Shipped runtime table (MVP)

| Card | Command (verify at implement time) | Notes |
|---|---|---|
| Claude Code | `claude-agent-acp` or `claude-code-acp` | Env: existing Claude login / `ANTHROPIC_API_KEY` |
| Codex | `codex-acp` | Env: existing `codex login` |
| Pi | `npx -y pi-acp` | Requires `pi` on PATH |
| Custom | user | ACP handshake required |

PATH-probe like Buzz tier 2. Missing binary → install hint, not a crash
on Start session.

## What we explicitly defer

- ACP v2 as a requirement (speak v1; tolerate v2 if negotiated).
- ACP HTTP/WebSocket remote (draft) — host can tunnel later.
- Per-harness native transports except as escape hatches.
- Rendering raw terminals (ACP display-only terminal is there when we
  need it).
- Installing harnesses for the user (Claude/Codex/Pi installers).

## Suggested first issues (from the brief)

1. ACP client in the host + connection supervisor (spawn, logs, kill).
2. Runtime catalog: three builtins + custom JSON schema.
3. Map `session/update` → chat transcript components (bubbles, toolblocks).
4. Permission modal implementing `session/request_permission`.
5. Persist `acpSessionId` + `cwd` + harness id (data-and-persistence).
6. Fold keeps the connection; idle stop-reason emits an Inbox item
   (session-lifecycle).
