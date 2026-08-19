# Pairing, security, and mobile

How a new device joins a host, what "secure" means when that host can
run a shell, and how thin the MVP2 phone client should be. Complements
[architecture.md](architecture.md) and
[protocol-and-reach.md](protocol-and-reach.md).

## Pairing

### Recommendation

**No JaBot accounts.** Each host and each client device generates a
keypair. The host displays a QR (GUI) or a short pairing code
(headless NAS). The new device scans or types; both sides show a
Signal-style safety number; the user confirms once. The host records
the device's public key, name, and capabilities. Revoke is a list on
the host. Pairing is an MVP2 *feature*; the protocol in MVP1 still
carries `deviceId` so the local UI is "the first paired device."

### What we are copying

| Prior art | Mechanism | Take | Leave |
|---|---|---|---|
| **Syncthing device IDs** | Device ID = encoding of SHA-256 of the TLS cert / public key. Mutual TLS; you add an ID, they accept. LAN mDNS + optional global discovery + relays. | Durable ID = key fingerprint. Discovery ≠ auth. Both sides must accept. | 52-character IDs as the only UX (they admit this is painful). Global discovery that publishes ID→IP. |
| **Happy Coder** | QR carries the shared secret / data key. Relay authenticates by signature. No account. | QR as the out-of-band channel. Keys never sent to a server. | Wrapping a single CLI session. |
| **Codex Remote** | QR from desktop; `codex remote-control pair` prints a code for headless. Pair is **phone↔this host**, not account↔fleet. | GUI QR + headless typed code. One-to-one host pairing. | ChatGPT account + MFA as a second root of trust (we have no account). |
| **Claude Remote Control** | Session URL + QR; same claude.ai login. Trusted Devices enrolls a WebAuthn-ish credential per browser/phone. | Device enroll + revoke list + biometric step-up as a *later* hardening. Session URL as a capability token is a warning. | Account as identity. Transcript stored on vendor servers. |
| **Tailscale** | Identity provider login, or auth keys for headless. Coordination server distributes node keys. | Auth keys = how a NAS joins *the overlay*. Not how a JaBot client joins a host. | Making Tailscale the only pairing story (phones, family Macs that are not on the tailnet). |
| **Signal safety numbers** | 60-digit per-conversation fingerprint of both identity keys; QR compare; mark verified; alert on change. | Show a short comparable number at pair time. Alert if a host key changes (reinstall). | Per-conversation numbers between humans; we pair device↔host. |
| **Apple Continuity / Handoff** | Same Apple Account; BLE 4.2 pairing via APNs; AES-256 keys in Keychain; proximity. | Proximity + existing identity is magical **if you already have an account ecosystem.** | iCloud as a requirement. JaBot is not an Apple-ID product. Continuity does not help a Linux NAS. |

Syncthing's own docs sketch the UX we want: short ID plus a one-time
PIN, Bluetooth-style, because full IDs are "a mouthful to read over the
phone." QR is that PIN channel when both devices have cameras; the
typed code is that channel for a NAS.

### Flow (MVP2, illustrative)

1. Host is already running (it has been since MVP1). User chooses
   **Pair a device**.
2. Host generates a single-use pairing nonce (seconds-to-minutes TTL,
   one successful use).
3. **GUI host:** QR encodes `{ v, hostId, hostPub, nonce, addrs[] }`
   where `addrs` may be `jabot.local`, a Tailscale MagicDNS name, and/or
   a relay locator. **Headless:** print an 8-character Crockford code +
   the host fingerprint.
4. Client scans/types, connects (LAN / Tailscale / relay), proves
   possession of its key, presents a device name ("Jabree's iPhone").
5. Both screens show the same 6–8 digit safety number derived from
   `hash(hostPub || devicePub || nonce)`. User taps **Pair** on the
   host (the machine that can run `rm -rf`). Optional: client confirms
   too.
