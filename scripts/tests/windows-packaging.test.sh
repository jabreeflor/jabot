#!/usr/bin/env bash
#
# Tests for scripts/windows-packaging.sh — the NSIS packaging gate (#281).
#
# The script is what stops a release.yml edit from dropping the Windows
# installer, or from letting that job write latest.json. The refusals get
# tests: a copied tree injects nsis into base targets, uploadUpdaterJson
# / createUpdaterArtifacts / APPLE_* / TAURI_SIGNING_* on the windows job,
# or drops releaseBody / script-shell, and expects a red. They run on
# Linux; `check` on this tree is real, artifacts use a fake bundle.
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

# --- check refusals against a copied tree ----------------------------------
# windows-packaging.sh cds to dirname(BASH_SOURCE)/.., so a copy of the
# script plus the files it reads is a fixture. Mutate one file; expect red.

fixture_tree() {
  local d="$SANDBOX/tree-$1"
  mkdir -p "$d/scripts" "$d/src-tauri/icons" "$d/.github/workflows" "$d/docs"
  cp "$SCRIPT" "$d/scripts/windows-packaging.sh"
  chmod +x "$d/scripts/windows-packaging.sh"
  cp "$REPO_ROOT/src-tauri/tauri.conf.json" "$d/src-tauri/"
  cp "$REPO_ROOT/src-tauri/tauri.windows.conf.json" "$d/src-tauri/"
  cp "$REPO_ROOT/src-tauri/icons/icon.ico" "$d/src-tauri/icons/"
  cp "$REPO_ROOT/.github/workflows/release.yml" "$d/.github/workflows/"
  cp "$REPO_ROOT/package.json" "$d/"
  cp "$REPO_ROOT/README.md" "$d/"
  cp "$REPO_ROOT/docs/packaging.md" "$d/docs/"
  printf '%s\n' "$d"
}

patch_windows_job() {
  # Caller stdin is JS that mutates `win` (the windows: job body).
  local file=$1
  local jsfile="$SANDBOX/patch-win.js"
  cat >"$jsfile"
  node -e '
    const fs = require("fs");
    const p = process.argv[1];
    const y = fs.readFileSync(p, "utf8");
    const start = y.search(/^  windows:/m);
    if (start < 0) {
      console.error("no windows: job");
      process.exit(2);
    }
    const before = y.slice(0, start);
    let win = y.slice(start);
    eval(fs.readFileSync(process.argv[2], "utf8"));
    fs.writeFileSync(p, before + win);
  ' "$file" "$jsfile"
}

expect_check_red() { # tree_dir needle label
  local root=$1 needle=$2 label=$3 out rc=0
  out=$(cd "$root" && ./scripts/windows-packaging.sh check 2>&1) || rc=$?
  [[ $rc -ne 0 ]] || { fail "$label: check stayed green"; return 1; }
  case "$out" in
    *"$needle"*) ;;
    *) fail "$label: refusal did not mention [$needle]: $out"; return 1 ;;
  esac
}

check_refuses_nsis_in_base_targets() {
  local tree
  tree=$(fixture_tree nsis-base)
  node -e '
    const fs = require("fs");
    const p = process.argv[1];
    const c = JSON.parse(fs.readFileSync(p, "utf8"));
    c.bundle.targets = [...(c.bundle.targets || []), "nsis"];
    fs.writeFileSync(p, JSON.stringify(c, null, 2) + "\n");
  ' "$tree/src-tauri/tauri.conf.json"
  expect_check_red "$tree" "nsis" "nsis in base targets"
}

check_refuses_missing_upload_updater_json() {
  local tree
  tree=$(fixture_tree no-upload-flag)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
win = win.replace(/^[ \t]*uploadUpdaterJson:[ \t]*false[ \t]*\n/m, '');
JS
  expect_check_red "$tree" "uploadUpdaterJson" "missing uploadUpdaterJson: false"
}

