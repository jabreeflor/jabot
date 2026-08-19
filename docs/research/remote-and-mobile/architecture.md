# Architecture — client and host from day one

Concrete shape for the process split, given [findings.md](findings.md).
This is research, not a build spec — enough to open the blocked issues
and to constrain [app-shell](../app-shell/brief.md).

## Decision

**Force a host daemon in MVP1.** The desktop app is a client even when it
is the only process the user launched. Local is "client + host colocated";
remote is the same protocol on another machine. Do not let the UI process
own ACP stdio, PTYs, or folded sessions.

```
JaBot UI (desktop / later phone)
        │
        │  JaBot host protocol  (JSON-RPC; Unix socket locally,
        │   WebSocket on LAN / Tailscale / later a relay)
        ▼
Host daemon  —  owns sessions, Inbox state, permission waiters, crew
        │
        │  ACP stdio  (locked by harness-integration)
        ▼
Adapter process  (claude-agent-acp / codex-acp / pi-acp / custom)
```

This is the diagram in
[harness-integration/adapter-design.md](../harness-integration/adapter-design.md)
with the left arrow named. Buzz already lives here: Tauri desktop spawns
`buzz-acp`; the UI never talks to Claude or Codex. Remote is "same
protocol, different machine" — [buzz.md](../harness-integration/buzz.md)
said that out loud; this topic decides it.

## Why the daemon is not optional

Four independent reasons. Any one would be enough; together they close
the "just spawn from Electron/Tauri" shortcut.

1. **Harness-integration already drew the line.** ACP is spoken by the
   host, not the renderer. Permissions are bidirectional RPC that must
   survive the chat view being folded. That waiter lives in a process
   that outlasts a window.

2. **Session-lifecycle needs a supervisor.** Folded threads keep working
   ([session-lifecycle brief](../session-lifecycle/brief.md) Q1 and Q4).
   Laptop sleep and "I quit the app" are the same problem as "I'm on my
   phone": the session cannot be a child of the UI. A host daemon is the
   supervisor, whether or not a second machine ever exists.

3. **The product already claims multiple hosts.** The prototype's `🖥`
   and the NAS backup thread are not decoration. If MVP1 inlines host
   into the UI, remote is a rewrite of the session supervisor, the event
   bus, and the permission path. That is the expensive order.

4. **Prior art converged.** Claude Code Remote Control, Codex
   app-server, Happy Coder, and Qwen's `qwen serve` all put a long-lived
   process on the machine that has the files and the shell, then attach
   UIs. Nobody of consequence drives a coding agent from a renderer
   process across a network.

The cost is real: two binaries (or one binary, two modes), a localhost
socket, and a "is the host up?" status. That is cheaper than painting
the UI into a corner.

## What "host" means

A **host** is a machine JaBot is allowed to run sessions on. It has:

- A durable `hostId` (keypair fingerprint — see
  [pairing-security-mobile.md](pairing-security-mobile.md)).
- A display name (`Jabree's MacBook`, `nas.home`).
- PATH + credentials for harnesses (`claude` login, `codex login`, `pi`).
- Filesystem cwd's the user pointed at (the New Chat folder).
- The ACP adapter subprocesses and their stdio.

The host is the **trusted computing base**. It can run `bash`. Pairing a
client to a host is pairing a remote control to a machine that can
destroy the working tree. Treat it that way.

A **client** is a UI that may:

- List threads / bots / Inbox cards the host will show it.
- Stream `session/update`-shaped events and render bubbles + toolblocks.
- Answer `session/request_permission` (and JaBot-level judgment calls).
- Send prompts, cancel, fold, archive.

A client must **not**:

- Spawn harnesses.
- Hold the only copy of session state.
- Auto-approve tools just because it is "the local window." Policy lives
  on the host; the client is one of several possible approvers.

## Local = colocated, not "a different protocol"

On the Mac that runs the UI:

```
┌─────────────────────────────────────────┐
│  jabot.app                              │
│    UI process  ──unix socket──►  host   │
│                         ▲               │
│                         └── same box    │
└─────────────────────────────────────────┘
```

Ship it as one app. The host can be a sidecar process started by the
app, a `launchd` user agent, or a hidden window-less child — that is
app-shell's packaging question. The *protocol* does not change if the
socket is `/tmp/jabot.sock` or `wss://nas.ts.net:7420`.

Do **not** special-case "in-process ACP client in the UI for local, daemon
only when remote." That fork bit every Electron app that tried it. The
local socket is the daemon; if you can talk to yourself you can talk to
the NAS.

