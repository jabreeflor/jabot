# Agent Client Protocol (ACP)

Zed's open standard (Apache-licensed) for editor ↔ coding-agent communication.
JSON-RPC, LSP-shaped: one protocol, many agents, many clients. Official site:
[agentclientprotocol.com](https://agentclientprotocol.com/).

This is the right default for JaBot. The protocol already models sessions,
streaming assistant text, tool calls with kinds, diffs, plans, slash commands,
permission prompts, cancel, resume, and (draft) remote transport.

## Why it covers us

JaBot is not an IDE. It is a chat-first client that wants the same things
ACP was designed to give editors:

| JaBot need | ACP primitive |
|---|---|
| Start a code thread in a folder | `session/new` with `cwd` |
| Stream assistant bubbles | `session/update` message chunks |
| Toolblocks (`read` / `edit` / `bash`) | `tool_call_update` with `kind`: `read`, `edit`, `execute`, … |
| Diffs in the thread | tool-call content `type: "diff"` |
| Permission chip in our UI | `session/request_permission` (bidirectional RPC) |
| Fold / cancel a running turn | `session/cancel` |
| Kill a session | `session/close` (capability) |
| Resume after app restart | `session/resume` (no replay) or `session/load` (replay history) |
| "Done" for Inbox | idle `state_update` with a stop reason |
| Custom harness | spawn any ACP stdio binary |
| MCP tools on the crew | `mcpServers` on `session/new` |

We do **not** need to invent a private event schema first. Speak ACP internally;
map ACP updates onto chat bubbles. Native harness SDKs (Claude Agent SDK, Codex
app-server, Pi RPC) become optional fallbacks or the thing an adapter already
wraps.

## Architecture (local)

1. Client (JaBot host) launches the agent as a **subprocess**.
2. JSON-RPC over **stdio**, newline-delimited. Agent stdout is ACP only;
   stderr is logs.
3. One connection can hold **several concurrent sessions**.
4. Agent → Client notifications stream UI updates.
5. Agent → Client **requests** (permissions, elicitation) pause work until we
   answer.

From the architecture doc: ACP assumes a trusted local editor talking to an
agent that may use the client's files and MCP servers. That matches a personal
desktop app.

## Lifecycle (v2 shape; v1 is similar)

Typical flow from the [v2 overview](https://agentclientprotocol.com/protocol/v2/overview):

1. Client → Agent: `initialize` (protocol version, capabilities).
2. Client → Agent: `auth/login` if the agent requires it.
3. Client → Agent: `session/new` **or** `session/resume`.
4. Client → Agent: `session/prompt` with the user message.
5. Agent → Client: `session/update` for accepted message, running state, text
   chunks, tool calls, plans.
6. Agent → Client: `session/request_permission` as needed. While blocked, agent
   **should** report `requires_action`.
7. Client → Agent: `session/cancel` to interrupt.
8. Agent → Client: idle `state_update` with a stop reason when foreground work
   ends.

`session/prompt` in v2 returns once the prompt is **accepted**. Completion is
signaled by updates, not by the RPC return. That is what Disappearing Threads
needs: we can fold the UI while the process keeps emitting updates.

## Tool calls and the prototype toolblock

ACP tool-call kinds map onto the prototype's `▸ read` / `▸ edit` / `▸ bash`:

| ACP `kind` | Prototype |
|---|---|
| `read` | `▸ read` |
| `edit` | `▸ edit` / `▸ write` |
| `execute` | `▸ bash` |
| `search` | grep / glob style toolblocks |
| `fetch` | network tools |
| `think` | optional, can hide or collapse |
| `delete` / `move` | edit-family |

Statuses: `pending` → `in_progress` → `completed` | `failed` | `cancelled`.
Pending is also used while input is still streaming or **awaiting approval**.

Streaming: `tool_call_content_chunk` appends; a later `tool_call_update` with
`content` replaces. Display-only terminals have their own `terminal_update` /
`terminal_output_chunk` (base64 byte chunks) — useful if we ever show a raw
terminal escape hatch ([app-shell](../app-shell/brief.md) question 4).

Diffs carry structured `changes` plus optional `git_patch` text. That is enough
for a "files touched" toolblock without scraping.

## Permissions

`session/request_permission` is a **request**, not a notification. The client
must reply.

v2 request shape (draft, but the UX is stable from v1):

- `title` (required), `description` (optional)
- `subject`: `tool_call` or `command` (`command` + absolute `cwd`)
- `options[]` with `optionId`, `name`, `kind`:
  - `allow_once` / `allow_always`
  - `reject_once` / `reject_always`

Client replies `{ outcome: { outcome: "selected", optionId } }` or
`{ outcome: { outcome: "cancelled" } }` if we cancelled the turn.

If the user hits cancel on the session, **every** outstanding permission
request must be answered with `cancelled`.

Clients may auto-allow/reject from settings. That is how "Wait for Inbox"
autonomy vs "judgment call" will work — policy in JaBot, not in each harness.

## Sessions

Stable v1 methods (all of these have been announced as stabilized):

| Method | Role |
|---|---|
| `session/new` | Create. Returns `sessionId`. `cwd` must be absolute. |
| `session/load` | Restore **and replay** history via `session/update`. Capability `loadSession`. |
| `session/resume` | Restore **without** replay. Capability `sessionCapabilities.resume`. |
| `session/close` | Cancel work + free resources. Capability `sessionCapabilities.close`. |
| `session/list` | Discover existing sessions. |
| `session/delete` | Remove from history. |
| `session/cancel` | Interrupt current prompt. |

JaBot should:

- Persist ACP `sessionId` + harness id + cwd as our thread key.
- On reopen: `session/resume` if we already have the transcript overlay;
  `session/load` if we need the agent to replay.
- On Delete: `session/close` then `session/delete` if advertised.
- On fold: leave the subprocess up; do not close the session.

`cwd` is the folder from the New Chat modal. `additionalDirectories` is
optional when the agent advertises it — useful later for monorepos, not MVP.

MCP servers can be attached per session. That is how crew tools (GitHub,
browser, …) reach a code thread without each harness inventing its own
config. Agents must support stdio MCP; HTTP is optional.

## Transports and remote

Stable transport today: **stdio subprocess**.

[Streamable HTTP / WebSocket](https://agentclientprotocol.com/protocol/v2/transports)
is a **draft**. The intro page still says full remote-agent support is a work
in progress.

Implications for [remote-and-mobile](../remote-and-mobile/brief.md):

- MVP1 can still split UI vs host: the **host** speaks ACP over stdio to the
  harness, and we pick our own client↔host wire (WebSocket of normalized
  events, or ACP tunneled).
- Do not wait for ACP remote to ship before deciding the client/host split.
- Codex already has its **own** remote (`codex app-server --listen ws://…` +
  `codex --remote`). That is Codex-specific, not ACP.

Custom transports are allowed if they preserve JSON-RPC framing. Fine for a
JaBot-internal daemon; not a substitute for "Custom harness" which should stay
stdio ACP so third-party tools work.

## Protocol versions

- **v1** is what shipped adapters and Zed speak today. Use this for MVP.
- **v2** is published in draft (prompt lifecycle split, richer permission
  subjects, state updates). Track it; do not require it.

SDKs exist: TypeScript, Rust, Python, Java, Kotlin. TypeScript + Rust both
hit 1.0. If the app shell is Tauri, the Rust SDK is the natural host-side
client. If Electron, use `@agentclientprotocol/sdk`.

## Ecosystem (named harnesses)

From the [official agents list](https://agentclientprotocol.com/overview/agents)
as of this research:

| Agent | ACP status |
|---|---|
| Claude Code / Claude Agent | Via Zed adapter (`claude-agent-acp` / `@zed-industries/claude-code-acp`), wrapping the Claude Agent SDK. Anthropic has not adopted ACP natively. |
| Codex CLI | Via adapter (`codex-acp` / `@zed-industries/codex-acp`). Official OpenAI ACP support still described as pending in secondary roundups; OpenAI's first-class rich-client API is **app-server**, not ACP. |
| Pi | Via community `pi-acp` (bridges ACP ↔ `pi --mode rpc`). Native ACP has been discussed, not the default. |
| Gemini CLI, Goose, Copilot CLI, Cline, OpenCode, OpenHands, Cursor CLI, … | Native or first-party ACP. Relevant for Custom / future presets. |

ACP Registry exists so clients can install agents by catalog rather than
hardcoding commands. Nice later; for MVP, ship command + args like Buzz.

## Gaps vs JaBot

ACP does **not** define:

- Inbox / fold / "Wait for Inbox" — our overlay.
- Git worktrees / PR linkage — [git-and-prs](../git-and-prs/brief.md).
- Crew bots, schedules, Chief of Staff — [bot-crew](../bot-crew/brief.md).
- A durable remote pairing story — draft transports only.

Those stay JaBot-owned. Everything that is "drive a coding agent and render
it as chat" is already in ACP.
