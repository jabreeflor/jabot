# Inbox — whose thread a card is on

Evidence for carrying `botId` on `inbox/list` so a card can wear its bot's
face. Captured by rendering the real `InboxView` in Chromium at
deviceScaleFactor 2, over the same three cards; the only difference is whether
the bot was resolved.

| file | what it shows |
| --- | --- |
| `before.png` | every card wearing the generic code mark — what shipped |
| `after.png` | the two bot threads wearing their bots' faces, the code thread keeping the code mark |

The avatar is the only thing on an Inbox row that says *who* this is, and every
host card drew the generic code mark — including cards on a named crew member's
thread. `cardRow` hardcoded `source: { type: "code" }` because `inbox/list`
never said whose thread it was.

#22 was right to refuse to invent a bot for the row rather than guess. The fix
was to put the id on the wire: `ThreadRow.bot_id` was already on the row
`inbox_event_view` is handed, so `InboxEventView` and `SleepingThreadView` now
carry it and the renderer resolves it against the crew roster.

Note the third row in `after.png`. It is a genuine code thread with no bot, and
it keeps the code mark — as does a card naming a bot the roster does not have,
which happens while the crew is still loading or after a bot is removed. A face
with the wrong name on it would be worse than no face.
