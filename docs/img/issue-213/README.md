# Issue #213 evidence

Real Vite renderer connected to this worktree’s `jabot-hostd`. A test-only ACP
process writes a known stderr line and either ends empty or exits before a
reply; the host classifies the log and the renderer shows the specific cause
instead of `failed: no reply`.

- `not-signed-in.png` — empty `end_turn` plus a Claude sign-in error on stderr
- `adapter-exit.png` — harness process exits on `session/prompt` with no ACP response
