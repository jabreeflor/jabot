#!/usr/bin/env bash
#
# Tests for scripts/windows-acceptance.sh — the #287 docs + smoke gate.
#
# The useful refusals (production AppData, production secret service, `run`
# on Linux, a missing gap word) are the ones nobody notices when they stop
# happening. They run on Linux. Launching JaBot.exe is not this suite.
#
#   ./scripts/tests/windows-acceptance.test.sh
#   ./scripts/tests/windows-acceptance.test.sh refuses
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
SCRIPT="$REPO_ROOT/scripts/windows-acceptance.sh"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-windows-acceptance.XXXXXX") || exit 1
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
}

matrix_names_required_cells() {
  local out
  out=$("$SCRIPT" matrix 2>&1) || { fail "matrix failed: $out"; return 1; }
  local cell
  for cell in launch create-open-bot-chat secret-round-trip adapter-spawn quit-no-orphans glass-vibrancy dock-hide signing-smartscreen notify; do
    case "$out" in
      *"$cell"*) ;;
      *) fail "matrix missing $cell"; return 1 ;;
    esac
  done
  case "$out" in
    *$'\n'Playwright$'\t'*|Playwright$'\t'*) fail "matrix labelled a Playwright cell"; return 1 ;;
  esac
}

checklist_names_smoke_and_gaps() {
  local out
  out=$("$SCRIPT" checklist 2>&1) || { fail "checklist failed: $out"; return 1; }
  for word in launch "create/open bot chat" "secret round-trip" "adapter spawn" "quit, no orphans" "glass/vibrancy" "Dock hide" SmartScreen notify "Playwright is not this list" "macOS parity"; do
    case "$out" in
      *"$word"*) ;;
      *) fail "checklist missing [$word]: $out"; return 1 ;;
    esac
  done
}

refuses_run_everywhere() {
  local out rc=0
  out=$("$SCRIPT" run 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "run succeeded"; return 1; }
  case "$out" in
    *#281*|*"not wired"*) ;;
    *) fail "run refusal did not name #281 / not wired: $out"; return 1 ;;
  esac
}

refuses_production_appdata() {
  local out rc=0
  out=$(JABOT_APP_DATA_DIR="$HOME/AppData/Roaming/com.jabot.app" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
    "$SCRIPT" _assert-isolation 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "accepted production AppData path"; return 1; }
  case "$out" in
    *production*) ;;
    *) fail "refusal did not say production: $out"; return 1 ;;
  esac
}

refuses_production_macos_path() {
  local rc=0
  JABOT_APP_DATA_DIR="$HOME/Library/Application Support/com.jabot.app" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
    "$SCRIPT" _assert-isolation >/dev/null 2>&1 || rc=$?
  [[ $rc -ne 0 ]] || { fail "accepted production macOS app-support path"; return 1; }
}

refuses_production_secret_service() {
  local rc=0
  JABOT_APP_DATA_DIR="$SANDBOX/jabot-acceptance-x/data" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app \
    "$SCRIPT" _assert-isolation >/dev/null 2>&1 || rc=$?
  [[ $rc -ne 0 ]] || { fail "accepted production secret service"; return 1; }
}

accepts_isolated_temp() {
  JABOT_APP_DATA_DIR="$SANDBOX/jabot-acceptance-x/data" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
    "$SCRIPT" _assert-isolation >/dev/null \
    || { fail "refused a valid isolated temp dir"; return 1; }
}

unknown_command_fails() {
  local rc=0
  "$SCRIPT" not-a-command >/dev/null 2>&1 || rc=$?
  [[ $rc -ne 0 ]] || { fail "unknown command succeeded"; return 1; }
}

printf 'windows-acceptance tests\n'
run_case "check passes on this tree" check_passes
run_case "matrix names the smoke cells and gaps" matrix_names_required_cells
run_case "checklist names smoke cells and gaps" checklist_names_smoke_and_gaps
run_case "refuses run until a packaged exe exists" refuses_run_everywhere
run_case "refuses production AppData" refuses_production_appdata
run_case "refuses production macOS app-support path" refuses_production_macos_path
run_case "refuses production secret service" refuses_production_secret_service
run_case "accepts isolated temp data + acceptance service" accepts_isolated_temp
run_case "unknown command fails" unknown_command_fails

printf '\n'
if [[ $FAILURES -eq 0 ]]; then
  printf '\033[32m%d passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d failed / %d run\033[0m\n' "$FAILURES" "$COUNT"
exit 1
