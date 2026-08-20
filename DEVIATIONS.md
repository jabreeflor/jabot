# DEVIATIONS

Running log of every place this build-out departs from the plan in
[`docs/plan.md`](docs/plan.md), the dependency graph in
[#30](https://github.com/jabreeflor/jabot/issues/30), or the settled
decisions in [`docs/decisions/issues-4-6.md`](docs/decisions/issues-4-6.md).

Each entry records **what the plan said**, **what was built instead**, and
**why**. Entries are appended in the order they were taken.

---

## D-001 — Added a test harness that was never scoped as an issue

**Plan:** No issue covers testing. CI (`.github/workflows/ci.yml`) ran
`npm run build` and `cargo test` only; there was no frontend test runner and
no way to exercise the host protocol end to end.

**Built:** A `scripts/verify.sh` single-command pipeline plus two new layers:

- **Vitest + jsdom + Testing Library** for the React renderer.
- **`jabot-hostd`** — an NDJSON-over-stdio binary wrapping the same
  `HostSession` the Tauri command uses, and a `StdioTransport` on the TS side,
  so `tests/e2e/` drives the *real* Rust host through the *real* wire protocol
  from TypeScript.

**Why:** The task requires "a way to test this all the way through once
things are done." The host protocol is the seam every issue crosses, so it is
the only place an end-to-end assertion is meaningful. This is additive — no
planned behaviour changed.

**Note:** `jabot-hostd` is *not* the sidecar extraction that decision #4 defers
("extract `jabot-host` when a second client exists"). It is a test-only
entrypoint over the in-process session; the shipping app still runs the host
inside the Tauri binary as decided. It does, however, prove the socket-shaped
claim in #4 — the same frames work off-process today.

---

## D-002 — #10 was already implemented in an open PR; merged instead of rebuilt

**Plan:** #10 (ACP client + adapter subprocess supervision) was an open issue
to implement.

**Found:** PR #36 (`cursor/harness-adapter-acp-e50f`, opened by another agent
before this session) already implements it — 1,843 lines across 19 files:
`host/acp/{connection,mod,runtime,spawn,wake}.rs`, a `fake_acp_agent` test
binary, and `tests/acp_adapter.rs`.

**Done:** Merged that branch into this one rather than writing a competing
implementation. Three conflicts, all additive, resolved as unions:

- `host/mod.rs` — both branches widened the protocol re-export list
- `store/overlay.rs` — their `set_thread_acp_session` alongside existing code
- `lib.rs` — both branches independently made the same non-macOS
  `window`/`api` fix

**Why:** Two implementations of the same issue would conflict on every file
#13, #20, and #21 need. Merging keeps the author's work and its history.

**Consequence:** If PR #36 is revised before it merges, this branch has to take
the update. If it merges to main first, this merge is already in the ancestry
and is a no-op.

### D-002a — fixed a false-negative in the merged tests

`kill_group_reaps_grandchild` failed on merge. The production code is correct:
`ps` shows the group-killed grandchild as `Z`, reparented to a PID 1 that does
not reap it. The test's `process_alive` used `kill -0`, which **succeeds for a
zombie** — so it reported a corpse as alive and failed correct code. Any
environment whose init does not reap promptly (most containers, including the
new Linux CI job) would hit this. Replaced the check with a process-state query
that treats `Z` as gone.

---

## D-003 — two host defects found by the e2e suite, fixed outside their issues

Writing the TypeScript-side e2e coverage for #10 surfaced two real defects
that the Rust-side tests could not see, because those tests drive
`HostSession` in-process and call `pump_acp()` themselves.

**1. `jabot-hostd` never pumped ACP.** My own bug, from D-001: the stdio host
was written before the adapter layer merged, so it drained outbound
notifications only after handling a request and called `pump_acp()` nowhere.
Adapter events sat unread — no `session/update`, `permission/ask`, or
`permission/resolved` ever reached a stdio client, and `sync/resumeFrom`
stayed at `headSeq: 0`. Fixed by giving it the same pump thread the Tauri host
runs (`spawn_acp_pump` in `lib.rs`), with stdout in place of the event bus.

**2. `session_cancel` cancelled before answering permissions.** In the merged
#10 code, `conn.cancel()` ran before `cancel_pending_permissions()`. #10
specifies the reverse: "Cancellation resolves outstanding permission requests
with `cancelled` before `session/cancel`". An agent blocked on a permission it
never gets an answer to has no reason to act on the cancel. Reordered.

Both were found because the e2e agent was told to report host bugs rather than
fix them out of zone — it wrote the cases against the intended behaviour and
`.skip`ped them with a precise repro. Both now run live; all 21 e2e cases pass.

This is the argument for D-001 in one paragraph: the in-process Rust tests all
passed against both defects.

---

## D-004 — #11: departures from the prototype, and why

