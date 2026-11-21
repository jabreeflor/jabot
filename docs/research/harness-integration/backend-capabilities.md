# Backend capability comparison (#299)

**Version 2 — 2026-09-13.** The evidence behind
[`docs/decisions/issue-299.md`](../../decisions/issue-299.md). Re-issue this
file, with a new version line, whenever an adapter version pinned below
changes or a row is closed.

Pinned versions this comparison was read against:

| Component | Version | Where it is pinned |
|---|---|---|
| ACP spec | v1 stable surface + RFDs as listed on the spec index | `agentclientprotocol.com/llms.txt` |
| `@agentclientprotocol/claude-agent-acp` | 0.75.1 (bundled); changelog read through 0.76.0 | `src-tauri/vendor/adapters/package.json` |
| `codex-acp` | **not audited** — pending #297 | `catalog.rs` Codex card |
| Claude Agent SDK direct | reference only: T3 Code `ClaudeAdapter.ts` | not a JaBot dependency |
| Codex app-server direct | reference only: T3 Code `CodexAdapter.ts`, `effect-codex-app-server` | not a JaBot dependency |

A cell that says **audit** is a gap in this document, not a claim about the
adapter. A cell that says **unverified** names a claim nobody has exercised
against a live process.

## Gap classes

- **Host** — JaBot's Rust host or TypeScript client does not consume or
  advertise something the protocol and adapter already carry. Closed without
  a new backend.
- **Adapter** — the shipped ACP adapter for that harness does not expose it.
  Closed by an adapter upgrade or an upstream PR.
- **Protocol** — ACP has no stable verb for it. Closed by an ACP RFD landing,
  or by a direct backend where the native protocol has the verb.
- **Upstream** — the harness itself cannot do it, on any transport.

## The comparison

| Capability | ACP spec | claude-agent-acp 0.75.1 | codex-acp | Direct (reference) | JaBot host today | Class | Closed by |
|---|---|---|---|---|---|---|---|
| Resume / load / close a session | Stable (`session/resume`, `session/load`, `session/close`) | Yes; forks restored on load since 0.75.1 | audit | SDK `resume`; app-server `thread/resume` | Implemented: `supervisor/resume.rs`, `BackendCapabilities` | none | — |
| Config options (model, mode, effort) | Stable v1 `session/set_config_option`, `config_option_update` | Yes; recommended values advertised since 0.76.0 | audit | SDK options; app-server config | Only the compat `session/set_config` on `session/new`; no host method; `current_mode_update` dropped (`transcript.ts`) | host | #297 |
| Slash commands | Stable v1 `available_commands_update` | Yes | audit | SDK skill dispatch | Update dropped (`transcript.ts`) | host | #297 |
| Structured questions | Extension only (Cursor `cursor/ask_question`; adapter permission extension) | Permission extension with editable choices and durable effects | audit | SDK `AskUserQuestion`; app-server async questions arrive as notifications, answered with a user message | Implemented: `acp/extensions.rs` + `host/interaction/`; cards via `interaction/reply` (#298 / #303) | none | #298 |
| Plan decisions | Extension only (`cursor/create_plan`) | — | audit | — | Implemented: same path as questions | none | #298 |
| Native subagent activity | Draft client capability (`clientCapabilities.subagents` / `_meta` mirror) | Yes since 0.71.0, gated on the client advertising it | audit | SDK subagent snapshots (T3 attributes tool runs to the owning agent) | Client advertises only `fs` and `terminal` (`connection.rs::initialize`); never asked | host | advertise the capability on `initialize`, then render `subagent` updates |
| Usage reporting | Session usage updates stable; end-turn token usage is an RFD | Per-model token usage on the prompt response since 0.71.0; usage stats as Markdown since 0.75.0 | audit; app-server has `thread/tokenUsage/updated` | Both natively | Prompt response is stored under `state_update.result` and never shown | host + UI | surface `result.usage`; add a usage card |
| Session fork / branch | RFD only (`rfds/session-fork`) | Message-specific forks via extension since 0.71.0 | audit; app-server has `thread/fork` | SDK `forkSession`; app-server `thread/fork` | Branches copy the transcript and carry it in the first prompt (#266) | protocol | ACP RFD landing, or the first direct backend |
| Conversation rollback | None | unverified — T3 exposes Claude rollback only through fork | audit | app-server: rollback of turns (T3 `supportsConversationRollback`) | None; T3's rule applies: refuse before touching the filesystem | upstream / unverified | evidence first |
| Auth identity and account scope | Extension (`authStatus`) | Yes since 0.75.0 | audit | SDK env; app-server `CODEX_HOME` | Doctor shells out to `claude auth status` / `codex login status` | host | #218 |
| Permission mode kinds | Stable (`session/set_mode`, `current_mode_update`) | Yes since 0.71.0 | audit | — | `current_mode_update` dropped | host | #297 |

## Reading

Every row where the bundled Claude adapter says **Yes** and JaBot says the
host does not consume it is a host gap, and there are six of them (questions
and plans closed on #298 / #303). None of them is a reason for a direct
Claude backend. The two rows that are not host gaps — fork/branch and
rollback — are the candidates, and for both the stronger native offer is
Codex app-server's, not the Claude SDK's.

The Codex column is empty because `codex-acp` has not been audited at a
pinned version. #297 owes that audit; until it lands, no direct Codex backend
is admitted either.

## How to update this file

1. Bump the version line and the date.
2. Change the pinned version table first, then the cells that version
   changes.
3. When a row is closed, keep the row and write what closed it in the last
   column with the PR number.
4. A live check is recorded as *verified against `<adapter> <version>` on
   `<date>`*; a fixture-only check is recorded as *fixture*. Missing
   credentials are not a passed live check.
