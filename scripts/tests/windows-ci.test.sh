#!/usr/bin/env bash
#
# Tests for the Windows CI planner, verify script, and workflow contract.
#
# The planner is how CI decides whether to spend 2x Windows minutes on
# scripts/windows-verify.sh. A classifier that quietly stops matching a
# host path is worse than no classifier, because the PR check still goes
# green and the port rots. The workflow must also keep failures real
# (no continue-on-error) and keep packaging opt-in.
#
#   ./scripts/tests/windows-ci.test.sh
#   ./scripts/tests/windows-ci.test.sh planner_
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
PLANNER="$REPO_ROOT/scripts/windows-needed.sh"
VERIFY_SH="$REPO_ROOT/scripts/windows-verify.sh"
WORKFLOW="$REPO_ROOT/.github/workflows/windows.yml"
DOCS="$REPO_ROOT/docs/windows-ci.md"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

pass() { printf '  \033[32mok\033[0m   %s\n' "$CASE"; }
fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$CASE"
  printf '       %s\n' "$@"
  FAILURES=$((FAILURES + 1))
}
assert_eq() {
  if [ "$1" != "$2" ]; then
    fail "$3: expected [$1], got [$2]"
    return 1
  fi
}
assert_contains() {
  case "$1" in
    *"$2"*) return 0 ;;
    *) fail "$3: [$2] not found in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-400)"; return 1 ;;
  esac
}
assert_not_contains() {
  case "$1" in
    *"$2"*) fail "$3: [$2] should not be in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-400)"; return 1 ;;
    *) return 0 ;;
  esac
}

plan() {
  "$PLANNER" --paths "$@" 2>/dev/null
}

run_case() {
  CASE="$1"
  if [ -n "$FILTER" ] && ! printf '%s' "$CASE" | grep -q "$FILTER"; then
    return 0
  fi
  COUNT=$((COUNT + 1))
  "$2"
}

# --- planner ----------------------------------------------------------------

planner_frontend_only_skips() {
  local out
  out=$(plan src/App.tsx src/styles.css docs/img/foo/after.png) || {
    fail "planner exited $?"; return
  }
  assert_eq 'verify=0' "$out" "frontend-only change" && pass
}

planner_docs_only_skips() {
  local out
  out=$(plan docs/windows-ci.md README.md CONTRIBUTING.md docs/packaging.md) || {
    fail "planner exited $?"; return
  }
  assert_eq 'verify=0' "$out" "docs-only change" && pass
}

planner_host_crate_runs() {
  local out
  out=$(plan src-tauri/src/lib.rs) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "src-tauri/src/lib.rs" && pass
}

planner_tauri_conf_runs() {
  local out
  out=$(plan src-tauri/tauri.conf.json) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "tauri.conf.json" && pass
}

planner_toolchain_runs() {
  local out
  out=$(plan rust-toolchain.toml) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "rust-toolchain.toml" && pass
}

planner_workflow_runs() {
  local out
  out=$(plan .github/workflows/windows.yml) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows.yml" && pass
}

planner_verify_script_runs() {
  local out
  out=$(plan scripts/windows-verify.sh) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows-verify.sh" && pass
}

planner_needed_script_runs() {
  local out
  out=$(plan scripts/windows-needed.sh) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows-needed.sh" && pass
}

planner_ci_test_runs() {
  local out
  out=$(plan scripts/tests/windows-ci.test.sh) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows-ci.test.sh" && pass
}

planner_secrets_check_runs() {
  local out
  out=$(plan scripts/windows-secrets-check.sh) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows-secrets-check.sh" && pass
}

planner_adapter_lifecycle_runs() {
  local out
  out=$(plan scripts/windows-adapter-lifecycle.sh) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows-adapter-lifecycle.sh" && pass
}

planner_adapter_lifecycle_test_runs() {
  local out
  out=$(plan scripts/tests/windows-adapter-lifecycle.test.sh) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "windows-adapter-lifecycle.test.sh" && pass
}

