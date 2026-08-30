# Mobile Inbox client (MVP2)

**Issue:** #29
**Status:** Implemented — `src/mobile/`

## What it is

A lightweight, mobile-optimized client that shows a paired phone the
Inbox (and enough context to answer a pending "needs you" prompt) without
running the full desktop shell.

## Why

The whole point of folding a thread is not having to babysit it at a
desktop — a mobile Inbox is where that payoff is actually realized when
the user is away from their Mac. It depends on device pairing (see
[device-pairing.md](device-pairing.md)) for a trust boundary and on the
same Inbox event feed as the desktop (see [inbox.md](inbox.md)) for data.

## Requirements

1. The mobile client authenticates using a pairing grant
   (`src/mobile/session.ts`, `scope.ts`) — it never has a separate,
   unscoped credential path into the host.
2. `transport.ts` implements the host-api connection appropriate for a
   remote client (over the socket-shaped protocol described in
   [host-api-protocol.md](host-api-protocol.md)), reusing the same
   protocol types as the desktop client where the pairing scope allows.
3. `InboxScreen.tsx` renders the same category of events as the desktop
   Inbox (needs you / done / failed / lost), scoped to whatever the
   pairing grant permits, in a layout usable one-handed on a phone.
4. `ask.ts` lets the user answer a pending permission prompt
   (see [permission-broker.md](permission-broker.md)) from the mobile
   client when the grant's scope includes it — a "needs you" run must be
   answerable from mobile, not just observable.
5. `inbox.ts` mirrors the desktop's Inbox data shape (`src/views/inbox.ts`)
   closely enough that event handling logic isn't duplicated and
   diverging between the two clients.
6. A revoked pairing grant (see requirement 4 of
   [device-pairing.md](device-pairing.md)) immediately stops the mobile
   client from receiving further Inbox data on its next request.
7. Mobile-specific behavior is covered under `src/mobile/__tests__/`
   independent of the desktop UI test suite.