6. Host stores `{ deviceId, pub, name, createdAt, lastSeen, role }`.
   Role for MVP2 is `full` (desktop) or `approver` (phone). Revoke
   deletes the row; outstanding sessions stay on the host, the device
   just cannot attach.

The local desktop in MVP1 is implicitly paired to its colocated host
(it spawned it). Persist that as device #1 so MVP2 is not a special
case.

### Multi-device, revoke, lost phone

- Pairing is **per host**. A phone that should steer Mac *and* NAS
  scans twice. Codex is explicit about this; it is the right call when
  each host is a trusted computer, not a cloud account.
- Revoke lives on the host UI (and later a `jabot-host devices`
  command). No cloud "sign out everywhere" unless we operate a relay —
  then the relay can drop that device's connection by `deviceId`, but
  the host is source of truth.
- Host key rotation (reinstall) must scream: safety number changed,
  re-pair. Silent replacement is a MITM.
- Do not let a newly paired `approver` widen host policy (Always allow
  this command) without a `full` device confirming — later, not MVP2.

## Security

### Threat model in one paragraph

The **host is trusted**. It has the repo, the shell, the harness
logins, and the ability to exfiltrate them. We are not sandboxes for
the user's own Mac. The **wire is untrusted** (LAN, café Wi-Fi, a
relay we or Happy or Tailscale operate). **Clients are semi-trusted**:
a paired phone may approve `git push` and read diffs; a stolen phone
should not stay paired. Other people on the LAN should not be able to
browse `_jabot._tcp` and become a client.

### Rules

