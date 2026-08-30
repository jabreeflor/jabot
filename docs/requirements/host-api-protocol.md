# Host API: typed JSON-RPC protocol

**Issue:** #8
**Status:** Implemented — `src-tauri/src/host/protocol/`, `src/host/protocol.ts`, `src/host/client.ts`

## What it is

The typed JSON-RPC 2.0 protocol the renderer uses to talk to the Rust
host: `host_rpc` for request/response calls and a `host-rpc` event stream
for pushed updates (run progress, transcript deltas, Inbox events).

## Why

The renderer must never own ACP subprocess I/O (see
[desktop-host-lifecycle.md](desktop-host-lifecycle.md)); every capability
the UI exercises — sending a message, folding a thread, editing crew,
firing a schedule — has to cross this one typed boundary so it can later
be served over a real Unix socket to a second client without changing
call sites.

## Requirements

1. Requests and responses follow JSON-RPC 2.0 (`src-tauri/src/host/protocol/jsonrpc.rs`):
   every request has a method name and params; every response is either a
   result or a structured error (`error.rs`).
2. The wire format is framed for a byte stream (`frame.rs`), so the same
   protocol works over Tauri's IPC channel today and a Unix socket later
   without reframing.
3. `methods.rs` is the single source of truth for the RPC surface — one
   named method per host capability (threads, crew, schedules, Inbox,
   pull requests, pairing, permissions, tools). Adding a capability means
   adding a method here, not inventing an ad hoc channel.
4. The renderer's `src/host/client.ts` exposes a typed call for every
   method in `src/host/protocol.ts`; TypeScript types and Rust types for
   a given method's params/result must stay in lock-step (checked by the
   test suite exercising both sides, e.g. `mock-host.test.ts`).
5. Server-pushed events (run status changes, transcript chunks, Inbox
   entries) are delivered as unsolicited `host-rpc` events, not polled —
   the renderer subscribes once and receives a stream.
6. Errors are structured (code + message + optional data), not raw
   strings, so the renderer can branch on error kind (e.g. permission
   denied vs. harness not found) rather than string-matching.
7. `src/views/mock-host.ts` provides a mock implementation of the same
   protocol surface for UI development and tests without a live Rust
   host.
