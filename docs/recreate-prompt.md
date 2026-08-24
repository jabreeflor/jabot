# Recreate-JaBot prompt

A single prompt to hand a coding agent that has an empty directory, a shell, and
nothing else. It describes what JaBot is, and it tells the agent to build it in
loops rather than in one pass. Copy everything below the rule.

---

## The prompt

You are building **JaBot** from scratch in this empty directory. Read this whole
brief before you touch a file, then work the loops at the bottom. Do not try to
produce the finished app in one pass — the loops are the method, not a
suggestion.

### What JaBot is

A macOS desktop app that wraps coding TUIs — Claude Code, Codex, Pi, or any
bring-your-own harness — in a chat-first messenger. The user talks to a *crew of
bots* instead of managing terminal sessions. The three ideas that make it
different from a terminal multiplexer:

1. **Every bot is a harness session.** There is no thin host-side LLM loop. A
   bot is a name, an avatar, a system prompt, a tool allowlist, and a
   `harness_id`. "Chief of Staff" is a bot like any other; it just has host
   tools for spawning and handing off work to the rest of the crew.
2. **Threads fold instead of ending.** A thread's visibility state is `active →
   folded → resurfaced → archived`. Folding hides a thread; it never stops the
   work. A separate `runs` ledger records what the session is actually doing,
   one row per turn.
3. **The Inbox is a projection, not a place.** Folded work resurfaces as Inbox
   cards when it finishes, needs a permission decision, hits a judgment call,
   gets stuck, or loses its process. The Inbox view is a pure read of an
   `inbox_events` table; the host decides what goes in it.

### Architecture

- **Host:** Rust, inside a Tauri 2 binary. Owns everything stateful: a SQLite
  database (WAL) for threads, runs, crew, transcript, folders, PRs, schedules,
  permissions, and devices; the OS keychain for secret bytes (the DB stores only
  references); one subprocess per live thread.
- **Renderer:** React 19 + TypeScript + Vite. Owns nothing but view state.
- **Between them:** JSON-RPC 2.0. Requests go over a Tauri command, host-pushed
  notifications come back as events — but shape the protocol as if it were a
  Unix socket, because it will be one. No Tauri types in the protocol layer.
- **Harness adapters:** one ACP (Agent Client Protocol) client per live thread,
  speaking stdio JSON-RPC to a spawned CLI. Spawn into its own process group so
  a kill takes the whole tree. Capture stderr to a per-session log.

Method surface, as a target to build toward — not all at once:

```
host/hello  host/health  supervisor/status
folder/register  folder/list  folder/update  folder/forget
thread/open  thread/state  thread/fold  thread/reopen  thread/resume
thread/archive  thread/delete  thread/transcript
session/prompt  session/cancel  session/update
crew/list  crew/create  crew/update  crew/remove  crew/thread
harness/list  harness/doctor
tools/list  tools/connect  tools/disconnect
permission/ask  permission/reply  permission/pending  permission/resolved
inbox/list  inbox/event  inbox/resurface
pr/list  pr/mine  pr/refresh
schedule/list  schedule/create  schedule/update  schedule/remove  schedule/run
github/login  github/status  notify/status
pairing/start  pairing/claim  pairing/confirm  pairing/status  pairing/cancel
device/list  device/revoke
```

### Screens

- **Sidebar:** registered folders, each with its threads; an Inbox row with a
  "needs you" badge; Crew, Pull Requests, Schedules.
- **Thread view:** the ACP transcript rendered as chat — assistant text, tool
  calls with kind and status, permission prompts inline, a queue strip, a Stop
  button, an error line.
- **Chat view:** a bot's one standing conversation. Every bot has exactly one;
  extra tasks append to it or fold away. No per-bot thread list.
- **Inbox:** cards under All / Needs you / Done. Sleeping (still-folded) work
  shows only under All.
- **Crew:** bot cards, a bot editor (name, color, avatar, prompt, harness,
  tools), templates as data.
- **Pull Requests:** PRs opened by sessions, linked back to their thread, with
  status and checks; a red check becomes an Inbox card of kind `pr`.
- **Schedules:** cron jobs that run as a named bot and deliver to the Inbox.
  Each row says which bot, when it is next owed, and what happened last —
  including a fire missed while the machine was asleep.
- **Onboarding:** a first-run takeover that renders instead of the shell and
  opens the host connection while the user reads it.

### Rules that are not negotiable

- The host is authoritative. A `null` from the host means "has not answered
  yet" and the UI keeps its fixtures; an empty list is an answer and the UI
  renders empty.
- Fold is visibility only. Nothing in a fold path may cancel work.
- Secrets never land in SQLite. Keychain, with a reference row.
- Every module gets a header comment saying *why* it exists, not what it does.
- One command verifies the whole product — `./scripts/verify.sh` — and it runs
  offline, needs no display, no GitHub token, and no macOS. Build it early, in
  cheapest-first stages: toolchain and lockfiles, TypeScript typecheck, renderer
  unit tests, `cargo fmt`, `cargo clippy -D warnings`, `cargo check` without dev
  binaries, `cargo test`, an end-to-end suite driving the real Rust host over
  stdio, and finally the Vite build. Wire it to a pre-push hook.

### How to work: loops

**Loop 1 — the slice loop (outer).** Build the app as a sequence of vertical
slices, each one shippable on its own. Do not start a slice until the previous
one is green. In order:

1. Scaffold: Tauri 2 workspace, React/Vite renderer, `verify.sh`, hooks, CI.
2. Host API: the JSON-RPC layer, `host/hello`, `host/health`, typed both sides.
3. Data layer: SQLite with migrations, keychain-backed secrets.
4. Adapter: ACP client, subprocess supervision, a fake ACP agent binary to test
   against.
5. UI shell: sidebar, views, CSS tokens, avatars.
6. Transcript + permission prompts — this is the thinnest end-to-end spine:
   one real session rendered as chat with working permission decisions.
7. Then, each as its own slice: thread state machine and run ledger, folders,
   crew store, Inbox on real data, worktrees, Chief of Staff, schedules,
   notifications, pull requests.

At the top of each slice, write down the three to five behaviors that will prove
it works. At the bottom, check them off out loud. If a slice grows past roughly
a day of work, split it and re-enter this loop.

**Loop 2 — the build loop (inner, per slice).** Repeat until the exit condition
holds: write the test that fails for the right reason → write the smallest code
that passes it → run `./scripts/verify.sh` → read the *first* failure and fix
only that → run again. Exit when verify is green and the slice's behaviors are
checked off. Never run the loop more than twice on the same failure without
changing your explanation of the cause; if the same error survives two fixes,
your model of it is wrong — go read the code that produces it before editing
anything else.

**Loop 3 — the review loop (per slice, before you commit).** Re-read your own
diff as an adversary and ask, each pass: what would a reviewer reject? What did
I leave stubbed and not say? Which comment now describes code that no longer
exists? Fix what you find and re-run Loop 2. Do at most three passes, then
commit — the loop is for catching real defects, not for polishing forever.

**Loop 4 — the failure loop (whenever something breaks).** Reproduce it, name
the cause in one sentence, fix the cause and not the symptom, then add the
check that would have caught it — a test, a verify stage, or a hook. A fix
without that last step is not finished, and the loop does not exit.

Commit at every green point with a message that says what changed and why.
Keep a running log of decisions you made that the brief did not settle, and the
deviations you took from it, so the next reader can tell intent from accident.

Start with Loop 1, slice 1. Announce which loop and which slice you are in
before each stretch of work.
