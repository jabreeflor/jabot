#!/usr/bin/env bash
#
# Tests for scripts/macos-acceptance.sh — the packaged-app acceptance gate.
#
# The script is the only thing that will launch a real JaBot.app, and most of
# its refusals (production app data, production Keychain, Linux `run`, missing
# updater archives, a tree that is not an .app) are the ones nobody notices
# when they stop happening. So the refusals get tests. They run on Linux
# against a stubbed `uname` where needed; `check` and `package` are real.
#
#   ./scripts/tests/macos-acceptance.test.sh
#   ./scripts/tests/macos-acceptance.test.sh refuses
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
SCRIPT="$REPO_ROOT/scripts/macos-acceptance.sh"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-macos-acceptance.XXXXXX") || exit 1
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

# --- cases -----------------------------------------------------------------

check_passes() {
  local out
  out=$("$SCRIPT" check 2>&1) || { fail "check failed: $out"; return 1; }
  case "$out" in
    *PASS*) ;;
    *) fail "check did not print PASS: $out"; return 1 ;;
  esac
}

matrix_names_required_cells() {
  local out
  out=$("$SCRIPT" matrix 2>&1) || { fail "matrix failed: $out"; return 1; }
  local cell
  for cell in tauri-ipc synthetic-turn close-to-dock quit-relaunch notify isolated-keychain packaged-adapter updater-artifacts signed-update-install compiled packaged launched; do
    case "$out" in
      *"$cell"*) ;;
      *) fail "matrix missing $cell"; return 1 ;;
    esac
  done
  case "$out" in
    *$'\n'Playwright$'\t'*|Playwright$'\t'*) fail "matrix labelled a Playwright cell"; return 1 ;;
  esac
}

release_checklist_distinguishes_stages() {
  local out
  out=$("$SCRIPT" release-checklist 2>&1) || { fail "checklist failed: $out"; return 1; }
  for word in compiled packaged launched "interactively verified" "Playwright WebKit is not this list" "signed update"; do
    case "$out" in
      *"$word"*) ;;
      *) fail "checklist missing [$word]: $out"; return 1 ;;
    esac
  done
}

refuses_linux_run() {
  local out rc=0
  out=$("$SCRIPT" run --app /no/such.app 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "run succeeded on $(uname -s)"; return 1; }
  case "$out" in
    *macOS-only*|*D-019*) ;;
    *) fail "run refusal did not name macOS/D-019: $out"; return 1 ;;
  esac
}

refuses_production_app_data() {
  local out rc=0
  out=$(JABOT_ACCEPTANCE_DIR="$SANDBOX/e" \
    JABOT_APP_DATA_DIR="$HOME/Library/Application Support/com.jabot.app" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
    "$SCRIPT" _assert-isolation 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "accepted production app-support path"; return 1; }
  case "$out" in
    *production*) ;;
    *) fail "refusal did not say production: $out"; return 1 ;;
  esac
}

refuses_production_keychain() {
  local out rc=0
  out=$(JABOT_ACCEPTANCE_DIR="$SANDBOX/e" \
    JABOT_APP_DATA_DIR="$SANDBOX/jabot-acceptance-x/data" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app \
    "$SCRIPT" _assert-isolation 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "accepted production Keychain service"; return 1; }
}

accepts_isolated_temp() {
  JABOT_ACCEPTANCE_DIR="$SANDBOX/e" \
    JABOT_APP_DATA_DIR="$SANDBOX/jabot-acceptance-x/data" \
    JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
    "$SCRIPT" _assert-isolation >/dev/null \
    || { fail "refused a valid isolated temp dir"; return 1; }
}

