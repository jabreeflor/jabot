# Coverage

What the numbers measure, what they do not, and how a miss fails the gate.

This is the #230 policy. Downloadable CI reports live on each `verify` run
as the `coverage-reports` artifact (uploaded even when a gate failed).

## Assumptions (coordination with #229 / #215)

Both #229 (determinism) and #215 (Node upgrade) are on `main`. This
policy is based on current `main` plus the include/exclude list here.

- **Selected Node:** 26.x (`.nvmrc`, `engines.node`, CI). jsdom
  `localStorage` is handled by `--no-experimental-webstorage` from #215.
- The original audit was Node 22.14.0 at `c898b8f` (92.96 / 85.52 /
  81.16). The 90/85/80 floor is the hold-the-line start under the same
  include policy; re-run `npm run test:coverage` after large renderer
  changes before treating the same headroom as given.
- Coverage still measures the existing suite; it is not a retry harness.

If CI moves to another major, re-run `npm run test:coverage` on that
major before trusting the same floors.

## What is measured

| Report | Command | Counts | Does not count |
| --- | --- | --- | --- |
| **Frontend unit** | `npx vitest run --project unit --coverage` (the verify `unit tests` stage) | Production `src/**/*.{ts,tsx}`, including files no test imported | Tests, `*.d.ts`, CSS/assets, `plugins/**` (vendored jabstack), nested `worktrees/**`, `dist/`, `src-tauri/` |
| **Frontend e2e** | `npx vitest run --project e2e` | Behaviour against a live `jabot-hostd`. **Not merged into the unit number.** | — |
| **Rust** | `./scripts/coverage-rust.sh` (CI: `JABOT_RUST_COVERAGE=1`) | `src-tauri` unit tests + crate integration tests, `dev-bins` on | TypeScript e2e, macOS-only `notify/mac.rs` on the Linux runner, frontend |

The frontend floor is **unit-only**. Saying "the product is 90% covered"
because the unit report says so is a lie: `src/host/client.ts` is a thin
RPC wrapper that unit tests mostly stub, and the host-protocol suite is
where those methods actually run.

Rust reporting has **no threshold** yet. The first job is a baseline and a
list of high-risk gaps, not a number to defend.

## Frontend include / exclude

Configured in `vitest.config.ts` (`coverage.all: true` plus an explicit
`include`). Reasons:

| Pattern | In or out | Why |
| --- | --- | --- |
| `src/**/*.{ts,tsx}` | in | Shipped renderer / client. A new file that nothing imports still appears and pulls the totals down. |
| `src/**/*.test.{ts,tsx}`, `src/**/__tests__/**` | out | The measurement, not the product. Scoring tests would make padding look like progress. |
| `src/**/*.d.ts` | out | Ambient types (`vite-env.d.ts`); no runtime. |
| `plugins/**` | out | Vendored jabstack snapshot, not JaBot renderer source. |
| `worktrees/**`, `**/.git/**` | out | Nested git worktrees and metadata are not this tree's production files. |
| `dist/**`, `src-tauri/target/**`, `coverage/**`, `node_modules/**` | out | Build output and dependencies. |

`scripts/tests/coverage.test.sh` rebuilds this policy on a throwaway
fixture: an unimported `src/unimported.ts` fails a 90% floor; covering it
passes; test / plugin / worktree files do not appear in the JSON.

## Frontend thresholds

| Metric | Floor | Audit at c898b8f (Node 22) / hold-the-line |
| --- | --- | --- |
| Lines / statements | 90 | 92.96 (audit: 93.0) |
| Branches | 85 | 85.52 (audit: 85.5) |
| Functions | 80 | 81.16 (audit: 81.2) |

