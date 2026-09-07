# Coverage

What the numbers measure, what they do not, and how a miss fails the gate.

This is the #230 policy. Downloadable CI reports live on each `verify` run
as the `coverage-reports` artifact (uploaded even when a gate failed).

## Assumptions (coordination with #229 / #215)

Neither #229 (determinism) nor #215 (Node upgrade) is merged. This policy
is based on `main` at the audit commit (`c898b8f`) and the **Node 22** pin
already in `.github/workflows/ci.yml` and `release.yml`.

- **Selected Node:** 22.x. The audit's Node 26 run died in
  `tests/support/setup-dom.ts` (`localStorage` undefined) — a runtime
  compatibility failure, not 553 product defects. Until #215 lands a
  working newer major, the floor is reproduced on Node 22, same as CI.
- **#229 flakes** (upload/fold timing, Chief runtime detection, OAuth
  loopback, mobile-inbox log prefix) are not solved here. Coverage is a
  measurement of what the existing suite executes, not a retry harness.

If #215 later moves CI to another major, re-run `npm run test:coverage` on
that major before trusting the same floors.

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

| Metric | Floor | Audit at c898b8f (Node 22, unit, `all` files) |
| --- | --- | --- |
| Lines / statements | 90 | 92.96 (audit: 93.0) |
| Branches | 85 | 85.52 (audit: 85.5) |
| Functions | 80 | 81.16 (audit: 81.2) |

Reproduced on this branch with Node v22.14.0, Vitest 3.2.7, `coverage.all`
and the include list above: **11122 / 11963 lines**, 2571 / 3006
branches, 573 / 706 functions. `main.tsx` (entry), `src/mobile/index.ts`
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
`cargo install`.

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

### Baseline (Linux, Node 22 host, `dev-bins`)

Recorded when this policy landed. Re-measure after large host changes.

<!-- rust-baseline: filled after the first cargo-llvm-cov run -->

| | Lines | Regions / branches | Functions |
| --- | --- | --- | --- |
| `src-tauri` (this runner) | *(see artifact `coverage/rust/summary.txt`)* | | |

### High-risk uncovered / under-covered (investigate before a floor)

These are the areas a threshold would either punish unfairly or paper over:

- **`notify/mac.rs`** — `cfg(target_os = "macos")`. Invisible on Linux CI.
  Same class as the `--check-mac` gate. Do not set a global floor that
  pretends this file is scored.
- **OAuth / loopback (`host/tools/oauth.rs`, `loopback.rs`)** — #229 saw a
  load-sensitive mismatch refusal. Instrument, do not silence.
- **Chief runtime resolution (`host/chief/mod.rs`)** — tests that assume
  no real Claude CLI is present. Coverage here is only as honest as the
  fixture.
- **`host/repo/gh.rs`, `host/pr/github.rs`** — shell out to `gh`; unit
  tests use fixtures, live GitHub is not in this report.
- **`host/git/worktree.rs`** — filesystem + git. Integration tests cover
  the happy path; error/cleanup branches are the risk.
- **Pairing crypto (`host/pairing/crypto.rs`)** — easy to "cover" with
  goldens that restate the implementation. Gaps here are security-relevant.
- **`src/bin/jabot-hostd.rs` / `fake_acp_agent.rs`** — process mains.
  Exercised by e2e, not by `cargo test` of the library.

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
