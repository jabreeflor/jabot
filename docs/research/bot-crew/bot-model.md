# What a bot is

Opinionated model for JaBot. Research, not a build spec.

**Runtime decision is locked** in
[`docs/decisions/issues-4-6.md`](../../decisions/issues-4-6.md) (#6):
every bot is an ACP harness session. The two-runtime split below is
historical; do not implement a host-owned Messages API loop.

## Decision

A **bot** is a named persona the user chats with in the messenger — a
**scope** (instructions, MCP allowlist, memory, credentials) running as an
**ACP harness session**. Harness is chosen from the Buzz-style catalog
(builtins / presets / custom JSON) and is customizable per bot.

```
  User taps Chief, Code, Inbox, Writer, a template, New Chat, …
                    │
                    ▼
          ┌─────────────────────────────────────────┐
          │  ACP harness session                    │
          │  claude / codex / pi / hermes /         │
          │  openclaw / Custom JSON stdio           │
          │  cwd + mcpServers from the JaBot catalog│
          └─────────────────────────────────────────┘
```

There is **one runtime**. What differs per bot is the harness pick, the
tool allowlist, memory, and cwd:

| | Code (and folder New Chat) | Chief / Inbox / Writer / templates |
|---|---|---|
| Engine | ACP harness | ACP harness (same adapters) |
| Default harness | Coding card (Claude/Codex/Pi); overridable per thread | Bot `harness_id`; user-customizable in the crew editor |
| cwd | Folder repo / host-owned worktree | Bot memory/workspace dir — **no git worktree** |
| Threads | Many, grouped in folders | One standing chat |
| MCP | GitHub + whatever is chipped | Only what is chipped (Gmail, …) |
| Host tools | — | Chief: `handoff_to_bot`, `spawn_code_session`, … |

Do **not** put Gmail inside a Code thread "because subagents exist" — that
gives a mail bot a terminal by default. Inbox Mgr is its own session with
a Gmail allowlist, even if its harness happens to be `claude-agent-acp`.
Host-selected MCP is authoritative; skip ambient harness MCP.

## Record shape (logical)

Matches the prototype editor (`name`, `instr`, color class, tool chips)
plus a harness picker:

```text
Bot
  id
  name
  color
  instructions          # user-edited; SOUL / system prompt
  tools[]               # ids into the MCP catalog; Chief also gets host tools
  harness_id            # default catalog id; thread may override (New Chat)
  is_chief
  memoryDir             # local files; see templates-memory-schedules.md
  threadId?             # standing chat for non-Code bots
```

A **template** is the same fields without `id` / `threadId` — a stamp, not a
live agent. Adding a template copies it into `CREW`.

## Kind: Code

A Code thread **is** an ACP session. The Code bot in the strip is the
persona that *owns* those sessions (instructions like "never push to
main"), not a second loop in front of ACP.

When the user (or Chief) starts coding work:

1. JaBot spawns the chosen harness over ACP stdio (thread override or the
   Code bot's `harness_id`).
2. `session/new` gets worktree `cwd` plus that bot's MCP servers (GitHub,
   maybe Browser).
3. Transcript, toolblocks, and permissions are the harness adapter.

The Code bot's `instructions` should be passed in as the session's extra
system prompt / ACP config if the adapter allows it; otherwise prepend them
to the first user message. Do not re-implement Claude Code inside JaBot.

## Kind: Worker (Inbox, Scheduler, Research, Writer, templates)

Same ACP path as Code. Differences are product, not engine:

- **cwd** is the bot memory/workspace directory, not a git worktree.
- **One standing thread.** Extra tasks append, or fold a long run to Inbox.
- MCP allowlist is the only tools the session should see (plus whatever
  the chosen harness ships — host must skip ambient MCP where possible).
- No repo write, no unbounded shell — unless Terminal is on the allowlist,
  which we treat as "this bot may start a code session" rather than an
  unbounded shell in the worker's cwd.
- Host still owns conversation overlay in SQLite (`transcript_events`).
  Native harness JSONL is a resume pointer only.

Compare, bluntly:

| | ACP harness + JaBot MCP allowlist | Claude Agent SDK / subagent |
|---|---|---|
| Loop owner | Adapter subprocess | Claude Code engine |
| Default tools | Harness-native + host `mcpServers` | Files + bash + Agent |
| Session identity | ACP `sessionId` (our `threadId` overlay) | Nested inside one Code session |
| Fits Inbox Mgr | **Yes** — own session, Gmail only | Overpowered; wrong grain |
| Fits Code | **Yes** | Yes, but we speak ACP instead |

[Claude's own agents doc](https://code.claude.com/docs/en/agents) is explicit:
in every parallel approach the workers are **Claude sessions**, and a
different tool is an **MCP server**. Use that split. Do not import
subagents as JaBot's crew.

Subagents ([docs](https://code.claude.com/docs/en/sub-agents)) are "delegated
workers **inside one session** that return a summary." Messenger bots are
standing chats the user can open tomorrow. Different grain.

Agent SDK `AgentDefinition` objects (`prompt`, `tools`, `mcpServers`,
`memory`) are a useful **checklist** for our bot record. They are not a
reason to embed the SDK.

Buzz persona packs are the prior art for "this agent = prompt + runtime +
MCP + skills" on top of the same ACP supervisor
([setup-porting/buzz.md](../setup-porting/buzz.md)). Copy that shape, not
Nostr keypairs.

## Kind: Chief

Chief is a worker (ACP harness session) with extra **host tools**, not a
third runtime and not `tools: ['Everything']` in the MCP sense. Prototype
"Everything" means "may route anywhere," not "unrestricted Gmail + shell +
Slack."

Chief's model sees:

- Its instructions ("Route work… surface only what matters").
- Shared `USER.md` (voice, timezone, repos).
- A **status board** the host injects: crew roster, running/folded code
  threads, last worker summaries, due schedules.
- Host tools: `handoff_to_bot`, `spawn_code_session`, `fold_thread`,
  `list_crew_status` — implemented in JaBot, passed as MCP on
  `session/new`.

Chief should **not** call Gmail directly on day one. If the user asks
"what's in my inbox?", Chief hands off to Inbox Mgr (or the user taps
Inbox). That keeps tool allowlists honest and matches the prototype's
delegation talk.

Later, if we want Chief to peek at mail without a handoff, add Gmail to
Chief's allowlist as an explicit user choice in the editor — same CRUD as
any bot. Default off.

Chief's `harness_id` is as customizable as any other bot's. Do not require
Claude Code to be installed for Chief or Inbox Mgr to work — pick a
catalog entry that is actually on PATH (including Custom JSON).

## What we are not

**Not Claude Code subagents.** Isolation yes; persistent named bots with
their own sidebar chats, no.

**Not Claude Agent Teams.** Experimental, disabled by default, teammates
message each other inside the coding product. JaBot's UI already *is* the
team view.

**Not CrewAI / LangGraph.** LangChain's own guidance is to start with a
single agent and add tools before adding agents
([multi-agent architecture](https://www.langchain.com/blog/choosing-the-right-multi-agent-architecture)).
Their "subagents / handoffs / router" taxonomy is useful vocabulary
(Chief ≈ stateful router + handoff tools; workers ≈ specialists that talk
to the user). The frameworks are not. A personal messenger does not need
a compiled graph, `Process.hierarchical`, or Deep Agents.

**Not OpenClaw as a dependency.** Closest prior art: isolated agents, file
memory, in-process cron, bindings that route inbound messages
([multi-agent](https://docs.openclaw.ai/concepts/multi-agent)). Steal the
shapes. Do not run their Gateway. JaBot already has a UI, ACP, and an
Inbox.

**Not one mega-agent with "skills."** The prototype's point is a **crew
you customize**. Skills (progressive disclosure of prompts) are how a
coding harness loads extra instructions. Fine inside Code. Wrong as the
only abstraction for Inbox vs Writer — those need different tools and
different standing threads.

## Harness for every bot

The engine is the Buzz-style catalog, not a vendor chat API that JaBot
wraps itself. Default `harness_id` per bot; New Chat may override for a
code thread. Custom JSON is a first-class card, same shape as
[harness-integration/buzz.md](../harness-integration/buzz.md).

Do not require Claude Code to be installed for Inbox Mgr to work. Pick any
catalog entry that probes ready — including a Custom ACP command.

## Permissions

Same UX as ACP `session/request_permission` for mutating tools (send mail,
create calendar events, post Slack). Read-only MCP can auto-allow under a
"Wait for Inbox" policy later. Session-lifecycle owns fold/resurface;
crew just emits "needs you" when a worker is blocked on approval.
