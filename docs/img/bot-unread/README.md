# The red dot on a crew blob

Evidence for #86. Both frames are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, over the same crew
and the same two Inbox cards. The host answers the same `crew/list` in both —
`unread: 1` for Writer and for Inbox Mgr — and the only difference is whether
the fix is applied.

| file | what it shows |
| --- | --- |
| `before.png` | five clean blobs. The dot could not appear however much was waiting |
| `after.png` | Writer and Inbox Mgr dotted, in the grid *and* in the sidebar rail; Chief, Scheduler and Researcher clean |

`Bot.unread` has been a prop since the prototype and `Avatar` has drawn the dot
for it just as long — with a test — but `BotView` had no such field, `botRow`
had nothing to copy, and the only `true` value anywhere in the app was a
fixture on the Code bot in `mock-host.ts`. So on a real host the dot was
unreachable code.

Note the sidebar's own badge in both frames: **2**, unchanged. The per-bot
count and the Inbox badge are two readings of one projection —
`count_unread_inbox_by_bot` reuses `count_unread_inbox`'s predicate character
for character — so a dot can never disagree with the number about which cards
count.
