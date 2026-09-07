#!/usr/bin/env bash
#
# Tests for the before-merge macOS lint planner and the notify cross-check.
#
# The planner is how CI decides whether to spend Linux minutes on
# check-mac-notify.sh or 10x macOS minutes on check-macos-clippy.sh. A
# classifier that quietly stops matching a path is worse than no classifier,
# because the PR check still goes green. So the matches are tested here,
# offline, with no rustup and no Mac.
#
# The notify script's refusal to skip a missing Apple target, and its
# "injected Clippy warning fails" proof (when the target is installed), live
# here too. Default `./scripts/verify.sh` runs this suite and never the
# network-using cross-check itself.
#
#   ./scripts/tests/macos-lint.test.sh            # all cases
#   ./scripts/tests/macos-lint.test.sh planner_   # cases whose name contains planner_
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
PLANNER="$REPO_ROOT/scripts/macos-lint-needed.sh"
NOTIFY_CHECK="$REPO_ROOT/scripts/check-mac-notify.sh"
NATIVE_CHECK="$REPO_ROOT/scripts/check-macos-clippy.sh"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-macos-lint.XXXXXX") || exit 1
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

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

# --- planner: which paths turn which jobs on --------------------------------

planner_frontend_only_skips_both() {
  local out
  out=$(plan src/App.tsx src/styles.css docs/img/foo/after.png) || {
    fail "planner exited $?"; return
  }
  assert_eq $'notify=0\nnative=0' "$out" "frontend-only change" && pass
}

planner_notify_mac_rs_runs_notify_only() {
  local out
  out=$(plan src-tauri/src/notify/mac.rs) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=0' "$out" "notify/mac.rs" && pass
}

planner_notify_dir_prefix_runs_notify() {
  local out
  out=$(plan src-tauri/src/notify/mod.rs src-tauri/src/notify/unsupported.rs) || {
    fail "planner exited $?"; return
  }
  assert_eq $'notify=1\nnative=0' "$out" "notify/ siblings" && pass
}

planner_secrets_runs_native_only() {
  local out
  out=$(plan src-tauri/src/host/store/secrets.rs) || { fail "planner exited $?"; return; }
  assert_eq $'notify=0\nnative=1' "$out" "secrets.rs" && pass
}

planner_lib_rs_runs_native_only() {
  local out
  out=$(plan src-tauri/src/lib.rs) || { fail "planner exited $?"; return; }
  assert_eq $'notify=0\nnative=1' "$out" "lib.rs" && pass
}

planner_cargo_toml_runs_both() {
  local out
  out=$(plan src-tauri/Cargo.toml) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "Cargo.toml" && pass
}

planner_cargo_lock_runs_both() {
  local out
  out=$(plan src-tauri/Cargo.lock) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "Cargo.lock" && pass
}

planner_clippy_toml_runs_both() {
  local out
  out=$(plan src-tauri/clippy.toml) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "clippy.toml" && pass
}

planner_toolchain_runs_both() {
  local out
  out=$(plan rust-toolchain.toml) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "rust-toolchain.toml" && pass
}

planner_ci_workflow_runs_both() {
  local out
  out=$(plan .github/workflows/ci.yml) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "ci.yml" && pass
}

planner_notify_script_runs_notify_only() {
  local out
  out=$(plan scripts/check-mac-notify.sh) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=0' "$out" "check-mac-notify.sh" && pass
}

planner_native_script_runs_native_only() {
  local out
  out=$(plan scripts/check-macos-clippy.sh) || { fail "planner exited $?"; return; }
  assert_eq $'notify=0\nnative=1' "$out" "check-macos-clippy.sh" && pass
}

planner_planner_script_runs_both() {
  local out
  out=$(plan scripts/macos-lint-needed.sh) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "macos-lint-needed.sh" && pass
}

planner_path_rs_is_deliberately_excluded() {
  local out
  out=$(plan src-tauri/src/host/harness/path.rs) || { fail "planner exited $?"; return; }
  assert_eq $'notify=0\nnative=0' "$out" "path.rs uses cfg! and is linted on Linux" && pass
}

planner_mixed_notify_and_secrets_runs_both() {
  local out
  out=$(plan src-tauri/src/notify/mac.rs src-tauri/src/host/store/secrets.rs) || {
    fail "planner exited $?"; return
  }
  assert_eq $'notify=1\nnative=1' "$out" "notify + secrets" && pass
}

planner_docs_only_skips_both() {
  local out
  out=$(plan docs/macos-lint.md CONTRIBUTING.md README.md) || {
    fail "planner exited $?"; return
  }
  assert_eq $'notify=0\nnative=0' "$out" "docs-only" && pass
}

planner_force_all_runs_both() {
  local out
  out=$("$PLANNER" --force-all 2>/dev/null) || { fail "planner exited $?"; return; }
  assert_eq $'notify=1\nnative=1' "$out" "--force-all" && pass
}

planner_github_output_writes_keys() {
  local out file="$SANDBOX/github-output"
  : > "$file"
  out=$(GITHUB_OUTPUT="$file" "$PLANNER" --github-output --paths src-tauri/src/notify/mac.rs 2>/dev/null) || {
    fail "planner exited $?"; return
  }
  assert_eq $'notify=1\nnative=0' "$out" "stdout"
  assert_eq $'notify=1\nnative=0' "$(cat "$file")" "GITHUB_OUTPUT" && pass
}

