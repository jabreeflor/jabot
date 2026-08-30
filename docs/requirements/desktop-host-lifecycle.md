# Desktop host process & lifecycle

**Issues:** #4 (decision), #7 (scaffold), #21 (supervisor)
**Status:** Implemented — `src-tauri/src/host/lifecycle/`, `src-tauri/src/host/supervisor/`, `src-tauri/src/main.rs`

## What it is

JaBot is a Tauri 2 desktop app: a Rust "host" process supervising ACP
harness subprocesses, talking to a React/TypeScript renderer over an
IPC channel shaped like a future Unix socket protocol. The host lives
**in-process** inside the Tauri binary for MVP1 — there is no separate
`jabot-host` daemon and no launchd agent.

## Why

Coding agents (Claude Code, Codex, Pi, etc.) are long-running subprocesses
with state (open threads, in-flight runs) that must survive window close
without turning into an unkillable background daemon a user didn't ask
for. The lifecycle policy in
[`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#4--physical-host-process-and-quit-policy)
draws the line between "hide" and "quit" explicitly so this doesn't get
relitigated per feature.

## Requirements

1. Closing the last window **hides to Dock**; the host process and any
   in-flight ACP child processes keep running.
2. Cmd-Q / Dock "Quit" **persists** the thread overlay (session ids,
   working directory, run state) to disk, then kills every ACP adapter
   **process group** (not just the parent PID).
3. On next launch, persisted threads are **resumed** via `session/resume`
   rather than reconnected to a live PID; any run left `running` at
   persist time surfaces as interrupted/stuck rather than silently lost.
4. Lid close, crash, and reboot are handled identically to Quit: state
   is recovered by resume on next boot, not by expecting the process to
   still exist.
5. The renderer never speaks ACP stdio directly — all harness I/O goes
   through the host. The IPC surface between renderer and host is
   designed as a request/response + event protocol so that extracting
   a standalone `jabot-host` sidecar later is a packaging change, not an
   API rewrite (see [host-api-protocol.md](host-api-protocol.md)).
6. The supervisor reconciles in-RAM "still working" state against the
   store on boot (`src-tauri/src/host/supervisor/boot.rs`,
   `resume.rs`) — "still working" is never a durable database enum, only
   a runtime fact reconstructed at startup.
7. `npm run tauri dev` boots the renderer + host for local development;
   `npm run build` produces a frontend-only build usable in CI/Linux
   where the native shell can't run.
8. A second client (phone, another Mac) is out of scope for MVP1; the
   design must not preclude extracting the in-process host into a
   sidecar speaking the same protocol over a real socket when that need
   arrives (MVP2, see [device-pairing.md](device-pairing.md)).

## Out of scope (MVP1)

- A LaunchAgent/launchd job that keeps agents running after Quit.
- A second physical host process.
