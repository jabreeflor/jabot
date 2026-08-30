# UI shell: React port of the HTML prototype

**Issue:** #11
**Status:** Implemented — `src/components/`, `src/views/`, `src/styles/`

## What it is

The React 19 + TypeScript port of `prototypes/jabot-classic.html` — the
messenger-style shell: sidebar, chat/thread view, Inbox, Pull Requests,
Crew, Schedules, and the New Chat / harness-picker flow — driven by the
host API instead of the prototype's static mock data.

## Why

`prototypes/jabot-classic.html` is the settled UX reference (chat,
Inbox, Pull Requests, thread sessions, New Chat with harness picker,
Crew management). Every other feature in this app is exposed through
this shell, so the port has to preserve its interaction model while
switching its data source to the real host API
(see [host-api-protocol.md](host-api-protocol.md)).

## Requirements

1. The shell renders the same top-level surfaces as the prototype:
   Sidebar (folders + threads), Chat/Thread view, Inbox, Pull Requests,
   Crew, Schedules (`src/views/*.tsx`).
2. Thread sessions render as a scrolling chat transcript with a composer
   (`src/components/Conversation.tsx`, `Composer.tsx`,
   `Transcript.tsx`) — see [chat-transcript.md](chat-transcript.md).
3. "New Chat" opens a modal that lets the user pick a harness before
   starting a thread (`src/components/NewChatModal.tsx`,
   `HarnessPicker.tsx`, `HarnessChip.tsx`, `HarnessIcon.tsx`).
4. Folding a thread is available from the thread's context menu and a
   dedicated Fold button (`src/components/FoldButton.tsx`,
   `ThreadContextMenu.tsx`); folded threads leave the Sidebar and any
   Inbox-worthy events resurface them (see
   [fold-and-wait.md](fold-and-wait.md)).
5. Crew is editable in-app via `BotEditorModal.tsx` and rendered as a
   `BotStrip.tsx`; crew CRUD talks to the crew store
   (see [crew-management.md](crew-management.md)).
6. Folder/repo registration is available via `AddFolderModal.tsx` and
   listed in `FolderList.tsx`
   (see [folder-repo-registration.md](folder-repo-registration.md)).
7. GitHub sign-in is available via `GithubSignInModal.tsx` and required
   before the Pull Requests view can show live data
   (see [pull-requests.md](pull-requests.md)).
8. Schedules are editable via `ScheduleEditorModal.tsx`
   (see [schedules.md](schedules.md)).
9. Visual tokens (color, spacing, type) live in `src/styles/tokens.css`
   and every other stylesheet (`chat.css`, `sidebar.css`, `crew.css`,
   `modal.css`, `menu.css`, `cards.css`, `schedules.css`,
   `onboarding.css`, `avatar.css`, `codeav.css`, `shell.css`) builds on
   those tokens rather than hardcoding values, so the prototype's visual
   language survives the port.
10. Every top-level surface has a corresponding test under
    `src/__tests__/` (e.g. `sidebar.test.tsx`, `crew.test.tsx`,
    `inbox.test.tsx`, `pull-requests.test.tsx`, `schedules.test.tsx`,
    `fold.test.tsx`) run against the mock host
    (`src/views/mock-host.ts`) so UI behavior is verifiable without a
    live Rust host or a real harness.
11. First-run onboarding (`src/onboarding/`) walks a new user through
    initial setup (state machine in `state.ts`, screens in
    `Onboarding.tsx`) before landing in the shell.
