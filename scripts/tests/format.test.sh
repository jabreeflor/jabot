#!/usr/bin/env bash
#
# Tests for the frontend formatter contract: scripts/frontend-format.sh,
# prettier.config.mjs, and .prettierignore.
#
# The interesting cases are the ones the ignore file claims: a misformatted
# file under plugins/, src-tauri/vendor/, node_modules/, dist/, or a nested
# worktree must stay untouched when --write runs, and a misformatted
# first-party file must fail --check and become clean after --write. A
# formatter whose test cannot fail when the ignore list is deleted is not
# a test.
#
# Every case builds a throwaway tree under $TMPDIR, copies the real config
# and ignore file into it, and invokes the real script with PRETTIER_ROOT
# pointed at that tree. Nothing here touches the repo it lives in.
#
#   ./scripts/tests/format.test.sh              # all cases
#   ./scripts/tests/format.test.sh ignores_     # cases whose name contains ignores_
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
FORMATTER="$REPO_ROOT/scripts/frontend-format.sh"
PRETTIER="$REPO_ROOT/node_modules/.bin/prettier"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-format.XXXXXX") || exit 1
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

pass() { printf '  \033[32mok\033[0m   %s\n' "$CASE"; }
fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$CASE"
  printf '       %s\n' "$@"
  FAILURES=$((FAILURES + 1))
}
assert_eq() { # expected actual label
  if [ "$1" != "$2" ]; then
    fail "$3: expected [$1], got [$2]"
    return 1
  fi
}
assert_contains() { # haystack needle label
  case "$1" in
    *"$2"*) return 0 ;;
    *) fail "$3: [$2] not found in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-400)"; return 1 ;;
  esac
}

# A tree with the real config/ignore and planted first-party + excluded files.
new_tree() { # name -> echoes path
  local name="$1" d="$SANDBOX/$1"
  mkdir -p "$d/src" \
    "$d/plugins/jabstack" \
    "$d/.agents/plugins" \
    "$d/src-tauri/vendor/adapters" \
    "$d/src-tauri/gen/schemas" \
    "$d/node_modules/left-pad" \
    "$d/dist" \
    "$d/.worktrees/nested" \
    "$d/.claude/worktrees/agent"
  cp "$REPO_ROOT/prettier.config.mjs" "$d/"
  cp "$REPO_ROOT/.prettierignore" "$d/"
  printf '%s\n' "$d"
}

# Not in a command substitution: `$(cat)` / `$(fn)` would drop trailing
# newlines and hide RUN_OUT from the caller (`set -u` then dies).
RUN_OUT=""
RUN_RC=0
run_fmt() { # tree --check|--write
  RUN_OUT="$SANDBOX/out.$$"
  # The script's prettier binary lives in the real repo; only the tree is swapped.
  PRETTIER_ROOT="$1" PRETTIER_BIN="$PRETTIER" \
    "$FORMATTER" "$2" >"$RUN_OUT" 2>&1
  RUN_RC=$?
}

# Distinctively ugly: no spaces around `=`, no trailing newline, extra blanks.
UGLY_TS=$'export const n=1;\n\n\n'
UGLY_JSON=$'{"a":1,"b":2}'
PRETTY_TS=$'export const n = 1;\n'
PRETTY_JSON=$'{\n  "a": 1,\n  "b": 2\n}\n'
# Short JSON that is not package.json stays one line (printWidth 80).
PRETTY_JSON_INLINE=$'{ "a": 1, "b": 2 }\n'

assert_file_eq() { # expected_contents path label
  local exp="$SANDBOX/expected.$$"
  printf '%s' "$1" > "$exp"
  if ! cmp -s "$exp" "$2"; then
    fail "$3: expected [$(od -An -tx1 "$exp" | tr -s ' ')], got [$(od -An -tx1 "$2" | tr -s ' ')]"
    return 1
  fi
}

run_case() { # name fn
  CASE="$1"
  if [ -n "$FILTER" ] && [[ "$CASE" != *"$FILTER"* ]]; then return; fi
  COUNT=$((COUNT + 1))
  if "$2"; then pass; fi
}

# ---------------------------------------------------------------------------

check_fails_on_misformatted_ts() {
  local d out
  d=$(new_tree check-ts)
  printf '%s' "$UGLY_TS" > "$d/src/probe.ts"
  run_fmt "$d" --check
  out=$(cat "$RUN_OUT")
  if [ "$RUN_RC" = "0" ]; then
    fail "misformatted src/probe.ts passed --check"$'\n'"$out"
    return 1
  fi
  assert_contains "$out" "src/probe.ts" "check output must name the dirty file" || return
}

write_restores_pass() {
  local d
  d=$(new_tree write-restore)
  printf '%s' "$UGLY_TS" > "$d/src/probe.ts"
  run_fmt "$d" --check
  if [ "$RUN_RC" = "0" ]; then
    fail "setup: ugly file already passed --check"
    return 1
  fi
  run_fmt "$d" --write
  if [ "$RUN_RC" != "0" ]; then
    fail "--write failed:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  run_fmt "$d" --check
  if [ "$RUN_RC" != "0" ]; then
    fail "--check still dirty after --write:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  assert_file_eq "$PRETTY_TS" "$d/src/probe.ts" "write must produce Prettier output"
}

