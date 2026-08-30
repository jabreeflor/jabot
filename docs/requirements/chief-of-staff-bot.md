# Chief of Staff bot

**Issue:** #24
**Status:** Implemented — `src-tauri/src/host/chief/`

## What it is

A standing crew bot (see [crew-management.md](crew-management.md)) that
acts as an orchestrator: an ACP harness session with extra host-provided
tools to hand off work to other bots, spawn new threads, and check on
run status — the "manager" a user talks to instead of individually
juggling every crew bot.

## Why

Per the architecture decision, the Chief is still just an ACP harness
session — not a special host-owned loop — but it needs host tools that a
regular crew bot doesn't (spawning other threads, querying run state)
to actually orchestrate. This module is the seam that grants those
host-privileged tools to one particular bot without breaking the "every
bot is a harness" rule.

## Requirements

1. The Chief of Staff ships as a crew template like any other bot (see
   requirement 2 of [crew-management.md](crew-management.md)) — it is
   not hardcoded outside the crew/template system.
2. `tools.rs` defines the Chief-specific host tools: at minimum,
   handing off a task to another crew bot, spawning a new thread, and
   querying the status of a run/thread.
3. `bridge.rs` connects those host tools into the Chief's ACP session as
   MCP-style tool calls the harness can invoke mid-conversation, using
   the same tool-catalog mechanism as any other tool grant (see
   [tools-mcp-framework.md](tools-mcp-framework.md)), not a bespoke
   side channel.
4. A handoff from the Chief to another bot creates a real thread/run
   under that bot (see [thread-state-and-runs.md](thread-state-and-runs.md))
   — it is not a simulated response inside the Chief's own transcript.
5. Handoffs are recorded (`store/handoff.rs`, see
   [data-layer-persistence.md](data-layer-persistence.md)) so a user can
   trace "the Chief asked Code to do X" back to the resulting thread.
6. The Chief's status-query tool reflects the same run ledger the Inbox
   reads from (see [inbox.md](inbox.md)) — it cannot report a run as
   done before the ledger says so.
7. A user can still talk to any other crew bot directly without going
   through the Chief — the Chief is a convenience orchestrator, not a
   mandatory gateway.
