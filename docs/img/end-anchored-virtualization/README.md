# End-anchored scrolling

Evidence for #89. All three are the real `Conversation` in Chromium at
deviceScaleFactor 2, over the same nine-turn transcript with a chunk arriving
every 400 ms — which is what #14's reducer does on a live turn, and what the
old effect reacted to.

Each frame is taken the same way: scroll to 35% of the way down, then wait
while several chunks land.

| file | what it shows | where the view ended up |
| --- | --- | --- |
| `before.png` | the reader is yanked to the bottom mid-read | `scrollTop` 4001 of 4001 |
| `after.png` | the position is held, and there is a way back | `scrollTop` 1602 of 4612 |
| `still-follows.png` | parked at the end, the tail still follows | `scrollTop` 4001 of 4001, no button |

The numbers are the point as much as the pixels. `Conversation` set
`scrollTop = scrollHeight` on every change to `items`, and `src/views/transcript.ts`
rebuilds `items` on *every streamed chunk* — so reading back through history
while an agent was talking was not merely awkward, it was impossible: the view
snapped to the end several times a second.

`still-follows.png` is the half that had to survive. A conversation opens at
its tail and stays there while you watch it; the change is that leaving the
bottom now means something.

The "Jump to latest" pill went through a correction the screenshot caught. The
first cut was flat and unshadowed, and over a bubble it read as a phrase inside
somebody's message rather than as chrome. It is opaque and raised now.

`docs/img/markdown-bubbles/` covers what is *inside* a bubble; this is about
where the bubbles sit.
