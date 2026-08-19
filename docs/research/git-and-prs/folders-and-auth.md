# Folders and auth

How a sidebar folder relates to a git repo, and how JaBot talks to GitHub.
Complements [findings.md](findings.md) Q2 and Q4.

## Folders = repos?

**Yes for MVP.** A CODE sidebar folder is **one registered local
directory**, almost always the root of a git worktree (usually the user's
main checkout). It is not a tag, not a multi-repo group, not a GitHub org.

The prototype's `JABOT-APP` / `GLOBNET-SYNC` match Conductor "projects"
and Codex "local projects": named containers of threads, each bound to one
path. "New thread in jabot-app" means: spawn a harness session whose cwd
is a worktree of that repo ([worktrees.md](worktrees.md)).

| Model | Use? |
|---|---|
| Folder = one local path | **MVP.** Register via directory picker. |
| Folder = GitHub `owner/repo` without a clone | No. Agents need a filesystem. Clone first, then register. |
| Folder = grouping of several repos | Later (monorepo / workspaces). ACP `additionalDirectories` exists but harness research deferred it. |
| Folder = git remote only | No. Derive remote *from* the path. |

### Registration

1. User picks a directory (or drops a path).
2. Host runs `git rev-parse --show-toplevel` (fail → still allow as a
   non-git folder: threads work, PR view empty, badge "not a git repo").
3. Store:

   ```text
   folderId
   displayName          basename, user-editable (JABOT-APP)
   repoRoot             absolute; the registered checkout, not a thread worktree
   originUrl            git remote get-url origin (optional)
   owner, name, host    parsed from origin (github.com vs GHES vs gitlab)
   defaultBranch        origin/HEAD or gh repo view --json defaultBranchRef
   setupCommand         optional
   filesToCopy          optional; plus honor .worktreeinclude
   ```

4. Display name is ours. Do not rename the directory.

Parse remotes the way `gh` does: `git@github.com:org/repo.git` and
`https://github.com/org/repo.git` both become `org/repo` on host
`github.com`. SSH vs HTTPS does not matter for API calls; it matters for
the **agent** pushing. Agents inherit the user's git credential helper /
`gh` as credential manager. Do not inject tokens into remotes.

If `origin` is GitLab or unknown: folder still works for threads; GitHub
PR view skips it (see forge note below).

### Threads vs folders

- Folder row = project. Lives forever until the user removes it.
- Thread row = ACP session + cwd (worktree path) + optional PR link.
- Removing a folder does not delete `repoRoot`. It stops listing threads
  and should prompt before removing JaBot worktrees under
  `~/.jabot/worktrees/<folderId>/`.

New Chat without a folder: require picking/creating a folder. The
prototype's New Chat card still needs a cwd; ACP `session/new` refuses a
relative one.

## Auth

### Decision

**Reuse the user's existing GitHub CLI login for MVP.** Probe
`gh auth status`. Call `gh auth token` (or `gh api graphql` which uses
the same store) for host-side API. Same pattern as harness-integration:
reuse local Claude / Codex login; do not invent a parallel OAuth product
on day one.

Do **not** ship a JaBot GitHub App or OAuth App for MVP.

### Why `gh`, not our own OAuth

| Option | Fit for a personal desktop app |
|---|---|
| **`gh auth login` already on the machine** | Best. Coding agents already run `gh pr create`. One identity, one org SSO dance, `gh` is often pre-approved in orgs that lock unknown OAuth apps. |
| GitHub App + device/web flow | Correct *later*: finer permissions, short-lived user tokens (8h + refresh), optional webhooks. Public native clients cannot hide a client secret ([GitHub App best practices](https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/best-practices-for-creating-a-github-app)); device flow is the documented path for CLIs/desktops. Extra install + org allow-list friction. |
| Classic PAT pasted into Settings | Works, worse UX, easy to over-scope, we would store a secret (data-and-persistence). Fallback if `gh` is missing. |
| Installation access tokens | Wrong. Those act as the *app*, not the user. Merge/review must be the user. |

GitHub Desktop uses browser OAuth and OS keychain — that is the polished
end state, not the MVP. GitHub's own CLI tutorial for "building a CLI
with a GitHub App" is device flow
([docs](https://docs.github.com/en/apps/creating-github-apps/writing-code-for-a-github-app/building-a-cli-with-a-github-app)).

`gh auth token` prints the token for the active account
([manual](https://cli.github.com/manual/gh_auth_token)). `GH_TOKEN` /
`GITHUB_TOKEN` override the store
([environment](https://cli.github.com/manual/gh_help_environment)). Prefer
invoking `gh` as a child (inherits the user's environment) over copying
the token into JaBot's DB. If we must cache, Keychain — never plaintext
in the thread store (handoff to data-and-persistence).

### Host vs agent

Two consumers, one login:

1. **Agent (in the worktree):** runs `gh` / `git push` with the user's
   PATH and env. JaBot does not PTY-wrap; we do not special-case `gh`.
   Permission UI still gates `execute` of `gh pr create` / `gh pr merge`
   like any other command.
2. **Host (PR view, Inbox link, Merge button):** GraphQL via Octokit or
   `gh api graphql`, authenticated as the same user.

If `gh` is missing: PR view shows install hint (`brew install gh` / GitHub
CLI). Threads still run. Do not block New Chat on GitHub auth — only the
PR surface.

### Scopes we actually need

Whatever `gh auth login` already granted is enough for list/view/checks
on repos the user can see. Merge and `workflow` files may need
`gh auth refresh -s workflow` in some orgs; surface that error, don't
pre-emptively refresh.

GitHub Enterprise: `gh auth token -h <host>` and `GH_HOST`. Parse
`origin` host; don't assume `github.com`.

## GitHub-only MVP vs GitLab later

**GitHub only for the Pull Requests view and Inbox PR chips.** The
prototype copy is "Pull Requests", Merge, checks — GitHub vocabulary.

Claude's own `--worktree "#n"` already fetches GitLab
`merge-requests/<n>/head`, and [glab](https://docs.gitlab.com/cli/) is
the `gh` equivalent (`glab mr`, `glab ci`). That is a **second forge
adapter**, not a reason to abstract on day one.

Keep a thin internal shape so we do not paint ourselves into a corner:

```text
ForgePr
  id, number, url, title, isDraft, state   # open | merged | closed
  repo { host, owner, name }
  headRef, baseRef
  additions, deletions, changedFiles
  checkRollup { state, items[] }           # pending | success | failure | …
  threadId                                 # JaBot link
```

MVP `GithubForge` fills that from GraphQL. Later `GitlabForge` from
`glab api` / GitLab GraphQL. Do not rename the UI to "Merge Requests"
until a GitLab folder exists.

Non-git folders and non-GitHub remotes: omit from the PR view, don't
error the whole page.
