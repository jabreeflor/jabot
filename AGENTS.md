# Agent Instructions

Before making changes in this repository, read and follow:

- `CLAUDE.md`
- All applicable instructions and configuration under `.claude/`

Jabstack (`/create-pr-artifact`, `/gauntlet-loop`, `gauntlet-critic`) is
vendored at `plugins/jabstack/`. Cursor loads it via
`.cursor-plugin/marketplace.json`; Codex via `.agents/plugins/marketplace.json`.
See `plugins/README.md`.

