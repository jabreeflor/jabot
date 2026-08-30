# Pull Requests — closed without merging

Evidence for giving closed pull requests somewhere to live on the board.
Captured by rendering the real `PullRequestsView` in Chromium at
deviceScaleFactor 2, over the same fixtures.

| file | what it shows |
| --- | --- |
| `before.png` | what shipped: the closed PR is nowhere on the board |
| `after.png` | a `CLOSED WITHOUT MERGING` section holding it, under `RECENTLY MERGED` |

`before.png` renders the fixtures with the closed row removed, because that is
exactly what the board did with it. `closed` was parsed by `github.rs`, kept by
`store/pr.rs`, carried on the wire as `PrWireStatus`, passed through `prRow`,
and `prTag` had returned a `CLOSED` pill for it all along — but
`PullRequestsView` composed its sections from only `open`, `draft` and
`merged`, so no code path could put the row on screen. A pull request someone
closed simply left the board, which reads as "JaBot lost it" rather than
"somebody closed it".

Note in `after.png` that the **Open tab still counts 2**. A closed PR is
finished work; it belongs on the board as a record, not in the count of what
still wants attention.

The section sits on the Open tab as well as the Merged one, for the same reason
`RECENTLY MERGED` is there: the question a vanished row raises gets asked while
looking at Open. Empty sections are already dropped, so a board with nothing
closed looks exactly as it did.
