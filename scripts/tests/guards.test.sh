#!/usr/bin/env bash
#
# Tests for the commit/push guards: scripts/checkpoint.sh, .githooks/pre-push,
# scripts/install-hooks.sh, scripts/lib/tree-hash.sh.
#
# Every case builds a throwaway git repo under $TMPDIR, copies the real scripts
# into it, and gives it a *stub* scripts/verify.sh whose exit code and
# side-effects the test controls. The scripts under test are therefore the real
# ones, invoked exactly as they are in this repo, without spending 90 seconds
# on the real gates. Nothing here touches the repo it lives in.
#
# The interesting case is `tree_moved_in_place`: a tracked file rewritten
# during verification, same path, same length of `git status` output. If
# anyone replaces the tree hash with a hash of `git status --porcelain` — the
# obvious implementation — that case starts failing. A guard whose test cannot
# fail when the guard breaks is not a test.
#
#   ./scripts/tests/guards.test.sh            # all cases
#   ./scripts/tests/guards.test.sh tree_      # cases whose name contains tree_
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-guards.XXXXXX") || exit 1
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

# Keep the developer's git identity, hooks, templates and signing config out of
# this. GIT_CONFIG_* needs git >= 2.32; HOME covers anything older.
export HOME="$SANDBOX/home"
export XDG_CONFIG_HOME="$SANDBOX/home/.config"
export GIT_CONFIG_GLOBAL="$SANDBOX/home/gitconfig"
export GIT_CONFIG_SYSTEM=/dev/null
mkdir -p "$HOME"
: > "$GIT_CONFIG_GLOBAL"
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_AUTHOR_DATE GIT_COMMITTER_DATE 2>/dev/null || true

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

# --- a repo with the real scripts and a scriptable stub gate ---------------
new_repo() { # name -> echoes path
  local name="$1" d="$SANDBOX/$1"
  mkdir -p "$d/scripts/lib" "$d/.githooks" "$d/src"
  cp "$REPO_ROOT/scripts/checkpoint.sh"     "$d/scripts/"
  cp "$REPO_ROOT/scripts/install-hooks.sh"  "$d/scripts/"
  cp "$REPO_ROOT/scripts/lib/tree-hash.sh"  "$d/scripts/lib/"
  cp "$REPO_ROOT/.githooks/pre-push"        "$d/.githooks/"
  chmod +x "$d/scripts/checkpoint.sh" "$d/scripts/install-hooks.sh" "$d/.githooks/pre-push"

  # The stub gate. STUB_LOG lives outside the repo on purpose: a gate that
  # wrote inside the worktree would move the tree it is verifying.
  cat > "$d/scripts/verify.sh" <<'STUB'
#!/usr/bin/env bash
set -u
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)" || exit 99
printf 'ran[%s]\n' "$*" >> "$STUB_LOG"
if [ -n "${STUB_WRITE:-}" ]; then
  printf '%s' "${STUB_WRITE_CONTENT:-changed-by-the-gate}" > "$STUB_WRITE"
fi
if [ -n "${STUB_COMMIT:-}" ]; then
  git commit -q --allow-empty -m "someone else committed"
fi
# The real verify.sh ends this way: green run, tree still still, leave the note
# the pre-push hook reads. Same function, from the same library, so the stamp
# the hook is tested against is the stamp the real gate writes.
if [ "${STUB_EXIT:-0}" -eq 0 ] && [ -z "${STUB_NO_STAMP:-}" ]; then
  . scripts/lib/tree-hash.sh
  jabot_stamp_verified "$(jabot_tree_hash)" "${STUB_MODE:-full}"
fi
exit "${STUB_EXIT:-0}"
STUB
  chmod +x "$d/scripts/verify.sh"

  git -C "$d" init -q
  git -C "$d" config user.name "Guard Test"
  git -C "$d" config user.email "guard@example.invalid"
  git -C "$d" config commit.gpgsign false
  git -C "$d" symbolic-ref HEAD refs/heads/main
  printf 'seed\n' > "$d/src/seed.txt"
  git -C "$d" add -A
  git -C "$d" commit -q -m "seed"
  printf '%s' "$d"
}

