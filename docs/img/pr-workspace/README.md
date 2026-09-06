# Pull request workspace evidence

Screenshots captured from the real `PrWorkspaceView` React component running in Vite with the deterministic `tests/support/pr-workspace-fixture.ts` response. The preview transport rejected writes; no real PRs were modified during visual verification. The temporary preview entry point was removed afterward.

- [Conversation](conversation.png): description, discussion, review form, merge controls and reviewer requests at 1440px.
- [Files](files.png): numbered unified diffs, inline comment buttons and viewed-file controls at 1440px.
- [Compact layout](compact.png): stacked review content and sidebar at 780px.

The production component uses `HostClient.pullRequestDetail` and `pullRequestAction`, which route through the authenticated host to GitHub CLI. Tokens stay with `gh`. Comments, review decisions, line comments, metadata edits, reviewer requests, draft transitions, close/reopen and merge actions are explicit user interactions. Merge uses the displayed head SHA and GitHub's regular merge endpoint without bypassing protections.

The board also accepts a PR URL to review other contributors' PRs. GitHub remains available for features outside this implementation, including resolving review threads, editing/deleting existing comments, managing labels/assignees, auto-merge and branch updates. Markdown uses the app's existing safe basic renderer; it does not implement all GitHub Markdown extensions. GitHub may omit large/binary patches; the viewer links to the original file. Checks are capped at 100 per API response and disclose additional results.

Automated coverage: frontend review/comment submission, draft preservation after failure, merge confirmation and gating, removed-line commenting and hunk numbering; backend input validation, stale-head refusal, merge SHA/strategy and review payloads. Writes are mocked in tests.

## Automatic refresh

[Automatic refresh](auto-refresh.png) shows a new comment delivered by the 30-second timer while the review draft remains intact. Visible screens refresh every 30 seconds and on focus/visibility return; hidden screens and active writes pause background reads. Requests are serialized and late responses after navigation are ignored. Errors preserve the last good data and retry. New head commits require explicit review before review, line-comment or merge actions; loading them resets viewed files and line selection while retaining feedback text.

Seven hook tests cover polling, visibility, overlap, writes, changed heads, recovery and navigation. A component test verifies draft/viewed-file preservation and changed-head action gating. All 14 focused refresh/workspace tests pass. The browser preview confirmed an automatic comment update with an intact draft and no runtime errors.

The full repository gate is run with Node 22, RUST_TEST_THREADS=1 and VITEST_MAX_FORKS=2/VITEST_MIN_FORKS=1; see the PR for its final result.
