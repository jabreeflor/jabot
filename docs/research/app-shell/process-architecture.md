# Process architecture, PTY, packaging

Questions 3–5 from [`brief.md`](brief.md). Locked: host owns subprocesses;
ACP over stdio; sessions survive UI close; raw PTY is deferred.

## Target shape (MVP)

```
┌─ JaBot.app (one OS process for the shell) ─────────────────────┐
│  WKWebView  ←Tauri IPC / events→  Rust host (supervisor)       │
│                                         │                      │
│                    spawn stdio ACP      │  hide window ≠ quit  │
│                                         ▼                      │
│                          claude-agent-acp / codex-acp / …      │
│                                         │                      │
│                                         ▼                      │
│                          claude / codex / pi (grandchildren)   │
└────────────────────────────────────────────────────────────────┘
```

This is Buzz's local split without Buzz's relay:
[adapter-design.md](../harness-integration/adapter-design.md) already drew
`UI ←events→ Host ←ACP stdio→ adapter`. The shell decision is only *where
the host process lives*.

**MVP: host in-process with the Tauri binary.** Not a launchd daemon. Not
an Electron utility process. The Mac convention we want: closing the last
window **hides** JaBot (Dock stays, children stay). Cmd-Q / Dock Quit tears
the host down and we resume from ACP `sessionId` on next launch (handoff to
[session-lifecycle](../session-lifecycle/brief.md)).

## UI vs daemon

[remote-and-mobile](../remote-and-mobile/brief.md) asks whether MVP1 must
be client/server. **Logically yes, physically not yet.**

| Layer | MVP | Later (without rewriting the UI) |
|---|---|---|
| Renderer | Webview. No child processes. No fs. | Same, maybe a browser client. |
| Host API | Tauri commands + event emit. Treat it as if it were a socket. | Unix socket / localhost WebSocket, same messages. |
| Process owner | Rust code in `src-tauri`. | Sidecar `jabot-host` or a launchd agent. |
| Session survival on **window close** | Don't exit the app. | Same. |
| Session survival on **Quit / reboot** | Persist `acpSessionId` + cwd; `session/resume`. | Same, plus optional keep-alive daemon. |

Do **not** install a launchd agent for MVP.

- Users will not expect a coding-agent supervisor to outlive Quit.
- Hardened Runtime + a blessed helper is a packaging project of its own.
- Community Tauri "background service" plugins note **launchd agents are
  incompatible with App Sandbox**; we should not sandbox anyway (see
  packaging), but we also should not take on plist installers yet.
- Laptop sleep and crash recovery are **resume**, not "the PID lived."
  Claude/Codex/Pi already persist transcripts. ACP `session/resume` /
  `session/load` is the contract.

Happy Coder's CLI daemon is the right *pattern* for remote/mobile (a host
that the phone talks to). Steal the idea in MVP2; don't ship it in MVP1.

**Electron equivalent** if we had picked Electron: keep one main process,
`child_process.spawn` the adapters, `app.dock` + don't `app.quit` on
window close. `utilityProcess` is for a JS worker, not for `codex-acp`.

## One process per session?

ACP allows **several sessions on one connection**. Still spawn **one
adapter subprocess per live JaBot thread** for MVP.

| Policy | Pros | Cons |
|---|---|---|
| One adapter process, many `session/new` | Fewer PIDs, less RAM if the adapter is fat | Blast radius: one crash kills the folder. Shared cwd/env bugs. Harder logs. |
| One adapter process per thread | Kill = that thread. Logs per PID. Matches "Custom command" mentally. Matches Buzz's managed-agent PIDs. | More processes. PATH probes repeated. |
| One adapter process per harness (all Claude threads multiplexed) | Middle ground | Still couples unrelated folders. |

Per-thread isolation is the correct default while we do not understand
adapter process hygiene. If `claude-agent-acp` is proven stable and RAM
hurts, multiplex later behind the same `AcpConnection` trait.

**Idle folded threads:** keep the subprocess if the turn is in flight
(Buzz-style). If ACP reports idle + stop reason, the supervisor *may*
kill the adapter and keep the `sessionId` for resume. That policy belongs
to session-lifecycle; the shell just has to be able to kill a PGID.

On Unix, spawn adapters in their **own process group** and kill the group
(Buzz `runtime.rs` does this). Otherwise `claude` grandchildren survive
JaBot and hold files/ports. This is a host concern, not a UI concern.

## Sidecar vs PATH spawn

Two different things people call "sidecar":

