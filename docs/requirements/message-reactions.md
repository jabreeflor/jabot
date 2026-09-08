# Emoji reactions on bot messages

**Issue:** #265
**Status:** Implemented — `src/components/Transcript.tsx`, `thread/react`, `message_reactions`

## What it is

A discoverable way to leave an emoji on an agent's reply, drawn as a badge
under the bubble, persisted with the thread so it survives a reopen or a
restart.

## Why

Bot replies rendered as Markdown with no mark of "I saw this." Reactions are
the lightest acknowledgement that still belongs on the message, not in a
follow-up turn.

## Requirements

1. An agent bubble offers an **Add reaction** control that opens a short
   emoji palette (`src/components/reactions.ts`). The control is a real
   button with that accessible name.
2. Choosing an emoji attaches it to that message and draws it as a badge
   beneath the reply. The same emoji cannot appear twice on one message.
3. Clicking a badge the user added — or choosing that emoji again in the
   picker — removes it.
4. Marks persist in `message_reactions` and travel with `thread/transcript`.
   Reopening the conversation or restarting the host redraws the same set
   on the same bubbles (`thread/react` is the write; hydrate overlays the
   read).
5. Every reaction control is keyboard reachable (Tab / Enter / Escape) and
   carries a descriptive accessible name (`React with thumbs up`,
   `Remove thumbs up reaction`).
6. User bubbles are unmarked. A reaction is a response to the agent, not
   to the reader's own words.

## Verification

- `src/__tests__/transcript.test.tsx` — picker, badges, toggle, keyboard
- `src/__tests__/thread-stream.test.tsx` — hydrate overlay + live toggle
- `src-tauri` store / transcript host tests — persist, no duplicates
- `tests/e2e/transcript.test.ts` — `thread/react` across a host restart
