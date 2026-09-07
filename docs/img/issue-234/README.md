# Issue #234 — Playwright visual + browser a11y

Evidence from the real app via `./scripts/live.sh` against a live
`jabot-hostd`. Automated baselines live under
`tests/browser/__screenshots__/` and are compared by `toHaveScreenshot`.
These PNGs are the human-readable proof the suite was pointed at the
same states.

| file | what it shows |
| --- | --- |
| `onboarding-dark.png` | first-run name pane, dark |
| `sidebar-stream-dark.png` | sidebar + Chief after a fake-acp turn |
| `inbox-permission-dark.png` | Inbox permission card |
| `new-chat-dark.png` | New Chat empty window |
| `settings-dark.png` | Settings, Appearance |
| `schedules-dark.png` | Schedules list with a seeded job |
| `pr-board-dark.png` | Pull Requests board (signed out) |

The PR explainer is `walkthrough.html`. Open it in a browser, or via the
htmlpreview link in the PR’s Artifact section. `artifact-full.png` and
`artifact-section-*.png` are screenshots of that rendered document.

Neither Chromium nor this live Chromium shot is native Tauri / WKWebView
proof. See `docs/browser-tests.md`.
