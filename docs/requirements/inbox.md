# Inbox view

**Issue:** #22
**Status:** Implemented — `src/views/InboxView.tsx`, `src/views/inbox.ts`, host-side `inbox_events` projection

## What it is

The view that surfaces what a human should look at: folded threads that
finished, failed, got lost, or need input, without requiring the user to
keep every thread visible in the Sidebar.

## Why

Per the fold/run/Inbox decision
([`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#5--fold--run--inbox-data-model)),
the Inbox is explicitly **not** its own source of truth — it is a
persist-then-notify **projection** of run transitions, so it can never
drift from what actually happened to a run.

## Requirements

1. Inbox entries are derived from `inbox_events`, which are themselves
   written from run state transitions (see requirement 7 of
   [thread-state-and-runs.md](thread-state-and-runs.md)) — the Inbox
   view never has its own independent notion of "done."
2. "Still sleeping" (a folded thread with no actionable event) is
   distinguished from "needs you" / "done" / "failed" / "lost" —
   the Inbox surfaces the latter categories, not every folded thread.
3. Opening an Inbox card navigates to and resurfaces the underlying
   thread (`threads.state` transitions per
   [thread-state-and-runs.md](thread-state-and-runs.md)), not a
   read-only summary disconnected from the real conversation.
4. New Inbox events appear without a manual refresh — they arrive as
   pushed host-api events (see requirement 5 of
   [host-api-protocol.md](host-api-protocol.md)).
5. Dismissing/clearing an Inbox entry does not delete the underlying run
   record or transcript — it only affects Inbox visibility.
6. The Inbox is testable against the mock host without a live harness
   (`src/__tests__/inbox.test.tsx`, `inbox-host.test.tsx`).
7. The same event stream that feeds the desktop Inbox is the basis for
   native notifications ([native-notifications.md](native-notifications.md))
   and the mobile Inbox client ([mobile-inbox.md](mobile-inbox.md)) — the
   Inbox is one logical feed with multiple presentations, not
   reimplemented per surface.
