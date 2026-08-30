# Requirements

Feature-by-feature requirements for JaBot, derived from the settled build
plan ([`docs/plan.md`](../plan.md)), the architecture decisions
([`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md)), and the
current implementation under `src/` and `src-tauri/`.

Each file below documents one feature: what it is, why it exists, what it
must do, and how it's verified. Where a feature traces to a GitHub issue,
the issue number is noted so it can be cross-referenced against
[`docs/plan.md`](../plan.md).

## Index

| File | Feature | Issue(s) |
|---|---|---|
| [desktop-host-lifecycle.md](desktop-host-lifecycle.md) | Desktop host process, window/Dock/Quit lifecycle | #4, #7, #21 |
| [host-api-protocol.md](host-api-protocol.md) | Typed JSON-RPC host API | #8 |
| [data-layer-persistence.md](data-layer-persistence.md) | SQLite data layer + OS keychain secrets | #9 |
| [harness-adapter-layer.md](harness-adapter-layer.md) | ACP harness adapters + catalog/Doctor | #10, #13 |
| [ui-shell.md](ui-shell.md) | React/TS UI shell ported from the HTML prototype | #11 |
| [packaging-distribution.md](packaging-distribution.md) | macOS signing, notarization, updater | #12 |
| [chat-transcript.md](chat-transcript.md) | Chat transcript renderer + persisted overlay | #14 |
| [thread-state-and-runs.md](thread-state-and-runs.md) | Thread overlay states + run ledger | #5, #15 |
| [folder-repo-registration.md](folder-repo-registration.md) | Folder/repo registration | #16 |
| [crew-management.md](crew-management.md) | Crew store, bot templates, "every bot is a harness" | #6, #17 |
| [tools-mcp-framework.md](tools-mcp-framework.md) | Tool/MCP catalog, OAuth, per-bot allowlists | #18 |
| [device-pairing.md](device-pairing.md) | QR + SAS device pairing, scoped grants (MVP2) | #19 |
| [permission-broker.md](permission-broker.md) | Permission prompts for tool/file access | #20 |
| [inbox.md](inbox.md) | Inbox view of run events | #22 |
| [worktree-manager.md](worktree-manager.md) | Per-thread git worktree manager | #23 |
| [chief-of-staff-bot.md](chief-of-staff-bot.md) | Chief of Staff bot + handoff/spawn/status tools | #24 |
| [schedules.md](schedules.md) | In-process cron schedules delivered to Inbox | #25 |
| [fold-and-wait.md](fold-and-wait.md) | Fold & "Wait for Inbox" wired to real sessions | #26 |
| [native-notifications.md](native-notifications.md) | macOS native notifications | #27 |
| [pull-requests.md](pull-requests.md) | Pull Requests view + thread↔PR linkage | #28 |
| [mobile-inbox.md](mobile-inbox.md) | Mobile Inbox client (MVP2) | #29 |

## How to read these

- **Status** reflects what's observably true in the repo today (code under
  `src/`, `src-tauri/src/`, or tests under `src/__tests__/` /
  `src-tauri/tests/`), not aspiration.
- **Requirements** are numbered, testable statements — the kind a test
  could assert against.
- Cross-cutting decisions (what a "bot" is, the fold/run/Inbox model, host
  process policy) are decided once in
  [`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md) and referenced
  rather than restated.
