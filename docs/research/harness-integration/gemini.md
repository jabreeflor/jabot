# Gemini CLI

Google's coding CLI. Native ACP — no third-party wrapper. Researched 2026-09-07
against [ACP mode](https://geminicli.com/docs/cli/acp-mode/) (docs updated
2026-04-10), [authentication](https://geminicli.com/docs/get-started/authentication/),
and the `packages/cli/src/acp/acpClient.ts` implementation.

## Integration mode

| Mode | How | Use in JaBot |
| --- | --- | --- |
| **ACP (preferred)** | `gemini --acp` (older: `--experimental-acp`) | Same ACP client as Claude / Codex / Pi. |
| Interactive REPL | `gemini` | Do not wrap. |
| Headless prompt | `gemini -p` | Not used. Approvals and streaming belong on ACP. |

`--acp` is stdio JSON-RPC. Methods: `initialize`, `authenticate`,
`session/new`, `session/load`, `session/prompt`, `session/cancel`, plus
`session/set_mode` and `unstable_setSessionModel`. File access can be
proxied through the ACP client; JaBot currently advertises no FS
capability and lets the agent use its own cwd.

## Auth and the account-profile system

Gemini CLI stores auth in the **user** profile, not per JaBot bot:

- `~/.gemini/settings.json` — `security.auth.selectedType` / `enforcedType`
- `~/.gemini/oauth_creds.json` and `google_accounts.json` (legacy file;
  newer builds migrate tokens into the OS keychain)
- `~/.gemini/.env`
- Process env: `GEMINI_API_KEY`, `GOOGLE_API_KEY`, Vertex
  (`GOOGLE_CLOUD_PROJECT`, `GOOGLE_APPLICATION_CREDENTIALS`)

JaBot can use that profile when it is present: the Doctor treats a
selected auth type, a cached OAuth file, or an exported key as signed in,
and a spawned `gemini --acp` inherits the same `HOME` and env.

### Upstream isolation limitations

1. **One profile per OS user.** Two Gemini threads, or two crew bots on
   Gemini, share `~/.gemini`. There is no `GEMINI_HOME` split like Hermes
   profiles.
2. **JaBot's Google MCP grant is not this grant.** Gmail / Calendar / Drive
   OAuth lives in JaBot's keychain for MCP servers passed on `session/new`.
   Gemini CLI will not see it, and we must not write JaBot refresh tokens
   into `~/.gemini`.
3. **ACP `authenticate` persists.** The vendor client writes
   `security.auth.selectedType` to user settings. An API key that is only
   ambient in the environment has been observed to rewrite a durable OAuth
   selection (gemini-cli#25687). Do not invent a second auth store to
   paper over that.
4. **Unpaid / Google One note.** Gemini CLI docs state that unpaid-tier
   and Google One users were moved to Antigravity CLI on 2026-06-18. A
   machine that only has Antigravity will look like "CLI missing" until
   Gemini CLI itself is installed.

## Capabilities

From `initialize`:

| Capability | Advertised | JaBot |
| --- | --- | --- |
| Streaming chunks | yes | same ACP pump |
| Tools / permissions | yes (`session/request_permission`) | permission broker |
| Cancel | yes | `session/cancel` |
| `loadSession` | **yes** | supervisor load path |
| `session/resume` | **no** | not sent; load or new |
| `session/close` | **no** | skipped |
| MCP HTTP / SSE | yes | host-selected servers only |

Empty successful turns are rewritten to `empty_response` (#181 / #203)
before they hit the transcript or the run ledger.

## Doctor

`Readiness::Inspect { Gemini }`:

1. `gemini` on PATH, else `cli_missing`.
2. `gemini --help` lists `--acp` or `--experimental-acp`, else
   `adapter_outdated`.
3. Auth from env / `~/.gemini`, else `logged_out`.
4. Vertex without a project, or `model.name: ""`, else `invalid_config`.
