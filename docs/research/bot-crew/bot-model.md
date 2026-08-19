# What a bot is

Opinionated model for JaBot. Research, not a build spec.

## Decision

A **bot** is a named persona the user chats with in the messenger. Technically
it is one of two runtimes:

```
                    ┌─────────────────────────────────────────┐
  User taps Code    │  ACP harness session                    │
  or New Chat       │  Claude / Codex / Pi / Custom stdio     │
                    │  cwd + mcpServers from the catalog      │
                    └─────────────────────────────────────────┘

                    ┌─────────────────────────────────────────┐
  User taps Chief,  │  Thin host loop                         │
  Inbox, Writer,    │  system prompt + MCP allowlist          │
  a template, …     │  model API tool-use; JaBot is the       │
                    │  MCP client and the permission UI       │
                    └─────────────────────────────────────────┘
```

Do not collapse these. Code needs a coding harness (files, bash, diffs,
permissions, resume). Inbox Mgr needs Gmail drafts and nothing else. Putting
Gmail inside Claude Code "because subagents exist" gives a mail bot a
terminal by default and couples crew UX to one vendor's harness.

## Record shape (logical)

Matches the prototype editor (`name`, `instr`, color class, tool chips):

```text
Bot
  id
  name
  color
  instructions          # user-edited; SOUL / system prompt
  tools[]               # ids into the MCP catalog; Chief = special
  kind                  # "chief" | "code" | "worker"
  memoryDir             # local files; see templates-memory-schedules.md
  threadId?             # the standing chat for this bot
```

A **template** is the same fields without `id` / `threadId` — a stamp, not a
live agent. Adding a template copies it into `CREW`.

## Kind: Code

Locked in [harness-integration](../harness-integration/findings.md): a Code
thread **is** an ACP session. The Code bot in the strip is the persona that
*owns* those sessions (instructions like "never push to main"), not a second
loop in front of ACP.

When the user (or Chief) starts coding work:

1. JaBot spawns the chosen harness over ACP stdio.
2. `session/new` gets `cwd` plus that bot's MCP servers (GitHub, maybe
   Browser).
3. Transcript, toolblocks, and permissions are the harness adapter.

The Code bot's `instructions` should be passed in as the session's extra
system prompt / ACP config if the adapter allows it; otherwise prepend them
to the first user message. Do not re-implement Claude Code inside JaBot.

## Kind: Worker (Inbox, Scheduler, Research, Writer, templates)

A **thin LLM + MCP bot**:

- Host holds conversation history (our store, not `~/.claude/projects`).
- Each turn: model API with the bot's system prompt, memory files, and
  **only** the MCP tools on its allowlist.
- JaBot's MCP client executes tool calls (or denies them in the same
  permission UI as ACP).
- No Bash, no repo write, no subagent spawn — unless Terminal is on the
  allowlist, which we treat as "this bot may start a code session" rather
  than an unbounded shell.

This is what Anthropic's **Messages API tool-use loop** already is. It is
**not** the Claude Agent SDK. The Agent SDK *is* Claude Code: `query()`
brings Read/Write/Bash/Grep, CLAUDE.md, and the `Agent` tool. That is the
right engine for Code, the wrong default for mail.

Compare, bluntly:

| | Thin LLM + MCP | Claude Agent SDK / subagent | ACP harness session |
|---|---|---|---|
| Loop owner | JaBot host | Claude Code engine | Adapter subprocess |
| Default tools | Only MCP you attach | Files + bash + Agent | Harness-native + `mcpServers` |
| Session identity | Our `threadId` | Claude `session_id` | ACP `sessionId` |
| Fits Inbox Mgr | Yes | Overpowered | Wrong product |
| Fits Code | No | Yes (via adapter) | **Yes — locked** |

[Claude's own agents doc](https://code.claude.com/docs/en/agents) is explicit:
in every parallel approach the workers are **Claude sessions**, and a
different tool is an **MCP server**. Use that split. Do not import
subagents as JaBot's crew.

Subagents ([docs](https://code.claude.com/docs/en/sub-agents)) are "delegated
workers **inside one session** that return a summary." Messenger bots are
standing chats the user can open tomorrow. Different grain.

Agent SDK `AgentDefinition` objects (`prompt`, `tools`, `mcpServers`,
`memory`) are a useful **checklist** for our bot record. They are not a
reason to embed the SDK for workers.

## Kind: Chief

Chief is a worker with extra **host tools**, not a third runtime and not
`tools: ['Everything']` in the MCP sense. Prototype "Everything" means
"may route anywhere," not "unrestricted Gmail + shell + Slack."

Chief's model sees:

- Its instructions ("Route work… surface only what matters").
- Shared `USER.md` (voice, timezone, repos).
- A **status board** the host injects: crew roster, running/folded code
  threads, last worker summaries, due schedules.
- Host tools: `handoff_to_bot`, `spawn_code_session`, `fold_thread`,
  `list_crew_status` — implemented in JaBot, not MCP.

Chief should **not** call Gmail directly on day one. If the user asks
"what's in my inbox?", Chief hands off to Inbox Mgr (or the user taps
Inbox). That keeps tool allowlists honest and matches the prototype's
delegation talk.

Later, if we want Chief to peek at mail without a handoff, add Gmail to
Chief's allowlist as an explicit user choice in the editor — same CRUD as
any bot. Default off.

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

## Model API for workers

Vendor-agnostic. Default to whatever login the user already has for chat
(Anthropic Messages, OpenAI, etc.). Workers are cheaper/faster models;
Code stays on the harness's model.

Do not require Claude Code to be installed for Inbox Mgr to work. That
would make the messenger half a hostage of the code half.

## Permissions

Same UX as ACP `session/request_permission` for mutating tools (send mail,
create calendar events, post Slack). Read-only MCP can auto-allow under a
"Wait for Inbox" policy later. Session-lifecycle owns fold/resurface;
crew just emits "needs you" when a worker is blocked on approval.
