# Gemini CLI harness

JaBot speaks to [Gemini CLI](https://geminicli.com/docs/cli/acp-mode/) over
ACP stdio. The vendor CLI **is** the adapter: there is no separate
`gemini-acp` package.

```bash
gemini --acp
```

Official docs (verified 2026-09-07 against
[ACP mode](https://geminicli.com/docs/cli/acp-mode/), last updated 2026-04-10):
start ACP with `--acp`. Older builds only advertised `--experimental-acp`.
JaBot's Doctor reads `gemini --help` and snapshots the flag that is actually
there.

## Install

```bash
npm i -g @google/gemini-cli
```

Onboarding can pin `@google/gemini-cli@0.58.0` into `~/.local` the same way it
installs the Claude / Codex / Pi adapters. Sign-in is separate.

## Auth (account profile)

Gemini CLI owns its own user profile under `~/.gemini`:

| What | Where |
| --- | --- |
| Selected auth + model | `~/.gemini/settings.json` (`security.auth.selectedType`, `model.name`) |
| Cached Google login | `~/.gemini/oauth_creds.json`, `~/.gemini/google_accounts.json`, OS keychain |
| Env overlay | `~/.gemini/.env`, or `GEMINI_API_KEY` / Vertex ADC in the process environment |

JaBot's Google MCP grant (Gmail / Calendar / Drive) is a **different** OAuth
client. The host does not copy that token into Gemini CLI, and Gemini CLI
does not read JaBot's keychain.

**Isolation limit:** every Gemini thread on this machine shares that one
user profile. There is no per-bot or per-thread Google account. ACP
`authenticate` can persist `security.auth.selectedType` into the user
settings file, so a method picked in one session is the default for the
next. See [gemini.md](../research/harness-integration/gemini.md).

Doctor remedies:

| Status | Typical cause | Fix |
| --- | --- | --- |
| CLI missing | no `gemini` on PATH | `npm i -g @google/gemini-cli` |
| Adapter outdated | build has neither `--acp` nor `--experimental-acp` | `gemini update` |
| Logged out | no key, Vertex ADC, or `~/.gemini` profile | run `gemini` and sign in, or export `GEMINI_API_KEY` |
| Invalid config | Vertex without a project, or empty `model.name` | set `GOOGLE_CLOUD_PROJECT` or `model.name` |

## Capabilities

Negotiated at ACP `initialize`. Gemini CLI advertises `loadSession` and
prompt/MCP capabilities. It does **not** advertise `session/resume` or
`session/close`. JaBot's supervisor already handles that: try resume if
advertised, else load, else a new session — and it says so instead of
pretending a restore happened.

Streaming, tool events, permission requests, and cancel use the same ACP
client as Claude / Codex. Errors and empty `end_turn` replies are failed
turns (`empty_response`), never a silent success.
