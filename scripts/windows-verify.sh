#!/usr/bin/env bash
#
# Windows-safe compile + portable Rust tests for the Tauri/host crate.
#
#   ./scripts/windows-verify.sh
#
# WHY THIS EXISTS
#
# Linux `./scripts/verify.sh` is the product gate. It compiles straight past
# `#[cfg(windows)]` the same way it compiles past `#[cfg(macos)]`. The macOS
# port has scoped PR lint; the Windows port (#280 / #286) needs a real
# windows-latest job so those branches cannot rot. This script is that job:
# the shipping crate (`cargo check` without `dev-bins`), Clippy with warnings
# as errors, and `cargo test` of the portable host suite.
#
# WHAT THIS IS NOT
#
# Not the full verify.sh gate (frontend, e2e, Playwright, macOS acceptance).
# Those stay on Ubuntu. Not a packaged installer — that is the opt-in
# `windows-package` job. Not a human smoke on a real PC. See docs/windows-ci.md.
#
# Not for the default `./scripts/verify.sh` path: that gate stays offline and
# platform-independent. CI's `windows verify` job is the PR check. Linux can
# still run this script; it will just redo a subset of the default gate on
# the host triple it already compiled.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || fail "cargo not found"

MANIFEST=(--manifest-path src-tauri/Cargo.toml)
LOCKED=(--locked)
DEV_BINS=(--features dev-bins)

# tauri-build walks bundle.resources and fails the build script when a glob
# matches nothing. The real adapters tree is a verify/bundle concern and
# needs npm; a stub file is enough for cargo to compile the crate. Skip if
# someone already staged the real tree. Avoid `find` — on Windows Git Bash
# PATH can resolve the system find.exe first.
ADAPTERS=src-tauri/vendor/adapters/node_modules
needs_stub=1
if [[ -d "$ADAPTERS" ]]; then
  for entry in "$ADAPTERS"/* "$ADAPTERS"/.[!.]*; do
    [[ -e "$entry" ]] || continue
    needs_stub=0
    break
  done
fi
if [[ $needs_stub -eq 1 ]]; then
  mkdir -p "$ADAPTERS/.windows-verify-stub"
  printf 'stub for cargo resource glob\n' > "$ADAPTERS/.windows-verify-stub/keep"
  printf 'staged adapter stub so tauri-build can resolve bundle.resources\n'
fi

printf 'windows verify on %s (%s)\n' "$(uname -s 2>/dev/null || echo unknown)" "$(rustc --version 2>/dev/null || echo rustc)"

printf '\n\033[1m> default-features check\033[0m\n'
cargo check "${MANIFEST[@]}" "${LOCKED[@]}" ||
  fail "src-tauri does not compile without dev-bins (the tauri build configuration)"
printf '\033[32mPASS default-features check\033[0m\n'

printf '\n\033[1m> rust clippy\033[0m\n'
cargo clippy "${MANIFEST[@]}" "${LOCKED[@]}" "${DEV_BINS[@]}" --all-targets -- -D warnings ||
  fail "src-tauri is not clippy-clean on this target (-D warnings)"
printf '\033[32mPASS rust clippy\033[0m\n'

printf '\n\033[1m> rust tests\033[0m\n'
cargo test "${MANIFEST[@]}" "${LOCKED[@]}" "${DEV_BINS[@]}" ||
  fail "portable rust tests failed on this target"
printf '\033[32mPASS rust tests\033[0m\n'

# Named #283 check: portable secrets:: tests everywhere; live Credential
# Manager / Keychain round-trip on Windows and macOS. cargo test above
# already includes the same filter; this is the entry #292 documented so
# the backend cannot rot without a named red.
printf '\n\033[1m> windows secrets check\033[0m\n'
./scripts/windows-secrets-check.sh ||
  fail "windows-secrets-check.sh failed"
printf '\033[32mPASS windows secrets check\033[0m\n'

printf '\n\033[32m=== windows verify passed ===\033[0m\n'
