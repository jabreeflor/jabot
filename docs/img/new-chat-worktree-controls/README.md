# New Chat's worktree controls

Evidence for #92. All three are the real `NewChatModal` inside the real `App`
in Chromium at deviceScaleFactor 2, against a stubbed `host_rpc` transport
whose `thread/open` refuses an unresolvable `baseRef` with the same
`WORKTREE_FAILED` frame `worktree::resolve` produces.

| file | what it shows |
| --- | --- |
| `collapsed.png` | the card as it opens. Advanced is shut, and the three fields are the ones that shipped |
| `expanded.png` | Advanced opened: the checkout opt-out and the base branch |
| `base-ref-refused.png` | the host's own sentence about a ref the repository does not have, with the draft still in the card |

`thread/open` has accepted `useCheckout` and `baseRef` since #23 — the Rust
host honours both, and `tests/e2e/worktree.test.ts` drives them — and no
renderer ever set either. They were reachable only by writing JSON-RPC by hand.

They are behind a disclosure and shut by default on purpose. A fresh worktree
per thread is what stops two threads in one repo standing on each other's
uncommitted work, so the opt-out gets the consequence spelled out under it
rather than a tooltip, and the card does not invite anybody to reach for it.

Neither control appears at all with "No folder" selected: a scratch session has
no checkout to work in and no branch to fork from.

The base branch is disabled while the opt-out is ticked, and is not sent — a
thread working in the folder's own checkout starts on whatever is checked out
there, so a base ref beside `useCheckout` would be asking the host for two
different things at once.
