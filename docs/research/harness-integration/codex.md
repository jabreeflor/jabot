# Codex

OpenAI's coding agent (Rust CLI, open source). Same rule as Claude: do not
PTY-wrap the TUI. Codex actually has the richest first-party **client
protocol** of the three — `codex app-server` — plus a community ACP adapter.

## Integration modes

| Mode | How | Use in JaBot |
|---|---|---|
| **ACP adapter (preferred for a unified layer)** | `codex-acp` / `@zed-industries/codex-acp`. Buzz documents this path. | Same ACP client as Claude/Pi. |
| **App Server (deepest official API)** | `codex app-server` — bidirectional JSON-RPC, same API the VS Code extension uses. | Use if ACP adapter is lossy (approvals, thread archive, steer). |
| **Headless exec** | `codex exec` / `codex e`. TypeScript SDK wraps this. | CI / one-shot. Exits when the task completes. Approvals must be pre-set or the run fails. |
| **Interactive TUI** | `codex` with no subcommand. | Do not wrap. |

OpenAI's own guidance: app-server for rich clients; SDK / `codex exec` for
automation. JaBot is a rich client.

## `codex exec` (not enough)

```bash
codex exec --sandbox workspace-write --json "Fix sidebar overflow"
```

- `--json`: newline-delimited events, one per state change.
- Resume: `codex exec resume <id>` or `--last` (cwd-scoped; `--all` searches
  everywhere).
- Default sandbox is read-only. Automation uses `--sandbox workspace-write`
  or `danger-full-access`. `--full-auto` is deprecated.
- Interactive approval prompts are **not** supported in exec; MCP tool
  calls that need a prompt fail unless policy auto-allows.

Fine for "run this and tell Inbox when `turn/completed`", bad for a chat
that must answer "allow this edit?".

## App Server (the real protocol)

Docs: [developers.openai.com/codex/app-server](https://developers.openai.com/codex/app-server).
Implementation: `openai/codex/codex-rs/app-server`.

Transports:

- `stdio` (default, `--listen stdio://`) — JSONL, no `"jsonrpc":"2.0"` on
  the wire (MCP-style).
- WebSocket (`--listen ws://IP:PORT`) — experimental. Local / SSH-forward
  only unless you set `--ws-auth`.
- Unix socket (`unix://`).
- Remote TUI: `codex --remote ws://host:port` talks to an app-server. This
  is Codex's own remote story, not ACP.

Handshake: `initialize` (clientInfo) then `initialized`. Anything before
that is rejected.

### Primitives

- **Thread** — durable conversation (`~/.codex/sessions`). Create, resume,
  fork, archive, list, read, rollback. Has `id`, `status`, optional `name`.
- **Turn** — one user request + agent work. `turn/start`, `turn/steer`
  (inject while running), `turn/interrupt`.
- **Item** — user message, agent message, command execution, file change,
  MCP tool call, review, etc.

This maps cleanly onto JaBot: thread = sidebar row, turn = a user send,
items = bubbles + toolblocks.

### Driving a chat

1. `thread/start` `{ model, cwd, sandbox, approvalPolicy }` → `thread.id`.
2. `turn/start` `{ threadId, input: [{ type: "text", text: "…" }] }`.
3. Read notifications: `item/started`, `item/agentMessage/delta`,
   `item/commandExecution/outputDelta`, `item/completed`, `turn/completed`.
4. Later: `thread/resume` with the stored id (config overrides allowed).
5. Fold analog: `thread/archive` moves JSONL into an archived directory
   (and tries to archive descendant threads). `thread/unarchive` restores.

`turn/steer` is the "user types while it's working" path — the prototype
composer stays open on a running session.

Generate matching types from the installed CLI:

```bash
codex app-server generate-ts --out ./schemas
```

Pin the Codex version; the schema is per-release.

### Approvals (server → client requests)

Not notifications. The turn pauses until we reply.

| Request | Decisions |
|---|---|
| `item/commandExecution/requestApproval` | `accept`, `acceptForSession`, `decline`, `cancel`, or accept-with-execpolicy-amendment |
| `item/fileChange/requestApproval` | `accept`, `acceptForSession`, `decline`, `cancel` |
| `item/permissions/requestApproval` | grant a **subset** of requested network/fs perms; `scope`: `session` or `turn` |
| `tool/requestUserInput` | MCP/app tools; Accept / Decline / Cancel. Optional `autoResolutionMs`. |
| `mcpServer/elicitation/request` | form or URL elicitation |

Order for a shell command: `item/started` (pending command) →
`requestApproval` → our UI → `serverRequest/resolved` → `item/completed`.

Network approvals can include `networkApprovalContext` (`host`, `protocol`)
and may batch by destination. Render those as "allow network to X", not as
a fake shell command.

`approvalPolicy` on `thread/start` (e.g. `never`, `unlessTrusted`) plus
sandbox (`workspaceWrite`, …) set how often these fire. JaBot should start
conservative (`unlessTrusted` / workspace-write) and let the user loosen.

### Errors and completion

`turn/completed` is the Inbox "done" signal. Error variants include
disconnect, too many failed attempts, sandbox, auth, etc. That is enough
for resurface triggers: done, failed, needs approval (`requires_action`
equivalent = outstanding server request).

## ACP adapter vs app-server

Buzz and Zed use **codex-acp**, not app-server, so a unified ACP layer
works. Cost: Codex-only features (thread archive as a first-class RPC,
`turn/steer`, experimental `dynamicTools`, permission profiles) may be
thinner through ACP.

Pragmatic split:

- MVP: ACP adapter, same as Claude and Pi.
- If permission or steer UX feels worse than the VS Code extension, add a
  **native Codex transport** behind the same JaBot event enum (see
  [adapter-design.md](adapter-design.md)). App-server is stable enough for
  that; keep it as a documented escape hatch, not the default.

Auth: reuse `codex login` / saved CLI auth. CI-style `OPENAI_API_KEY` /
`CODEX_API_KEY` is the fallback.
