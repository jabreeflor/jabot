# Crew store, bot templates, and "every bot is a harness"

**Issues:** #6 (decision), #17 (implementation)
**Status:** Implemented — `src-tauri/src/host/crew/`, `src/components/BotEditorModal.tsx`, `src/components/BotStrip.tsx`, `src/views/crew.ts`

## What it is

CRUD for "crew": the set of bots (Chief of Staff, Inbox Manager, Writer,
Code, and user-added bots) a user can start a thread with, each defined
as data — persona, tools, memory scope, credentials, and a
`harness_id` — rather than code.

## Why

Per the settled architecture
([`docs/decisions/issues-4-6.md`](../decisions/issues-4-6.md#6--what-is-a-bot)),
a crew "bot" is a **scope** (who it is, what it can touch) layered over a
harness (the engine that actually runs it, from the catalog in
[harness-adapter-layer.md](harness-adapter-layer.md)). Crew data is what
makes bots user-editable without shipping code changes.

## Requirements

1. Every crew member record includes at minimum: display identity
   (name/avatar), persona/system prompt, a `harness_id` referencing the
   harness catalog, an allowlist of tools/MCP servers, and a memory
   scope (`src-tauri/src/host/crew/memory.rs`).
2. Built-in bots (Chief of Staff, Inbox Manager, Writer, Code) ship as
   **templates** (`src-tauri/src/host/crew/templates.rs`,
   `templates/`) — data, not special-cased Rust branches — so a user can
   fork/edit them the same way as a bot they created from scratch.
3. Users can create, edit, and delete crew members via
   `BotEditorModal.tsx`; changes persist immediately through the crew
   store and are reflected in `BotStrip.tsx` and the New Chat harness
   picker without requiring a restart.
4. `standing.rs` distinguishes "standing" crew (always available, e.g.
   Chief) from ad hoc/one-off bot configurations if the UI creates
   throwaway ones (e.g. for a single custom-harness experiment).
5. A crew bot's engine is always an ACP harness session
   (see [harness-adapter-layer.md](harness-adapter-layer.md)) — there is
   no crew bot implemented as a host-owned "thin LLM + tool loop," and
   crew bots are never modeled as Claude Code subagents.
6. Deleting a crew member does not delete threads already started with
   it; existing threads keep working against their already-resolved
   harness configuration (or are clearly marked orphaned if the
   dependency truly can't resolve).
7. Crew editing is covered by `src/__tests__/crew.test.tsx`,
   `crew-store.test.tsx`, `crew-style.test.tsx`, and
   `bot-editor.test.tsx` against the mock host.
