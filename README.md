# JaBot

Bot-crew messenger UI prototypes. Wraps coding TUIs (Claude Code, Codex, Pi, or bring-your-own harness) in a chat-first interface with a Chief of Staff bot, folding "disappearing" threads, and an Inbox where long-running tasks resurface.

## Desktop app (Tauri 2)

The scaffold (#7) lives at the repo root:

- **Host:** `src-tauri/` — Rust supervisor inside the Tauri binary
- **Renderer:** `src/` — React 19 + TypeScript + Vite

```bash
npm install
npm run tauri dev    # macOS dev (requires Tauri prerequisites)
npm run build        # frontend-only build (CI / Linux)
```

macOS MVP: overlay title bar, hide-to-Dock on window close (#4). The renderer talks **JSON-RPC 2.0** to the Rust host (`host_rpc` + `host-rpc` events) — same messages a Unix socket will carry later (#8). Thread overlay, crew, and Inbox live in host-owned **SQLite** (`jabot.sqlite`, WAL); secret bytes stay in the **OS keychain** (#9).

## Prototypes

Open `prototypes/jabot-classic.html` in a browser — the main MVP (chat, Inbox, Pull Requests, thread sessions, New Chat with harness picker, Crew management).

Build plan and settled architecture decisions (#4 host/quit, #5 fold/run/Inbox, #6 every bot is a harness): [`docs/plan.md`](docs/plan.md), [`docs/decisions/issues-4-6.md`](docs/decisions/issues-4-6.md).

Other prototypes in `prototypes/` are earlier design directions.
