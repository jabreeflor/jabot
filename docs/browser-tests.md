# Browser visual and accessibility tests

Playwright drives the real renderer against an isolated `jabot-hostd` — the
same Vite host bridge `scripts/live.sh` uses (`src/host/devTransport.ts` →
`scripts/dev/host-plugin.ts`). It does **not** call `live.sh smoke` or
`reset`, and it never touches `.jabot-dev/`.

These checks prove the **web renderer** in Chromium and WebKit. They are not
proof of native Tauri IPC, Keychain, or WKWebView. Native packaged-app
acceptance is [#235](https://github.com/jabreeflor/jabot/issues/235).

The default `./scripts/verify.sh` gate stays offline and display-free. This
suite is a separate CI job and an opt-in local flag.

## Commands

Install matching browsers once (network):

```bash
npx playwright install chromium webkit
```

Then:

```bash
npm run test:browser                 # Chromium + WebKit, visual + axe + keyboard
npm run test:browser:smoke           # Chromium axe + keyboard only
npm run test:browser:visual          # Chromium screenshot project
npm run test:browser:ui              # Playwright UI
./scripts/verify.sh --check-browser  # same as test:browser, after the usual gates
```

`npm run test:browser` builds `jabot-hostd` / `fake-acp-agent` when they are
missing.

## Updating screenshot baselines

CI sets `updateSnapshots: "none"` and must never be told to approve diffs.
A changed pixel fails the job and publishes `test-results/` (expected /
actual / diff, plus traces and host logs).

To refresh baselines, run on the **baseline environment** — Linux, Playwright
1.63, the Chromium and WebKit builds that package installs, locale `en-US`,
timezone `UTC`, viewports 1180×780 (desktop) and 900×600 (minimum):

```bash
# Review every PNG Playwright writes before you commit.
npm run test:browser:update-snapshots
```

That is `playwright test --update-snapshots` with the visual projects. Open
the new files under `tests/browser/__screenshots__/` and check them the way
you would a `docs/img/` shot: did the layout you meant to change actually
change, and only that?

Human review expectations:

- Commit baselines in the same change that altered the pixels.
- Do not raise `maxDiffPixels` or add a ratio to hide drift.
- Mask only clocks that must move (`.when`, `.sched-next`). If a mask is
  hiding content or layout, remove it and fix the fixture instead.
- WebKit and Chromium baselines are separate. A WebKit-only diff is still a
  renderer bug to look at, not a reason to skip that engine.

## What is covered

| State | Visual | Browser axe (contrast on) | Keyboard |
| --- | --- | --- | --- |
| Onboarding | dark/light, both window sizes | yes | focus + Escape skip |
| Sidebar + streamed conversation | dark/light, both window sizes | yes | tab to primary nav |
| Inbox permission card | dark/light, desktop | yes | — |
| New Chat | dark/light, desktop | open harness listbox | Escape restores trigger |
| Settings | dark/light, both window sizes | yes | visible focus |
| Schedules | dark/light, desktop | yes | — |
| PR board + sign-in dialog | dark/light, desktop | dialog + error banner | focus trap + Escape restore |

Critical and serious axe findings fail. Moderate and minor are attached to
the HTML report. Named exceptions, if any, live in
`tests/browser/support/a11y.ts` (`AXE_EXCEPTIONS`) — never a blanket rule
disable.

jsdom axe (`npm run test:a11y`) still exists. It cannot paint, so contrast
stays off there on purpose.

## Isolation

Each test starts its own Vite process, port, and temp `--data-dir`. Workers
stay at 1 until a later change proves parallel hosts do not collide. Failure
artifacts include the page trace/screenshot and the Vite/host log.
