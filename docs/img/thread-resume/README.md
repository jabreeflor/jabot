# Resume, wired to `thread/resume`

Evidence for #90. All frames are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, on the same thread
in the same state — no adapter attached, `resumable: true`, which is what a
quit or an idle evict leaves behind. The host answers the same in every frame;
the only difference is whether the fix is applied.

| file | what it shows |
| --- | --- |
| `before.png` / `before-head.png` | the header as it shipped. Nothing offers to reattach; the pane looks live and is not |
| `after.png` / `after-head.png` | a Resume button beside Fold |
| `after-drifted.png` | what a `drifted` outcome renders after clicking it |

The `-head` pair is the header cropped, since the button is a small part of a
1180-wide window.

`thread/resume` has been implemented, routed, typed and e2e-covered since #21,
and `HostClient.resumeThread` had no caller anywhere in `src/`. The renderer's
only reattach path was `thread/reopen` — a store write that spawns nothing — so
the conversation came back only when the user happened to send another prompt.

`after-drifted.png` is the frame worth reading twice. The outcome is six
answers, not a success and a failure, and `drifted` is neither: the thread is
fine, but the *stored session* is a different job now, so resuming it would
continue someone else's. The line is the host's own sentence plus the fields
that moved, in the words the drift notice already uses. A bare "could not
resume" would send that user, a user whose folder is gone (`cwd_missing`) and
a user whose adapter cannot resume at all (`unsupported`) to three different
wrong fixes.

The capitalised second sentence there is a correction the screenshot caught:
the first cut read "…matches this thread. the engine and the folder have
moved.", which reads as a typo rather than as two facts. There is a test for
both that and for a host `detail` that arrives with no full stop.
