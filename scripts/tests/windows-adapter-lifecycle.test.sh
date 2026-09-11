#!/usr/bin/env bash
#
# Contract tests for scripts/windows-adapter-lifecycle.sh (#285).
# Offline. Does not compile Rust or start Windows.
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
SCRIPT="$REPO_ROOT/scripts/windows-adapter-lifecycle.sh"
FAILURES=0
COUNT=0
CASE=""

pass() { printf '  \033[32mok\033[0m   %s\n' "$CASE"; }
fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$CASE"
  printf '       %s\n' "$@"
  FAILURES=$((FAILURES + 1))
}

run_case() {
  CASE=$1
  COUNT=$((COUNT + 1))
  shift
  if "$@"; then
    pass
  fi
}

check_passes() {
  local out
  out=$("$SCRIPT" check 2>&1) || { fail "check failed: $out"; return 1; }
  case "$out" in
    *PASS*) ;;
    *) fail "check did not print PASS: $out"; return 1 ;;
  esac
}

check_names_job_objects() {
  grep -q 'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE' "$REPO_ROOT/src-tauri/src/host/procgroup.rs" \
    || { fail "procgroup.rs lost the Job Object limit"; return 1; }
  grep -q 'windows-latest' "$REPO_ROOT/.github/workflows/windows.yml" \
    || { fail "windows.yml has no windows-latest job"; return 1; }
  grep -q 'windows-adapter-lifecycle' "$REPO_ROOT/.github/workflows/windows.yml" \
    || { fail "windows.yml lost windows-adapter-lifecycle"; return 1; }
}

unknown_command_fails() {
  local out status
  out=$("$SCRIPT" not-a-command 2>&1) && status=0 || status=$?
  [[ "$status" -ne 0 ]] || { fail "unknown command exited 0: $out"; return 1; }
  case "$out" in
    *unknown*) ;;
    *) fail "unknown command did not say so: $out"; return 1 ;;
  esac
}

script_is_executable() {
  [[ -x "$SCRIPT" ]] || { fail "$SCRIPT is not executable"; return 1; }
}

run_rejects_zero_matches() {
  grep -q 'running 0 tests' "$SCRIPT" \
    || { fail "run does not reject libtest 0-match success"; return 1; }
  grep -q 'require_tests_ran' "$SCRIPT" \
    || { fail "run lost require_tests_ran"; return 1; }
}

filters_are_platform_gated() {
  grep -Eq 'on_windows|Windows_NT|MINGW' "$SCRIPT" \
    || { fail "run does not platform-gate unix/windows filters"; return 1; }
  grep -q 'unix_only' "$SCRIPT" \
    || { fail "run lost the unix-only filter list"; return 1; }
  grep -q 'windows_only' "$SCRIPT" \
    || { fail "run lost the windows-only filter list"; return 1; }
}

birth_into_job_is_contracted() {
  grep -q 'CREATE_SUSPENDED' "$REPO_ROOT/src-tauri/src/host/procgroup.rs" \
    || { fail "procgroup.rs lost CREATE_SUSPENDED birth-into-job"; return 1; }
  grep -q 'ResumeThread' "$REPO_ROOT/src-tauri/src/host/procgroup.rs" \
    || { fail "procgroup.rs lost ResumeThread"; return 1; }
  grep -q 'job_assigned' "$REPO_ROOT/src-tauri/src/host/acp/spawn.rs" \
    || { fail "spawn.rs lost the job_assigned assertion"; return 1; }
}

run_case "check_passes"              check_passes
run_case "check_names_job_objects"   check_names_job_objects
run_case "unknown_command_fails"     unknown_command_fails
run_case "script_is_executable"      script_is_executable
run_case "run_rejects_zero_matches"  run_rejects_zero_matches
run_case "filters_are_platform_gated" filters_are_platform_gated
run_case "birth_into_job_is_contracted" birth_into_job_is_contracted

printf '\n%d cases, %d failed\n' "$COUNT" "$FAILURES"
[[ "$FAILURES" -eq 0 ]]
