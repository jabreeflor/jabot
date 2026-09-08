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
| `npm run test:browser:smoke` | Chromium, `@smoke` only — the PR gate |
| `npm run test:browser` | Chromium + WebKit |
| `npm run test:browser:ui` | Playwright UI mode |
| `npm run test:browser:install` | download Chromium and WebKit |

`./scripts/verify.sh` stays offline and display-less. Pass
`--check-browser` to run the Chromium smoke after the usual gates. CI's
`browser` job is the required PR check. `@playwright/test` and
`playwright-core` stay pinned together (1.63+); 1.56 hangs extracting
Chromium on Node 26.

## Isolation

Each test owns a temp `--data-dir`, a free port (never 1420), and a
process group (Vite + hostd + adapters). Restarts inside a test reuse that
directory (`jabot.restart()`). Teardown kills the group and deletes only
that directory.

Workers stay at 1 until a worker can prove it owns an independent
host/data/port triple.

## Adding a journey (#232+)

Import `{ test, expect }` from `./fixtures` and the locators from `./ui`.
Use `jabot.rpc` only for prerequisites and independent host assertions.
Clicks and composer sends go through the visible controls.

```ts
test.use({ seedChief: true }); // default; set false for first-launch tests
```

WebKit is renderer compatibility, not native WKWebView/Tauri. Packaged-app
acceptance is #235.

## Failures

On failure Playwright keeps the trace, screenshot, and video under
`test-results/`, plus an HTML report in `playwright-report/`. The fixture
attaches `vite.log` and `adapter-logs/*.stderr.log` when the test does not
pass. No retries — a flake is a failure.
