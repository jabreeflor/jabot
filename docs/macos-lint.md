# macOS Rust lint coverage

How `cfg(target_os = "macos")` code is linted before merge, what is
deliberately left to a Mac or to post-merge packaging, and how to reproduce
each check locally.

This follows [#109](https://github.com/jabreeflor/jabot/issues/109) and
decision [#73](https://github.com/jabreeflor/jabot/issues/73) (D-019): do not
recreate the notification scratch-crate, and do not pretend the full Tauri
crate can be cross-checked from Linux. The PR gate is [#228](https://github.com/jabreeflor/jabot/issues/228).

## Why a second gate exists

`./scripts/verify.sh` is the local and Linux CI gate. It is offline,
display-free, and platform-independent on purpose (`CONTRIBUTING.md`). Every
`cargo clippy` it runs therefore compiles straight past `#[cfg(target_os =
"macos")]` modules.

The macOS `bundle` job is a packaging step (`tauri build`, icons, plist,
lipo). It does **not** run on pull requests, and it does not run
`clippy -D warnings`. Treating it as a lint gate would put a 10x-billed
full build on every PR — the spend that already exhausted this repo once.

So cfg(macos) lint is a pair of scoped PR checks, not a new default
`verify.sh` stage and not a per-PR bundle.

## Inventory

| Module | What the macOS-only code does | Before-merge lint | Why that check |
| --- | --- | --- | --- |
| `src-tauri/src/notify/mac.rs` | `UNUserNotificationCenter` delivery | **mac notify cross-check** (Linux) | Existing scratch crate from #109. `objc2*` only; no Apple `cc`. |
| `src-tauri/src/notify/mod.rs` | Decision layer + `cfg` split onto `mac` / `unsupported` | Linux `verify` clippy (portable + unsupported) **and** the notify cross-check (mac backend) | Portable tests already run on Linux; `mod mac` is what the scratch crate compiles. |
| `src-tauri/src/notify/unsupported.rs` | No-op backend | Linux `verify` clippy | Compiled on every non-macOS target. |
| `src-tauri/src/host/store/secrets.rs` (`os_put` / `os_get` / `os_delete`) | macOS Keychain via `keyring` | **macos clippy** (native) | `keyring` / Security.framework are not in the notify scratch crate. |
| `src-tauri/src/lib.rs` (updater `.plugin()`, hide-to-Dock, Dock Reopen) | Tauri + `tauri-plugin-updater` | **macos clippy** (native) | Pulls `tauri`, which pulls `objc2-exception-helper`'s Apple `cc` build script. Linux cross-check of the whole crate fails; that is D-019, not a gap to close. |
| `src-tauri/src/host/harness/path.rs` (`login_shell_path`) | Login-shell `PATH` probe | Linux `verify` clippy | Uses `cfg!(target_os = "macos")` (a boolean), not `#[cfg]`. Both branches type-check on Linux. **Not** a native-job trigger. |

Shared inputs that turn the relevant job(s) on: `src-tauri/Cargo.toml`,
`src-tauri/Cargo.lock`, `src-tauri/clippy.toml`, `rust-toolchain.toml`,
`.github/workflows/ci.yml`, and the check scripts themselves. The live lists
are in [`scripts/macos-lint-needed.sh`](../scripts/macos-lint-needed.sh);
the suite at [`scripts/tests/macos-lint.test.sh`](../scripts/tests/macos-lint.test.sh)
fails if a listed path stops matching.

## PR checks

On `pull_request` and `workflow_dispatch` only (never on the post-merge
bundle path):

| Check name | Runner | Command | When it starts |
| --- | --- | --- | --- |
| `macos lint plan` | Linux | `scripts/macos-lint-needed.sh` | Every PR. Writes the two outputs below. |
| `mac notify cross-check` | Linux + `x86_64-apple-darwin` std | `scripts/check-mac-notify.sh` then the injected-lint proof in `macos-lint.test.sh` | Plan says `notify=1` |
| `macos clippy` | `macos-latest` | `scripts/check-macos-clippy.sh` | Plan says `native=1` |

Both lint jobs use `clippy -- -D warnings`. A warning is a red check.

`macos clippy` is scoped to `cargo clippy --locked --lib` and uses
`Swatinem/rust-cache`. It is not `npm run tauri build`. It does stage the
bundled ACP adapters first (`scripts/bundle-adapters.sh`) because
`tauri-build` reads `bundle.resources` at build-script time and fails when
the glob matches nothing. A frontend-only or docs-only PR does not start a
Mac runner.

## Local reproduction

Default verify stays offline. None of the commands below are implied by
`./scripts/verify.sh` with no flags.

```bash
# Which jobs would this branch turn on against main?
./scripts/macos-lint-needed.sh --base origin/main

# Notify / mac.rs, from Linux or a Mac. Needs network the first time and:
rustup target add x86_64-apple-darwin
./scripts/check-mac-notify.sh
# same stage, via the existing opt-in:
./scripts/verify.sh --check-mac

# Keychain + lib.rs cfg(macos) branches. Needs a Mac, and stages
# bundled adapters (tauri-build reads bundle.resources).
./scripts/check-macos-clippy.sh

# Planner matches, refusals, and (if the Apple target is installed)
# "an injected ptr_arg in mac.rs fails the notify check".
./scripts/tests/macos-lint.test.sh
```

## Deliberate exclusions

These are not missing coverage; they are the cost and honesty limits from
#73 / #109 / the bundle-job comment in `.github/workflows/ci.yml`.

- **Default `./scripts/verify.sh`.** Stays offline and Mac-free. `--check-mac`
  remains opt-in locally. The planner tests (2e) run on that path; the
  cross-check itself does not.
- **Cross-checking the whole Tauri crate from Linux.**
  `objc2-exception-helper` needs an Apple `cc`. Do not add a
  `--target x86_64-apple-darwin` on `src-tauri` to `verify.sh`.
- **Per-PR `tauri build`, signing, notarization, universal lipo.** Still the
  post-merge `bundle` job. The `bundle-config` gate in `verify.sh` is the
  offline packaging check.
- **Runtime notification QA on a signed `JaBot.app`.** Still the four-item
  checklist on #73 (prompt, banner, click routing, replace-not-stack).
  Clippy cannot prove delivery.
- **`path.rs` login-shell probe as a native-job trigger.** Already linted on
  Linux; starting a Mac for it would be spend without new types.
- **`macos clippy` on push-to-main.** The PR already ran it when the paths
  matched. The merge pays for `bundle`, not a second Clippy compile.

## Adding a new `cfg(macos)` module

1. Put it on one of the two lists in `scripts/macos-lint-needed.sh` (notify
   scratch-crate if it only needs the objc2 notification crates; native
   otherwise).
2. Add a planner case in `scripts/tests/macos-lint.test.sh` that would fail
   if the path stopped matching.
3. Update the inventory table above.
4. Prefer extending `check-mac-notify.sh` over a new Mac job when the
   module does not need tauri / keyring / an Apple `cc`.
