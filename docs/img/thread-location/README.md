# Where a code thread is actually editing

Evidence for #93. Both frames are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, on the same code
thread. The host serves the same `thread/state` in both — `worktreePath` and
`branch` included, in the shape `tests/e2e/worktree.test.ts` already asserts
the real host produces — and the only difference is whether the fix is applied.

| file | what it shows |
| --- | --- |
| `before.png` / `before-head.png` | the header as it shipped: title, engine, status. Nothing says where the agent is working |
| `after.png` / `after-head.png` | the same header with `jabot/t-auth` beside the engine chip |

The `-head` pair is the header cropped, because the chip is a few pixels of a
1180-wide window and the point is easy to miss in the full frame.

A code thread opened in a git folder does not run in the user's checkout. It
runs in a host-owned worktree under the app data directory, on a `jabot/<id>`
branch — which is right, because two threads in one repo cannot stand on each
other's uncommitted work. Until now nothing on screen said so, and someone
looking at a running thread would go looking for the edits in the wrong tree.

The full path is the chip's `title`, so it is one hover away:
`/Users/j/Library/Application Support/jabot/worktrees/t-auth` here. It is not
in these frames because a native tooltip is not part of the page and does not
appear in a screenshot; the assertion in
`src/__tests__/thread-stream.test.tsx` is what pins it, querying the chip *by*
that title.

Nothing is drawn for a thread that works in place — a bot's standing thread, a
folder that is not a checkout, the "use my own checkout" opt-out. A chip on
every thread would say nothing about any of them.
