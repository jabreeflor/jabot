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
| `./scripts/windows-needed.sh --base origin/main` | which host/CI paths would start the 2x `windows verify` job — [docs/windows-ci.md](docs/windows-ci.md), #286 |
| `./scripts/windows-verify.sh` | local repro of that job (Git Bash on Windows; on Linux it is a subset of this gate) |
| `./scripts/windows-acceptance.sh check` | Windows install-docs + smoke-checklist integrity (Linux). **Not** a launched `JaBot.exe` — [docs/windows-acceptance.md](docs/windows-acceptance.md), #287 |
| `./scripts/verify.sh --check-browser` | Playwright visual + browser axe + smoke; also a dedicated `browser` CI job |
| `./scripts/checkpoint.sh -m "message"` | verify **and** commit, atomically (below) |
| `git push` | the `pre-push` hook re-checks unless you just verified these exact bytes, and refuses a push it cannot check |
| `npm test` / `npm run test:a11y` / `npm run test:e2e` | one slice, while you are working on it |
| `npm run lint` / `npm run lint:fix` | frontend lint (hooks + no-explicit-any + promises; same command `verify.sh` runs, including `--fast`) |
| `npm run format:check` / `npm run format` | frontend layout — after lint; check is in `verify.sh`; format rewrites |
| `npm run test:browser:smoke` | Playwright Chromium `@smoke` against a real host — not in the default gate |
| `npm run test:browser` | Playwright Chromium + WebKit: journeys, visual, axe, keyboard. See [docs/browser-tests.md](docs/browser-tests.md) |
| `./scripts/live.sh up` + `shot` | see the change running, on any OS (below) |

Only `verify.sh` is the gate. The others are conveniences around it.

## Running it live, on any machine