run_checkpoint() { # dir [args...] -> sets OUT, RC
  local d="$1"; shift
  OUT=$(cd "$d" && ./scripts/checkpoint.sh "$@" 2>&1)
  RC=$?
}

head_of()   { git -C "$1" rev-parse HEAD; }
log_lines() { [ -f "$1" ] && wc -l < "$1" | tr -d ' ' || printf '0'; }

# ===========================================================================
# scripts/lib/tree-hash.sh
# ===========================================================================
case_tree_hash_sees_in_place_edits() {
  local d; d=$(new_repo tree_hash_edit)
  # shellcheck source=/dev/null
  . "$REPO_ROOT/scripts/lib/tree-hash.sh"
  local a b porcelain_a porcelain_b
  printf 'one\n' > "$d/src/seed.txt"
  a=$(cd "$d" && jabot_tree_hash); porcelain_a=$(git -C "$d" status --porcelain)
  printf 'two\n' > "$d/src/seed.txt"
  b=$(cd "$d" && jabot_tree_hash); porcelain_b=$(git -C "$d" status --porcelain)

  [ -n "$a" ] || { fail "tree hash was empty"; return; }
  if [ "$a" = "$b" ]; then fail "an in-place edit did not change the tree hash ($a)"; return; fi
  # The premise of the whole design: porcelain output is identical across that
  # edit, so a hash of it would have seen nothing.
  assert_eq "$porcelain_a" "$porcelain_b" "porcelain should be identical here (test premise)" || return
  pass
}

case_tree_hash_leaves_the_index_alone() {
  local d; d=$(new_repo tree_hash_index)
  . "$REPO_ROOT/scripts/lib/tree-hash.sh"
  printf 'unstaged\n' > "$d/src/seed.txt"
  local before after staged
  before=$(cd "$d" && git rev-parse HEAD^{tree})
  (cd "$d" && jabot_tree_hash >/dev/null)
  after=$(cd "$d" && git write-tree)
  staged=$(git -C "$d" diff --cached --name-only)
  assert_eq "$before" "$after" "hashing must not stage anything" || return
  assert_eq "" "$staged" "hashing must not stage anything" || return
  pass
}

# ===========================================================================
# scripts/checkpoint.sh
# ===========================================================================
case_commits_a_green_still_tree() {
  local d; d=$(new_repo happy)
  export STUB_LOG="$SANDBOX/happy.log" STUB_EXIT=0
  printf 'new work\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "checkpoint one"
  assert_eq 0 "$RC" "should have committed" || { fail "$OUT"; return; }
  assert_eq "checkpoint one" "$(git -C "$d" log -1 --pretty=%s)" "commit message" || return
  # what was committed is what was verified
  local committed live
  committed=$(git -C "$d" rev-parse HEAD^{tree})
  . "$REPO_ROOT/scripts/lib/tree-hash.sh"
  live=$(cd "$d" && jabot_tree_hash)
  assert_eq "$committed" "$live" "committed tree must equal the worktree it verified" || return
  assert_eq "" "$(git -C "$d" status --porcelain)" "worktree should be clean afterwards" || return
  assert_eq 1 "$(log_lines "$STUB_LOG")" "the gate should have run exactly once" || return
  [ -f "$d/.git/jabot-verified" ] || { fail "no verification stamp was written"; return; }
  pass
}

case_no_flag_passes_no_argument_to_verify() {
  local d; d=$(new_repo noflag)
  export STUB_LOG="$SANDBOX/noflag.log" STUB_EXIT=0
  printf 'x\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m m
  assert_eq 0 "$RC" "should have committed" || { fail "$OUT"; return; }
  # "ran[]" not "ran[ ]": an empty array expanded to an empty string would be
  # passed through as an unknown flag and verify.sh would exit 2.
  assert_eq "ran[]" "$(cat "$STUB_LOG")" "verify.sh must be called with no arguments" || return
  pass
}

