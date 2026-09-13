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

## Questions and plan reviews (Cursor extensions)

Cursor's ACP bridge sends two requests that block the CLI until a client
answers, and neither is a permission:

- `cursor/ask_question` — one or more questions, each with the agent's own
  options. JaBot draws a **question card** in the thread: radios for a single
  choice, checkboxes where the agent allows several, then **Send answer**,
  **Skip** or **Cancel**. The answer goes back with the agent's own question
  and option ids. Free text is not offered: Cursor's bridge only reads
  selected option ids, so a typed answer would never reach the model.
- `cursor/create_plan` — a plan to review before the agent carries it out.
  JaBot draws a **plan review card** with the plan body, its steps and
  phases, and **Accept plan**, **Reject…** (with an optional reason) or
  **Not now**. JaBot sends no `planUri`; Cursor writes its own plan file and
  says where it put it.

Both appear in the Inbox as **QUESTION** and **PLAN REVIEW** cards (never as
permissions), can be answered from a paired phone, and are settled exactly
once: a second click reports what stands. Accepting a plan is a decision
about what the agent will do, not a grant of tool permission — nothing in
the permission broker reads it.

A card that can no longer be answered says why instead of offering buttons:
**expired** when the turn ended while it was open, **unavailable** when the
Cursor process (or JaBot) went away. Neither is replayed on restart; a
question the agent stopped waiting on is closed, not resurfaced.

Cursor does not advertise these extensions in `initialize` and does not check
the client before sending them, so there is nothing to negotiate. Any other
`cursor/*` request (for example `cursor/update_todos`), and a question whose
payload JaBot cannot read, is refused with JSON-RPC `-32601` — the signal
Cursor treats as "the client cannot do this", after which it falls back to
plain permission prompts or writes the plan itself. A turn never hangs on an
extension nobody rendered, and no support is claimed for one nobody
implemented.

Verified against `cursor-agent 2026.08.04` (bundled ACP bridge). Resume uses
ACP `session/load` / `session/resume` only when the adapter advertises them.