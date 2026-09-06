# Invert chat bubble colours

Evidence for inbound cream / own graphite. Both frames are the real `App` in
Chromium at deviceScaleFactor 2 against the Vite preview (no Tauri host), on
the same Chief fixture: one inbound turn, one reply, and the fold offer.

The host TypeError in the sidebar footer is the preview transport, not this
change — it is identical in both frames.

| file | what it shows |
| --- | --- |
| `before.png` | cream on the right (me), graphite on the left (inbound) |
| `after.png` | cream on the left (inbound), graphite on the right (me) |

Primary chrome (tabs, `.btn.primary`, tool chips) stays cream. Those used to
borrow `--bub-me`; they now use `--cream` so inverting the bubbles does not
invert the rest of the shell.
