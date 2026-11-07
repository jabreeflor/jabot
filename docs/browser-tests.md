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

## Where the browsers run

Screenshots depend on the fonts and font-rendering libraries of the machine
the browser runs on, and the visual tests allow zero pixels of drift. So the
browsers run in one pinned image everywhere — a Mac, a cloud agent
container, and CI: `mcr.microsoft.com/playwright:v<@playwright/test version>-noble`.

`scripts/browser-docker.sh` starts `playwright run-server` in that image
(serving the repo's own `node_modules/playwright-core`, read-only) and sets
`PLAYWRIGHT_WS_ENDPOINT`. `playwright.config.ts` then connects every project
to it with `exposeNetwork: "<loopback>"`, so the page's `localhost` traffic
tunnels back to the Vite + `jabot-hostd` running on your machine. Only the
browsers are in Docker; the runner, Vite, and the host are not. No host
networking is needed, so Docker Desktop on macOS works.

The host is also kept from seeing the machine it runs on
(`tests/browser/host.ts`): `JABOT_HERMETIC_PATH=1` makes `jabot-hostd`
search only the PATH it is given (no login shell, `~/.local/bin`, or
Homebrew), and that PATH is fixture dirs, a dir holding only `node`, and
`/usr/bin:/bin:/usr/sbin:/sbin`. A logged-out fixture `gh` stands in for a
real one and `GH_TOKEN` / `GITHUB_TOKEN` are blanked. Without this, an
installed `claude` or a signed-in `gh` changes Settings, New Chat, and the
sidebar counts.

You need Docker running (Docker Desktop; in a cloud agent container,
`sudo dockerd &`). Bumping `@playwright/test` bumps the image with it; the
script refuses an image whose tag does not match.

## Commands

```bash
npm run test:browser                 # Chromium + WebKit, visual + axe + keyboard (Docker)
npm run test:browser:native          # same, with locally installed browsers (screenshots will drift)
npm run test:browser:smoke           # Chromium axe + keyboard only (local browsers)
npm run test:browser:visual          # Chromium screenshot project (Docker)
npm run test:browser:ui              # Playwright UI
./scripts/verify.sh --check-browser  # same as test:browser, after the usual gates
```

`npm run test:browser` builds `jabot-hostd` / `fake-acp-agent` when they are
missing.

## Updating screenshot baselines

CI sets `updateSnapshots: "none"` and must never be told to approve diffs.
A changed pixel fails the job and publishes `test-results/` (expected /
actual / diff, plus traces and host logs).

To refresh baselines, run the update script on any machine with Docker. The
**baseline environment** is the pinned Playwright image above, locale
`en-US`, timezone `UTC`, viewports 1180×780 (desktop) and 900×600 (minimum):

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
