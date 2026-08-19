# Harness Integration — Findings

Researched August 2026 against current public docs, adapters, and prior art.
This file answers the seven questions in [`brief.md`](brief.md). Deep dives
live in sibling files.

**Recommendation in one sentence:** JaBot should speak **ACP (Agent Client
Protocol) as its harness adapter**, spawn first-party adapters for Claude Code,
Codex, and Pi, and treat Custom as "any ACP-speaking command" — same shape
Buzz already ships. Do not PTY-wrap interactive TUIs for MVP.

| Question | Short answer | Detail |
|---|---|---|
| 1. Integration mode | Headless + SDK + ACP adapters exist for all three. No TUI scraping. | [claude-code.md](claude-code.md), [codex.md](codex.md), [pi.md](pi.md) |
| 2. Standard protocol | Yes: ACP. Cover the chat/toolblock/permission UX we need. | [acp.md](acp.md) |
| 3. How Buzz does it | ACP over stdio. Tiered runtimes + BYOH JSON. | [buzz.md](buzz.md) |
| 4. Event model | Structured events for text, tools, permissions, completion, errors. | [adapter-design.md](adapter-design.md) |
| 5. Permission prompts | Bidirectional RPC. Surface in our UI, reply allow/deny. | [adapter-design.md](adapter-design.md#permissions) |
| 6. Custom harness | Minimal contract is ACP stdio + a small JSON config. | [adapter-design.md](adapter-design.md#custom-harness) |
| 7. Session identity | ACP `sessionId` is the JaBot session key. Native IDs stored as overlay. | [adapter-design.md](adapter-design.md#session-identity) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Harness adapter trait** — ACP client in the host process. See
   [adapter-design.md](adapter-design.md).
2. **Shipped adapters** — Claude via `claude-agent-acp` / `@zed-industries/claude-code-acp`;
   Codex via `codex-acp` / `@zed-industries/codex-acp`; Pi via `pi-acp`.
3. **Chat transcript renderer** — consume ACP `session/update` (agent message
   chunks, `tool_call_update` with kinds `read` / `edit` / `execute`, diffs,
   plans). Maps 1:1 onto the prototype's bubbles + toolblocks.
4. **Permission-prompt UI** — implement ACP `session/request_permission`
   (allow once / always, reject once / always).
5. **Session lifecycle** — ACP `session/new`, `session/resume`, `session/load`,
   `session/close`, `session/cancel`, plus idle `state_update` with stop
   reasons. Feeds [session-lifecycle](../session-lifecycle/brief.md).

## Prototype note

`prototypes/jabot-classic.html` labels Pi as "Inflection's agent". Inflection
Pi is a consumer chatbot, not a coding TUI. Everything else in this repo
(wrapping Claude Code / Codex / Pi / Custom, Buzz as prior art, "Oh My Pi" in
Buzz's preset list) points at **Mario Zechner's Pi coding agent**
([pi.dev](https://pi.dev/)). Treat the prototype copy as a leftover.

## Sources

Primary docs, not secondary blogs, unless noted:

- ACP: [agentclientprotocol.com](https://agentclientprotocol.com/),
  [zed.dev/acp](https://zed.dev/acp)
- Claude: [code.claude.com Agent SDK](https://code.claude.com/docs/en/agent-sdk/sessions),
  Claude CLI `-p` / `stream-json`
- Codex: [developers.openai.com/codex/app-server](https://developers.openai.com/codex/app-server),
  [codex exec](https://developers.openai.com/codex/noninteractive)
- Pi: [packages/coding-agent/docs/rpc.md](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md)
- Buzz: [block/buzz `crates/buzz-acp`](https://github.com/block/buzz/blob/main/crates/buzz-acp/README.md)
