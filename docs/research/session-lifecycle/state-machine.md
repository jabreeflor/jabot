# State machine (Disappearing Threads)

Concrete overlay JaBot owns. ACP already has a **process** state
(`running` / `idle` / `requires_action`); we do not duplicate it. The
Inbox is a **UI lifecycle** on top.

Locked from [harness-integration](../harness-integration/adapter-design.md):
ACP `sessionId` is the harness key; `nativeSessionRef` is an overlay;
fold does **not** `session/close`.

## Two layers

```text
UI lifecycle (JaBot)          Process / ACP (harness)
────────────────────          ───────────────────────
active                        connected × running | idle | requires_action
folded                        same — connection stays if work is in flight
resurfaced                    usually idle or requires_action (or dead)
archived / deleted            disconnected; session closed
```

A folded thread can still be `running`. That is the whole feature.
A resurfaced thread can still have a live process (Needs you is blocked
on `request_permission`; Done may keep a warm idle connection until
idle-evict).

## UI states

| State | Sidebar | Inbox | Meaning |
|---|---|---|---|
| `active` | Visible in its folder | Hidden | User is (or could be) looking at the chat. Default after New Chat. |
| `folded` | Hidden | **Still Sleeping** | Disappeared. Supervisor keeps working or waits idle. |
| `resurfaced` | Hidden until reopen | **Resurfaced** | Done, failed, stuck, or needs a human. Badge on Inbox. |
| `archived` | Hidden | Hidden | Closed on purpose. Transcript overlay kept. |
| `deleted` | Gone | Gone | Close + optional `session/delete`. Tombstone optional. |

`Wait for Inbox` is **not** a state. It is `folded` plus
`foldPolicy: wait_for_inbox`. Archive / Delete are actions that land in
`archived` / `deleted`.

Inbox tabs (prototype):

| Tab | Filter |
|---|---|
| All | `folded` ∪ `resurfaced` |
| Needs you | `resurfaced` where `reason ∈ {needs_you, failed, stuck}` |
| Done | `resurfaced` where `reason = done` |

Failed and stuck are absent from the prototype. Put them in **Needs you**,
not Done — they need a human (retry, reopen, archive), not a celebration.

## Legal transitions

```text
                    New Chat / session/new
                              │
                              ▼
                           active
                          ╱     ╲
            Fold /        │      │     Archive
            Wait for Inbox│      │
                          ▼      ▼
                       folded   archived ◄──────── resurfaced
                          │         ▲                    │
            (done/fail/   │         │ Archive            │ Reopen
             stuck/       │         │                    ▼
             needs_you)   │         │                 active
                          ▼         │
                      resurfaced ───┘
                          │
                          │ Reopen thread
                          ▼
                       active

Any non-deleted state ──Delete──► deleted
deleted has no outbound edges.
```

Allowed:

| From | Action | To | What we do on the wire |
|---|---|---|---|
| `active` | Fold ("Disappear until done") | `folded` | Keep subprocess. Do **not** `session/close`. Snapshot `foldedAt`. |
| `active` | Wait for Inbox | `folded` | Same, and set `foldPolicy = wait_for_inbox`. |
| `active` | Archive | `archived` | Reply `cancelled` to pending permissions, then `session/close`. Idle-evict process if this was the last live session on that connection. |
| `active` / `folded` / `resurfaced` / `archived` | Delete | `deleted` | Cancel pending permissions, `session/close`, then `session/delete` if advertised. SIGTERM the adapter if unused. |
| `folded` | Open sleeping row | `active` | Reattach UI to the live connection (or `session/resume` if dead). Clear Inbox row. |
| `folded` | Supervisor trigger | `resurfaced` | Set `resurfacedReason` + `resurfacedAt`. Notify per [resurface.md](resurface.md). **Keep** the connection if `needs_you` (permission still outstanding). |
| `resurfaced` | Reopen thread | `active` | Same as opening a sleeping row. |
| `resurfaced` | Archive | `archived` | As above. |
| `archived` | (later) Restore | `active` | `session/resume` + `cwd`. Not MVP UI. |

Illegal / do not invent:

- `folded` → `archived` without going through resurface **or** an explicit
  Archive on the sleeping row. Allow Archive on a sleeping row (user
  giving up); that is explicit, not automatic.
