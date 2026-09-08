# Browser E2E (Playwright + real host)

The suite in `tests/browser/` drives a real Chromium page against the **web
renderer** and a real `jabot-hostd`, using the same Vite bridge
(`scripts/dev/host-plugin.ts`, `src/host/devTransport.ts`) as
`./scripts/live.sh up`.

```
Chromium → React renderer → devTransport → Vite jabot-host plugin
         → host-bridge → jabot-hostd → SQLite + fake-acp-agent
```

## What this covers

- Host disconnect / reconnect (and the product `Reconnect` control)
- Persistence across **browser reload**, **Vite restart**, and **host restart**
- Worktree sessions started from New Chat
- Schedules created and fired with **Run now** (no wall-clock cron wait)
- PR board / workspace against a synthetic repo and a fixture `gh`
- Host refusals that restore prior UI state

Each test owns a temp data directory and a dedicated Vite port. Fixture RPC
seeds prerequisites; the action under test goes through visible controls.

## What this does not cover

These tests are **not** native acceptance. They do not prove:

- Tauri IPC (`invoke("host_rpc")` / `listen("host-rpc")`)
- Native file / folder dialogs (`pick_workspace`, `scratch_workspace`)
- macOS Keychain (the live bridge sets `JABOT_SECRETS_BACKEND=memory`)
- AppKit notifications, Dock hide/quit, updater, or packaged-app identity
- WKWebView rendering (a Playwright WebKit project would still be a browser,
  not JaBot.app — that remains #235)
- Mobile browsers — there is no browser-accessible mobile transport yet;
  existing Unix-socket tests do not establish that

GitHub fixtures never use real tokens and do not claim API compatibility.

## Reload vs Vite restart vs host restart

These are different mechanisms. Test titles name the one they exercise.

| Mechanism | What dies | What stays | Recovery |
| --- | --- | --- | --- |
| **Browser reload** | The tab / `HostClient` | Vite and `jabot-hostd` | `useHost()` runs again |
| **Host restart** | `jabot-hostd` only | Vite and the page | Bridge emits `host/disconnected`; **Reconnect** (or a later RPC) respawns the host on the same data dir |
| **Vite restart** | Vite, the HMR socket, and the host process | The data directory | A new Vite is spawned; the page must navigate again |

Do not treat a page reload as proof that the host survived, or a Vite bounce
as proof that the renderer reconnected in place.

## Commands

```bash
npm run host:build                  # jabot-hostd + fake-acp-agent
npx playwright install chromium     # once per machine
npm run test:browser                # Chromium + WebKit
npm run test:browser:smoke          # one persistence case
./scripts/verify.sh --check-browser # default gates + Chromium suite
```

CI runs `browser` as its own job (`.github/workflows/ci.yml`) so the default
`verify` job stays offline and display-free.

Mobile browser journeys are deferred until a real browser-accessible mobile
transport exists (#233).
