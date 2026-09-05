# Pull request workspace evidence

Screenshots captured from the real `PrWorkspaceView` React component running in Vite with the deterministic `tests/support/pr-workspace-fixture.ts` response. The preview transport rejected writes; no real PRs were modified during visual verification. The temporary preview entry point was removed afterward.

- [Conversation](conversation.png): description, discussion, review form, merge controls and reviewer requests at 1440px.
- [Files](files.png): numbered unified diffs, inline comment buttons and viewed-file controls at 1440px.
- [Compact layout](compact.png): stacked review content and sidebar at 780px.

The production component uses `HostClient.pullRequestDetail` and `pullRequestAction`, which route through the authenticated host to GitHub CLI. Tokens stay with `gh`. Comments, review decisions, line comments, metadata edits, reviewer requests, draft transitions, close/reopen and merge actions are explicit user interactions. Merge uses the displayed head SHA and GitHub's regular merge endpoint without bypassing protections.

The board also accepts a PR URL to review other contributors' PRs. GitHub remains available for features outside this implementation, including resolving review threads, editing/deleting existing comments, managing labels/assignees, auto-merge and branch updates. Markdown uses the app's existing safe basic renderer; it does not implement all GitHub Markdown extensions. GitHub may omit large/binary patches; the viewer links to the original file. Checks are capped at 100 per API response and disclose additional results.

Automated coverage: frontend review/comment submission, draft preservation after failure, merge confirmation and gating, removed-line commenting and hunk numbering; backend input validation, stale-head refusal, merge SHA/strategy and review payloads. Writes are mocked in tests.

## Verification results

- 20 PR frontend tests and 6 PR backend tests passed.
- TypeScript, production Vite build, Rust formatting, Clippy and default-feature compilation passed.
- Browser reported no runtime errors in the fixture preview.
- Broad frontend rerun with `NODE_OPTIONS=--no-experimental-webstorage` passed 476/477 tests; the remaining sidebar/folding timing test passed when its suite was rerun alone (10/10).
- The full verification script did not pass in this environment: its default Node 26 run hit the existing jsdom/localStorage incompatibility, two commit-guard timing cases failed under load, and three OAuth local-server tests failed. These areas were not changed by this feature. Do not treat this branch as having a clean full verification stamp.
