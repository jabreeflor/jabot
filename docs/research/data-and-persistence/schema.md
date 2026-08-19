# Schema first pass

Research draft, not a migration to paste blindly. Types are SQLite.
Timestamps are ISO-8601 UTC text (`strftime('%Y-%m-%dT%H:%M:%fZ')`) so they
survive language bindings. IDs are UUID text.

**Aligned with [session-lifecycle](../session-lifecycle/findings.md).**
Thread `state` and Inbox triggers below match that overlay. "Stuck" is a
`resurfacedReason`, not a fifth state. If lifecycle later renames a reason
enum, migrate the CHECK; do not add a parallel column.

## State machine (draft)

Thread `state`:

```
          new / reopen
              │
              ▼
          ┌────────┐  fold / Wait for Inbox   ┌────────┐
          │ active │ ───────────────────────► │ folded │
          └───┬────┘                          └───┬────┘
              │ reopen from Inbox                 │ done / fail / needs you
              │                                   ▼
              │                            ┌────────────┐
              └────────────────────────────│ resurfaced │
                                           └─────┬──────┘
                     archive                     │ archive
              ┌──────────┐                       │
              │ archived │ ◄─────────────────────┘
              └──────────┘
                     │ delete
                     ▼
                 (tombstone, then purge)
```

