#!/usr/bin/env bash
#
# Clippy the jabot library on a real Mac, warnings as errors.
#
#   ./scripts/check-macos-clippy.sh
#
# WHY THIS EXISTS
#
# `scripts/check-mac-notify.sh` already lints `notify/mac.rs` from Linux
# against x86_64-apple-darwin. That scratch-crate trick does not extend to
# the rest of the crate: `objc2-exception-helper`, pulled in through tauri,
# has a `cc` build script that needs an Apple toolchain (D-019 / #73 / #109).
# The modules that still need a Mac are the Keychain helpers in
# `host/store/secrets.rs` and the updater / hide-to-Dock / Reopen branches
# in `lib.rs`.
#
# WHY `--lib` AND NOT `tauri build`
#
# The macOS `bundle` job is the packaging step and is still post-merge on
# purpose (see .github/workflows/ci.yml). This script is only Clippy. `--lib`
# compiles the production crate — where every remaining `cfg(target_os =
# "macos")` block lives — without the `dev-bins` test binaries or a
# frontend/bundle. CI runs it only when scripts/macos-lint-needed.sh says
# the change list touches those modules, their deps, or this check.
#
# Not for the default `./scripts/verify.sh` path: that gate stays offline
# and platform-independent. CI's `macos clippy` job is the PR check.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "scripts/check-macos-clippy.sh needs macOS.

Linux can lint notify/mac.rs with ./scripts/check-mac-notify.sh
(or ./scripts/verify.sh --check-mac). The rest of the cfg(macos)
crate cannot be cross-checked from Linux; CI's \`macos clippy\`
job is the before-merge gate. See docs/macos-lint.md."

command -v cargo >/dev/null 2>&1 || fail "cargo not found"

# tauri-build reads bundle.resources at build-script time. The adapters
# glob matching nothing fails `cargo clippy --lib` the same way it fails
# `cargo check` — not only `tauri build`. Stage (or confirm) first.
./scripts/bundle-adapters.sh || fail "bundled adapters must be staged before clippy (tauri-build reads bundle.resources)"

printf 'clippy --lib -D warnings on %s (%s)\n' "$(uname -sm)" "$(rustc --version 2>/dev/null || echo rustc)"
cargo clippy \
  --manifest-path src-tauri/Cargo.toml \
  --locked \
  --lib \
  -- -D warnings ||
  fail "src-tauri lib does not build clean for this Mac (clippy -D warnings)"

printf '\033[32mlib is clean for macOS (clippy -D warnings)\033[0m\n'