case_fast_flag_reaches_verify() {
  local d; d=$(new_repo fastflag)
  export STUB_LOG="$SANDBOX/fastflag.log" STUB_EXIT=0
  printf 'x\n' > "$d/src/work.txt"
  run_checkpoint "$d" --fast -m m
  assert_eq 0 "$RC" "should have committed" || { fail "$OUT"; return; }
  assert_eq "ran[--fast]" "$(cat "$STUB_LOG")" "--fast must reach verify.sh" || return
  pass
}

case_refuses_when_the_gate_fails() {
  local d; d=$(new_repo redgate)
  export STUB_LOG="$SANDBOX/redgate.log" STUB_EXIT=1
  local before; before=$(head_of "$d")
  printf 'broken\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "should not exist"
  assert_eq 1 "$RC" "a failing gate must exit 1" || { fail "$OUT"; return; }
  assert_eq "$before" "$(head_of "$d")" "HEAD must not move when the gate fails" || return
  assert_eq "" "$(git -C "$d" diff --cached --name-only)" "nothing may be left staged" || return
  assert_contains "$OUT" "REFUSING TO COMMIT" "should say why" || return
  pass
}

case_tree_moved_in_place() {
  # The regression that reached CI: a tracked file rewritten while the gates
  # ran. Same path, same `git status` line — invisible to anything that hashes
  # status output.
  local d; d=$(new_repo movedinplace)
  export STUB_LOG="$SANDBOX/movedinplace.log" STUB_EXIT=0
  printf 'verified content\n' > "$d/src/work.txt"
  local before; before=$(head_of "$d")
  export STUB_WRITE="$d/src/work.txt" STUB_WRITE_CONTENT="written during verification"
  run_checkpoint "$d" -m "should not exist"
  unset STUB_WRITE STUB_WRITE_CONTENT
  assert_eq 2 "$RC" "a tree that moved must exit 2" || { fail "$OUT"; return; }
  assert_eq "$before" "$(head_of "$d")" "HEAD must not move" || return
  assert_eq "" "$(git -C "$d" diff --cached --name-only)" "nothing may be left staged" || return
  assert_contains "$OUT" "changed while the gates were running" "should say what happened" || return
  assert_contains "$OUT" "src/work.txt" "should name the file that moved" || return
  pass
}

case_tree_moved_new_file() {
  local d; d=$(new_repo movednew)
  export STUB_LOG="$SANDBOX/movednew.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  local before; before=$(head_of "$d")
  export STUB_WRITE="$d/src/sneaked-in.txt"
  run_checkpoint "$d" -m "should not exist"
  unset STUB_WRITE
  assert_eq 2 "$RC" "a new file during verification must exit 2" || { fail "$OUT"; return; }
  assert_eq "$before" "$(head_of "$d")" "HEAD must not move" || return
  assert_contains "$OUT" "src/sneaked-in.txt" "should name the new file" || return
  [ -f "$d/src/sneaked-in.txt" ] || { fail "the test did not actually create the file"; return; }
  pass
}

case_head_moved_during_verification() {
  local d; d=$(new_repo headmoved)
  export STUB_LOG="$SANDBOX/headmoved.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  export STUB_COMMIT=1
  run_checkpoint "$d" -m "should not exist"
  unset STUB_COMMIT
  assert_eq 2 "$RC" "HEAD moving must exit 2" || { fail "$OUT"; return; }
  assert_contains "$OUT" "HEAD moved during verification" "should say why" || return
  assert_eq "someone else committed" "$(git -C "$d" log -1 --pretty=%s)" "the other commit stands" || return
  pass
}

case_nothing_to_commit() {
  local d; d=$(new_repo nothing)
  export STUB_LOG="$SANDBOX/nothing.log" STUB_EXIT=0
  run_checkpoint "$d" -m "nope"
  assert_eq 3 "$RC" "a clean tree must exit 3" || { fail "$OUT"; return; }
  assert_eq 0 "$(log_lines "$STUB_LOG")" "must not spend 90s verifying nothing" || return
  pass
}

