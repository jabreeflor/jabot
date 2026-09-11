#!/usr/bin/env bash
#
# Windows ACP adapter spawn + teardown (#285).
#
#   check  Offline contract: Job Objects are still the documented kill-tree,
#          cfg(unix) / cfg(windows) stay explicit, PATHEXT is consulted, and
#          CI still has a windows-latest job in windows.yml. Runs on Linux.
#   run    cargo test the spawn + teardown cases. Required filters that match
#          0 tests fail the run (libtest exits 0 on no matches). Unix-only
#          names are not invoked on Windows and vice versa. Needs a Rust
#          toolchain.
#
# Not the full Windows verify (#286). This script is only the adapter
# lifecycle slice. CI runs `run` from windows.yml as `windows-adapter-lifecycle`
# when the #286 planner says host/CI paths changed.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

usage() {
  sed -n '3,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

die() { printf 'FAIL %s\n' "$*" >&2; exit 1; }
pass() { printf 'PASS %s\n' "$*"; }

need_file() {
  local path=$1
  [[ -f "$path" ]] || die "missing $path"
}

need_text() {
  local path=$1
  local needle=$2
  grep -Fq -- "$needle" "$path" || die "$path does not mention $needle"
}

on_windows() {
  case "${OS:-}$(uname -s 2>/dev/null || true)" in
    Windows_NT*|MINGW*|MSYS*|CYGWIN*) return 0 ;;
    *) return 1 ;;
  esac
}

check() {
  need_file src-tauri/src/host/procgroup.rs
  need_file src-tauri/src/host/acp/spawn.rs
  need_file src-tauri/src/host/harness/mod.rs
  need_file .github/workflows/windows.yml

  need_text src-tauri/src/host/procgroup.rs 'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE'
  need_text src-tauri/src/host/procgroup.rs 'CREATE_NEW_PROCESS_GROUP'
  need_text src-tauri/src/host/procgroup.rs 'CREATE_SUSPENDED'
  need_text src-tauri/src/host/procgroup.rs 'ResumeThread'
  need_text src-tauri/src/host/procgroup.rs 'taskkill'
  need_text src-tauri/src/host/procgroup.rs '#[cfg(unix)]'
  need_text src-tauri/src/host/procgroup.rs '#[cfg(windows)]'
  need_text src-tauri/src/host/procgroup.rs 'Ok(Some(_)) => return'
  need_text src-tauri/src/host/acp/spawn.rs 'kill_group_reaps_grandchild'
  need_text src-tauri/src/host/acp/spawn.rs 'kill_job_reaps_grandchild'
  need_text src-tauri/src/host/acp/spawn.rs 'job_assigned'
  need_text src-tauri/src/host/acp/spawn.rs 'job_contains'
  # Test-only helpers must stay behind cfg(test) or windows-verify clippy
  # (-D dead_code / unused_imports) fails the non-test lib compile.
  need_text src-tauri/src/host/procgroup.rs 'all(test, windows)'
  need_text src-tauri/src/host/acp/spawn.rs 'taskkill_fallback_reaps_grandchild'
  # The tests module must compile on Windows. `#[cfg(all(test, unix))]`
  # above `mod tests` hides every Job Object case; libtest then prints
  # `running 0 tests` and a required-filter check is the only thing that
  # fails (cade3a0 / run 34563853452).
  if grep -B1 -- '^mod tests' src-tauri/src/host/acp/spawn.rs \
      | grep -Fq 'all(test, unix)'; then
    die 'spawn.rs tests module is #[cfg(all(test, unix))]; Windows Job Object tests will match 0 tests'
  fi
  need_text src-tauri/src/host/acp/spawn.rs '#[cfg(test)]'
  # `cargo test --features dev-bins` builds jabot-hostd. The accept loop is
  # unix-only; the call site must stay behind the same cfg so Windows compiles.
  need_text src-tauri/src/bin/jabot-hostd.rs '#[cfg(unix)]'
  need_text src-tauri/src/bin/jabot-hostd.rs 'spawn_accept_loop'
  need_text src-tauri/src/host/harness/mod.rs 'PATHEXT'
  need_text src-tauri/src/host/harness/mod.rs 'resolve_in_dir'
  need_text .github/workflows/windows.yml 'windows-latest'
  need_text .github/workflows/windows.yml 'windows-adapter-lifecycle'

  # Unix process-group kill must still be the SIGTERM / SIGKILL pair, not a
  # Windows-shaped rewrite that leaked into the unix branch.
  grep -n 'SIGTERM\|SIGKILL' src-tauri/src/host/procgroup.rs >/dev/null \
    || die 'unix SIGTERM/SIGKILL path is gone from procgroup.rs'

  pass "windows adapter lifecycle contract"
}

# libtest exits 0 on zero matches. A renamed required filter used to keep
# CI green. Fail that case explicitly.
require_tests_ran() {
  local filter=$1
  local out=$2
  if printf '%s\n' "$out" | grep -Eq 'running 0 tests'; then
    die "required filter '$filter' matched 0 tests (libtest exits 0 on no matches)"
  fi
}

run_required_lib_test() {
  local name=$1
  local out status
  printf '> cargo test --lib %s\n' "$name"
  set +e
  out=$(cargo test "${CARGO_TEST_MANIFEST[@]}" --lib -- "$name" 2>&1)
  status=$?
  set -e
  printf '%s\n' "$out"
  require_tests_ran "$name" "$out"
  [[ "$status" -eq 0 ]] || die "cargo test --lib $name failed"
}

run_required_integration_test() {
  local harness=$1
  local name=$2
  local out status
  printf '> cargo test --test %s %s\n' "$harness" "$name"
  set +e
  out=$(cargo test "${CARGO_TEST_MANIFEST[@]}" --test "$harness" -- "$name" 2>&1)
  status=$?
  set -e
  printf '%s\n' "$out"
  require_tests_ran "$name" "$out"
  [[ "$status" -eq 0 ]] || die "cargo test --test $harness $name failed"
}

run_tests() {
  # One cargo invocation per filter. A single `a|b|c` string is a libtest
  # regex only after `--`, and a cargo TESTNAME otherwise — keep them
  # separate so a typo in one name cannot silently skip the rest.
  # Not `local`: helpers above read this array.
  CARGO_TEST_MANIFEST=(--manifest-path src-tauri/Cargo.toml --features dev-bins --locked)
  local name
  local portable=(
    stderr_log_accepts_windows
    pathext_
    excerpt_treats_crlf
    windows_cmd_not_recognized
    probe_finds_sh
    resolve_finds_a_binary
    a_name_with_separators
    an_absolute_path_is_taken
    extra_windows_dirs
  )
  local unix_only=(
    kill_group_reaps_grandchild
    snapshotted_env
    a_probe_that_times_out_takes_its_grandchildren
  )
  local windows_only=(
    kill_job_reaps_grandchild
    taskkill_fallback_reaps_grandchild
    a_probe_that_times_out_takes_its_grandchildren
  )

  if on_windows; then
    for name in "${windows_only[@]}" "${portable[@]}"; do
      run_required_lib_test "$name"
    done
  else
    for name in "${unix_only[@]}" "${portable[@]}"; do
      run_required_lib_test "$name"
    done
  fi

  run_required_integration_test acp_adapter shutdown_kills_adapter
  run_required_integration_test acp_adapter tasklist_csv
}

case "${1:-check}" in
  check) check ;;
  run) run_tests ;;
  -h|--help|help) usage ;;
  *) die "unknown command: $1 (try check|run)" ;;
esac
