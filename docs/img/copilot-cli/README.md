# GitHub Copilot CLI harness — screenshot evidence

Captured with `./scripts/live.sh` (Vite + real `jabot-hostd`) and
`scripts/dev/shot.mjs` against this worktree. Not mocked or hand-drawn.

| file | what it shows |
| --- | --- |
| `onboarding-engine.png` | First-run engine picker: GitHub Copilot is a shipped card with its own mark and Doctor install hint. |
| `onboarding-doctor.png` | Copilot selected; Adapter setup reports `cli_missing` and the install/`copilot login` remedy. |
| `new-chat-picker.png` | New Chat harness menu lists GitHub Copilot among the shipped engines. |
| `new-chat-copilot-selected.png` | New Chat chip set to GitHub Copilot; Doctor install text under the composer. |
| `settings-harnesses.png` | Settings → Harnesses: Copilot enabled, with capability notes that refuse to claim resume. |
| `settings-copilot-disabled.png` | The same row unchecked. |
| `new-chat-without-copilot.png` | After disable, New Chat’s picker no longer offers Copilot. |