case_dry_run_commits_nothing() {
  local d; d=$(new_repo dryrun)
  export STUB_LOG="$SANDBOX/dryrun.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  local before; before=$(head_of "$d")
  run_checkpoint "$d" --dry-run
  assert_eq 0 "$RC" "--dry-run on a green tree exits 0" || { fail "$OUT"; return; }
  assert_eq "$before" "$(head_of "$d")" "--dry-run must not commit" || return
  assert_eq 1 "$(log_lines "$STUB_LOG")" "--dry-run still runs the gate" || return
  pass
}

case_quiet_for_waits_out_a_writer() {
  local d; d=$(new_repo quietbusy)
  export STUB_LOG="$SANDBOX/quietbusy.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  ( sleep 0.4; printf 'still typing\n' > "$d/src/work.txt" ) &
  local writer=$!
  run_checkpoint "$d" --quiet-for 1 -m "should not exist"
  wait "$writer" 2>/dev/null
  assert_eq 5 "$RC" "a tree still being written must exit 5" || { fail "$OUT"; return; }
  assert_eq 0 "$(log_lines "$STUB_LOG")" "the gate must not run on a moving tree" || return
  pass
}

case_quiet_for_proceeds_on_a_still_tree() {
  local d; d=$(new_repo quietstill)
  export STUB_LOG="$SANDBOX/quietstill.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" --quiet-for 1 -m "quiet then green"
  assert_eq 0 "$RC" "a still tree should proceed" || { fail "$OUT"; return; }
  assert_eq "quiet then green" "$(git -C "$d" log -1 --pretty=%s)" "should have committed" || return
  pass
}

case_rejects_unknown_flags() {
  local d; d=$(new_repo badflag)
  export STUB_LOG="$SANDBOX/badflag.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" --commit-anyway
  assert_eq 4 "$RC" "unknown flags must exit 4, not be ignored" || { fail "$OUT"; return; }
  assert_eq 0 "$(log_lines "$STUB_LOG")" "must not run the gate on a usage error" || return
  pass
}

# ===========================================================================
# scripts/install-hooks.sh
# ===========================================================================
case_install_hooks_sets_hookspath() {
  local d; d=$(new_repo installhooks)
  ( cd "$d" && ./scripts/install-hooks.sh --check >/dev/null 2>&1 )
  assert_eq 1 "$?" "--check must fail before installation" || return
  local out; out=$(cd "$d" && ./scripts/install-hooks.sh 2>&1)
  assert_eq ".githooks" "$(git -C "$d" config --local --get core.hooksPath)" "core.hooksPath" || return
  assert_contains "$out" "pre-push" "should say what it installed" || return
  ( cd "$d" && ./scripts/install-hooks.sh --check >/dev/null 2>&1 )
  assert_eq 0 "$?" "--check must pass after installation" || return
  ( cd "$d" && ./scripts/install-hooks.sh --uninstall >/dev/null 2>&1 )
  assert_eq "" "$(git -C "$d" config --local --get core.hooksPath 2>/dev/null)" "--uninstall clears it" || return
  pass
}

case_install_hooks_fixes_a_non_executable_hook() {
  # git silently ignores a hook without +x. That is the one failure mode a
  # guard must not have.
  local d; d=$(new_repo hookperm)
  chmod -x "$d/.githooks/pre-push"
  ( cd "$d" && ./scripts/install-hooks.sh >/dev/null 2>&1 )
  [ -x "$d/.githooks/pre-push" ] || { fail "install left pre-push non-executable"; return; }
  pass
}

# ===========================================================================
# .githooks/pre-push
# ===========================================================================
with_remote() { # dir -> adds a bare remote and installs hooks
  local d="$1"
  git init -q --bare "$d.remote.git"
  git -C "$d" remote add origin "$d.remote.git"
  ( cd "$d" && ./scripts/install-hooks.sh --quiet >/dev/null 2>&1 )
}

