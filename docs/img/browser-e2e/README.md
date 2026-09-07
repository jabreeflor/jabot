# Browser E2E smoke (issue #231)

Captured from the Playwright Chromium smoke against a real `jabot-hostd`
(`JABOT_BROWSER_EVIDENCE=docs/img/browser-e2e npm run test:browser:smoke`).
Not `live.sh smoke` — that resets the shared developer data directory.

| file | moment |
| --- | --- |
| `connected-chief.png` | Host hello done, Chief open, composer ready |
| `fake-reply.png` | User send through the composer, fake-acp reply visible |
| `reload-persisted.png` | Same transcript after a full reload |
