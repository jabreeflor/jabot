# A PR card nobody asked for

Evidence for #112. Both frames are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, sitting on the
Inbox — never on the Pull Requests tab.

| file | what it shows |
| --- | --- |
| `inbox-before.png` | the Inbox, quiet. The PR's checks are green |
| `inbox-card-without-pr-tab.png` | the same Inbox after the checks went red |

Be precise about what these prove and what they do not.

The **card's drawing** is unchanged by this PR — `pr` is a card kind the Inbox
has rendered since #28, pill and all. What changed is that the card gets
**written at all** when nobody is looking. `card::transition` only writes one
when a *refresh* observes a change, and the refresh was a `setInterval` inside
`usePullRequests` — armed only while a webview was alive and running. In the
Dock with throttled timers it stopped; under `jabot-hostd`, which has no
renderer, it never existed, so a paired phone got zero PR polling and zero PR
cards.

The card **arriving** is proved where it can be proved for real:
`tests/e2e/pr.test.ts` → *"polls GitHub and cards a change with nobody
asking"*, against the actual Rust host with a faked `gh`, which never calls
`refreshPullRequests` and fails on the old code however long it waits. These
frames are what the user sees at the end of that.