package_inspects_fake_app() {
  local app="$SANDBOX/JaBot.app"
  local id name ver
  id=$(node -p 'require("'"$REPO_ROOT"'/src-tauri/tauri.conf.json").identifier')
  name=$(node -p 'require("'"$REPO_ROOT"'/src-tauri/tauri.conf.json").productName')
  ver=$(node -p 'require("'"$REPO_ROOT"'/src-tauri/tauri.conf.json").version')
  mkdir -p "$app/Contents/MacOS" \
    "$app/Contents/Resources/vendor/adapters/node_modules/@agentclientprotocol/claude-agent-acp/dist"
  cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>$id</string>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleShortVersionString</key><string>$ver</string>
</dict>
</plist>
PLIST
  printf 'module.exports = {}\n' \
    >"$app/Contents/Resources/vendor/adapters/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"
  local out
  out=$(cd "$REPO_ROOT" && "$SCRIPT" package --app "$app" 2>&1) || { fail "package failed: $out"; return 1; }
  case "$out" in
    *PASS*package*) ;;
    *) fail "package did not PASS: $out"; return 1 ;;
  esac
  case "$out" in
    *"updater archives are a separate check"*) ;;
    *) fail "package claimed updater archives: $out"; return 1 ;;
  esac
}

package_refuses_wrong_bundle_id() {
  local app="$SANDBOX/Wrong.app"
  mkdir -p "$app/Contents/Resources/vendor/adapters/node_modules/@agentclientprotocol/claude-agent-acp/dist"
  cat >"$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.someone.else</string>
  <key>CFBundleName</key><string>JaBot</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
</dict>
</plist>
PLIST
  printf 'x\n' >"$app/Contents/Resources/vendor/adapters/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"
  local out rc=0
  out=$(cd "$REPO_ROOT" && "$SCRIPT" package --app "$app" 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "package accepted a foreign bundle id"; return 1; }
}

updater_artifacts_need_archive_and_sig() {
  local dir="$SANDBOX/out"
  mkdir -p "$dir"
  local out rc=0
  out=$("$SCRIPT" updater-artifacts "$dir" 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "updater-artifacts passed an empty dir"; return 1; }
  case "$out" in
    *.app.tar.gz*) ;;
    *) fail "empty-dir refusal did not name the archive: $out"; return 1 ;;
  esac

  : >"$dir/JaBot.app.tar.gz"
  rc=0
  out=$("$SCRIPT" updater-artifacts "$dir" 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "updater-artifacts passed without a sig"; return 1; }
  case "$out" in
    *.sig*) ;;
    *) fail "missing-sig refusal did not name .sig: $out"; return 1 ;;
  esac

  : >"$dir/JaBot.app.tar.gz.sig"
  out=$("$SCRIPT" updater-artifacts "$dir" 2>&1) || { fail "updater-artifacts failed with both files: $out"; return 1; }
  case "$out" in
    *"not a signed update"*) ;;
    *) fail "did not disclaim signed install: $out"; return 1 ;;
  esac
}

unknown_command_fails() {
  local rc=0
  "$SCRIPT" not-a-command >/dev/null 2>&1 || rc=$?
  [[ $rc -ne 0 ]] || { fail "unknown command succeeded"; return 1; }
}

# --- run -------------------------------------------------------------------

printf 'macos-acceptance tests\n'
run_case "check passes on this tree" check_passes
run_case "matrix names the native cells" matrix_names_required_cells
run_case "release checklist distinguishes compiled/packaged/launched" release_checklist_distinguishes_stages
run_case "refuses run on this OS unless Darwin" refuses_linux_run
run_case "refuses production app data" refuses_production_app_data
run_case "refuses production Keychain service" refuses_production_keychain
run_case "accepts isolated temp data + acceptance service" accepts_isolated_temp
run_case "package inspects a fake JaBot.app" package_inspects_fake_app
run_case "package refuses a foreign bundle id" package_refuses_wrong_bundle_id
run_case "updater-artifacts requires archive and sig, not an install" updater_artifacts_need_archive_and_sig
run_case "unknown command fails" unknown_command_fails

printf '\n'
if [[ $FAILURES -eq 0 ]]; then
  printf '\033[32m%d passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d failed / %d run\033[0m\n' "$FAILURES" "$COUNT"
exit 1
