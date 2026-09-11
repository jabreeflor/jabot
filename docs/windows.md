# Windows desktop (in progress)

JaBot ships today as a signed, notarized **macOS** app. Windows is the
[#280](https://github.com/jabreeflor/jabot/issues/280) tracking epic — a real
desktop port, not a rewrite of the React UI.

This page is install, `tauri dev` prerequisites, and the **known gaps vs
macOS**. It does **not** claim macOS parity. Child issues (#281–#286) close
the gaps; this page (#287) names them so a Windows run is judged against
what is actually wired.

| Related | Where |
|---|---|
| Tracking epic | [#280](https://github.com/jabreeflor/jabot/issues/280) |
| Smoke checklist | [windows-acceptance.md](windows-acceptance.md) |
| macOS packaged-app gate (not this) | [macos-acceptance.md](macos-acceptance.md) |
| macOS signing / notarization / updater | [packaging.md](packaging.md) |
| Any-OS renderer + host (no native window) | [CONTRIBUTING.md](../CONTRIBUTING.md) (`scripts/live.sh`) |

Sibling Windows PRs should **update the gap table below** rather than open a
second Windows install page.

- [#281](https://github.com/jabreeflor/jabot/pull/291) owns the NSIS overlay
  (`tauri.windows.conf.json`), the `windows` release job, and the packaging
  runbook in [packaging.md](packaging.md). This page is the user/dev install
  + gaps + smoke list that PR points at.
- [#282](https://github.com/jabreeflor/jabot/issues/282)–[#286](https://github.com/jabreeflor/jabot/issues/286)
  close chrome, secrets, toasts, kill-tree, and CI. When they land, flip the
  matching row here instead of restating install.

---

## Supported versions

| | |
|---|---|
| OS | **Windows 10** (1803 / build 17134 or later) and **Windows 11** |
| Arch | **x64** (`x86_64-pc-windows-msvc`) only |
| Webview | Edge **WebView2** (preinstalled on Windows 11 and current Windows 10) |
| Not yet | ARM64 Windows, Windows Server, Windows 10 LTSC without WebView2, 32-bit |

Those floors match [Tauri 2's Windows prerequisites](https://v2.tauri.app/start/prerequisites/).
They are a proposal until a Windows installer actually ships (#281); do not
treat them as a tested compatibility matrix.

---

## Install the artifact

**Primary installer: NSIS `JaBot_*_x64-setup.exe`.** MSI / WiX is not an
MVP target. The macOS `bundle.targets` stay `["app", "dmg"]`; Windows NSIS
lives in `src-tauri/tauri.windows.conf.json` and is merged only on a
Windows build ([#281](https://github.com/jabreeflor/jabot/issues/281) /
[PR #291](https://github.com/jabreeflor/jabot/pull/291)). Until that PR
is on `main`, GitHub Releases will not have a Windows asset.

When the asset exists:

1. Open
   [Releases](https://github.com/jabreeflor/jabot/releases/latest).
2. Download `JaBot_*_x64-setup.exe`. Do **not** invent a `curl | bash`
   Windows installer to match `scripts/install.sh` — that script is
   macOS-only (`hdiutil`, `codesign`, `spctl`).
3. Run it. It is **current-user** (no admin). WebView2 is required:
   Windows 11 already has it; on Windows 10 the installer downloads the
   Evergreen bootstrapper if needed.
4. Launch **JaBot** from the Start menu.

Until #281 is merged and a release is cut, a Windows user cannot install
a release build. A cross-compiled binary from macOS or Linux is not a
supported install. Local production of the same artifact, once the
overlay exists:

```powershell
npm run tauri build -- --target x86_64-pc-windows-msvc --bundles nsis
```

Cross-build via `cargo-xwin` is last-resort only — see
[packaging.md](packaging.md) once #281 lands. Do not add `nsis` to the
base `bundle.targets` array from this docs PR.

### Signing and SmartScreen

macOS releases are Developer ID signed and notarized ([packaging.md](packaging.md)).
Windows has **no Authenticode certificate and no SmartScreen reputation** in
this repo today. An unsigned installer will show
"Windows protected your PC" — **More info → Run anyway** is the documented
dev path, not a bypass we tell users to ignore forever.

Do not call an unsigned `.exe` "safe" or "equivalent to the notarized
`.dmg`". Signing is follow-up on #281 once a cert exists. SmartScreen
reputation accumulates after signed downloads; it is not something a first
release can claim.

---

## Develop (`tauri dev`)

Native `npm run tauri dev` on Windows needs the Tauri 2 Windows toolchain.
The renderer-plus-host loop without a native window is still
`./scripts/live.sh` and works on Windows the same way it does on Linux.

### Prerequisites

1. **Microsoft C++ Build Tools** — [Build Tools for Visual Studio 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   (or a full VS 2022). Workload: **Desktop development with C++**, including
   the **Windows 10/11 SDK**.
2. **WebView2** — skip on Windows 11 and current Windows 10. Otherwise the
   [Evergreen Bootstrapper](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
3. **Rust** via [rustup](https://rustup.rs/), default host
   `x86_64-pc-windows-msvc` (MSVC, not GNU). Same `stable` channel as
   `rust-toolchain.toml`.
4. **Node 26** (Current) — `.nvmrc` / `.node-version` / `engines.node`.
   [nvm-windows](https://github.com/coreybutler/nvm-windows) or the
   official installer.

VBSCRIPT (an optional Windows feature) is only required if you build an
**MSI**. NSIS does not need it. See Tauri's
[Windows installer](https://v2.tauri.app/distribute/windows-installer/) notes.

### First run

In a Developer PowerShell / "x64 Native Tools" prompt so `link.exe` is on
`PATH`:

```powershell
npm install
npm run bundle:adapters
npm run tauri dev
```

`bundle:adapters` stages the Claude ACP adapter into
`src-tauri/vendor/adapters/node_modules` the same way a `tauri build` does.
Without it the Claude card is only ready if `claude-agent-acp` is already
on the machine. See [packaging.md](packaging.md#what-ships-inside-the-bundle-the-claude-acp-adapter).

What you should expect **today**, before the child issues land:

- A WebView2 window, not WKWebView.
- Opaque chrome (no glass / Mica).
- Closing the last window **quits** the process (no hide-to-Dock).
- Secrets `put` fails closed (`SecretsUnavailable`) — not Credential Manager.
- Inbox cards persist; **no** toast banner (`notify` is the unsupported no-op).
- ACP children can start; killing the tree on quit is [#285](https://github.com/jabreeflor/jabot/issues/285), not proven.

`npm run tauri build` on a Windows box produces the NSIS installer only
after #281's `tauri.windows.conf.json` is on the tree. Do not add `nsis`
to the base `bundle.targets` from this docs PR (D-005: macOS updater
archives need `app` + `dmg` there).

---

## Gaps vs macOS

This is the honest list. A green Linux `./scripts/verify.sh` does not close
any row. Closing a row is the linked child issue, plus the matching cell on
[windows-acceptance.md](windows-acceptance.md).

| Surface | macOS today | Windows today | Owner |
|---|---|---|---|
| **Glass / vibrancy** | Under-window material (`Effect::UnderWindowBackground`); renderer paints `data-translucency="on"` only when it applied | **Opaque.** `window.rs` refuses `set_effects` off macOS. Mica/Acrylic exist in Tauri and are **not** applied | [#282](https://github.com/jabreeflor/jabot/issues/282) |
| **Dock hide** | Close of the last window hides; Dock click / Reopen shows it again (#4) | **Close quits.** `lib.rs` only intercepts `CloseRequested` on macOS. No hide-to-tray, no Dock equivalent | [#280](https://github.com/jabreeflor/jabot/issues/280) (behavior), [#282](https://github.com/jabreeflor/jabot/issues/282) (chrome) |
| **Signing / SmartScreen** | Developer ID + notarization + stapled ticket; `install.sh` refuses anything else | **Unsigned.** SmartScreen will warn. No Authenticode secret in this repo | [#281](https://github.com/jabreeflor/jabot/issues/281) (artifact); signing is called out there as follow-up |
| **Notify** | `UNUserNotificationCenter` banners; click-to-thread | **Unsupported no-op** (`notify/unsupported.rs`). Inbox still records the card (persist-then-notify) | [#284](https://github.com/jabreeflor/jabot/issues/284) |
| Secrets | macOS Keychain, service `com.jabot.app` | `Secrets::Unavailable` — `put` fails closed. In-memory backend is tests/CI only, not persistence | [#283](https://github.com/jabreeflor/jabot/issues/283) |
| Adapter kill tree | Process-group SIGTERM then SIGKILL | `CREATE_NEW_PROCESS_GROUP` on spawn; terminate falls through to `Child::kill()` (parent only). Job objects are the planned fix | [#285](https://github.com/jabreeflor/jabot/issues/285) |
| Installer / release | `.dmg` + `install.sh` + updater archives from `release.yml` | NSIS `*-setup.exe` from a sibling `windows` release job — not on `main` until [#281](https://github.com/jabreeflor/jabot/issues/281) / [PR #291](https://github.com/jabreeflor/jabot/pull/291) | [#281](https://github.com/jabreeflor/jabot/issues/281) |
| Auto-update | `tauri-plugin-updater` registered on macOS only | Plugin is not registered; no `windows-*` feed entry | [#281](https://github.com/jabreeflor/jabot/issues/281) (artifacts first) |
| CI so the port does not rot | Linux `verify` + scoped macOS lint / packaged-acceptance | No Windows compile/verify job | [#286](https://github.com/jabreeflor/jabot/issues/286) |

The renderer, SQLite store, and ACP protocol are the same code. The gaps
are the native host: window chrome, OS secret store, process tree, toasts,
and the installer.

---

## What you may claim

| Surface | You may claim |
|---|---|
| This page + `windows-acceptance.sh check` | The install story, the gap list, and the smoke cells are still named |
| `scripts/live.sh` on Windows | Renderer + `jabot-hostd` over the Vite transport — **not** a native `.exe` |
| Playwright | Same as on Linux: not Tauri, not WebView2-in-JaBot |
| `tauri dev` on a Windows box | A native window launched, with the gaps above still true |
| A GitHub Release `.exe` / `.msi` | **Nothing until #281 uploads one** |
| "Works like the Mac app" | **Never**, until the child issues in the table are done |

Playwright WebKit is not Windows acceptance and is not WebView2 acceptance.
Same rule as [macos-acceptance.md](macos-acceptance.md): the browser engine
is not the packaged webview.