planner_linux_ci_does_not_start_windows() {
  local out
  out=$(plan .github/workflows/ci.yml .github/workflows/macos-native.yml) || {
    fail "planner exited $?"; return
  }
  assert_eq 'verify=0' "$out" "Linux/macOS workflow-only change" && pass
}

planner_force_verify() {
  local out
  out=$("$PLANNER" --force-verify 2>/dev/null) || { fail "planner exited $?"; return; }
  assert_eq 'verify=1' "$out" "--force-verify" && pass
}

planner_unknown_flag_fails() {
  local rc=0
  "$PLANNER" --nope >/dev/null 2>&1 || rc=$?
  assert_eq 1 "$rc" "unknown flag must be a failure" && pass
}

planner_bad_base_fails_closed() {
  local rc=0 out
  out=$("$PLANNER" --base definitely-not-a-ref 2>/dev/null) || rc=$?
  assert_eq 1 "$rc" "unresolvable --base must fail closed" || return
  assert_not_contains "${out:-}" 'verify=0' "failed git diff must not skip" && pass
}

# --- verify script contract -------------------------------------------------

verify_script_is_the_windows_gate() {
  local body
  body=$(cat "$VERIFY_SH")
  assert_contains "$body" 'cargo check' "shipping crate check" || return
  assert_contains "$body" '--locked' "must use the lockfile" || return
  assert_contains "$body" 'cargo clippy' "clippy on Windows cfg" || return
  assert_contains "$body" '-D warnings' "warnings are errors" || return
  assert_contains "$body" 'dev-bins' "tests need the gated bins" || return
  assert_contains "$body" 'cargo test' "portable rust tests" || return
  assert_contains "$body" 'windows-secrets-check.sh' "named Credential Manager check" || return
  assert_contains "$body" 'windows-verify-stub' "adapter stub without npm" || return
  assert_not_contains "$body" 'playwright' "Playwright is out of scope" || return
  assert_not_contains "$body" 'continue-on-error' "script must not swallow failures" || return
  pass
}

# --- workflow contract ------------------------------------------------------

workflow_uses_windows_latest_without_continue_on_error() {
  local wf
  wf=$(cat "$WORKFLOW")
  assert_contains "$wf" 'windows-latest' "verify/package run on windows-latest" || return
  assert_contains "$wf" 'windows-needed.sh' "planner is wired" || return
  assert_contains "$wf" 'windows-verify.sh' "verify job runs the script" || return
  assert_contains "$wf" 'windows-adapter-lifecycle' "adapter lifecycle job is named" || return
  assert_contains "$wf" 'windows-adapter-lifecycle.sh' "adapter lifecycle script is wired" || return
  assert_contains "$wf" 'windows-package' "package label is named" || return
  assert_contains "$wf" '--bundles nsis' "optional package is NSIS" || return
  assert_contains "$wf" 'workflow_dispatch' "dispatch avoids burning PR minutes" || return
  assert_contains "$wf" 'cancel-in-progress' "superseded PR runs do not keep paying" || return
  assert_contains "$wf" "github.event.action != 'labeled'" "labeled must not start or cancel verify" || return
  assert_not_contains "$wf" 'continue-on-error:' "failures are real (no YAML key)" || return
  assert_not_contains "$wf" 'playwright' "no Playwright on Windows in the first cut" || return
  pass
}

workflow_package_is_opt_in() {
  local wf
  wf=$(cat "$WORKFLOW")
  assert_contains "$wf" 'inputs.package' "dispatch package input" || return
  assert_contains "$wf" 'windows-package' "PR label may package" || return
  assert_contains "$wf" "github.event.label.name == 'windows-package'" "only that label starts package" || return
  assert_contains "$wf" 'if-no-files-found: error' "missing installer is a red job" || return
  # The verify job must not be the package job. Tags belong to release.yml.
  assert_contains "$wf" 'name: windows verify' "verify job exists" || return
  assert_contains "$wf" 'name: windows package' "package job is separate" || return
  assert_not_contains "$wf" "startsWith(github.ref, 'refs/tags/v')" "tags must not package here" || return
  assert_not_contains "$wf" 'if-no-files-found: warn' "warn is a false green" || return
  pass
}

