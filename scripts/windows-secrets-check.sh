#!/usr/bin/env bash
#
# Host-level check for the OS secrets vault (#283).
#
#   ./scripts/windows-secrets-check.sh
#
# Always runs the portable `secrets::` unit tests (backend names, fail-closed
# Linux path, explicit denied-access mapping). Those are already in
# `./scripts/verify.sh`'s cargo-test stage.
#
# On Windows (and macOS) the same filter also runs
# `os_secret_round_trip_put_get_delete` against Credential Manager / Keychain.
# Linux CI cannot talk to either store; this script is the named acceptance
# step a Windows runner (#286) should invoke so the backend does not rot.
#
# Not a default verify.sh stage: the portable tests already run there, and
# a Credential Manager round-trip needs Windows.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || fail "cargo not found"

MANIFEST=(--manifest-path src-tauri/Cargo.toml --locked --lib)
# Match verify.sh: --locked so a stale Cargo.lock cannot hide here.

printf 'windows-secrets-check: portable secrets:: tests'
if [[ "$(uname -s)" == "Darwin" ]]; then
  printf ' + macOS Keychain round-trip\n'
elif [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* || "${OS:-}" == "Windows_NT" ]]; then
  printf ' + Windows Credential Manager round-trip\n'
else
  printf ' (no OS store on this host; Linux fail-closed is the coverage)\n'
fi

cargo test "${MANIFEST[@]}" secrets:: -- --nocapture
status=$?
if [[ $status -ne 0 ]]; then
  fail "secrets vault tests failed (exit $status)"
fi

printf 'windows-secrets-check: ok\n'
