# Jabstack maintenance

Jabstack is one plugin distributed for ChatGPT Work, Codex, Claude Code, Cursor,
and Agent Plugins clients. Keep all formats in sync in the same change.

- Keep the plugin name and release version identical in `plugin.json`,
  `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`,
  `.cursor-plugin/plugin.json`, and the Jabstack entry in
  `.claude-plugin/marketplace.json`. The marketplace's own `metadata.version`
  is separate from the plugin release version.
- Keep shared metadata and descriptions consistent with the same capabilities.
  Preserve format-specific fields: the root manifest's schema, OpenAI's
  `skills` and `interface` fields, Claude's marketplace configuration, and
  Cursor's `displayName`, `skills`, and `agents` paths. Do not blindly copy a
  manifest into a different format. This repo is a single Cursor plugin, so do
  not add `.cursor-plugin/marketplace.json`.
- Treat `skills/` as the single source for skill instructions across all clients.
  Update client fallbacks and referenced `agents/` instructions together when
  behavior changes. Do not maintain divergent per-client copies of a skill.
- Update README installation notes, skill listings, and layout when they change.
- When making a release or supplying an export, include the same current skills,
  references, and manifests in every distribution. Update any existing local
  distribution copies and supplied ZIPs within the task's scope; do not leave a
  stale archive beside updated source. Match archive names to the plugin version.
- Before handing off changes, parse all manifests, check plugin name/version
  agreement, validate changed skills and plugin packaging, and inspect exported
  contents for missing references or stale files. Do not claim live installation
  testing unless it was actually performed.
- No export script is required or wanted. Keep packaging helper scripts out of
  this repository and distributed plugin unless the user requests one.

`AGENTS.md` is the shared maintenance policy. `CLAUDE.md` imports it so Claude and
Codex follow the same instructions; keep that import intact.
