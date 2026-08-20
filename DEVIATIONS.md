# DEVIATIONS

Running log of every place this build-out departs from the plan in
[`docs/plan.md`](docs/plan.md), the dependency graph in
[#30](https://github.com/jabreeflor/jabot/issues/30), or the settled
decisions in [`docs/decisions/issues-4-6.md`](docs/decisions/issues-4-6.md).

Each entry records **what the plan said**, **what was built instead**, and
**why**. Entries are appended in the order they were taken.

---

## D-001 — Added a test harness that was never scoped as an issue

**Plan:** No issue covers testing. CI (`.github/workflows/ci.yml`) ran
`npm run build` and `cargo test` only; there was no frontend test runner and
no way to exercise the host protocol end to end.

**Built:** A `scripts/verify.sh` single-command pipeline plus two new layers:

- **Vitest + jsdom + Testing Library** for the React renderer.
- **`jabot-hostd`** — an NDJSON-over-stdio binary wrapping the same
  `HostSession` the Tauri command uses, and a `StdioTransport` on the TS side,
  so `tests/e2e/` drives the *real* Rust host through the *real* wire protocol
  from TypeScript.

**Why:** The task requires "a way to test this all the way through once
things are done." The host protocol is the seam every issue crosses, so it is
the only place an end-to-end assertion is meaningful. This is additive — no
planned behaviour changed.

**Note:** `jabot-hostd` is *not* the sidecar extraction that decision #4 defers
("extract `jabot-host` when a second client exists"). It is a test-only
entrypoint over the in-process session; the shipping app still runs the host
inside the Tauri binary as decided. It does, however, prove the socket-shaped
claim in #4 — the same frames work off-process today.
