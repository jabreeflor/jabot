# Permission broker + prompt UI

**Issue:** #20
**Status:** Implemented — `src-tauri/src/host/permission/`

## What it is

The chokepoint that turns an ACP `session/request_permission` call from
a harness into a human-facing prompt in the UI, and turns the human's
answer back into a protocol response — plus the policy for when a prior
answer can be remembered instead of asked again.

## Why

A harness asking to read/write a file or run a shell command needs a
human in the loop by default; without a single broker, every call site
that might need permission would have to reimplement prompting,
remembering, and denial handling separately, and inconsistently.

## Requirements

1. Every `session/request_permission` from a harness routes through the
   permission broker (`src-tauri/src/host/permission/mod.rs`) — no ACP
   adapter answers a permission request on its own.
2. The broker surfaces a prompt to the renderer over the host API (see
   [host-api-protocol.md](host-api-protocol.md)) and blocks the run
   until the user answers, times out, or the thread is folded/killed.
3. A user's answer (allow / deny / allow-and-remember / deny-and-remember)
   is recorded via the data layer (`store/permission.rs`) when
   "remember" is chosen, scoped to at least (bot, tool, resource) so a
   remembered "allow" for one bot's use of a tool doesn't silently grant
   a different bot the same access.
4. If the thread is folded or the app quits while a permission prompt is
   pending, the run transitions to `needs_you` (see
   [thread-state-and-runs.md](thread-state-and-runs.md)) rather than
   hanging forever or auto-denying silently.
5. Denied permissions are reported back to the harness as a structured
   denial the harness can react to (e.g. try another approach), not as a
   generic error.
6. Remembered decisions are inspectable and revocable by the user (e.g.
   from crew or settings), not a one-way ratchet.
