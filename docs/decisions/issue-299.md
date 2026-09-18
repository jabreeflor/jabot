# Decision: the session backend boundary (#299)

Drafted 2026-09-13 with the first stage of
[#299](https://github.com/jabreeflor/jabot/issues/299). Amends
[decision #6](issues-4-6.md#6--what-is-a-bot). Everything #6 says about
*what a bot is* still holds: a scope (persona, tools, memory, credentials)
over an engine from the catalog, host-owned process trees, host-owned
permission prompts, host-selected MCP. What changes is one sentence.
"Every crew bot is an ACP harness session" becomes: **every crew bot is a
session on a host backend, and ACP is the default backend.**

**Status: boundary landed. No direct backend is justified yet.**

| Question | Decision |
|---|---|
| Where the host chooses a transport | `host/backend::select`, once per thread, at the first spawn. Never re-selected on resume. A thread stays on the backend that created it for the life of the conversation. |
| What the host sees | `SessionBackend` (`src-tauri/src/host/backend/`): create / restore / close a session, prompt, cancel, answer an interaction, tenancy, liveness. `HostSession` holds `Box<dyn SessionBackend>` and never names a transport. |
| The common event model | ACP `session/update` JSON, exactly as today. A backend that is not ACP translates its native events into that shape at its own boundary. There is no second typed event enum on the host. |
| Optional operations | Explicit flags on `BackendCapabilities`. Absent means no. An operation the backend did not advertise is refused before any conversation or workspace state changes. |
| Fallback | None. A backend that will not start is an error that names it. The host never silently mints a new session on a different transport after a failure. |
| Adding a direct backend | One `impl SessionBackend` plus one `BackendKind` variant, in its own change, tied to one capability the matrix marks unreachable over ACP, with its compatibility cost written down here first. |
| Tier 2 and tier 3 harnesses | ACP only, unchanged. Direct backends are a property of a compiled-in card. |

## Why a boundary, and why now

Until this change the ACP client *was* the host's session driver: an
`impl HostSession` block in `host/acp/mod.rs`, with the prompt queue, the
permission broker, the supervisor and the Doctor all reaching into
`AcpConnection` directly. That was the right first build — one runtime, one
process model, one seam to debug — and it is what #10 and #21 delivered.

It also meant a second integration had nowhere to go. #299 asks for direct
Claude Agent SDK and Codex app-server backends *where the evidence justifies
them*, and the honest way to hold that door open is a seam that the existing
path already passes through, so the day a direct backend is justified the
change is an `impl` and not a rewrite. Landing the seam first, with no wire
change, is also what makes the capability comparison a real comparison: both
columns would run through the same host code.

## Why the event model is not abstracted

`BackendEvent::Update` carries an ACP `session/update`-shaped payload, and
that is deliberate rather than lazy. That payload is already the host's
normalized event: it is what `persist_transcript_event` stores, what the one
reducer in `src/views/transcript.ts` replays, what the lifecycle reads a stop
reason from, what the preview and the PR watch scan. A native backend has to
translate its events into *something*; translating into the vocabulary every
consumer already speaks is the cheapest translation and the only one that
does not fork the transcript. T3 Code makes the same call in the other
direction — native events into its common model at the adapter boundary
(`docs/internals/providers.md`).

## Why no direct backend yet

The comparison in
[`backend-capabilities.md`](../research/harness-integration/backend-capabilities.md)
is the evidence. Its short form, as of the adapter versions it pins:

- **Claude.** The adapter JaBot bundles, `@agentclientprotocol/claude-agent-acp`,
  is itself a Claude Agent SDK sidecar, and since 0.71 it maps native
  subagents and async tasks, per-model token usage, message-specific session
  forks, permission mode kinds, slash commands and (0.75) the agent's auth
  identity onto ACP. The features #299 names as targets are reachable
  through the adapter JaBot already ships. What is missing is on JaBot's
  side: the client never advertises the subagent capability, drops mode and
  command updates, has no config-option method and does not surface usage
  from the prompt response. #298's questions and plan reviews now land
  through `BackendEvent::Extension`; remaining host gaps are #297 work
  plus small host changes, not a new transport.
- **Codex.** ACP session fork is still a draft RFD, and the app-server
  protocol offers `thread/fork`, `thread/resume` and per-turn usage
  natively. Whether `codex-acp` reaches those is the audit #297 owes. If it
  does not, Codex app-server is the first direct backend this decision
  would admit — a plain JSON-RPC subprocess needing no Node sidecar.

So the boundary lands with one implementation. That is not a placeholder;
it is the decision working as intended: a direct backend has to earn its
column.

## The cost a direct backend carries

Written before the first one exists, so it is not rationalised after.

- **A second release train.** The Claude SDK ships weekly; the ACP adapter
  pins it for us today. A direct sidecar would pin it for itself, and every
  bump is JaBot's to test. A Codex app-server backend tracks Codex's own
  protocol versioning and has to handshake a version at start and fail with
  both numbers in the error.
- **Packaging and process ownership.** A Node sidecar reuses
  `scripts/bundle-adapters.sh`, `bundle.resources` and
  `harness/bundled.rs`'s `CLAUDE_CODE_EXECUTABLE` rule; the same *process
  group* discipline (`procgroup.rs`) applies. Codex app-server is the
  user's own `codex`, like the Claude card drives the user's own `claude`.
- **Account scope.** A direct backend owns its own auth surface. It has to
  line up with #218's per-profile isolation (`CODEX_HOME`, `HOME`-scoped
  Claude state) rather than inherit the host's ambient login.
- **Two code paths for one card.** The catalog card gains a backend, the
  Doctor gains a probe, `thread/state` and `supervisor/status` gain a
  field, and every contract test runs twice. That is the ongoing cost the
  issue asks us to name, and it is paid per capability, not per provider.

## What this does not change

- No Anthropic or OpenAI tool loop in the host. A backend supervises a
  process and translates its events; it does not call a model.
- Crew bots are not Claude Code or Codex subagents.
- Decision #4's process and quit policy: the host stays in the Tauri binary,
  Quit kills adapter process groups, durability is resume.
- The permission broker is host-owned for every backend. An interaction the
  backend raises is answered by the host's policy or by a human, never by
  the backend.
- Existing ACP threads: same `sessionId`, same receipt, same resume recipe.
  Nothing on the wire moved in the change that landed this decision.

## Sequence

1. **Landed:** `SessionBackend`, `BackendEvent`, `BackendCapabilities`,
   `backend::select` / `backend::spawn`; the ACP path behind them; the
   shared contract test with an in-process fake and the real ACP
   connection. Wire-identical to before.
2. Persist `backend_kind` on threads and receipts, show it in diagnostics,
   refuse unsupported fork / rollback / config operations before state
   changes. Still one backend.
3. Complete the remaining ACP baseline (#297) and update the matrix with live
   adapter versions. #298's questions and plan reviews already ride
   `BackendEvent::Extension`.
4. A first direct backend, only if a matrix row still says *unreachable
   over ACP*. Codex app-server is the likelier candidate; Claude direct is
   expected to stay deferred.
5. Mark this decision settled with what shipped, and update
   [`harness-adapter-layer.md`](../requirements/harness-adapter-layer.md)
   and the setup docs.
