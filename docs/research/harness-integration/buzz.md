# How Buzz does it

Named prior art in the brief. Buzz (Block, [github.com/block/buzz](https://github.com/block/buzz))
is a desktop agent manager whose entire harness story is **ACP**. Broader
setup (relay, supervisor, pairing, personas) lives in
[setup-porting/buzz.md](../setup-porting/buzz.md).

JaBot should copy the *seam*, not the product (Buzz is Nostr/team-oriented;
we are a personal messenger with Inbox).

## Shape

```
Desktop (Tauri)  ──spawns──►  buzz-acp  ──stdio ACP──►  agent binary
     │                            │
     │                            └── env: BUZZ_ACP_AGENT_COMMAND
     │                                BUZZ_ACP_AGENT_ARGS
     └── never talks to Claude/Codex/Pi directly
```

The desktop does **not** spawn `claude` or `codex`. It spawns `buzz-acp`,
which speaks ACP to whatever command was configured. Custom harnesses are
the same path.

That is the client/host split [remote-and-mobile](../remote-and-mobile/brief.md)
asks about, already done locally: UI vs harness supervisor. Remote is "same
protocol, different machine."

## Supported agents

From `crates/buzz-acp/README.md`:

- Any agent that speaks ACP over stdio.
- **Goose** (native).
- **Codex** via [codex-acp](https://github.com/agentclientprotocol/codex-acp).
- **Claude Code** via [claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp).
  Older `claude-code-acp` is treated as the same zero-arg runtime.

Example:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export BUZZ_ACP_AGENT_COMMAND="claude-agent-acp"
buzz-acp
```

## Bring Your Own Harness (three tiers)

Implemented in desktop as a data-driven catalog ([PR #2773](https://github.com/block/buzz/pull/2773)):

| Tier | What | Examples |
|---|---|---|
| 1 — compiled-in | Auto-installers, auth probes, reserved ids | `goose`, `claude`, `codex`, `buzz-agent` |
| 2 — presets | Static table, PATH-probed, not user-editable | Cursor (`cursor-agent acp`), Oh My Pi (`omp acp`), Grok, OpenCode, Kimi, Amp, Hermes, OpenClaw |
| 3 — user JSON | Drop a file in `custom_harnesses/` or Settings UI | Anything ACP |

Reserved ids cannot be overridden by custom JSON.

### Custom harness JSON

```json
{
  "id": "my-agent",
  "label": "My Agent",
  "command": "my-agent-bin",
  "args": ["acp"],
  "env": {
    "MY_AGENT_MODE": "acp"
  },
  "installInstructionsUrl": "https://example.com/docs",
  "installHint": "Download from example.com"
}
```

Rules worth copying:

- `id`: `[a-z0-9_][a-z0-9_-]*`
- `command`: name or absolute path, required
- `args`: optional defaults; instance-level args can override
- `env`: floor; user/global env overrides; host-reserved keys stripped
- `installHint` / URL shown when the binary is missing — no install scripts
  for tier 3
- Verify the vendor's **actual ACP entrypoint** (subcommand is often `acp`)
  before adding a preset. Do not trust a blog.

This is the answer to brief question 6. JaBot's Custom card should be this
form, not "point at any TUI and we scrape the screen."

## What Buzz does *not* do (and we shouldn't either)

- PTY-wrap Ink/Ratatui UIs.
- Per-harness event parsers in the UI.
- First-class support for agents that only have a TUI.

If a user wants a TUI that cannot speak ACP, the honest MVP is: refuse, or
offer an escape-hatch terminal pane later ([app-shell](../app-shell/brief.md)
question 4) — not parse ANSI for chat bubbles.

## Process management takeaway

Buzz's desktop tracks PIDs, logs, readiness, auth probes, and sweeps
orphans. Folded JaBot threads will need the same supervisor regardless of
Electron vs Tauri. ACP does not define process lifetime; the host does.
See [session-lifecycle](../session-lifecycle/brief.md) and
[app-shell](../app-shell/brief.md).
