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

---

## D-002 — #10 was already implemented in an open PR; merged instead of rebuilt

**Plan:** #10 (ACP client + adapter subprocess supervision) was an open issue
to implement.

**Found:** PR #36 (`cursor/harness-adapter-acp-e50f`, opened by another agent
before this session) already implements it — 1,843 lines across 19 files:
`host/acp/{connection,mod,runtime,spawn,wake}.rs`, a `fake_acp_agent` test
binary, and `tests/acp_adapter.rs`.

**Done:** Merged that branch into this one rather than writing a competing
implementation. Three conflicts, all additive, resolved as unions:

- `host/mod.rs` — both branches widened the protocol re-export list
- `store/overlay.rs` — their `set_thread_acp_session` alongside existing code
- `lib.rs` — both branches independently made the same non-macOS
  `window`/`api` fix

**Why:** Two implementations of the same issue would conflict on every file
#13, #20, and #21 need. Merging keeps the author's work and its history.

**Consequence:** If PR #36 is revised before it merges, this branch has to take
the update. If it merges to main first, this merge is already in the ancestry
and is a no-op.

### D-002a — fixed a false-negative in the merged tests

`kill_group_reaps_grandchild` failed on merge. The production code is correct:
`ps` shows the group-killed grandchild as `Z`, reparented to a PID 1 that does
not reap it. The test's `process_alive` used `kill -0`, which **succeeds for a
zombie** — so it reported a corpse as alive and failed correct code. Any
environment whose init does not reap promptly (most containers, including the
new Linux CI job) would hit this. Replaced the check with a process-state query
that treats `Z` as gone.
