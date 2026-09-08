# Browser E2E evidence (issues #231 / #233)

Captured from Playwright Chromium against a real `jabot-hostd`
(`JABOT_BROWSER_EVIDENCE=docs/img/browser-e2e npx playwright test --project=chromium`).
Not `live.sh smoke` — that resets the shared developer data directory.

| file | moment |
| --- | --- |
| `connected-chief.png` | Host hello done, Chief open, composer ready (#231 smoke) |
| `fake-reply.png` | User send through the composer, fake-acp reply visible |
| `reload-persisted.png` | Same transcript after a full **browser reload** |
| `host-disconnected.png` | Host process killed; sidebar shows **Host disconnected** + Reconnect |
| `worktree-session.png` | New Chat folder session on a temp repo, Fake ACP reply |
| `schedules-run-now.png` | Schedule after **Run now** (no wall-clock wait) |
| `pr-board.png` | PR board against a synthetic repo + fixture `gh` |
| `host-refusal.png` | Invalid cron: alert stays, draft kept, list empty |
