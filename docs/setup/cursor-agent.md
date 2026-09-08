# Set up Cursor Agent in JaBot

Cursor Agent is a first-class harness: New Chat, onboarding, Settings, and the
crew editor all list it. JaBot talks to it through `agent acp` (Agent Client
Protocol). It does **not** pass `--force`, `--yolo`, or `--approve-mcps`.
Permission prompts stay in JaBot.

## 1. Install the CLI

```bash
curl https://cursor.com/install -fsS | bash
```

Windows: `irm 'https://cursor.com/install?win32=true' | iex`

Confirm you have one of these on PATH (JaBot also searches `~/.local/bin`):

```bash
agent --version
# or, on older installs:
cursor-agent --version
```

Docs: [cursor.com/docs/cli/installation](https://cursor.com/docs/cli/installation).

## 2. Sign in

```bash
agent login
agent status          # should show your Cursor account
agent models          # should list at least one model
```

For CI or a headless box, export a user or service-account key instead:

```bash
export CURSOR_API_KEY=…    # or CURSOR_AUTH_TOKEN
```

JaBot treats that environment as the account-profile. It does not store a
second Cursor login per bot.

## 3. Enable the harness

Settings → Harnesses → **Cursor Agent**. Disabled harnesses do not appear in
New Chat or the bot editor. The preference survives relaunch.

Onboarding's engine pane offers the same card. JaBot cannot `npm i` this
adapter — the CLI *is* the adapter — so the Doctor's install hint is the
curl line above, not an in-app installer.

## 4. Start a thread

Pick **Cursor Agent** in New Chat (or set it as a bot's engine). The first
prompt must produce a persisted reply in that folder's worktree. If the CLI
is missing, you are logged out, the build is too old, or no model is
available, the Doctor says so with a fix. Those states never look like a
successful turn.

## Isolation limitation

Every Cursor thread on this Mac shares the same `agent login` (or the same
`CURSOR_API_KEY`). Crew members are isolated as JaBot bots (instructions,
tools, transcripts), not as separate Cursor accounts. Team-level MCP from
the Cursor dashboard is not available in ACP mode.

## Unsupported Cursor extensions

`cursor/ask_question` and `cursor/create_plan` block the CLI until a client
answers. JaBot declines them (`cancelled`) so a turn cannot hang waiting for
UI JaBot does not have. Resume uses ACP `session/load` / `session/resume`
only when the adapter advertises them.