#!/usr/bin/env bash
#
# Decide which before-merge macOS lint jobs a change list must run.
#
#   ./scripts/macos-lint-needed.sh --base origin/main
#   ./scripts/macos-lint-needed.sh --paths src-tauri/src/notify/mac.rs
#   ./scripts/macos-lint-needed.sh --force-all
#
# Prints a human summary on stderr, then two machine lines on stdout:
#
#   notify=0|1
#   native=0|1
#
# `notify` is the existing Linux cross-check (scripts/check-mac-notify.sh).
# `native` is Clippy on a real Mac (scripts/check-macos-clippy.sh) for the
# modules that cannot be cross-checked. The path lists below are the
# inventory: a file that is not listed here is a deliberate exclusion from
# the paid macOS runner, not an oversight. See docs/macos-lint.md.
#
# --github-output also writes those keys to $GITHUB_OUTPUT for Actions.
# The default local verify.sh path never calls this; it stays offline.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)

FORCE_ALL=0
GITHUB_OUT=0
BASE=""
HEAD="HEAD"
PATHS=()

usage() {
  sed -n '3,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --force-all)     FORCE_ALL=1; shift ;;
    --github-output) GITHUB_OUT=1; shift ;;
    --base)          BASE="${2:-}"; shift 2 ;;
    --head)          HEAD="${2:-}"; shift 2 ;;
    --paths)         shift; PATHS+=("$@"); break ;;
    -h|--help)       usage; exit 0 ;;
    *) fail "unknown flag: $1 (try --help)" ;;
  esac
done

# ---------------------------------------------------------------------------
# The inventory. Prefixes end in `/`; everything else is an exact path.
# Keep docs/macos-lint.md in sync when you add a row.
# ---------------------------------------------------------------------------

# Linux scratch-crate Clippy of notify/ against x86_64-apple-darwin.
NOTIFY_PATHS=(
  src-tauri/src/notify/
  scripts/check-mac-notify.sh
  scripts/macos-lint-needed.sh
  scripts/tests/macos-lint.test.sh
  src-tauri/Cargo.toml
  src-tauri/Cargo.lock
  src-tauri/clippy.toml
  rust-toolchain.toml
  .github/workflows/ci.yml
)

# Native macOS Clippy of the jabot lib. Needed for cfg(macos) code that
# pulls Apple-toolchain crates (keyring, tauri-plugin-updater, tauri itself).
NATIVE_PATHS=(
  src-tauri/src/lib.rs
  src-tauri/src/host/store/secrets.rs
  scripts/check-macos-clippy.sh
  scripts/macos-lint-needed.sh
  scripts/tests/macos-lint.test.sh
  src-tauri/Cargo.toml
  src-tauri/Cargo.lock
  src-tauri/clippy.toml
  rust-toolchain.toml
  .github/workflows/ci.yml
)

matches_rule() {
  local path="$1" rule="$2"
  path="${path#./}"
  if [[ "$rule" == */ ]]; then
    [[ "$path" == "${rule}"* || "$path" == "${rule%/}" ]]
  else
    [[ "$path" == "$rule" ]]
  fi
}

matches_any() {
  local path="$1"
  shift
  local rule
  for rule in "$@"; do
    if matches_rule "$path" "$rule"; then
      return 0
    fi
  done
  return 1
}

collect_git_paths() {
  local base="$1" head="$2"
  command -v git >/dev/null 2>&1 || fail "git is required to discover the change list"
  git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
    fail "not a git worktree; pass --paths or --force-all"
  git -C "$ROOT" rev-parse --verify "$head^{commit}" >/dev/null 2>&1 ||
    fail "cannot resolve --head $head"
  git -C "$ROOT" rev-parse --verify "$base^{commit}" >/dev/null 2>&1 ||
    fail "cannot resolve --base $base

Fetch it first, e.g. git fetch origin ${base#origin/}"
  git -C "$ROOT" diff --name-only "$base...$head"
}

NOTIFY=0
NATIVE=0
REASON_NOTIFY=()
REASON_NATIVE=()

if [[ $FORCE_ALL -eq 1 ]]; then
  NOTIFY=1
  NATIVE=1
  REASON_NOTIFY+=("--force-all")
  REASON_NATIVE+=("--force-all")
else
  if [[ ${#PATHS[@]} -eq 0 ]]; then
    [[ -n "$BASE" ]] || fail "pass --base <ref>, --paths, or --force-all"
    while IFS= read -r line; do
      [[ -n "$line" ]] && PATHS+=("$line")
    done < <(collect_git_paths "$BASE" "$HEAD")
  fi
  if [[ ${#PATHS[@]} -gt 0 ]]; then
    for path in "${PATHS[@]}"; do
      path="${path#./}"
      [[ -n "$path" ]] || continue
      if matches_any "$path" "${NOTIFY_PATHS[@]}"; then
        NOTIFY=1
        REASON_NOTIFY+=("$path")
      fi
      if matches_any "$path" "${NATIVE_PATHS[@]}"; then
        NATIVE=1
        REASON_NATIVE+=("$path")
      fi
    done
  fi
fi

{
  printf 'macos lint plan\n'
  if [[ $NOTIFY -eq 1 ]]; then
    printf '  notify cross-check: RUN  (Linux, scripts/check-mac-notify.sh)\n'
    for p in "${REASON_NOTIFY[@]}"; do printf '    - %s\n' "$p"; done
  else
    printf '  notify cross-check: skip (no notify/check-config/shared-dep paths)\n'
  fi
  if [[ $NATIVE -eq 1 ]]; then
    printf '  native macOS clippy: RUN  (macOS, scripts/check-macos-clippy.sh --lib)\n'
    for p in "${REASON_NATIVE[@]}"; do printf '    - %s\n' "$p"; done
  else
    printf '  native macOS clippy: skip (no lib.rs/secrets.rs/shared-dep paths)\n'
  fi
} >&2

printf 'notify=%s\n' "$NOTIFY"
printf 'native=%s\n' "$NATIVE"

if [[ $GITHUB_OUT -eq 1 ]]; then
  [[ -n "${GITHUB_OUTPUT:-}" ]] || fail "--github-output needs GITHUB_OUTPUT"
  {
    printf 'notify=%s\n' "$NOTIFY"
    printf 'native=%s\n' "$NATIVE"
  } >> "$GITHUB_OUTPUT"
fi
