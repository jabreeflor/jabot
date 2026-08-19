# Resurface, judgment calls, notifications

When a folded thread comes back, and how loudly.

Locked from [adapter-design.md](../harness-integration/adapter-design.md):

- Completion: prefer ACP idle + stop reason; idle-timeout is a backstop only.
- Folded threads **must still deliver** permission prompts (notification +
  Inbox card). ACP does not queue this for us; the host keeps the connection.
- Wait for Inbox: still prompt for execute/delete; auto-allow reads.
  Unanswered execute while folded = resurface as judgment call, do not
  invent an answer.

## Detection {#detection}

Do not use "stdout went quiet" as the primary signal.

### Done

ACP (prefer):

- **v2:** `session/update` `state_update` with `state: "idle"` and
  `stopReason: "end_turn"`.
- **v1 (MVP adapters):** `session/prompt` returns with
  `StopReason` `end_turn`. Same enum:
  `end_turn` | `max_tokens` | `max_turn_requests` | `refusal` | `cancelled`.
  Custom reasons start with `_`.

Native fallbacks if the adapter is lossy:

| Harness | Done signal |
|---|---|
| Claude | `result` with `subtype: "success"` / `is_error: false` |
| Codex | `turn/completed` with `turn.status: "completed"` |
| Pi | **`agent_settled`** (not `agent_end`; `agent_end.willRetry` means more work) |

Only resurface `done` from `uiState == folded`. An `active` thread just
shows "Session finished" in chat.

`cancelled` is not Done. If the user cancelled from a sleeping thread,
land in `archived` or resurface with a quiet "stopped" — prefer **no
banner**, Inbox row with a stopped pill, then they archive. Uncertainty:
prototype has no Stopped pill; a small `sys` line plus Archive is enough.

### Failed

- ACP idle with `max_tokens`, `max_turn_requests`, `refusal`, or an
  error update / failed tool that the adapter marks as terminal.
- Codex `turn/completed` `status: "failed"` (`error.message`,
  `codexErrorInfo`).
- Claude `result` `is_error: true` / `permission_denials` that empty the
  run.
- Adapter process crash while `running`.
- `cwd` gone on resume.

Do not treat a single failed `tool_call` as thread failure. The agent
often recovers. Terminal = idle with a non-success stop reason, or the
process is dead.

### Stuck

Backstop only. Fire when **all** of:

1. `uiState == folded` (or `active` if we want a chat banner — later).
2. Last `acpState == running`.
3. No `session/update` for T seconds (start at **10 minutes**; make it
   a setting). Running tools that still stream chunks reset the timer.
4. Not waiting on `request_permission` (that is Needs you, not stuck).

Action: resurface `stuck`, **keep the process**, copy "no output for 10
min." User can reopen, cancel, or wait. Do not SIGKILL from the
backstop.

