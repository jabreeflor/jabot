# Composer model picker (#296)

Real Vite renderer + `jabot-hostd` via `./scripts/live.sh`. Captured on this
worktree after `harness/doctor` listed Claude as `cli_missing` and Fake ACP
advertised `sonnet` / `opus` from `session/new`.

| file | what it shows |
| --- | --- |
| `new-chat-claude.png` | New Chat with Claude Code selected. The **Model** chip sits between harness and host (`Harness default`). The Doctor install hint is under the composer because `claude` is not on this machine — no invented ids. |
| `new-chat-model-menu.png` | Same chrome after switching the harness chip to Fake ACP (the live-loop adapter). The model menu lists **Harness default**, **sonnet**, and **opus** — ids the adapter advertised, plus the default. |
| `live-thread.png` | Existing code thread. Composer chrome shows the stored pin (`sonnet`) above the input. |
| `live-thread-menu.png` | Opening the chip on that thread. Same advertised list. |
| `live-thread-opus.png` | After picking `opus`. The chip updates and the status line says the session is not live, so the pin applies on next spawn — not a silent no-op. `thread/state` then reported `model: "opus"`. |

Claude Code's installed-adapter listing is still open on this box: there is no
`claude` CLI, so the chip cannot show Anthropic's advertised ids here. The
disabled/default Claude chip plus the Fake ACP listing is the honest evidence
this machine can produce.
