# Session Lifecycle — Findings

Researched August 2026 against current public docs, adapters, and prior art.
This file answers the 6 questions in [`brief.md`](brief.md). Deep dives
live in sibling files.

**Recommendation in one sentence:** JaBot should own a **user-space session
supervisor** that keeps the ACP subprocess alive while a folded thread is
still working, checkpoints `sessionId` + `cwd` so idle / sleep / crash can
resume, and maps ACP idle + stop reason / `requires_action` onto a four-state
Inbox overlay (`active` → `folded` → `resurfaced` → `archived`) — not
launchd, not "kill the process on fold."

| Question | Short answer | Detail |
|---|---|---|
| 1. Keeping sessions alive | Folded + working ⇒ keep the ACP subprocess. Folded + idle / sleep ⇒ process may die; resume from `sessionId` + `cwd`. Claude `--resume`, Codex `thread/resume` / `codex resume`, Pi `switch_session`. | [keep-alive.md](keep-alive.md) |
| 2. Resurface triggers | Prefer ACP idle + stop reason (v1: `session/prompt` return; v2: idle `state_update`). `requires_action` / outstanding permission = Needs you. Idle-timeout is a backstop for stuck. | [resurface.md](resurface.md) |
| 3. State machine | UI overlay: `active` → `folded` → `resurfaced` → `archived` / `deleted`. Wait for Inbox is a **permission policy**, not a fifth state. Process layer is orthogonal (`connected`/`dead` × ACP `running`/`idle`/`requires_action`). | [state-machine.md](state-machine.md) |
| 4. Crash / laptop-sleep | Persist overlay always. On wake: reuse live PID if stdio still works, else `session/resume`. No launchd KeepAlive for MVP. Optional detached host later (app-shell). | [keep-alive.md](keep-alive.md#crash-and-sleep) |
| 5. Notifications | Local `UNUserNotificationCenter` only. Needs you = `.active` banner; done = `.passive` / replace-in-place; never time-sensitive. One live notification per thread. | [resurface.md](resurface.md#notifications) |
| 6. Judgment calls while away | Two buckets: **blocking** (unanswered `request_permission` / AskUserQuestion → Needs you; do not invent an answer) vs **reviewable** (auto-allowed edits / classifier choices logged on the fold card). | [resurface.md](resurface.md#judgment-calls) |

## What this unblocks

The brief's "What this blocks" list can become issues:

1. **Thread state machine + store** — overlay enum, legal transitions, and
   per-state fields in [state-machine.md](state-machine.md). Persistence
   shape feeds [data-and-persistence](../data-and-persistence/brief.md).
2. **Background session supervisor** — spawn / PID / logs / idle-evict /
   resume from `sessionId` + `cwd`. Same host process as the ACP client for
   MVP; do not install a launchd agent. See [keep-alive.md](keep-alive.md).
3. **Inbox view (real data)** — Resurfaced + Still Sleeping sections; tabs
   All / Needs you / Done driven by `uiState` + `resurfacedReason`, not
   mock rows. See [resurface.md](resurface.md).
4. **Fold / Wait for Inbox wired to real sessions** — Fold keeps the
   connection; Wait for Inbox is fold + the locked permission policy
   (auto-allow reads; still prompt execute/delete).
5. **Native notifications** — `UNUserNotificationCenter` categories, noise
   budget, replace-in-place per `threadId`. See
   [resurface.md](resurface.md#notifications).

## Prototype note

`prototypes/jabot-classic.html` Inbox already has the UX we should ship:
Resurfaced vs Still Sleeping, tabs All / Needs you / Done, right-click
Wait for Inbox / Archive / Delete, fold card, and
"1 judgment call made while you were away" on a **done** card. The
prototype does not distinguish failed vs stuck; we should, as separate
`resurfacedReason` values that still land in the Needs you tab.

## Sources

Primary docs, not secondary blogs, unless noted:

- ACP v1 sessions:
  [session-setup](https://agentclientprotocol.com/protocol/v1/schema)
  (methods `session/new`, `session/load`, `session/resume`, `session/close`,
  `session/cancel`); v2:
  [session-setup](https://agentclientprotocol.com/protocol/v2/session-setup),
  [prompt-lifecycle](https://agentclientprotocol.com/protocol/v2/prompt-lifecycle)
  (idle `state_update`, `requires_action`, stop reasons)
- Claude: [Agent SDK sessions](https://code.claude.com/docs/en/agent-sdk/sessions),
  [CLI sessions](https://code.claude.com/docs/en/sessions)
  (`--resume` / `--continue`),
  [permissions](https://code.claude.com/docs/en/agent-sdk/permissions),
  [user-input / AskUserQuestion](https://code.claude.com/docs/en/agent-sdk/user-input)
- Codex: [app-server](https://developers.openai.com/codex/app-server)
  (`thread/resume`, `turn/completed`),
  [CLI reference](https://developers.openai.com/codex/cli/reference)
  (`codex resume`, `codex exec resume`)
- Pi: [RPC](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md)
  (`agent_settled` vs `agent_end`),
  [sessions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sessions.md)
- Buzz: [`crates/buzz-acp`](https://github.com/block/buzz/blob/main/crates/buzz-acp/README.md),
  [durable session load PR #2633](https://github.com/block/buzz/pull/2633),
  [idle-session leak #2961](https://github.com/block/buzz/issues/2961)
- Conductor (local vs cloud keep-alive):
  [cloud FAQ](https://www.conductor.build/docs/cloud/faq),
  [cloud workspaces](https://www.conductor.build/docs/cloud)
- macOS: [NSProcessInfo activities](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/PrioritizeWorkAtTheAppLevel.html),
  [HIG managing notifications](https://developer.apple.com/design/human-interface-guidelines/managing-notifications),
  [UNNotificationInterruptionLevel](https://developer.apple.com/documentation/usernotifications/unnotificationcontent/interruptionlevel)
