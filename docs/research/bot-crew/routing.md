# Chief routing and handoff

How work moves between the user, Chief, workers, and code sessions.

## Decision

**Two equally valid ways to talk to a bot:**

1. **User picks** — tap Code / Inbox / Writer in the strip (prototype
   `botstrip`). Opens that bot's standing thread.
2. **Chief delegates** — Chief calls a host tool that **opens or spawns**
   the worker's thread with a brief, then stays in Chief with a status
   card.

Do not hide (1). The Crew view and the strip exist so the user can skip
the receptionist. Do not make (2) a nested subagent inside Chief's
context window.

This is LangChain's **handoff** pattern (active conversation moves; the
specialist talks to the user) plus a **status overlay** back to Chief —
not their **subagents** pattern (worker is stateless, results only return
to the supervisor). JaBot is a messenger: Inbox Mgr should draft in
*Inbox Mgr's* thread, where the user can keep talking.

## What Chief sees

Not the full crew transcripts. A **board**, injected by the host each
turn (and on poll while idle):

```text
crew[]           id, name, tools, lastThreadAt
codeThreads[]    title, folder, state (running|folded|needs you|done), harness
inboxPreview[]   latest resurfaced items (from session-lifecycle)
schedules[]      next few jobs
lastHandoffs[]   botId, one-line summary, threadId
```

That is enough for the prototype line: "the auth migration is running —
about 40 minutes left — and Scheduler fixed Thursday." Those facts are
host state, not something Chief grepped out of worker logs.

If Chief needs detail, it hands off or the user opens the thread. Dumping
worker tool traces into Chief is how you blow the context window and
confuse "who sent this email."

## Handoff mechanics

Host tools on Chief (names illustrative):

| Tool | Effect |
|---|---|
| `handoff_to_bot` | Ensure worker thread exists; append a `sys` brief; run the worker loop; return a card to Chief |
| `spawn_code_session` | New ACP session (`cwd`, harness, Code bot instructions + MCP); optional fold |
| `fold_thread` | Session-lifecycle fold; Inbox on done/fail/question |
| `list_crew_status` | Refresh the board without guessing |

`handoff_to_bot` payload:

```text
botId
task            # what to do
context         # optional pasted facts; not a transcript dump
openForUser     # true: switch main pane to that bot (explicit transfer)
                # false: worker runs, Chief keeps the pane, card in-thread
```

**Explicit transfer** (`openForUser: true`) when the user should keep
talking to the specialist ("draft this in my voice" → Writer).

**Background handoff** (`openForUser: false`) when Chief is coordinating
("fix Thursday," "keep Gmail at zero"). The worker thread still exists;
the user can open it. Fold long work into Inbox — same as code.

Never: Chief's model emits a fake "I've asked Scheduler…" without the
host actually creating the job. The tool call *is* the handoff.

## Code vs workers

| User / Chief intent | Runtime |
|---|---|
| "Migrate auth in jabot-app" | ACP `session/new` in that folder. Code bot instructions apply. |
| "What's on Thursday?" | Scheduler worker + Calendar MCP. |
| "Zero the inbox" | Inbox Mgr + Gmail MCP. |
| "Brief me on this paper" | Research + Browser/Notion MCP. |
| "Write the update in my voice" | Writer + Gmail/Notion MCP. |
| "Watch deploys" (Ops template) | Prefer a folded **code** session with Terminal, or Slack MCP + a scoped exec later. |

Chief does not PTY-wrap a harness. Spawning code is the adapter from
[adapter-design.md](../harness-integration/adapter-design.md).

When a code session needs GitHub/Browser, pass those MCP servers on
`session/new` from the **same catalog** workers use. One OAuth grant,
two runtimes.

## Cross-talk

Workers do **not** message each other. OpenClaw's isolated agents plus
bindings are the right instinct
([docs](https://docs.openclaw.ai/concepts/multi-agent)): shared knowledge
is files (`USER.md`) or a Chief-mediated handoff, not a mesh.

If Writer needs a calendar slot, Chief (or the user) hands off to
Scheduler; Writer does not get Calendar unless the user chips that tool
on.

Claude Code [cross-session messaging](https://code.claude.com/docs/en/cross-session-messaging)
and [agent teams](https://code.claude.com/docs/en/agent-teams) are prior
art for "sessions can ping sessions." Useful later for "tell Chief the
PR opened." MVP: worker finishes → Inbox item + optional one-line
summary into Chief's board. Session-lifecycle already owns resurface.

## User-pick vs misroute

Chief will misroute. Mitigations, keep them small:

- Strip is always there.
- After a handoff, the card has **Open thread** (prototype already thinks
  in cards).
- Do not auto-chain five bots on one utterance. One handoff per Chief
  turn unless the user asked for a broadcast ("tell Inbox and Scheduler").

LangGraph supervisor graphs and CrewAI hierarchical process exist to
retry and replan. We do not need them until Chief is measurably bad.
A wrong tap is cheaper than a graph.

## Standing threads

Each worker has **one** standing messenger thread (the bot chat). Extra
tasks append to it, or the bot can ask to fold a long run into Inbox as
a child item. Do not spawn a new sidebar chat per Gmail draft.

Code is the opposite: **many** threads, grouped in folders. Chief's
`spawn_code_session` always creates a new code thread, never reuses an
unrelated running migration.

## Prior art we are not copying

- **CrewAI**: role + goal + backstory + manager LLM. The prototype
  already has name + instructions + tools. Adding "goals" is noise.
- **LangGraph `create_supervisor`**: hidden `transfer_to_*` tools. We
  will have the same idea, **implemented as our host tools**, visible in
  the transcript as a card, not a library node.
- **OpenClaw bindings**: inbound WhatsApp → agentId. JaBot inbound is
  the desktop UI (mobile later). Bindings map to "this chat is this
  botId." We already have that in the strip.
