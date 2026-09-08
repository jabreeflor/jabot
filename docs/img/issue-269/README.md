# Code conversation summary (#269)

Live evidence from `./scripts/live.sh` against a real `jabot-hostd`. Two
registered checkouts (`jabot`, `jabot-frontend`), a dirty worktree, attached
sources, and a non-Git folder were seeded over JSON-RPC, then driven with
`scripts/dev/shot.mjs`.

| file | what it shows |
| --- | --- |
| `header-closed.png` | Code thread header: the trigger names the selected repo (`jabot`) |
| `summary-open.png` | Popover: Changes `+1 −0`, Local, branch, Git actions, extra repo row, Sources |
| `extra-repo-selected.png` | Selecting `jabot-frontend` retargets counts, branch, and Git actions |
| `inspect-changes.png` | Inspect opens the review modal for that repo (`added.rs`) |
| `commit-push.png` | Commit or push opens the existing Git flow for the selected repo |
| `sources-all.png` | View all lists attached sources (`notes.md`, `preview.png`) |
| `not-git.png` | Non-Git folder: named status, Git actions disabled, other repos still listed |
| `walkthrough.html` | Mechanism explainer for the PR artifact |
