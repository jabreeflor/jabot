# CLAUDE.md

Guidance for Claude (and any Claude Code agent) working in this repository.
See `CONTRIBUTING.md` for the human-facing dev workflow — `./scripts/verify.sh`
is the one required gate before anything lands on `main`.

## Plugin: jabstack

`.claude/settings.json` enables the [jabstack](https://github.com/jabreeflor/jabstack)
plugin (marketplace `jabstack`, plugin `jabstack@jabstack`). Claude Code offers to
install it on session start; accept. It provides `/create-pr-artifact`,
`/gauntlet-loop` and the `gauntlet-critic` subagent. If the skill is not
loaded, the plugin is not installed — run `/plugin install jabstack@jabstack`
rather than working around it.

## Rule: every PR gets a PR artifact

**Every pull request opened against this repo must run `/create-pr-artifact`
before it is handed to a reviewer.** No exceptions for small, docs-only, or
config-only changes — the artifact is how a reviewer gets the mechanism in two
minutes, and it is cheap for a small PR.

The order is:

1. Push the branch and open the PR (draft is fine).
2. Run `/create-pr-artifact <PR number>`. It builds the explainer artifact,
   screenshots it, and rewrites the `## Artifact` section of the PR body with
   the link and screenshots. The PR template already carries that heading;
   leave it in place so the skill has a section to fill.
3. Only then mark the PR ready for review or ask for review.

The skill attaches screenshots with `gh pr edit --attach` (gh v2.99.0+). In an
environment without `gh` (Claude Code on the web), do the same work by hand:
publish the artifact, save the screenshots under `docs/img/<pr-or-feature>/`,
commit them, and edit the PR body's `## Artifact` section through the GitHub
tools so it carries the artifact link and the embedded images.

A PR whose `## Artifact` section is still the template comment is not done.

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

1. Launch the real app with `./scripts/live.sh up` and capture with
   `./scripts/live.sh shot --out docs/img/<feature>/<name>.png [steps…]` —
   this works on Linux and in Claude Code on the web, against the real Rust
   host, and refuses to shoot until the host is live. Drive the UI into the
   state under test with the step flags (`--click`, `--fill`, `--wait-text`,
   …; see `scripts/dev/shot.mjs`), or seed it with `--rpc`. Never fake or
   hand-draw a screenshot, and never describe a screenshot instead of
   capturing one. `./scripts/live.sh smoke` proves the loop works on the
   machine you are on; run it first if `up` misbehaves.
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
