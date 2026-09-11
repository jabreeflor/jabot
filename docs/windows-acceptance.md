# Windows smoke acceptance

A **smaller** checklist than [macos-acceptance.md](macos-acceptance.md).
macOS has a four-stage packaged-app gate (compiled / packaged / launched /
interactively verified) because Dock, Keychain, `UNUserNotificationCenter`,
and updater archives are load-bearing on the shipping `.app`.

Windows is not that product yet. [#287](https://github.com/jabreeflor/jabot/issues/287)
asks for five smoke cells, not a Dock/notify/updater matrix. This page names
them. It does **not** claim macOS parity and it does **not** replace the
macOS gate.

Parent: [#280](https://github.com/jabreeflor/jabot/issues/280).
Install and the gap list: [windows.md](windows.md).
macOS signing runbook: [packaging.md](packaging.md).

**Do not label a Playwright run, a `live.sh` session, or a Linux
`verify.sh` as Windows desktop acceptance.** Those prove the renderer and
`jabot-hostd`. They never talk Tauri IPC inside a WebView2 window, never
touch Credential Manager, and never prove a process tree died with the
`.exe`.

---

## Five cells (do not grow them quietly)

| Cell | What it proves | Blocked by today | Evidence |
|---|---|---|---|
| **launch** | Native window starts; renderer reaches the host through Tauri IPC (`host_rpc` / `host/hello`) | Packaged NSIS `.exe`: [#281](https://github.com/jabreeflor/jabot/issues/281) / [PR #291](https://github.com/jabreeflor/jabot/pull/291). Local: `tauri dev` prerequisites in [windows.md](windows.md) | Screenshot of the window past "Connecting to host…"; pid of `JaBot.exe` |
| **create/open bot chat** | New or existing bot thread opens and can take a message | UI is shared; WebView2 must actually paint it | Screenshot of the standing thread / composer |
| **secret round-trip** | `put` then `get` of a throwaway secret via the **OS** store | [#283](https://github.com/jabreeflor/jabot/issues/283). Today `Secrets::Unavailable` | Credential Manager item under an isolated service, not `JABOT_SECRETS_BACKEND=memory` |
| **adapter spawn** | An ACP child starts for that thread (`fake-acp-agent` is enough) | Spawn flags exist; tree-kill is [#285](https://github.com/jabreeflor/jabot/issues/285) | Adapter stderr log or a live child pid |
| **quit, no orphans** | File → Exit / closing the last window leaves no `JaBot.exe` and no adapter / `node` grandchild | [#285](https://github.com/jabreeflor/jabot/issues/285). Close already quits the host; grandchildren may leak | `tasklist` / Process Explorer empty of those pids |

Owner of every cell: a human on a Windows 10/11 x64 box. Linux CI can only
prove this document and the script still *name* the cells
(`./scripts/windows-acceptance.sh check`).

`fake-acp-agent` (`dev-bins`) is the agent for spawn / chat. Do not use a
contributor's Claude or Codex credentials.

---

## Isolation

Same contract as the macOS probe: never the user's production data.

| Variable | Production (Windows) | Acceptance |
|---|---|---|
| App data | `%APPDATA%\com.jabot.app` (Tauri `app_data_dir()` for `com.jabot.app`) | `JABOT_APP_DATA_DIR` under `%TEMP%\jabot-acceptance-*` |
| Secret service | `com.jabot.app` | `JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.<id>` (name stays even when the backend is Credential Manager, #283) |
| Agent | user's Claude / Codex / … | `fake-acp-agent` |
| Credentials | never | never |

`JABOT_SECRETS_BACKEND=memory` is **not** a pass for the secret cell. That
backend dies with the process and exists so Linux CI can exercise OAuth
without a Keychain. A memory vault is not Credential Manager.

The script refuses a production `%APPDATA%\com.jabot.app` path and the
production service name. Exit 2, same idea as macOS: do not fall through
to the user's store.

---

## What is automated where

| Stage | Where | What it proves |
|---|---|---|
| **named** | Linux, `./scripts/verify.sh` → `windows-acceptance.sh check` | Docs still list the five cells and the gap list; README / packaging still point here; no "macOS parity" claim; isolation still refuses production paths |
| **run** | A Windows box, after #281 produces an installer (or `tauri dev` for a local smoke) | The five cells above, by hand, with screenshots |
| **not this** | Playwright, `live.sh`, macOS `macos-acceptance.sh run` | Other gates. Do not retitle them |

There is no Windows packaged-acceptance CI job yet. [#286](https://github.com/jabreeflor/jabot/issues/286)
is compile/verify so the port does not rot — that is not this checklist,
and a green Windows `cargo check` is not a launched `.exe`.

---

## Commands

```sh
./scripts/windows-acceptance.sh check       # Linux, part of verify.sh
./scripts/windows-acceptance.sh matrix
./scripts/windows-acceptance.sh checklist
```

`run` is reserved for a future Windows launch helper. Today it **refuses**
on every OS and tells you to walk the checklist by hand: there is no
packaged `JaBot.exe` in this tree (#281), and inventing a headless
WebView2 probe on Linux would be the same lie D-019 already recorded for
macOS notifications.

---

## Manual checklist

Printable copy: `./scripts/windows-acceptance.sh checklist`.

- [ ] **launch** — native window, host reachable over Tauri IPC
- [ ] **create/open bot chat** — standing thread visible, composer works
- [ ] **secret round-trip** — isolated Credential Manager item (#283); memory backend does not count
- [ ] **adapter spawn** — `fake-acp-agent` (or a real harness) is a live child
- [ ] **quit, no orphans** — after exit, no `JaBot.exe` / adapter / leftover `node` from that session

Also record, so the gap list stays honest:

- [ ] **glass / vibrancy** — chrome is **opaque** (or #282 has landed and the screenshot shows the new material)
- [ ] Close **quit** the app (no hide-to-Dock / hide-to-tray unless a later issue defines one)
- [ ] SmartScreen warning noted if the build is unsigned
- [ ] No toast, or a toast if #284 has landed — Inbox card still present either way

Playwright is not this list.