- `resurfaced` → `folded` (you cannot re-sleep a card that already came
  back; fold again from `active` after reopen).
- `deleted` → anything.
- Auto-answering an outstanding execute while folded. That is a
  `needs_you` resurface, not a transition to `done`.

Folding an already-idle `active` thread: still legal. It lands in Still
Sleeping until the user archives it, **or** we immediately resurface as
`done` if the last stop reason was `end_turn` and there is no pending
permission. Prefer **immediate resurface as done** so Inbox is not a
parking lot for finished work. Uncertainty: the prototype's fold card
always implies "still running"; if the user folds after the agent
already stopped, skip Sleeping.

## Data per state

Thread row (lifecycle overlay; harness fields already in
[adapter-design.md](../harness-integration/adapter-design.md)):

```text
threadId              our uuid
title                 JaBot name (push to harness --name when we can)
folderId              optional
harnessId             claude | codex | pi | custom
acpSessionId          from session/new
nativeSessionRef      Claude uuid | Codex thread id | Pi JSONL path
cwd                   absolute; resume requires this
runtime               { command, args, env } snapshot
uiState               active | folded | resurfaced | archived | deleted
foldPolicy            ask | wait_for_inbox | accept_edits
foldedAt              timestamp | null
resurfacedAt          timestamp | null
resurfacedReason      done | failed | stuck | needs_you | null
lastStopReason        ACP/native stop reason string | null
process               { pid, connectionId, startedAt } | null
acpState              running | idle | requires_action | unknown
pendingPermissions[]  outstanding request_permission ids + subject
awayLog[]             reviewable decisions while folded (see resurface.md)
inboxSummary          one-liner for the card
unread                bool — drives badge
```

`acpState` is a cache of the last `state_update` (v2) or inferred from
v1 `session/prompt` in-flight vs returned. `unknown` after a cold start
before resume.

Minimum to persist across app quit (data-and-persistence will own the
store): everything except `process` and live stdio. `pendingPermissions`
must persist **enough to tell the user we dropped them** — we cannot
answer a request after the process died; on resume the agent will
re-request or skip. If we quit with an outstanding permission, next
launch resurfaces as `needs_you` with copy "the agent was waiting on you;
reopen to continue" rather than replaying a dead RPC.

## Right-click actions (prototype)

| Action | When shown | Effect |
|---|---|---|
| Wait for Inbox | `active` sidebar row | Fold + `foldPolicy = wait_for_inbox`. |
| Archive | any non-deleted row | → `archived`. |
| Delete | any non-deleted row | → `deleted`. |

Fold from the in-chat card ("Disappear until done") keeps the current
`foldPolicy` (default `ask`). That is the difference: Wait for Inbox is
the quieter permission policy; Fold is hide-and-keep-working.

## Process × UI matrix (what the supervisor actually does)

| uiState | acpState | Supervisor |
|---|---|---|
| `active` | `running` | Stream to chat. |
| `active` | `requires_action` | Permission modal in chat. |
| `active` | `idle` | Composer ready. |
| `folded` | `running` | Keep process. Update sleeping-row subtitle (step/elapsed). No chat focus. |
| `folded` | `requires_action` | **Must still deliver.** Notify + immediately resurface `needs_you`. Do not auto-answer execute/delete. Auto-allow reads if `wait_for_inbox`. |
| `folded` | `idle` + `end_turn` | Resurface `done`. Eligible for idle-evict. |
| `folded` | `idle` + error stop | Resurface `failed`. |
| `folded` | `running` but silent too long | Resurface `stuck`. Keep process. |
| `resurfaced` + `needs_you` | `requires_action` | Keep process until answered or cancelled. |
| `archived` / `deleted` | — | No process. |

## What we explicitly defer

- Fork / branch as a first-class JaBot state (Claude `forkSession`, Codex
  `thread/fork`, Pi `/fork`). Reopen-as-new-thread later.
- Crew / Chief-of-Staff threads sharing this machine — same enum, different
  `folderId` / bot id ([bot-crew](../bot-crew/brief.md)).
- Remote host holding `process` while this Mac only shows Inbox
  ([remote-and-mobile](../remote-and-mobile/brief.md)). The overlay still
  works; `process` lives on the host.
