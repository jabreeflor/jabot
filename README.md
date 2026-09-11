# JaBot

Bot-crew messenger UI prototypes. Wraps coding TUIs (Claude Code, Codex, Pi, GitHub Copilot, Gemini CLI, or bring-your-own harness) in a chat-first interface with a Chief of Staff bot, folding "disappearing" threads, and an Inbox where long-running tasks resurface.

## Install

macOS 13+, Apple Silicon or Intel:

```sh
curl -fsSL https://github.com/jabreeflor/jabot/releases/latest/download/install.sh | bash
```

That downloads the latest signed, notarized release, checks Apple's own
verdict on it (`spctl`) plus our bundle identifier *before* anything is
copied, and puts `JaBot.app` in `/Applications`. Options go after `bash -s --`
— `--version v0.2.0` to pin a release, `--to ~/Applications` to install
somewhere you own, `--force` to quit a running copy, `--dry-run` to see what
it would do. The script is [`scripts/install.sh`](scripts/install.sh); it is
uploaded to each release, so that URL always serves the installer that shipped
with the latest published build.

Prefer doing it by hand? Download the `.dmg` from
[Releases](https://github.com/jabreeflor/jabot/releases/latest) and drag JaBot
to Applications. Either way, installed copies update themselves after that —
the script is a one-time thing.

### Windows

Windows 10/11 **x64**: download `JaBot_*_x64-setup.exe` from the same
[Releases](https://github.com/jabreeflor/jabot/releases/latest) page and run
it. The installer is NSIS, current-user (no admin). WebView2 is required —
Windows 11 already has it; the installer downloads it on Windows 10 if
needed.

The Windows build is **not Authenticode-signed**. SmartScreen will show
"Windows protected your PC"; choose More info → Run anyway. That warning
is expected until a code-signing certificate exists (follow-up on #281 /
#280). Windows auto-update is not shipped yet — install a newer setup.exe
by hand. This is **not** macOS parity. Install steps, `tauri dev`
prerequisites, the gap list, and the smoke checklist:
[docs/windows.md](docs/windows.md)
([#287](https://github.com/jabreeflor/jabot/issues/287) /
[#280](https://github.com/jabreeflor/jabot/issues/280)).

## Desktop app (Tauri 2)

The scaffold (#7) lives at the repo root:

- **Host:** `src-tauri/` — Rust supervisor inside the Tauri binary
- **Renderer:** `src/` — React 19 + TypeScript + Vite

```bash
# Node 26 (Current). `.nvmrc` / `.node-version` match CI.
npm install
npm run tauri dev    # native window (macOS today; Windows: docs/windows.md)
npm run build        # frontend-only build (CI / Linux)
```

macOS MVP: overlay title bar, hide-to-Dock on window close (#4). Windows: a decorated opaque title bar; close exits (no tray) (#282). The renderer talks **JSON-RPC 2.0** to the Rust host (`host_rpc` + `host-rpc` events) — same messages a Unix socket will carry later (#8). Thread overlay, crew, and Inbox live in host-owned **SQLite** (`jabot.sqlite`, WAL); secret bytes stay in the **OS credential store** — macOS Keychain or Windows Credential Manager (#9, #283). The host spawns **one ACP adapter subprocess per live thread** (stdio JSON-RPC, Unix process-group / Windows Job Object kill-tree, stderr logs) (#10, #285).

## Working on it

CI is not the safety net right now — this repo's Actions minutes are spent and
the macOS bundle job no longer runs on pull requests. One command is the gate,
and it runs on your machine:

```bash
# Node 26 (Current) — `.nvmrc` matches CI and release
npm install                              # deps, and installs the git hooks
./scripts/verify.sh                      # the whole gate, ~1.5 min warm
./scripts/checkpoint.sh -m "message"     # verify and commit, atomically
./scripts/live.sh up                     # the real app in a browser, on any OS
```

`git push` re-runs the gate through `.githooks/pre-push` unless you just
verified those exact bytes, and refuses a push whose commits are not the files
that gate can read. **[CONTRIBUTING.md](CONTRIBUTING.md)** has the
whole local workflow: what every gate proves, what to do when each one fails,
and the escape hatches. Native `JaBot.app` launch, Dock, Keychain, and
updater-archive checks are [docs/macos-acceptance.md](docs/macos-acceptance.md)
(#235) — Playwright WebKit is not that gate. macOS-only Rust (`notify/mac.rs`,
Keychain, the updater / hide-to-Dock branches) is linted on the PR by scoped
jobs, not by `verify.sh` and not by a per-PR bundle — see
[`docs/macos-lint.md`](docs/macos-lint.md). Windows toasts (`notify/win.rs`,
#284) are `cfg(windows)` and are not that Linux scratch crate; smoke steps
live in [native-notifications.md](docs/requirements/native-notifications.md).
Windows host compile is a path-filtered `windows-latest` job so
`#[cfg(windows)]` cannot rot
([`docs/windows-ci.md`](docs/windows-ci.md), #286); it is not a copy of
this gate and not a substitute for a human smoke on a real PC.

The renderer-against-real-host suite is Playwright, not the default gate:

```bash
npm run test:browser:smoke    # Chromium, one smoke journey, no credentials
npm run test:browser          # Chromium + WebKit (recovery / workspace)
```

See [`tests/browser/README.md`](tests/browser/README.md).

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

Both pages are now the record of an exploration rather than a preview of one:
#44 landed on neither set. A bot wears a flat colour disc with its initials in
it, and anyone who wants it to be somebody in particular uploads a picture in
the bot editor — the app stopped inventing an identity for a bot and made one
the user can give it. `src/components/avatar/` is the whole of it.

`prototypes/jabot-voice.html` is the dictation exploration for the
voice-mode research ([`docs/research/voice-mode/`](docs/research/voice-mode/findings.md)):
the three-pane setup with a Yes / Not now row on Chief, the composer
mic lighting up, and the Settings toggle. Scripted speech, not a
real microphone — the host would own that.

Other prototypes in `prototypes/` are earlier design directions.
