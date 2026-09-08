# OpenCode

The open-source coding agent at [opencode.ai](https://opencode.ai/). Unlike
Claude / Codex / Pi, OpenCode **is** the ACP adapter: there is no separate
npm package. The documented transport is stdio JSON-RPC via
[`opencode acp`](https://opencode.ai/docs/acp/).

Shipped as a tier-1 catalog card (`id: opencode`) in #220.

## Integration mode

| Mode | How | Use in JaBot |
|---|---|---|
| **ACP (preferred)** | `opencode acp` — first-party, stdio JSON-RPC. | Default OpenCode card. Same ACP client as Claude/Codex/Pi. |
| Interactive TUI | `opencode` with no subcommand. | Do not wrap. |
| Headless run | `opencode run "…"` | Scripting / debugging. Not the chat path. |

Vendor claim: ACP exposes the same features as the terminal — built-in
tools, custom tools and slash commands, MCP from `opencode.json`,
`AGENTS.md` project rules, agents, and the permissions system.

JaBot still owns host MCP: servers passed on `session/new` come from the
bot's tool allowlist (#18). OpenCode may *also* load MCP from the project
`opencode.json`. Ambient harness MCP is not skipped here the way Hermes
is (`HERMES_ACP_SKIP_CONFIGURED_MCP`); there is no equivalent flag.

## Launch

```
command: opencode
args:    ["acp"]
```

Do not run `opencode acp` as a Doctor probe — it is a long-lived stdio
server and will hang until killed. Probes are `acp --help`, `auth list`,
and `models`.

## Auth

Credentials live in one global file:

```
~/.local/share/opencode/auth.json
```

Sign in with `opencode auth login`, or export a provider key
(`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENCODE_API_KEY`,
`GOOGLE_GENERATIVE_AI_API_KEY`, `GEMINI_API_KEY`). `opencode auth list`
prints the connected providers; an empty list with no env key is
`logged_out`.

### Account isolation (#218)

OpenCode does not isolate concurrent accounts today. `OPENCODE_CONFIG_DIR`
overrides agents / commands / plugins, **not** `auth.json`. Isolation
needs a dedicated data directory:

| Variable | What it moves | Status |
|---|---|---|
| `XDG_DATA_HOME` | `…/opencode/auth.json` (current, portable) | Works today |
| `OPENCODE_DATA_DIR` / `OPENCODE_APPNAME` | Vendor portable-mode proposal | Not universally shipped |

JaBot does **not** invent a profile system here. When #218 lands, the
host can set `XDG_DATA_HOME` (and `OPENCODE_DATA_DIR` when present) per
account profile. Until then the isolation note on the catalog card is
the honest answer: one `auth.json` for the machine.

## Model selection

OpenCode reads `model` from layered config
([docs](https://opencode.ai/docs/config/)), `provider/model` form:

```json
{ "model": "anthropic/claude-sonnet-4-5" }
```

Precedence that matters here, high last:

1. `~/.config/opencode/opencode.json`
2. project `opencode.json` (ACP `cwd`)
3. `OPENCODE_CONFIG_CONTENT` — inline JSON, runtime override

`session/new` `model` and `session/set_config` are version-dependent.
The host does all three so a New Chat pick wins without rewriting the
user's files:

- persist `model` on the thread's `runtime_json`
- pass it on `session/new`
- best-effort `session/set_config` (`configId: model`)
- spawn-time `OPENCODE_CONFIG_CONTENT={"model":"…"}` when the thread
  recorded one and the caller did not already set that env

Project config remains the default when New Chat leaves the picker on
"Project default".

## Capabilities

Declared on the catalog card (handshake still wins per process):

| Capability | Declared | Notes |
|---|---|---|
| streaming | yes | ACP `session/update` agent chunks |
| tools | yes | `tool_call` / `tool_call_update` |
| permissions | yes | `session/request_permission`; host broker, not silent allow |
| cancel | yes | `session/cancel` |
| resume | yes | `session/resume` when the process advertises it |

An empty successful turn is rewritten to `empty_response` and a failed
run — never a silent success (#203). Startup / auth failures on
`session/new` surface as RPC errors.

## Doctor

`Readiness::AuthAndModels`:

| Probe | Failure | Status | Remedy |
|---|---|---|---|
| `opencode` missing | — | `cli_missing` | install hint |
| `opencode acp --help` non-zero | no ACP subcommand | `adapter_outdated` | `opencode upgrade` |
| `opencode auth list` empty and no env key | not signed in | `logged_out` | `opencode auth login` or export a key |
| `opencode models` empty | signed in, nothing usable | `invalid_config` | set `model` in `opencode.json` or pick one in New Chat |

Deep Doctor still handshakes ACP and reports `adapter_outdated` when
the protocol version is below 1.

## Tests

`src-tauri/tests/opencode.rs` drives the real host against
`fake-acp-agent`: catalog card, a persisted reply on `harnessId:
opencode`, `auth-fail` as an error, cancel that is not success, and
`empty-reply` → `empty_response`.

## Sources

- [opencode.ai/docs/acp](https://opencode.ai/docs/acp/)
- [opencode.ai/docs/config](https://opencode.ai/docs/config/)
- Setup for humans: [docs/setup/opencode.md](../../setup/opencode.md)