1. **Encrypt on the wire, always.** Local Unix socket can be
   filesystem-permissioned (`0700` socket in a user dir) and skip TLS.
   Anything that leaves the box is TLS (if we have names/certs) or
   Noise/NaCl with the paired keys (if we don't). Happy uses TweetNaCl;
   Codex Remote uses Noise; Tailscale uses WireGuard underneath. Pick
   one in implementation; do not ship plaintext WebSocket on the LAN
   "because it's home."

2. **Do not store session transcripts on a third-party relay in
   cleartext.** Claude Remote Control does, for sync. That is the
   opposite of a no-account personal product. If we introduce a relay,
   it is Happy-shaped: ciphertext + device wakeup. Metadata (that a
   session exists, packet sizes) will leak; live `bash` output must not.

3. **Bind permissions to a device.** The host records `deviceId` on
   every `permission/reply`. The Inbox card for a judgment call says
   which device is being asked. A paired device can answer; an
   unpaired process cannot. Policy presets (Ask / Accept edits / Wait
   for Inbox) still live on the host — adapter-design already said
   that — so a phone cannot quietly switch the host to YOLO.

4. **Round-trip to whichever device you're on.** Locked by
   harness-integration: ACP `session/request_permission` is a request,
   not a notification; someone must reply. The host holds that RPC and
   fans it out. If you folded the thread and left the house, the phone
   is the client that answers; the desktop does not need to be open.
   If two devices answer, first authentic wins. On `session/cancel` or
   Delete, the host replies `cancelled` to ACP itself.

5. **Push is a hint, not a capability.** APNs/FCM bodies for
   "Auth migration needs you" should be generic if they traverse Apple
   or Google. The sensitive command string waits for the app to
   decrypt over the paired channel. Happy encrypts push contents; do
   that.

6. **The session URL is a secret if we ever have one.** Claude's QR is
   a capability; leak the URL, leak the session (account helps, Trusted
   Devices helps more). Our pairing nonce is single-use. Long-lived
   capability URLs are how this class of product gets blogged as a
   CVE.

7. **Do not expose the host on `0.0.0.0` with a shared password.**
   Codex's experimental WebSocket currently allows unauthenticated
   non-loopback unless you set `--ws-auth`; their own remote-connections
   doc says don't put app-server on a public network. Believe them.

### What we are not claiming

End-to-end encryption does not stop a malicious host binary, a
compromised harness, or an ACP adapter. It stops the café and the
relay. Say that in any security copy. Sandbox presets (Claude
`--sandbox`, Codex workspace-write) remain host-side harness config,
not a pairing concern.

## Mobile

### Recommendation

**MVP2 phone client = Inbox + notifications + permission prompts**,
with enough thread readback to know what you are approving. Not a
full second desktop. **Native**, not a PWA: Expo/React Native (Happy
already shipped this) or SwiftUI-first if we are willing to skip
Android at first. PWA fails the actual job (reliable iOS push + a
permission sheet you trust).

### Why thin

The prototype's Inbox is the product: folded work comes back when it
is done, failed, or needs you. Claude Remote Control's own push
toggles are named after that job — "Push when Claude decides", "Push
when actions required." Happy's store listing leads with permission
notifications.

A full mobile chat (composer, toolblocks, diffs, crew switcher, PR
view) is how Codex-in-ChatGPT and Claude's Code tab compete with each
other. It is a year of UI. JaBot's wedge is: you already have a Mac
window; the phone exists so a 40-minute migration can ping you at
dinner without you SSHing home.

MVP2 scope that is worth shipping:

- Pairing (QR).
- Host list (one or many) with online/asleep.
- Inbox: Needs you / Done / still sleeping.
- Permission sheet: command + cwd + host name + Allow / Deny /
  Always (if we allow Always from mobile at all — see security rule 3).
- Optional: read-only transcript tail so the approval is not blind.
- Optional: a composer attached only to Chief, not to every code
  thread.

Explicitly later: New Chat from the phone, host picker for a new
session, PR review, voice (Happy's 11Labs toy).

### Native vs PWA vs SwiftUI vs RN

| Option | Push / background | Why it fits or doesn't |
|---|---|---|
| **PWA** | iOS web push exists since 16.4 for *home-screen installed* apps; no silent push; text+icon; friction. Background work is still a service worker the OS kills. | Fine for a status page. Bad for "approve this `rm` in the next 30 seconds." Skip. |
| **Expo / React Native** | APNs + FCM via Expo or native. Happy shipped iOS, Android, and web from this stack. | Best if MVP2 includes Android. Matches "one small companion," not a 60 fps IDE. |
| **SwiftUI (iOS only)** | First-class APNs, Live Activities later, Keychain, Camera for QR. | Best if we stay in the Apple household and the NAS is the only non-Apple machine. Faster permission-sheet polish. Loses Android until a rewrite. |
| **Share the desktop webview** | Would imply the desktop is already a web app talking to the host. | Do not let mobile drive the Electron/Tauri choice. App-shell decides the desktop; mobile can be a separate client on the same host protocol. |

Opinion: **Expo if we want Android in MVP2; SwiftUI if we don't.** Do
not start a PWA "to learn" — iOS will make the Inbox feel broken and
we will rewrite it. Do not wait for a Tauri mobile story.

Push provider reality: APNs/FCM imply *some* network endpoint that can
wake the app. That can be a tiny relay we operate, a self-hosted Happy
server, or "phone is on Tailscale and we don't push, we poll" (worse).
Budget a wakeup path in MVP2 even if MVP1 has zero network.

### What the phone is allowed to do on the host

Same host protocol, smaller grant:

- `approver` role: Inbox, permission reply, read transcript, maybe
  cancel.
- Not: delete a thread, change crew tools, pair *another* device, dump
  host logs, set Always-allow globally.

The desktop remains the admin console. That is how you sleep at night
after pairing a phone that also has a messaging app and a toddler.

## Feed into other topics (not edits)

- **app-shell:** daemon always; UI is a client. Packaging must keep the
  host alive after window close.
- **session-lifecycle:** permission waiters and Inbox cards live on the
  host, so they survive UI quit and can surface on another device.
- **data-and-persistence:** `hostId` on every thread; device keystore
  on each client; pairing table on the host.
- **bot-crew:** Chief-on-Mac + workers-on-NAS is an MVP2 layout, not an
  MVP1 requirement; routing still does not require hosts to mesh.
