# CLAUDE.md

Guidance for Claude (and any Claude Code agent) working in this repository.
See `CONTRIBUTING.md` for the human-facing dev workflow — `./scripts/verify.sh`
is the one required gate before anything lands on `main`.

## Rule: screenshot evidence is mandatory for visual/UI work

**ALWAYS ALWAYS ALWAYS attach screenshot evidence, committed inside this
repo, for any change that touches UI, rendering, layout, styling, or visual
output.** This is not optional and not satisfied by a text description of
what the change looks like.

Applies to (non-exhaustive):

- Any change under `src/` that affects a component, view, page, or anything
  that renders to screen.
- Tauri window/UI changes in `src-tauri/`.
- CSS/styling, theming, layout, or asset changes.
- New or updated icons/images (see `docs/img/emoji-to-svg`).
- Any bug fix or feature where "does it look right" is part of "is it done."

### What "screenshot evidence" means

1. Use the `run` skill (or the project's own app-launch skill if one exists)
   to actually launch/build the app and get it into the state under test —
   never fake or hand-draw a screenshot, and never describe a screenshot
   instead of capturing one.
2. Capture a real screenshot of the before/after (or just after, when there's
   no meaningful before) state.
3. Save it into the repo under `docs/img/` (create a task-specific
   subdirectory if it helps organize, e.g. `docs/img/<feature-or-pr>/`).
   Do not leave evidence only in `/tmp`, the scratchpad, or an ephemeral
   artifact — it must be committed to the repo so it survives in PR history.
4. Reference the screenshot(s) in the PR description (embed with normal
   markdown image syntax, e.g. `![before](docs/img/foo/before.png)`) so
   reviewers see it inline without opening the repo.
5. If a change is purely non-visual (backend logic, CI config, docs-only,
   pure refactor with no rendering surface), screenshot evidence is not
   required — but say explicitly in the PR body *why* it was skipped
   ("no visual surface — backend-only change") rather than silently omitting
   it. When in doubt, capture the screenshot anyway.

### Why

Two production regressions already shipped because a change "looked right"
in the diff but wasn't actually run. A screenshot committed to the repo is
the only artifact that proves the app was launched and the change was
observed working, not just read.
