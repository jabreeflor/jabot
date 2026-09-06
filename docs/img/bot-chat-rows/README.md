# Bot chats as vertical rows, with the chat's last line

Evidence for the change that turns the sidebar's crew grid into a list of chat
rows and gives each row the last thing said in that bot's standing thread.

Every frame is the real `App` in headless Chromium at `deviceScaleFactor: 2`,
against the production renderer bundle (`npm run build` + `vite preview`), and
all of them were recaptured after `main` was merged in — so they carry the
solid circular icons, the sparkle thread marks and the collapsing rail that
landed there while this branch was open.

| file | what it shows |
| --- | --- |
| `before.png` | the crew as a three-up grid of faces. A face names the bot and can say nothing about the conversation |
| `after.png` | the same crew as rows: face, name, and the chat's own last line. Chief keeps the larger face and stays first; the Crew row is last |
| `window.png` | the whole window. Chief's row quotes exactly the last message in the chat open beside it — the sidebar and the transcript cannot disagree, because both read the same events |
| `host.png` | the same rows over a **stubbed `host_rpc`**, i.e. `BotView.preview` off the wire rather than a fixture. Scheduler has been talked to by nobody, so its row falls back to the persona in italics |
| `artifact-top.png`, `artifact-path.png` | the PR artifact this repo requires, captured from the published page — the before/after pair, and the five stages a sentence travels from adapter stdio to a sidebar row |

`before.png` and `after.png` are the same fixtures at the same width; the only
difference is whether the change is applied. In `before.png` the previews are
unreachable — there was no row to draw them in and, before this change, nothing
ever wrote `threads.preview` either, so `crew/list` had nothing to send.

`host.png` is the one that pins the wire: the crew comes back from a stubbed
`crew/list` carrying `preview` on three of four bots, and the fourth draws its
`instructions` in italics instead. That italic line is the whole of the
"nothing has been said yet" state — a blank second line would read as a chat
with nothing in it.