case_pre_push_refuses_a_failing_push() {
  local d; d=$(new_repo pushred)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushred.log" STUB_EXIT=1
  printf 'work\n' > "$d/src/work.txt"
  git -C "$d" add -A && git -C "$d" commit -q -m "unverified work"
  local out rc
  out=$(cd "$d" && git push origin main 2>&1); rc=$?
  [ "$rc" -ne 0 ] || { fail "the push succeeded despite a failing gate: $out"; return; }
  assert_contains "$out" "REFUSING THE PUSH" "should explain itself" || return
  assert_eq "" "$(git -C "$d.remote.git" rev-parse --quiet --verify refs/heads/main 2>/dev/null)" \
    "nothing may reach the remote" || return
  assert_eq 1 "$(log_lines "$STUB_LOG")" "the hook should have run the gate once" || return
  pass
}

case_pre_push_allows_a_passing_push() {
  local d; d=$(new_repo pushgreen)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushgreen.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  git -C "$d" add -A && git -C "$d" commit -q -m "verified work"
  local out rc
  out=$(cd "$d" && git push origin main 2>&1); rc=$?
  assert_eq 0 "$rc" "a passing gate must let the push through: $out" || return
  assert_eq "$(head_of "$d")" "$(git -C "$d.remote.git" rev-parse refs/heads/main)" "remote should have it" || return
  pass
}

case_pre_push_honours_no_verify() {
  local d; d=$(new_repo pushnoverify)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushnoverify.log" STUB_EXIT=1
  printf 'work\n' > "$d/src/work.txt"
  git -C "$d" add -A && git -C "$d" commit -q -m "emergency"
  local rc
  ( cd "$d" && git push --no-verify origin main >/dev/null 2>&1 ); rc=$?
  assert_eq 0 "$rc" "--no-verify is the documented escape hatch and must work" || return
  assert_eq 0 "$(log_lines "$STUB_LOG")" "--no-verify must skip the gate entirely" || return
  pass
}

case_pre_push_ignores_branch_deletion() {
  local d; d=$(new_repo pushdelete)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushdelete.log" STUB_EXIT=0
  git -C "$d" push -q origin main 2>/dev/null
  git -C "$d" push -q origin main:refs/heads/scratch 2>/dev/null
  : > "$STUB_LOG"
  local rc
  ( cd "$d" && git push origin --delete scratch >/dev/null 2>&1 ); rc=$?
  assert_eq 0 "$rc" "deleting a branch should not be blocked" || return
  assert_eq 0 "$(log_lines "$STUB_LOG")" "a deletion pushes no content and must not verify" || return
  pass
}

case_pre_push_trusts_a_fresh_matching_stamp() {
  local d; d=$(new_repo pushstamp)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushstamp.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "verified by checkpoint"
  assert_eq 0 "$RC" "checkpoint should have committed" || { fail "$OUT"; return; }
  assert_eq 1 "$(log_lines "$STUB_LOG")" "one gate run so far" || return
  local out rc
  out=$(cd "$d" && git push origin main 2>&1); rc=$?
  assert_eq 0 "$rc" "the push should succeed: $out" || return
  assert_eq 1 "$(log_lines "$STUB_LOG")" "the hook must not re-run gates on bytes just verified" || return
  assert_contains "$out" "Not re-running the gates" "should say why it skipped" || return
  pass
}

case_pre_push_distrusts_a_stamp_once_the_tree_moves() {
  local d; d=$(new_repo pushstampdirty)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushstampdirty.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "verified by checkpoint"
  assert_eq 0 "$RC" "checkpoint should have committed" || { fail "$OUT"; return; }
  printf 'edited after verification\n' > "$d/src/work.txt"
  ( cd "$d" && git push origin main >/dev/null 2>&1 )
  assert_eq 2 "$(log_lines "$STUB_LOG")" "a moved worktree must invalidate the note and re-verify" || return
  pass
}

case_pre_push_distrusts_a_stamp_for_another_commit() {
  # Worktree still matches the note, but the ref being pushed does not.
  local d; d=$(new_repo pushstampotherref)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushstampotherref.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "verified by checkpoint"
  git -C "$d" branch older HEAD~1
  ( cd "$d" && git push origin older >/dev/null 2>&1 )
  assert_eq 2 "$(log_lines "$STUB_LOG")" "pushing a ref whose tree was never verified must re-verify" || return
  pass
}

