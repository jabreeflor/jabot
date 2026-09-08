# Browser tests (Playwright + real host)

Playwright drives the React renderer against a live `jabot-hostd`, the same
bridge `scripts/live.sh up` uses:

```
Playwright → renderer → src/host/devTransport.ts
  → Vite scripts/dev/host-plugin.ts / host-bridge.ts
  → jabot-hostd → SQLite + fake-acp-agent
```

External service/agent boundaries are the only mocks. The Vitest `e2e`
project (TypeScript client ↔ host protocol, no renderer) stays as it is.

## One command

```bash
npm run host:build                 # once, or whenever the Rust bins move
npx playwright install chromium    # once per machine (webkit optional)
npm run test:browser:smoke         # Chromium smoke, no credentials
```

That starts its **own** Vite on a dedicated loopback port with a temp data
directory. It does **not** call `live.sh smoke` or `reset` (those wipe the
shared `.jabot-dev/data` a developer may be using on port 1420).

| command | what |
| --- | --- |
| `npm run test:browser:smoke` | Chromium, `@smoke` only |
| `npm run test:browser:chromium` | all Chromium journeys — the PR gate (#232 + #233) |
| `npm run test:browser:repeat` | `@smoke` × 20, retries 0 |
| `npm run test:browser` | Chromium + WebKit (recovery / workspace / smoke) |
| `npm run test:browser:ui` | Playwright UI mode |
| `npm run test:browser:install` | download Chromium and WebKit |

`./scripts/verify.sh` stays offline and display-less. Pass
`--check-browser` to run the Chromium suite after the usual gates. CI's
`browser` job is the required PR check (`npx playwright test --project=chromium`).
`@playwright/test` and `playwright-core` stay pinned together (1.63+); 1.56
hangs extracting Chromium on Node 26.

## Isolation

Each test owns a temp `--data-dir`, a free port (never 1420), and a
process group (Vite + hostd + adapters). Restarts inside a test reuse that
directory (`jabot.restart()`). Teardown kills the group and deletes only
that directory.

Workers stay at 1 until a worker can prove it owns an independent
host/data/port triple.

## Journeys (#232)

Onboarding, streaming, scrolling, fold → Inbox, permission, and
failure/cancel live next to the #231 smoke. They import `{ test, expect }`
from `./fixtures` and locators from `./ui`. `jabot.rpc` / `seedCodeThread`
are prerequisites and independent host assertions only — clicks and composer
sends go through visible controls.

```ts
test.use({ seedChief: true }); // default; set false only when first-launch must not pre-seed
```

Need Vite/host env before spawn (a fixture `gh` on PATH)? Use
`browserTest(() => ({ pathPrefix }))` from the same file — do not fork a
second fixture.

`jabot.stopHost()` / `jabot.startHost()` kill only `jabot-hostd`.
`jabot.restart()` is a **Vite restart**. Name the test after the mechanism
it exercises (browser reload vs Vite restart vs host restart).

A deliberate send-path break is `./scripts/dev/browser-break-demo.sh`.
Representative screenshots land in `docs/img/browser-journeys/`.

WebKit is renderer compatibility, not native WKWebView/Tauri. Packaged-app
acceptance is #235. Recovery/workspace journeys are #233. Web-renderer+host
limits: [`docs/browser-e2e.md`](../../docs/browser-e2e.md). Mobile browser
is deferred.

## Failures

On failure Playwright keeps the trace, screenshot, and video under
`test-results/`, plus an HTML report in `playwright-report/`. The fixture
attaches `vite.log` and `adapter-logs/*.stderr.log` when the test does not
pass. No retries — a flake is a failure.