Lid-close wake while it was `running`: also `stuck` (we cannot prove the
tool finished). See [keep-alive.md](keep-alive.md#crash-and-sleep).

### Needs you

ACP:

- Outstanding `session/request_permission` (a **request**, not a
  notification — we must reply).
- v2 `state_update` `state: "requires_action"` while blocked. Treat as
  a hint; the RPC itself is authoritative. Agents **SHOULD** send it;
  some v1 adapters will not.

Native:

- Claude `canUseTool` / `AskUserQuestion` / MCP
  `_meta["anthropic/requiresUserInteraction"]`.
- Codex `item/*/requestApproval`, `tool/requestUserInput`.
- Pi: only if `pi-acp` or an extension actually prompts. Until verified,
  assume Pi may **not** block — document the Pi card as less gated.

Wait for Inbox policy (host-side, not the harness):

| Subject | Folded behavior |
|---|---|
| `kind: read` / read-only | Auto-select `allow_once`. Log on `awayLog`. Do **not** resurface. |
| `kind: edit` | Still **ask** under Wait for Inbox (locked policy is reads-only auto-allow). If we later add Accept edits, auto-allow edits and log them as reviewable. |
| `kind: execute` / `delete` / `command` | Do **not** auto-allow. Resurface `needs_you`. Do not invent an answer. |
| `AskUserQuestion` / elicitation | Always Needs you. Never auto-pick an option. |

If the user never opens the card, the process stays blocked. That is
correct. Timeout → still Needs you, not Failed.

## Judgment calls {#judgment-calls}

The prototype mixes two things under one phrase. Split them.

### 1. Blocking ("needs a judgment call")

The human must answer. Inbox tag **NEEDS YOU**. Notification.
`resurfacedReason = needs_you`.

Sources: unanswered `request_permission`, AskUserQuestion, Codex
approval RPC, Wait-for-Inbox execute/delete.

Locked line "Unanswered execute while folded = resurface as judgment
call" means **this bucket**. Do not auto-select `allow_once`.

### 2. Reviewable ("1 judgment call made while you were away")

The agent **already chose** and moved on. Inbox tag **DONE** (or Failed)
with a bullet on the fold card. Not a permission modal.

This is not in ACP. We capture it in an `awayLog[]` the supervisor
appends while `uiState == folded`:

| Event | When to record | Card copy |
|---|---|---|
| `auto_allow` | Host auto-selected a permission (read under Wait for Inbox; edit under Accept edits; Claude `auto` classifier / `acceptEdits` that never hit `canUseTool`) | "Allowed read of `src/auth.ts`" — usually **omit** from the user-facing list unless we want a verbose digest. Reads are noise. |
| `edit` | `tool_call_update` kind `edit`/`delete` completed | Path + tool title. Candidate for the digest. |
| `execute` | kind `execute` completed **after** an allow (user or always-allow) | Command summary. Candidate. |
| `choice` | Agent picked among options **without** AskUserQuestion — we cannot see this in protocol. Approximate via edit to config / "decision-shaped" plan items. | Weak. Prefer structured sources. |

User-facing "N judgment calls" = count of `awayLog` entries we mark
`reviewable: true`. Heuristic for MVP:

- Include: completed `edit` / `delete` while folded; any `auto_allow`
  that was **not** `read`; plan items that flipped to completed.
- Exclude: reads, token usage, thinking, `execute` that was explicitly
  allowed by the user in a previous Needs you.

The prototype line
"One judgment call: kept the 30-day cookie expiry, flagged for review"
is an **edit-level product decision**, not a permission event. We will
not get that sentence for free. Generate the card bullets from tool
titles + paths (`Rewrote session middleware — 3 files changed`). Optional
later: one cheap summarization pass at idle — out of scope until the
store exists.

Claude-specific pitfall: `AskUserQuestion` has historically auto-completed
empty under `acceptEdits` ([issue #29618](https://github.com/anthropics/claude-code/issues/29618)).
That would look like a reviewable judgment call but is a **bug** (silent
default). JaBot policy: AskUserQuestion always Needs you; never rely on
the harness to prompt if we can see the tool name in `request_permission`
/ `canUseTool`. If the adapter swallows it, we cannot fix it — mark
uncertainty and prefer `permissionMode: default` on folded Wait-for-Inbox
threads so interactive tools still reach us.

Claude `auto` mode: the classifier auto-approves many tools. Those
approvals never hit `canUseTool`. If we fold with auto mode on, the
away-log **must** come from `tool_call_update` completions, not from
permission RPCs. Prefer Wait for Inbox (`ask` + auto-read) over Claude
`auto`/`bypassPermissions` for folded threads so execute still surfaces.

## Inbox card

Resurfaced row:

```text
title            thread title + verb (finished / needs a call / failed / stuck)
when             resurfacedAt
subtitle         folder · slept {duration} · files changed · tests · PR
tag              DONE | NEEDS YOU | FAILED | STUCK
detail.path      started → folded → ran {dur} → resurfaced
detail.bullets   awayLog reviewable + plan completions
actions          Reopen thread; Archive; (Done) Open PR if git-and-prs linked
```

Still Sleeping row: dim, tag SLEEPING, subtitle "resurfaces on success,
failure, or question." Updating elapsed time is enough; do not stream
tokens into Inbox.

Badge on the Inbox nav = count of `resurfaced && unread`. Sleeping does
not badge.

## Notifications {#notifications}

Local only. `UNUserNotificationCenter`. Not `NSUserNotification`
(deprecated; crashes on recent macOS). Not APNs for MVP (no server).

### Categories

| Category | Interruption | Sound | When |
|---|---|---|---|
| `needs_you` | `.active` | Yes | Permission / AskUserQuestion / unanswered execute |
| `failed` | `.active` | Yes | Terminal error / crash |
| `stuck` | `.active` | No | Idle-timeout backstop |
| `done` | `.passive` | No | Successful idle `end_turn` |

Never `.timeSensitive` or `.critical`. Apple's HIG reserves those for
events that matter in the next hour (delivery, health, security). A
coding agent finishing a refactor is not that; abusing Time Sensitive
trains users to revoke the entitlement
([HIG: Managing notifications](https://developer.apple.com/design/human-interface-guidelines/managing-notifications)).

Respect Focus. `.active` already does not break through; do not fight it.

### Noise budget

1. **One live notification per `threadId`.** Use a stable
   `UNNotificationRequest` identifier (`jabot.thread.<threadId>`) and
   **replace** it when the reason upgrades (done → needs_you never
   happens; stuck → needs_you replaces). `threadIdentifier = threadId`
   so Notification Center groups them.
2. **Foreground:** no OS banner. Inbox badge + in-app card only
   (`willPresent` → `.badge` / `.list`, not `.banner`).
3. **Sleeping threads never notify.**
4. **Coalesce bursts:** if two tools fail then the process dies, one
   `failed` notification, not three.
5. **Cap:** at most one `.active` banner per thread per 5 minutes.
   Further events update the existing notification body.
6. **Provisional authorization** (`.provisional`) is a quiet trial —
   optional for first launch so we do not hard-prompt on day one.
   Needs you should still request `.alert` + `.sound` after the first
   fold, because a blocked execute is the one notification that earns
   the permission.
7. **Actions on the notification:** Open (foreground the thread),
   Allow / Deny only if we can answer `request_permission` from a
   notification action category **and** the process is still alive.
   MVP: Open only. Answering from Notification Center is a footgun
   (stale request). Do it later.

Badge: Inbox unread count. Clear when the user opens that card.

### Copy

Short. Title = thread title. Body = reason, not a transcript.

- Needs you: "Allow `git push` in jabot-app?"
- Done: "Auth migration finished · 3 files changed"
- Failed: "Auth migration failed · adapter exited"
- Stuck: "Auth migration has had no output for 10 min"

## Mapping table (stop reason → Inbox)

| Signal | `resurfacedReason` | Tab | Notify |
|---|---|---|---|
| idle + `end_turn` | `done` | Done | passive |
| idle + `max_tokens` / `max_turn_requests` / `refusal` | `failed` | Needs you | active |
| idle + `cancelled` | (stopped; no celebrate) | All, not Done | none |
| `requires_action` / pending permission / AskUserQuestion | `needs_you` | Needs you | active |
| unanswered execute under Wait for Inbox | `needs_you` | Needs you | active |
| process crash while running | `failed` | Needs you | active |
| silence while running | `stuck` | Needs you | active, no sound |
| Claude `result` error | `failed` | Needs you | active |
| Codex `turn/completed` failed | `failed` | Needs you | active |
| Pi `agent_settled` | `done` | Done | passive |
| Pi `agent_end` only | ignore | — | — |

## Uncertainties

- Shipped ACP adapters may still be v1-only (completion on
  `session/prompt` return, no `state_update`). Supervisor should accept
  **either** signal and not wait for both.
- `pi-acp` translation of `agent_settled` → idle is unverified.
- Claude `auto` mode hides permission RPCs; folded Wait-for-Inbox should
  not enable `auto`/`bypassPermissions`.
- Whether macOS will deliver `.passive` `done` notifications when the
  app is backgrounded but not focused — treat badge as the reliable
  channel; banners are bonus.
