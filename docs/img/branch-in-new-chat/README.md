# Branch in new chat

Live-host evidence for #266. Captured with `./scripts/live.sh shot` against a
real `jabot-hostd` and `fake-acp`, not a mock.

A Code thread in `demo` was seeded with two turns, then forked at the first
assistant reply (`throughSeq` 2).

`action-row.png` — every user and assistant bubble has the compact branch
control. Hover uses the native `title` tooltip (“Branch in new chat”); the
accessible name is the same string.

`branched.png` — the child opens as **Branch of Auth migration**. History
stops at the cut. The header and a transcript stamp both name the source.

`source-intact.png` — the original still has the later turn. Further messages
on either side stay independent.

`walkthrough.html` — the two-minute mechanism explainer.
