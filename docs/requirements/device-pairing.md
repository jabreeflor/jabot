# Device pairing: QR + SAS + scoped grants (MVP2)

**Issue:** #19
**Status:** Implemented (host side) — `src-tauri/src/host/pairing/`; consumed by [mobile-inbox.md](mobile-inbox.md)

## What it is

The mechanism by which a second device (a phone, another Mac) is granted
access to talk to a JaBot host: a QR-code offer, a Short Authentication
String (SAS) verification step, and a revocable, scope-limited grant
rather than a shared full-access password.

## Why

MVP1 assumes exactly one client talking to the host. MVP2's mobile Inbox
client (see [mobile-inbox.md](mobile-inbox.md)) needs a second device to
authenticate without the host handing out an unscoped credential —
pairing is how that trust is established and later revoked.

## Requirements

1. Pairing starts with the host generating a short-lived **offer**
   (`src-tauri/src/host/pairing/offer.rs`) encoded as a QR code the
   second device scans.
2. The offer exchange derives a shared secret verified out-of-band via a
   **Short Authentication String** (`crypto.rs`) — a human compares a
   short code on both devices — so a passive network observer of the QR
   payload alone cannot complete pairing.
3. A completed pairing produces a **scoped grant**
   (`scope.rs`): the paired device is authorized for specific
   capabilities (e.g. "read Inbox," not "everything the desktop app can
   do") rather than a full-access token.
4. Grants are stored via the data layer (`store/pairing.rs`) and are
   individually **revocable** — revoking one grant does not affect
   others, and a revoked device's next request is rejected rather than
   silently still succeeding until some cache expires.
5. An expired or never-completed offer cannot be replayed to complete
   pairing after its window closes.
6. Pairing is the only way a non-desktop client obtains host access — the
   host does not offer an unauthenticated or password-only path for a
   second client.
