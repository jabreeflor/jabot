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
  "already verified" note described below. See `DEVIATIONS.md` D-014.

## Setup

```bash
npm install          # deps, and installs the git hooks (see below)
./scripts/verify.sh  # ~1.5 min warm, several minutes on a cold Rust build
```

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
| `./scripts/checkpoint.sh -m "message"` | verify **and** commit, atomically (below) |
| `git push` | the `pre-push` hook re-checks unless you just verified these exact bytes, and refuses a push it cannot check |
| `npm test` / `npm run test:e2e` | one slice, while you are working on it |

Only `verify.sh` is the gate. The others are conveniences around it.

## What each gate means, and what to do when it fails

`verify.sh` runs these in order, cheapest first, and always runs all of them so
one run tells you everything that is wrong.

| gate | what it proves | when it fails |
| --- | --- | --- |
| `toolchain` | your rustc/clippy/node match what CI would use, and clear the declared MSRV floor | `rustup update stable` for drift; if it says clippy and rustc disagree, that is a half-finished update and clippy is lying to you (D-014). A *warning* here about local stable being old is worth acting on before you trust a green clippy. |
| `lockfiles` | `package-lock.json` satisfies `package.json`, and `Cargo.lock` satisfies `src-tauri/Cargo.toml` | `npm install` or `cargo update -p <crate>` and commit the lock. CI runs `npm ci`, which refuses to install through this. |
| `bundle-config` | the packaging config the macOS job reads is still sane without macOS: `bundle.targets` still has `app`, `createUpdaterArtifacts` is still false, every icon exists, `entitlements.plist` parses, every `src/bin/*.rs` is still gated behind `dev-bins` | read the message — each case names the release that would have shipped broken. D-005 is the cautionary one: a build that succeeds and ships an unupdatable app. |
| `commit guards` | `checkpoint.sh`, `pre-push` and `install-hooks.sh` still refuse what they claim to refuse (`scripts/tests/guards.test.sh`, ~7s, throwaway repos) | you changed the guards; run `npm run test:guards` directly, the failing case names the refusal that stopped working |
| `typecheck` | `tsc --noEmit`, strict, no `any` | fix the types. Unused-variable errors (TS6133) are errors here, exactly as in CI. |
| `unit tests` | 200+ vitest cases in jsdom: React components and the host client | `npx vitest --project unit` to iterate |
| `rust fmt` | `cargo fmt --check` | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| `rust clippy` | `-D warnings` over all targets, `dev-bins` included | fix, or justify a narrow `#[allow]` in the code. Do not suggest APIs newer than the `msrv` in `src-tauri/clippy.toml`. |
| `default-features check` | the crate still compiles *without* `dev-bins`, i.e. what `tauri build` actually compiles | usually a `cfg` or an import that only exists under the dev binaries |
| `rust tests` | host unit tests + 8 integration suites | `cargo test --manifest-path src-tauri/Cargo.toml --features dev-bins <name>` |
| `build jabot-hostd` | the NDJSON host the e2e suite drives still links | not run under `--fast` |
| `e2e (ts to rust host)` | 123 cases over 17 suites: the production TypeScript client against a live `jabot-hostd` over real NDJSON | `npx vitest run --project e2e -t "<name>"`. Needs the binary, so build it first or run the full `verify.sh`. Not run under `--fast`. |
| `renderer build` | `vite build` produces a bundle | usually an import that typechecks but does not resolve |

A **warning** (`!!`) does not fail the run. It is something the script cannot
prove offline — toolchain drift, an unhooked clone — and every one of them has
caused a real failure at least once.

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

## Before you call something a gap

`DEVIATIONS.md` records every deliberate departure and deferral, D-001 through
D-024, with the reasoning. Check it before filing or "fixing" one.

## House rules

- Rust: `cargo fmt` clean, `clippy -D warnings` clean, nothing newer than the
  `msrv` pinned in `src-tauri/clippy.toml`.
- TypeScript: strict, no `any`.
- Anything added to `verify.sh` must run offline, need no display, no macOS and
  no GitHub token, and be fast enough that people still run it. If a check
  needs any of those, it goes behind a flag — `--check-toolchain` is the
  precedent.
- A test that cannot fail when the thing it covers breaks is worse than no
  test, because it reads as coverage. Break it once and watch it fail before
  you trust it.
