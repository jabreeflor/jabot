# macOS packaged-app acceptance

The native boundary around `JaBot.app`. Browser Playwright (#231–#234) covers
the renderer plus a real `jabot-hostd` over the Vite transport. It does **not**
replace this gate: it never talks Tauri IPC, never delivers a
`UNUserNotificationCenter` banner, never touches Keychain, never hides to the
Dock, and never resolves adapters out of `Contents/Resources`.

**Do not label a Playwright WebKit run as Tauri or WKWebView acceptance.**
Playwright WebKit is not Tauri acceptance and is not WKWebView acceptance.
WebKit-the-browser-engine is renderer compatibility. WKWebView inside
`JaBot.app` is a different process, a different transport, and a different
permission domain.

Cell ids used by the gate: `tauri-ipc`, `synthetic-turn`, `close-to-dock`,
`quit-relaunch`, notify, keychain, packaged adapter, updater.

Windows is a **separate, smaller** smoke list
([docs/windows-acceptance.md](windows-acceptance.md), #287 / tracking #280).
It is not this gate and does not claim macOS parity.

Related: D-019 ([#73](https://github.com/jabreeflor/jabot/issues/73)) — a Linux
box can type-check `notify/mac.rs` and prove the decision layer. It cannot
honestly claim a banner was delivered or clicked. The macOS CI cost tradeoff
in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) still holds:
`macos-latest` bills at **10x**, and the old per-PR `bundle` job was ~86% of
spend while catching almost nothing `verify.sh` did not.

---

## Four stages (do not collapse them)

| Stage | What it proves | What it does not |
|---|---|---|
| **compiled** | The crate and renderer typecheck and test on Linux (`./scripts/verify.sh`) | A `.app` exists, launches, or talks IPC |
| **packaged** | `tauri build` produced `JaBot.app` with our bundle id, version, and staged adapter | The app was launched. Updater *archives* are a sibling check, not this one |
| **launched** | The packaged binary started against temp data, the webview invoked `host_rpc`, fake-acp completed a turn, Keychain used an isolated service | Dock, notification permission UI, a signed update *install* |
| **interactively verified** | A logged-in Mac session: close-to-Dock/reopen, notification prompt, click-to-thread | Anything a headless `macos-latest` runner can claim just by compiling |

A green `bundle` job is **packaged**, not launched, not interactively
verified. Assuming GitHub's headless build proves interaction is how D-019's
gap gets relabelled as coverage.

---

## Matrix

| Cell | Automated? | Blocks | Evidence |
|---|---|---|---|
| Launch / connect through **Tauri IPC** | `run` on a Mac | packaged-acceptance | `ipc-connected.json` from `host_rpc` (webview `invoke` only) |
| Synthetic agent turn (`fake-acp-agent`) | `run` | packaged-acceptance | `synthetic-turn.json` containing `hello from fake-acp` |
| Close-to-Dock / reopen | `run` when not `CI=true` | release (manual if skipped) | `dock-reopen.json` + screenshot; process still running after close |
| Quit / relaunch durability | `run` | packaged-acceptance | `durable-reloaded.json` — same thread id in the temp SQLite |
| Notification permission denied / granted | **manual** | release | Screenshot of the prompt; denied run still shows Inbox (D-019) |
| Notification click-to-thread | **manual** | release | Screenshot: banner click un-hides and opens that thread |
| Isolated Keychain | `run` | packaged-acceptance | `keychain.json` service `com.jabot.app.acceptance.*`; production service refused |
| Packaged adapter resolution | `package` + `run` | packaged-acceptance | `Contents/Resources/.../claude-agent-acp/dist/index.js` |
| Updater metadata / archive presence | `updater-artifacts` on the release output | release CI | `.app.tar.gz` + `.sig`. **Not** a signed update installation |
| Signed update installation | **manual**, controlled fixture | publish | `latest.json` + updater log + before/after version |

Owner of every manual cell: the human cutting the tag. Evidence is required,
not optional — a missing screenshot is a failed cell, same as a missing JSON
file on the automated path.

Commands:

```sh
./scripts/macos-acceptance.sh check                 # Linux, part of verify.sh
./scripts/macos-acceptance.sh matrix
./scripts/macos-acceptance.sh package --app PATH
./scripts/macos-acceptance.sh updater-artifacts DIR
./scripts/macos-acceptance.sh run --app PATH        # macOS, isolated temp data
./scripts/macos-acceptance.sh release-checklist
```

`run` always creates a throwaway data dir and a Keychain service under
`com.jabot.app.acceptance.`. It refuses `~/Library/Application Support/com.jabot.app`
and `JABOT_KEYCHAIN_SERVICE=com.jabot.app`. It never reads user credentials or
production app data.

---

## What is automated where

### Always (Linux, `./scripts/verify.sh`)

`macos-acceptance` stage: docs still name every cell, workflows still point at
the script, isolation still refuses production paths, `package` / `updater-artifacts`
still fail when the thing they cover is missing. Offline, no display, no Mac.
`scripts/tests/macos-acceptance.test.sh` is the behaviour.

### Native-sensitive pull requests (Linux, targeted)

[`.github/workflows/macos-native.yml`](../.github/workflows/macos-native.yml)
`native-check` job, **path-filtered**, `ubuntu-latest`:

- `./scripts/verify.sh --check-mac` — the only compile of `notify/mac.rs`
  (D-019's scratch crate; needs network + `x86_64-apple-darwin` std)
- `./scripts/macos-acceptance.sh check`

This is the cheap, honest PR trigger. It is **not** a Mac and does not launch
the app. Paths: `src-tauri/src/notify/`, `src-tauri/src/lib.rs`,
`src-tauri/src/acceptance.rs`, secrets, bundled adapters, `tauri.conf.json`,
entitlements, the acceptance script/docs, and the workflows themselves.

### Packaged acceptance (macOS, not per-PR by default)

Same workflow, `macos-packaged` job, `macos-latest`, ~2–4 minutes wall /
**~20–40 billable minutes** at 10x:

- `npm run tauri build` (unsigned, `createUpdaterArtifacts: false`)
- `./scripts/macos-acceptance.sh package`
- `./scripts/macos-acceptance.sh run --no-interactive`

Triggers: `workflow_dispatch`, `v*` tags, and pull requests labelled
`macos-acceptance`. Not every native-sensitive PR — that would rebuild the
cost problem D-019 and `ci.yml` already recorded.

The existing `bundle` job on `main` (not PRs) also runs `package` after the
build it already pays for. Headless `macos-latest` has no logged-in GUI
session: Dock and notification clicks are skipped (`CI=true`) and remain
release-manual.

### Release

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs
`updater-artifacts` against the universal bundle output after
`createUpdaterArtifacts` is merged in. That is archive + signature presence.
Installing a signed update into a real `JaBot.app` is the checklist item, on
a controlled fixture, never against a contributor's production install.

---

## Runner / display / signing context

| Surface | Runner | Display | Signing | What you may claim |
|---|---|---|---|---|
| `verify.sh` | Linux laptop / `ubuntu-latest` | none | none | compiled + script integrity |
| `native-check` | `ubuntu-latest` | none | none | `mac.rs` still lints; matrix still named |
| `bundle` / `macos-packaged` | `macos-latest` | none / limited | unsigned | packaged; launched-if-the-window-loads; not Dock/notify UI |
| Interactive `run` | logged-in Mac | Aqua session | whatever the .app has | launched + Dock if Accessibility works |
| Release tag | `macos-latest` + Developer ID secrets | none | signed + notarized | packaged + updater archives; not click-to-thread |
| Human + signed .dmg | the release owner's Mac | yes | notarized | interactively verified |

`UNUserNotificationCenter` aborts when the process has no bundle identifier
(D-019). `tauri dev` is unbundled and silent. Acceptance launches a real
`.app` or it is not this gate.

---

## Isolation contract

| Variable | Production | Acceptance |
|---|---|---|
| App data | `~/Library/Application Support/com.jabot.app` | `JABOT_APP_DATA_DIR` under `/tmp/jabot-acceptance-*` |
| Keychain service | `com.jabot.app` | `JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.<id>` plus a throwaway keychain |
| Agent | user's Claude / Codex / … | `fake-acp-agent` (`dev-bins`) |
| Credentials | never | never |

The process exits 2 rather than fall through to production data if isolation
is requested and fails. That is the only safe default.

---

## In-process probe (not a listener)

`src-tauri/src/acceptance.rs` writes the evidence files when
`JABOT_ACCEPTANCE_DIR` is set. It does **not** bind a Unix socket inside
`JaBot.app` — decision #4 keeps the shipping host behind Tauri IPC;
`jabot-hostd --listen` stays the test binary.

- `ipc-connected.json` is written from `host_rpc`, which only the webview
  `invoke` path calls.
- The synthetic turn talks to the same `HostSession` the webview uses, with
  `fake-acp-agent`, after isolation has been checked.
