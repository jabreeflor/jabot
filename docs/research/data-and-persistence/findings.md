# Data & Persistence — Findings

Researched August 2026 against SQLite docs, harness session-log layouts, OS
secret stores, and the prototype's data shapes. This file answers the five
questions in [`brief.md`](brief.md). Deep dives live in sibling files.

**Recommendation in one sentence:** Make **one SQLite file (WAL, single-writer
host process) the source of truth** for threads, crew, inbox, PRs, and
schedules; persist the ACP `session/update` stream we already consume as the
transcript overlay; keep native harness JSONL as a resume pointer only; put
secrets in the OS keychain — never plaintext in the store.

| Question | Short answer | Detail |
|---|---|---|
| 1. Store | SQLite WAL as SoT. Files only for export/debug. Not JSON-as-database. | [store.md](store.md) |
| 2. Transcript ownership | Overlay, do not mirror. Store ACP updates we saw + `nativeSessionRef`. | [store.md](store.md#transcript-ownership) |
| 3. Schema first pass | Thread overlay + `runs` + Inbox projection. Wait for Inbox is a fold policy. Bots have `harness_id`. | [schema.md](schema.md) |
| 4. Secrets | OS keychain (Electron `safeStorage` / Rust `keyring` + Security.framework). Never plaintext. | [secrets-and-sync.md](secrets-and-sync.md) |
| 5. Sync later | Single-writer now. UUIDs + `updated_at`. No CRDT. Litestream-class backup is the later door. | [secrets-and-sync.md](secrets-and-sync.md#sync-later) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Host-owned SQLite store** — one connection (or one writer + read-only
   extras), WAL + foreign keys, numbered migrations.    Binding: `rusqlite` (`bundled`, SQLite ≥ 3.51.3) —
   [app-shell](../app-shell/findings.md) picked Tauri 2. Schema is SQL
   either way if that fork ever flips.
   See [store.md](store.md).
2. **Thread + transcript overlay** — persist the adapter-design row
   (`threadId`, `harnessId`, `acpSessionId`, `nativeSessionRef`, `cwd`,
   runtime snapshot, state) plus append-only ACP `session/update` events.
   Do not parse Claude/Codex/Pi JSONL as our renderer input.
3. **Inbox as a projection** — query folded threads for Still sleeping;
   write `inbox_events` from `runs` transitions (done / failed / needs_you /
   lost). Wait for Inbox is `fold_policy` on the thread. See
   [#5](../../decisions/issues-4-6.md).
4. **Secrets vault** — `secret_refs` in SQLite, bytes in the OS store.
   Inject into MCP / adapter env at spawn; never write tokens into
   `threads.runtime_json`.
5. **Crew / folders / PRs / schedules CRUD** — first-pass DDL in
   [schema.md](schema.md). Enough to open the data-layer issue; bot-crew and
   git-and-prs still own product rules.

The thread **state machine** (`active → folded → resurfaced → archived`,
Wait for Inbox as `fold_policy`) matches
[session-lifecycle](../session-lifecycle/findings.md). `deleted` is a
tombstone (`deleted_at`), not a fifth live state. If lifecycle later
renames a reason enum, migrate the CHECK; do not fork a second state column.

## Prototype note

`prototypes/jabot-classic.html` already has the entities this store must
cover: sidebar **folders** of **code threads**, **Crew** bots (Chief +
templates + tools + color + instructions), **Inbox** (resurfaced vs still
sleeping), **Pull Requests**, right-click **Wait for Inbox / Archive /
Delete**, and a stub **schedules** count in the night prototype. None of
that is persisted today. The schema maps 1:1 onto those views.

## Sources

Primary docs, not secondary blogs, unless noted:

- SQLite WAL (incl. WAL-reset bug, fixed 3.51.3):
  [sqlite.org/wal.html](https://www.sqlite.org/wal.html)
- `better-sqlite3` WAL:
  [WiseLibs/better-sqlite3](https://github.com/WiseLibs/better-sqlite3),
  [docs/performance.md](https://github.com/WiseLibs/better-sqlite3/blob/HEAD/docs/performance.md)
- rusqlite: [github.com/rusqlite/rusqlite](https://github.com/rusqlite/rusqlite)
- sqlx SQLite:
  [SqliteConnectOptions](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)
- ACP session/load vs resume:
  [agentclientprotocol.com/protocol/session-setup](https://agentclientprotocol.com/protocol/session-setup)
- Claude transcripts: [code.claude.com/docs/en/sessions](https://code.claude.com/docs/en/sessions)
- Codex home / sessions: [developers.openai.com/codex/config-advanced](https://developers.openai.com/codex/config-advanced),
  [CODEX_HOME](https://developers.openai.com/codex/environment-variables)
- Pi JSONL:
  [pi.dev/docs/latest/sessions](https://pi.dev/docs/latest/sessions),
  [session-format.md](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md)
- Electron `safeStorage`:
  [electronjs.org/docs/latest/api/safe-storage](https://www.electronjs.org/docs/latest/api/safe-storage)
- Tauri Stronghold (not the MVP pick):
  [v2.tauri.app/plugin/stronghold](https://v2.tauri.app/plugin/stronghold/)
- Rust Keychain: [keyring-core](https://crates.io/crates/keyring-core),
  [security-framework](https://crates.io/crates/security-framework)
- Litestream (later backup, not multi-device):
  [litestream.io/how-it-works](https://litestream.io/how-it-works/)
