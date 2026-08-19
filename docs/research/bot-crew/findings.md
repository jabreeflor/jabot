# Bot Crew & Orchestration — Findings

Researched August 2026 against current public docs, official MCP servers, and
prior art (Claude Code subagents, OpenClaw, CrewAI/LangGraph). This file answers
the six questions in [`brief.md`](brief.md). Deep dives live in sibling files.

**Superseded on the runtime split.** Issue
[#6](../../decisions/issues-4-6.md) settled that **every crew bot is an ACP
harness session** with a Buzz-style catalog (builtins / presets / custom JSON)
and a per-bot `harness_id`. There is no host-owned “thin LLM + MCP” loop.
Chief still routes by opening or spawning a worker thread, not by nesting
Claude Code subagents.

The rest of this topic still holds: crew as named personas, MCP allowlists,
templates as data, local memory files, in-process schedules.

**Recommendation in one sentence (updated):** JaBot should treat **every bot
as an ACP harness session** (persona + MCP allowlist + memory + default
harness) — Chief routes by **opening or spawning a worker thread**, not by
nesting Claude Code subagents or adopting CrewAI / LangGraph.

| Question | Short answer | Detail |
|---|---|---|
| 1. What is a bot? | One kind. Named scope (persona, tools, memory) running as an ACP harness session. Harness is chosen from the Buzz-style catalog and is customizable per bot. | [bot-model.md](bot-model.md), [#6](../../decisions/issues-4-6.md) |
| 2. Chief routing | User can pick a bot. Chief can also hand off: spawn/open that bot's thread with a brief. Chief sees roster + status cards, not full worker transcripts. | [routing.md](routing.md) |
| 3. Tools | Official MCP exists and is solid enough for all eight prototype tools except Terminal (that's harness `execute`). Auth is OAuth 2.1 per remote MCP; tokens never live in crew JSON. | [mcp-and-tools.md](mcp-and-tools.md) |
| 4. Templates | Shipped JSON: name, color, instructions, tool ids, default harness. Core crew is pre-installed; Expense / Talent / Social / Ops are add-from-template snapshots. | [templates-memory-schedules.md](templates-memory-schedules.md#templates) |
| 5. Bot memory | Yes, across chats, local-first: per-bot markdown (`instructions.md` + `MEMORY.md`) plus the thread transcript in SQLite. No cloud memory store. | [templates-memory-schedules.md](templates-memory-schedules.md#memory) |
| 6. Schedules | In-process cron in the JaBot host. A job is `{botId, cron, prompt}`. Fire = run that bot's harness session, fold to Inbox. Not launchd, not EventKit. | [templates-memory-schedules.md](templates-memory-schedules.md#schedules) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Crew store + CRUD** — bots as data (id, name, color, instructions, tool
   allowlist, memory dir). Prototype Crew view, real. See
   [templates-memory-schedules.md](templates-memory-schedules.md).
2. **Chief of Staff chat** — ACP harness session + host tools
   (`handoff_to_bot`, `spawn_code_session`, `list_crew_status`). Same adapter
   path as Code. See [routing.md](routing.md) and
   [#6](../../decisions/issues-4-6.md).
3. **Tool/MCP connection framework** — one host-owned MCP catalog; OAuth in
   the OS keychain; per-bot allowlist. Pass the same servers on ACP
   `session/new` for **every** bot thread. See
   [mcp-and-tools.md](mcp-and-tools.md).
4. **Bot templates shipped as data** — JSON packs for Expense / Talent /
   Social / Ops plus the five default workers.
5. **Schedules view** — SQLite job rows + in-process scheduler; Inbox on
   completion. Feeds [session-lifecycle](../session-lifecycle/brief.md) and
   [data-and-persistence](../data-and-persistence/brief.md).

## Prototype note

`prototypes/jabot-classic.html` already encodes the model we should keep:

- `CREW`: Chief (not removable, `tools: ['Everything']`) plus Code, Inbox
  Mgr, Scheduler, Research, Writer.
- `TEMPLATES`: expense / talent / social / ops — starting snapshots, not
  extra runtimes.
- `ALL_TOOLS`: Gmail, Calendar, GitHub, Terminal, Browser, Notion, Drive,
  Slack.
- Bot editor fields: **name, instructions, color, tool chips**. That *is*
  the bot record. Do not invent a hidden graph behind it.
- Chief's sample chat already delegates ("Scheduler fixed Thursday",
  "fold the auth migration") — routing is a **host action**, then a
  status line back in Chief, not a nested subagent dump.

The sidebar has no Schedules view yet. MVP can be a list under Crew or a
later pane; the data model does not wait on the UI.

## Locked decisions this depends on

From [harness-integration](../harness-integration/findings.md):

- Code threads are **ACP** harness sessions (Claude / Codex / Pi / Custom
  stdio).
- Crew tools reach a code thread as `mcpServers` on `session/new`.
- Custom harness is ACP stdio — not a place to hide the Chief.

Do **not** run Inbox Mgr or Writer as Claude Code *subagents* just because the
Agent SDK can spawn them. Subagents live *inside one coding session*
and return a summary; they are the wrong grain for messenger bots. Inbox Mgr
is its own ACP session (possibly on Claude, Codex, Pi, or Custom) with a
Gmail allowlist — not a nested worker inside a Code thread.

## Sources

Primary docs, not secondary blogs, unless noted:

- ACP MCP on session create:
  [agentclientprotocol.com session-setup](https://agentclientprotocol.com/protocol/v1/session-setup)
- Claude Code agents vs subagents:
  [code.claude.com/docs/en/agents](https://code.claude.com/docs/en/agents),
  [sub-agents](https://code.claude.com/docs/en/sub-agents),
  [Agent SDK subagents](https://code.claude.com/docs/en/agent-sdk/subagents)
- Claude memory files:
  [code.claude.com/docs/en/memory](https://code.claude.com/docs/en/memory.md)
- Google Workspace remote MCP (public developer preview, May 2026):
  [Configure the Google Workspace MCP servers](https://developers.google.com/workspace/guides/configure-mcp-servers),
  [Workspace Updates announcement](https://workspaceupdates.googleblog.com/2026/05/agent-tools-and-security-updates-for-workspace-developers.html)
- GitHub MCP: [github/github-mcp-server](https://github.com/github/github-mcp-server)
- Playwright MCP: [microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp),
  [playwright.dev/mcp](https://playwright.dev/mcp/configuration/user-profile)
- Notion MCP: [developers.notion.com/docs/mcp](https://developers.notion.com/docs/mcp),
  [mcp.notion.com](https://mcp.notion.com/mcp)
- Slack MCP: [Guide to the Slack MCP server](https://slack.com/help/articles/48855576908307-Guide-to-the-Slack-MCP-server),
  [mcp.slack.com](https://mcp.slack.com/mcp)
- MCP OAuth: [modelcontextprotocol.io authorization](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/authorization)
- Electron secrets: [safeStorage](https://electronjs.org/docs/latest/api/safe-storage)
- Prior art only: [OpenClaw multi-agent](https://docs.openclaw.ai/concepts/multi-agent),
  [OpenClaw memory](https://docs.openclaw.ai/concepts/memory.md),
  [OpenClaw cron](https://docs.openclaw.ai/automation/cron-jobs.md),
  [LangChain multi-agent patterns](https://www.langchain.com/blog/choosing-the-right-multi-agent-architecture)
