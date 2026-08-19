# Remote Hosts & Mobile Pairing

Bots don't have to live on the machine running the UI. They can be hosted on
this Mac or a separate machine (e.g. a NAS or home server), so the app needs to
connect to sessions across various locations. Mobile pairing (phone talks to
your crew) is MVP2, but the architecture decision is MVP1 — it decides whether
the UI talks to sessions directly or over a connection layer.

Depends on: [harness-integration](../harness-integration/brief.md) and
[session-lifecycle](../session-lifecycle/brief.md) (shapes what a "session
connection" carries). Feeds [app-shell](../app-shell/brief.md) (UI vs daemon
split becomes UI vs host split). Prior art:
[setup-porting](../setup-porting/findings.md) (OpenClaw device pairing, Hermes
multi-machine Bots, Buzz QR/SAS — copy the state machine, not Nostr or
master-secret transfer).

## Questions to answer

1. **Architecture** — does this force a client/server split from day one: a
   "bot host" daemon that owns sessions, and the desktop app as just a client?
   If yes, the local case is "client + host on the same machine" and remote is
   the same protocol over the network.
2. **Wire protocol** — what goes between client and host: the normalized
   harness event stream (from harness-integration) over WebSocket/gRPC? Does
   ACP already cover remote transport, or is it local-only?
3. **Reaching the host** — LAN only (mDNS discovery, Tailscale-style overlay)
   vs a relay for when you're out of the house. What do similar tools do
   (Claude Code remote control, happy-coder, Codex cloud)?
4. **Pairing & auth** — how a new device joins: QR code / pairing code, keys
   per device, revoking a device. No accounts/cloud if we can avoid it.
5. **Security** — sessions can run shell commands on the host machine. E2E
   encryption? Permission prompts must round-trip to whichever device you're
   on.
6. **Multi-host** — crew spread across machines (Chief on the Mac, code bots
   on the server)? Or one host at a time for MVP2? What does the sidebar show
   about where a bot lives?
7. **Mobile client scope** — MVP2 phone app: full chat, or just Inbox +
   notifications + answering permission prompts? Native vs PWA?

## What this blocks (future issues)

- Client/host split decision (affects MVP1 repo scaffold)
- Bot-host daemon + connection protocol
- Device pairing flow
- Mobile client (MVP2)
- Host picker in the UI (where a bot/session runs)
