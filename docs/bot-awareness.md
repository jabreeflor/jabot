# Jabot app awareness and conversational bot drafts

**Issues:** #237
**Status:** Implemented — `src-tauri/src/host/acp/prompt.rs`, `src-tauri/src/host/crew/draft.rs`

## What it is

Every dispatched ACP prompt carries a host-owned context block so a harness
knows what Jabot is, which crew member it is, and what this thread can do.
Chief and Bot Recruiter can propose a new crew member with `draft_bot`. The
proposal is a durable draft. Only a full-device Save creates the bot.

## How harnesses receive context

Composition happens immediately before every `session/prompt`, including
queued drains. The host prepends **one ACP text block** to the user's original
blocks (types and order preserved). That is framing, not a `systemPrompt`
parameter and not system-message authority.

The same `session/prompt` array reaches every supported harness:

| Harness | How it sees the context |
| --- | --- |
| Claude Code | ACP `session/prompt` text block, first in the array |
| Codex | same |
| Gemini CLI | same |
| Pi | same |
| Profile-scoped / custom adapters | same — composition is per thread, not per process |
| Fake ACP agent (tests) | logs `session_prompt=` with the composed params |

The transcript records the user's exact content only. Injected context is not
replayed as something the user typed. `MEMORY.md` stays on disk; it is not
embedded. Bearer tokens and provider credentials never enter the prompt.

Botless coding threads get app and thread facts without an invented persona
or a management grant. A bot without `draft_bot` is told so and should route
the user to Crew or a bot that has the tool.

A changed persona applies on the **next** dispatch. An in-flight turn keeps
the prompt it already sent. Removed tools are refused on the next host call
even if the model still remembers them.

## Drafts vs saved bots

`draft_bot` validates, persists, notifies, and returns
`{ status: pending_review, saved: false }`. It does not create a bot, start a
run, connect a provider, or add a schedule.

Save is a full-device UI RPC (`crew/drafts/save`). It is not an agent tool.
Approver phones cannot call it. Child proposals cannot include `draft_bot` or
`get_bot_draft`.

Pending drafts survive Close, navigation, and restart. Close is not Dismiss.
Stale drafts (source bot gone or grant revoked) remain reviewable; Save is a
fresh user action. Drafts do not appear as Inbox permission cards.

Default grant: untouched shipped Chief and Recruiter only. Customized seats
and a deleted Recruiter are left alone. New MCP tools attach at the next
session start if the bot previously had no host tools; the prompt text says
when a grant is not yet attached.

## Fallback when the tool is unavailable

A user can still add a bot from Crew. A bot that lacks `draft_bot` must say
so rather than invent success.
