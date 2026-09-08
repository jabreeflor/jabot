# Cursor Agent CLI

Research + implementation notes for [#222](https://github.com/jabreeflor/jabot/issues/222).
Official references: [Headless CLI](https://cursor.com/docs/cli/headless),
[ACP](https://cursor.com/docs/cli/acp),
[Parameters](https://cursor.com/docs/cli/reference/parameters),
[Authentication](https://cursor.com/docs/cli/reference/authentication).

## Integration mode

JaBot uses Cursor's **maintained ACP server**, not print-mode:

```bash
agent acp            # current CLI name
cursor-agent acp     # older installs / Buzz's preset
```

`--print` / `--output-format stream-json` can map into a transcript, but file
writes in print mode require `--force` / `--yolo`. The issue forbids turning
those on by default to paper over missing permission integration. ACP already
has `session/request_permission`, streaming `session/update`s, `session/cancel`,
and `session/load`.

JaBot **never** passes `--force`, `--yolo`, `--approve-mcps`, or `--trust`.
Approvals stay in the existing permission broker.

## Capabilities

| Capability | Status |
|---|---|
| Streaming (`session/update` agent chunks) | Supported |
| Tool events (`tool_call` / `tool_call_update`) | Supported |
| Permission requests | Supported — surfaced in JaBot, not auto-allowed |
| Cancellation (`session/cancel`) | Supported |
| Resume / continuation | **If advertised.** Cursor documents `session/load`. JaBot only resumes when `initialize` says `loadSession` or `sessionCapabilities.resume`. Otherwise a new ACP session is opened and that is declared, not faked. |
| `cursor/ask_question`, `cursor/create_plan` | **Unsupported.** Blocking extensions. JaBot replies `cancelled` so the turn does not hang. |
| Team-level MCP from the Cursor dashboard | **Unsupported** (upstream ACP limitation) |
| Per-bot Cursor account isolation | **Unsupported** (see below) |

## Auth / account-profile

The CLI's account-profile is one of:

1. `agent login` / `agent status` (browser login stored by the CLI)
2. `CURSOR_API_KEY` or `CURSOR_AUTH_TOKEN` in the process environment

JaBot uses whichever of those is already available. It does not mint a second
Cursor identity per crew member. **Isolation limitation:** every Cursor thread
on this machine shares that one account and quota. A JaBot bot is a persona and
tool allowlist, not a Cursor billing boundary.

Doctor probes:

- no `agent` / `cursor-agent` → `cli_missing`
- `--version` below the ACP-era floor → `adapter_outdated`
- `status` fails and no API key env → `logged_out`
- `models` fails → `invalid_config`

## Session continuation, cancellation, permissions

Evaluated against the ACP client and supervisor that already exist:

- **Continuation.** `thread/resume` calls `session/resume` or `session/load` only
  when the adapter advertised the verb. Otherwise the host opens `session/new`
  and says so.
- **Cancellation.** `session/cancel` is a notification; the supervisor already
  waits for the turn to end (or the grace) and records `cancelled`, not success.
- **Permissions.** `session/request_permission` is the only approval path. Empty
  `end_turn` with no visible reply is rewritten to `empty_response` and fails
  the run.

## Headless print mode (not used)

`agent -p --output-format stream-json` is the scripting surface. It is the
wrong default for a chat that must answer "allow this edit?": without `--force`
writes are not applied, and with `--force` JaBot never sees the ask. Kept as
the documented non-path so nobody "fixes" ACP by adding those flags.