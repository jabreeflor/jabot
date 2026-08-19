# Templates, memory, and schedules

Shipped crew, what a template contains, where memory lives, and the
minimal scheduler. Local-first.

## Templates

### What ships in the box

**Pre-installed crew** (prototype `CREW`, Chief not removable):

| id | Name | Tools | Instructions (sense) |
|---|---|---|---|
| chief | Chief | host routing tools | Route, fold, surface what matters |
| code | Code | GitHub (+ Terminal as ACP execute) | Coding sessions; PRs; never push main |
| inboxm | Inbox Mgr | Gmail | Zero inbox; park drafts |
| sched | Scheduler | Calendar | Guard calendar; protect deep work |
| rsrch | Research | Browser, Notion | Sources in, short brief out |
| writer | Writer | Gmail, Notion | Draft in the user's voice |

**Addable templates** (prototype `TEMPLATES` — copy-on-add):

| id | Name | Tools | Instructions (sense) |
|---|---|---|---|
| expense | Expense Manager | Gmail, Drive | Receipts, monthly report, flag oddities |
| talent | Talent Scout | Browser, Gmail | Watch people; draft intros; hold for review |
| social | Social Media | Browser | Draft from shipped work; never publish |
| ops | Ops / On-call | Terminal, Slack | Deploys/alerts; wake on real fires |

Users can also add a **blank** bot (prototype "Blank bot") and chip
whatever they want. After add, it is a normal crew row — edit, recolor,
remove (not Chief).

### What's in a template

Exactly the editor fields. Ship as JSON (or a tiny folder) in the app
bundle:

```text
id
name
color          # prototype cls: b-green, b-pink, …
instructions   # the textarea
tools[]        # catalog ids
```

Optional later, not MVP: default schedules, sample `MEMORY.md`, icon
beyond color blob.

Do not ship executable code per template. Do not ship CrewAI YAML.
Templates are **data**. The runtime is always thin-loop or ACP based on
`kind` (Code vs worker).

Ops' Terminal chip means "may start folded code sessions," not a second
shell runtime ([mcp-and-tools.md](mcp-and-tools.md)).

## Memory

### Decision

**Local-first files + our transcripts.** Each bot gets a directory the
model can read (and, for `MEMORY.md`, write via a narrow host tool).
Chat history lives in SQLite with everything else
([data-and-persistence](../data-and-persistence/brief.md) owns the
schema). Do not use Anthropic Managed Agents memory stores, cloud vector
DBs, or "the harness's `~/.claude` folder" for worker bots.

This copies the part of OpenClaw that is actually good
([memory](https://docs.openclaw.ai/concepts/memory.md)):
plain markdown, human-editable, no hidden state — not their Gateway.

Claude Code's own split is the same idea
([memory.md](https://code.claude.com/docs/en/memory.md)):
user-written instructions vs model-written notes. Use it at JaBot paths,
not inside `.claude/` for Inbox.

### Layout (illustrative)

```text
<jabot-data>/bots/<botId>/
  instructions.md     # user-edited; mirrors Crew textarea
  MEMORY.md           # bot-written durable facts; cap ~200 lines / 25KB
                      # (same budget Claude uses for auto-memory index)
  memory/YYYY-MM-DD.md  # optional daily log; not injected every turn
<jabot-data>/USER.md    # shared: voice, timezone, names, repos
```

Inject on every worker turn: `USER.md` + `instructions.md` + first N
lines of `MEMORY.md`. Daily files only if the bot searches / the host
includes "today."

Code sessions keep using the **repo's** `CLAUDE.md` / harness memory.
Do not overwrite that with crew `MEMORY.md`. The Code bot's
`instructions.md` is extra persona ("never push to main"), passed into
ACP as available.

### Across chats

Yes. Opening Inbox Mgr tomorrow still has `MEMORY.md` ("user prefers
drafts, never auto-send"). The **thread** is also durable (our store).
Memory files are for facts that should survive a `/new` or a compacted
transcript.

Chief's memory is routing preferences ("fold migrations by default"),
not a copy of Gmail.

Workers do not read each other's `MEMORY.md`. Shared facts go in
`USER.md` or a Chief handoff brief.

### What we reject for MVP

- SQLite FTS / sqlite-vec as a requirement (OpenClaw's builtin index is
  fine *later* if `MEMORY.md` grows). Grep + injecting the cap is enough.
- Letting the thin loop have unconstrained `Write` to the whole disk.
  One `update_memory` host tool that patches `MEMORY.md`.
- Storing secrets in memory files.

## Schedules

### Decision

**In-process scheduler in the JaBot host.** A schedule is a row:

```text
id
botId
cron            # 5-field cron, local timezone
prompt          # user text the bot will receive
enabled
lastRunAt
lastStatus
```

When it fires: start/continue that bot's loop with `prompt` (plus
"this is a scheduled run"), then **fold to Inbox** on done / fail /
needs-you — same as a long code thread. Do not require the main pane to
be open.

Persist jobs in SQLite (with crew). The clock is
[`node-cron`](https://www.npmjs.com/package/node-cron) or the Rust
equivalent if the host is Tauri — **as long as the app (or a small
sidecar it owns) is running.**

### What MVP is not

**Not launchd as the job runner.** Launchd is the right *OS* tool to
**wake the app** later, not to speak MCP and draw Inbox. Community
writeups of "Claude Code on a LaunchAgent" spend most of their words on
PATH, GUI session, and silent failure. Our jobs need the host's OAuth
keychain, MCP clients, and permission UI. Keep the clock next to that.

**Not macOS EventKit** as the scheduler. EventKit is for *calendar
events*. Scheduler *the bot* uses Calendar MCP to change the user's
calendar. JaBot's cron is "run Inbox Mgr every weekday at 8." Different
layers. (A job *creating* an EventKit reminder is a product idea, not
the MVP clock.)

**Not OpenClaw's Gateway cron** as a dependency. Steal the idea: jobs
in SQLite, isolated run sessions, announce into chat
([cron-jobs](https://docs.openclaw.ai/automation/cron-jobs.md)).
Implement 20% of that.

**Not cloud routines** (Claude Code cloud schedules). This is a personal
desktop app; local-first.

### Sleep and missed runs

If the laptop is asleep, the job does not run until the app is alive
again. MVP policy: **run once on wake if the window was missed**, and
stamp the Inbox item "missed 8:00, ran 10:12." Do not pile five catch-up
Gmail sweeps.

Document it. A later LaunchAgent can `open` JaBot at 7:55; still our
process runs the bot.

### UI

Prototype has no Schedules view. Minimal: on the bot editor, "Run
daily at…" plus a list on Crew. Recurrence is cron; fancy calendars can
wait.

Chief may create a schedule via a host tool (`schedule_bot`) so "every
morning, Inbox Mgr" is spoken, not only clicked. The row is still data
the user can delete.

### Code vs workers on a timer

Scheduled **code**: spawn ACP, fold, Inbox — session-lifecycle.

Scheduled **worker**: thin loop, same Inbox. Playwright/Gmail OAuth must
already be connected; a cron cannot click Allow. If the grant is dead,
fail the job into Inbox as "needs you," do not pop OAuth at 3am.