`prototypes/jabot-classic.html` is the visual contract, but it disagrees with
the settled decisions in several places. Where they conflict, the decisions won.

| # | Prototype / plan said | Built instead | Why |
|---|---|---|---|
| 1 | — | `tauri.conf.json` untouched for chrome | The scaffold already shipped `titleBarStyle: Overlay` + `hiddenTitle`, so only the fake traffic lights needed removing |
| 2 | Keeps a sleeping thread row in its folder | Folded threads leave the sidebar entirely | Decision #5: "Fold = hide from sidebar". The prototype contradicted itself — its own Wait-for-Inbox handler removed the row |
| 3 | Fold increments the Inbox badge | Badge counts needs-you cards only | Decision #5: fold is visibility, not a notification. The prototype's own initial state agreed — badge "2" against two NEEDS YOU rows |
| 4 | Decorative search box | Search really filters folders and threads | No issue owns search; a control that does nothing is worse than ten lines that work. Easy to drop |
| 5 | Host picker in the chat header only | Also in the code-thread header | A thread is arguably the thing a second host would run. Trivial to remove |
| 6 | `[kind, html]` tuples with embedded markup | Per-tool-call items matching ACP `session/update`; grouping into one visual toolblock is a render concern | The prototype's shape cannot receive real ACP events |
| 7 | No harness picker in the bot editor | Bot / BotTemplate / BotDraft carry `harnessId` | Decision #6 — beyond the prototype, but the settled contract #17 will store |

`src/views/mock-host.ts` is a reducer rather than a fixture: each action maps
1:1 onto the host call that will replace it, so #14/#15/#17/#22/#26 swap the
reducer for real calls without reshaping the views.

