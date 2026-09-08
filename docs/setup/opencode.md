# Set up OpenCode

OpenCode is a first-class JaBot harness. The CLI *is* the ACP adapter —
there is nothing extra to `npm i -g`.

Vendor docs: [opencode.ai/docs/acp](https://opencode.ai/docs/acp/).

## 1. Install the CLI

```bash
curl -fsSL https://opencode.ai/install | bash
# or
npm i -g opencode-ai
```

Confirm:

```bash
opencode --version
opencode acp --help
```

`acp --help` must succeed. If it does not, run `opencode upgrade` — an
older build has no ACP subcommand, and JaBot will say the adapter is
outdated.

## 2. Sign in

```bash
opencode auth login
```

Or export a provider key JaBot already treats as credentials:

- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`
- `OPENCODE_API_KEY`
- `GOOGLE_GENERATIVE_AI_API_KEY` / `GEMINI_API_KEY`

Check:

```bash
opencode auth list
opencode models
```

An empty auth list with no env key is "logged out". Signed in with no
models is a config problem, not a missing install.

## 3. Pick a model

OpenCode reads `model` from config as `provider/model`. Project file
wins over the user file unless New Chat overrides it.

`opencode.json` in the repo (or `~/.config/opencode/opencode.json`):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4-5"
}
```

In JaBot, New Chat shows a model chip when OpenCode is selected.
"Project default" leaves the files above alone. A pick is snapshotted
on that thread only.

## 4. Enable it in JaBot

1. Settings → Harnesses → leave **OpenCode** checked.
2. New Chat → Harness → **OpenCode**.
3. Optionally pick a model, then start the thread.

Onboarding uses the same catalog: choosing OpenCode runs Doctor and
shows the install / login / model remedy. There is no "Install adapter"
button — the CLI already speaks ACP.

## Permissions and tools

JaBot's permission broker still answers `session/request_permission`.
Host-selected tools on the bot are the MCP list passed to `session/new`.
OpenCode may also load MCP and permission rules from the project's
`opencode.json`. A deny in JaBot is a deny; do not rely on the project's
file to bypass the host.

## Accounts

OpenCode stores credentials in one file:

```
~/.local/share/opencode/auth.json
```

Two JaBot bots cannot hold two OpenCode logins at once. Isolating them
needs a dedicated data directory (`XDG_DATA_HOME`, or `OPENCODE_DATA_DIR`
/ `OPENCODE_APPNAME` when the vendor ships those). JaBot will set that
when the account-profile system lands (#218). Until then, assume one
OpenCode identity per machine.

## Doctor statuses

| What you see | What to do |
|---|---|
| CLI missing | Install OpenCode (step 1) |
| Adapter outdated | `opencode upgrade` so `acp` exists |
| Logged out | `opencode auth login` or export a provider key |
| Invalid config | Set `model` in `opencode.json`, or pick one in New Chat |
