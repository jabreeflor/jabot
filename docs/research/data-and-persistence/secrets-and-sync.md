# Secrets and later sync

Locked: **secrets never plaintext in the store.** SQLite holds
`secret_refs` (id, kind, label, keychain account). The bytes live in the OS
credential store. Adapter `runtime_json.env` and MCP `env` snapshots must
be redacted before INSERT.

## Decision

```
JaBot host
  │  spawn MCP / harness
  │  inject env in memory
  ▼
OS credential store  ◄── account = secret_refs.account
  macOS Keychain
  Windows DPAPI / Credential Manager
  Linux Secret Service (or fail closed)

jabot.sqlite  ── contains refs only, never tokens
```

Reuse the user's existing `claude` / `codex login` / `gh auth` when those
tools already have a session on the machine. JaBot-managed secrets are for
**our** MCP connections (Gmail, GitHub app tokens we issued, etc.), not a
second copy of Anthropic's key.

## Keychain vs file vs Stronghold

| Mechanism | What it actually is | Verdict |
|---|---|---|
| **macOS Keychain** via Electron [`safeStorage`](https://www.electronjs.org/docs/latest/api/safe-storage) | Chromium OSCrypt: Keychain holds the encryption key; ciphertext can sit on disk | **Electron pick.** Use async `encryptStringAsync` / `decryptStringAsync`. Code-sign or macOS re-prompts on every build. |
| **macOS Keychain** via Rust [`security-framework`](https://crates.io/crates/security-framework) or [`keyring-core`](https://crates.io/crates/keyring-core) + `apple-native-keyring-store` | Generic password items, service = bundle id | **Tauri pick.** Prefer `keyring-core` so Windows/Linux are the same API. |
| **Tauri Stronghold** ([plugin docs](https://v2.tauri.app/plugin/stronghold/)) | IOTA encrypted snapshot file (`vault.hold`) unlocked by a **user password** (Argon2) | **No for MVP.** We would invent an app password the prototype never shows. Use Keychain, not a second vault password. |
| Encrypted file next to SQLite (SQLCipher, age, homemade AES) | Key-of-keys problem: where does that key live? | Only as a **Linux fallback** if Secret Service is missing — and then warn in Settings. Never the Mac default. |
| Plaintext `.env` in Application Support | Convenient | Forbidden. |

Electron note: `safeStorage` on Linux can silently use `basic_text` (hardcoded
password) when no wallet is present
([docs](https://www.electronjs.org/docs/latest/api/safe-storage)). Detect
`getSelectedStorageBackend() === 'basic_text'` and refuse to store Gmail
tokens until a real keyring exists, or show an explicit "unprotected on
this machine" opt-in. macOS Keychain is available; `isEncryptionAvailable()`
should be true.

Tauri note: community plugins (`tauri-plugin-keyring`,
`tauri-plugin-keyring-store`) wrap the same OS stores. Prefer a thin
`keyring-core` helper in the host over Stronghold's wallet-shaped API. We
are storing OAuth refresh tokens, not BIP39 seeds.

### Envelope vs native items

Two honest designs:

1. **Native items** — each secret is a Keychain generic password
   (`service = app.jabot`, `account = secret_refs.account`). SQLite has no
   ciphertext.
2. **Envelope** — one Keychain item holds a data-encryption key; SQLite
   stores ciphertext (`secret_refs` grows a `ciphertext BLOB` column).
   Electron `safeStorage` is this model.

For MVP (a handful of MCP tokens): **native items** on Tauri/Rust;
**envelope via `safeStorage`** on Electron (that API encrypts strings, it
does not give you a Keychain item per token unless we layer keytar-style
access on top — and keytar is the old path; Chromium's OSCrypt is the
supported one).

Host-side interface (illustrative):

```text
Secrets
  put(id, bytes)      -- creates/updates OS item; upserts secret_refs
  get(id) -> bytes    -- never logs, never writes to sqlite
  delete(id)          -- OS item + row
  export_env(id)      -- returns env pair for spawn, in memory only
```

Spawn path: load secret → set child env → drop the string. Do not persist
the child command line with the token interpolated.

## What is *not* a JaBot secret

- Claude / Codex / Pi CLI logins already on disk (`~/.claude`, `~/.codex`,
  Pi provider files). Point the adapter at the user's environment; do not
  scrape `auth.json` into our vault.
- Git: prefer `gh auth token` at call time over storing a PAT. If we must
  store a GitHub token we created, it goes through `Secrets.put`.
- Folder paths, thread titles, cron expressions — ordinary SQLite.

## Sync later {#sync-later}

Brief question 5: *any future multi-device story worth not designing
ourselves out of?*

**Keep the store single-writer. Do not put CRDTs in the schema.**

Local-first here means: the Mac is the source of truth, the UI never needs
the network to render Inbox, secrets never go to "our cloud" by default.
It does **not** mean cr-sqlite / Automerge / Electric on day one.

Do this now so later is possible:

| Do now | Why it helps later |
|---|---|
| UUID primary keys, not autoincrement | Merge/import without collisions |
| `updated_at` on every mutable row | Last-write-wins or "this row changed since cursor" |
| Append-only `transcript_events` with `(thread_id, seq)` | Logs merge; do not UPDATE history |
| `deleted_at` tombstones | Sync can see deletes |
| Host owns the file; UI is a client | [remote-and-mobile](../remote-and-mobile/brief.md) client/host split is the real multi-device path |
| Secrets out of SQLite | A future DB replica (Litestream, iCloud of the sqlite file, a NAS host) does not leak tokens |

Do **not** do now:

- `cr-sqlite` / ElectricSQL / PowerSync / Jazz. Those assume either a
  server or multi-master. We have neither, and Inbox fold-state is a poor
  CRDT (two phones folding the same thread).
- Putting `jabot.sqlite` in iCloud Drive. WAL + two machines = corruption.
  SQLite WAL [does not work over network filesystems](https://www.sqlite.org/wal.html).
- Designing device pairing inside this schema. That is remote-and-mobile
  (pairing codes, per-device keys). The store just needs to stay a file one
  host process writes.

When backup matters (not sync): **Litestream**-class WAL shipping
([how it works](https://litestream.io/how-it-works/)) to a user-owned
bucket or the NAS. Single-writer, replica is disaster recovery, restore is
replace-the-file. Fine later; do not run Litestream in MVP.

When a second device appears, the likely shape is **one bot-host** (the
Mac/NAS that already has the DB and the harnesses) and **thin clients**
(desktop UI, then phone). Clients do not each get a writable SQLite replica
of threads. That avoids CRDTs entirely. If we ever want a true replica on a
laptop that works offline, that is a new research pass — and `secret_refs`
still must not travel with the replica unless we wrap the whole DB in an
envelope key the user unlocked.

## Single-writer rules

1. Only the host process opens SQLite read-write.
2. A second JaBot launch detects the lock / a pid file and focuses the
   first window (or attaches as a client to the host).
3. Harness children never see the DB path.
4. Backup = `VACUUM INTO 'jabot-backup.sqlite'` or file copy after
   `wal_checkpoint(TRUNCATE)`, not a Finder copy of a live `-wal`.
5. Multi-device "sync" = talk to the host, or restore a backup onto a new
   host — not two writers.

## What we explicitly defer

- User-facing passphrase / Stronghold vault.
- Syncing secrets between devices (would need pairing keys from
  remote-and-mobile).
- Encrypting the whole SQLite file.
- CRDT columns, Electric, PowerSync, Turso.
