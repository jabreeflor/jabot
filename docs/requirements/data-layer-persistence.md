# Data layer: SQLite + OS credential-store secrets

**Issue:** #9, [#283](https://github.com/jabreeflor/jabot/issues/283) (Windows)
**Status:** Implemented — `src-tauri/src/host/store/`

## What it is

All host state is owned by the host process and persisted to a single
SQLite database (`jabot.sqlite`, WAL mode) plus a secrets vault backed by
the OS credential store. This is the persistence layer underneath threads,
runs, crew, schedules, pull requests, permissions, and pairing.

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
   stored via `src-tauri/src/host/store/secrets.rs` in the OS credential
   store, never inline in the SQLite tables — SQLite may hold a reference
   (e.g. a service/account id), not the secret itself. `put` / `get` /
   `delete` are the same host APIs on macOS and Windows. A denied
   credential-store prompt is [`StoreError::SecretsDenied`] (logged and
   returned), not a silent miss. Linux has no OS backend yet and fails
   closed (`SecretsUnavailable`) unless `JABOT_SECRETS_BACKEND=memory`.
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

## Where secret bytes live

| Platform | Store | How to find them |
|---|---|---|
| **macOS** | Keychain generic password, service `com.jabot.app` (overridable with `JABOT_KEYCHAIN_SERVICE`) | Keychain Access → login keychain → service `com.jabot.app` |
| **Windows** | Credential Manager generic credential (`keyring` `windows-native`). Target name is `{account}.{service}`, e.g. `jabot.secret.<id>.com.jabot.app`. Wincred blob cap is 2560 bytes; `set_password` stores UTF-16 (~1280 ASCII chars). Oversize is `StoreError::SecretsTooLong`. | Control Panel → Credential Manager → **Windows Credentials** |
| **Linux / CI** | None. `host/hello` reports `secretsBackend: "unavailable"` | Use `JABOT_SECRETS_BACKEND=memory` only for tests; bytes die with the process |

Acceptance isolation (`JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.<id>`) applies on both macOS and Windows so a probe never reads or writes the user's production items. The live OS round-trip is `os_secret_round_trip_put_get_delete` in `secrets.rs`; `scripts/windows-secrets-check.sh` is the named Windows entry (#286 can invoke it on a Windows runner).
