# Pi (coding agent)

**Not Inflection Pi.** This is Mario Zechner's minimal coding harness:
[pi.dev](https://pi.dev/), npm `@mariozechner/pi-coding-agent` /
`@earendil-works/pi-coding-agent`. The prototype's "Inflection's agent"
label is wrong.

Pi is built to be embedded. It is the easiest of the three to wrap without
ACP — and it also has a community ACP adapter.

## Four modes

| Mode | Command | Role |
|---|---|---|
| Interactive TUI | `pi` | Do not wrap. |
| Print | `pi -p "query"` | One-shot scripts. |
| JSON events | `--mode json` | Event stream for scripting. |
| **RPC** | `pi --mode rpc` | Headless JSONL over stdin/stdout. **This is the embed API.** |
| SDK | `AgentSession` from `@earendil-works/pi-coding-agent` | In-process if JaBot is Node. |

Docs: [packages/coding-agent/docs/rpc.md](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md).
OpenClaw is cited as a real-world SDK integration.

## RPC protocol

Spawn:

```bash
pi --mode rpc --name "Auth migration" --session-dir <dir>
```

Framing: strict JSONL, LF only. Node `readline` is **not** safe (it splits
on U+2028/U+2029). Split on `\n` yourself.

Commands (stdin) and events (stdout). Optional `id` correlates
request/response. Responses: `{ type: "response", command, success }`.

### Commands we care about

| Command | Purpose |
|---|---|
| `prompt` | User message. If already streaming, must set `streamingBehavior`: `steer` or `followUp`. |
| `steer` / `follow_up` | Explicit queue while running / after settle. |
| `abort` | Cancel current operation. |
| `new_session` | Fresh session (extensions can cancel). |
| `get_state` | `sessionId`, `sessionFile`, `sessionName`, `isStreaming`, model, queue depths. |
| `get_messages` | Full transcript. |
| `set_session_name` | Display name. Also `--name` / `-n` at spawn. |
| `switch_session` | Load another JSONL path. |
| `fork` / `clone` | Branch history. |
| `get_session_stats` | Tokens, cost, context %. |
| `bash` | Host-initiated shell; streams `bash_execution_update`. |

Kill the session = abort + exit the process, or `switch_session` away.
Persistence is the JSONL file (`sessionFile` from `get_state`).
`--no-session` disables disk.

Sessions are a **tree** (in-place branching, `/tree` time travel), stored
as JSONL per cwd. Resume: `pi -c` (TUI most-recent) or RPC
`switch_session` to the file path. JaBot should store `sessionFile` +
`sessionId`, not only an ephemeral pid.

### Events (the chat stream)

| Event | Maps to |
|---|---|
| `message_update` | Streaming assistant / thinking / tool-call deltas |
| `message_start` / `message_end` | Bubble lifecycle |
| `tool_execution_start` / `_update` / `_end` | Toolblocks. `partialResult` is **accumulated**, not a delta — replace the display. Correlate with `toolCallId`. |
| `turn_start` / `turn_end` | One assistant + tools |
| `agent_start` / `agent_end` | Low-level run; `willRetry` means it is not done |
| **`agent_settled`** | **True idle.** No retry, compaction, or queued follow-up left. **Inbox "done".** |
| `queue_update` | Steer/follow-up pending |
| `compaction_*` / `auto_retry_*` | Show as system lines, not failures |
| `extension_error` | Error bubble |

`agent_end` is **not** done. Wait for `agent_settled`. That distinction
matters for Disappearing Threads.

Built-in tools: `read`, `write`, `edit`, `bash` (plus grep/find/ls
depending on version). No sub-agents and no background bash in core — Pi
tells you to use tmux. Long jobs are still in-process unless we add that.

## Permissions

Pi does **not** have Claude/Codex-style permission modes on the core agent.
RPC will run tools unless an **extension** intercepts `tool_call`.

Extension UI in RPC is a nested request/response: `ctx.ui.select()`,
`confirm()`, `input()`, `editor()` become stdout requests the host must
answer. The `rpc-demo` extension blocks dangerous `bash` (`rm -rf`, `sudo`)
that way.

For JaBot:

- Do not assume prompts will appear.
- Ship a small Pi extension (or configure one) that forwards bash/write
  to our permission UI via the extension-UI protocol — **or** go through
  `pi-acp`, which is supposed to bridge Pi permission hooks to ACP
  `session/request_permission`.
- Sandbox at the host (OS, worktree, landlock) rather than trusting Pi
  defaults.

## ACP adapter

[pi-acp](https://www.npmjs.com/package/pi-acp) (and forks such as
`@victor-software-house/pi-acp`):

- Speaks ACP JSON-RPC on stdio.
- Spawns `pi --mode rpc` **or** embeds `AgentSession` in-process.
- One Pi subprocess (or session) per ACP `session/new`.
- Session files map to ACP load/resume (Zed history from ~v0.225).
- Limitations called out by maintainers: filesystem/terminal **delegation**
  to the client is incomplete or in progress; MCP passthrough may be
  missing. Native ACP support has been discussed on the Pi repo
  ([discussion #4444](https://github.com/earendil-works/pi/discussions/4444))
  as a translation layer that would **not** replace RPC.

For a unified JaBot ACP client, `npx pi-acp` is still the right Pi card
command, with native RPC as the fallback if the adapter is thin.

## Auth / models

Multi-provider: Anthropic, OpenAI, Google, OpenRouter, etc. Credentials are
whatever Pi already uses on the machine. `--provider` / `--model` at RPC
start; `set_model` / `cycle_model` at runtime. Expose model as a session
config chip later; not MVP.
