#!/usr/bin/env bash
#
# Windows ACP adapter spawn + teardown (#285).
#
#   check  Offline contract: Job Objects are still the documented kill-tree,
#          cfg(unix) / cfg(windows) stay explicit, PATHEXT is consulted, and
#          CI still has a windows-latest job. Runs on Linux.
#   run    cargo test the spawn + teardown cases. On windows-latest that is
#          the Job Object proof; on Unix it is the process-group cases plus
#          the portable PATHEXT / CRLF tests. Needs a Rust toolchain.
#
# Not the full Windows verify (#286). This script is only the adapter
# lifecycle slice the issue asked for.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

usage() {
  sed -n '3,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
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

check() {
  need_file src-tauri/src/host/procgroup.rs
  need_file src-tauri/src/host/acp/spawn.rs
  need_file src-tauri/src/host/harness/mod.rs
  need_file .github/workflows/ci.yml

  need_text src-tauri/src/host/procgroup.rs 'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE'
  need_text src-tauri/src/host/procgroup.rs 'CREATE_NEW_PROCESS_GROUP'
  need_text src-tauri/src/host/procgroup.rs 'taskkill'
  need_text src-tauri/src/host/procgroup.rs '#[cfg(unix)]'
  need_text src-tauri/src/host/procgroup.rs '#[cfg(windows)]'
  need_text src-tauri/src/host/acp/spawn.rs 'kill_group_reaps_grandchild'
  need_text src-tauri/src/host/acp/spawn.rs 'kill_job_reaps_grandchild'
  # `cargo test --features dev-bins` builds jabot-hostd. The accept loop is
  # unix-only; the call site must stay behind the same cfg so Windows compiles.
  need_text src-tauri/src/bin/jabot-hostd.rs '#[cfg(unix)]'
  need_text src-tauri/src/bin/jabot-hostd.rs 'spawn_accept_loop'
  need_text src-tauri/src/host/harness/mod.rs 'PATHEXT'
  need_text src-tauri/src/host/harness/mod.rs 'resolve_in_dir'
  need_text .github/workflows/ci.yml 'windows-latest'
  need_text .github/workflows/ci.yml 'windows-adapter-lifecycle'

  # Unix process-group kill must still be the SIGTERM / SIGKILL pair, not a
  # Windows-shaped rewrite that leaked into the unix branch.
  grep -n 'SIGTERM\|SIGKILL' src-tauri/src/host/procgroup.rs >/dev/null \
    || die 'unix SIGTERM/SIGKILL path is gone from procgroup.rs'

  pass "windows adapter lifecycle contract"
}

run_tests() {
  # One cargo invocation per filter. A single `a|b|c` string is a libtest
  # regex only after `--`, and a cargo TESTNAME otherwise — keep them
  # separate so a typo in one name cannot silently skip the rest.
  local manifest=(--manifest-path src-tauri/Cargo.toml --features dev-bins --locked)
  local name
  for name in \
    kill_group_reaps_grandchild \
    kill_job_reaps_grandchild \
    stderr_log_accepts_windows \
    snapshotted_env \
    pathext_ \
    excerpt_treats_crlf \
    windows_cmd_not_recognized \
    probe_finds_sh \
    resolve_finds_a_binary \
    a_name_with_separators \
    an_absolute_path_is_taken
  do
    printf '> cargo test --lib %s\n' "$name"
    cargo test "${manifest[@]}" --lib -- "$name"
  done

  printf '> cargo test --test acp_adapter shutdown_kills_adapter\n'
  cargo test "${manifest[@]}" --test acp_adapter -- shutdown_kills_adapter
}

case "${1:-check}" in
  check) check ;;
  run) run_tests ;;
  -h|--help|help) usage ;;
  *) die "unknown command: $1 (try check|run)" ;;
esac
