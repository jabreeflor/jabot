# Harness Integration

**The core technical bet.** JaBot wraps coding agents (Claude Code, Codex, Pi as
examples; user brings their own harness, like Buzz does) but keeps our chat UI.
We need to know how to drive each harness programmatically and normalize their
output into our message/toolblock format.

**Findings (2026-08):** questions below are answered in
[findings.md](findings.md). Deep dives: [acp.md](acp.md),
[claude-code.md](claude-code.md), [codex.md](codex.md), [pi.md](pi.md),
[buzz.md](buzz.md), [adapter-design.md](adapter-design.md). Headline:
speak ACP; do not PTY-wrap TUIs.

## Questions to answer

1. **Integration mode per harness** — for each of Claude Code, Codex, Pi:
   - Headless / non-interactive mode? (e.g. `claude -p` with `--output-format stream-json`, `codex exec`)
   - SDK? (Claude Agent SDK, Codex SDK/protocol)
   - Or do we have to PTY-wrap the interactive TUI and parse the screen?
2. **Is there a standard protocol?** Look hard at ACP (Agent Client Protocol, from Zed)
   — adapters already exist for Claude Code and other agents. If ACP covers us,
   our "harness adapter" layer may mostly be "speak ACP + ship a few adapters."
3. **How does Buzz do it?** It's the named prior art. Find out what it wraps and how.
4. **Event model** — can we get structured events for: assistant text, tool calls
   (read/edit/bash), permission requests, task completion, errors? Our UI renders
   these as bubbles + toolblocks, so structured > scraped.
5. **Permission prompts** — when the harness asks "allow this edit/command?",
   how do we surface and answer that from our UI?
6. **"Custom" harness** — what's the minimal contract a user-supplied harness must
   meet? (command + output format? ACP? a config file?)
7. **Session identity** — how do we start, name, resume, and kill a session per
   harness? (feeds session-lifecycle research)

## What this blocks (future issues)

- Harness adapter interface / trait definition
- Claude Code adapter, Codex adapter, Pi adapter, BYO-harness config
- Chat transcript renderer (needs the normalized event model)
- Permission-prompt UI
- Everything in [session-lifecycle](../session-lifecycle/brief.md)
