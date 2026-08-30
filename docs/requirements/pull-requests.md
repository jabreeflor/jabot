# Pull Requests view + thread↔PR linkage

**Issue:** #28
**Status:** Implemented — `src-tauri/src/host/pr/`, `src/views/PullRequestsView.tsx`, `src/views/pulls.ts`, `src/components/GithubSignInModal.tsx`

## What it is

A view of GitHub pull requests relevant to the user's registered repos,
linked back to the JaBot thread that produced them (when a bot opened
the PR from its worktree branch), plus PR cards surfaced in the Inbox.

## Why

For a coding-agent product, "the bot opened a PR" is one of the most
important outcomes a user needs surfaced — this closes the loop from
thread → worktree branch → real GitHub PR → back into the UI the user is
already watching.

## Requirements

1. GitHub access requires sign-in via `GithubSignInModal.tsx`; PR data
   is not fetched without an authenticated session
   (`src-tauri/src/host/pr/github.rs`).
2. `detect.rs` associates a thread with a PR when the thread's worktree
   branch (see [worktree-manager.md](worktree-manager.md)) has an open
   PR against the repo's default branch — this linkage is automatic, not
   manually entered by the user.
3. `card.rs` renders PR summary data (title, status, checks, review
   state) into the compact card shape used both in the Pull Requests
   view and in Inbox entries — one card representation, two
   presentations.
4. The Pull Requests view lists PRs across all registered repos the user
   has signed-in access to, not just the currently open thread's repo.
5. PR state changes (opened, checks passed/failed, merged, closed)
   surface as Inbox events for their linked thread, using the same
   event feed as run transitions (see [inbox.md](inbox.md)), so a user
   doesn't need two different places to check on a bot's work.
6. `fixtures/` provides recorded GitHub API fixtures so PR detection and
   card rendering are testable without live network access
   (`src/__tests__/pull-requests.test.tsx`, `pulls.test.tsx`,
   `github-signin.test.tsx`).
7. Losing GitHub authentication (revoked token, sign-out) degrades the
   Pull Requests view to a clear "sign in" state rather than showing
   stale or partial data as if it were current.
