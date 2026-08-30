# First-run engine picker — the host's catalog

Evidence for wiring the onboarding takeover's engine pane to `harness/list`
instead of the compiled-in fixtures. Captured by rendering the real
`Onboarding` component on its engine pane in Chromium at deviceScaleFactor 2.

| file | what it shows |
| --- | --- |
| `before.png` | the three engines hard-coded in `mock-host.ts` — what every fresh install saw |
| `after.png` | the host's catalog: a tier-2 preset and a tier-3 entry from the user's own JSON, with the Doctor's remedies on the two that are not installed |

This is the one screen in the app that asks a user to choose an engine, and it
was choosing from a fixture. A fresh install never saw a tier-2 preset or its
own tier-3 JSON here, and could pick an engine the host would refuse at thread
start with `HARNESS_UNAVAILABLE`.

The readiness in `after.png` is the same `withReadiness` the crew picker uses.
First run is when it matters most: it is the moment the choice is made, and
the alternative is finding out at the first prompt.

The fixtures are still the fallback. `useHarnessCatalog` returns `null` until
the catalog answers, and the gate renders `liveHarnesses ?? HARNESSES` — a
setup screen that waited on the host would be a blank window, and one that
blanked because a catalog read failed would be worse.
