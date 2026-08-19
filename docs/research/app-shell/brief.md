# App Shell

What JaBot is built in. The prototype is a mac-style desktop window; the real
app must spawn and supervise long-lived local processes (harness sessions),
so it's a desktop app first.

Constrained by: [harness-integration](../harness-integration/brief.md) — the
adapter layer decides how much process/PTY machinery the shell must own.
Prior art: [setup-porting](../setup-porting/findings.md) (OpenClaw LaunchAgent
host, Hermes Electron+`hermes serve`, Buzz Tauri supervisor — daemon split
first, Electron vs Tauri second).

## Questions to answer

1. **Electron vs Tauri** — for an app that spawns many child processes / PTYs,
   streams their output, and needs small memory per idle session. Tauri (Rust
   core) vs Electron (Node core, `node-pty` maturity). What did similar tools
   pick (Buzz, Conductor, other agent-manager apps)?
2. **UI stack** — React vs Svelte vs keep-it-vanilla. The prototype is one
   vanilla file; what do we gain/lose porting it?
3. **Process architecture** — UI process vs a separate daemon that owns
   sessions (so the UI can close while sessions run). One process per session?
4. **PTY needs** — if any harness requires PTY wrapping, which PTY lib per
   shell choice, and do we ever need to show a raw terminal escape-hatch view?
5. **Packaging & updates** — signing/notarizing on macOS, auto-update. macOS
   only for MVP?

## What this blocks (future issues)

- Repo scaffold (shell + UI framework)
- Port of jabot-classic.html into real components
- Session daemon / process supervisor (shared with session-lifecycle)
