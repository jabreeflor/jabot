# Data & Persistence

Where everything lives: threads, transcripts, crew config, inbox state,
schedules. Local-first — this is a personal desktop app.

Depends on: shapes decided in
[harness-integration](../harness-integration/brief.md) and
[session-lifecycle](../session-lifecycle/brief.md). Prior art:
[setup-porting](../setup-porting/findings.md) (per-agent SQLite, Hermes
`state.db`, Buzz persist-then-notify — local-first SQLite + keychain, not
Postgres/Redis).

## Questions to answer

1. **Store** — SQLite vs plain files (JSON/markdown) vs both. Transcripts are
   append-heavy streams; crew config is tiny. What's simplest that survives
   crashes?
2. **Transcript ownership** — harnesses keep their own session logs (e.g.
   Claude Code's `~/.claude/projects/*.jsonl`). Do we mirror into our store, or
   reference theirs and store only our overlay (thread state, inbox items)?
3. **Schema first pass** — threads, folders, bots, inbox items, PR links,
   schedules. Draft it once lifecycle states are known.
4. **Secrets** — tool auth tokens (Gmail, GitHub…): macOS Keychain vs
   file-based. Never plaintext in the store.
5. **Sync (later)** — any future multi-device story worth not designing
   ourselves out of? (Probably: keep the store single-writer, decide later.)

## What this blocks (future issues)

- Data layer + schema
- Migration story
- Secrets handling
