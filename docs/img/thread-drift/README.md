# Thread drift — the next message starts a new conversation

Evidence for drawing `ProcessView.drift` above the composer. Captured by
rendering the real `ThreadView` in Chromium at deviceScaleFactor 2, twice over
the same thread; the only difference is the `drift` prop.

`drift.png`, top to bottom:

1. **Nothing has moved** — unchanged. This is the overwhelmingly common case,
   and a banner on every thread would train the user to ignore the one that
   matters.
2. **The engine and the folder have moved** — "This thread's setup has changed —
   the engine and the folder are not what this conversation was started with.
   Your next message begins a new one."

This is the one thing on the screen a user cannot possibly infer. Everything
looks normal: the transcript is there, the composer works, and the next message
silently opens a *new* conversation the agent has no memory of — because
resuming a session whose harness or cwd has moved would be continuing someone
else's job. The host has computed this on every `thread/state` since #21;
nothing drew it.

Amber, not red, and above the composer rather than in the header. Nothing has
failed and nothing is lost — it is a fact about what pressing Enter will do, so
it sits with the control it is about.

The field names are translated out of their wire spelling (`harnessId` → "the
engine", `cwd` → "the folder") and joined with an "and" rather than commas: the
sentence is the whole warning, so it has to read as one. A field name the host
learns to report before this list does is printed raw rather than dropped.
