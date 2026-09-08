# Aider

Aider is a terminal paired-programming agent
([aider.chat](https://aider.chat)). JaBot treats it as a **tier-2 preset**:
PATH-probed, reserved id `aider`, selectable in New Chat, Settings, onboarding,
and the crew editor.

**Aider does not speak ACP.** An upstream ACP adapter PR
([Aider-AI/aider#4936](https://github.com/Aider-AI/aider/pull/4936)) is
unmerged. Community bridges exist; JaBot does not vendor them. This card
wraps Aider's [documented scripting CLI](https://aider.chat/docs/scripting.html)
in a JaBot-owned ACP adapter (`jabot --aider-acp` /
`jabot-hostd --aider-acp`). The adapter's `initialize` result says so
(`_meta.jabot.nativeAcp: false`).

## Integration mode

| Mode | How | Use in JaBot |
|---|---|---|
| **Scripting CLI (what we ship)** | `aider --message-file … --yes --no-auto-commits --no-dirty-commits --chat-history-file … --restore-chat-history` | Default. One ACP `session/prompt` = one CLI invocation in the thread cwd. |
| **Python `Coder.run()`** | Unofficial, may change without notice | Not used. |
| **Interactive TUI** | `aider` with no `--message` | Do not wrap. |
| **Native ACP** | Unmerged upstream / community bridges | Not claimed. |

## Capabilities

| Capability | Support | How |
|---|---|---|
| Streaming | Yes | `--stream`; stdout lines become `agent_message_chunk`. ANSI is stripped. |
| Multi-turn context | Yes, via Aider history | Per-session `--chat-history-file` under `$TMPDIR/jabot-aider/<sessionId>/`, plus `--restore-chat-history`. Not a native ACP session store. |
| File selection | Partial | `resource_link` / `resource` blocks and `@path` / `file.ext` mentions become extra CLI args. Otherwise Aider's repo map decides. |
| Cancellation | Yes | `session/cancel` kills the Aider process group. |
| Resume / load | Yes, adapter-side | `session/resume` reuses the history file; `session/load` replays it as text if present. |
| Tool events | Synthesized | One `execute` tool call wraps the `aider --message` run. Aider does not emit structured tool events. |
| Permission requests | **Unsupported** | Aider's confirmations are TTY. The adapter always passes `--yes` so a turn cannot hang on stdin. `_meta.jabot.unsupported` lists `session/request_permission`. |
| Native ACP | **No** | Do not advertise Aider as an ACP agent. |

Errors never look like success: spawn failure and non-zero Aider exits return
`stopReason: "error"` plus a visible agent message. Empty successful stdout
still returns `end_turn`; the host maps that to `empty_response`.

## Automatic commits and worktrees

Aider defaults to auto-committing LLM edits and to committing a dirty tree
before it starts. JaBot owns the worktree (`#23`) and the PR commit path
(`#28`), so the adapter **always** passes `--no-auto-commits --no-dirty-commits`
and floors `AIDER_AUTO_COMMITS=false` / `AIDER_DIRTY_COMMITS=false`. A user
who exported `AIDER_AUTO_COMMITS=true` cannot override the CLI flags.

Aider still *uses* git (repo map, diffs). It just does not create commits
inside the JaBot worktree.

## Auth and account-profile isolation

Aider authenticates with provider keys in the process environment
(`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, …) or in
`~/.aider.conf.yml`. JaBot does not store those keys in its vault today.
When an account-profile / user-profile RPC later exports the same names,
the adapter inherits them the way any child does (`floor_env`).

**Upstream isolation limit:** Aider's config is user-global. Every Aider
thread on the machine shares `~/.aider.conf.yml` and the host environment.
JaBot cannot give two bots different Aider accounts without Aider growing
a profile API. Do not set `HOME` per thread to fake isolation — that
breaks git.

Doctor checks, in order: `aider` on PATH, `aider --version` ≥ 0.35.0, an
API key in env or `~/.aider.conf.yml`, and a model (`AIDER_MODEL` or
`model:` in config, or a provider key that implies Aider's default).

## Setup

1. Install Aider: `python -m pip install aider-chat` (or pipx / the
   [install guide](https://aider.chat/docs/install.html)).
2. Export a provider key, or write it to `~/.aider.conf.yml`.
3. Optionally set `AIDER_MODEL` (Aider will default one when a well-known
   key is present).
4. Enable **Aider** in Settings if you disabled it. Onboarding's engine
   pane lists it with the rest of the catalog.
5. JaBot ships the ACP wrapper; there is no `npm i -g` adapter to install.
   Onboarding will not offer "Install adapter" for this card.

`harness/doctor` names the missing piece (binary, version, key, model)
instead of a generic "not installed".
