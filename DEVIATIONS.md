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
