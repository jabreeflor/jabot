# Bot Crew & Orchestration

The messenger half: a Chief of Staff bot that routes work, plus template bots
(Code, Inbox Manager, Scheduler, Research, Writer) the user can add, edit,
recolor, and give instructions + tools.

Depends on: [harness-integration](../harness-integration/brief.md).

**Findings (2026-08):** questions below are answered in
[findings.md](findings.md). Deep dives: [bot-model.md](bot-model.md),
[routing.md](routing.md), [mcp-and-tools.md](mcp-and-tools.md),
[templates-memory-schedules.md](templates-memory-schedules.md). Headline:
Code is ACP; every other bot is a thin LLM + MCP allowlist; Chief hands
off to worker threads instead of nesting subagents.

## Questions to answer

1. **What is a bot, technically?** A system prompt + tool allowlist on top of a
   model API call? Or a full harness session with a persona? Chief vs worker
   bots may differ.
2. **Chief of Staff routing** — how does the Chief delegate? Explicit handoff
   (Chief spawns a thread with another bot / a code session) vs the user just
   picking a bot. What does the Chief see across the crew?
3. **Tools** — prototype lists Gmail, Calendar, GitHub, Terminal, Browser,
   Notion, Drive, Slack. MCP servers are the obvious answer — which exist and
   are solid for each? How does auth work per tool?
4. **Templates** — what ships in the box (Expense, Talent, Social, Ops in the
   prototype)? What's in a template: instructions, tools, color, name.
5. **Bot memory** — does a bot remember across chats? Where does that live?
6. **Schedules** — the sidebar promises scheduled/recurring bot work. Cron-like
   local scheduler? What's the minimal MVP version?

## What this blocks (future issues)

- Crew store + CRUD (prototype's Crew view, real)
- Chief of Staff chat (real model behind it)
- Tool/MCP connection framework + per-tool auth
- Bot templates shipped as data
- Schedules view (not yet designed in the prototype)
