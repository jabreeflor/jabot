# Set up GitHub Copilot CLI

JaBot talks to Copilot over Agent Client Protocol: it spawns
`copilot --acp` and uses the same session client as Claude Code and Codex.
There is no separate ACP adapter package to install.

## Install

1. Install the CLI: `npm i -g @github/copilot`  
   Official steps: [Install Copilot CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli).
2. Confirm the build speaks ACP: `copilot --help` must list `--acp`.  
   Update with `copilot update` or reinstall if it does not.
3. Sign in: `copilot login`  
   Headless / CI: export `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, or `GITHUB_TOKEN`
   (that order). A fine-grained PAT needs the **Copilot Requests** permission.
   `gh auth login` also works when JaBot inherits the token.
4. Confirm a license: [github.com/settings/copilot](https://github.com/settings/copilot).  
   If your org or enterprise has disabled Copilot CLI, JaBot will report a
   policy error — ask an admin to enable it. A personal Pro plan does not
   override an org policy that turns the CLI off.

## In JaBot

- Enable **GitHub Copilot** under Settings → Harnesses if you previously
  turned it off.
- Pick it on New Chat or on a bot. The Doctor (New Chat card and first-run
  setup) says whether the CLI is missing, too old, signed out, or blocked.
- A real prompt writes the reply into that thread's project / worktree. An
  empty or failed turn is stored as a failure, not as a successful blank
  bubble.

## What works, and what does not

Supported over ACP: streaming text, tool events, permission prompts (JaBot
asks; Copilot is not launched with `--allow-all`), and cancel.

**Resume after Copilot exits is not supported.** Upstream sessions live in
the ACP process. JaBot will start a new session rather than pretend the old
conversation was restored. `session/close` is also unimplemented upstream.

## Accounts and isolation

JaBot will use the account-profile system (#218) when that ships. Isolation
for Copilot **must** be a distinct `COPILOT_HOME` per profile.

Upstream limitations to know now:

| Mechanism | Isolates sessions | Isolates plugins / MCP |
|---|---|---|
| `/user switch` | Changes the active GitHub login | No — still one `~/.copilot` |
| `activeProfile` in `config.json` | Partial (sessions, some state) | **No** |
| `--config-dir` (deprecated) | Partial | **No** (plugins stay in `~/.copilot`) |
| `COPILOT_HOME` | Yes | **Yes** |

Until #218 lands, set `COPILOT_HOME` yourself if you need a work identity
that must not see personal plugins or MCP servers. Do not rely on Copilot's
account switcher for that.

Model: leave unset to use Copilot's default, or pin one with `COPILOT_MODEL`
/ `/model`. An empty `model` value is a configuration error.