| State | Sidebar | Inbox | Process (lifecycle owns this) |
|---|---|---|---|
| `active` | Yes | No (unless also has an unread event — shouldn't) | Live or resumable |
| `folded` | No | "Still sleeping" | Keep subprocess if working; else checkpoint |
| `resurfaced` | No | "Resurfaced" | Idle; waiting for human |
| `archived` | No | No | Closed; keep overlay for history |

**Wait for Inbox is not a state and not a table.** It is `fold_policy =
'wait_for_inbox'` on the thread: same `folded` row, different permission
preset (auto-allow reads; execute/delete still prompt; unanswered execute
while folded → resurface as `needs_you`). Matches
[adapter-design.md](../harness-integration/adapter-design.md#permissions).

Right-click **Archive** → `archived`. **Delete** → `session/close` then
`session/delete` if advertised, then tombstone (`deleted_at`) and stop
rendering; purge overlay after a grace period. **Reopen** from Inbox →
`active`.

Inbox **cards** are not a second copy of the thread. The Resurfaced /
Still sleeping sections are queries on `threads.state`. `inbox_events` is
the audit / notification log (and the expanded "1 judgment call" list).

## ER sketch

```
folders 1──* threads 1──* transcript_events
                │
                ├──* inbox_events
                ├──* thread_prs
                └──* permission_decisions

bots 1──* threads          (nullable: Chief chats vs code threads)
bots 1──* schedules
bots *──* bot_tools        (or tools_json on bots — MVP: JSON)

harnesses  (catalog; threads.harness_id)

secret_refs  (pointers only; bytes live in the OS store)
```

## DDL

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE schema_migrations (
  version    INTEGER PRIMARY KEY,
  applied_at TEXT    NOT NULL
);

-- Sidebar folders. Prototype: JABOT-APP, GLOBNET-SYNC.
-- path is the cwd we pass to session/new. Unique so two folders
-- cannot claim the same repo.
CREATE TABLE folders (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  path        TEXT NOT NULL UNIQUE,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

-- Crew. Prototype CREW[]: chief, code, inboxm, sched, rsrch, writer.
-- tools_json: JSON array of tool ids (gmail, github, …).
-- secret_ref_id: optional pointer for that bot's bundled MCP auth.
CREATE TABLE bots (
  id              TEXT PRIMARY KEY,
  name            TEXT NOT NULL,
  color           TEXT NOT NULL,           -- prototype cls: b-teal, …
  instructions    TEXT NOT NULL DEFAULT '',
  tools_json      TEXT NOT NULL DEFAULT '[]',
  is_chief        INTEGER NOT NULL DEFAULT 0 CHECK (is_chief IN (0, 1)),
  template_id     TEXT,                    -- expense | talent | social | ops | NULL
  host_id         TEXT,                    -- NULL = this machine; remote-and-mobile later
  sort_order      INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);

CREATE UNIQUE INDEX bots_one_chief ON bots(is_chief) WHERE is_chief = 1;

-- Built-in + Custom harness catalog (Buzz-style JSON).
CREATE TABLE harnesses (
  id             TEXT PRIMARY KEY,         -- claude | codex | pi | user id
  label          TEXT NOT NULL,
  command        TEXT NOT NULL,
  args_json      TEXT NOT NULL DEFAULT '[]',
  env_json       TEXT NOT NULL DEFAULT '{}',  -- non-secret env only
  install_hint   TEXT,
  is_builtin     INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);

-- Adapter-design thread row, plus JaBot overlay.
CREATE TABLE threads (
  id                   TEXT PRIMARY KEY,   -- threadId
  folder_id            TEXT REFERENCES folders(id) ON DELETE SET NULL,
  bot_id               TEXT REFERENCES bots(id) ON DELETE SET NULL,
  harness_id           TEXT NOT NULL REFERENCES harnesses(id),
  acp_session_id       TEXT,               -- opaque; JaBot session key once assigned
  native_session_ref   TEXT,               -- Claude uuid / Codex thread id / Pi path
  cwd                  TEXT NOT NULL,
  runtime_json         TEXT NOT NULL,      -- { command, args, env } snapshot; env must not contain secrets
  title                TEXT NOT NULL,
  state                TEXT NOT NULL DEFAULT 'active'
                         CHECK (state IN ('active', 'folded', 'resurfaced', 'archived')),
  fold_policy          TEXT NOT NULL DEFAULT 'default'
                         CHECK (fold_policy IN ('default', 'wait_for_inbox')),
  last_stop_reason     TEXT,               -- ACP idle reason: end_turn | cancelled | error | …
  last_error           TEXT,
  preview              TEXT,               -- last agent line / Inbox subtitle
  created_at           TEXT NOT NULL,
  updated_at           TEXT NOT NULL,
  folded_at            TEXT,
  resurfaced_at        TEXT,
  archived_at          TEXT,
  deleted_at           TEXT                -- tombstone; NULL = live
);

CREATE UNIQUE INDEX threads_acp_session ON threads(acp_session_id)
  WHERE acp_session_id IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX threads_folder_state ON threads(folder_id, state) WHERE deleted_at IS NULL;
CREATE INDEX threads_inbox ON threads(state, resurfaced_at) WHERE deleted_at IS NULL;

-- Append-only ACP session/update (and permission request/response) log.
-- payload_json is the ACP notification/request object, not a JaBot DTO.
CREATE TABLE transcript_events (
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  seq           INTEGER NOT NULL,
  acp_method    TEXT NOT NULL,             -- session/update | session/request_permission | …
  payload_json  TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  PRIMARY KEY (thread_id, seq)
);

-- Inbox is mostly threads.state. This table is the notification log:
-- "Auth migration finished", "1 judgment call while you were away."
CREATE TABLE inbox_events (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL
                  CHECK (kind IN (
                    'folded', 'done', 'failed', 'needs_you',
                    'judgment_call', 'permission'
                  )),
  title         TEXT NOT NULL,
  summary       TEXT NOT NULL DEFAULT '',
  payload_json  TEXT,                      -- structured extras (files changed, PR number, decision)
  created_at    TEXT NOT NULL,
  read_at       TEXT,
  dismissed_at  TEXT
);

CREATE INDEX inbox_events_thread ON inbox_events(thread_id, created_at);

-- git-and-prs will fill this when a session opens a PR.
CREATE TABLE thread_prs (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  provider      TEXT NOT NULL DEFAULT 'github',
  repo          TEXT NOT NULL,             -- owner/name
  number        INTEGER NOT NULL,
  url           TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'open'
                  CHECK (status IN ('open', 'draft', 'merged', 'closed')),
  check_state   TEXT,                      -- pending | green | red — git-and-prs owns polling
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  UNIQUE (provider, repo, number)
);

CREATE INDEX thread_prs_thread ON thread_prs(thread_id);

-- Local scheduler (bot-crew). Cron in standard 5-field form.
CREATE TABLE schedules (
  id            TEXT PRIMARY KEY,
  bot_id        TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
  title         TEXT NOT NULL,
  cron          TEXT NOT NULL,
  prompt        TEXT NOT NULL,
  enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  last_run_at   TEXT,
  next_run_at   TEXT,
  last_thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE INDEX schedules_next ON schedules(enabled, next_run_at);

-- Remember allow/deny patterns per thread (ACP allow_always / reject_always).
CREATE TABLE permission_decisions (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  scope         TEXT NOT NULL CHECK (scope IN ('once', 'session', 'always')),
  kind          TEXT NOT NULL,             -- allow | reject
  subject_json  TEXT NOT NULL,             -- ACP subject snapshot (command + cwd, or tool_call)
  created_at    TEXT NOT NULL
);

-- Pointers only. ciphertext lives in Keychain / safeStorage / keyring.
-- See secrets-and-sync.md.
CREATE TABLE secret_refs (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL,             -- mcp | github | gmail | harness_env | …
  label         TEXT NOT NULL,
  account       TEXT NOT NULL UNIQUE,      -- OS keychain account, e.g. jabot.secret.<id>
  bot_id        TEXT REFERENCES bots(id) ON DELETE SET NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TABLE app_meta (
  key           TEXT PRIMARY KEY,
  value         TEXT NOT NULL
);
```

Seed `harnesses` with the three New Chat cards plus no Custom rows until the
user adds one. Seed `bots` with Chief of Staff. Folders are user-created
(register a repo path).

## How the prototype views query this

**Sidebar folders.** `folders` left join `threads` where `state = 'active'`
and `deleted_at IS NULL`. Status chip = derived: running if the supervisor
says the process is live; else `last_stop_reason` / PR status.

**Inbox → Resurfaced.** `threads` where `state = 'resurfaced'`. Expand uses
the latest `inbox_events` for that thread (done / needs_you / judgment_call
list). Badge count = those rows with `read_at IS NULL`.

**Inbox → Still sleeping.** `threads` where `state = 'folded'`. Prototype
`tagpill slp`. No extra entity.

**Inbox tabs (All / Needs you / Done).** Filter `inbox_events.kind` (and/or
`last_stop_reason`). Still-sleeping stays a section, not a tab.

**Crew.** `SELECT * FROM bots ORDER BY is_chief DESC, sort_order`. Edit
updates `name`, `color`, `instructions`, `tools_json`.

**Pull Requests.** `thread_prs` joined to `threads` / `folders`. Prototype
sections (needs review / checks running / merged) are `status` +
`check_state`.

**Schedules.** `schedules` joined to `bots`. MVP can be "enabled cron fires
`session/new` with `prompt` on that bot" — product rules stay in bot-crew.

## Transcript replay vs resume

| We have | On app restart |
|---|---|
| `transcript_events` nonempty | Render from SQLite; ACP `session/resume` with `acp_session_id` + `cwd` |
| Overlay empty, agent has `loadSession` | `session/load`; insert replayed updates as they arrive |
| Overlay empty, only native ref | Spawn adapter; native resume; fill overlay from live ACP |

Never SELECT from Claude/Codex/Pi JSONL in JaBot queries.

## What might change after session-lifecycle

Mark these as unstable:

- Exact `state` strings (e.g. they may want `sleeping` instead of `folded`).
- Whether a folded-but-still-running thread is a substate (`folded` +
  supervisor flag) or a fifth CHECK value. **Recommendation:** keep four
  states; "still working" is runtime, not durable — the host process table
  (PID, child) is ephemeral and must not live only in SQLite or it will lie
  after a crash. Persist `state=folded` and let the supervisor reconcile
  "process alive?" on boot.
- Judgment-call capture: `inbox_events.kind = 'judgment_call'` plus
  `payload_json` is enough if lifecycle agrees.

A live-process table, if needed:

```sql
-- Ephemeral. Truncate on boot, then reattach or mark threads folded.
CREATE TABLE runtime_sessions (
  thread_id     TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
  pid           INTEGER,
  started_at    TEXT NOT NULL,
  last_event_at TEXT
);
```

Do not treat `runtime_sessions` as durable truth.

## Size and retention

- Threads: thousands, fine.
- `transcript_events`: the hot table. A long Claude session can be tens of
  thousands of chunks if we store every token. Coalesce agent text if the
  DB crosses a few hundred MB; never coalesce permission events.
- Claude may delete *its* JSONL after `cleanupPeriodDays` (default 30). Our
  overlay does not expire with it. That is a reason not to depend on their
  files for the UI.
- Tombstoned threads: keep overlay for a while (app_meta
  `purge_deleted_after_days`, default 30), then `DELETE FROM threads` and
  cascade.

## What we explicitly defer

- FTS5 (`transcript_fts`). Add when search is a product feature.
- Per-host `hosts` table — `bots.host_id` is a nullable string until
  remote-and-mobile lands.
- Bot memory store (bot-crew question 5). If they want cross-chat memory,
  it is another table (`bot_memories`), not a reuse of `transcript_events`.
- Worktree paths — git-and-prs. A nullable `threads.worktree_path` is the
  only hook this schema needs later.
