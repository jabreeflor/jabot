# Remote Hosts & Mobile Pairing — Findings

Researched August 2026 against current public docs, adapters, and prior art.
This file answers the seven questions in [`brief.md`](brief.md). Deep dives
live in sibling files.

**Recommendation in one sentence:** Force a **host daemon in MVP1** — the
desktop UI is a client even when it sits on the same Mac; speak a
**JaBot-owned JSON-RPC host protocol** (not ACP-over-network); treat remote
as the same protocol over Tailscale/LAN; leave pairing and a thin mobile
Inbox client to MVP2.

| Question | Short answer | Detail |
|---|---|---|
| 1. Architecture | Yes: client/host from day one. Local = colocated. | [architecture.md](architecture.md) |
| 2. Wire protocol | JaBot JSON-RPC over Unix socket / WebSocket. ACP stays stdio south of the host. ACP HTTP is draft — do not wait. | [protocol-and-reach.md](protocol-and-reach.md#wire-protocol) |
| 3. Reaching the host | Localhost first. LAN via mDNS. WAN via bring-your-own Tailscale. No JaBot relay in MVP1. | [protocol-and-reach.md](protocol-and-reach.md#reaching-the-host) |
| 4. Pairing & auth | QR + short code, per-device keys, Syncthing-style IDs, revoke on the host. No accounts. | [pairing-security-mobile.md](pairing-security-mobile.md#pairing) |
| 5. Security | Host is trusted (it runs the shell). Encrypt the wire. Bind permissions to a paired device. E2E if a relay exists. | [pairing-security-mobile.md](pairing-security-mobile.md#security) |
| 6. Multi-host | Data model from day one (`hostId`). MVP1 ships one host. Crew-across-machines is MVP2. | [architecture.md](architecture.md#multi-host) |
| 7. Mobile client | MVP2: Inbox + push + permission prompts, not full chat. Native (Expo/RN or SwiftUI), not a PWA. | [pairing-security-mobile.md](pairing-security-mobile.md#mobile) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Client/host split in the MVP1 repo scaffold** — UI process never owns
   ACP stdio. See [architecture.md](architecture.md). Feeds
   [app-shell](../app-shell/findings.md) question 3 (UI vs daemon).
   **Fork:** this topic wants a separate OS process in MVP1; app-shell
   wants the host in-process with a socket-shaped API. See the
   [research README](../README.md#open-fork-physical-host) for the
   recommended issue-writing resolution (logical split now, sidecar later).
2. **Bot-host daemon + connection protocol** — JSON-RPC host API carrying
   normalized ACP events plus JaBot overlay (fold, Inbox, crew, host
   identity, permission routing). See
   [protocol-and-reach.md](protocol-and-reach.md).
3. **Device pairing flow** — deferred to MVP2 as a *feature*, but the host
   protocol must have a `device` identity slot in MVP1 so pairing is not a
   rewrite. See [pairing-security-mobile.md](pairing-security-mobile.md).
4. **Mobile client (MVP2)** — Inbox + notifications + answering permission
   prompts. Not a second full desktop.
5. **Host picker in the UI** — the prototype's `🖥` in the chat header is
   the affordance. Every bot/thread stores `hostId`; the picker is "this
   Mac" until a second host is paired.

## Prototype note

`prototypes/jabot-classic.html` puts a `🖥` in the Chief header
(`.head .mon`) and has a **NAS backup script** thread under GLOBNET-SYNC.
There is no host-picker modal yet — the icon is a product claim, not a
working control. Treat it as: bots can live on this Mac *or* a separate
machine, and the UI should always show *where*. The NAS thread is a Pi
session, which is the interesting case: the harness and the files live
where the host lives, not where the window is.

## What we explicitly defer

- Shipping a JaBot-operated relay or libp2p stack in MVP1.
- Speaking ACP Streamable HTTP / WebSocket to the UI (draft; host can
  grow an ACP-HTTP façade later).
- Pairing UI, QR flow, device revoke (MVP2).
- A phone app (MVP2).
- Live session handoff between hosts (Codex does this; we don't need it).
- Cloud execution of sessions (Claude Code on the web / Codex cloud).
  JaBot sessions run on a machine the user owns.

## Sources

Primary docs, not secondary blogs, unless noted:

- ACP transports: [v2 transports](https://agentclientprotocol.com/protocol/v2/transports)
  (stdio stable; Streamable HTTP "draft proposal in progress"),
  [Streamable HTTP & WebSocket RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport)
  (Active as of 2026-07-02, not Completed),
  [Transports Working Group](https://agentclientprotocol.com/announcements/transports-working-group)
  (2026-04-22)
- Claude Code Remote Control:
  [code.claude.com/docs/en/remote-control](https://code.claude.com/docs/en/remote-control)
- Codex: [app-server](https://developers.openai.com/codex/app-server)
  (`--listen` / `--remote`),
  [remote connections](https://developers.openai.com/codex/remote-connections),
  [Work with Codex from anywhere](https://openai.com/index/work-with-codex-from-anywhere/)
- Happy Coder: [happy.engineering](https://happy.engineering/),
  [How it works](https://happy.engineering/docs/how-it-works/),
  [slopus/happy](https://github.com/slopus/happy/),
  [slopus/happy-server](https://github.com/slopus/happy-server)
- Tailscale: [MagicDNS](https://tailscale.com/docs/features/magicdns),
  [auth keys](https://tailscale.com/docs/features/access-control/auth-keys),
  [Headscale](https://headscale.net/)
- Pairing prior art: [Syncthing device IDs](https://docs.syncthing.net/dev/device-ids.html),
  [Signal safety numbers](https://support.signal.org/hc/en-us/articles/360007060632-What-is-a-safety-number-and-why-do-I-see-that-it-changed),
  [Apple Handoff security](https://support.apple.com/guide/security/handoff-security-secf78dbe639/web)
- Reach alternatives: [libp2p DCUtR](https://libp2p.io/docs/dcutr/),
  [Cloudflare Quick Tunnels](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/),
  [Bonjour / mDNS](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/NetServices/Articles/about.html)
