# Chat: bot as prose, me as a bubble

Evidence for taking the balloon off the agent. Both frames are the real `App`
in Chromium at deviceScaleFactor 2 against the Vite preview (no Tauri host),
on the same Chief fixture.

The host TypeError in the sidebar footer is the preview transport, not this
change — it is identical in both frames.

| file | what it shows |
| --- | --- |
| `before.png` | cream bubble on the right (me), graphite bubble on the left (bot) |
| `after.png` | bot as light ink on the chat ground; me still a graphite bubble |

Primary chrome (tabs, `.btn.primary`, tool chips) stays cream via `--cream`.
The fold offer is still a notice card — that is a decision, not a reply.
