# Store: SQLite as source of truth

Opinionated pick for a personal desktop app that already decided ACP is the
harness dialect ([harness-integration](../harness-integration/findings.md)).

## Decision

```
JaBot host process  ──single writer──►  jabot.sqlite   (WAL)
        │                                      ▲
        │ overlay: ACP session/update          │
        │ row: thread + nativeSessionRef       │
        ▼                                      │
 harness JSONL (Claude / Codex / Pi)     files only as
 owned by the harness; we point at it    export / debug dumps
```

- **SQLite is the source of truth** for everything JaBot owns: threads,
  folders, bots, inbox events, PR links, schedules, harness catalog,
  secret *references*.
- **Files are not a database.** Optional JSONL/markdown dumps under the app
  data dir (or a user-picked folder) for debugging and "export this thread."
- **Do not dual-write** the live UI to both SQLite and a folder of JSON
  files. That is how you get two truths after a crash.

Crew config is tiny. Transcripts are append-heavy. SQLite does both: a
`bots` row is an UPDATE; a `transcript_events` insert is an append. WAL
makes the append cheap and crash-safe without rolling our own journal.

## Why not "plain files"

| Approach | Crash story | Query story |
|---|---|---|
| One JSON file per thread | Mid-write truncation unless we do write-temp-fsync-rename every event (slow, easy to get wrong) | Inbox / folder lists become a directory walk |
| Append-only JSONL per thread (Pi/Claude style) | Good for the stream; bad for "this thread is folded, title is X, PR is #23" | Need a second index anyway |
| Markdown notes | Nice export; not a store | Same |

Harnesses already use JSONL. Copying that pattern for *our overlay* would
mean we still need a queryable index for the sidebar, Inbox, and Crew.
That index is SQLite. Stop at one store.

SQLite's own pitch for WAL ([sqlite.org/wal.html](https://www.sqlite.org/wal.html)):
commits append a record to the WAL; readers keep going; a crash recovers
from the WAL on next open. That is the crash model we want for
`session/update` storms.

Caveats we accept:

- WAL needs a local disk, not NFS/SMB. App-support dir on the Mac is fine.
- Do not put the DB on iCloud Drive / Dropbox as a "sync" hack.
- WAL does not work well as a "document the user double-clicks." We are an
  app with an Application Support file, not a `.jabot` document format.

## Crash safety (pragmas)

