# Chat transcript renderer + persisted overlay

**Issue:** #14
**Status:** Implemented — `src-tauri/src/host/transcript/`, `src/components/Transcript.tsx`, `src/views/transcript.ts`

## What it is

The renderer and storage for a thread's chat history: turning a stream of
ACP protocol messages (user turns, assistant text, tool calls, tool
results, permission prompts) into a readable transcript that is also
durably persisted so it survives Quit/resume.

## Why

An ACP session's live message stream is not itself a durable transcript —
without a persisted overlay, folding a thread or quitting the app would
lose the conversation. The Chat view's core promise (pick up where you
left off) depends on this.

## Requirements

1. Every message the harness emits over ACP for a thread is appended to
   a durable transcript queue before being forwarded to the UI
   (`src-tauri/src/host/transcript/queue.rs`) — the store, not the live
   subprocess, is the source of truth for "what did this thread say."
2. Transcript entries preserve message kind (user, assistant, tool call,
   tool result, system/permission event) and order; the renderer must be
   able to reconstruct the same visual transcript from stored entries
   as from the live stream.
3. Re-opening a folded or previously-closed thread renders its full
   persisted transcript, not just messages received after reopening.
4. `src/components/Transcript.tsx` renders each entry kind distinctly
   (plain text vs. a tool call/result) rather than flattening everything
   to plain text.
5. Streaming updates (partial assistant output) update the transcript
   incrementally in the UI without duplicating or reordering completed
   entries once the run finishes.
6. Transcript growth is bounded/paginated appropriately so a long-running
   thread's history doesn't degrade renderer performance (verified by
   `src/__tests__/transcript.test.tsx` and
   `src/__tests__/thread-stream.test.tsx`).
