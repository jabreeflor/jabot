# JaBot

Bot-crew messenger UI prototypes. Wraps coding TUIs (Claude Code, Codex, Pi, or bring-your-own harness) in a chat-first interface with a Chief of Staff bot, folding "disappearing" threads, and an Inbox where long-running tasks resurface.

## Desktop app (Tauri 2)

The scaffold (#7) lives at the repo root:

- **Host:** `src-tauri/` — Rust supervisor inside the Tauri binary
- **Renderer:** `src/` — React 19 + TypeScript + Vite

```bash
npm install
npm run tauri dev    # macOS dev (requires Tauri prerequisites)
npm run build        # frontend-only build (CI / Linux)
```

macOS MVP: overlay title bar, hide-to-Dock on window close (#4). The renderer talks **JSON-RPC 2.0** to the Rust host (`host_rpc` + `host-rpc` events) — same messages a Unix socket will carry later (#8). Thread overlay, crew, and Inbox live in host-owned **SQLite** (`jabot.sqlite`, WAL); secret bytes stay in the **OS keychain** (#9). The host spawns **one ACP adapter subprocess per live thread** (stdio JSON-RPC, process-group kill, stderr logs) (#10).

## Working on it

CI is not the safety net right now — this repo's Actions minutes are spent and
the macOS bundle job no longer runs on pull requests. One command is the gate,
and it runs on your machine:

```bash
npm install                              # deps, and installs the git hooks
./scripts/verify.sh                      # the whole gate, ~1.5 min warm
./scripts/checkpoint.sh -m "message"     # verify and commit, atomically
```

`git push` re-runs the gate through `.githooks/pre-push` unless you just
verified those exact bytes, and refuses a push whose commits are not the files
that gate can read. **[CONTRIBUTING.md](CONTRIBUTING.md)** has the
whole local workflow: what every gate proves, what to do when each one fails,
and the escape hatches.

## Prototypes

Open `prototypes/jabot-classic.html` in a browser — the main MVP (chat, Inbox, Pull Requests, thread sessions, New Chat with harness picker, Crew management).

Build plan and settled architecture decisions (#4 host/quit, #5 fold/run/Inbox, #6 every bot is a harness): [`docs/plan.md`](docs/plan.md), [`docs/decisions/issues-4-6.md`](docs/decisions/issues-4-6.md).

`prototypes/jabot-avatars.html` is the avatar exploration for #44 — five
directions for a bot's identity (monogram blob, glyph tile, generative
identicon, refined blob, shape-as-role sigil) shown at every size the shell
asks for, with greyscale and light-theme toggles. It is a phone-sized page on
purpose: open it on a phone and scroll.

`prototypes/jabot-avatars-characters.html` is round two of the same issue, and
the more interesting half. The systems above all pass the tests and none of
them is anybody, so this one starts from the animator's rule instead: black out
a character, and if you cannot tell who it is, it was never a character — which
is the same test as greyscale at 28px. Five crews: a blob with a face that
answers back, a body whose hat is its name, a generated critter kit, hand-drawn
pixel pets, and watchers whose eyes follow the page and look straight at you
when they need something.

Other prototypes in `prototypes/` are earlier design directions.
