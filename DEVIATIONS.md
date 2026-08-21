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

### D-010a — two things the reducer could not learn from the events alone

Review found that `ThreadStream` had exactly one writer for two facts the host
owns, and both went stale in the same way: **the renderer only ever heard about
its own actions.**

**1. A drained queue never shortened the strip.** `queued` was raised by
`markPromptQueued` and lowered only by an error or by `prompt_dropped`. When
the host *dispatched* a held prompt it wrote the ordinary `user_message_chunk`
item 2 above — indistinguishable from the echo of a prompt the user had just
typed — so the chat showed the same message as sent (a bubble) and as waiting
(the strip) at once, one stale entry per drain. The strip's **Send now** button
is `session/cancel`, so clicking it to unstick a message delivered minutes ago
killed whatever turn was actually in flight. Fixed on the wire, not in the
renderer: `drain_prompt_queue` now records the prompt with
`jabot: { event: "prompt_dispatched" }` beside the ACP shape, and the reducer
shifts the head of its mirror when it sees one. Beside and not inside, so a
client that has never heard of the marker still draws the bubble.

**2. `busy` could not describe a turn the renderer did not start.** It was set
only by `markPromptSent`, so a turn begun by a queue drain — or one already
running when the view mounted, which is every reopened thread, since
`LiveThreadView` is keyed on the thread id and remounts from `EMPTY_STREAM` —
offered no Stop button, built agent bubbles with `streaming: false`, and left
the header reporting the *previous* turn's stop reason while the agent was
mid-sentence. Two halves to the fix, because there are two ways to learn it:

- *From the events.* Agent output after a stop reason is a new turn, whoever
  started it, so `agent_message_chunk`, `tool_call`, `tool_call_update`, `plan`
  and a dispatched `user_message_chunk` all raise `busy` and clear the stale
  stop reason.
- *From the ledger.* The events cannot cover the mount-mid-turn case at all:
  the last row of a live turn and the last row of one whose host died under it
  are the same row. So `thread/transcript` now reports `runState` — the
  thread's **open** run, absent once it ended — read in the same call as the
  rows it has to agree with, and `hydrate` seeds `busy` from it. An optimistic
  `busy` from a prompt sent while that read was in flight still wins, or the
  Stop button would vanish under the user.

The second half also closed a smaller lie the first half would have introduced:
a replay over a run that has already ended now closes its trailing bubble, so a
thread whose host died mid-sentence stops blinking a caret at the user.

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

### D-011b — two holes review found in #21, and a new error code

**1. A missing `cwd` was refused on `thread/resume` and nowhere else.**
`keep-alive.md` is explicit — "cwd mismatch: refuse and resurface `failed`
('folder missing'). Do not silently `session/new` in a different directory" —
and `resume.rs` implemented exactly that. But the path a user actually takes
after a restart is not `thread/resume`; it is reopening the thread and typing.
`session_prompt` spawned first and asked later: `spawn_adapter` silently
dropped `current_dir` for a directory that was not there, so the adapter
inherited **JaBot's own working directory**, `attach_session` then saw a
receipt whose `cwd` failed `is_dir`, judged the session unresumable, and minted
a fresh one over it. An unmounted volume, a moved checkout or a #23 worktree
removed under a folded thread all reach it, and the result is an agent editing
files in whatever folder the `.app` was launched from with the real
conversation now unreachable. `session/prompt` now resolves the `cwd` before
anything is spawned and refuses with a new `CWD_MISSING` (-32012), resurfacing
`failed` the way resume does; `spawn_adapter` errors on a missing directory
instead of falling through, as the backstop for every other caller.

**2. Archive and delete dropped the adapter but kept the queue.**
`close_out` called `drop_adapter`, which — unlike `on_adapter_gone` — does not
touch `prompt_queue`. A thread archived while the user had typed a follow-up
kept those words in RAM, and `thread/reopen` then handed them back to
`intercept_in_flight`, which held every subsequent prompt behind them. Nothing
could drain it: #14's drain hangs off a `session/prompt` response, and D-011a's
backstop iterates live connections, which is empty for a thread with no
adapter. The thread reported "waiting" for the life of the app and could never
be prompted again — the exact failure D-011a set out to close, in the one case
its backstop could not reach. `close_out` now drops the queue, writing the
`prompt_dropped` rows #14 renders as "Not sent — …", and the backstop was
widened to *drop* (never send) a queue on a thread that has no connection and
no open run, so no future path can reintroduce the stranding.

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

---

## D-012 — #23: the worktree policy, and what happens to uncommitted work

**Plan:** `worktrees.md` specifies a host-owned `git worktree` per concurrent
code thread, created on spawn and cleaned up on Delete or on Archive-after-
merge, with a per-folder setup script and files-to-copy (#16 already stores
both). The cleanup table says "Unlock + `remove --force` after confirm if
dirty/unpushed" for Delete, and leaves Archive at "Remove worktree".