Originally reproduced on Node v22.14.0 at the audit commit: **11122 /
11963 lines**, 2571 / 3006 branches, 573 / 706 functions. Re-measured
on this branch after merging current `main` (OpenCode, conversation
summary, copy-response, Playwright #256; Node 22 locally, CI is **26**):
**92.70 / 85.97 / 81.17**. `main.tsx` (entry), `src/mobile/index.ts`
(barrel), and the type-only `src/host/prWorkspace.ts` sit at 0% on
purpose — they count, they are not excluded to flatter the number.

The proposed 90/85/80 start is a **hold-the-line** floor under the same
include policy, not a target to climb. A few points of headroom absorb
v8/empty-line noise and a small honest refactor; a new untested view or a
deleted test will still miss.

`npm test` does **not** enforce the floor (tight iteration).
`./scripts/verify.sh` and `npm run test:coverage` do. An intentional miss
is also what `npm run test:coverage:policy` (`threshold_violation_fails`)
demonstrates.

Do **not** add snapshot tests or method-by-method mirrors of
`HostClient.listX()` to lift a number. That is worse than a gap: it reads
as coverage and cannot fail when the host contract moves.

## `src/host/client.ts` (unit vs integration)

The audit measured this file at **52.36% lines** in a unit-only run. That
is not the file's product coverage.

**What unit tests already exercise**

- `selectTransport` — Tauri vs HMR-bridge vs bare Vite (`devTransport.test.ts`).
- `HostClient` + `HostRpcError` on a dropped hot-transport
  (`answers every in-flight request when the dev server drops`).
- Almost every feature slice stubs `connectHost` / `HostClient` and never
  enters `request()`.

**Uncovered unit paths, and why they are not padded here**

| Path | What it does | Why a unit test would be the wrong next step |
| --- | --- | --- |
| `createTauriTransport` | `invoke("host_rpc")` + `listen(HOST_RPC_EVENT)` | Needs a mocked Tauri IPC that restates the two SDK calls. The e2e project already drives the real NDJSON host through `HostTransport`. |
| `defaultTransport` | Reads `window.__TAURI_INTERNALS__`, `import.meta.env.JABOT_LIVE_HOST`, `import.meta.hot` | `selectTransport` already encodes the decision table; this is just the env adapter. |
| `HostClient.{hello,prompt,fold,…}` | One-line `this.request(METHOD, params)` wrappers | Implementation-mirroring. The host-protocol / feature e2e suites are the contract. |
| `request` error branch | `throw new HostRpcError(response.error)` | Hit by the hot-transport disconnect test and by e2e `HostRpcError` cases. |
| `onNotificationActivated` | Tauri `listen`, no-op `catch` when there is no event bus | The no-op is for previews/unit; App tests stub the export. |
| `connectHost` | `new HostClient()` + `connect()` (ignore missing bus) + `hello()` | Same: unit tests mock the export; e2e constructs a real client. |

High-value remaining work on this file is **transport/error** behaviour
that e2e cannot see (for example a subscribe failure that is not "no Tauri
bus"), not 40 tests that call `listFolders` and assert `FOLDER_LIST` went
on the wire.

## Rust reporting

CI installs `llvm-tools-preview` and `cargo-llvm-cov` in the workflow
**setup**, then sets `JABOT_RUST_COVERAGE=1` so verify's rust-tests stage
is one instrumented `cargo test` rather than a second compile.
`scripts/coverage-rust.sh` exits 2 if the binary is missing; it will not
`cargo install`. Instrumented bins land under
`src-tauri/target/llvm-cov-target/`; integration tests find
`fake-acp-agent` via `src-tauri/tests/common/mod.rs` rather than
assuming `target/debug/`.

Local verify keeps plain `cargo test`. To produce the report on a laptop:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
./scripts/coverage-rust.sh
```

Baseline and high-risk gaps (filled from the first CI/local llvm-cov run
on this branch) are in the section below. A Rust *threshold* is deferred
until those gaps are understood — several of the hottest paths are
macOS-only, talk to `gh`, or were flaky under load in #229.

### Baseline (Linux, `dev-bins`)

Recorded on this branch with `cargo-llvm-cov` 0.9.1, rustc 1.98.1,
`dev-bins` on. Re-measure after large host changes. Totals include the
0% process mains (`jabot-hostd`, `fake-acp-agent`, `lib.rs` Tauri
commands, `main.rs`).

| | Regions | Functions | Lines |
| --- | ---: | ---: | ---: |
| Covered / total | 35123 / 41009 | 2269 / 2709 | 21479 / 24887 |
| **Percent** | **85.65%** | **83.76%** | **86.31%** |

A 90% line floor would fail today because of those 0% entry points, not
because the host library is untested. That is why there is no Rust
threshold yet.

### High-risk uncovered / under-covered (investigate before a floor)

These are the areas a threshold would either punish unfairly or paper over.
Line % from the same run:

- **`notify/mac.rs`** — `cfg(target_os = "macos")`. Invisible on Linux CI
  (only `notify/unsupported.rs` appears). Same class as `--check-mac`.
- **Process mains (0%)** — `bin/jabot-hostd.rs`, `bin/fake_acp_agent.rs`,
  `lib.rs` (Tauri `host_rpc` / window wiring), `main.rs`. Exercised by e2e
  and the real app, not by `cargo test` of the library.
- **`host/settings.rs` (0%)** — `settings/get` + `settings/set` on
  `HostSession`. The store layer is covered; the session methods are only
  hit over the wire.
- **`host/repo/workspace.rs` (24% lines)** — worktree checkout / path
  resolution. Error and cleanup branches are the risk.
- **`host/harness/install.rs` (40%)** — adapter installer. Talks to the
  network and the user's toolchain.
- **`host/pr/workspace.rs` (41%)** — PR workspace actions (review, merge,
  comment). `gh` I/O; fixtures cover the parser more than the session.
- **`host/router.rs` (63%)** — JSON-RPC method dispatch. Large match;
  many arms are only reached from e2e.
- **`host/repo/gh.rs` (74%)** and **`host/pr/github.rs` (90%)** — shell
  out to `gh`. Unit tests use fixtures; live GitHub is not in this report.
- **OAuth / loopback (`oauth.rs` 91%, `loopback.rs` 92%)** — #229 saw a
  load-sensitive mismatch refusal. High coverage here is not the same as
  determinism. Do not silence that flake to protect a future floor.
- **Chief (`chief/mod.rs` 94%)** — tests that assume no real Claude CLI
  is present. Coverage is only as honest as the fixture (#229).
- **Pairing crypto (`pairing/crypto.rs` 100%)** — well covered already.
  Treat new tests here as security tests, not goldens that restate the
  implementation.

## How to read a downloaded artifact

```
coverage/SCOPE.md          this file
coverage/frontend/
  index.html               browsable unit report
  lcov.info                LCOV
  coverage-summary.json    istanbul totals (machine-readable)
  coverage-final.json      per-file hits
coverage/rust/
  SCOPE.md                 rust-specific scope
  summary.txt              cargo-llvm-cov text table
  lcov.info
  coverage.json
  html/index.html
```

## Commands

```bash
npm test                          # unit, no coverage (iteration)
npm run test:coverage             # unit + floor (what verify runs)
npm run test:coverage:policy      # fixture: a miss fails, excludes hold
npm run test:e2e                  # host protocol; not in the unit number
./scripts/coverage-rust.sh        # rust report; needs cargo-llvm-cov
./scripts/verify.sh               # whole gate, including frontend floor
JABOT_RUST_COVERAGE=1 ./scripts/verify.sh   # CI shape
```
