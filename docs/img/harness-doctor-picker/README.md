# Harness picker — Doctor readiness

Evidence for wiring `harness/doctor` into the engine picker. Captured by
rendering the real `HarnessPicker` over the real `withReadiness`, in Chromium
at deviceScaleFactor 2. The only difference between the two images is whether
the Doctor's reports were folded in — same component, same CSS, same cards.

| file | what it shows |
| --- | --- |
| `before.png` | what shipped: every engine advertises its blurb, because `available` was never set |
| `after.png` | Codex and Hermes carry the Doctor's remedy; Claude, which is installed, keeps its blurb |

`before.png` is the bug. `.harness-card .missing` has been styled since #13 and
`HarnessPicker` has always branched on `available === false`, but nothing in
the renderer ever called `harnessDoctor()` — so the branch was dead and every
engine painted as ready, including ones the host's own Doctor knew were
`cli_missing` or `logged_out`. A user picked one, and found out at thread
start.

`after.png` is the same screen once the probe answers. The text shown is the
report's `remedy` ("Run: codex login", "Run: npm i -g hermes-acp") rather than
the catalog's constant `installHint`, because the Doctor writes it knowing
what it actually found on this machine.
