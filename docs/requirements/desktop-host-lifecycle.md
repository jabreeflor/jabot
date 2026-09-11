# Desktop host process & lifecycle

**Issues:** #4 (decision), #7 (scaffold), #21 (supervisor), #282 (Windows chrome)
**Status:** Implemented — `src-tauri/src/host/lifecycle/`, `src-tauri/src/host/supervisor/`, `src-tauri/src/main.rs`, `src-tauri/src/window.rs`

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

1. Closing the last window **hides to Dock** on macOS; the host process and any
   in-flight ACP child processes keep running. On Windows and Linux the
   title-bar close button **exits** — there is no system tray and no
   hide-to-tray (#282). Minimize keeps the process; the X does not.
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

## Window chrome

macOS keeps the overlay title bar, private API, and under-window vibrancy
in [`src-tauri/tauri.macos.conf.json`](../../src-tauri/tauri.macos.conf.json).
The shared [`tauri.conf.json`](../../src-tauri/tauri.conf.json) is a
portable decorated window; Windows merges
[`tauri.windows.conf.json`](../../src-tauri/tauri.windows.conf.json)
(`decorations: true`, `transparent: false`). `macos-private-api` stays
on the untargeted `tauri` dep because tauri-build's allowlist reads the
TOML (a Mac `cargo clippy --lib` fails without it once
`tauri.macos.conf.json` sets `macOSPrivateApi`). Windows/Linux never
*call* the API: `window.rs` returns before clearing the webview fill.

The renderer reads `window_chrome` and paints `data-window-chrome`.
Decorated chrome zeros `--titlebar-h` / `--traffic-lights-w` so agent
pills and the chat composer are not inset for traffic lights the OS
already drew. Glass tokens stay solid when vibrancy is unsupported
(#278 / #250). Mica/Acrylic parity is out of scope.

## Out of scope (MVP1)

- A LaunchAgent/launchd job that keeps agents running after Quit.
- A second physical host process.
- A Windows system tray, or Mica/Acrylic glass matching macOS vibrancy.