On every connection, in this order:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;     -- FULL if we ever see lost commits after power loss
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
```

`NORMAL` + WAL is the better-sqlite3 default
([docs/performance.md](https://github.com/WiseLibs/better-sqlite3/blob/HEAD/docs/performance.md))
and is the usual desktop trade: a process crash is safe; an OS crash in a
tiny window after COMMIT can lose the last transaction. For a chat overlay
that can `session/load` to rebuild, that window is acceptable. Thread *state*
transitions (fold / resurface) should still be their own short transactions
so we do not lose "this is in Inbox" while keeping the last bubble.

Application rules:

1. **One writer process** — the host / session supervisor, not the UI
   renderer and not each harness child. UI talks to the host; host talks to
   SQLite. Matches Buzz's desktop-vs-`buzz-acp` split.
2. **Short transactions.** One `session/update` (or a small batch of chunks
   for the same tool call) per COMMIT. Do not hold a transaction open across
   a permission prompt.
3. **Do not share one WAL file across two SQLite libraries.** Mixing
   `better-sqlite3` and `rusqlite` on the same file has produced
   `SQLITE_CORRUPT` in the wild when each library checkpoints the other's
   frames. Pick one binding in the host. If a future native helper must
   touch the same DB, checkpoint + close before handoff, or don't.
4. **Checkpoint on clean shutdown:** `PRAGMA wal_checkpoint(TRUNCATE);`
   then close. Idle autocheckpoint (SQLite default 1000 pages) is enough
   while running.
5. **Integrity on open after an unclean exit:** `PRAGMA integrity_check`
   (or `quick_check` if the DB grows). Fail loud; offer "restore from last
   export" later, not silent repair.
6. **Pin SQLite ≥ 3.51.3.** The WAL-reset bug
   ([sqlite.org/wal.html §11](https://www.sqlite.org/wal.html)) is rare
   (needs two connections racing a checkpoint) but it corrupts. It is
   fixed in 3.51.3 (2026-03-13). rusqlite 0.38.0's `bundled` feature was
   still 3.51.1 — **verify the bundled version at implement time** and bump
   until ≥ 3.51.3. Single-writer makes the bug even less likely; still
   take the patched library.

## Bindings (Electron vs Tauri)

[app-shell](../app-shell/brief.md) has not picked the shell. The **schema
does not care**. The **process that owns the file** does.

| Shell | Binding | Why |
|---|---|---|
| **Electron / Node host** | [`better-sqlite3`](https://github.com/WiseLibs/better-sqlite3) | Sync API, WAL-friendly, worker_threads for heavy FTS later. Do not use `sql.js` or the old async `sqlite3` driver as the writer. |
| **Tauri / Rust host** | [`rusqlite`](https://github.com/rusqlite/rusqlite) with `features = ["bundled"]` | Thin, sync, full SQLite (FTS5, JSON1). Desktop host is not a web server; we do not need async. |
| Tauri but already on sqlx | [`sqlx`](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html) `SqliteJournalMode::Wal` + `sqlx::migrate!` | Fine if the crate already uses sqlx. Async pool of many writers is the wrong shape — keep `max_connections` tiny (1 writer + a couple readers). Apply WAL via connect options, not a migrated `PRAGMA` inside a transaction. |

**Recommend rusqlite (Tauri) or better-sqlite3 (Electron), not both, not
sqlx-unless-already-there.** sqlx's value is compile-time checked SQL and
Postgres dual-backend. JaBot has no Postgres. Migrations can be a numbered
`schema_migrations` table and a folder of `.sql` files in either binding.

If the eventual architecture is **UI process + bot-host daemon**
([remote-and-mobile](../remote-and-mobile/brief.md)), the daemon owns
SQLite. The UI never opens the file. That is the option that stays
single-writer when a phone or a second Mac appears.

## Transcript ownership {#transcript-ownership}

Locked: *do not invent a fourth JSON event format; transcripts can be ACP
session/update streams or references to harness logs.*

Harnesses already persist:

| Harness | On-disk log | Official stance |
|---|---|---|
| **Claude Code** | `~/.claude/projects/<dash-encoded-cwd>/<session-id>.jsonl` ([sessions docs](https://code.claude.com/docs/en/sessions)) | Format is **internal and changes**. Parse at your own risk. Prefer `/export`, hooks' `transcript_path`, or the Agent SDK. 30-day cleanup via `cleanupPeriodDays`. |
| **Codex** | Under `CODEX_HOME` (default `~/.codex`): date-partitioned `sessions/YYYY/MM/DD/rollout-…-uuid.jsonl`, plus SQLite indexes (`state_*.sqlite`). [config-advanced](https://developers.openai.com/codex/config-advanced), [CODEX_HOME](https://developers.openai.com/codex/environment-variables) | Rollout JSONL is how Codex resumes. Schema drifts. App-server / `thread/resume` is the supported API. |
| **Pi** | `~/.pi/agent/sessions/--<cwd-with-slashes-as-dashes>--/<timestamp>_<uuid>.jsonl` ([sessions](https://pi.dev/docs/latest/sessions), [session-format](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md)) | Documented JSONL, tree via `id`/`parentId`, currently v3. Still *Pi's* file. RPC `get_state.sessionFile` is the pointer. |

**Do not mirror those files into SQLite.** Mirroring means: copy every
native line, keep it in sync, survive their retention (Claude's 30-day
wipe), and parse three unstable dialects. That is a fourth event format in
disguise, plus a cleanup race.

**Do overlay:**

1. While the ACP connection is live, **append every `session/update` we
   already handle** into `transcript_events` (JSON payload = the ACP
   notification, not a JaBot-specific bubble schema). The chat renderer
   already maps ACP → prototype bubbles
   ([adapter-design.md](../harness-integration/adapter-design.md)).
   Replaying our table is the same mapper.
2. Store `native_session_ref` (Claude uuid, Codex thread id + optional
   rollout path, Pi `sessionFile`) so we can resume if the ACP adapter is
   swapped or our overlay is empty.
3. On reopen:
   - Overlay present → `session/resume` (no replay) and render from SQLite.
   - Overlay missing/corrupt and agent advertises `loadSession` →
     `session/load`, which **replays ACP updates**; persist those as we
     receive them.
   - Neither → native resume via `native_session_ref`, then catch up the
     overlay from whatever the adapter emits.
4. Compaction / thinking / tool guts that ACP does not send stay in the
   harness log. We do not need them for Inbox or the prototype renderer.

This is "ACP stream or a reference," not a new dialect.

### Append-heavy implementation notes

`transcript_events` is insert-only, keyed by `(thread_id, seq)`. `seq` is
monotonic per thread (not global). Streaming token chunks can either:

- **Coalesce in memory** and flush on `message_end` / idle / every N ms
  (fewer rows, more loss on crash), or
- **Insert every chunk** (crash-safe, fatter). Prefer this for tool output
  that the user might fold away; coalesce *agent text* if profiling says so.

Do not UPDATE previous rows to "rebuild the bubble." Treat the table as a
log. The renderer reduces it. If ACP later replaces tool content (it does:
a later `tool_call_update` with `content` replaces chunks), store the
replacement as a new event with the same `toolCallId`; the reducer knows
the rule.

FTS later: `transcript_fts` (FTS5) on user/agent text only, updated in the
same transaction as the event insert. Not MVP.

### What we explicitly do not store

- Full copies of `~/.claude/projects/**/*.jsonl` / Codex rollouts / Pi
  session files.
- Harness stderr logs (keep those as rotating files next to the DB if the
  supervisor wants them).
- Filesystem diffs / git objects — git-and-prs owns the worktree; we store
  a PR *link*.

## On-disk layout (host)

macOS:

```
~/Library/Application Support/JaBot/
  jabot.sqlite
  jabot.sqlite-wal
  jabot.sqlite-shm
  logs/                 # supervisor / adapter stderr
  exports/              # user-triggered dumps, not live
```

Linux: `~/.local/share/jabot/` (or XDG). Windows: `%APPDATA%\JaBot\`.

Never next to the `.app` bundle (updates wipe it). Never in the repo cwd.

Optional debug dump (Settings → "Export thread"): write
`exports/<threadId>.acp.jsonl` — one ACP update per line. That is an
export, not a second live copy.

## Migrations

`schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT)`.

Ship numbered SQL files (`0001_init.sql`, …). Run in a single transaction
per file except PRAGMAs (WAL cannot be set inside a sqlx migration
transaction — apply pragmas in connect code).

First migration creates every table in [schema.md](schema.md). Later
lifecycle-state renames are `ALTER` + CHECK rebuild, not a new database.

## What we explicitly defer

- SQLCipher / encrypted DB. Keychain covers secrets; the rest is local
  user data. Revisit if the DB ever leaves the machine.
- SQLite as a CRDT (`cr-sqlite`). See [secrets-and-sync.md](secrets-and-sync.md).
- Sharing the file with a second JaBot instance. Second instance is
  read-only or "app already open."
- Storing blobs (screenshots, large bash output) in SQLite. Cap tool-output
  rows; spill oversized payloads to `logs/tool-output/<eventId>` if needed.