1. **Tauri sidecar** ([docs](https://v2.tauri.app/develop/sidecar/)): a
   binary *we ship*, named `foo-aarch64-apple-darwin`, listed in
   `bundle.externalBin`. Use for `jabot-host` if/when we split, or for a
   bundled adapter we truly own.
2. **User/PATH binaries**: `claude-agent-acp`, `npx -y pi-acp`, Custom.
   Spawn with `Command`, not sidecar. Probe PATH like Buzz tier 2. Missing
   → install hint, not a crash.

Do not bundle Node + Claude. We are a client of the user's toolchain.

If JS ever needs to spawn, `tauri-plugin-shell` requires an allowlist.
Prefer Rust-only spawn so the webview cannot start arbitrary commands.

## PTY {#pty}

Harness-integration already deferred this. Shell answer:

**MVP: no PTY.** `stdin`/`stdout` pipes. `stderr` to a log file per
thread (Buzz does this). JSON-RPC stays clean.

**Later escape hatch** (fourth runtime type, not a harness adapter):

```
Rust portable-pty  →  bytes  →  xterm.js in a pane
```

- Host: [`portable-pty`](https://docs.rs/portable-pty/latest/portable_pty/)
  0.9 (WezTerm). Native `forkpty` on macOS.
- UI: [`xterm.js`](https://github.com/xtermjs/xterm.js) (`@xterm/xterm`).
  Same widget VS Code, Hyper, and ChatML use. Forward WINCH on pane resize.
- Do **not** parse ANSI into bubbles. If they picked raw PTY, they get a
  terminal. ACP `terminal_update` chunks are a possible *display* path if
  an agent speaks them; still not TUI scraping.

Electron's [`node-pty`](https://github.com/microsoft/node-pty) + the
[official Electron example](https://github.com/microsoft/node-pty/blob/main/examples/electron/README.md)
is the other stack. It is why people pick Electron for IDEs. We are not
an IDE, and we do not need it on day one.

ACP display-only terminals can wait until a shipped adapter actually
emits them.

## macOS-only MVP {#macos}

**Yes.** The prototype is a Mac window. The user is on a Mac. Conductor
shipped Mac-first. Warp started Metal/Mac. We should too.

Reasons that are product, not nostalgia:

- WKWebView is the native engine; the usual Tauri knock (WebKit vs
  Chromium drift) is a *cross-platform* problem.
- Signing/notarization is one pipeline, not three.
- Process groups, `forkpty`, notifications, titlebar overlay, keychain —
  implement once.
- App Store / Windows waitlists are how this category actually launches.

Do not pretend Linux is free because Tauri compiles. Notifications,
auto-update, PATH, and PTY would each be a second product.

When Windows happens: Tauri + WebView2 is still the bet. Do not take App
Sandbox now in a way that makes PATH adapters illegal.

## Packaging, signing, updates {#packaging}

### Distribution channel

**Direct download of a signed+notarized `.dmg` / `.app`.** Not Mac App
Store for MVP.

App Store **requires App Sandbox**. A sandboxed app cannot freely exec
`claude` from the user's PATH, talk to arbitrary cwd worktrees, or
supervise grandchildren. JaBot is a process supervisor; sandbox is the
wrong box. Developer ID outside the store is the same path Tauri
documents for DMG distribution
([distribute](https://v2.tauri.app/distribute/),
[sign/macos](https://v2.tauri.app/distribute/sign/macos/)).

### Signing & notarization

Apple's rule, independent of shell
([Notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution),
[Hardened Runtime](https://developer.apple.com/documentation/security/hardened-runtime)):

1. Sign with **Developer ID Application**.
2. Enable **Hardened Runtime**.
3. Notarize (`notarytool`); staple the ticket.
4. Nested binaries (sidecars, helpers) must be signed too.

Tauri: set `APPLE_API_KEY` / `APPLE_API_ISSUER` / `APPLE_API_KEY_PATH`
(or Apple ID + app-specific password) and it notarizes on `tauri build`
([signing docs](https://v2.tauri.app/distribute/sign/macos/)).

**Entitlements we will likely need** (declare the minimum):

- Inherit / disable library validation — spawning **unsigned** Node CLIs
  from PATH under Hardened Runtime otherwise dies with `code signature
  invalid` / library validation. Electron exposes this as
  `allowLoadingUnsignedLibraries` on utility processes; Tauri apps in this
  category typically set `com.apple.security.cs.disable-library-validation`.
- **Do not** enable App Sandbox.
- Keychain / notifications as we add those features.

Plan a day of "the adapter won't start from the notarized build" — ChatML
spent real time on signing + updater across Intel and Apple Silicon.

### Auto-update

| Mechanism | Use when |
|---|---|
| **[Tauri updater](https://v2.tauri.app/plugin/updater/)** | Default. Ed25519 signatures **cannot be disabled**. Static `latest.json` on GitHub Releases or a small HTTPS endpoint. macOS artifact is `app.tar.gz` + `.sig`. |
| **Sparkle** | If we want the native Mac update dialog (1Password/Raycast-class). Community `tauri-plugin-sparkle-updater` exists; macOS-only, which matches us. Optional polish, not a blocker. |
| **electron-updater / Squirrel.Mac** | Only if Electron. Also requires signed builds. GitHub Releases works. |

Wire Tauri updater from the first public build. Losing the signing key
means we cannot update existing installs — treat `TAURI_SIGNING_PRIVATE_KEY`
as a production secret from day one.

### Notifications

Folded threads that need a human: `UserNotifications` via Tauri's
notification plugin. Noise budget is a session-lifecycle question. Shell
just has to be allowed to post and to click-focus a thread.

## What we explicitly defer

- launchd agent / login-item host.
- Bundling Node or harness installers.
- App Store.
- Windows / Linux.
- `xterm.js` pane.
- Multiplexing many ACP sessions on one adapter PID.
- Extracting `jabot-host` as a sidecar — *design* the IPC as if we might;
  don't split the binary until Quit-vs-hide is not enough.

## Suggested first issues (from the brief)

1. Tauri 2 + Vite/React scaffold, overlay title bar, CSS tokens from the
   prototype.
2. Rust supervisor: spawn/kill PGID, stderr logs, PATH probe, ACP
   `initialize` via `agent-client-protocol`.
3. Window-close hides; Quit persists thread keys and drops children.
4. Signing identity + notarize + updater endpoint in CI (even if the
   channel is empty).
5. (Later) `portable-pty` + `xterm.js` as Custom runtime `type: "pty"`.