check_refuses_upload_updater_json_true() {
  local tree
  tree=$(fixture_tree upload-true)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
win = win.replace(/uploadUpdaterJson:\s*false/, 'uploadUpdaterJson: true');
JS
  expect_check_red "$tree" "uploadUpdaterJson" "uploadUpdaterJson: true"
}

check_refuses_create_updater_artifacts_on_windows() {
  local tree
  tree=$(fixture_tree create-updater)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
// Must land on a non-comment line — windowsCode strips `#` lines.
win = win.replace(
  /^([ \t]*--bundles nsis)\s*$/m,
  '$1 --config {"bundle":{"createUpdaterArtifacts":true}}'
);
JS
  expect_check_red "$tree" "createUpdaterArtifacts" "createUpdaterArtifacts on windows"
}

check_refuses_missing_release_body() {
  local tree
  tree=$(fixture_tree no-body)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
win = win.replace(/^[ \t]*releaseBody:[ \t]*\|[ \t]*\n(?:[ \t]+.+\n)+/m, '');
JS
  expect_check_red "$tree" "releaseBody" "missing windows releaseBody"
}

check_refuses_missing_script_shell() {
  local tree
  tree=$(fixture_tree no-shell)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
win = win.replace(/^[ \t]*-[ \t]*name:[ \t]*Point npm scripts at Git Bash\n(?:[ \t]+run:[ \t].+\n)+/m, '');
JS
  expect_check_red "$tree" "script-shell" "missing npm script-shell"
}

check_refuses_apple_secret_on_windows() {
  local tree
  tree=$(fixture_tree apple-secret)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
const needle = 'GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}';
const i = win.indexOf(needle);
if (i < 0) throw new Error('missing GITHUB_TOKEN in windows job');
win = win.slice(0, i + needle.length) + '\n          APPLE_ID: ${{ secrets.APPLE_ID }}' + win.slice(i + needle.length);
JS
  expect_check_red "$tree" "APPLE_" "APPLE_* on windows"
}

check_refuses_tauri_signing_on_windows() {
  local tree
  tree=$(fixture_tree signing-secret)
  patch_windows_job "$tree/.github/workflows/release.yml" <<'JS'
const needle = 'GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}';
const i = win.indexOf(needle);
if (i < 0) throw new Error('missing GITHUB_TOKEN in windows job');
win = win.slice(0, i + needle.length) + '\n          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}' + win.slice(i + needle.length);
JS
  expect_check_red "$tree" "TAURI_SIGNING_" "TAURI_SIGNING_* on windows"
}

printf 'windows-packaging tests\n'
run_case "check passes on this tree" check_passes
run_case "artifacts requires a setup exe" artifacts_need_setup_exe
run_case "artifacts accepts a fake NSIS setup exe" artifacts_accepts_setup_exe
run_case "artifacts refuses an empty setup exe" artifacts_refuse_empty_file
run_case "unknown command fails" unknown_command_fails
run_case "check refuses nsis in base targets" check_refuses_nsis_in_base_targets
run_case "check refuses missing uploadUpdaterJson" check_refuses_missing_upload_updater_json
run_case "check refuses uploadUpdaterJson true" check_refuses_upload_updater_json_true
run_case "check refuses createUpdaterArtifacts on windows" check_refuses_create_updater_artifacts_on_windows
run_case "check refuses missing windows releaseBody" check_refuses_missing_release_body
run_case "check refuses missing npm script-shell" check_refuses_missing_script_shell
run_case "check refuses APPLE_* on windows" check_refuses_apple_secret_on_windows
run_case "check refuses TAURI_SIGNING_* on windows" check_refuses_tauri_signing_on_windows

printf '\n'
if [[ $FAILURES -eq 0 ]]; then
  printf '\033[32m%d passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d failed / %d run\033[0m\n' "$FAILURES" "$COUNT"
exit 1
