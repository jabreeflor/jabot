# App Shell — Findings

Researched August 2026 against current public docs, similar tools, and the
locked harness-integration decisions. This file answers the five questions in
[`brief.md`](brief.md). Deep dives live in sibling files.

**Recommendation in one sentence:** Ship JaBot as a **macOS-only Tauri 2 app**
with a **React 19 + TypeScript + Vite** UI and a **Rust host** that owns ACP
adapter subprocesses (`std::process` + [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)),
hide-to-Dock rather than a launchd daemon for MVP, and defer PTY/`xterm.js`
until a raw-terminal escape hatch is actually needed.

| Question | Short answer | Detail |
|---|---|---|
| 1. Electron vs Tauri | Tauri 2. Closest peers (Buzz, Conductor) already did. Electron wins if the *host* is TypeScript; ours is a process supervisor. | [electron-vs-tauri.md](electron-vs-tauri.md) |
| 2. UI stack | React 19, not Svelte/Solid/vanilla. Port `jabot-classic.html` as CSS tokens + components, not as a single file. | [ui-stack.md](ui-stack.md) |
| 3. Process architecture | UI webview ↔ in-process Rust host ↔ one ACP subprocess per live thread. No launchd daemon for MVP. | [process-architecture.md](process-architecture.md) |
| 4. PTY needs | None for MVP. Stdio ACP. Later: `portable-pty` + `xterm.js` as a fourth runtime type, not as chat bubbles. | [process-architecture.md](process-architecture.md#pty) |
| 5. Packaging & updates | macOS-only MVP. Developer ID + notarize, **not** App Store. Tauri updater; Sparkle only if the dialog matters. | [process-architecture.md](process-architecture.md#packaging) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Repo scaffold** — Tauri 2 workspace: `src-tauri` (Rust host + ACP client)
   and a Vite/React renderer. See [electron-vs-tauri.md](electron-vs-tauri.md).
2. **Port of jabot-classic.html** — CSS custom properties stay; markup becomes
   React components (sidebar, chat, Inbox, PRs, New Chat, crew). See
   [ui-stack.md](ui-stack.md).
3. **Session daemon / process supervisor** — in-process Rust host for MVP,
   with a socket-shaped API so it can detach later. Shared with
   [session-lifecycle](../session-lifecycle/brief.md). See
   [process-architecture.md](process-architecture.md).

Also feeds [remote-and-mobile](../remote-and-mobile/findings.md): the local
case is already "UI client + host that owns sessions." Remote is the same
protocol over the network; do not wait for ACP HTTP.

**Fork (recorded, not reopened here):** remote-and-mobile wants that host
to be a **separate OS daemon in MVP1**. This topic says **in-process +
socket-shaped API**, extract a sidecar when a second client exists. See
the [research README](../README.md#open-fork-physical-host) for the
recommended issue-writing resolution.

## Locked constraints (from harness-integration)

Do not relitigate these in the shell:

- Speak ACP over stdio. Do not PTY-wrap TUIs for MVP.
- Host/session supervisor owns subprocesses.
- Custom harness = ACP-speaking command.
- Raw PTY escape hatch is deferred (this folder, not adapter).
- Sessions must survive UI close (hide the window; keep the host).

## Prototype note

`prototypes/jabot-classic.html` is a **visual contract**, not a runtime.
Traffic-light window chrome, SF-style type, iMessage bubbles, and the
sidebar/Inbox/PRs layout should survive the port. The fake window chrome
goes away — Tauri draws a real `titleBarStyle: overlay` window. Do not
preserve the vanilla event-handler soup.

## Sources

Primary docs, not secondary blogs, unless noted:

- Tauri 2: [sidecar](https://v2.tauri.app/develop/sidecar/),
  [shell plugin](https://v2.tauri.app/plugin/shell/),
  [updater](https://v2.tauri.app/plugin/updater/),
  [macOS signing](https://v2.tauri.app/distribute/sign/macos/)
- Electron: [utilityProcess](https://www.electronjs.org/docs/latest/api/utility-process),
  [autoUpdater](https://www.electronjs.org/docs/latest/api/auto-updater),
  [updates tutorial](https://www.electronjs.org/docs/latest/tutorial/updates)
- ACP Rust SDK: [crates.io/agent-client-protocol](https://crates.io/crates/agent-client-protocol),
  [agentclientprotocol/rust-sdk](https://github.com/agentclientprotocol/rust-sdk)
- PTY: [microsoft/node-pty](https://github.com/microsoft/node-pty),
  [portable-pty](https://docs.rs/portable-pty/latest/portable_pty/),
  [xterm.js](https://github.com/xtermjs/xterm.js)
- Apple: [Notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution),
  [Hardened Runtime](https://developer.apple.com/documentation/security/hardened-runtime)
- Similar tools: [block/buzz](https://github.com/block/buzz),
  [conductor.build](https://conductor.build/),
  [OpenCode → Electron](https://dev.to/brendonovich/moving-opencode-desktop-to-electron-4hip),
  [Warp](https://www.warp.dev/blog/how-warp-works)