Closing the window should not kill sessions. That is the whole Inbox
product. If app-shell picks a design where quitting the app tears down
children, the host must be a separate user-agent (launchd / systemd /
Windows service later). Call that out in the scaffold issue; do not
discover it when fold is wired.

## What the host protocol carries (vs ACP)

ACP is the **southbound** dialect: host → adapter. The **northbound**
dialect is JaBot's:

| Concern | Where it lives | Why not ACP |
|---|---|---|
| Chat events, tools, diffs, plans | Pass through (already ACP-shaped) | Host already normalized to ACP |
| `session/request_permission` | Host parks the RPC; fans it out to connected clients; first valid reply wins | ACP has one client; we have many devices |
| Fold / Inbox / Wait for Inbox | JaBot overlay | ACP has no Inbox |
| Crew / Chief routing | JaBot overlay | ACP has no crew |
| Which host, which device answered | JaBot overlay | ACP assumes a single local editor |
| Pairing, device revoke, host name | JaBot overlay | ACP has no device model |

Do not invent a second event schema for bubbles. The renderer already
consumes ACP `session/update` (adapter-design). The host protocol wraps
those updates in a JaBot envelope (`threadId`, `hostId`, `seq`) so a
client can multiplex many sessions and survive reconnect.

ACP Streamable HTTP is the wrong northbound default in 2026. It is
still a draft (see [protocol-and-reach.md](protocol-and-reach.md)). It
does not know about devices, Inbox, or "reply to this permission from
the phone." A host *may* later expose `/acp` so Zed can drive a JaBot
host; that is a façade, not the UI's wire.

## Multi-host

Every thread row already needs `cwd`, `harnessId`, `acpSessionId`
([adapter-design](../harness-integration/adapter-design.md)). Add
`hostId`. The sidebar can then show where a bot lives — the `🖥` plus a
name, or a dim marker on the thread like the prototype's NAS row.

**MVP1:** one host, the Mac running the app. The field exists; the
picker has one entry ("This Mac"). Connecting to a second host is not
required to ship.

**MVP2:** more than one host at a time. The interesting crew shape is
exactly the brief's example: Chief of Staff on the Mac (calendar, mail,
"what should I do today"), code bots on the NAS/home server (always-on,
the repo already lives there). A client sees a unified sidebar; each
row knows its host. A permission prompt names the host ("Allow `rm` on
**nas**?").

Rules that keep this from becoming a distributed-systems science fair:

- A session **does not move**. It is born on a host and dies there.
  Codex thread-handoff (git bundle + worktree on another machine) is a
  different product; ignore it.
- The client is a multiplexer, not a mesh. Clients talk to hosts; hosts
  do not talk to each other in MVP2.
- Crew routing (Chief → code bot) is a host-local or client-mediated
  prompt, not an overlay network between daemons.
- If the Mac is asleep, NAS sessions still run; Inbox cards for those
  threads can wait on the NAS host until a client reconnects — which is
  why mobile pairing matters.

Do not build "one host at a time, switcher in Settings" as a permanent
model. The data model is many; the MVP1 *UI* is one. Switching later is
a picker, not a migration.

## Implications for app-shell

[app-shell brief](../app-shell/brief.md) Q3 asks "UI process vs a
separate daemon." This topic answers: **separate daemon, always.**

Further constraints, not decisions:

- The shell must be able to spawn and *keep alive* a child (or talk to a
  user-agent) that is not the renderer.
- Packaging: one `.app` that starts both is fine; two user-visible apps
  is worse.
- macOS only for MVP is compatible: the host is "a Unix process we
  start." A NAS host in MVP2 is the same binary on Linux. Do not
  Windows-prove the daemon in MVP1.
- PTY: still south of the host (adapter-design deferred raw PTY). The
  client never sees a PTY.

Electron vs Tauri is still app-shell's call. Either can own a sidecar.
Tauri's Rust core is a natural place for the host if they pick Tauri;
an Electron app can still spawn a `jabot-host` binary. Do not pick the
shell *because* of remote — pick it for process/PTY/memory, then put
the host on the native side.

## What we are not doing

- **UI-owned sessions with an optional "remote mode."** That is how you
  get two supervisors.
- **Waiting for ACP remote** to define the split. Custom transports are
  allowed; we are allowed to be our own client.
- **A JaBot cloud that runs agents.** Sessions run on a machine the user
  owns. Cloud is other people's product (Claude Code on the web, Codex
  cloud).
- **Per-harness remotes.** Do not use `claude remote-control` or
  `codex --remote` as JaBot's connection layer. Those lock us to one
  harness and one vendor relay. The host speaks ACP stdio to whatever
  adapter; clients speak JaBot to the host.
