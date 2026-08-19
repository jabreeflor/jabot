# Decisions: GitHub issues #4–#6

Settled 2026-08-19 after an interview on the three forks that gate
[the build plan](../plan.md). Research that framed the forks:
[`docs/research/README.md`](../research/README.md).

These decisions **supersede** the open-fork text in the research README and
the two-runtime bot model in [`bot-crew`](../research/bot-crew/findings.md).
Build issues should follow this file, not the superseded recommendations.

| Issue | Decision |
|---|---|
| [#4](https://github.com/jabreeflor/jabot/issues/4) | Logical host split in-process; hide-to-Dock; Quit resumes from disk |
| [#5](https://github.com/jabreeflor/jabot/issues/5) | Thread overlay + `runs` table; Inbox is a projection of run events |
| [#6](https://github.com/jabreeflor/jabot/issues/6) | Every bot is an ACP harness session; Buzz-style catalog + per-bot harness |

---

## #4 — Physical host process and quit policy

**Ship the logical split in MVP1. Keep the host in the Tauri binary.
Do not install launchd. Extract `jabot-host` when a second client exists.**

```
WKWebView  →  socket-shaped host API (Tauri IPC now; Unix socket later)
                    ↓
            Rust host in JaBot.app  →  ACP stdio adapters
```

| Event | Policy |
|---|---|
| Close last window | **Hide to Dock.** Host and in-flight ACP children stay. |
| Cmd-Q / Dock Quit | Persist overlay (`sessionId`, `cwd`, run state). Kill adapter process groups. Next launch: `session/resume`. In-flight work resurfaces as interrupted / stuck. |
| Lid close / crash / reboot | Same as Quit: resume, not a living PID. |
| Second client (phone, another Mac) | Extract sidecar `jabot-host`. Same messages. |

The UI never owns ACP stdio. The host API is designed as if it were already
a socket so the extract is packaging, not a rewrite.

**Not in MVP1:** a LaunchAgent so Quit leaves agents running. Users should
not expect a coding supervisor to outlive Cmd-Q. Sleep already kills PIDs;
durability is resume.

Unblocks [#7](https://github.com/jabreeflor/jabot/issues/7) scaffold,
[#8](https://github.com/jabreeflor/jabot/issues/8) host API,
[#21](https://github.com/jabreeflor/jabot/issues/21) supervisor.

---

## #5 — Fold / run / Inbox data model

**Four-state thread overlay plus a first-class `runs` table. Inbox is a
persist-then-notify projection. Fold is visibility only.**

| Layer | What it is | Store |
|---|---|---|
| Conversation | Standing chat / ACP `sessionId` | `threads` |
| Work | One turn/job | `runs` |
| Fold | Hide from sidebar | `threads.state` |
| Inbox | Cards the human should see | `inbox_events` from run transitions |

Thread UI state stays:

```
active → folded → resurfaced → archived
```

Wait for Inbox is `fold_policy` on the thread, not a fifth state. “Still
working” is supervisor RAM, reconciled on boot — not a durable enum.

One thread has **many sequential runs** (another prompt, a schedule fire,
a Chief re-dispatch) on the same ACP `sessionId`. Run states:

```
queued → running → succeeded | failed | cancelled | timed_out | lost | needs_you
```

Inbox:

- **Still sleeping** = `threads.state = folded`.
- **Needs you / Done** (and failed / lost) = filter `inbox_events` /
  latest run.
- Write the event **before** notifying the UI. Notification failure must
  not lose the result.

Unblocks [#9](https://github.com/jabreeflor/jabot/issues/9) schema,
[#15](https://github.com/jabreeflor/jabot/issues/15) state machine,
[#22](https://github.com/jabreeflor/jabot/issues/22) Inbox.

Schema sketch: [`schema.md`](../research/data-and-persistence/schema.md).

---

## #6 — What is a bot?

**Every crew bot is an ACP harness session.** There is no second
host-owned “thin LLM + MCP” runtime. Harness choice is Buzz-shaped:
three-tier catalog, per-bot default, custom JSON.

This is the override of the bot-crew research headline (Code = ACP;
everyone else = Messages API loop). Crew is still a **scope** (persona,
tools, memory, credentials) — the *engine* under every scope is a
harness from the catalog.

### Runtime (one)

```
User / Chief / schedule
        ↓
JaBot host supervisor
        ↓  ACP stdio
Harness from catalog (claude-agent-acp, codex-acp, pi-acp,
                      hermes acp, openclaw acp, Custom JSON, …)
        ↓  MCP from JaBot catalog on session/new
Gmail / Calendar / GitHub / …  (allowlisted per bot)
```

Chief, Inbox Mgr, Writer, Code, and user-added templates all take this
path. The host still owns process trees, permission prompts, SQLite, and
which MCP servers are passed in. Skip ambient harness MCP
(`HERMES_ACP_SKIP_CONFIGURED_MCP=1` as a general rule).

Do **not** implement a parallel Anthropic/OpenAI tool-use loop in the
host for workers. Do **not** turn crew into Claude Code subagents.

### Buzz-style harness catalog

Copy the seam from [harness-integration/buzz.md](../research/harness-integration/buzz.md)
and [setup-porting/buzz.md](../research/setup-porting/buzz.md):

| Tier | What | Examples |
|---|---|---|
| 1 — compiled-in | Shipped cards, auth probes, reserved ids | `claude`, `codex`, `pi` |
| 2 — presets | PATH-probed, not user-editable | Hermes, OpenClaw, Cursor, … |
| 3 — user JSON | Settings / `custom_harnesses/` | Anything that speaks ACP stdio |

Custom JSON (Buzz schema):

```json
{
  "id": "my-agent",
  "label": "My Agent",
  "command": "my-agent-bin",
  "args": ["acp"],
  "env": { "MY_AGENT_MODE": "acp" },
  "installHint": "Download from example.com",
  "installInstructionsUrl": "https://example.com/docs"
}
```

Rules: reserved ids cannot be shadowed; no install scripts; host-reserved
env keys stripped; Doctor distinguishes CLI missing / adapter missing /
logged out / daemon not running.

### Bot record (scope + harness)

```text
Bot
  id, name, color
  instructions          # persona / system prompt
  tools[]               # MCP catalog ids (Chief also gets host tools)
  harness_id            # default from the catalog; user-customizable
  memoryDir
  is_chief
  template_id?
```

A **template** is the same fields without `id` — including `harness_id`.
Adding a template copies it into crew. The crew editor (prototype name /
instructions / color / tool chips) gains a **harness picker**.

Thread spawn:

1. Resolve harness: thread override (New Chat card) else `bots.harness_id`.
2. Snapshot `{ command, args, env }` onto the thread (`runtime_json`).
3. `session/new` with `cwd` + host-selected `mcpServers` from the bot allowlist.
4. Pass `instructions` as extra system prompt / ACP config when the adapter
   allows it; otherwise prepend to the first user message.

**Code** remains one crew member that owns **many** folder threads. New Chat
in a folder still picks a harness for *that thread*; it may differ from the
Code bot’s default.

**Everyone else** has **one standing thread**. Extra tasks append (or fold a
long run to Inbox). They do not get a git worktree. `cwd` is the bot’s
memory/workspace directory.

### Isolation (credentials and memory, not a DB per bot)

- One JaBot SQLite. No OpenClaw-style private database per bot.
- Tokens in the OS keychain. **One user-level OAuth grant per provider**
  (one Gmail login); each bot **allowlists** which grants it may use.
  Do not silently share harness session stores (`~/.claude`, `HERMES_HOME`)
  across crew — each bot’s ACP session is its own `sessionId`.
- Workers have no repo cwd unless they spawn a code thread (`spawn_code_session`).

### Chief

Chief is a harness session with extra **host MCP tools**, not a third
runtime and not `tools: ['Everything']` in the product-MCP sense:

- `handoff_to_bot`, `spawn_code_session`, `fold_thread`, `list_crew_status`
- Default: Chief does not call Gmail itself; it hands off to Inbox Mgr
- Routing is still a host action (handoff), not a nested subagent

Unblocks [#17](https://github.com/jabreeflor/jabot/issues/17) crew store,
[#18](https://github.com/jabreeflor/jabot/issues/18) MCP,
[#24](https://github.com/jabreeflor/jabot/issues/24) Chief.

---

## What this does not change

Already locked and still true:

- Speak ACP. Do not PTY-wrap TUIs.
- Never auto-allow execute because a thread is folded.
- SQLite WAL + OS keychain.
- Logical client/host split; pairing / mobile are MVP2.
- Host-owned worktree per concurrent **code** thread.

## Thinnest vertical slice (unchanged spine)

[#4](https://github.com/jabreeflor/jabot/issues/4) →
[#7](https://github.com/jabreeflor/jabot/issues/7) →
[#10](https://github.com/jabreeflor/jabot/issues/10) +
[#11](https://github.com/jabreeflor/jabot/issues/11) →
[#14](https://github.com/jabreeflor/jabot/issues/14) →
[#20](https://github.com/jabreeflor/jabot/issues/20):
one Claude Code thread rendered as chat with real permission prompts.

The #6 change means that same adapter path is how Chief and Inbox Mgr
will run later — not a second loop to invent after the spine.