Deliberately not built: the permission prompt UI (#20). The `notice` transcript
variant is shaped for it and says so, but no permission card exists.

## D-005 — #12: `bundle.targets` must include `app`, not just `dmg`

**Escalated by the implementer, and worth reading before anyone "tidies" it.**

`bundle.targets` changed from `["dmg"]` to `["app", "dmg"]`. In
`tauri-bundler`, the macOS updater archive is only emitted when the plain
`app` target is in the list. With `["dmg"]` alone the build **succeeds**, logs
one easily-missed warning, and publishes a release containing no
`.app.tar.gz` — an update feed nobody can update from. Reverting that list to
just `dmg` silently breaks updates for every installed copy. Documented in
`docs/packaging.md` so the next reader does not undo it.

Two further consequences of #12 worth recording:

- `createUpdaterArtifacts` stays `false` in `tauri.conf.json` and is merged in
  at release time via `--config`. With it on, `tauri build` hard-errors unless
  `TAURI_SIGNING_PRIVATE_KEY` is set, which would break every unsigned build —
  including CI's `bundle` job and anyone building on a laptop.
- **The first release run will fail by design**, at the preflight step, until
  the maintainer replaces the `REPLACE_ME__…` updater pubkey. That is
  deliberate: the alternative was committing a fabricated key that looks real.
  The failure is loud, immediate, and self-documenting.

The signing and notarization path has never been executed — there are no Apple
credentials here and the tooling is macOS-only. Its correctness rests on
reading the `tauri-bundler`, `tauri-cli`, updater-plugin and `tauri-action`
sources, not on running it.

---

## D-006 — #15: what the lifecycle core added, deferred, and reshaped

**1. `thread/open` is a new host method nobody scoped.** #15 owns the state
machine but no issue owns "create a thread row" — #16 registers folders, #17
stores crew, and #14 renders a transcript that assumes a thread exists. Without
it the state machine had no entry edge and nothing could be tested through the
API. `thread/open` is that edge (New Chat → `active`) and nothing more: it is
idempotent, takes the fields `threads` already has, and #16/#17 can layer folder
and bot defaults on top without changing the wire.

**2. Archive and delete kill the adapter; they do not send `session/close`.**
`state-machine.md` says both should reply `cancelled` to pending permissions and
then `session/close`. The first half is implemented. The ACP layer has no
`session/close` yet — #21 owns close and resume — so the process group is
terminated instead, which is what Quit already does. When #21 adds `close`, it
goes in `close_out`.

**3. The stuck backstop does not end the run.** Decision #5 lists `timed_out`
among the run states, and the obvious reading is that the idle timeout produces
it. `resurface.md` is explicit that stuck must **keep the process** so the user
can wait, reopen, or cancel — so the thread resurfaces `stuck` while its run
stays `running`, and `timed_out` is left for a hard cap that really does end a
run (#25 schedules, #21 supervision). A `stuck` card whose run said `timed_out`
would be lying about a process that is still working.

**4. Folding writes no Inbox event.** `inbox_events` has a `folded` kind, but
decision #5 defines Still Sleeping as `threads.state = folded`. Writing both
would give the Inbox two sources of truth for the same row, so `inbox/list`
projects sleeping threads from the thread table and reserves events for things
that actually happened.

**5. The away log is `judgment_call` rows, not a new table.** `resurface.md`
sketches an `awayLog[]` on the thread. `inbox_events` already has the kind, the
thread link, the run link, and a JSON payload, so an auto-allowed read lands
there with `reviewable: false` and `read_at` set on arrival — recorded, never
badged. If the digest in `resurface.md` ever needs richer structure, it can grow
into the payload.

**6. Not built here:** native notifications (#27 — the host emits
`inbox/resurface` and the badge count; no `UNUserNotificationCenter`), the
notification noise budget that goes with them, and boot reconciliation of the
process layer (#21 — after a restart every thread reports `acpState: unknown`
until something resumes it, which is the honest answer).

---

## D-007 — #13: what the harness catalog built, and what it declared instead of building

**Plan:** #13 asks for the Buzz three-tier catalog, a Doctor that says *why* a
harness is not ready, the GUI-launch PATH fix, and "one long-lived ACP process
per Hermes profile (not per chat); multiplex via ACP sessions."

**Built:** tiers 1–3 in `host/harness/` (compiled-in cards and presets, tier-3
JSON under `<data dir>/custom_harnesses/`), a concurrent Doctor with six
distinct failure statuses, and PATH augmentation used by both the probe and
every adapter spawn.

**1. Process pooling per Hermes profile is declared, not implemented.** The
catalog carries `sessionScope` (`thread` | `profile`) and
`HarnessDescriptor::profile_key`, and both reach the wire — so a client can
already see that Hermes chats belong on one process. The supervisor still keys
`HostSession.connections` by `thread_id`, so today each Hermes chat gets its own
process. Pooling means one connection carrying several ACP sessions, which
changes how permission requests and `session/update` are routed back to a
thread — that routing is #21's (session supervisor, keep-alive, resume), and
doing it here would have rewritten the map #15 and #10 both hold. The catalog
half is done so #21 does not have to guess which harnesses may share.

**2. `AdapterOutdated` needs the deep probe.** A binary's own `--version` says
nothing about which ACP it speaks, and no research source gives a minimum
version per adapter. `harness/doctor { deep: true }` spawns each ready adapter,
runs ACP `initialize`, compares the answer with the version the host speaks, and
kills it. The shallow default never reports outdated, because it cannot know.

**3. The renderer still reads its harness list from `mock-host.ts`.** The host
now serves `harness/list` and `harness/doctor`, and `HostClient` calls both, but
wiring the picker to live data belongs with the rest of the mock-to-host swap
(#17/#22). The drift guard in `src/__tests__/mock-host.test.ts` was widened
instead: every card the mock offers must match the Rust catalog's id, label,
blurb, and accent exactly.

**4. No new migration.** `harnesses` already had every column a catalog row
needs. Tier and card copy live in the compiled catalog and in the user's own
JSON, which stay the source of truth; the table holds only what
`threads.harness_id` must point at. A row is never deleted when a tier-3 file
disappears — that would take the threads that used it along with it.

---

## D-008 — #18: how OAuth was built without a client id, and what curl is doing here

**Plan:** #18 asks for the MCP catalog, OAuth 2.1 per remote server with tokens
in the #9 vault, per-bot allowlists, the same catalog passed as `mcpServers` on
ACP `session/new`, and connection status for the bot editor's chips.

**Built:** all of it, in `host/tools/`, plus migration `0003_tool_connections`.
Four things departed from the obvious reading and are worth recording.

**1. No OAuth endpoints and no client ids are compiled in.** The research lists
provider URLs, and the first instinct is a table of authorize/token endpoints
plus a JaBot client id per provider. There is no JaBot client id — one is issued
to a registered application with its own consent screen, and a fabricated string
produces a browser page reading `invalid_client` that no user can act on. So the
flow *discovers* instead: RFC 9728 protected-resource metadata names the
authorization server, RFC 8414 metadata names its endpoints, and RFC 7591
dynamic client registration mints the client id where the provider offers one.
Where it does not (Google, Slack), the id comes from the user's own registration
in `<data dir>/oauth_clients.json`, and the error names that file. This is also
what the MCP authorization spec asks of a client, so it is the correct design
rather than a workaround for a missing secret.

**2. HTTP goes through `curl`.** The crate has no HTTP dependency; the only one
in the tree is `reqwest`, pulled in by the macOS-only updater plugin, and adding
a TLS stack to every target — including the Linux e2e build the Cargo.toml
deliberately keeps thin — for four requests per grant is a poor trade. Form
bodies are written to curl's **stdin**, never argv, so no code, refresh token or
secret is visible in `ps`, and `require_safe_url` refuses plaintext http to
anything but loopback. The loopback exception is what makes the flow testable
against a local authorization server; it can never widen to a real provider.

**3. `JABOT_SECRETS_BACKEND=memory` is a new env knob on the vault.** On
Linux/Windows `Secrets::platform()` is `Unavailable` and every `put` fails
closed, which is right for production and makes the whole OAuth path impossible
to exercise on CI. The opt-in gives a process-local vault that dies with the
process — it is not a persistence path and cannot become one — so the Linux e2e
host and the flow tests can run the real thing.

**4. `tools/connect` is asynchronous, and the UI polls.** Consent takes as long
as a human takes, and the host answers JSON-RPC on one thread; blocking would
freeze every other thread in the app. So `tools/connect` returns once the flow
is running, publishes the authorize URL into `tools/list`, and the grant is
committed on the host thread when the browser comes back. There is no
notification for it: every host → client notification carries a `threadId`
envelope, and a connection belongs to no thread.

**5. One grant covers a provider, so `resource` repeats.** Decision #6 says one
user-level OAuth grant per provider, and Google serves Gmail, Calendar and Drive
from three separate MCP URLs. Asking for consent from the clicked chip alone
would mint a token carrying that chip's scopes and audience-bound to that chip's
URL, then hand it to the other two — a chip that says "Connected" over a server
whose every call fails. So `tools/connect` requests the union of the provider's
scopes and names every one of its MCP servers as an RFC 8707 `resource` (§2
allows the parameter to repeat). Status is then gated on coverage rather than on
the row: a chip whose scopes the grant does not carry reads `needs_auth` with
the missing scope named, and the session is denied that server instead of given
a dead one. The alternative — one grant per resource — would be a vault item per
chip and three consent windows for one Google login, which is the model decision
#6 rejected.

**6. The browser profile is a lease, not a path.** `--user-data-dir` is a
Chromium profile lock: two Playwright MCP processes on one directory is the
second one dying inside the adapter. JaBot runs one adapter per live thread and
the seeded crew chips Browser on three bots, so overlapping runs are ordinary.
The host therefore leases `mcp-profiles/browser` to one thread at a time and
skips the entry — with a reason — for anyone else, rather than handing out the
same locked directory twice. Liveness is read off the live adapter map, so a
thread whose adapter is gone has released the profile with no bookkeeping to get
wrong. The rejected alternative was giving the loser `--isolated`: the profile
holds the user's logged-in cookies and is treated as a credential, so silently
swapping in a logged-out browser is the same dishonest-tool failure the scope
gate above exists to prevent.

**What could not be exercised:** a real provider handshake. There is no
registered client and no human to click Allow, so no Google/GitHub/Notion/Slack
endpoint has ever answered this code. Everything between JaBot and the provider
*is* protocol, and it is tested against a local authorization server that checks
the PKCE verifier itself (`host/tools/testing.rs`) — discovery, dynamic
registration, the loopback redirect, the code exchange, and the token bundle
landing in the vault. What remains unproven is provider-specific behaviour:
whether Google's preview MCP endpoints publish the metadata documents this
discovery expects, and whether the scopes in the catalog match what those
servers actually require. First contact with each provider will need a live run.

---

## D-009 — #17: what the crew store built, what it derived instead of storing, and what it left to #22/#24

**Plan:** #17 asks for crew tables and CRUD over the host API, Chief
pre-installed and un-removable, the five default workers, four template packs
as JSON with add-from-template as a snapshot, the bot editor wired to real
CRUD, per-bot markdown memory, and the decision #6 isolation floor.

**Built:** `host/crew/` — `crew/list`, `crew/create`, `crew/update`,
`crew/remove` over `bots`; `crew/templates/*.json` compiled in; per-bot
`instructions.md` + `MEMORY.md`; `src/views/crew.ts` and the App wiring that
replaces the mock reducer's `saveBot` / `removeBot` with host calls, in the
shape #16 used for folders. Nine departures are worth recording.

**1. No new migration, and `memoryDir` is derived rather than stored.**
`bots` already had every column decision #6 names except `memory_dir`, and a
stored absolute path is the wrong shape for it: it would be written once,
outlive a moved or restored data directory, and then point at nothing. The
directory is `<data dir>/bots/<bot id>`, computed on every read, so a bot's
memory follows the app rather than a string written down last year. `BotView`
still carries `memoryDir` on the wire — the UI and #24 need the path, they just
do not need it persisted. It is `null` on an ephemeral host, which is the
honest answer when there is no data directory at all.

**2. `crew/list` writes.** Listing the crew creates each bot's directory and
refreshes its `instructions.md`. A read with a side effect is not free, and the
alternative was worse: the directory *is* a worker's `cwd` (decision #6), so a
crew whose workspaces do not exist is a crew that cannot be prompted, and the
shipped five would otherwise have no memory files until someone happened to
edit them. Failures are logged and do not fail the list — a read-only data
directory should cost the user their markdown, not their crew.

**3. `instructions.md` is derived; `MEMORY.md` is never touched after it is
created.** "No unattended memory writes without review" is implemented as a
rule about who owns which file. The persona is the record's, regenerated on
every save (and the file's header says so, because otherwise the first person
to hand-edit it loses their edit with no explanation). The notes are the bot's:
the host writes a two-line starter once and never again, so anything that lands
there landed through a session the user could watch, under the permission
prompts that session was already subject to. `crew/mod.rs` and `memory.rs` each
carry a test that fails if a save ever overwrites `MEMORY.md`.

**4. A new error code: `CHIEF_REQUIRED` (-32010).** Refusing Chief as
`INVALID_PARAMS` would make "you asked wrong" and "this seat cannot be vacated"
the same answer. Chief is refused twice over — at the API edge and inside
`store::catalog::delete_bot` — because `bots_one_chief` means a deleted seat
cannot simply be re-added, and the store function is the last thing between any
future caller and the row. Chief stays fully **editable**: persona, colour,
tools and harness. What cannot go is the seat.

**5. The allowlist vocabulary is closed at the write.** A colour has to be one
of the eight the renderer can render, and a tool id has to be an MCP catalog id
(#18) or one of Chief's host tools. This is stricter than the issue asked for,
and the reason is #18's enforcement model: a bot's chips are an allowlist the
session layer filters `mcpServers` through, so an id nothing recognises is a
capability that silently never arrives — a bot that looks equipped and is not.
Refusing it at `crew/create` / `crew/update` means the store can never hold one.

**6. Chief's host tools ship in `crew/list`, not `tools/list`.** `handoff_to_bot`
and friends are host actions, not MCP servers, and #18's catalog is explicitly
"one entry per provider surface". But the crew grid still has to *name* them or
it prints `spawn_code_session` at the user. They are compiled into `host/crew`
with labels and blurbs and travel with the crew; #24 implements them.

**7. The harness picker is now live, which closes D-007 §3 for the crew
surfaces.** #13 recorded that the renderer still read its harness list from
`mock-host.ts` and named #17/#22 as the owners of the swap. `useCrew` calls
`harness/list` alongside `crew/list` and `tools/list`, so the bot editor, the
crew cards, New Chat and the thread header all draw the real catalog —
including tier-2 presets and tier-3 user JSON, which the mock could never
offer. `mock-host.ts` stays as the fixture the shell renders before the host
answers (and in unit tests), and `mock-host.test.ts` gained two more drift
guards: the fallback templates must equal the shipped JSON packs byte for byte,
and the fallback host-tool labels must equal the host's.

**8. `Bot.unread` is not on `BotView`.** The red dot on a bot's blob is unread
work on that bot's standing thread — a projection of `inbox_events`, which is
#22's, over a standing thread, which is #24's. Nothing creates a bot's standing
thread yet, so the honest answer today is "no dot", and inventing one in the
crew payload would have put a second source of truth next to `inbox/list`.
The fixtures still show it; the host does not.

**9. The isolation floor holds, with one exception that is #18's decision, not
mine.** Each bot's ACP session is its own `sessionId` (one connection per
thread), each bot has its own directory, no credential is ever written into one,
and no bot's harness session store is handed to another. The exception is the
JaBot-owned browser profile: D-008 §6 leases one `--user-data-dir` to one thread
at a time precisely *because* the directory holds the user's logged-in cookies
and is treated as a credential — which means two different bots that both chip
Browser take turns with the same profile. That is a shared credential store
across bots, it is deliberate, and the alternative D-008 rejected (silently
handing the loser a logged-out `--isolated` browser) is worse. Recording it here
because #17's brief says "never share credential stores across bots" and this is
the one place the shipped product does. A per-bot browser profile would mean a
per-bot login, which is the model decision #6 rejected for OAuth; if it is ever
wanted, it belongs with #24 and needs a UI that explains why the user is signing
in twice.

**Not built here:** a bot's standing thread and Chief's host tools themselves
(#24), the unread badge (#22), and any use of the memory directory as a `cwd` —
the path is served and the files exist, but nothing spawns a session in one yet,
because nothing opens a bot's standing thread yet.

---

## D-010 — #14: the transcript overlay, and how steer-vs-redispatch was settled

**Plan:** #14 asks for ACP `session/update` mapped onto the prototype's chat,
streaming that does not re-render the world, the consumed events persisted
append-only so a reopened thread replays from our store, stop reasons feeding
the status line, the send box on `session/prompt` and cancel on
`session/cancel`, and a decision on what happens when the user talks to a
thread with a turn already in flight (setup-porting §6).

**Built:** `host/transcript/` (`thread/transcript` + the prompt queue), the
`src/views/transcript.ts` reducer and its hook, `LiveThreadView`, and the
queue/Stop chrome on `Conversation`. Eight things departed from the obvious
reading.

**1. Steer is not implementable, so the product answer is *queue*, with
interrupt underneath it.** Buzz's fork is `queue | steer | interrupt |
owner-interrupt`, and JaBot can offer exactly two of those: **every** ACP
adapter lacks steer. `session/prompt` is one turn per session and the stop
reason comes back on the *response*, matched to the thread rather than to the
prompt it answers — which is precisely why #15 refuses a second concurrent run
(a second turn would collect the first turn's outcome). There is no message
you can send an ACP agent that means "and also, while you work".

So `session/prompt` gained `mode`:

| `mode` | Behaviour |
|---|---|
| `reject` (default) | #15's `RUN_IN_FLIGHT`, unchanged |
| `queue` | Held in the host; sent the instant the turn ends |
| `interrupt` | `session/cancel`, then sent on the back of the cancellation |

**The UI always sends `queue`**, and shows a strip saying *N messages waiting*
with a **Send now** button that cancels the turn in flight — which is what lets
the queue go. That is the interrupt path, composed of two things the user can
see, so it was not also wired as a one-shot from the composer; `mode:
"interrupt"` exists on the wire for a client that wants both in one call, and
is covered by `src-tauri/tests/transcript.rs`.

Why not refuse-with-an-affordance? Because a refusal is what the user gets for
typing a sentence at a thread that happens to be thinking, and "your message
was rejected, try again in a minute" is a worse answer than "it will go next".
Why not queue by default on the wire too? Because `reject` is #15's contract
and a client that has never heard of this module must not grow a queue by
accident — the error is still the backstop for a UI whose idea of "busy" is
stale.

**The queue is RAM**, like `connections` and `pending_permissions` (#5: "still
working is supervisor RAM"). A queued prompt has not been said to the agent, so
nothing durable claims it has, and a host that dies holding one has lost a
draft rather than a turn. When the adapter goes, the queue is emptied into
`prompt_dropped` events that the chat renders as *Not sent — …: "…"*, because
silently swallowing the user's own words is the one failure this surface
cannot have.

**2. The host now writes the user's prompt into the transcript.** #10 persisted
only what the *adapter* said. Replaying that gives you the agent's half of a
conversation and none of the human's. `session/prompt` now appends the prompt
as the ACP `user_message_chunk` an agent would have sent — at dispatch, never
at accept, so a queued prompt is not written down as one the agent was given.
This shifts every existing thread's `seq` numbering by one and changed three
#10/#15 tests that asserted exact seq values; each was updated in place with
the reason, not loosened.

**3. `session/update` gained `transcriptSeq`, and it is not the envelope's
`seq`.** A client that hydrates while streaming has to know which live events
the replay already contained. The envelope's `seq` cannot answer that — it
counts permission and resurface notifications too — so it would compare two
different counters. Every `session/update` now carries the
`transcript_events` row it was written to, and `thread/transcript` reports
`headSeq`; above the head is new, at or below it is already drawn. Exact,
rather than a heuristic on payload equality.

**4. A host-ordering defect, found by this work and fixed outside its zone.**
Both `jabot-hostd` and the Tauri `host_rpc` command drained `outbound` and then
released the session lock *before* writing. Two drainers (a request thread and
the ACP pump) could therefore interleave, and a client would see a thread's
`seq` 3 before its `seq` 1 — observed, not theorised: the first draft of
`tests/e2e/transcript.test.ts` caught `[3, 1, 2]`. Both now emit under the
lock, and the e2e case asserts monotonic delivery. The reducer is also
defended: it de-duplicates against the *hydration boundary*, never against a
running high-water mark, because the latter silently discards a late-arriving
lower `seq`.

**5. Diffs and plans map onto blocks that already exist, because the prototype
has no others.** `jabot-classic.html` has no diff card and no plan checklist —
what it has is the toolblock's trailing note ("+18 −7") and a header status
line. So tool-call `content` of `type: "diff"` becomes the note, and a `plan`
update becomes `running · step 2/3` in the header, which is what
adapter-design.md asks for in as many words. Line counts come from `gitPatch`
exactly when one is sent, and from a line-multiset difference otherwise —
exact for pure insertions and deletions, an honest approximation for a rewrite,
and not an LCS over two whole files on every chunk.

**6. Not every ACP update is drawn.** `agent_thought_chunk` is dropped (ACP has
a `think` tool kind for reasoning an agent chooses to show; a raw thought
stream would double every chat with text the prototype has no bubble for), and
so are `available_commands_update` and `current_mode_update`. Persisted
`session/request_permission` rows are skipped by the reducer too: #20 owns that
card, and the tool call it refers to already shows as `pending`, which ACP
defines as "awaiting approval". A replayed permission prompt drawn as if it
were still waiting would be worse than none.

**7. Streaming is a structural-sharing contract, not a virtualization one.**
Every reducer step returns a new array whose other elements are *the same
objects*, and the rows are `React.memo`'d — so a chunk re-renders one bubble
and a tool status flip re-renders one block. The tests assert object identity
rather than render counts, because identity is the property that makes the
memoization work and a render count is a proxy for it. End-anchored
virtualization is still not built; `Conversation` keeps the tail-follow effect
#11 left, and the comment naming #14 as its replacement now names the
condition instead (a transcript long enough to jank).

**8. Bot chats are still the mock reducer.** `ChatView` carries the same queue,
Stop and error props as the code thread, and nothing passes them: a bot's
standing thread does not exist yet (#24 opens it, per D-009). The live path is
`LiveThreadView`, and `App` uses it for any thread the host owns — a fixture
thread keeps the mock.

**Not built:** the permission card (#20 — a request still resolves through the
host, it just draws as a pending tool line), markdown rendering inside agent
bubbles (the prototype's bubbles are plain text and no issue has asked for
more), `session/load` replay for a thread whose overlay is missing (store.md
step 3; #21 owns resume), and transcript search/FTS (store.md says explicitly
not MVP). One honest gap in the queue: if an adapter never answers a
`session/cancel` with a stop reason, a prompt queued behind it waits until the
adapter closes and is then reported as dropped — the host cannot invent an end
to a turn the agent will not end.

---

## D-011 — #21: what the supervisor built, what it refused to guess, and what it left declared

**Plan:** #21 asks for keep-alive on live adapter sessions, dead-adapter
detection, `session/load` and `session/resume` on the ACP layer, boot
reconciliation of the runs a stopped host left open, crash and sleep recovery,
the #15 fingerprint as the drift check on resume, and — from D-007 item 1 —
Hermes process pooling "if it falls out naturally".

**Built:** `host/supervisor/` (`boot`, `clock`, `keepalive`, `resume`), ACP
`session/resume` / `session/load` / `session/close` in `acp/connection.rs`, two
host methods (`thread/resume`, `supervisor/status`), and three new fields on
`ProcessView` (`pid`, `resumable`, `drift`). Nine things departed from the
obvious reading.

**1. Every run a stopped host left open becomes `lost`, and the card says
something else.** The two research files disagree about what the user should be
told, and both are right about their own case. `state-machine.md`: quitting on
an outstanding permission resurfaces `needs_you` with "the agent was waiting on
you; reopen to continue". `keep-alive.md`: a restart that interrupted a running
turn resurfaces `stuck` ("interrupted by restart"). Neither says `failed`,
because `failed` invites a retry of work we have no evidence went wrong. So the
*ledger* records one thing — `lost`, its word for "we stopped being able to
find out" — and the *Inbox* records what the human is being asked to do. That
is the two-axis model from decision #5 doing exactly what it exists for, and it
is why a `needs_you` card sitting over a `lost` run is not a contradiction.

**2. A boot card can restate an existing row instead of adding one.** The
common shape of "we quit with a permission outstanding" is a thread that had
*already* resurfaced `needs_you` when the agent asked — before the quit. The
transition is therefore a no-op the second time, and a boot pass that could
only insert would either say nothing (leaving a card that describes a live
request, over a process that is gone) or stack a second card for the same
unanswered question. `store::overlay::restate_inbox_event` updates the newest
undismissed card of that kind and marks it unread. It exists for this one case
and should not grow others.

**3. `thread/reopen` does not resume; `session/prompt` does, and there is an
explicit `thread/resume`.** `state-machine.md` describes reopening a sleeping
row as "reattach, or `session/resume` if dead". Making reopen do it would put
an adapter spawn and an ACP handshake — up to eight seconds against a wedged
binary — inside a click that today is a store write, on a host that answers
JSON-RPC on one thread. So resume is where it cannot surprise anyone: the first
prompt after a restart resumes instead of minting a new session (`keep-alive.md`
is explicit that `session/new` there "orphans the conversation"), and
`thread/resume` is the explicit verb for a client that wants to reattach
without prompting. Wiring a Reopen button to it is #22's.

**4. Drift refuses the resume, and the *prompt* still goes through.** A
harness, model, cwd, tool or permission-mode change means the conversation on
disk is not the job that would be spawned now, so `thread/resume` reports
`drifted` and spawns nothing. But refusing the user's next prompt on the same
grounds would leave a thread that can never be used again, so a prompt starts a
new session — and `lifecycle_run_started` rewrites the receipt, which is what
stops `thread/state` from going on claiming the old conversation is resumable.
`process.drift` names the fields, so a client can warn *before* the prompt.
There is no synthetic transcript event for it: #14's reducer ignores payloads
it has no `sessionUpdate` for, so a "we started over" row would have been
written to a table nobody renders.

**5. Sleep is detected from two clocks, not from a macOS API.** `Instant` does
not tick while the machine is suspended — on either platform — which is why the
idle backstop correctly refuses to call a lid-close "no output for ten minutes",
and equally why the supervisor cannot see the sleep at all if it reads only one
clock. `clock::SleepDetector` compares elapsed wall time against elapsed
monotonic time; the difference is the suspend. A stepped wall clock (NTP, the
user changing the date) produces the same signal, and that is an acceptable
trade: the response is to re-probe adapters and resurface threads that were
mid-turn, which costs one Inbox card, while missing a real sleep leaves a dead
session reported as live. `wake_from_sleep` is public so a future
`NSWorkspace.didWakeNotification` observer calls the same path.

**6. A pid is reaped, not waited on.** The connection layer learns an adapter
died from EOF on its stdout — but an adapter that forks something inheriting
that pipe exits without ever closing it, and the reader thread then blocks
forever on a pipe nobody will write to while JaBot reports a live session. The
keep-alive probe calls `try_wait` on every live adapter instead. The
`orphan-stdout` mode of `fake-acp-agent` is that exact process, and the test
fails without the probe.

**7. Idle eviction shipped, gated on the session being restorable.**
`keep-alive.md` asks for it (Buzz never evicts and pins a Claude process tree
per session for the life of the app). It is only safe because resume exists, so
an adapter that advertises neither `session/resume` nor `session/load` — or a
thread whose receipt has drifted — keeps its process: closing it would trade a
few megabytes for the agent's entire context and let the next prompt silently
continue in a new session. Folded-and-still-working never evicts at any age,
and neither does a thread with a pending permission, a queued prompt, or an
`active` overlay.

**8. Quit still kills without `session/close`; archive, delete and idle-evict
now close first.** D-006 item 2 left close to this issue. It is wired into
`drop_adapter`, which is what archive and delete already went through. Quit
deliberately does not: `session/close` "cancels work then frees adapter-side
resources", and cancelling a folded-and-running turn on the way out is the one
thing decision #4's resume policy exists to avoid. Killing the group and
resuming from the receipt keeps the turn's work; closing it would not.

**9. Hermes process pooling is still declared, not implemented — and here is
the specific reason.** D-007 handed it over on the grounds that the routing is
#21's. Having done the routing work, it does not fall out: `connections` is
keyed by `thread_id` in the ACP layer, the lifecycle layer, the queue, the tool
leases and the keep-alive probe, and pooling means keying by `profile_key` and
routing every inbound event back to a thread by `sessionId`. Two of the three
inbound kinds carry one — and the third, the ACP v1 prompt *response*, carries
only a request id, so the host would need a second request-id → thread map that
nothing today maintains. That is a rewrite of the map #10 and #15 both hold, on
a harness nobody has run yet. What shipped instead is visibility:
`supervisor/status` reports each live adapter's `profileKey`, so two threads
that *could* have shared a process are identifiable from the wire, and the
contract does not change when pooling lands.

### D-011a — a stranded prompt queue, found by this work and fixed in the supervisor

`src-tauri/tests/transcript.rs::interrupt_cancels_the_turn_and_then_sends_the_follow_up`
failed once under the load of a full rebuild, and the failure was real rather
than a slow test. #14 drains the prompt queue when a `session/prompt`
**response** arrives — the ACP v1 completion signal. Two paths end a turn
without one:

- A v2 adapter reports completion as an idle `state_update` and may never send
  the response at all.
- `intercept_in_flight` calls `session_cancel`, which pumps, *before* it
  enqueues the follow-up. An adapter that answers the cancel inside that window
  gets its turn end drained against an empty queue.

Either way the user's next message sits in a queue nothing will ever drain. The
fix is in the supervisor rather than in #14's ordering, because the condition is
a reconciliation and not a sequencing bug: the queue claims work is pending, the
ledger says no run is open, and the adapter says it is idle. All three have to
agree before anything is sent, so a turn genuinely in flight — which keeps its
run open — is never overtaken. `fake-acp-agent`'s new `v2-cancel` mode makes the
first path deterministic, and the test hangs for the full timeout without the
backstop.

### What #21 did not build

- **App Nap.** `keep-alive.md` wants
  `NSProcessInfo.beginActivity(.userInitiatedAllowingIdleSystemSleep)` while any
  thread is running. That is an Objective-C API and the crate has no `objc`
  dependency; adding one for a single call, on a target no test here can
  exercise, was not worth it. The consequence is a backgrounded JaBot whose
  adapters may be throttled with the lid open — visible as slowness, not as
  lost work, and the sleep path already covers the case where they die.
- **Native resume fallbacks.** Step 5 of the resume recipe (Claude
  `resume: <uuid>`, Codex `thread/resume`, Pi `switch_session`) is not
  implemented, and `threads.native_session_ref` is still never written — no
  adapter has handed us one. Each fallback is per-harness behaviour that cannot
  be tested without the vendor CLI, and the research calls it a last resort.
  Everything ACP-shaped is implemented and tested against a real subprocess.
- **A UI for any of it.** `thread/resume`, `process.resumable`, `process.drift`
  and `supervisor/status` are on the wire and unused by the renderer. The Inbox
  and thread views are #22's.