**Built:** `src-tauri/src/host/git/` — `worktree.rs` (add / lock / status /
save / remove / restore / prune, over the real `git`), `setup.rs`
(files-to-copy, `.worktreeinclude`, the setup command), and `mod.rs` (the
`HostSession` integration and the boot sweep). `thread/open` provisions the
tree **before** the row is inserted, so `cwd`, `branch` and `worktree_path` are
written by the same INSERT as the rest of the spawn record (#16's rule, kept).
`thread/archive` and `thread/delete` release it; `thread/fold` does not.
`threads.worktree_path` was already in migration 0001, so there is no new
migration. Two fields on the wire (`useCheckout`, `baseRef` on
`ThreadOpenParams`; `worktreePath` on `ThreadStateResult`), one new error code
(`WORKTREE_FAILED`, −32011), and no new methods.

**Why, and the five places this departs from the plan:**

**1. Uncommitted work is committed, not confirmed away.** The research assumes
a confirmation dialog — Conductor and Claude Code both prompt on exit. The host
has no prompt surface for cleanup (permission prompts are ACP's, and a thread
being archived has no adapter left to ask through), so a confirm would have to
be invented in the renderer and then trusted by the host. Instead **archive and
delete both run `git add -A` + `commit` on the thread's own `jabot/<id>` branch
first**, with `user.name`/`user.email` overridden, `--no-verify` and
`--no-gpg-sign` — a machine with no git identity, a failing `pre-commit`, or a
GPG key that wants a passphrase must not be able to turn "save this" into "lose
this". So the honest answer to *what happens to uncommitted changes when a
thread is archived*: they become one commit, on a branch JaBot never deletes,
recoverable with `git checkout jabot/<id>`. The commit is not assumed to have
landed there: `git commit` moves whatever `HEAD` is, and an agent that ran
`git checkout <sha>`, `git bisect`, or a rebase that stopped on a conflict
leaves `HEAD` detached — a commit made there is held only by the worktree's own
`HEAD`, which `git worktree remove` deletes, so the save would become a dangling
object while the host logged that it had saved it. The save therefore asks
`symbolic-ref` where `HEAD` is and, when the answer is not a `jabot/` branch,
plants `jabot/<id>-rescue` at the new commit before returning; the thread's row
is updated to that branch, so a reopen restores the tree holding the work rather
than one that does not. Ignored files (`node_modules`, the
copied `.env`) are not preserved — they were put there by setup, not by the
agent, and they are re-created by setup next time. If the commit **cannot** be
made, archive keeps the tree rather than removing it, and says so in the log;
delete forces, because delete is the user saying they meant it.

**2. The `jabot/<id>` branch outlives the thread, including a deleted one.**
The research leaves branch deletion to `gh pr merge -d` or a later sweep, and
this goes one step further: nothing here ever deletes a branch. A branch costs
a ref; it is also the only copy of anything the agent never pushed.

**3. Reopen restores what archive removed — an edge the research does not
cover.** `archived → active` is a legal transition in #15's table (kept for
#21's resume). Removing the tree on archive would otherwise leave that
transition pointing an adapter at a directory that no longer exists, so
`thread/reopen` re-adds the tree at the same path on the same branch and runs
setup again. Three guards make it a restore rather than an invention: the cwd
must be under our worktree root, must currently be missing, and the branch must
be one we minted.

**4. The setup command runs synchronously, and blocks the request.** A folder
configured with `npm ci` makes `thread/open` take as long as `npm ci` does
(capped at `SETUP_TIMEOUT`, five minutes). This is deliberate — an agent must
not start work in a half-built tree — but it is a real cost: the host handles
one request at a time, so New Chat spins for the duration. The alternative is a
"preparing" thread state plus a notification, which is protocol and renderer
surface nothing draws yet. Setup *failing* is not spawn failing: the tree
exists, the thread is the user's to prompt, and the incomplete report is logged.

**5. No `git fetch` before branching.** The research says "from `origin/<default>`
(fetch if stale)". Resolution is `origin/<default>` → `<default>` → `HEAD`, all
local. A fetch on every spawn adds a network round trip to New Chat and can
fail or hang on a credential helper for a repository the user has not
authenticated; a slightly stale base is a rebase, while a hung spawn is a
frozen app. `baseRef` is the explicit way to name something else, and a ref
that does not resolve refuses the spawn instead of silently using `HEAD`.

**Also decided rather than deferred:** a worktree is created for a thread whose
**folder is a git repository** and nothing else — a worker's standing thread has
no folder and gets none (decision #6), a folder that is not a checkout gets
none, and a repository with no commits gets none (there is no base to branch
from and nothing to collide over). Failing to create one **refuses the spawn**
rather than falling back to the shared checkout, since that fallback is exactly
the collision the issue exists to remove; `useCheckout: true` is the deliberate
way to work in the user's own tree.

### What #23 did not build

- **A renderer surface.** `useCheckout` and `baseRef` are on the wire and
  unused: New Chat has no "work in my current folder" toggle and no base-branch
  picker, and no view shows `worktreePath`. #11's `NewChatDraft` is unchanged.
- **A Repair action.** The boot sweep collects trees nobody claims, and the
  research's "offer Repair in folder settings" would be the same code behind a
  button; there is no folder-settings surface to put it on yet.
- **A disk cap.** Cursor's max-count sweep is explicitly not-MVP in the
  research, and it is not here. Nothing auto-deletes a tree on age or count —
  only on archive, on delete, and on being unclaimed at boot. "Unclaimed" is
  read off the store, so a boot whose store would not open skips the sweep
  entirely rather than treating an empty answer as "nobody claims anything" —
  a corrupt `jabot.sqlite` is exactly the boot where every live thread's
  checkout must survive.
- **PR-aware archive.** "Archive after merged" is #28's knowledge; archive here
  cleans up whether or not a PR merged.

---

## D-013 — #20: the broker's ledger, and what "answerable" means once the agent is gone

**Plan:** #20 asks for a permission broker with "a durable record of
outstanding requests: a request that arrives while the app is closed must still
be answerable when it reopens", the prompt UI, Wait for Inbox wired to #15's
policy, and answering that is idempotent and race-safe. #10 already routed
`session/request_permission` out as `permission/ask` and `permission/reply`
back; #15 already decided *whether* to ask. Neither wrote anything down: the
outstanding request lived in one `HashMap` in `HostSession`, so a quit — or an
adapter dying — took the agent's question with it, and a second click on the
same card came back as `unknown permission requestId`.

**Built:** `host/permission/` (the broker) over a new
`permission_requests` table (migration `0005`), a `permission/pending` host
method, and permission cards in the live thread view.

- **Every ask is a row, written before it is announced.** The same
  persist-then-notify rule decision #5 gives the Inbox, applied to the question
  instead of the result. The row carries what the agent asked, the options it
  offered, and which run it belongs to — everything a human needs to be asked
  again — and deliberately *not* the ACP request id, which belongs to a live
  adapter call and is meaningless to the next process.
- **Auto-allowed reads get a row too.** Wait for Inbox is still #15's
  `lifecycle_permission_policy` and there is no second policy path; what is new
  is that the answer the host gave on the user's behalf is recorded as
  `decided_by = 'host'` beside the asks it did put to them.
- **Answering is idempotent, and it says whether the agent heard it.**
  `permission/reply` now returns `alreadyAnswered`, `optionId` and `cancelled`
  as well as `delivered`. A second click returns what the *first* one decided.
  An id nothing has ever heard of is still `INVALID_PARAMS` — idempotence is
  about a request the host actually took.
- **The prompt UI** is #14's `notice` card, fed by `permission/ask` live and by
  `permission/pending` on hydrate, with the agent's own ACP options as its
  buttons and nothing else. Answering locks the card immediately, which is the
  renderer half of "two clicks must not double-answer".

### Where this departs, and why

**1. "A request that arrives while the app is closed" cannot literally
happen, so the durable record covers the ask that was outstanding when it
closed.** Decision #4 kills adapter process groups on Quit; a closed app has no
agent running and therefore nothing that can ask. The real case — and the one
`state-machine.md` describes — is an ask that was on the screen when the app
went away. That is what survives here.

**2. Quit leaves the record `pending` on purpose; a cancel resolves it.**
`shutdown_adapters` used to answer outstanding requests `cancelled`, which
would now also have resolved the row and thrown away the question at exactly
the moment it needs keeping. So withdrawal has two shapes: `Cancelled` (the
user stopped the turn, or the adapter died — nobody will ever answer, resolve
it) and `Abandoned` (the *host* is going away — the agent is still told
`cancelled` so it is not left blocked, and the row stays outstanding). #10's
ordering claim is unchanged and its test still passes.

**3. Answering a stale ask is recorded and not delivered, and the UI says
so.** After a restart there is no ACP call to hand the outcome to, and
`state-machine.md` is explicit that the next launch must not replay a dead RPC.
So `delivered: false` comes back, the card carries "JaBot restarted while it
was waiting, so your answer is recorded rather than delivered", and the
transcript gains a line saying to message the thread to pick the work back up.
The alternative — a card that fades on click exactly as it would have if the
agent had acted on it — is the one lie this surface cannot tell. The run
ledger is left alone in that case for the same reason: #21 has already closed
that run as `lost`, and putting it back to `running` because of an answer no
process is acting on would be the ledger asserting work that is not happening.

**4. A new table rather than the unused `permission_decisions` from `0001`.**
That table is scoped (`once` / `session` / `always`) and is about *remembered*
decisions — "always allow this" — which nothing offers yet. Outstanding
requests are a different question with a different lifetime, so they got their
own table and `permission_decisions` stays unused until a remembered scope
exists to put in it.

**5. An ask on a thread with no row is live-only.** `permission_requests` has
a foreign key to `threads`, so a prompt against a thread the store has never
heard of (an ephemeral host, `tests/e2e/acp-adapter.test.ts`) records nothing.
The ask still appears and is still answerable — the live half stands on its
own, and `permission/pending` unions RAM with the store rather than serving
only rows. What such a thread loses is durability it never had.

**6. The store grew methods, and the migration count moved to 5.** #20's file
zone is the broker and the UI, but "a durable record" is a table, and the
`Store` is the only thing that may open SQLite. Four `Store` methods and one
SQL module were added in the store's own style; nothing existing changed shape.

### What #20 did not build

- **A remembered "always allow".** ACP offers `allow_always` and the host
  passes it through as a button, but the host treats every option as a
  one-shot answer to one request: nothing consults `permission_decisions`
  before asking. That is a policy feature with a settings surface (#26) behind
  it, and inventing half of it — remembering without a way to see or revoke
  what was remembered — is worse than not having it.
- **Answering from the Inbox.** `permission/pending` takes an optional
  `threadId` precisely so the Inbox can list every outstanding ask across
  threads, and #22 owns that surface. Today the card is drawn in the thread.
  Answering a stale ask also does not clear the thread's `needs_you` Inbox
  row — the Inbox's read/dismiss rules are #22's.
- **Permission cards on a bot's standing thread.** `ChatView` still renders
  from the fixtures (D-009 leaves the standing thread to #22/#24), so the card
  is wired into `LiveThreadView` only. The reducer and the hook are the same
  for both the day the bot view goes live.
- **A second device answering.** `permission/reply` records `deviceId` and the
  card locks on anyone's `permission/resolved`, so the pieces are there, but
  pairing is MVP2 and nothing else was built for it.

---

## D-014 — #24: how Chief's host tools reach an ACP session, and what that cost

**Plan:** #24 asks for Chief's four host tools (`handoff_to_bot`,
`spawn_code_session`, `fold_thread`, `list_crew_status`) as real host actions,
Chief's standing thread (D-009 left it unbuilt), the tools exposed to Chief's
ACP session as **host-implemented** tools rather than as MCP servers from #18's
catalog, and a traceable handoff on a new table.

**Built:** `host/chief/` — `mod.rs` (the four actions), `tools.rs` (the schemas
a model reads), `bridge.rs` (a loopback MCP server the host answers itself);
`host/crew/standing.rs` (a bot's one standing thread); migration `0006_handoffs`
plus `store/handoff.rs`; `crew/thread` on the wire; `handoff` on
`ThreadStateResult`. Eight departures are worth recording.

**1. "Host-implemented tools" is a loopback HTTP MCP server the host runs
itself.** ACP has exactly one seam for giving a session tools — `mcpServers` on
`session/new` — and no notion of a client-implemented tool beyond `fs/*` and
`terminal/*`. So the host binds `127.0.0.1:0`, speaks MCP over it, and passes
the session a single `{"type":"http"}` entry pointing at itself. From the
adapter's side that is an ordinary remote server, which is the point: nothing
in `acp/` or in any harness had to learn about this. What makes it a *host*
tool rather than a catalog entry is who answers — the `HostSession` itself,
through the actions in `chief/mod.rs`, with no third-party process anywhere.
The alternatives were worse: a stdio bridge binary would need bundling and
would still have to talk back to the host over a socket, and a fifth
`tools/catalog.rs` entry would have made Chief's routing look like a provider
integration, which decision #6 explicitly refuses.

The security story is the ephemeral port plus a per-thread bearer token
generated at bind. Another process on the machine that finds the port cannot
hand work to the crew, and every call is attributable to the thread that made
it — which is what the handoff trail records. `bridge.rs` has the test.

**2. The bridge never touches `HostSession`, because it cannot.** The host is a
single `&mut` owner driven by a pump; a listener thread reaching into it would
need a lock around everything. So a request becomes a `Pending` on a channel,
the connection thread blocks on the answer, and the host drains and answers
from `pump_acp` — the same shape the ACP reader threads already use. The
adapter wake is pinged so an answer takes a millisecond instead of a tick.
`chief_dispatching` guards the one real cycle: a handoff prompts another
thread, prompting pumps, and the pump comes back here.

**3. What the MCP server deliberately does not implement.** No SSE stream (a
`GET` is answered `405`, which is what makes a client fall back to plain
POSTs), no MCP sessions, no resources or prompts, and no JSON-RPC batching —
removed from MCP in 2025-06-18, and every tool here answers in one round trip.
A second transport would be two code paths proving the same thing.

**4. A dispatch that could not be delivered is still a handoff.** The
`handoffs` row is written *before* the prompt is sent and `dispatched` is set
afterwards, for the reason #5 gives about the Inbox. A bot whose harness is not
installed produces a real, traceable handoff with `dispatched: false` and the
reason in `detail`, never a silent nothing. Both halves are tested — the
in-process tests assert the undelivered path (no `claude` on a test machine),
and the e2e suite registers a tier-3 harness pointing at `fake-acp-agent` so it
can assert the delivered one all the way into the receiving bot's transcript.

**5. The standing thread's id is derived, not minted.** `bot-<bot id>`, so "one
standing thread per bot" is enforced by the primary key rather than by a lookup
that can race, and `crew/thread` is idempotent for free because `thread/open`
already returns an existing thread. `cwd` is the memory directory #17 serves on
`BotView`, and `use_checkout` is set explicitly even though a thread with no
folder would not get a worktree anyway — the day someone gives a worker a
folder, that line is what keeps decision #6's promise.

**6. An archived standing thread comes back; a deleted one is replaced.**
A handoff has to land where the human can see it. Archive is a user closing a
conversation, not retiring a bot, so `crew/thread` reopens an archived standing
thread rather than putting new work into a closed row — and leaves a *folded*
one folded, because fold's promise is that it stays away until its own run
brings it back. Delete is terminal (`state::next_state` refuses every move off
it), which would otherwise leave a bot permanently unreachable, so the next
generation takes a suffixed id — `bot-writer-2` — the same way #23 finds a free
branch name. The live standing thread is still exactly one; the deleted
conversation stays deleted.

**7. The Code bot is resolved by convention, not by a column.** Decision #6
says Code owns folder threads and everyone else has one standing thread, but
nothing in the schema marks which bot that is. `spawn_code_session` resolves it
as: the bot with id `code` (so a *renamed* Code bot still owns its work), else
a non-Chief bot named "Code" (so an install where the user rebuilt it from a
blank bot still has an owner), else `None` — the thread opens with no bot, the
same as a New Chat in a folder with no bot selected. A `bots.owns_folders`
column would be more principled and is #17's schema; this is the smaller
change, and the fallback chain is tested.

`crew/thread` is *not* refused for the Code bot. Nothing needs a standing
thread for it, but refusing would mean hard-coding the same convention in a
second place to deny something harmless.

**8. Two cross-issue fixes, in the spirit of D-003.** `store/mod.rs`'s
`open_uses_wal_and_seeds_catalog` asserted `schema_version() == 5` as a
literal; it now asserts `migrate::head()`, so the next migration does not have
to edit it. And #19's pairing work, which landed in the tree during this issue,
left a duplicated `#[allow(unused_imports)]` in `host/mod.rs` and its
`map_paired_device` / `impl Store` block *after* `mod tests` — both are clippy
errors under `-D warnings`. The block was moved above the test module and the
duplicate attribute removed. No behaviour changed in either case.

**Not built here:**

- **Any renderer surface.** `crew/thread` and `HostClient.botThread` are
  served and typed, and `ThreadStateResult.handoff` is on the wire, but nothing
  in `src/views/` opens a bot's standing thread or draws "handed off by Chief"
  yet. The bot view is #22's, and inventing a Crew-grid gesture for it here
  would have put a second design next to the one that issue is going to make.
- **Chief's persona.** The seeded instructions still say "Route work across the
  crew" and nothing was added about *when* to use which tool; the routing
  policy decision #6 settled ("Chief does not call Gmail itself; it hands off
  to Inbox Mgr") is written into the tool descriptions instead, because that is
  the text a model reads at the moment it chooses.
- **A handoff list on the wire.** `store::list_handoffs_to` exists and is
  tested through the store; only the latest handoff is served, because that is
  what "where did this work come from" needs and a full trail has no reader yet.
- **Cancelling or reassigning a handoff.** A handoff is a record of a dispatch,
  not a job with a lifecycle; the run ledger (#15) already owns what happened
  next.

---

## D-015 — #19: what the pairing handshake actually proves, without a curve

**Plan:** #19 asks for device pairing — "QR + SAS + revocable scoped grants".
`docs/research/remote-and-mobile/pairing-security-mobile.md` sketches it: each
host and each device generates a keypair, the host shows a QR carrying a
single-use nonce, both sides display a Signal-style safety number, the user
confirms, the host records `{ deviceId, pub, name, createdAt, lastSeen, role }`,
and revoke is a list on the host. #8 already carried `deviceId` on
`host/hello` and refused any id but the local console's.

**Built:** `host/pairing/` (offer state, crypto primitives, role scope, the
seven methods), a `paired_devices` table (migration `0008`), `host/hello`
extended to admit a paired device that can prove itself, and a scope check in
`router::handle` that runs on every request.

**1. There are no keypairs, and the docs say so rather than implying there
are.** The research says "generates a keypair"; this host has no asymmetric
cryptography and adding one would mean either a new dependency (the set is
deliberately tiny — see `src-tauri/Cargo.toml`) or a hand-rolled curve, which
is a worse idea than an honest symmetric handshake. So a "fingerprint" here is
a **commitment** — `H(domain, key_material)` — not a verifying key. Each side
publishes a stable name for key material it never sends, a reinstall changes
it (the signal the research says must not be silent), and both fingerprints
are folded into the safety number. What it does *not* do is let either side
verify a signature, and `host/pairing/mod.rs` opens by saying exactly that.
Authentication comes from the out-of-band channel instead: both sides MAC the
transcript with the secret off the host's own screen, so a man in the middle
who never saw it cannot produce either proof and fails at the MAC check,
before any number is displayed. The credential is never put on the wire in
either direction — `pairing/claim` carries only the MAC, and the host learns
which channel was used by seeing which of the offer's two keys that MAC
verifies under. That is not decoration: every other field of a claim is
transcript material, so a frame carrying the credential too would be a frame
from which the safety number and the device token both fall out.

**2. The SAS is derived from both sides' material, and `pairing/claim`
deliberately does not return it.** The transcript is
`H["jabot/pairing/v1", hostId, hostFingerprint, hostNonce, pairingId,
deviceId, deviceFingerprint, deviceNonce, via]`, length-framed so no field can
absorb another's bytes, and every derivation is `HMAC(oobSecret, H[domain,
transcript])`. The device computes its own number; the host shows its own on
`pairing/status`; **both** send the number they are looking at to
`pairing/confirm`, and the host refuses to pair unless the two agree with each
other and with its own. Returning the number from `claim` would have been more
convenient and would have made the check theatre — a string one side computed
and the other displayed proves nothing. `tests/support/pairing.ts` is a
second, independent implementation of the device half written from the
protocol docs, so the e2e assertion is that two programs agreed rather than
that one function was called twice.

**3. Offers are RAM-only; only the grant is durable.** The research's step 2
says "single-use nonce, seconds-to-minutes TTL". Keeping offers out of SQLite
is how that promise survives a crash: a QR photographed off a monitor is
worthless the moment the host restarts, and a secret that was never written
down cannot be read out of a backup. An offer also stops answering after three
wrong credentials, which is what makes the headless eight-character Crockford
code defensible — 40 bits is a human's entropy, and the offer's patience is
not. `pairing/status` never returns `secret` or `code`; they are handed out
exactly once, by the call that creates them.

**4. Revoke is a tombstone, not a `DELETE`.** The research says "revoke
deletes the row". `paired_devices.revoked_at` keeps the promise — the device
is refused from the moment the row is stamped, and the stamp is on disk before
the answer goes out — and additionally answers "was this phone ever paired,
and when did we cut it off", which is the question a stolen phone actually
raises. Re-pairing upserts the same row with fresh key material and clears the
tombstone, so the list does not grow a second entry per device.

**5. The scope check runs in `router::handle`, on every method, as an
allowlist.** #19's file zone did not name the router, but "enforced at the
HOST on every call, never trusted from the client" is not a property a
per-handler check can have — the next handler somebody adds would not have it.
The role is read from the `paired_devices` row on each request rather than
cached at hello, which is what makes a revoke land on a device's *next call*
instead of its next connection. `approver` gets the list from the research
(Inbox, permission reply, read transcript, cancel) and nothing else, including
methods that do not exist yet: a denylist would silently open every new
surface to the least trusted device on the account.

**6. `host/hello` gained a proof, and `hello_rejects_unknown_device` is
untouched.** A paired device sends `auth: { counter, mac }`, an HMAC under the
token its pairing derived, with a counter that must strictly exceed the last
one this host accepted (stored per device, bumped in one guarded `UPDATE`).
Without that, `deviceId` would be a bearer token on a wire the host does not
yet control. Every way of failing — unknown id, revoked row, missing token,
bad MAC, replayed counter — returns the same `UNPAIRED_DEVICE` the host has
always returned, so the handshake cannot be used as an oracle. The local
console still says hello with no proof at all: it spawned the host, and the
research says to persist it as device #1 rather than make it a special case.
That free arm is fenced by *where* the caller is and *who it already is*, not
by what it says. It answers only the colocated connection (#29 gave each
connection an id; the socket's is not the console's), and only when that
connection is not already bound to a paired device — otherwise an `approver`,
which must have `host/hello` in its allowlist so a phone can reconnect, could
say hello a second time with no `device` at all and be re-bound as device #1.
A re-hello is re-authentication: from a paired device the only route to
another identity is a proof.

**7. The token is derived on both sides and never transmitted; the host keeps
it in the vault.** `token = base64url(HMAC(oobSecret, H[tokenDomain,
transcript]))`. Being precise about what that rests on: the token has no
ephemeral contribution, so its secrecy is exactly the secrecy of the
out-of-band credential — anyone who ever learns the credential of an offer
that completed can recompute the token of the device that completed it. Hence
the credential never travels (point 1), the offer is single-use, and its TTL
is seconds-to-minutes. SQLite stores only `token_ref`, the vault account name — the
same rule `store/secrets.rs` already holds. The cost is that where the vault
cannot produce the bytes (a Linux host, a locked keychain) the host cannot
check a proof and fails closed: the device must re-pair. That is the right
direction for an unverifiable credential, but it does mean the e2e suite runs
with `JABOT_SECRETS_BACKEND=memory` and therefore asserts the SQLite half of
durability — the grant and the revoke survive a restart — rather than
pretending a device can reconnect to a host whose vault was never persisted.

**8. Migrations may now have gaps, and `schemaVersion` is asserted against the
list.** Two waves were allocating migration numbers at once, so this one took
`0008` while `0007` did not exist yet, and `migrate` refused any version that
was not exactly `current + 1`. The contiguity rule was the wrong invariant:
numbers are allocated per issue and branches land out of order. It is replaced
by `check_order`, which still refuses a list that repeats or goes backwards —
the mistake the old check was really for — and by `migrate::head()`, so a test
about `host/hello`'s `schemaVersion` is a statement about the migrations that
exist rather than a number every wave has to edit.

**9. The store grew a module, as #20 did.** #19's file zone is `host/pairing/`,
`identity.rs` and a migration, but "revocable and durable" is a table, and
`Store` is the only thing that may open SQLite. One SQL module, five `Store`
methods, two row structs. Nothing existing changed shape.

### What #19 did not build

- **A transport.** There is still one host and one colocated client (decision
  #4). Everything here is exercised over the in-process session and over
  `jabot-hostd`'s stdio, which is the same frames; nothing listens on a
  network, no address is published in the QR (`addrs` is empty rather than a
  guess), and no encryption of the wire is claimed. Rule 1 of the research —
  TLS or Noise for anything that leaves the box — is the transport's job when
  one exists. The handshake is designed to survive an untrusted wire, but this
  build does not put it on one.
- **A device being able to authenticate the host on later connections.** The
  derived token is shared, so the material for a mutual challenge exists, but
  `host/hello` only proves the device to the host. A phone that wants to know
  it is talking to the same Mac as last time has the fingerprint to compare
  and nothing yet that makes the host prove it.
- **Any UI.** No QR is rendered, no safety-number sheet, no device list
  screen. `pairing/start` returns the exact string to encode and
  `pairing/status` returns the number to display, so the surface is a
  component away, but MVP1 ships one device and drawing a pairing screen for
  it would be inventing a feature nobody can use yet.
- **Notifications.** The host UI learns that a device has claimed an offer by
  polling `pairing/status`. A `pairing/update` notification would be nicer, but
  the notification envelope is thread-shaped (`hostId`, `threadId`, `seq`) and
  widening it for one poll-able screen was not worth the protocol change.
- **`approver` step-up, and "always allow" from a phone.** The research says a
  newly paired `approver` must not be able to widen host policy without a
  `full` device confirming, and calls that "later, not MVP2". Nothing offers a
  remembered scope yet (D-013), so there is nothing to widen; when there is,
  it belongs behind the settings surface, not behind this role check.
- **Multi-host anything.** Pairing is per host, as the research insists.
  A phone that should steer two machines scans twice, and nothing here
  coordinates that.

---

## D-014 — the local toolchain must match CI, or clippy is checked twice badly

`rust-toolchain.toml` says `channel = "stable"`, so CI installs whatever stable
is on the day. This container was pinned at 1.94 while CI ran 1.98, which meant
`./scripts/verify.sh` passed locally and then failed `clippy -D warnings` in CI
on lints the local compiler had never heard of. It happened twice:

- `chunks_exact` → `as_chunks` in the SHA-256 block loop behind PKCE. Taking
  clippy's advice would have been **wrong** — `slice::as_chunks` is newer than
  the 1.85 floor `rust-toolchain.toml` declares, so the "fix" would have broken
  the minimum version the project claims to support. The right answer was
  `src-tauri/clippy.toml` with `msrv = "1.85"`, telling clippy the same thing
  the toolchain file already says.
- `useless_borrows_in_formatting` in the pairing SAS formatter — a genuine
  redundant `&`, simply invisible to the older clippy.

Fixed by running `rustup update stable` so the container tracks the same
channel CI does. A gate that can only fail in CI is not a gate; it is a
surprise. Anyone reproducing this locally should update before trusting
`verify.sh`.

---

## D-016 — #29: the Mobile Inbox client, and the socket it needed to be a second device

**Plan:** #29 ships "MVP2 — Mobile Inbox client": the phone reads the Inbox and
answers permissions, over the socket-shaped host API (#8), as an `approver`
device (#19), with the host enforcing the role. Decision #4 says the host stays
in-process "until a second client exists", and
[`protocol-and-reach.md`](docs/research/remote-and-mobile/protocol-and-reach.md)
puts rung 0 of the reach ladder at a Unix domain socket.

**Built:** `src/mobile/` — a client surface written against the *existing*
protocol, plus the one host change a second client turned out to need.

### What is in `src/mobile/`

| File | What it is |
|---|---|
| `transport.ts` | `HostTransport` over any `LineChannel` — a duplex of NDJSON lines. A Unix socket, a WebSocket, an SSH tunnel: the reach ladder is a choice of channel, not of protocol. |
| `session.ts` | `MobileSession`: hello-with-proof, the live Inbox projection, `answer` / `decline` / `cancel` / `transcript`. Wraps the production `HostClient`. |
| `inbox.ts` | Needs you / done / still sleeping, from `inbox/list` + `permission/pending`. |
| `ask.ts` | Defensive reading of the ACP subject and options the host passes through verbatim. |
| `scope.ts` | The approver allowlist, mirrored from Rust, with a drift check. |
| `InboxScreen.tsx` | The screen: the agent's options as buttons, sleeping cards with nothing to press. |

**There is no mobile API.** Every method the phone calls already existed; the
frames are the frames `jabot-hostd` has been answering since D-001. That was
the point of decision #4, and the honest way to close #29 was to find out
whether it held rather than to assert it.

### The host changes, and why each was unavoidable

**1. A device binding is per *connection*, not per process.** `HostSession`
had one `connected_device`, which is correct for one webview and wrong the
instant two clients share a host: the phone's hello would have re-roled the
desktop, and `host/pairing/scope.rs` would have been reading the wrong row.
`handle_request_on(connection, request)` swaps the binding in around dispatch
and stashes it back; `handle_request` is that, with the id nobody had to
choose. Nothing else in `HostSession` was split — one store, one set of
adapters, one broker is exactly right, and pretending otherwise would have
been a rewrite rather than a second client.

**2. `jabot-hostd --listen <path>`.** A second connection needs a second
transport. The socket serves the same codec to every client, and notifications
are **broadcast** — which is not a convenience, it is the research's contract:
the host broadcasts `permission/ask` to everyone, the first authentic reply
wins, and everyone else gets `permission/resolved`. This is still the dev/test
binary (`dev-bins`); **nothing puts a listener inside JaBot.app**, so decision
#4's "in-process until it is not" is untouched.

**3. `host/hello` answers `scopedMethods`.** The host already knew what this
device's role permits. A phone hard-coding that list is a phone whose buttons
drift out of sync with the enforcement, and the symptom is a control that
exists and always fails. The list is still enforced by `scope.rs` on every
request; this only lets a client agree with it out loud, and
`src/mobile/scope.ts` fails a test when the two part.

**4. `permission/reply` no longer takes the caller's word for `deviceId`.**
It was a client-supplied string, written into `permission_requests.decided_by`
and broadcast in `permission/resolved`. With one device that was harmless. With
two it is the field that says *the phone answered this*, so a client that could
put the Mac's id in it could be recorded as the console it is deliberately not.
The host now uses the device this connection said hello as and refuses a claim
to be anybody else. Every existing caller already sent its own id, so nothing
had to change but the guarantee.

**5. Being the console is a fact about *where*, not a thing you can say.**
`host/hello` with no device — or with the console's own id — used to bind the
caller to device #1, `full`, on the strength of "who else would be calling".
That was true of one webview and false the moment a listener exists: the
console's id is not a secret (every `host/hello`, `host/health` and
`device/list` answer prints it), so a socket client could name it and be the
Mac, which makes #19's handshake optional for anyone who can reach the
transport — including a paired `approver` promoting itself on its own open
connection. `hello` now reads the connection the request arrived on and grants
the colocated identity only there; anywhere else, both spellings go through
`authenticate_paired_device` and come back `UnpairedDevice`.

**6. A connection is not a subscription.** Notifications are pushed, so
`require_hello` — which gates *requests* — said nothing about them, and a
socket was added to the broadcast set at accept. A client that connected and
sent nothing therefore received every `session/update` and `permission/ask`:
the prompt text, the agent's replies, the command being approved. That is the
leak rule 2 is about, from a process that cannot answer a single request.
`Clients::broadcast` now asks the session whether that connection has a device
before each frame, so the answer follows a revoke rather than being latched at
accept.

**7. The socket is `0600` in a `0700` directory, and a test says so.** Rule 1
lets rung 0 skip TLS *because* the socket can be "`0700` in a user dir", and
the bullet below leans on the same sentence. Under a default umask `bind`
produced `0755` — every account on the machine could connect — so the one
control the design rests on was prose. `bind_listener` sets the umask around
the bind (a `chmod` afterwards is a window, not a fix), creates a parent it
owns at `0700`, and the e2e asserts the resulting mode.

### What #29 did **not** build, and why

- **A phone.** There is no app, no store listing, no push notification, no
  React Native shell. What exists is the client surface and its protocol
  conformance. `InboxScreen.tsx` is a DOM component; a real device replaces it
  and keeps `session.ts` unchanged, which is the seam the split was for.
- **A transport that leaves the machine.** The socket is rung 0 — loopback,
  filesystem permissions (`0600`, and asserted, per change 7), no TLS, no
  Noise, nothing bound to a network interface. Rungs 1–3 (mDNS, Tailscale, an E2E relay) are reach work, and
  `pairing-security-mobile.md`'s rule 1 — encrypt anything that leaves the box
  — is the transport's job when one exists. The handshake was designed to
  survive an untrusted wire (D-015); this still does not put it on one.
- **Push, presence, and waking a sleeping phone.** The client applies a
  `permission/ask` the moment it arrives, which is the half that is protocol.
  Getting the device to be listening in the first place is APNs and a relay,
  and both are the reach work above.
- **Production pairing crypto in `src/mobile/`.** `MobileSession` takes a
  `DeviceCredentials` and never sees the token: which enclave or keystore holds
  it, and how the hello counter survives being killed, are device questions. The
  derivations are documented on `PairingClaimParams` and implemented
  independently in `tests/support/pairing.ts`, which is what the e2e drives.
  A real app ports that to WebCrypto; writing a third copy here, with no device
  to run it on, would have been code nothing tested against a real keystore.
- **Reconnect and replay.** `sync/resumeFrom` is in the approver allowlist and
  the host still logs per-thread `seq`, so a phone that drops mid-turn *can*
  ask for the rest. `MobileSession` does not yet: it reconnects by saying hello
  again and calling `refresh()`. That is correct and lossy — a `session/update`
  that arrived while the phone was in a tunnel is not replayed into the
  transcript view, because there is no transcript view yet.
- **Prompting from the phone.** Deliberately out of scope: `session/prompt` is
  not an approver method, and the research is explicit that the desktop stays
  the admin console. The phone answers questions; it does not start work.

### How it is proved

`tests/e2e/mobile-inbox.test.ts` runs **two clients against one
`jabot-hostd`** — the desktop on stdio, the phone on the socket — pairs the
phone through #19's real handshake with the independent device implementation,
and then asserts the thing the issue is actually about: the phone answers a
`permission/ask` it received by broadcast, and **the ACP adapter's own stderr
records the reply**. Asserting on the client's return value would have passed
with nothing reaching ACP at all. The same file asserts the host refusing
`thread/delete` from the phone, refusing the phone's attempt to be recorded as
the Mac, and refusing a revoked device on its next connection.

The same file also drives the socket from a client that is *not* a phone —
connected, unpaired, saying whatever it likes — because that is what opening a
listener created and what changes 5–7 close. It asserts that a bare hello and a
hello borrowing the console's id are both refused, that `pairing/start` (which
would hand out the pairing secret) is unreachable behind them, that a
connection which never identified itself overhears none of a prompt the desktop
drives to completion while a paired phone on an identical connection hears the
next one, and that the socket's mode is `0600`.

---

## D-017 — #25: what the in-process cron does about a Mac that was shut

**Plan:** #25 asks for an in-process cron in the host (decision #4: no launchd,
no daemon in MVP1), a fire that creates a run and delivers its result to the
Inbox, schedules that belong to a bot and run on its standing thread, a durable
record, and a **decision** — stated here and in the notes — about what happens
to an occurrence whose time passed while the app was closed.

**Built:** `host/schedule/` (cron parser, catch-up policy, tick, five host
methods), migration `0009`, `schedule_fires`, a `Schedules` screen in the
renderer, and the seam that finally writes `runs.kind = 'schedule'`.

### 1. The missed-fire decision: catch up once, never replay

This is the ruling the issue asked for.

| Case | What happens |
|---|---|
| Occurrence due while the host is running | Runs on the next tick (≤1s). |
| Occurrences missed while JaBot was closed, `catchUp: once` (default) | **One** run, for the most recent occurrence. Every earlier one is dropped, counted, and reported. |
| …and the most recent one is more than **12 hours** late | Nothing runs. The outage is recorded with the count. |
| Occurrences missed, `catchUp: skip` | Nothing runs. The outage is recorded with the count. |
| Any of the above | Exactly **one** `schedule_fires` row per outage, carrying `caughtUp` and `skippedCount`. |

Three properties fall out of that, and each one is a bug avoided:

- **A week of missed dailies is one run**, which is the failure the issue names.
  Seven agents starting at once against a laptop that has just woken up, each
  acting on a day that is over, is worse than six runs never happening.
- **Twelve hours is where "late" becomes "wrong".** A standup summary produced
  three days later is not the job the user wrote down. The window is a constant
  (`catchup::STALE_AFTER`) rather than a setting, because #26 owns settings and
  a knob nobody has asked for is worse than a number that can be moved.
- **Nothing is silent.** A skipped outage still writes a fire row, so
  `schedule/list` can say "6 runs were missed while JaBot was closed" — the
  screen never quietly shows a green schedule that has not run since Tuesday.

`skip` deliberately still runs an occurrence that is due *now*: the policy is
about occurrences that were missed, and reading it as "never run" would make it
a second off switch.

### 2. `0001` already had a `schedules` table; this extends it rather than
replacing it

The schema sketch shipped `schedules` in the initial migration — id, bot_id,
title, cron, prompt, enabled, last_run_at, next_run_at, last_thread_id — long
before anything could run one. `0009` adds the one column a working cron needs
(`catch_up`) and the `schedule_fires` table, and the store field names mirror
the existing columns (`title`, not `name`) so a reader of `models.rs` and a
reader of the schema are looking at the same thing. The rename to `name` on the
wire happens once, in the view.

The migration is numbered **0009**, not the free 0007. Filling a gap below a
number that has already been applied on somebody's machine would mean that
migration never runs for them — `migrate` skips anything `<= current`. D-015's
`check_order` allows gaps for exactly this reason; it does not make them
re-fillable.

### 3. The cron is written here, and takes an optional seconds field

No crate: it is a bitmask per field and one day-stepping search, and none of the
crates answer the question this module exists for, which is *"what did this
schedule owe between these two instants?"*. That needs `prev_at_or_before` as
well as `next_after` — collapsing an outage by walking forward from the oldest
missed occurrence costs a step per occurrence, and walking backwards from now
costs a step per day.

It is evaluated in **local time**, because "9am" has to keep meaning 9am after
the clocks change. Two consequences are decided rather than inherited: a local
time that does not exist (the hour spring-forward skips) is **not** an
occurrence, and one that happens twice (autumn) fires on the **first**, so a
1:30am job runs once on the night the clocks go back.

Six fields with a leading seconds field are accepted alongside the usual five
(the Quartz / robfig form). The UI only ever writes five. It is in for two
reasons: sub-minute schedules are a real thing to want, and the end-to-end
catch-up case — stop a host, wait, start it — cannot be tested at all if the
finest granularity is a minute. `tests/e2e/schedules.test.ts` uses a
two-second schedule to make a real quit-and-relaunch produce a real backlog.

### 4. A fire never queues, and never overlaps its own last run

`chief/mod.rs` delivers a handoff with `PromptMode::Queue`, so a busy bot ends
up with both jobs. A schedule does the opposite: if the bot's standing thread
has an open run or a queued prompt, the occurrence is **skipped** with a reason,
not held. This is Kubernetes' `concurrencyPolicy: Forbid`, and it is the right
default for a *timer*: a nightly job that overran would otherwise come back to N
copies of itself, all acting on a day that has moved on. A user who wants the
second job can press Run now.

### 5. One card per fire, and a new notification to announce it

A worker's standing thread is `active`, not folded, so #15's resurface path
correctly does nothing when a scheduled run ends — there is no thread to bring
back. The schedule therefore writes the `inbox_event` itself, titled with the
*schedule's* name rather than the thread's.

Two things follow. First, if the user has folded that thread, #15 has already
written a card for the run, and the schedule writes none: the check is
`inbox_events.run_id`, so one finished job is always exactly one row. The cost
is that a folded thread's card says "Writer finished" rather than "Morning
triage finished" — the schedule name survives in the fire row either way.

Second, a new notification: **`inbox/event`**. `inbox/resurface` is a claim
about the overlay ("a folded thread came back"), and emitting it for a thread
that never moved would be a lie about the sidebar. `inbox/event` is the honest
half — the Inbox changed, the thread did not.

### 6. Delivery is a reconciliation against the ledger, not a callback

`schedule_fires` rows are dispatched and then *watched*. Each tick asks the run
ledger how the fire's run ended and writes the card when it is terminal —
nothing is held in RAM between dispatch and delivery. That is what makes a fire
survive a quit: #21's boot pass closes the run as `lost`, and the delivery pass
turns that into a `failed` card on the next launch. A schedule whose result
depended on the process that started it would lose exactly the runs a user most
needs to hear about.

This rides **#21's existing boot pass** rather than adding a second one:
`reconcile_boot` ends with `reconcile_schedule_fires`. Deliberately only the
delivery half — dispatching there would put an adapter spawn on the app's
startup path, in front of the window opening, and the pump picks it up a tick
later anyway.

### 7. `runs.kind` was hard-coded, and now is not

`lifecycle_run_started` wrote `"prompt"` for every run. It now asks
`take_run_kind`, which returns `schedule` (plus a `trigger_json` naming the
schedule, the fire and the occurrence) for exactly one run on exactly one
thread — the one a dispatch has just claimed — and `prompt` for everything else.
The ledger has accepted `kind = 'schedule'` since `0001`; this is the first
thing to write it.

The run id is captured as the run *opens*, not looked up afterwards: a fast
agent finishes the whole turn inside `session_prompt`'s own pump, so by the time
the call returns there is no open run to find. The same reentrancy is why
`ScheduleState` carries a `dispatching` guard, exactly as `chief_dispatching`
does (D-014) — a fire's own prompt pumps, and the pump ticks the cron.

### What #25 did not build

- **A Settings home for the cron interval and the staleness window.** Both are
  constants (`JABOT_SCHEDULE_TICK_MS` exists only so a test does not wait out a
  second). #26 owns settings.
- **Native notification of a fire.** The host emits `inbox/event`; nothing
  raises an OS notification. #27 owns that, as D-006 already recorded for
  resurfaces.
- **A real Inbox reading these cards.** `inbox/list` returns them and the e2e
  asserts on them, but `App.tsx` still renders `InboxView` from `mock-host`
  fixtures — swapping that is #22's slice, and doing it here would have been
  rewriting someone else's view.
- **A card for a fire that could not open a thread at all.** An `inbox_event`
  needs a `thread_id`, so a bot with no workspace or no harness leaves the
  failure on the `schedule_fires` row (visible in `schedule/list`) and nowhere
  else. Inventing a thread to hang a card on would be worse.
- **Schedules for the Code bot's folder threads.** A schedule opens the bot's
  *standing* thread (decision #6), which Code does not use. A recurring job in a
  repository would need `spawn_code_session`'s path and a worktree policy for
  work nobody is watching; that is a bigger question than a timer.
- **`approver` access.** `schedule/*` is absent from #19's allowlist, so a
  paired phone cannot list or edit schedules. That is the default-deny working
  as intended: the desktop stays the admin console.

---

## D-018 — #26: what the fold path actually needed, and what it turned out already to have

**Plan:** #26 wires Fold and "Wait for Inbox" to real sessions on top of #15
(the fold transition, `fold_policy`, the resurface reasons), #20 (the broker and
its auto-allow-reads policy), #21 (keep-alive) and #22 (the Inbox).

**Built:** the affordance, the client wiring, and — mostly — the proof. The
honest headline is that **the host needed no production change at all**. Every
host-side claim in the issue already held; what did not exist was a test that
folded a session while it was *genuinely running*, and a UI that folded anything
but a fixture.

### 1. Every existing fold test folded an idle thread. That was the hole.

`src-tauri/tests/lifecycle.rs` and `tests/e2e/lifecycle.test.ts` between them
covered fold-then-prompt, fold-after-the-turn-ended, stuck, failed, needs-you
and the away log. Not one of them folded a thread whose adapter was *mid-turn*,
which is the only shape that proves the product's premise: the row leaves the
sidebar, the subprocess keeps working, and the thread comes back on its own.

Proving it needs an agent that will not finish until the test says so. A sleep
would make the ordering a race the test usually wins, so the fake agent got a
**`gated`** mode instead: the turn stays open until a gate file appears, and the
file's contents are a small script — a stop reason (`end_turn`, `max_tokens`),
or a comma-separated list of ACP tool kinds to ask permission for first
(`read,delete`). One mode covers finishing, failing, going quiet, and asking two
different questions in one turn.

That is new test machinery, not new product machinery. It is why
`tests/e2e/fold.test.ts` can fold a live session, watch it go on running, and
only then decide how the work it was already doing ends.

### 2. The host was right, and now says so out loud

Six new cases (three Rust, five TypeScript) assert what was previously only
implied: a folded-while-running thread keeps `process.connected`, keeps
`acpState: running`, keeps the *same run id*, disappears from `folder/list`,
appears under `sleeping` with `runState: running`, and resurfaces `done`,
`failed` or `stuck` with the right card. `failed` and `stuck` are asserted
against each other, because they are the pair the prototype conflated.

Each was checked against a deliberate regression before being kept — dropping
the adapter in `thread_fold` fails the live-fold case; always sending a `policy`
fails the "Disappear until done" case.

### 3. "Disappear until done" sends **no** policy, and that is deliberate

`state-machine.md` gives the in-chat fold the thread's *existing* `foldPolicy`
and reserves the policy change for Wait for Inbox. So the plain fold omits the
field entirely rather than sending `default`, which would silently undo a
quieter policy the user chose earlier. `thread/fold` already behaved this way;
the renderer now matches it, and so does `mock-host` — which had been
hard-coding `wait_for_inbox` on every fold. That drift was a mock making a
promise the host does not make, and `src/__tests__/mock-host.test.ts` now
asserts the host's rule instead.

### 4. The fold items are hidden where the fold is illegal

The transition table refuses `resurfaced → folded`, and the sidebar shows
resurfaced rows. Rather than let the user press a menu item that can only
produce `IllegalTransition`, `canFold` gates both fold items on `active`. The
error path still exists and is still tested — the host is the authority, and a
race can still lose — but it is no longer the ordinary outcome of a visible
affordance.

### 5. A refused fold puts the row back

The shell animates the row out before the call lands, so `useFoldThread` runs
its reload in a `finally`: on success the row is gone because the host says so,
and on failure it returns because the host says so. The alternative — reloading
only on success — leaves the sidebar hiding a thread that is still active.

### What #26 did **not** build

- **A live Inbox.** `inbox/list` returns the sleeping row and the resurfaced
  card, and `tests/e2e/fold.test.ts` asserts on both, but `App.tsx` still
  renders `InboxView` from `mock-host` fixtures. That swap is #22's, exactly as
  D-017 recorded for #25's cards. The consequence is visible: folding the thread
  you are reading navigates to the Inbox — the right destination, and the place
  fold promises the work will come back — but until #22 lands, that Inbox is
  showing fixtures rather than the row that was just folded.
- **Folding a thread with no folder.** The shell finds host threads through
  `folder/list`, so a folder-less row — a bot's standing thread — takes the
  fixture branch. Nothing creates one yet (D-009 leaves the standing thread to
  #22/#24), so there is no such row to fold; when there is, the lookup is the
  one line that has to change.
- **Chief's card against a real Chief.** The fold on Chief's notice card now
  goes to the host whenever the card names a thread the host owns, and there is
  a test for exactly that. Chief's transcript itself is still fixtures, so today
  the card in the shipping app names a fixture thread and takes the reducer
  branch. Wiring Chief's own conversation is #22/#24's.
- **A settings home for the fold defaults.** D-006, D-013 and D-017 each parked
  something here — the idle-timeout threshold (`JABOT_IDLE_TIMEOUT_MS`), a
  remembered permission scope, the cron interval. #26 added the *per-fold*
  policy choice, which is the affordance those settings would default; it did
  not add a Settings surface. That is still unowned, and naming #26 for it was
  optimistic: nothing in this issue's scope creates a place to put it.
- **An OS notification when a folded thread resurfaces.** Still #27's, as D-006
  recorded.

---

## D-019 — #27: what a banner is allowed to interrupt for, and what a Linux box can honestly prove about macOS code

D-006 left this for #27 ("the host emits `inbox/resurface` and the badge count;
no `UNUserNotificationCenter`") and D-018 pointed at it again. This is the entry
that closes it — and the honest account of what could not be executed here.

### 1. The noise budget is `needs_you`, `done`, `failed` — and nothing else

The issue names three transitions; `src-tauri/src/notify/mod.rs` implements
exactly those and lets everything else fall through to silence by default. Two
absences are decisions rather than omissions:

- **`stuck` does not ring.** It is a real `inbox_events` kind, and the whole
  point of D-006 #3 is that the process behind a stuck card is *still alive*.
  The ask is patience or a cancel. Interrupting someone to say "still working"
  is the exact noise a budget exists to prevent; the Inbox card says it just as
  well, and `notify/status` publishes the omission so it is discoverable.
- **`folded` does not ring**, because nothing writes it (D-006 #4) — and if
  something ever does, folding is the user asking *not* to be told.

`session/update`, `permission/ask` and `permission/resolved` are stream traffic
and are refused by name in the tests, so a future frame that happens to carry a
`threadId` cannot start ringing by accident.

### 2. `InboxResurfaceParams` grew `title` and `summary`

A notification has to name the thread it opens, and the card copy existed only
inside `resurface_and_notify`'s transaction. Rather than have the notify layer
query the store — which would put a read *between* the write and the notify, in
the one place decision #5 cares about order — the frame now carries the copy it
just wrote. Both fields are optional and `skip_serializing_if`, so:

- a client written against the original three fields still parses;
- `supervisor::boot`'s *restate* path, which re-announces a row it did not
  re-read, keeps calling the two-argument `notify_inbox_resurface` and gets
  reason-shaped fallback copy ("A thread needs you") instead of a blank banner.

`notify_inbox_resurface_card` is the new entry point; the old name delegates to
it with `None, None` so no existing caller moved.

### 3. `notify/status` is a host method nobody scoped

"A refused OS permission must degrade to the in-app Inbox, not break" is
satisfied structurally — nothing in the card path can observe a delivery
failure — but *silently* degrading leaves the user unable to tell "notifications
are off" from "JaBot is broken". `notify/status` is the difference: `supported`,
`authorization` (`granted` / `denied` / `notDetermined` / `unsupported`), and
`kinds` — the budget above, published rather than buried in Rust.

`unsupported` is deliberately not `denied`. Denied is a permission a human can
go and change; unsupported is a Linux build, or a dev build running outside
`JaBot.app`, with nowhere to send them. A settings screen that conflated them
would point people at a pane that does not exist.

It is **not** in `APPROVER_METHODS`: whether *this Mac* can ring is not a
phone's business, and the scope list is an allowlist, so it stays closed until
somebody decides otherwise.

### 4. Delivery choices worth knowing before someone "fixes" them

- **One banner per thread.** The notification identifier is
  `jabot.inbox.<threadId>`, and `UNUserNotificationCenter` *replaces* a
  delivered notification whose identifier it already holds. A thread that asks
  for permission and then finishes updates its one banner instead of stacking
  two. The durable record is the Inbox; this is a tap on the shoulder, and five
  taps about one thread is worse than one current one.
- **No `willPresentNotification:` delegate method**, so macOS suppresses
  banners while JaBot is frontmost. That is the right default here: someone
  looking at the app has the Inbox in front of them, and #27 is about the times
  they are somewhere else.
- **No badge.** Authorization asks for `Alert | Sound` only. The Dock badge
  belongs to the Inbox count (#22), and a second, divergent number would be
  worse than none.
- **Every entry point is guarded on `NSBundle.mainBundle().bundleIdentifier()`.**
  `UNUserNotificationCenter` raises an Objective-C exception — an abort, not an
  `Err` — when the process is not a bundled app, and `tauri dev` runs the bare
  binary. Unbundled, the whole module is silence.
- **The click sink is a process global.** Its other end is an Objective-C
  delegate the system owns and calls on its own schedule; there is no `self` to
  hang it off. Installed once, from `lib.rs`'s `setup`.

### 5. What was actually verified, and how

There is no display and no macOS here, so **no notification has ever been
delivered, and no banner has ever been clicked.** What *was* done:

- The whole decision layer — which kinds ring, the payload, the `userInfo`
  round-trip that routes a click back to a thread, the identifier policy, the
  fallback copy — is portable Rust with 11 unit tests that run on Linux.
- `notify/status` and the widened resurface frame have e2e cases in
  `tests/e2e/notifications.test.ts` driving the production `HostClient` against
  a live `jabot-hostd`, including the one that matters most: the Inbox card
  lands on a host whose own answer to "can you notify?" is *no*.
- The renderer half — the shell subscribes, an activation opens that thread, a
  thread that is gone falls through to "check the Inbox", and the subscriber
  degrades to a no-op with no Tauri event bus — is in
  `src/__tests__/notifications.test.tsx`.
- **`src/notify/mac.rs` was type-checked and clippy-checked against the real
  macOS target**, which is more than D-005 could claim for the signing path.
  The recipe, for whoever touches it next:

  ```
  rustup target add x86_64-apple-darwin
  # a scratch crate with objc2 / objc2-foundation /
  # objc2-user-notifications / block2 and:
  #   mod host { pub const INBOX_EVENT: &str = "inbox/event";
  #              pub const INBOX_RESURFACE: &str = "inbox/resurface"; }
  #   #[path = "…/src-tauri/src/notify/mod.rs"] mod notify;
  cargo clippy --target x86_64-apple-darwin
  ```

  Cross-checking the *whole* crate does not work: `objc2-exception-helper`
  (pulled in by tauri) has a `cc` build script that needs an Apple toolchain.
  The scratch crate avoids it because nothing in `notify` uses ObjC exceptions.

**What still needs a real Mac**, in the order someone should try it:

1. That the permission prompt appears once, on first launch of a signed
   `JaBot.app`, and that refusing it leaves the app working.
2. That a banner appears at all — this is where an unbundled or unsigned build
   fails, and where `bundleIdentifier` guard turns a crash into silence.
3. That clicking one un-hides the window and lands on the named thread.
4. That a second card on the same thread *replaces* the first rather than
   stacking, which is the identifier policy in §4 and the only claim here that
   depends on framework behaviour rather than on our own code.

### What #27 did **not** build

- **A view that reads `notify/status`.** The method, the client call and the
  types are there; nothing renders them. The natural home is the Settings
  surface D-018 records as still unowned — or a one-line note in the Inbox
  header when `supported && authorization === "denied"`, which is #22's view to
  change, not this issue's file zone.
- **Notification actions.** No Allow / Deny buttons on a `needs_you` banner and
  no inline reply. `UNNotificationCategory` is the mechanism and the category
  identifier is already stamped on every notification, so the seam exists; the
  decision that answering a permission from a banner is safe belongs with #20's
  broker, not here.
- **A user-facing on/off switch, or per-bot notification settings.** The budget
  is a constant. Making it configurable needs the same missing Settings home.
- **Anything on the Dock icon.** See §4.

---

## D-020 — #28: how a thread proves it opened a PR, and how much of GitHub had to be faked

**Plan:** #28 asks for the Pull Requests view on real data, a link between a
thread and the PR its branch opened (`thread_prs.thread_id` NOT NULL,
`provider + repo + number` as the dedupe key), PR state / checks / review state
in the view, Inbox cards for PR events worth surfacing, and `gh auth token` as
the auth story with no JaBot GitHub App and no token in SQLite.

**Built:** `src-tauri/src/host/pr/` — `detect.rs` (linkage from ACP traffic),
`github.rs` (the GraphQL document, the parser, the `gh pr view` / `gh pr list`
fallbacks), `card.rs` (which change earns an Inbox row) — over a new
`store/pr.rs` and migration `0010_pull_requests`. Two host methods (`pr/list`,
`pr/refresh`), one new field on `ThreadStateResult` (`pullRequests`), one new
`inbox_events.kind` (`pr`), `git::worktree::head_branch`, and
`src/views/pulls.ts` + the `PullRequestsView` props that put the board on real
data. Nine things departed from the obvious reading.

**1. Two methods, because they fail differently.** The obvious design is one
`pr/list` that refreshes as it reads. That makes the board unavailable to
everyone the poll cannot serve — and the poll needs `gh`, a login, and a
network, while the *linkage* needs none of them. So `pr/list` is a store read
that cannot fail, `pr/refresh` is the network, and a refresh that reaches
nobody is **not an error frame**: it resolves with the rows intact and the
reason in `unavailable`, in the same three-fact shape (`reason` / `detail` /
`remedy`) `github/status` already established. A client polls this every fifteen
seconds; one that throws because `gh` is not installed is a client that takes
its own board down.

**2. The host asks GitHub through `gh api graphql`, so the token is never read
into this process.** #16's brief says auth is `gh auth token`, and
`repo::gh::token` exists for it. Using it here would mean a token in this
process's memory, in an argv or an environment somewhere, for every poll —
where `pr-linkage.md` explicitly allows "`gh api graphql` … fine as the first
implementation (no extra dep)". Letting `gh` make the request is the same login
one layer up and one fewer place a credential can leak from. It also avoids
adding an HTTP+TLS stack to a crate that deliberately has none (D-008 made the
same call for OAuth, and reached for `curl` for the same reason). Nothing here
writes a token to SQLite, logs one, or puts one on the wire.

**3. `statusCheckRollup` hangs off the commit, not off the pull request.**
`pr-linkage.md`'s sketch puts it on `PullRequest`; that field does not exist in
GitHub's schema and the query would not resolve. The document built here walks
`commits(last: 1) { nodes { commit { statusCheckRollup } } }`, which is the path
`gh pr checks` itself uses. Worth recording because the research file is
otherwise the spec and someone will read it and wonder why the code disagrees.

**4. Every owner and repo name is a GraphQL *variable*.** Aliases (`pr0`, `pr1`,
…) are the only thing this code writes into the document. An owner/name comes
out of a git remote, which is a string a repository controls, and interpolating
one into a query is an injection with the user's own token behind it. There is a
test that fails if a repository name ever appears in the document text.

**5. Only `execute` output is evidence, and what the agent *says* is not.**
`pr-linkage.md`'s ladder is stdout → `gh pr view` → head-branch match → chat
text, and the last rung is explicitly not allowed to write a row. Two guards
make that real. Tool-call output is scanned only for calls the adapter declared
as `kind: "execute"` — a `tool_call_update` need not repeat the kind, so the
execute ids are remembered per thread, per turn, in RAM — because an agent that
*reads a file* mentioning a pull request has not opened one, and a link is
written once and never re-derived. And a compare URL is refused outright: `git
push` prints one on every single push, it names no PR, and linking it would
attach a number that does not exist. Agent prose only *arms* the post-turn `gh`
call.

**6. A link is refused for a repository the thread has nothing to do with.**
An agent that runs `gh pr view --repo somebody/else` would otherwise attach a
stranger's PR to this conversation permanently. A link is accepted for the
thread's own `repo`, or for a repository of the same *name* under a different
owner — which is what a fork's `gh pr create` prints, since it opens against
upstream. A thread with no repo stamped on it accepts nothing at all
(decision #6: workers have no checkout).

**7. The first thread to claim a PR keeps it.** `(provider, repo, number)` is
the dedupe key and `thread_id` is not in it, so a second detection updates
GitHub's half of the row and leaves the link alone. Re-pointing would make the
board's "Reopen thread" silently change which conversation it opens, on evidence
no stronger than what wrote it. The one thing that removes a link is the thread
going: the foreign key cascades. The key is also **not** widened to include
`forge_host`, even though the column was added — a UNIQUE constraint cannot be
altered in SQLite without rebuilding the table, and the case it would catch (the
same `owner/name` on github.com *and* on a GHES host, both linked on this Mac)
is rarer than the cost. `forge_host` exists because `gh` is addressed per host.

**8. `inbox_events.kind` gained `pr`, and the table was rebuilt for it.** Every
existing kind is a claim about a *run*: `failed` over a green run whose CI went
red an hour later says the turn failed, and `needs_you` says an agent is blocked
on the human. Both are lies the Inbox would then have to draw, about a session
that is usually finished and archived by the time its checks go red. The kind is
a CHECK constraint and SQLite cannot alter one, so `0010` copies the table,
drops it and renames — safe because nothing has a foreign key *into*
`inbox_events`. Two consequences:

- `count_unread_inbox` gained one exception. It counted only threads in
  `folded` / `resurfaced`, on the reasoning that an archived thread's badge
  points at a row that is not there. A `pr` card is not about the thread, so
  gating it that way would mean the only cards this kind produces are cards
  nobody is told about. `pr` counts for any non-deleted thread; everything else
  is unchanged.
- The card goes out as `inbox/event`, never `inbox/resurface` — it moves no
  thread, which is exactly the distinction #25 drew for schedule fires.

**9. Cards are written on *transitions*, never on states.** The poll runs every
fifteen seconds while checks are moving. `card::transition` compares the row
before against the row after, so a PR that has been red since lunch produces one
card, not one every fifteen seconds; red *and* reviewed in the same poll is one
interruption, about the fire. Three events clear the bar — opened, checks
failed, changes requested. Deliberately not cards: a merge (the user did that,
usually in the browser they are looking at), an approval (good news, not a task
— it shows on the row), and checks going green again (the absence of the red
card is that news).

### What could not be exercised, and what that leaves unproven

**There is no GitHub credential here and no egress to the API, so no GitHub
endpoint has ever answered this code.** What is tested, and how:

- The GraphQL **parser** runs against fixtures in `host/pr/fixtures/`. Those
  fixtures are **hand-built from the documented schema, not captured from a live
  API** — there was no token to capture with. They cover the two
  `statusCheckRollup.contexts` union members (`CheckRun`, `StatusContext`), a
  draft, a merged PR with no rollup at all, and a `data` + `errors` partial
  failure.
- The **transport** runs for real in `tests/e2e/pr.test.ts`: a `gh` script is
  put on the host's PATH and answers `gh api graphql` from a file, so the query
  the host builds, the argv it runs, the JSON it parses, the row it writes and
  the card it earns are all production code. Only the network is a fixture.
- **Linkage** is end-to-end against a real `fake-acp-agent` subprocess and a real
  git repository (`src-tauri/tests/pr.rs`, plus the e2e file), using a new
  `execute` mode on the fake agent that echoes the prompt as shell output.

What a real token would still have to prove: that GitHub's field names and enum
spellings match what `snapshot()` reads (they are from the published schema, but
`reviewDecision` and the rollup states are the kind of thing that has variants
in the wild); that a GHES instance answers the same document; that the rate-limit
behaviour of a 15-second poll over a real board is as cheap as the research says;
and that `gh pr view` in a #23 worktree resolves the branch's PR — the fallback
is implemented and its JSON parsing is fixture-tested, but it has never been run
against a `gh` that could answer.

### What #28 did not build

- **The desktop Inbox rendering these cards.** The host writes them, `inbox/list`
  serves them, and the *mobile* Inbox (#29) draws them today because it already
  reads the host. `src/views/InboxView.tsx` still renders `mock-host.ts`'s
  fixtures — swapping that is #22's, and doing it here would have meant
  rewriting the projection for every card kind, not just this one. The pieces
  this issue owes it are in place: `InboxKind` has `pr`, `inboxTag` draws it,
  and `NEEDS_YOU_KINDS` counts it.
- **A native banner for a PR card.** #27's budget is `needs_you` / `done` /
  `failed` and was left alone. Widening the noise budget is that issue's call,
  and "your CI went red" is exactly the kind of thing that should be argued for
  rather than added quietly.
- **Merge from JaBot.** `pr-linkage.md` defers it and there is no host method for
  it, so the row's buttons are "View on GitHub" and "Reopen thread". A Merge
  button that opened a browser would be worse than the link that is honest.
- **An in-app diff.** Same file, same reason. The thread's own diffs are #14's;
  the PR diff is GitHub's, after the push.
- **A closed PR anywhere on the board.** `PullRequestsView`'s sections are
  open / draft / merged (#11), so a `closed` row is stored, refreshed, and never
  drawn. Adding a section is a view decision nobody has made.
- **Background polling while the app is in the Dock.** `usePullRequests` polls
  only while the renderer is mounted, at the two cadences `pr-linkage.md` gives
  (15s with checks in flight, 60s otherwise). The research's "app backgrounded"
  and "laptop sleep" rows would need the poll to move into the host beside #21's
  keep-alive, which is where it belongs the day the Inbox badge has to be right
  without anyone looking at the PR tab.
- **ETag / conditional requests and rate-limit headers.** GraphQL has no ETag
  story and `gh` owns the response headers, so respecting
  `x-ratelimit-remaining` would mean reading them back out of `gh`. The cap that
  *is* implemented is the batch: 25 PRs per document, least-recently-polled
  first, so a long board still refreshes every row eventually rather than the
  same 25 for ever.

---

## D-021 — #22: the Inbox on real data, and the revoke that only cut half the wire

**Plan:** #22 is "Inbox view on real data", blocked by #11 and #15. Decision #5
settles what that data *is*: the thread overlay plus a `runs` table, and "Inbox
is a projection of run events".

**Why this is an entry rather than a slice built in order:** it was not built in
order. #26, #27, #28 and #29 were all closed on top of it — D-018, D-019 and
D-020 each name #22 as the owner of the swap they were leaving undone, and
`src/views/InboxView.tsx` went on rendering `mock-host.ts`'s three fixture cards
the whole time. The consequence was that the entire lifecycle group terminated
in a screen nobody could see: fold a live thread and the shell navigated to an
Inbox showing three fake rows and not the one just folded; a `resurface_and_notify`
card never appeared; the away-log `judgment_call` rows never appeared; clicking
a #27 banner opened the thread with fixtures still behind it. This entry records
what closing it actually took.

### What was built

- **`src/views/inbox.ts`** — `useInbox(client, onThreadChanged)`, the same shape
  as `folders.ts` / `crew.ts` / `pulls.ts`: `null` until the host answers, then
  the host wins. It calls `inbox/list` and `permission/pending`, re-reads itself
  on `inbox/resurface`, `inbox/event`, `permission/ask` and
  `permission/resolved`, and exposes the two verbs a card has — `open` and
  `act`.
- **`App` prefers it over `state.inbox`** whenever it has an answer, and passes
  the host's error and its first-load state to the pane, which now says why it
  is empty instead of looking empty.
- **The card's buttons reach the host.** Open thread is `thread/reopen` — not a
  navigation: reopen is what clears the thread's badge (`resurface.md`), puts an
  archived thread's worktree back (#23), and moves the row out of Still Sleeping
  into the sidebar. It is sent only from the states the transition table allows
  it from, which the card knows because `InboxEventView` carries `threadState`.
  Archive is `thread/archive`. An `ask:` button is `permission/reply`.

**The projection is the phone's, deliberately.** `src/views/inbox.ts` imports
`projectInbox` from `src/mobile/inbox.ts` rather than restating it. That file
lives under `src/mobile/` only because #29 needed it first, and its own module
docs say two devices disagreeing about what needs you would be two products; a
second copy in `src/views/` is exactly the drift it warns about. What the
desktop adds on top is presentation — the journey line, the buttons — which is
where the two clients are allowed to differ.

**An outstanding permission is a card here too.** The desktop draws an ask
inline in the transcript (#20), which is the right place when you are reading
the thread and no place at all when you are not. Folding in
`permission/pending` is what makes the Inbox answerable, and it collapses with
the thread's own `needs_you` card because two rows for one question is how a
human answers twice. D-013 left "the Inbox's read/dismiss rules" here; the rule
this took is the one already implemented in the host — **opening the thread is
what marks it read** (`thread_reopen` → `mark_inbox_read`) — so no new method
was invented for it.

### The badge: one definition, not two

The sidebar badge was `needsYouCount(state)` — the renderer's own
`NEEDS_YOU_KINDS` filter over the fixture array — while the phone has always
drawn `InboxListResult.unread`, the host's `count_unread_inbox`. Those are two
different numbers over two different sources, and `resurface.md` only specifies
one of them: resurfaced-and-unread, which counts a `done` card too, because work
that came back while you were away and has not been looked at is what the badge
is for. The desktop now draws `unread`; `NEEDS_YOU_KINDS` keeps the job it is
actually right for, which is the "Needs you" tab and the phone's sections. The
fixture count survives only as the fallback for a shell with no host answer.

### What #22 did **not** build

- **A dismiss that is not an archive.** `inbox_events.dismissed_at` exists and
  `resurface_and_notify` writes it when a thread comes back again, but there is
  no `inbox/dismiss` method and this did not add one. A card the user is done
  with is a *thread* they are done with, and `thread/archive` says that in a way
  the host already acts on. A per-card dismiss that left the thread resurfaced
  would be a third state of "handled" with nothing reading it.
- **A face on a card.** `inbox/list` does not say which bot a card came from —
  the row is about a thread, and a folded thread is in no list that carries a
  colour — so every host card draws the code-session avatar. Inventing a bot for
  it would be worse than the honest icon.
- **Marking a card read without opening it.** Same reason as dismiss: the host's
  only read rule is `thread/reopen`, and the away-log entries the host writes on
  its own behalf are already stamped read when they are written.
- **A Delete button on a card.** Delete is on the row's context menu, where the
  thread is. Two places to destroy a thread from is one more than the number of
  places a user expects to find it.

### The revoke that only cut half the wire (a host fix, not a renderer one)

Found while auditing this work and fixed here because it is one line of the same
sentence D-016 §6 wrote: *"`Clients::broadcast` now asks the session whether that
connection has a device before each frame, so the answer follows a revoke rather
than being latched at accept."* It did not follow a revoke. The question it asked
was `connection_devices.contains_key(connection)` — a RAM map written by `hello`
and cleared only by `drop_connection` — and `device_revoke` touched neither. A
paired `approver` revoked **while its socket was open** kept receiving every
broadcast frame: `session/update` (the prompt text, the agent's replies, the
command in each tool call), `permission/ask`, `inbox/resurface`, for as long as
it cared to hold the connection. The case revoke exists for is a stolen or
hostile phone, which is precisely the one that will not hang up.

Both halves are now real: `device_revoke` drops the device's bindings, and
`connection_has_device` re-reads `paired_devices` on every frame the way
`connected_grant` re-reads it on every request, so a revoked row stops the
stream at the next frame rather than at the next reconnect. It fails closed on a
store it cannot read. `tests/e2e/mobile-inbox.test.ts` revokes a phone mid-
connection and asserts it hears nothing of the next prompt while the desktop
hears all of it; `host::pairing::tests` asks the transport's own question either
side of the revoke. Both fail on the old gate.

### How it is proved

- `src/__tests__/inbox-host.test.tsx` — the host's cards replace the fixtures,
  the badge is the host's `unread` and not a second tally of the same rows,
  Open thread sends `thread/reopen`, Archive sends `thread/archive`, an
  `inbox/resurface` notification re-reads the list with nobody looking at it, an
  ask is answered with the agent's own option id, and a host that will not
  answer says so instead of showing an empty pane. Six of the seven fail on the
  fixture shell.
- `src/__tests__/fold.test.tsx` gains the other two items on #26's first
  checklist line: Archive and Delete from the row's menu were still being
  dispatched to the mock reducer for host-owned threads, so a real thread was
  animated away while its adapter kept running, its permissions stayed
  outstanding, its run stayed open and its #23 worktree was orphaned — and the
  row came back on the next `folder/list`. They now branch on `hostThreads` the
  way `foldThread` always did, and a refusal puts the row back and says why.

---

## D-020 — findings the audit under-rated, fixed anyway

The final audit graded three findings "minor" that are worse than that label.
Fixed with regression tests rather than filed.

**1. `gh api` would read a file for an agent (`host/pr/github.rs`).** Values
reaching `-F name=value` came from a repo's `origin` and from tool output an
agent can influence, and were interpolated unchecked. The sharp edge is not
the shell — `exec::run` takes argv, so there is no shell — it is `gh`'s own
syntax: `-F name=@path` makes gh **read that file and send it**. An agent that
could put `@~/.ssh/id_ed25519` into a value it controls would exfiltrate that
file through a request JaBot makes with the user's own token. Values are now
rejected if they lead with `@` or `-`, or contain anything outside the
owner/name/number/branch character set, and `--hostname` must look like a
hostname. Three tests cover it, including one asserting `fetch` refuses
*before* it spawns anything.

**2. Migration 0010 silently dropped an index.** Rebuilding `inbox_events`
takes its indexes with it. 0010 recreated `inbox_events_thread` and forgot
`inbox_events_unread`, which 0002 created for the unread-badge query that runs
on every projection. Nothing fails — the badge just goes to a full scan, which
is exactly the kind of regression that never gets noticed until the table is
large. Recreated in 0010.

**3. `permission/reply` accepted an option the agent never offered.** Any
`optionId` was forwarded verbatim. Now checked against the options recorded
for that request.

The audit is also worth recording for what it caught at the top: **#22 was
reported complete and was not built.** `src/App.tsx` still rendered
`mock-host` fixtures, so every Inbox card the lifecycle group wrote was
invisible in the app, and the desktop badge counted fixture rows through the
renderer's own classification while the phone counted the host's — two numbers,
two sources, same host. An implementer's report is not evidence; reading the
code is. That is the argument for auditing against the issue text rather than
against the summaries.
## D-024 — `list_crew_status` keeps "idle = no threads"; a refinement was reverted

A wave-6 fix agent began changing `idle` in Chief's `list_crew_status` from
"this bot has no threads" to "this bot has no *busy* threads", threading an
`is_busy(run, acp_state)` through the roster and adding a per-thread `busy`
field. It is a defensible reading — a bot whose only thread is sitting idle is
arguably not working — but it was left unfinished: it broke two `chief.test.ts`
cases (`names the crew when asked for a bot nobody has` expects Chief to read
as working while it is mid-tool-call, and `spawn_code_session ... its own
worktree` fails downstream of the same change), and the agent did not
reconcile them.

Reverted to the committed behaviour, which is green: at HEAD all seven Chief
cases pass. The refinement is recorded here rather than half-applied, because
a partially-changed definition of "working" is worse than either definition
consistently applied — Chief routes work by this field.

If it is picked up later, the question to settle first is what `busy` should
say about the bot that is *asking*: Chief calls `list_crew_status` from inside
its own turn, so under a strict run-state reading Chief reports itself idle
while demonstrably working.