case_pre_push_distrusts_a_fast_stamp_for_a_full_push() {
  local d; d=$(new_repo pushstampfast)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushstampfast.log" STUB_EXIT=0 STUB_MODE=fast
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" --fast -m "verified fast"
  unset STUB_MODE
  assert_eq 0 "$RC" "checkpoint should have committed" || { fail "$OUT"; return; }
  ( cd "$d" && git push origin main >/dev/null 2>&1 )
  assert_eq 2 "$(log_lines "$STUB_LOG")" "a --fast run skipped e2e and cannot stand in for a full push" || return
  # ...but it is enough for an explicitly fast push.
  local d2; d2=$(new_repo pushstampfast2)
  with_remote "$d2"
  export STUB_LOG="$SANDBOX/pushstampfast2.log" STUB_MODE=fast
  printf 'work\n' > "$d2/src/work.txt"
  run_checkpoint "$d2" --fast -m "verified fast"
  unset STUB_MODE
  ( cd "$d2" && JABOT_PREPUSH=fast git push origin main >/dev/null 2>&1 )
  assert_eq 1 "$(log_lines "$STUB_LOG")" "a fast note satisfies an explicitly fast push" || return
  pass
}

case_pre_push_distrusts_an_expired_stamp() {
  # The gates are not a pure function of the content: rust-toolchain.toml
  # tracks `stable` and clippy gains lints (D-014). A day-old green is not a
  # promise about today's compiler.
  local d; d=$(new_repo pushstampold)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushstampold.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "verified by checkpoint"
  local tree older
  tree=$(git -C "$d" rev-parse HEAD^{tree})
  older=$(( $(date -u +%s) - 90000 ))
  printf 'tree %s %s full\n' "$tree" "$older" > "$d/.git/jabot-verified"
  ( cd "$d" && git push origin main >/dev/null 2>&1 )
  assert_eq 2 "$(log_lines "$STUB_LOG")" "a note older than 24h must not be trusted" || return
  pass
}

case_pre_push_ignores_a_corrupt_stamp() {
  local d; d=$(new_repo pushstampjunk)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushstampjunk.log" STUB_EXIT=0
  printf 'work\n' > "$d/src/work.txt"
  run_checkpoint "$d" -m "verified by checkpoint"
  printf 'garbage not a stamp\n' > "$d/.git/jabot-verified"
  local rc
  ( cd "$d" && git push origin main >/dev/null 2>&1 ); rc=$?
  assert_eq 0 "$rc" "an unreadable note must fall back to verifying, not crash the push" || return
  assert_eq 2 "$(log_lines "$STUB_LOG")" "an unreadable note must fall back to verifying" || return
  pass
}

case_pre_push_refuses_without_a_gate() {
  local d; d=$(new_repo pushnogate)
  with_remote "$d"
  export STUB_LOG="$SANDBOX/pushnogate.log" STUB_EXIT=0
  rm -f "$d/scripts/verify.sh"
  printf 'work\n' > "$d/src/work.txt"
  git -C "$d" add -A && git -C "$d" commit -q -m "work"
  local out rc
  out=$(cd "$d" && git push origin main 2>&1); rc=$?
  [ "$rc" -ne 0 ] || { fail "pushed with no gate present: $out"; return; }
  assert_contains "$out" "refusing to push blind" "should explain itself" || return
  pass
}

# ===========================================================================
CASES=$(grep -o '^case_[a-z0-9_]*' "${BASH_SOURCE[0]}" | sort -u)
printf '\n\033[1mguards\033[0m (%s)\n' "$SANDBOX"
for c in $CASES; do
  case "$c" in *"$FILTER"*) ;; *) continue ;; esac
  CASE="${c#case_}"
  COUNT=$((COUNT + 1))
  "$c"
done

printf '\n'
if [ "$FAILURES" -eq 0 ]; then
  printf '\033[32m%d guard case(s) passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d of %d guard case(s) failed\033[0m\n' "$FAILURES" "$COUNT"
exit 1
