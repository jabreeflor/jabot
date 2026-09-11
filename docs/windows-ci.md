# Windows CI

How the Windows desktop port (#280) is kept from rotting in CI, what a green
job actually proves, and what still needs a human on a real PC.

This is [#286](https://github.com/jabreeflor/jabot/issues/286). Decorated
window chrome (#282 / #294), Action Center toasts (#284 / #288), NSIS
packaging (#281 / #291), Credential Manager (#283 / #292), and install
docs (#287 / #289) are on `main`; this job compiles those
`#[cfg(windows)]` paths and runs the portable host tests, including the
named secrets check. It does not launch the app. Job-object kill is
still a sibling (#285).

## Why a second gate exists

`./scripts/verify.sh` is the local and Linux CI gate. It is offline,
display-free, and platform-independent on purpose (`CONTRIBUTING.md`). Every
`cargo clippy` it runs therefore compiles straight past `#[cfg(windows)]`
the same way it compiles past `#[cfg(macos)]`.

macOS already has scoped PR lint (`docs/macos-lint.md`). Windows has no
Apple-toolchain excuse and no 10x runner, but `windows-latest` is still **2x**
Linux, and this repo has exhausted Actions minutes once already. So Windows
CI is a lean compile + portable test job, not a copy of `verify.sh`, and not
a per-PR installer.

Linux and macOS jobs stay the sources of truth they are today
(`.github/workflows/ci.yml`, `.github/workflows/macos-native.yml`). A green
Ubuntu `verify` does not mean the Windows crate compiled.

## What CI guarantees

| Check | Runner | Command | When it starts |
| --- | --- | --- | --- |
| `windows plan` | Linux | `scripts/windows-needed.sh` | Every PR, tag, and `workflow_dispatch`. Writes the verify output. |
| `windows verify` | `windows-latest` | `scripts/windows-verify.sh` | Plan says `verify=1` and the event is not `labeled`, or a tag / dispatch (forced). |
| `windows package` | `windows-latest` | `npm run tauri -- build --bundles nsis` | A PR labelled `windows-package`, or dispatch with `package=true`. Tag NSIS is `release.yml` (#281). |

`windows-verify.sh` is the Windows-safe subset:

1. Stub `bundle.resources` if the vendored adapters are not staged (no `npm ci`).
2. `cargo check --locked` without `dev-bins` — the configuration `tauri build` compiles.
3. `cargo clippy --locked --features dev-bins --all-targets -- -D warnings`.
4. `cargo test --locked --features dev-bins` — host unit tests plus the crate integration suites. Unix-only cases (`cfg(unix)` process-group, Unix sockets) stay skipped, which is honest.
5. `scripts/windows-secrets-check.sh` — named #283 check. Portable `secrets::` tests on every host; live Credential Manager round-trip on this runner.

A warning is a red check. There is no `continue-on-error`. If the job is red,
the Windows port is broken; do not relabel that as "Windows is flaky."

The optional package job produces an **unsigned** NSIS installer artifact.
It is not the release path (#281), not MSI/WiX, and not a SmartScreen-blessed
build. `--bundles nsis` overrides `bundle.targets` (`app` / `dmg`) for that
invocation only; the macOS targets in `tauri.conf.json` are unchanged.

## What CI does not guarantee (human smoke)

A green Windows job is **compiled** (and, when opted in, **packaged**). It is
not a launch, not window chrome, and not a signed install. The cells that
still need a person at a real Windows PC — tracked on #280 / #287:

| Still human | Why CI cannot claim it |
| --- | --- |
| Installer UX, Start Menu, SmartScreen | Unsigned NSIS; no Authenticode cert in this cut (#281) |
| Window chrome / close / tray | Headless runner. Decorated opaque chrome + close-exits is on `main` (#282 / #294); CI cannot launch a window. |
| Secrets in Credential Manager | Compiled and unit-tested (#283 / #292). `windows-secrets-check.sh` runs the live `os_secret_round_trip` on this runner; Control Panel visibility is still a desktop smoke. |
| Toast notifications | `notify/win.rs` is compiled (#284 / #288); Action Center delivery is a desktop smoke, not this job. |
| ACP adapter kill tree | `CREATE_NEW_PROCESS_GROUP` is compiled; Job Objects are #285 |
| Playwright visual / axe / WebKit | Out of scope for the first cut; Ubuntu `browser` stays the suite |

Do not call a green `windows verify` "the app works on Windows."

## When the Windows runner starts

Path lists live in [`scripts/windows-needed.sh`](../scripts/windows-needed.sh).
A file that is not listed there is a deliberate exclusion from the paid
Windows runner, not an oversight. The suite at
[`scripts/tests/windows-ci.test.sh`](../scripts/tests/windows-ci.test.sh)
fails if a listed path stops matching, if `continue-on-error` appears, or if
packaging is no longer opt-in.

Turns **verify** on:

- `src-tauri/` (host crate, Tauri config, lock, icons, adapters manifest)
- `rust-toolchain.toml`
- `scripts/windows-verify.sh`, `scripts/windows-needed.sh`, `scripts/windows-secrets-check.sh`, `scripts/tests/windows-ci.test.sh`
- `.github/workflows/windows.yml`

Does **not** turn verify on (Linux `verify` / `browser` already cover them):

- `src/`, Playwright, frontend lint
- `docs/**`, `README.md`, `CONTRIBUTING.md`
- `.github/workflows/ci.yml` and `macos-native.yml`

`workflow_dispatch` always runs verify. Check **Also build an unsigned NSIS
installer** to add the package job without labelling a PR.

The `labeled` trigger exists so adding `windows-package` can start the
package job without a dummy push. Verify does **not** run on label events,
and those runs sit in their own concurrency group, so `size:*` /
`agent-ready` cannot cancel and re-bill an in-flight Windows verify.

`windows-package` is not a pre-created repo label. The first person to apply
it needs write access (GitHub auto-creates it then). After that, anyone who
can label the PR can opt in. See [docs/labels.md](labels.md).

Local `windows-verify.sh` on a PC needs Git for Windows (its `usr/bin` `sh` /
`cat`). Running it from `cmd.exe` without that PATH is a red that is not a
crate bug.

## Local reproduction

Default verify stays offline and does not need Windows.

```bash
# Which verify job would this branch turn on against main?
./scripts/windows-needed.sh --base origin/main

# The contract (planner matches, workflow rules, docs). Part of verify.sh:
./scripts/tests/windows-ci.test.sh

# The same commands CI runs. On Linux this is a subset of verify.sh; on a
# Windows box (Git Bash) it is the port check:
./scripts/windows-verify.sh
```

Packaging, when you have the Tauri Windows prerequisites
([Tauri Windows setup](https://v2.tauri.app/start/prerequisites/#windows)):

```bash
npm ci
npm run bundle:adapters
npm run tauri -- build --bundles nsis
```

## Deliberate exclusions

These are not missing coverage; they are the cost and honesty limits from
#286 and the minutes comment in `.github/workflows/ci.yml`.

- **Default `./scripts/verify.sh`.** Stays offline and Windows-free. The
  contract tests (planner / workflow) run on that path; `windows-verify.sh`
  itself does not.
- **Frontend, TypeScript e2e, Playwright.** Already required on Ubuntu.
  Enabling the visual suite on Windows is out of scope for the first cut
  unless it becomes cheap.
- **Per-PR NSIS/MSI.** Opt-in only. Minutes matter; a cold `tauri build` on
  `windows-latest` is the expensive half of this workflow.
- **Changing `bundle.targets` to include `nsis` / `msi`.** That is #281.
  This workflow passes `--bundles nsis` so the macOS `app`/`dmg` list stays
  the release default.
- **Code signing / SmartScreen / a published Windows release asset.** #281.
- **Runtime QA of chrome, toasts, and kill trees.** Chrome and toasts are compiled (#294 / #288); Job Objects are #285. Credential Manager is compiled and unit-tested here (#292); Control Panel / a launched app is still a human smoke.
