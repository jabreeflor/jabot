# Data layer: SQLite + OS keychain secrets

**Issue:** #9
**Status:** Implemented — `src-tauri/src/host/store/`

## What it is

All host state is owned by the host process and persisted to a single
SQLite database (`jabot.sqlite`, WAL mode) plus a secrets vault backed by
the OS keychain. This is the persistence layer underneath threads, runs,
crew, schedules, pull requests, permissions, and pairing.

## Why

The fold/run/Inbox model
([`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#5--fold--run--inbox-data-model))
requires durable, queryable state that survives Quit and reconstructs
Inbox and thread state on resume; secrets (OAuth tokens, API keys) must
never sit in plaintext SQLite rows.

## Requirements

1. One SQLite file (`jabot.sqlite`) in WAL mode owns all structured host
   state: threads, runs, inbox_events, crew, schedules, pull_requests,
   pairing grants, permissions (`src-tauri/src/host/store/models.rs`).
2. Schema changes go through versioned migrations
   (`src-tauri/src/host/store/migrate.rs`,
   `src-tauri/src/host/store/migrations/`) — no hand-edited schema at
   runtime, and migrations must be forward-only and idempotent to apply.
3. Secret bytes (OAuth tokens, harness credentials, pairing keys) are
   stored via `src-tauri/src/host/store/secrets.rs` in the OS keychain,
   never inline in the SQLite tables — SQLite may hold a reference
   (e.g. a keychain item id), not the secret itself.
4. `catalog.rs` persists the harness/tool catalog state; `overlay.rs`
   persists the thread fold/state overlay described in
   [thread-state-and-runs.md](thread-state-and-runs.md).
5. `handoff.rs`, `pairing.rs`, `permission.rs`, `pr.rs`, `schedule.rs`
   each own the storage for their respective feature (Chief handoffs,
   device pairing, permission grants, PR cards, schedules) behind a
   narrow module API — other code does not write raw SQL against these
   tables from outside `store/`.
6. `seed.rs` provides deterministic seed data for development/tests so
   the app can boot into a populated state without a live harness.
7. Store errors are typed (`error.rs`) and distinguishable from
   protocol-level errors so callers can tell "data layer failed" from
   "harness failed."
8. Every write that the Inbox depends on (a run transition) is committed
   to the store **before** any UI notification is sent — notification
   delivery failure must never lose a result (restated from the fold/run
   decision; enforced here because this is where the ordering is
   implemented).
