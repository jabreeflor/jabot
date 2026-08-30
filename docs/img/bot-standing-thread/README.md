# A bot's standing chat, live

Evidence for #97. Both frames are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, on the pane the app
opens on — Chief's standing chat. The host serves the same answers in both; the
only difference is whether the fix is applied.

| file | what it shows |
| --- | --- |
| `before.png` | `mock-host.ts`'s compiled-in conversation — the auth-migration card, from a fixture, on every install | 
| `after.png` | the transcript the host actually has for Chief's standing thread |

The bug is not that the fixture looks wrong. It is that the fixture is *all*
there was: `crew/thread` and `HostClient.botThread` had been served and typed
since #24 with no caller anywhere in `src/`, so `case "bot"` read
`state.transcripts[bot.id]` — the mock reducer, keyed by bot id — and every
message typed into that chat went back to the reducer. The bot's real thread,
its runs, its queued prompts and its memory directory were somewhere else
entirely, and nothing on screen said so.

`before.png` is what a user saw with a working host and a real conversation
sitting behind it.
