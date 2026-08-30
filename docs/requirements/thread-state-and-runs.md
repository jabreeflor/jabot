# Thread overlay state machine + run ledger

**Issues:** #5 (decision), #15 (implementation)
**Status:** Implemented — `src-tauri/src/host/lifecycle/state.rs`, `ledger.rs`, `receipt.rs`, `resurface.rs`

## What it is

The state machine and storage for two related but distinct concepts,
settled in
[`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#5--fold--run--inbox-data-model):

- **Thread overlay state** — sidebar visibility:
  `active → folded → resurfaced → archived`.
- **Runs** — one row per unit of work (a prompt turn, a schedule fire, a
  Chief re-dispatch) on a thread's ACP session:
  `queued → running → succeeded | failed | cancelled | timed_out | lost | needs_you`.

## Why

A thread is a standing conversation; a run is one piece of work on that
conversation. Conflating them (e.g. a fifth "waiting" thread state) makes
"is this thread done" ambiguous when it has many sequential runs. Keeping
them separate is what makes the Inbox a clean projection instead of
another bag of special-cased flags.

## Requirements

1. Thread overlay state transitions only along
   `active → folded → resurfaced → archived`; there is no fifth state for
   "waiting" — waiting is `fold_policy` on the thread
   (see [fold-and-wait.md](fold-and-wait.md)), not a state value.
2. One thread (one ACP `sessionId`) can have many sequential runs; a new
   prompt, a schedule firing, or a Chief re-dispatch each create a new
   run row rather than mutating a shared "current status" field.
3. Run states are exactly:
   `queued, running, succeeded, failed, cancelled, timed_out, lost, needs_you`.
   `lost` is reserved for a run whose subprocess disappeared without a
   terminal signal (crash, force-kill) — it must not be silently
   reported as `succeeded` or dropped.
4. `receipt.rs` records a durable receipt for each completed/terminal
   run so history can be reconstructed without replaying ACP messages.
5. `resurface.rs` implements bringing a folded thread back to
   `resurfaced` when a run transitions to a state the user needs to see
   (`needs_you`, `failed`, or a policy-defined "done").
6. "Still working" is never persisted as a database enum — it is
   reconstructed at boot from the store's terminal/non-terminal run
   records plus live supervisor state
   (`src-tauri/src/host/supervisor/boot.rs`), per requirement 6 of
   [desktop-host-lifecycle.md](desktop-host-lifecycle.md).
7. Every run transition writes to the store before any Inbox
   notification is emitted (see requirement 8 of
   [data-layer-persistence.md](data-layer-persistence.md)).
8. `ledger.rs` is the single writer of run transitions — other modules
   request a transition through it rather than updating run rows
   directly, so invariants (valid state transitions, receipt-on-terminal)
   are enforced in one place.