`npm run tauri dev` opens the native window (macOS is the shipping shell;
Windows prerequisites and the current gaps vs macOS are
[docs/windows.md](docs/windows.md)). Everything else about the product does not
need that window:
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
| `windows ci` | the path planner that turns CI's Windows verify job on still matches what `docs/windows-ci.md` claims, and the workflow still treats failures as real (`scripts/tests/windows-ci.test.sh`) | you changed the planner, `windows-verify.sh`, or `.github/workflows/windows.yml`; run `./scripts/tests/windows-ci.test.sh` |
| `coverage policy` | the frontend include/exclude list and 90/85/80 floors still fail a fixture that violates them (`scripts/tests/coverage.test.sh`) | you changed coverage config or CI upload wiring; run `npm run test:coverage:policy`. Scope and the unit-vs-e2e split: [docs/coverage.md](docs/coverage.md) |
| `typecheck` | `tsc --noEmit`, strict: implicit any, unused locals, unused parameters, no fallthrough | fix the types. Unused-variable errors (TS6133) are errors here, exactly as in CI. `tsc` does **not** reject an explicit `any` annotation or an `as any` cast — that is the linter, below. |
| `frontend lint` | shared ESLint: Rules of Hooks, exhaustive-deps, `@typescript-eslint/no-explicit-any`, and type-aware `no-floating-promises` / `no-misused-promises` | replace `any` with a concrete type, a generic, or `unknown` plus narrowing. A discarded promise needs `await`, a returned promise, or `void` plus an explicit error strategy. `npm run lint` is the same command; `npm run lint:fix` applies safe fixes (`no-explicit-any` is not auto-fixable). A clean tree does not prove the rules are on — `scripts/tests/lint-probe.mjs` and `scripts/tests/lint-rules.mjs` do. |
| `frontend format` | first-party TS/TSX, JS/MJS, CSS, and JSON match Prettier (`npm run format:check`), and the ignore/check/apply contract still holds (`npm run test:format`) | `npm run format` to apply. Do not format vendored plugins, adapters, lockfiles, generated `src-tauri/gen/`, or nested worktrees — `.prettierignore` lists them. |
| `unit tests` | 200+ vitest cases in jsdom, plus scoped `src/**` coverage floors (90/85/80 lines/branches/functions). Unimported production files count. | `npx vitest --project unit` to iterate without coverage; `npm run test:coverage` for the gated run; `npm run test:a11y` for the axe slice |
| `rust fmt` | `cargo fmt --check` | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| `rust clippy` | `-D warnings` over all targets, `dev-bins` included | fix, or justify a narrow `#[allow]` in the code. Do not suggest APIs newer than the `msrv` in `src-tauri/clippy.toml`. |
| `default-features check` | the crate still compiles *without* `dev-bins`, i.e. what `tauri build` actually compiles | usually a `cfg` or an import that only exists under the dev binaries |
| `rust tests` | host unit tests + 8 integration suites. CI sets `JABOT_RUST_COVERAGE=1` so this stage is `cargo llvm-cov` (no extra test run). There is no Rust floor yet. | `cargo test --manifest-path src-tauri/Cargo.toml --features dev-bins <name>` locally; `./scripts/coverage-rust.sh` if you have installed `cargo-llvm-cov` yourself. Reports: [docs/coverage.md](docs/coverage.md) |
| `build jabot-hostd` | `jabot-hostd` and `fake-acp-agent` still link (e2e needs both; llvm-cov's target dir is not `target/debug`) | not run under `--fast` |
| `e2e (ts to rust host)` | 123 cases over 17 suites: the production TypeScript client against a live `jabot-hostd` over real NDJSON | `npx vitest run --project e2e -t "<name>"`. Needs the binary, so build it first or run the full `verify.sh`. Not run under `--fast`. |
| `renderer build` | `vite build` produces a bundle | usually an import that typechecks but does not resolve |
| `browser visual + a11y + smoke` | opt-in, `--check-browser` only: Playwright drives the renderer against a real `jabot-hostd` (smoke journey, axe with contrast, keyboard, visual baselines). CI's `browser` job is the required PR check | `npx playwright install chromium webkit`, then `npm run test:browser`. Needs the host binaries (`npm run host:build`). |
| `mac notify cross-check` | opt-in locally (`--check-mac`); CI runs `scripts/check-mac-notify.sh` on relevant PRs: `src-tauri/src/notify/` type-checks and lints clean for `x86_64-apple-darwin` | `rustup target add x86_64-apple-darwin` if it says the std is missing. Otherwise it is a real error in `mac.rs`, and the path it names is the repo's file, not a copy. |
| `macos acceptance` | #235: the packaged-app matrix still names Tauri IPC, Dock, Keychain, adapters, and updater archives; isolation still refuses production app data; Playwright WebKit is not this gate | you changed the script, the docs, or the workflows; `./scripts/macos-acceptance.sh check` and `./scripts/tests/macos-acceptance.test.sh` name the cell that moved. Launching `JaBot.app` is `run` on a Mac — D-019 is why that is not this stage |
| `windows acceptance` | #287: Windows install docs + five-cell smoke checklist still name launch, bot chat, secret round-trip, adapter spawn, quit-no-orphans, and the glass / Dock / SmartScreen / notify gaps; no macOS-parity claim | you changed `docs/windows.md`, the checklist, or the script; `./scripts/windows-acceptance.sh check` and `./scripts/tests/windows-acceptance.test.sh`. Launching `JaBot.exe` is not this stage — there is no packaged installer yet (#281) |

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

`eslint-config-prettier` is last in that file so lint cannot restate
layout. Prettier is the formatter; do not add `eslint-plugin-prettier`.

## Frontend formatting

Prettier is the one frontend formatter — the Rust equivalent of `cargo fmt`.
It covers first-party TypeScript, JavaScript, CSS, and applicable JSON.
Markdown, HTML prototypes, YAML workflows, lockfiles, vendored
`plugins/` / `src-tauri/vendor/`, build output, generated
`src-tauri/gen/` schemas, dependencies, and nested worktrees are out of
scope (see `.prettierignore`).

```bash
npm run format:check   # read-only; this is the verify.sh gate
npm run format         # rewrite in place
npm run test:format    # check-fails / write-restores / ignore contract
```

`./scripts/verify.sh` and `--fast` both run the check, after lint. It is
offline after `npm install`.

Prettier owns layout. Frontend lint owns correctness (Hooks, `any`,
promises) and must not restate style.

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

## Browser tests (Playwright + real host)

The renderer against a live `jabot-hostd`, not jsdom and not the protocol-only
Vitest `e2e` project. Each test owns a Vite process, a temp data directory,
and a dedicated port — it does not call `live.sh smoke` or `reset`. Details:
[`tests/browser/README.md`](tests/browser/README.md). Web-renderer+host limits
(not native dialogs, Keychain, Tauri IPC, or WKWebView) live in
[`docs/browser-e2e.md`](docs/browser-e2e.md). Browser axe (contrast on),
keyboard/focus/Escape, and `toHaveScreenshot` are in the same suite;
baseline updates are in [docs/browser-tests.md](docs/browser-tests.md).

```bash
npm run host:build
npx playwright install chromium webkit
npm run test:browser:smoke          # Chromium @smoke (journey + axe + keyboard)
npm run test:browser:chromium       # all Chromium journeys without screenshots
npm run test:browser:repeat         # @smoke × 20, no retries
npm run test:browser                # Chromium + WebKit, including visual
./scripts/verify.sh --check-browser # same full suite, after the usual gates
```

The default `./scripts/verify.sh` does **not** run these: the gate stays
offline and display-less. CI's `browser` job is the required PR check and
uploads the Playwright report on failure. WebKit is renderer compatibility,
not proof of native WKWebView/Tauri. The `browser` job uses the same Node
major as `verify` (Node 26 / `.nvmrc`). Playwright is pinned at 1.63+
because 1.56 hangs extracting Chromium on Node 26.

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
  `eslint.config.js`; do not add a second JS/TS linter. Frontend files stay
  Prettier-clean (`npm run format`). Prettier owns layout; lint owns
  correctness and must not restate style.
- Anything added to `verify.sh` must run offline, need no display, no macOS and
  no GitHub token, and be fast enough that people still run it. If a check
  needs any of those, it goes behind a flag — `--check-toolchain`,
  `--check-mac`, and `--check-browser` are the precedents. Coverage tool
  *installs* (`cargo-llvm-cov`, extra Node majors) belong in CI setup,
  not in this script.
- Frontend coverage floors live in `vitest.config.ts` and are part of the
  default unit stage. `npm test` is still the fast no-coverage slice. Do not
  add snapshot or implementation-mirroring tests just to lift a number — see
  [docs/coverage.md](docs/coverage.md).
- `src-tauri/src/notify/mac.rs` is `cfg(target_os = "macos")` and
  `src-tauri/src/notify/win.rs` is `cfg(target_os = "windows")`, so the
  default gate compiles straight past both. The Linux scratch crate lints
  the macOS backend only (`docs/macos-lint.md`); Windows toast delivery is
  a desktop smoke step in `docs/requirements/native-notifications.md`. CI's
  macOS `bundle` job does not run on
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
  Playwright WebKit a Tauri acceptance run. Windows install + the five-cell
  smoke list is `#287` / [docs/windows.md](docs/windows.md); it does not claim
  macOS parity and `windows-acceptance.sh run` is not wired until #281 ships
  an installer.
- Windows host compile is a sibling workflow
  ([`.github/workflows/windows.yml`](.github/workflows/windows.yml), #286):
  a cheap Linux planner on every PR, and `windows-latest` only when
  `src-tauri/`, the toolchain, or the Windows scripts change. It runs
  `scripts/windows-verify.sh` (check + clippy `-D warnings` + portable
  `cargo test`). Failures are real — no `continue-on-error`. An unsigned
  NSIS build is opt-in (`windows-package` label or
  `workflow_dispatch` with `package=true`), not every PR. Tag NSIS is
  `release.yml` (#281). Linux/macOS jobs
  stay the sources of truth they are today. What that job proves, and what
  still needs a human smoke on a real PC:
  [docs/windows-ci.md](docs/windows-ci.md). Playwright on Windows is out of
  scope for the first cut.
- A test that cannot fail when the thing it covers breaks is worse than no
  test, because it reads as coverage. Break it once and watch it fail before
  you trust it.
