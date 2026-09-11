#!/usr/bin/env bash
#
# Decide whether a change list must start the Windows verify job.
#
#   ./scripts/windows-needed.sh --base origin/main
#   ./scripts/windows-needed.sh --paths src-tauri/src/lib.rs
#   ./scripts/windows-needed.sh --force-verify
#
# Prints a human summary on stderr, then one machine line on stdout:
#
#   verify=0|1
#
# The Windows runner is 2x Linux. Docs-only and renderer-only PRs must not
# start it. Packaging is a separate job gated by label / dispatch in
# .github/workflows/windows.yml — this script does not turn that on.
# See docs/windows-ci.md.
#
# --github-output also writes the key to $GITHUB_OUTPUT for Actions.
# The default local verify.sh path never calls this; it stays offline.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)

FORCE_VERIFY=0
GITHUB_OUT=0
BASE=""
HEAD="HEAD"
PATHS=()

usage() {
  sed -n '3,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --force-verify)  FORCE_VERIFY=1; shift ;;
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
# Keep docs/windows-ci.md in sync when you add a row.
# ---------------------------------------------------------------------------

# Compile + portable cargo test on windows-latest.
VERIFY_PATHS=(
  src-tauri/
  rust-toolchain.toml
  scripts/windows-verify.sh
  scripts/windows-needed.sh
  scripts/windows-secrets-check.sh
  scripts/tests/windows-ci.test.sh
  .github/workflows/windows.yml
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
    fail "not a git worktree; pass --paths or --force-verify"
  git -C "$ROOT" rev-parse --verify "$head^{commit}" >/dev/null 2>&1 ||
    fail "cannot resolve --head $head"
  git -C "$ROOT" rev-parse --verify "$base^{commit}" >/dev/null 2>&1 ||
    fail "cannot resolve --base $base

Fetch it first, e.g. git fetch origin ${base#origin/}"
  git -C "$ROOT" diff --name-only "$base...$head"
}

VERIFY=0
REASON_VERIFY=()

if [[ $FORCE_VERIFY -eq 1 ]]; then
  VERIFY=1
  REASON_VERIFY+=("--force-verify")
else
  if [[ ${#PATHS[@]} -eq 0 ]]; then
    [[ -n "$BASE" ]] || fail "pass --base <ref>, --paths, or --force-verify"
    # Capture first so a failed `git diff` is a red planner, not verify=0.
    collected=$(collect_git_paths "$BASE" "$HEAD")
    while IFS= read -r line; do
      [[ -n "$line" ]] && PATHS+=("$line")
    done <<< "$collected"
  fi
  if [[ ${#PATHS[@]} -gt 0 ]]; then
    for path in "${PATHS[@]}"; do
      path="${path#./}"
      [[ -n "$path" ]] || continue
      if matches_any "$path" "${VERIFY_PATHS[@]}"; then
        VERIFY=1
        REASON_VERIFY+=("$path")
      fi
    done
  fi
fi

{
  printf 'windows ci plan\n'
  if [[ $VERIFY -eq 1 ]]; then
    printf '  windows verify: RUN  (windows-latest, scripts/windows-verify.sh)\n'
    for p in "${REASON_VERIFY[@]}"; do printf '    - %s\n' "$p"; done
  else
    printf '  windows verify: skip (no host/CI/toolchain paths)\n'
  fi
  printf '  windows package: decided by the workflow (windows-package label / dispatch)\n'
} >&2

printf 'verify=%s\n' "$VERIFY"

if [[ $GITHUB_OUT -eq 1 ]]; then
  [[ -n "${GITHUB_OUTPUT:-}" ]] || fail "--github-output needs GITHUB_OUTPUT"
  printf 'verify=%s\n' "$VERIFY" >> "$GITHUB_OUTPUT"
fi
