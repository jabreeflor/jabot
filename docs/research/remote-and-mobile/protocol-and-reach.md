# Protocol and reach

Wire protocol between client and host, and how a client finds a host
that is not on `localhost`. Complements [architecture.md](architecture.md).
Research, not a build spec.

## Wire protocol

### Recommendation

**JaBot-owned JSON-RPC 2.0**, newline-delimited on a Unix socket locally
and one JSON-RPC message per WebSocket text frame over the network. The
payload is the normalized ACP event stream plus a JaBot envelope
(`hostId`, `threadId`, `deviceId`, monotonic `seq`). Bidirectional:
client requests (`prompt`, `permission/reply`, `fold`, `cancel`) and
host requests (`permission/ask`, `inbox/resurface`).

Do **not** require ACP-over-HTTP for MVP1. Do **not** speak a second
chat dialect. The renderer already maps ACP `session/update` onto
bubbles ([adapter-design](../harness-integration/adapter-design.md)).

### Why not ACP remote as the client↔host pipe

ACP stdio is stable and is the southbound lock.

Northbound, ACP Streamable HTTP / WebSocket is still a **draft**:

- Official transports page: stdio is defined; Streamable HTTP is marked
  *"draft proposal in progress"*
  ([protocol/v2/transports](https://agentclientprotocol.com/protocol/v2/transports)).
- A Transports Working Group was announced 22 April 2026
  ([announcement](https://agentclientprotocol.com/announcements/transports-working-group)).
- The RFD ([streamable-http-websocket-transport](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport))
  moved to **Active on 2 July 2026**. It is not Completed. v1 of that
  transport explicitly leaves reconnect, keepalive, and in-flight replay
  to the implementer; v2 is where resume IDs land.
- Shape: single `/acp` endpoint; POST for client→server; long-lived GET
  SSE (connection-scoped + per-session) requiring **HTTP/2**; WebSocket
  upgrade as the duplex alternative. Clients that speak HTTP MUST
  implement both.
- Qwen Code is implementing it as a daemon façade (`qwen serve` already
  had a bespoke REST+SSE northbound; they are adding official `/acp`).
  That is the right *later* move for "Zed talks to a JaBot host." It is
  the wrong MVP1 move for "our UI talks to our host."

Gaps vs JaBot even if the RFD froze tomorrow:

- No device pairing, no multi-client fan-out, no "this permission was
  answered on the phone."
- No Inbox / fold overlay.
- Disconnect loses in-flight `session/update` until v2 resume. Folded
  threads *are* the disconnect case.
- Cookie-affinity and HTTP/2 are cloud-deployment concerns. We have one
  user and a NAS.

ACP custom transports are explicitly allowed if JSON-RPC framing is
preserved. A JaBot host protocol *is* that. If ACP HTTP later stabilizes,
the host can grow an `/acp` façade without rewriting the UI.

### Why not each harness's own remote

| Prior art | What it actually is | Why it is not our wire |
|---|---|---|
| **Claude Code Remote Control** | Local `claude` process makes **outbound HTTPS** to Anthropic, registers, polls. Phone/browser at [claude.ai/code](https://claude.ai/code) is a window. QR / session URL. Transcript **stored on Anthropic servers** while connected. No inbound ports. | Vendor account, vendor relay, vendor transcript store. API-key users are ineligible. Ties JaBot to Claude. |
| **Codex app-server `--listen ws://` + `codex --remote`** | JSON-RPC over stdio, Unix socket, or experimental WebSocket. Docs: loopback or SSH-forward; non-loopback currently unauthenticated unless `--ws-auth`. | Codex-only. Experimental WS. We already wrap Codex *via ACP*, not app-server. |
| **Codex Remote / ChatGPT mobile** | Noise-encrypted relay, QR pairing host↔phone, same ChatGPT account. SSH remote projects from the desktop app. Official line: don't expose app-server on a public network. | Account-gated. OpenAI relay. Full mobile Codex, not a JaBot Inbox. |
| **Happy Coder** | CLI wraps Claude/Codex; TweetNaCl E2E; QR shares the key; relay stores ciphertext only. Self-hostable `happy-server`. Expo app. | Closest cousin. Still a wrapper around *one session's TTY/SDK*, not a multi-bot host with Inbox. Steal the pairing/E2E shape; don't become their protocol. |

JaBot's host already multiplexes many ACP sessions (Chief + crew +
folded threads). None of the vendor remotes are a crew host.

### Envelope (illustrative)

Not a schema freeze — a shape so session-lifecycle and data-and-persistence
know what crosses the socket.

```text
Host → Client  session/update     { hostId, threadId, seq, acp: <session/update> }
Host → Client  permission/ask     { hostId, threadId, requestId, subject, options }
Client → Host  permission/reply   { requestId, optionId | cancelled, deviceId }
Client → Host  prompt             { threadId, content }
Client → Host  thread/fold        { threadId }
Host → Client  inbox/resurface    { threadId, reason: done|failed|needs_you }
```

`seq` is per-thread and monotonic so a reconnecting client can ask
`resumeFrom: seq` and the host replays from its log. ACP HTTP v1 will
not do this for us. Happy's relay is a blob store for the same reason.

Permissions: the host **owns** the outstanding ACP
`session/request_permission`. It broadcasts `permission/ask` to every
connected client (desktop window, later phone). First authentic reply
wins; others get `permission/resolved`. If no client is connected, the
host keeps the ACP waiter and emits an Inbox card — that is the
session-lifecycle contract, not a transport trick.

## Reaching the host

A ladder, not a menu. Climb only as far as the milestone needs.

### 0. Localhost (MVP1, required)

Unix domain socket (preferred on macOS/Linux) or `ws://127.0.0.1`. The
UI on this Mac always uses this. If you cannot do this, you cannot do
Inbox.

Bind loopback only. No `0.0.0.0` in MVP1.

### 1. LAN (cheap, no account)

mDNS / DNS-SD (Bonjour on the Mac): advertise something like
`_jabot._tcp.local.` with TXT `hostId`, `name`, `ver`. The other Mac on
the desk browses and connects. Apple's
[Bonjour overview](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/NetServices/Articles/about.html)
is exactly "no directory server, no account." Syncthing's local
discovery is the same idea with less polish.

This does **not** get you off the house network. Phones on LTE will not
see mDNS. Treat LAN as "second Mac in the studio" and "I pointed the
laptop at the NAS while on home Wi-Fi."

Auth is still pairing (next file). Discovery is not authorization —
Syncthing itself warns that local discovery is spoofable. You connect,
then you prove keys.

### 2. User overlay — Tailscale (recommended WAN, MVP1 docs / MVP2 default)

Do not ship a NAT-traversal stack. Tell the user: if this host should
be reachable from a café, put it on a tailnet.

Why Tailscale is the right *recommendation* for a personal no-account
product:

- WireGuard between devices; coordination server only swaps keys and
  endpoints. Data plane is E2E.
- Direct UDP when it can; **DERP** relay when it cannot. DERP forwards
  WireGuard packets it cannot read. Reported ~92% eventually direct.
- [MagicDNS](https://tailscale.com/docs/features/magicdns):
  `nas.tail-xxxx.ts.net` or just `nas`. The JaBot client can store
  `wss://nas:7420` and not care about CGNAT.
- [Auth keys](https://tailscale.com/docs/features/access-control/auth-keys)
  for a headless NAS (`tailscale up --auth-key`). That is how a box
  without a browser joins.
- Free personal tier is large enough for one human's Mac + NAS + phone.
- Headscale ([headscale.net](https://headscale.net/)) exists if the user
  refuses Tailscale's coordination server. Same clients, your control
  plane. Mention it; do not ship it.

JaBot does not vendor Tailscale. We do not spawn `tailscaled`. We
accept a hostname the user typed, or we detect MagicDNS names. The
host still authenticates JaBot devices on top — Tailscale proves "this
packet came from a node on my tailnet," not "this is Jabree's phone
app."

### 3. Encrypted relay (MVP2, optional, Happy-shaped)

Phones will not all run Tailscale. Happy's whole product is: both sides
dial **out** to a relay because firewalls allow outbound; the relay
sees ciphertext; the QR carried the key.

If JaBot wants "scan this QR, no VPN, phone on LTE," that is the
pattern:

- Host and phone connect outbound.
- Identity = public keys from pairing.
- Relay stores/forwards opaque blobs + wakeup for push.
- Self-host the relay (Happy's server is small; we can too) **or** run
  a JaBot-operated one that is useless without the device key.

Do **not** put this on the MVP1 critical path. Local + Tailscale covers
the NAS-in-the-closet user, which is the architecture customer. The
phone user is MVP2.

A relay without E2E is Claude's model (transcript on Anthropic's
servers). Fine for them; wrong for a no-account personal host that
streams `bash` output.

### Explicitly reject as the product path

| Tool | What it is | Why not JaBot's reach layer |
|---|---|---|
| **Cloudflare Tunnel (named)** | `cloudflared` outbound to CF edge; public hostname; CF terminates TLS. | Account, domain, CF can read HTTP unless you add another layer. Designed to *publish a site*, not pair a personal daemon. |
| **Quick Tunnels / trycloudflare** | `cloudflared tunnel --url http://localhost:8080`, no account. | Dev-only. **200 in-flight request cap. No SSE.** A streaming agent protocol will die here. Hostname is random and disposable. |
| **localhost.run** | `ssh -R 80:localhost:3000 localhost.run`. No install. | Ephemeral public URL. Anyone with the URL hits your host. Free hostnames rotate. Optional TLS passthrough still leaves a public attack surface. Demo tool, not a pairing story. |
| **libp2p / DCUtR / Holepunch** | Decentralized hole punch via relays; ~70% success after reservation in IPFS measurements; still needs relays for the rest. | A networking product inside our product. Identity (peer IDs) is good; NAT success is not "it always works." Tailscale already did this engineering. Revisit only if we cannot stomach recommending Tailscale *and* cannot run a tiny Happy-style relay. |

SSH port-forward (`ssh -L` / `-R`) is a perfectly fine *power-user*
escape hatch — Codex documents it for app-server. Document it. Do not
build a UI around it.

## Prior art, compared to JaBot

### Claude Code Remote Control

Official docs:
[Continue local sessions from any device](https://code.claude.com/docs/en/remote-control).

- Execution stays on your machine; web/mobile are a window. That split
  is the one we want.
- Reach is **their** relay: outbound HTTPS, no inbound ports. That is
  why it feels like magic and why Zero Data Retention orgs cannot use
  it. Transcript sync is stored server-side.
- Pairing is "session URL + QR + same claude.ai account," not a device
  key. Team/Enterprise add **Trusted Devices** (beta): enroll a device
  at sign-in, biometric step-up every 18 hours, revoke from account
  settings. Steal the *revoke a device* UX; do not steal the account.
- `claude remote-control` is a **server mode** (up to 32 sessions,
  `--spawn worktree`, `--capacity`). `claude --remote-control` / `/rc`
  attaches one interactive session. `-c` / `--continue` resumes the
  server's session after Ctrl+C (about four hours). VS Code has `/rc`
  with a banner, no QR.
- Push: "when Claude decides" and "when actions required" (permission
  prompts). Presence detection skips pushes while you are at the
  terminal. That is the mobile Inbox in miniature.
- Process must keep running (`tmux` on a remote box). Network outage
  ~10 minutes kills server mode. We have the same constraint: the
  **host** must stay up, not the laptop lid.

JaBot should look like Remote Control to the user ("phone steers a
machine I own") and like Happy under the hood (keys, not an account).

### Happy Coder (slopus/happy)

The named prior art in the brief. Open source mobile companion for
Claude Code and Codex. Expo app, CLI wrapper, optional self-hosted
relay.

- QR exchanges the encryption key. Server never sees it.
- Auth is signatures on a public key — **no account**.
- Push for permissions and completion; encrypted, server cannot read
  the body.
- Dual mode: local TTY vs remote; press a key to take the session back.
  JaBot will have several clients at once instead — broadcast, don't
  steal the TTY.
- Limitation: wraps `claude` / `codex`, one project session, not a crew
  host with folders and Inbox. We still need our daemon.

Show HN (Aug 2025) and the current site agree: E2E is the product
promise. Copy that promise if we ever run a relay.

### Codex cloud vs Codex remote

Two different products. Keep them straight.

- **Codex cloud / ChatGPT Work** — agent runs in OpenAI's environment.
  Not a host you pair. Irrelevant except as the thing we are not.
- **Codex app-server `--remote`** — DIY wire to a process you started.
  Useful as proof that JSON-RPC-over-WebSocket is enough; dangerous
  defaults (unauthenticated non-loopback).
- **Codex Remote in ChatGPT mobile** (GA around June 2026 per secondary
  writeups; official page
  [remote connections](https://developers.openai.com/codex/remote-connections)
  and [Work with Codex from anywhere](https://openai.com/index/work-with-codex-from-anywhere/)):
  QR from the **desktop app**, scan with ChatGPT mobile, same account.
  Headless: `codex remote-control pair` prints a code. Noise-encrypted
  relay. Phone is a full control plane (threads, diffs, approvals,
  terminal). SSH hosts are connected from desktop, then available
  through the same relay. Thread *handoff* moves git state between
  machines — out of scope for us.

Steal: QR on a GUI host, typed code on a headless NAS, phone as an
approver. Do not steal: ChatGPT account as the pairing root, or
cross-machine worktree teleport.

## Recommendation for reach, restated

| Milestone | Reach |
|---|---|
| MVP1 | Unix socket / loopback. Host daemon always on. Optional: connect a second Mac via mDNS or a typed `host:port` on LAN, if it falls out of the protocol work — not a launch requirement. |
| Docs / power users | "Install Tailscale on the NAS and this Mac; use the MagicDNS name." |
| MVP2 | Pairing + (Tailscale on the phone **or** an E2E relay). mDNS as the zero-config LAN onboarding before anyone types an address. |

The architecture decision does not depend on the relay existing. It
depends on the host protocol existing on localhost.
