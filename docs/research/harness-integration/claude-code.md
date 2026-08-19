# Claude Code

Anthropic's coding agent. JaBot should **not** PTY-wrap the TUI. Three
programmatic surfaces exist; the ACP adapter sits on the Agent SDK, which
sits on the same engine as the CLI.

## Integration modes

| Mode | How | Use in JaBot |
|---|---|---|
| **ACP adapter (preferred)** | `@zed-industries/claude-code-acp` / `claude-agent-acp`. Spawn as ACP agent over stdio. Wraps `@anthropic-ai/claude-agent-sdk`. | Default Claude card. Same client code as Codex/Pi. |
| **Agent SDK** | `@anthropic-ai/claude-agent-sdk` (`query()` in TS, `query` / `ClaudeSDKClient` in Python). | Fallback if the ACP adapter lags a SDK feature we need (`canUseTool`, hooks). |
| **Headless CLI** | `claude -p` (`--print`) with `--output-format stream-json --verbose`. | Scripting / debugging. Worse for a live chat: one-shot by default, permission UX is flags not a callback. |
| **Interactive TUI** | `claude` with no `-p`. | Do not wrap. |

The npm adapter advertises: context @-mentions, images, tool calls with
permission requests, follow-along, edit review, TODOs, interactive and
background terminals, slash commands, client MCP servers.

Auth: `ANTHROPIC_API_KEY`, or the user's existing Claude Code login on the
machine. Prefer "reuse local CLI login" for a personal desktop app.

## Headless CLI (for completeness)

```bash
claude -p "Migrate auth to sessions" \
  --output-format stream-json \
  --verbose \
  --include-partial-messages
```

`--output-format stream-json` needs `--verbose`. Token-level deltas need
`--include-partial-messages`.

Output is NDJSON. Useful types:

| `type` | What it is |
|---|---|
| `system` (`subtype: init`) | `session_id`, tools, model, cwd, permissionMode |
| `assistant` | Content blocks: `text`, `thinking`, `tool_use` (`id`, `name`, `input`) |
| `user` | Tool results (`tool_result` / `tool_use_id`) |
| `result` | Terminal: `subtype` success/error, `is_error`, `duration_ms`, `num_turns`, `total_cost_usd`, `session_id`, optional `permission_denials` |
| `stream_event` | Partial message deltas when requested |

`--input-format stream-json` exists for chaining. A long-lived chat is still
awkward: each `-p` invocation is a process that runs until the turn ends.

Permissions in headless: pre-approve with `--allowedTools` and
`--permission-mode`. Unattended CI uses `bypassPermissions`. That is the
wrong default for JaBot — we want prompts in our UI.

## Agent SDK

This is what a real product should use if not going through ACP.

```ts
import { query } from "@anthropic-ai/claude-agent-sdk";

for await (const message of query({
  prompt: "Migrate auth to sessions",
  options: {
    permissionMode: "default",
    canUseTool: async (toolName, input, ctx) => {
      // show JaBot permission UI, return allow / deny
    },
  },
})) {
  // message.type: system | assistant | user | result | ...
}
```

**Permissions** ([docs](https://code.claude.com/docs/en/agent-sdk/permissions)):

Evaluation order: hooks → deny rules → ask rules → permission mode → allow
rules → `canUseTool`. Auto-approved tools never hit the callback.

Modes: `default` (prompt via callback), `dontAsk` (deny instead of prompt),
`acceptEdits`, `bypassPermissions`, `plan`, `auto` (model classifier).

For JaBot MVP: `permissionMode: "default"` + `canUseTool` mapped to the
permission modal. Optionally `acceptEdits` as a per-thread "trust this
folder" toggle. Never default to `bypassPermissions`.

`AskUserQuestion` and MCP tools marked
`_meta["anthropic/requiresUserInteraction"]` always fall through to the
callback. That is the "judgment call" path.

**Sessions** ([docs](https://code.claude.com/docs/en/agent-sdk/sessions)):

- Session = conversation history on disk under `~/.claude/projects/…`
  (JSONL). Filesystem changes are **not** in the session; those need
  checkpointing if we ever offer revert.
- Capture `session_id` from the init `system` message or the `result`.
- `resume: "<uuid>"` continues that session.
- `continue: true` (TS) resumes the most recent session in **this cwd**.
- `forkSession: true` with `resume` branches a new id.
- Lookup is scoped to the project directory (and git worktrees). Resume
  must use the same cwd we started with.
- `persistSession: false` for ephemeral runs.

`--name` / SDK session title exists on the CLI (`--name`). Use it so Claude's
own session list matches our thread title.

Kill: drop the `query()` / child process. There is no separate daemon; the
SDK process **is** the session runtime. Folded threads must keep that
process (or a supervisor that can `resume` later). Checkpoint-and-resume
survives laptop sleep better than keeping a live Node child; see
[session-lifecycle](../session-lifecycle/brief.md).

## Recommendation for the Claude card

1. Spawn `claude-code-acp` (or `claude-agent-acp` — Buzz treats both as the
   same zero-arg runtime) with the user's env.
2. Speak ACP. Do not parse `stream-json` in the app.
3. Keep native `session_id` in our overlay so we can resume even if we
   bypass the adapter later.
4. Permission UI is ACP `session/request_permission`; the adapter already
   bridges SDK `canUseTool`.
