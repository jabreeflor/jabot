# Workspace plugins

JaBot vendors [jabstack](https://github.com/jabreeflor/jabstack) so Cursor and
Codex can load `/create-pr-artifact`, `/gauntlet-loop`, and the
`gauntlet-critic` agent without a separate marketplace install.

```
plugins/jabstack/                 # plugin root (skills + gauntlet-critic)
├── .cursor-plugin/plugin.json    # Cursor manifest
├── .codex-plugin/plugin.json     # Codex / ChatGPT Work manifest
└── skills/

.cursor-plugin/marketplace.json   # Cursor marketplace → plugins/jabstack
.cursor/settings.json             # enable jabstack for this workspace
.agents/plugins/marketplace.json  # Codex marketplace → plugins/jabstack
```

Claude Code still uses the GitHub marketplace pin in `.claude/settings.json`.
The vendored tree is a snapshot; see `plugins/jabstack/SOURCE.md`.
