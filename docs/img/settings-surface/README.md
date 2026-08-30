# The Settings surface

Evidence for #107. All three are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport.

| file | what it shows |
| --- | --- |
| `sidebar-entry.png` | the way in, under Schedules |
| `settings-view.png` | the pane, seeded from `settings/get` |
| `env-in-force.png` | a host running under `JABOT_IDLE_TIMEOUT_MS` |

Three decision records parked a preference on a settings surface that did not
exist — D-006 the stuck backstop's threshold, D-013 a remembered permission
scope, D-017 the cron interval — and D-018 said plainly that naming #26 for it
had been optimistic, because nothing in that issue's scope created a place to
put one. So the threshold stayed an env var on the host process, which a
bundled Tauri app gives nobody.

**Two controls, not five.** A remembered permission scope has no host support
at all. A pane offering a control that decides nothing would be worse than no
pane: the user would set it, it would do nothing, and they would have no way to
find that out.

**Minutes on screen, milliseconds on the wire.** Nobody thinks about a backstop
in milliseconds; the protocol keeps them because every other duration on it is
in milliseconds.

`env-in-force.png` is the case the third frame exists for. The env var wins
over the stored preference — the e2e suite sets it on every spawned host, and a
saved value that beat it would make those tests wait on a threshold they never
wrote. So the control is disabled and the pane *says which knob is deciding*
rather than silently ignoring the one on screen. It reads "0 minutes" because
1500 ms rounds there; the field is disabled and the sentence under it is the
answer, and only a developer or a test is ever in this state.
