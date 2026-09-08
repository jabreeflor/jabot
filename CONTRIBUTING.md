# Contributing to JaBot

## Read this first: CI is not the safety net

This repo is private, so GitHub Actions minutes are metered — and they are
spent. The macOS `bundle` job billed at 10x and was about 86% of the spend, so
it no longer runs on pull requests at all
([`.github/workflows/ci.yml`](.github/workflows/ci.yml) says why). The `verify`
job is nothing but `npm ci` + `./scripts/verify.sh`.

So there is one gate, it runs on your machine, and if you skip it nothing else
catches you before `main`:

```bash
./scripts/verify.sh
```

Two failures already reached CI that this would have caught, and both are the
reason the tooling below exists rather than being a nicety:

- a commit with `error TS6133: 'client' is declared but its value is never
  read` — the code was fine when it was checked; the broken file was written
  *after* the check and before `git add`, and nothing noticed the tree had
  moved. Hence [`scripts/checkpoint.sh`](scripts/checkpoint.sh).
- `clippy -D warnings` failing in CI twice while it passed locally, because
  `rust-toolchain.toml` tracks `stable` and this box had drifted behind the one
  CI installs. Hence the `toolchain` gate, and the 24-hour expiry on the
  "already verified" note described below. See [issue #69](https://github.com/jabreeflor/jabot/issues/69).

## Setup

```bash
# Node 26 (Current) — same major as CI / release. `.nvmrc` and `.node-version`
# are the local pin; `package.json` `engines.node` is `>=26`.
npm install               # deps, and installs the git hooks (see below)
npm run bundle:adapters   # the ACP adapter the app ships inside its bundle
./scripts/verify.sh       # ~1.5 min warm, several minutes on a cold Rust build
```

The app ships the Claude ACP adapter inside its own bundle, staged into
`src-tauri/vendor/adapters/node_modules` (gitignored) from the manifest and
lock committed beside it. `./scripts/live.sh setup` and `npm run build:app` —
the `beforeBuildCommand` a `tauri build` runs — both stage it; to do it on its
own, `npm run bundle:adapters`. Without it the Claude card is only ready on a
machine where someone separately ran `npm i -g`, which is the state this
replaced. See [docs/packaging.md](docs/packaging.md#what-ships-inside-the-bundle-the-claude-acp-adapter).

`npm install` runs `scripts/install-hooks.sh` for you through npm's `prepare`
lifecycle. If you cloned and did something else, or you want to check:

```bash
./scripts/install-hooks.sh          # point git at .githooks/ (idempotent)
./scripts/install-hooks.sh --check  # exits 1 if this clone is unguarded
./scripts/install-hooks.sh --uninstall
```

It sets `core.hooksPath = .githooks`, which is local to your clone and travels
to nobody. `verify.sh` warns when it is not set.

## The everyday loop

| what | when |
| --- | --- |
| `./scripts/verify.sh` | before you commit anything; the whole gate, ~1.5 min |
| `./scripts/verify.sh --fast` | tight iteration — skips building `jabot-hostd` and the e2e suite |
| `./scripts/verify.sh --check-mac` | local repro of the PR `mac notify cross-check` — **run it when you touch `src-tauri/src/notify/`** so you find rot before CI does |
| `./scripts/check-macos-clippy.sh` | on a Mac, local repro of the PR `macos clippy` job — Keychain + `lib.rs` cfg(macos) branches |
| `./scripts/macos-acceptance.sh check` | packaged-app matrix / isolation (Linux). **`run` needs a Mac** — [docs/macos-acceptance.md](docs/macos-acceptance.md), #235 |
| `./scripts/checkpoint.sh -m "message"` | verify **and** commit, atomically (below) |
| `git push` | the `pre-push` hook re-checks unless you just verified these exact bytes, and refuses a push it cannot check |
| `npm test` / `npm run test:a11y` / `npm run test:e2e` | one slice, while you are working on it |
| `npm run lint` / `npm run lint:fix` | frontend lint (hooks + no-explicit-any + promises; same command `verify.sh` runs, including `--fast`) |
| `./scripts/live.sh up` + `shot` | see the change running, on any OS (below) |

Only `verify.sh` is the gate. The others are conveniences around it.

## Running it live, on any machine

`npm run tauri dev` needs macOS. Everything else about the product does not:
the host is `jabot-hostd` on Linux in CI already, and the renderer is a web
page. `scripts/live.sh` puts the two together so a change can be *seen*
working on a Linux box, in a container, or in Claude Code on the web, with no
agent improvising the setup:

```bash
./scripts/live.sh setup   # system libs (apt on Linux), npm deps, bundled adapters, dev bins, a Chromium — idempotent
./scripts/live.sh up      # vite + a real jabot-hostd behind it; returns once the host says hello
./scripts/live.sh shot --out docs/img/<feature>/after.png --click 'text=Inbox'
./scripts/live.sh smoke   # reset, up, seed, one real agent turn, screenshot — the whole loop
./scripts/live.sh down
```

What `up` serves is the product minus the window chrome, not a mock. The
`jabot-host` Vite plugin (`scripts/dev/host-plugin.ts`) spawns `jabot-hostd`
with a real SQLite under `.jabot-dev/data` and bridges its NDJSON over Vite's
own HMR WebSocket; `src/host/devTransport.ts` is the renderer's end of that
bridge, picked by `defaultTransport()` only when the plugin has announced
itself (`import.meta.env.JABOT_LIVE_HOST`). Inside JaBot.app, under `tauri
dev`, in a unit test, in a production bundle: Tauri IPC exactly as before.

The loop is then: edit, HMR reloads the tab, `live.sh shot …` writes the PNG
under `docs/img/`, look at it, repeat, `verify.sh` before the push. `shot`
refuses to take a picture until the sidebar's host line reports a live host,
so a screenshot of "Connecting to host…" cannot be mistaken for evidence.

Driving it without a real harness installed: `fake-acp-agent` (the scriptable
ACP agent the e2e suite uses) is registered as the `fake-acp` harness whenever
it is built, and `live.sh seed` puts Chief on it. `live.sh smoke` is that plus
one turn through the UI, and is the acceptance test for this whole section —
if it passes, the machine can run the loop.

Two more handles, both for scripts: `GET /__jabot/host` is the host's status
(what `up` polls), and `POST /__jabot/rpc` forwards one JSON-RPC request
(`live.sh rpc '{"method":"host/health"}'`), which is how a screenshot script
seeds folders and threads instead of clicking them into existence.

Claude Code on the web runs `setup` from `.claude/hooks/session-start.sh`
before the session starts, so a web session begins with the loop ready.

## What each gate means, and what to do when it fails

`verify.sh` runs these in order, cheapest first, and always runs all of them so
one run tells you everything that is wrong.

| gate | what it proves | when it fails |
| --- | --- | --- |
| `toolchain` | your rustc/clippy/node match what CI would use, and clear the declared MSRV floor | `rustup update stable` for drift; if it says clippy and rustc disagree, that is a half-finished update and clippy is lying to you (D-014). A *warning* here about local stable being old is worth acting on before you trust a green clippy. |
| `lockfiles` | `package-lock.json` satisfies `package.json`, `Cargo.lock` satisfies `src-tauri/Cargo.toml`, and `src-tauri/vendor/adapters`' lock satisfies its own manifest | `npm install` or `cargo update -p <crate>` and commit the lock. CI runs `npm ci`, which refuses to install through this. For the vendored adapters, `npm install --prefix src-tauri/vendor/adapters` and commit both files. |
| `bundle-config` | the packaging config the macOS job reads is still sane without macOS: `bundle.targets` still has `app`, `createUpdaterArtifacts` is still false, every icon exists, every `bundle.resources` path exists, `entitlements.plist` parses, every `src/bin/*.rs` is still gated behind `dev-bins` | read the message — each case names the release that would have shipped broken. D-005 is the cautionary one: a build that succeeds and ships an unupdatable app. |
| `commit guards` | `checkpoint.sh`, `pre-push` and `install-hooks.sh` still refuse what they claim to refuse (`scripts/tests/guards.test.sh`, ~7s, throwaway repos) | you changed the guards; run `npm run test:guards` directly, the failing case names the refusal that stopped working |
| `macos lint tests` | the path planner that turns CI's macOS jobs on still matches what `docs/macos-lint.md` claims (`scripts/tests/macos-lint.test.sh`) | you changed the planner or the notify/native check scripts; run `./scripts/tests/macos-lint.test.sh` |
| `typecheck` | `tsc --noEmit`, strict: implicit any, unused locals, unused parameters, no fallthrough | fix the types. Unused-variable errors (TS6133) are errors here, exactly as in CI. `tsc` does **not** reject an explicit `any` annotation or an `as any` cast — that is the linter, below. |
| `frontend lint` | shared ESLint: Rules of Hooks, exhaustive-deps, `@typescript-eslint/no-explicit-any`, and type-aware `no-floating-promises` / `no-misused-promises` | replace `any` with a concrete type, a generic, or `unknown` plus narrowing. A discarded promise needs `await`, a returned promise, or `void` plus an explicit error strategy. `npm run lint` is the same command; `npm run lint:fix` applies safe fixes (`no-explicit-any` is not auto-fixable). A clean tree does not prove the rules are on — `scripts/tests/lint-probe.mjs` and `scripts/tests/lint-rules.mjs` do. |
| `unit tests` | 200+ vitest cases in jsdom: React components, host client, and axe on the primary views | `npx vitest --project unit` to iterate; `npm run test:a11y` for the axe slice |
| `rust fmt` | `cargo fmt --check` | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| `rust clippy` | `-D warnings` over all targets, `dev-bins` included | fix, or justify a narrow `#[allow]` in the code. Do not suggest APIs newer than the `msrv` in `src-tauri/clippy.toml`. |
| `default-features check` | the crate still compiles *without* `dev-bins`, i.e. what `tauri build` actually compiles | usually a `cfg` or an import that only exists under the dev binaries |
| `rust tests` | host unit tests + 8 integration suites | `cargo test --manifest-path src-tauri/Cargo.toml --features dev-bins <name>` |
| `build jabot-hostd` | the NDJSON host the e2e suite drives still links | not run under `--fast` |
| `e2e (ts to rust host)` | 123 cases over 17 suites: the production TypeScript client against a live `jabot-hostd` over real NDJSON | `npx vitest run --project e2e -t "<name>"`. Needs the binary, so build it first or run the full `verify.sh`. Not run under `--fast`. |
| `renderer build` | `vite build` produces a bundle | usually an import that typechecks but does not resolve |
| `mac notify cross-check` | opt-in locally (`--check-mac`); CI runs `scripts/check-mac-notify.sh` on relevant PRs: `src-tauri/src/notify/` type-checks and lints clean for `x86_64-apple-darwin` | `rustup target add x86_64-apple-darwin` if it says the std is missing. Otherwise it is a real error in `mac.rs`, and the path it names is the repo's file, not a copy. |
| `macos acceptance` | #235: the packaged-app matrix still names Tauri IPC, Dock, Keychain, adapters, and updater archives; isolation still refuses production app data; Playwright WebKit is not this gate | you changed the script, the docs, or the workflows; `./scripts/macos-acceptance.sh check` and `./scripts/tests/macos-acceptance.test.sh` name the cell that moved. Launching `JaBot.app` is `run` on a Mac — D-019 is why that is not this stage |

A **warning** (`!!`) does not fail the run. It is something the script cannot
prove offline — toolchain drift, an unhooked clone — and every one of them has
caused a real failure at least once.

## Frontend lint

The renderer, its tests, and the TypeScript/JavaScript under `scripts/dev`
share one ESLint config (`eslint.config.js`). `./scripts/verify.sh` runs it
on every path, `--fast` included. After `npm install` the check is offline.

```bash
npm run lint        # eslint --max-warnings=0 . , then probes that a
                    # conditional hook, a missing effect dependency, an
                    # explicit `any`, an `as any` cast, a discarded
                    # promise, and a misused async callback still fail
npm run lint:fix    # apply auto-fixes only; does not run the probes
```

`tsc --noEmit` is strict: it rejects *implicit* any. It still permits
`const x: any` and `x as any`. The linter is what enforces the documented
no-any policy (`@typescript-eslint/no-explicit-any`). Do not treat a green
typecheck as "no any".

The same file also enforces React Rules of Hooks, exhaustive-deps, and
type-aware `no-floating-promises` / `no-misused-promises`. A discarded
promise needs `await`, a returned promise, or `void` plus an explicit
error strategy. Do not stand up a second linter. Generated output,
vendored code, `node_modules`, and nested `worktrees/` are ignored.
Unavoidable interop exceptions stay narrow and documented next to the
site; tests are not broadly exempt.

## Accessibility tests

Axe-core runs inside the unit project (`src/__tests__/a11y.test.tsx`) against
the primary views: sidebar, chat thread, Settings, PR board, and New Chat /
harness picker. Critical and serious violations fail the run. Moderate and
minor findings do not, and color-contrast is off because jsdom cannot paint.

```bash
npm run test:a11y
# same slice:
npx vitest run --project unit src/__tests__/a11y.test.tsx
```

`npm test` and `./scripts/verify.sh` already include them — there is no extra
CI job. To add a view, render it the way the existing unit test does and call
`expectNoSeriousA11yViolations(container)` from `tests/support/a11y.ts`.

## Committing: `scripts/checkpoint.sh`

Verification takes about 90 seconds. Anything that writes into the tree during
those 90 seconds — an agent, a watch task, format-on-save, another terminal —
makes "verify passed" a statement about a tree that no longer exists. That is
precisely how a TypeScript error reached CI from a green local run.

```bash
./scripts/checkpoint.sh -m "Add the thing"
```

It hashes the working tree (a real git tree object, so an in-place edit to a
tracked file is visible — `git status` output alone is not), runs the gates,
hashes again, and refuses to commit if anything moved. When it does commit, it
commits the *index* after proving the index's tree is the one that passed, and
checks the resulting commit against that same tree afterwards. Nothing that was
not verified can end up in the commit.

```
--fast              pass --fast to verify.sh (no e2e)
--quiet-for N       refuse to even start until the tree has been still for N seconds
--dry-run           verify and report, commit nothing
--push [--remote R] push if it committed
```

Exit codes, so a script can tell the cases apart: `0` committed, `1` a gate
failed, `2` the tree or HEAD moved during verification, `3` nothing to commit,
`4` usage or environment, `5` the tree was still being written (`--quiet-for`), `6` the commit
does not match the verified tree — read that one carefully, it should be
impossible.

For an unattended loop, wait for stillness first so you do not spend the 90
seconds on a tree someone is halfway through writing:

```bash
while :; do
  ./scripts/checkpoint.sh --quiet-for 120 -m "Checkpoint" && git push
  sleep 60
done
```

## Pushing: the `pre-push` hook

[`.githooks/pre-push`](.githooks/pre-push) runs `./scripts/verify.sh` and
refuses the push if it fails. It is the last thing between a mistake and
`main`, because CI is not going to look.

It also refuses a push it *cannot* check. The gates read the files on disk; a
push carries commits. When those are not the same content, a green run would be
a statement about bytes that are not going anywhere — so instead of verifying
one thing and shipping another, the hook stops before spending the 90 seconds
and prints both tree OIDs. Two ways to land there:

- **uncommitted work on disk.** Commit it (`./scripts/checkpoint.sh -m "..."`
  does both) or stash it, then push.
- **pushing a ref you do not have checked out** — `git push origin main` from a
  feature branch, `git push --all`, `git push origin HEAD~1:main`,
  `git push origin some-branch`. Check that branch out and push from there.

It compares tree OIDs, not commits, so a rebase or an amended message that
produces byte-identical content still counts as verified. And if the worktree
moves *while* the gates are running, the green describes neither tree and the
push is refused there too — the same rule `checkpoint.sh` applies to commits.

It is usually free. `verify.sh` leaves a note in `.git/` naming the tree it
passed; if the worktree is still exactly that tree and every commit being
pushed carries it, the hook says so and exits without re-running anything. So
`checkpoint.sh` followed by `git push` pays for one verification, not two. The
note expires after 24 hours — the gates are not a pure function of your files,
because `stable` moves under them (D-014) — and a `--fast` note never satisfies
a full push, because it never ran e2e.

```bash
JABOT_PREPUSH=fast git push   # skip the e2e suite in the hook (nothing else)
git push --no-verify          # skip the hook entirely
```

`--no-verify` is the emergency exit and it is deliberately blunt: it is for a
hotfix at 2am or a push that cannot possibly affect the gates. If you use it,
say so in the PR, and run `./scripts/verify.sh` when you are back. Nothing
downstream will catch what you skipped.

Deleting a branch pushes no content and is not gated.

## Opening a PR: the artifact

Every PR carries an explainer artifact in its `## Artifact` section (the PR
template has the heading). It is produced by `/create-pr-artifact <n>` from the
vendored [jabstack](https://github.com/jabreeflor/jabstack) plugin at
`plugins/jabstack/`. Cursor and Codex load it from this repo
(`.cursor-plugin/marketplace.json` and `.agents/plugins/marketplace.json`);
Claude Code still uses the GitHub marketplace pin in `.claude/settings.json`.
Run the skill after the PR exists and before you ask for review; it needs
`gh` v2.99.0+ for the `--attach` upload. `CLAUDE.md` has the full rule.

## Before you call something a gap

Every deliberate departure and deferral is recorded as a GitHub issue labelled
[`decision`](https://github.com/jabreeflor/jabot/issues?q=is%3Aissue+label%3Adecision),
one per entry (D-001 through D-025), with the reasoning. These used to live in
`DEVIATIONS.md`; that file was retired in favour of issues. Check them before
filing or "fixing" a gap.

## House rules

- Rust: `cargo fmt` clean, `clippy -D warnings` clean, nothing newer than the
  `msrv` pinned in `src-tauri/clippy.toml`.
- TypeScript: `tsc` is strict (implicit any is an error). Explicit `any` and
  `as any` are forbidden by the shared frontend ESLint config
  (`npm run lint` / `npm run lint:fix`), not by `tsc`. The same file is the
  React Hooks and promise-handling gate. A discarded promise needs `await`,
  a returned promise, or `void` plus an explicit error strategy (the callee
  reports the failure, or the same expression has `.catch`). There is no
  blanket exemption for JSX event handlers — wrap `async` work so `onClick`
  itself returns `void`. The one Hooks exception is `installing` in
  `AdapterSetup`: listing it would clear a failed install's error
  (`installingRef` is sampled instead). Exceptions stay next to the line
  they silence and say why. Subsequent TypeScript lint issues extend
  `eslint.config.js`; do not add a second JS/TS linter.
- Anything added to `verify.sh` must run offline, need no display, no macOS and
  no GitHub token, and be fast enough that people still run it. If a check
  needs any of those, it goes behind a flag — `--check-toolchain` and
  `--check-mac` are the precedents.
- `src-tauri/src/notify/mac.rs` is `cfg(target_os = "macos")`, so the default
  gate compiles straight past it and CI's macOS `bundle` job does not run on
  pull requests. PRs that touch `notify/`, its check, or a shared Cargo /
  toolchain file get the existing `scripts/check-mac-notify.sh` automatically
  (`mac notify cross-check`). PRs that touch `lib.rs`, `secrets.rs`, or those
  same shared files get native `macos clippy` (`scripts/check-macos-clippy.sh
  --lib` only — not a `tauri build`). Coverage, local repro, and deliberate
  exclusions: [docs/macos-lint.md](docs/macos-lint.md). **If you touch
  `src-tauri/src/notify/`, still run `./scripts/verify.sh --check-mac`
  locally** so you see the failure before the PR check does. It needs the
  network and `rustup target add x86_64-apple-darwin`, and no Mac.
  Native-sensitive PRs also run the packaged-app matrix in
  [`.github/workflows/macos-native.yml`](.github/workflows/macos-native.yml)
  (Linux, path-filtered — not a 10x `macos-latest` bundle). Packaged-app
  launch is `#235` / [docs/macos-acceptance.md](docs/macos-acceptance.md);
  label a PR `macos-acceptance` to opt into the expensive Mac job. Do not call
  Playwright WebKit a Tauri acceptance run.
- A test that cannot fail when the thing it covers breaks is worse than no
  test, because it reads as coverage. Break it once and watch it fail before
  you trust it.