planner_unknown_flag_fails() {
  local rc=0
  "$PLANNER" --nope >/dev/null 2>&1 || rc=$?
  assert_eq 1 "$rc" "exit code" && pass
}

# --- notify check: never silently skip --------------------------------------

notify_missing_target_fails_loudly() {
  # Stub rustup so default verify.sh never talks to the rustup CDN just to
  # prove this refusal. The real binary is what CI's notify job runs.
  local bin="$SANDBOX/bin-no-apple" err rc=0
  mkdir -p "$bin"
  cat > "$bin/rustup" <<'STUB'
#!/bin/sh
if [ "$1" = "target" ] && [ "$2" = "list" ]; then
  echo "x86_64-unknown-linux-gnu"
  exit 0
fi
exit 0
STUB
  chmod +x "$bin/rustup"
  err=$(PATH="$bin:$PATH" "$NOTIFY_CHECK" 2>&1) || rc=$?
  assert_eq 1 "$rc" "exit code" || return
  assert_contains "$err" "rustup target add x86_64-apple-darwin" "install line" && pass
}

notify_missing_rustup_fails_loudly() {
  # Keep /usr/bin so `#!/usr/bin/env bash` can find bash, but no rustup.
  local err rc=0
  err=$(PATH="/usr/bin:/bin" HOME="$SANDBOX/empty-home" "$NOTIFY_CHECK" 2>&1) || rc=$?
  assert_eq 1 "$rc" "exit code" || return
  assert_contains "$err" "rustup not found" "refusal" && pass
}

notify_injected_clippy_warning_fails() {
  if ! command -v rustup >/dev/null 2>&1; then
    printf '  skip  %s (no rustup)\n' "$CASE"
    return 0
  fi
  if ! rustup target list --installed | grep -qx x86_64-apple-darwin; then
    printf '  skip  %s (no apple-darwin target; the CI notify job runs this)\n' "$CASE"
    return 0
  fi
  local tmp="$SANDBOX/notify-rot" rc=0 err
  mkdir -p "$tmp"
  cp -a "$REPO_ROOT/src-tauri/src/notify/." "$tmp/"
  cat >> "$tmp/mac.rs" <<'RS'

pub fn _intentional_lint_violation(s: &String) {
    let _ = s.len();
}
RS
  err=$(JABOT_NOTIFY_DIR="$tmp" "$NOTIFY_CHECK" 2>&1) || rc=$?
  if [ "$rc" -eq 0 ]; then
    fail "check-mac-notify.sh passed despite a ptr_arg warning in mac.rs"
    return
  fi
  assert_contains "$err" "_intentional_lint_violation" "names the injected fn" && pass
}

# --- native script: refuse Linux instead of compiling past macos cfg --------

native_refuses_linux() {
  if [ "$(uname -s)" = "Darwin" ]; then
    printf '  skip  %s (on macOS; the CI macos clippy job is the happy path)\n' "$CASE"
    return 0
  fi
  local err rc=0
  err=$("$NATIVE_CHECK" 2>&1) || rc=$?
  assert_eq 1 "$rc" "exit code" || return
  assert_contains "$err" "needs macOS" "refusal" && pass
}

# --- scripts exist and are executable ---------------------------------------

scripts_are_executable() {
  local missing=0 f
  for f in "$PLANNER" "$NOTIFY_CHECK" "$NATIVE_CHECK"; do
    if [ ! -x "$f" ]; then
      fail "$f is not executable"
      missing=1
    fi
  done
  [ "$missing" -eq 0 ] && pass
}

run_case planner_frontend_only_skips_both planner_frontend_only_skips_both
run_case planner_notify_mac_rs_runs_notify_only planner_notify_mac_rs_runs_notify_only
run_case planner_notify_dir_prefix_runs_notify planner_notify_dir_prefix_runs_notify
run_case planner_secrets_runs_native_only planner_secrets_runs_native_only
run_case planner_lib_rs_runs_native_only planner_lib_rs_runs_native_only
run_case planner_cargo_toml_runs_both planner_cargo_toml_runs_both
run_case planner_cargo_lock_runs_both planner_cargo_lock_runs_both
run_case planner_clippy_toml_runs_both planner_clippy_toml_runs_both
run_case planner_toolchain_runs_both planner_toolchain_runs_both
run_case planner_ci_workflow_runs_both planner_ci_workflow_runs_both
run_case planner_notify_script_runs_notify_only planner_notify_script_runs_notify_only
run_case planner_native_script_runs_native_only planner_native_script_runs_native_only
run_case planner_planner_script_runs_both planner_planner_script_runs_both
run_case planner_path_rs_is_deliberately_excluded planner_path_rs_is_deliberately_excluded
run_case planner_mixed_notify_and_secrets_runs_both planner_mixed_notify_and_secrets_runs_both
run_case planner_docs_only_skips_both planner_docs_only_skips_both
run_case planner_force_all_runs_both planner_force_all_runs_both
run_case planner_github_output_writes_keys planner_github_output_writes_keys
run_case planner_unknown_flag_fails planner_unknown_flag_fails
run_case notify_missing_target_fails_loudly notify_missing_target_fails_loudly
run_case notify_missing_rustup_fails_loudly notify_missing_rustup_fails_loudly
run_case notify_injected_clippy_warning_fails notify_injected_clippy_warning_fails
run_case native_refuses_linux native_refuses_linux
run_case scripts_are_executable scripts_are_executable

printf '\n%s cases, %s failed\n' "$COUNT" "$FAILURES"
[ "$FAILURES" -eq 0 ]
