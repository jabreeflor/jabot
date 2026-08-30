# Fold & "Wait for Inbox" wired to real sessions

**Issue:** #26
**Status:** Implemented — `src/views/fold.ts`, `src/components/FoldButton.tsx`, host lifecycle/supervisor integration

## What it is

The end-to-end behavior of folding a thread — hiding it from the
Sidebar while its ACP session keeps working in the background — and
having it come back to the user's attention automatically when it needs
to, wired to the real supervisor and permission broker rather than mocked
state.

## Why

Fold is the app's core promise: you can hand a bot a task, stop watching
it, and trust you'll be told when it's done or stuck. This issue is where
[thread-state-and-runs.md](thread-state-and-runs.md),
[permission-broker.md](permission-broker.md), the supervisor
(see [desktop-host-lifecycle.md](desktop-host-lifecycle.md)), and
[inbox.md](inbox.md) actually get connected end to end.

## Requirements

1. Folding a thread only changes `threads.state` (visibility) — it does
   not pause, cancel, or otherwise interfere with an in-flight run
   (per the fold/run/Inbox decision).
2. A folded thread's `fold_policy` determines what "wait for Inbox"
   means for it (e.g. resurface on any terminal state vs. only on
   `needs_you`/`failed`) — this policy is per-thread data, not a global
   switch.
3. A run on a folded thread that needs a permission decision
   (see [permission-broker.md](permission-broker.md)) transitions to
   `needs_you` and, per the thread's fold policy, resurfaces the thread
   and creates an Inbox event — it does not hang silently just because
   the thread is out of view.
4. A run on a folded thread that completes (success or failure) creates
   an Inbox event per the thread's fold policy without requiring the
   user to have the thread open.
5. Folding, quitting, and resuming compose correctly: a thread folded
   before Quit resumes still folded, and any run that was `running` at
   Quit time is reconciled to a terminal or `needs_you` state on resume
   rather than left `running` forever (see requirement 3 of
   [desktop-host-lifecycle.md](desktop-host-lifecycle.md)).
6. Unfolding (resurfacing) a thread from the Sidebar or from an Inbox
   card shows the same live/persisted transcript as if it had never been
   folded (see [chat-transcript.md](chat-transcript.md)).
7. This wiring is covered end-to-end by `src/__tests__/fold.test.tsx`
   against the mock host, exercising fold → run event → Inbox → resurface.
