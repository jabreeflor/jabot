# Thread provenance — who handed the work over

Evidence for drawing `ThreadStateResult.handoff` in the thread header. Captured
by rendering the real `ThreadView` in Chromium at deviceScaleFactor 2, once per
provenance shape. Same component and same CSS in all four; the only thing that
varies is the `handoff` prop.

`handoff-states.png`, top to bottom:

1. **No provenance** — a thread the human started. Unchanged from before this
   change, which is the point: the ordinary case must stay clean, and a header
   that said "started by you" would be noise on every thread in the app.
2. **Handed off by a bot** — "Handed off by Chief — Chase the failing migration
   test and report back".
3. **A coding job a bot opened** — `kind: "code_session"` reads "Coding job
   from Chief", because Chief did not hand this to a colleague, it opened a job.
4. **Sent, but nobody picked it up** — `dispatched: false`. The task really was
   sent and the thread really exists, so this is not an error state for the
   thread; but a line saying only "Handed off by Writer" would be describing
   work that is not happening. The host's own `detail` says why, in amber.

The host has resolved all of this since #24 (`handoff_view`, backed by
`store::latest_handoff_to`) and served it as `ThreadStateResult.handoff`.
Nothing in the renderer ever read it — `client.threadState` had no callers at
all — so a thread Chief spawned was visually identical to one the human
started.
