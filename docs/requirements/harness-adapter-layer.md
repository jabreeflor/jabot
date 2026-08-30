# Harness adapter layer: ACP client + catalog + Doctor

**Issues:** #10 (adapter layer), #13 (catalog + Doctor)
**Status:** Implemented — `src-tauri/src/host/acp/`, `src-tauri/src/host/harness/`, `src-tauri/src/bin/fake_acp_agent.rs`

## What it is

The layer that turns "a bot" into a real subprocess speaking the Agent
Client Protocol (ACP) over stdio: spawning, connection setup, wake/idle
signaling, and a catalog of known harnesses (Claude Code, Codex, Pi,
Hermes, OpenClaw, custom JSON) with a "Doctor" that checks whether each
is actually installed and usable.

## Why

Per the settled architecture
([`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#6--what-is-a-bot)),
**every crew bot is an ACP harness session** — there is no second,
host-owned "thin LLM + MCP" runtime. This module is that one runtime.

## Requirements

### Adapter (#10)

1. Each live thread gets **one ACP adapter subprocess**
   (`src-tauri/src/host/acp/spawn.rs`) communicating over stdio JSON-RPC.
2. The host owns the subprocess's **process group** so Quit / Kill can
   terminate the harness and any children it spawned in one signal
   (`src-tauri/src/host/procgroup.rs`).
3. Subprocess stderr is captured to host logs (`src-tauri/src/host/log.rs`)
   for diagnosability, not silently dropped or mixed into stdout.
4. `connection.rs` implements the ACP session handshake
   (`session/new`, `session/resume`) and message framing over stdio.
5. `wake.rs` implements liveness/idle signaling so the supervisor
   (see [desktop-host-lifecycle.md](desktop-host-lifecycle.md)) knows
   when a session is actually working vs. idle vs. gone.
6. `src-tauri/src/bin/fake_acp_agent.rs` is a minimal ACP-speaking test
   double used by the test suite so adapter behavior can be exercised
   without a real Claude Code/Codex/Pi binary installed.

### Catalog + Doctor (#13)

7. Harnesses are organized in a three-tier catalog
   (`src-tauri/src/host/harness/catalog.rs`), matching the Buzz-shaped
   design in
   [`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#buzz-style-harness-catalog):
   - **Tier 1 — compiled-in**: shipped cards with reserved ids
     (`claude`, `codex`, `pi`), including auth probes.
   - **Tier 2 — presets**: PATH-probed, not user-editable (Hermes,
     OpenClaw, Cursor, …), resolved via `path.rs`.
   - **Tier 3 — user JSON**: user-supplied custom harness definitions
     (`custom.rs`) under settings / `custom_harnesses/`, validated
     against the Buzz schema (`id`, `label`, `command`, `args`, `env`,
     `installHint`, `installInstructionsUrl`).
8. `doctor.rs` reports, per harness, whether it is installed, on PATH,
   and (where applicable) authenticated — surfaced to the UI so a user
   can tell why a bot won't start.
9. Harness selection is per-bot (a crew member picks a harness id from
   the catalog), not a single global choice — see
   [crew-management.md](crew-management.md).
10. Ambient harness-provided MCP servers are skipped by default (e.g.
    `HERMES_ACP_SKIP_CONFIGURED_MCP=1`); MCP servers passed to a session
    come only from the JaBot tool catalog
    (see [tools-mcp-framework.md](tools-mcp-framework.md)), not whatever
    the harness ships configured with.

## Out of scope

- A parallel Anthropic/OpenAI tool-use loop implemented in the host for
  "worker" bots. Crew bots are never turned into Claude Code subagents.