linux_macos_jobs_still_own_ci_yml() {
  local ci native
  ci=$(cat "$REPO_ROOT/.github/workflows/ci.yml")
  native=$(cat "$REPO_ROOT/.github/workflows/macos-native.yml")
  assert_contains "$ci" 'runs-on: ubuntu-latest' "Linux verify still in ci.yml" || return
  assert_contains "$ci" $'  verify:\n    runs-on: ubuntu-latest' "verify job is Ubuntu" || return
  assert_contains "$ci" 'runs-on: macos-latest' "macOS bundle/clippy still in ci.yml" || return
  assert_contains "$native" 'macos-packaged' "macOS packaged job untouched" || return
  assert_not_contains "$ci" 'windows-latest' "Windows stays in windows.yml" || return
  assert_not_contains "$native" 'windows-latest' "macos-native is still macOS" || return
  pass
}

# --- docs + verify.sh wiring ------------------------------------------------

docs_name_guarantees_vs_smoke() {
  local docs
  docs=$(cat "$DOCS")
  assert_contains "$docs" 'windows-latest' "docs name the runner" || return
  assert_contains "$docs" 'guarantees' "docs name what CI guarantees" || return
  assert_contains "$docs" 'human' "docs name the human smoke" || return
  assert_contains "$docs" 'NSIS' "docs mention optional packaging" || return
  assert_contains "$docs" 'continue-on-error' "docs say failures are real" || return
  assert_contains "$docs" 'Playwright' "docs say visual suite is out of scope" || return
  assert_contains "$docs" '#286' "docs cite the issue" || return
  pass
}

verify_sh_runs_this_suite() {
  local v
  v=$(cat "$REPO_ROOT/scripts/verify.sh")
  assert_contains "$v" 'windows-ci.test.sh' "verify.sh must run this contract" || return
  assert_contains "$v" 'windows ci' "verify.sh names the stage" || return
  pass
}

contributing_mentions_windows_ci() {
  local c
  c=$(cat "$REPO_ROOT/CONTRIBUTING.md")
  assert_contains "$c" 'windows-ci' "CONTRIBUTING points at the Windows docs" || return
  pass
}

printf '\nwindows ci contract\n'
run_case planner_frontend_only_skips planner_frontend_only_skips
run_case planner_docs_only_skips planner_docs_only_skips
run_case planner_host_crate_runs planner_host_crate_runs
run_case planner_tauri_conf_runs planner_tauri_conf_runs
run_case planner_toolchain_runs planner_toolchain_runs
run_case planner_workflow_runs planner_workflow_runs
run_case planner_verify_script_runs planner_verify_script_runs
run_case planner_needed_script_runs planner_needed_script_runs
run_case planner_ci_test_runs planner_ci_test_runs
run_case planner_secrets_check_runs planner_secrets_check_runs
run_case planner_adapter_lifecycle_runs planner_adapter_lifecycle_runs
run_case planner_adapter_lifecycle_test_runs planner_adapter_lifecycle_test_runs
run_case planner_linux_ci_does_not_start_windows planner_linux_ci_does_not_start_windows
run_case planner_force_verify planner_force_verify
run_case planner_unknown_flag_fails planner_unknown_flag_fails
run_case planner_bad_base_fails_closed planner_bad_base_fails_closed
run_case verify_script_is_the_windows_gate verify_script_is_the_windows_gate
run_case workflow_uses_windows_latest_without_continue_on_error workflow_uses_windows_latest_without_continue_on_error
run_case workflow_package_is_opt_in workflow_package_is_opt_in
run_case linux_macos_jobs_still_own_ci_yml linux_macos_jobs_still_own_ci_yml
run_case docs_name_guarantees_vs_smoke docs_name_guarantees_vs_smoke
run_case verify_sh_runs_this_suite verify_sh_runs_this_suite
run_case contributing_mentions_windows_ci contributing_mentions_windows_ci

printf '\n%d cases, %d failed\n' "$COUNT" "$FAILURES"
if [ "$FAILURES" -ne 0 ]; then
  exit 1
fi
exit 0
