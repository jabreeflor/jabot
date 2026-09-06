# New Chat without the Advanced selection

Evidence for the removal of the worktree controls (#23, #92). All three are the
real `NewChatModal` inside the real `App`, in Chromium at deviceScaleFactor 2,
with a folder picked — the only state in which the disclosure ever appeared.

| file | what it shows |
| --- | --- |
| `before.png` | the card as it was: the ADVANCED disclosure sitting under the workspace |
| `before-expanded.png` | the disclosure opened: the checkout opt-out and BASE BRANCH |
| `after.png` | the card now: no disclosure, no opt-out, no base branch |

A folder thread always gets its own worktree from the host's default base ref.
Two threads sharing one checkout is the collision worktrees exist to prevent,
and a card that shows the way out of it is a card that invites somebody to take
it, so the way out is no longer on the card.

`thread/open` still accepts `useCheckout` and `baseRef` — the Rust host honours
both and `tests/e2e/worktree.test.ts` drives them — but nothing the renderer
sends sets either.
