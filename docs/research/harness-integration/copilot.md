# GitHub Copilot CLI

First-class JaBot harness (#221). Evaluated September 2026 against GitHub's
programmatic CLI reference and the ACP server docs.

## Integration mode

| Mode | How | Use in JaBot |
|---|---|---|
| **ACP server (preferred)** | `copilot --acp` (stdio by default; `--stdio` is explicit) | Same ACP client as Claude / Codex / Pi. |
| **Programmatic one-shot** | `copilot -p PROMPT -s` | CI / scripts. No permission mediation, no streaming into JaBot bubbles. |
| **Interactive TUI** | `copilot` with no `--acp` | Do not wrap. |

GitHub shipped ACP on the CLI itself (public preview, 2026-01-28). There is
no second npm adapter to install — the vendor binary *is* the adapter. A
maintained third-party ACP wrapper would be a worse contract than `copilot
--acp`.

Docs:

- [Programmatic reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference)
- [ACP server](https://docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server)
- [CLI command reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference)
- [Auth troubleshooting](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/troubleshoot-copilot-cli-auth)

## Capabilities (verified against docs, not advertised from a binary being present)

| Capability | Upstream | What JaBot claims |
|---|---|---|
| Streaming `session/update` | Yes | Supported |
| Tool events | Yes (`tool_call` / `tool_call_update`) | Supported |
| Permission requests | Yes (`session/request_permission`) | Supported — JaBot mediates; do not pass `--allow-all` |
| Cancel | Yes (`session/cancel`) | Supported |
| `session/new` | Yes | Supported |
| Resume after process exit | **No.** Sessions are process-local. `loadSession` is advertised but a new `copilot --acp` cannot see another process's sessions ([copilot-cli#1767](https://github.com/github/copilot-cli/issues/1767)). `session/close` is unimplemented. | **Not advertised.** JaBot starts a new ACP session. |
| Cross-account isolation via `/user` | Partial. `/user switch` changes the active login; `activeProfile` and `--config-dir` do **not** isolate plugins or MCP. Only `COPILOT_HOME` does. | When #218 account profiles exist, set `COPILOT_HOME` per profile. Do not claim `/user` is isolation. |

## Auth

Precedence (highest first): `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`,
`GITHUB_TOKEN`, then the credential store / `~/.copilot` (or `$COPILOT_HOME`)
login from `copilot login`. `gh auth login` is a valid source when JaBot
inherits `GH_TOKEN`.

Org and subscription policy can refuse a valid login (`Access denied by
policy settings`). That is `InvalidConfig`, not `LoggedOut`.

## Model

Unset `model` uses Copilot's default — that is not an error. An empty
`COPILOT_MODEL` or `"model": ""` in settings is.

## Account profiles (#218)

Not implemented yet. The hook is `copilot::profile_env`: it sets
`COPILOT_HOME` and nothing else. Copilot's own account switcher is a
shared-home login, not a sandbox.
