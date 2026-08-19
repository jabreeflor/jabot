# JaBot build plan → GitHub issues

Planned 2026-08-19 from the completed research in [`docs/research/`](research/README.md).
Each research topic's "What this unblocks" list became a GitHub issue; the three
forks the research deliberately left open became **decision issues** that gate
build work. Those forks are **settled** in
[`docs/decisions/issues-4-6.md`](decisions/issues-4-6.md).
The umbrella with the full dependency graph is
[#30](https://github.com/jabreeflor/jabot/issues/30).

## Decisions (settled — they gate the rest)

| Issue | Decision | Gates |
|---|---|---|
| [#4](https://github.com/jabreeflor/jabot/issues/4) | **Settled:** in-process Tauri host, hide-to-Dock, socket-shaped API; Quit persists and resumes. No launchd in MVP1. | scaffold, host API, supervisor |
| [#5](https://github.com/jabreeflor/jabot/issues/5) | **Settled:** thread overlay `active → folded → resurfaced → archived` plus a `runs` table; Inbox is a projection of run events. | schema, state machine, Inbox |
| [#6](https://github.com/jabreeflor/jabot/issues/6) | **Settled:** every bot is an ACP harness session; Buzz-style three-tier catalog + per-bot harness. No thin host LLM loop. | crew store, Chief, adapters |

## Build issues

| Issue | Title | Blocked by |
|---|---|---|
| [#7](https://github.com/jabreeflor/jabot/issues/7) | Scaffold: Tauri 2 workspace — Rust host + React 19/Vite renderer | #4 |
| [#8](https://github.com/jabreeflor/jabot/issues/8) | Host API: typed JSON-RPC protocol (socket-shaped, device-aware) | #4, #7 |
| [#9](https://github.com/jabreeflor/jabot/issues/9) | Data layer: host-owned SQLite + secrets vault in OS keychain | #5, #7 |
| [#10](https://github.com/jabreeflor/jabot/issues/10) | Harness adapter layer: ACP client + subprocess supervision | #7 |
| [#11](https://github.com/jabreeflor/jabot/issues/11) | UI port: jabot-classic.html → React components + CSS tokens | #7 |
| [#12](https://github.com/jabreeflor/jabot/issues/12) | Packaging: Developer ID signing, notarization, updater | #7 |
| [#13](https://github.com/jabreeflor/jabot/issues/13) | Harness catalog + Doctor (Claude/Codex/Pi, Hermes/OpenClaw, custom JSON) — used by every bot | #6, #10 |
| [#14](https://github.com/jabreeflor/jabot/issues/14) | Chat transcript renderer + persisted ACP transcript overlay | #9, #10, #11 |
| [#15](https://github.com/jabreeflor/jabot/issues/15) | Thread state machine + run ledger | #5, #9 |
| [#16](https://github.com/jabreeflor/jabot/issues/16) | Folder/repo registration | #9, #11 |
| [#17](https://github.com/jabreeflor/jabot/issues/17) | Crew store + CRUD + bot templates as data (includes per-bot `harness_id`) | #6, #9, #11 |
| [#18](https://github.com/jabreeflor/jabot/issues/18) | Tool/MCP framework: catalog, OAuth in keychain, per-bot allowlists | #9, #10 |
| [#19](https://github.com/jabreeflor/jabot/issues/19) | MVP2 — Device pairing: QR + SAS + revocable scoped grants | #8 |
| [#20](https://github.com/jabreeflor/jabot/issues/20) | Permission broker + prompt UI (`session/request_permission`) | #10, #14 |
| [#21](https://github.com/jabreeflor/jabot/issues/21) | Session supervisor: keep-alive, resume, crash & sleep recovery | #4, #10, #15 |
| [#22](https://github.com/jabreeflor/jabot/issues/22) | Inbox view on real data | #11, #15 |
| [#23](https://github.com/jabreeflor/jabot/issues/23) | Worktree manager: one host-owned worktree per concurrent thread | #10, #16 |
| [#24](https://github.com/jabreeflor/jabot/issues/24) | Chief of Staff bot: ACP harness session + host handoff/spawn/status tools | #10, #17, #18 |
| [#25](https://github.com/jabreeflor/jabot/issues/25) | Schedules: in-process cron jobs delivered to Inbox | #15, #17 |
| [#26](https://github.com/jabreeflor/jabot/issues/26) | Fold & Wait for Inbox wired to real sessions | #15, #20, #21, #22 |
| [#27](https://github.com/jabreeflor/jabot/issues/27) | Native notifications (UNUserNotificationCenter) | #22 |
| [#28](https://github.com/jabreeflor/jabot/issues/28) | Pull Requests view + thread↔PR linkage + Inbox PR cards | #11, #22, #23 |
| [#29](https://github.com/jabreeflor/jabot/issues/29) | MVP2 — Mobile Inbox client | #19, #22, #27 |

## Thinnest vertical slice

#4 → #7 → #10 + #11 → #14 → #20: one Claude Code thread rendered as chat with
real permission prompts. Everything else layers onto that spine.