formats_first_party_json() {
  local d
  d=$(new_tree json)
  printf '%s' "$UGLY_JSON" > "$d/package.json"
  run_fmt "$d" --write
  if [ "$RUN_RC" != "0" ]; then
    fail "--write failed:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  assert_file_eq "$PRETTY_JSON" "$d/package.json" "first-party JSON must be formatted"
}

ignores_stays_ugly() { # dir relpath
  local d="$1" rel="$2"
  printf '%s' "$UGLY_TS" > "$d/$rel"
  cp "$d/$rel" "$SANDBOX/before.$$"
  run_fmt "$d" --write
  if [ "$RUN_RC" != "0" ]; then
    fail "--write failed while $rel should have been ignored:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  if ! cmp -s "$SANDBOX/before.$$" "$d/$rel"; then
    fail "$rel must be left untouched"
    return 1
  fi
  run_fmt "$d" --check
  if [ "$RUN_RC" != "0" ]; then
    fail "--check failed because ignored $rel was still considered:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
}

ignores_node_modules() {
  local d
  d=$(new_tree ign-nm)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  ignores_stays_ugly "$d" "node_modules/left-pad/index.ts"
}

ignores_plugins() {
  local d
  d=$(new_tree ign-plugins)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  ignores_stays_ugly "$d" "plugins/jabstack/vendored.ts"
}

formats_agents_plugins_json() {
  local d
  d=$(new_tree agents-json)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  printf '%s' "$UGLY_JSON" > "$d/.agents/plugins/marketplace.json"
  run_fmt "$d" --write
  if [ "$RUN_RC" != "0" ]; then
    fail "--write failed:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  assert_file_eq "$PRETTY_JSON_INLINE" "$d/.agents/plugins/marketplace.json" \
    ".agents/plugins JSON is first-party and must be formatted"
}

ignores_vendor_adapters() {
  local d
  d=$(new_tree ign-vendor)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  ignores_stays_ugly "$d" "src-tauri/vendor/adapters/adapter.ts"
}

ignores_nested_worktrees() {
  local d
  d=$(new_tree ign-wt)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  ignores_stays_ugly "$d" ".worktrees/nested/checkout.ts" || return
  ignores_stays_ugly "$d" ".claude/worktrees/agent/checkout.ts"
}

ignores_dist() {
  local d
  d=$(new_tree ign-dist)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  ignores_stays_ugly "$d" "dist/bundle.js"
}

ignores_tauri_gen() {
  local d
  d=$(new_tree ign-gen)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  ignores_stays_ugly "$d" "src-tauri/gen/schemas/desktop-schema.json"
}

ignores_package_lock() {
  local d
  d=$(new_tree ign-lock)
  printf '%s' $'export const ok = 1;\n' > "$d/src/ok.ts"
  printf '%s' "$UGLY_JSON" > "$d/package-lock.json"
  cp "$d/package-lock.json" "$SANDBOX/lock-before.$$"
  run_fmt "$d" --write
  if [ "$RUN_RC" != "0" ]; then
    fail "--write failed:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  if ! cmp -s "$SANDBOX/lock-before.$$" "$d/package-lock.json"; then
    fail "package-lock.json must be left untouched"
    return 1
  fi
}

refuses_unknown_flag() {
  local rc
  RUN_OUT="$SANDBOX/out-flag.$$"
  "$FORMATTER" --pretty-please >"$RUN_OUT" 2>&1
  rc=$?
  if [ "$rc" != "2" ]; then
    fail "unknown flag should exit 2, got $rc:"$'\n'"$(cat "$RUN_OUT")"
    return 1
  fi
  assert_contains "$(cat "$RUN_OUT")" "usage:" "unknown flag must print usage"
}

# ---------------------------------------------------------------------------

if [[ ! -x "$PRETTIER" ]]; then
  printf 'prettier is not installed at %s — run npm install\n' "$PRETTIER" >&2
  exit 1
fi
if [[ ! -x "$FORMATTER" ]]; then
  printf '%s is missing or not executable\n' "$FORMATTER" >&2
  exit 1
fi

printf 'format contract\n'
run_case "check_fails_on_misformatted_ts" check_fails_on_misformatted_ts
run_case "write_restores_pass"             write_restores_pass
run_case "formats_first_party_json"        formats_first_party_json
run_case "ignores_node_modules"            ignores_node_modules
run_case "ignores_plugins"                 ignores_plugins
run_case "formats_agents_plugins_json"     formats_agents_plugins_json
run_case "ignores_vendor_adapters"         ignores_vendor_adapters
run_case "ignores_nested_worktrees"        ignores_nested_worktrees
run_case "ignores_dist"                    ignores_dist
run_case "ignores_tauri_gen"               ignores_tauri_gen
run_case "ignores_package_lock"            ignores_package_lock
run_case "refuses_unknown_flag"            refuses_unknown_flag

printf '\n'
if [ "$FAILURES" -eq 0 ]; then
  printf '\033[32m%d passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d failed / %d run\033[0m\n' "$FAILURES" "$COUNT"
exit 1
