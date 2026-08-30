# Inbox — when the OS has refused banners

Evidence for reading `notify/status` in the Inbox. Captured by rendering the
real `InboxView` in Chromium at deviceScaleFactor 2, over the same cards; the
only difference is the `notify` prop.

| file | what it shows |
| --- | --- |
| `before.png` | the header as it was, and as it still is for every state but one |
| `after.png` | the note, when `supported && authorization === "denied"` |

`notify/status` has been served end to end since #27 — the Rust method, both
wire types, and `HostClient.notifyStatus()` — and read by nobody. So a user
whose macOS permission was refused could not tell "notifications are off" from
"JaBot is broken": the Inbox filled up and nothing ever interrupted them.

Only `denied` earns a line, and only where there is a Notification Center to
refuse:

- `unsupported` is a Linux build or a dev build outside `JaBot.app`. There was
  never anything to permit, so pointing at System Settings would send the user
  somewhere that cannot help.
- `notDetermined` means nobody has been asked yet — the first banner asks.
  Saying "notifications are off" now would be wrong the moment it is read.
- `granted` has nothing to report.

The copy leads with the consequence and closes with the reassurance, because
the card is written first and always (decision #5). A refused permission costs
the interruption and nothing else, and a user who read this as "I have been
losing work" would have read it wrong. Amber rule rather than a red banner,
for the same reason.
