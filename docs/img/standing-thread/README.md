# A host thread that lives in no folder

Evidence for #106. Captured by running the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, clicking the same
Inbox card in both — `Overnight mail summarised`, on `bot-writer`, a bot's
standing thread whose `folder_id` is null. The only difference between the two
frames is whether the fix is applied.

| file | what it shows |
| --- | --- |
| `before.png` | "That thread is gone. Check the Inbox." — what shipped |
| `after.png` | the standing thread open, with its transcript, header and Fold control |

`folder/list` walks folder rows, so a thread with no folder is not in the set
the shell flattens to decide "is this the host's?". `inbox/list` is not
folder-scoped, so the card was there and clickable and led to the dead end
above. Worse than the dead end: fold, archive and delete on that thread went to
the mock reducer, so the row moved on screen while the host's permissions, runs
and process behind it kept going.

The fix resolves the selected id against `thread/state` when no folder claims
it — one call, for the thread the user is actually looking at, rather than a
call per bot on every load. A thread the host does not know stays `null`, which
leaves every fixture path exactly as it was.
