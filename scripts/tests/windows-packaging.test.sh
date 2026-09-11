#!/usr/bin/env bash
#
# Tests for scripts/windows-packaging.sh — the NSIS packaging gate (#281).
#
# The script is what stops a release.yml edit from dropping the Windows
# installer, or from letting that job write latest.json. The refusals get
# tests. They run on Linux against a fake bundle tree; `check` is real.
#
#   ./scripts/tests/windows-packaging.test.sh
#   ./scripts/tests/windows-packaging.test.sh artifacts
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
SCRIPT="$REPO_ROOT/scripts/windows-packaging.sh"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-windows-packaging.XXXXXX") || exit 1
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

pass() { printf '  \033[32mok\033[0m   %s\n' "$CASE"; }
fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$CASE"
  printf '       %s\n' "$@"
  FAILURES=$((FAILURES + 1))
}

run_case() {
  CASE=$1
  COUNT=$((COUNT + 1))
  if [[ -n "$FILTER" && "$CASE" != *"$FILTER"* ]]; then
    return 0
  fi
  shift
  if "$@"; then
    pass
  fi
}

check_passes() {
  local out
  out=$(cd "$REPO_ROOT" && "$SCRIPT" check 2>&1) || { fail "check failed: $out"; return 1; }
  case "$out" in
    *PASS*) ;;
    *) fail "check did not print PASS: $out"; return 1 ;;
  esac
  case "$out" in
    *nsis*) ;;
    *) fail "check did not mention nsis: $out"; return 1 ;;
  esac
}

artifacts_need_setup_exe() {
  local dir="$SANDBOX/empty"
  mkdir -p "$dir"
  local out rc=0
  out=$("$SCRIPT" artifacts "$dir" 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "artifacts passed an empty dir"; return 1; }
  case "$out" in
    *setup.exe*) ;;
    *) fail "empty-dir refusal did not name setup.exe: $out"; return 1 ;;
  esac
}

artifacts_accepts_setup_exe() {
  local dir="$SANDBOX/nsis"
  mkdir -p "$dir"
  printf 'fake' >"$dir/JaBot_0.1.0_x64-setup.exe"
  local out
  out=$("$SCRIPT" artifacts "$dir" 2>&1) || { fail "artifacts failed with a setup exe: $out"; return 1; }
  case "$out" in
    *PASS*) ;;
    *) fail "artifacts did not PASS: $out"; return 1 ;;
  esac
  case "$out" in
    *unsigned*|*Authenticode*|*SmartScreen*) ;;
    *) fail "did not disclaim signing: $out"; return 1 ;;
  esac
}

artifacts_refuse_empty_file() {
  local dir="$SANDBOX/empty-exe"
  mkdir -p "$dir"
  : >"$dir/JaBot_0.1.0_x64-setup.exe"
  local out rc=0
  out=$("$SCRIPT" artifacts "$dir" 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "artifacts accepted an empty setup exe"; return 1; }
}

unknown_command_fails() {
  local rc=0
  "$SCRIPT" not-a-command >/dev/null 2>&1 || rc=$?
  [[ $rc -ne 0 ]] || { fail "unknown command succeeded"; return 1; }
}

printf 'windows-packaging tests\n'
run_case "check passes on this tree" check_passes
run_case "artifacts requires a setup exe" artifacts_need_setup_exe
run_case "artifacts accepts a fake NSIS setup exe" artifacts_accepts_setup_exe
run_case "artifacts refuses an empty setup exe" artifacts_refuse_empty_file
run_case "unknown command fails" unknown_command_fails

printf '\n'
if [[ $FAILURES -eq 0 ]]; then
  printf '\033[32m%d passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d failed / %d run\033[0m\n' "$FAILURES" "$COUNT"
exit 1
