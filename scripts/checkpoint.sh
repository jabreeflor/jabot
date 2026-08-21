#!/usr/bin/env bash
#
# Verify and commit atomically: commit only a tree that passed the gates and
# did not move while they ran.
#
#   ./scripts/checkpoint.sh -m "message"          # full gate, then commit
#   ./scripts/checkpoint.sh --fast -m "message"   # skip the e2e project
#   ./scripts/checkpoint.sh --quiet-for 120 ...   # require 120s of stillness first
#   ./scripts/checkpoint.sh --dry-run             # verify, report, commit nothing
#   ./scripts/checkpoint.sh --push -m "message"   # ...and push if it committed
#
# WHY THIS EXISTS
#
# `error TS6133: 'client' is declared but its value is never read` reached CI
# from a tree that had verified green. Nothing was wrong with the code that was
# checked; the file with the error was written *after* the check and before
# `git add -A`, minutes later. Verification takes ~1.5 minutes, so "verify,
# then stage" is verifying a moving target. Every long-running agent, watch
# task, formatter-on-save and second terminal is a writer racing that window.
#
# So the order here is: hash the tree, run the gates, hash again, refuse on any
# difference — and then commit the *index*, after proving the index's tree OID
# is the one that was verified. `git commit` without -a takes its content from
# the index, not from disk, so a write landing between that proof and the
# commit still cannot get in. The commit is checked against the verified OID
# afterwards as well, so even a concurrent `git add` is caught rather than
# assumed away.
#
# The tree hash is a real git tree object (scripts/lib/tree-hash.sh explains
# why porcelain output is not enough — it cannot see an in-place edit).
#
# EXIT CODES  (a caller in a loop can tell these apart)
#   0  committed (or --dry-run finished clean)
#   1  a gate failed — nothing committed
#   2  the tree or HEAD moved during verification — nothing committed
#   3  nothing to commit
#   4  usage or environment error (bad flag, not a worktree, git refused)
#   5  --quiet-for: the tree was still being written — gates never ran
#   6  committed, but the commit does not match the verified tree (see output)
set -uo pipefail

cd "$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)" || exit 4
# shellcheck source=lib/tree-hash.sh
. "scripts/lib/tree-hash.sh"

MESSAGE="Checkpoint: verified green, tree unchanged through verification"
VERIFY_ARGS=()
QUIET_FOR=0
DRY_RUN=0
PUSH=0
REMOTE=origin

die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 4; }

while [ $# -gt 0 ]; do
  case "$1" in
    -m|--message)  [ $# -ge 2 ] || die "$1 needs a value"; MESSAGE="$2"; shift 2 ;;
    --fast)        VERIFY_ARGS+=(--fast); shift ;;
    --quiet-for)   [ $# -ge 2 ] || die "$1 needs a value"; QUIET_FOR="$2"; shift 2 ;;
    --dry-run)     DRY_RUN=1; shift ;;
    --push)        PUSH=1; shift ;;
    --remote)      [ $# -ge 2 ] || die "$1 needs a value"; REMOTE="$2"; shift 2 ;;
    -h|--help)     sed -n '3,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)             die "unknown flag: $1 (try --help)" ;;
  esac
done
case "$QUIET_FOR" in (*[!0-9]*|'') die "--quiet-for wants a whole number of seconds, got '$QUIET_FOR'" ;; esac

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not inside a git worktree"
[ -x scripts/verify.sh ] || die "scripts/verify.sh is missing or not executable"

step() { printf '\n\033[1m> %s\033[0m\n' "$*"; }
note() { printf '  %s\n' "$*"; }
fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; }

moved() {
  # $1 before, $2 after — both real tree objects, so name the files.
  fail "REFUSING TO COMMIT: the working tree changed while the gates were running."
  fail "What was verified is not what would be committed. Nothing was staged or committed."
  git diff --name-status "$1" "$2" 2>/dev/null | sed 's/^/    /' >&2
  fail "Stop whatever else is writing here, then run this again."
}

# --- 1. the tree we are proposing to commit --------------------------------
TREE_BEFORE=$(jabot_tree_hash) || die "could not hash the working tree (unmerged paths from a conflict? finish the merge first)"
HEAD_BEFORE=$(git rev-parse --verify --quiet HEAD || printf 'none')

# --- 2. optional stillness window ------------------------------------------
# For unattended loops: if something is mid-write, verifying is wasted minutes.
if [ "$QUIET_FOR" -gt 0 ]; then
  step "waiting ${QUIET_FOR}s for the tree to go quiet"
  sleep "$QUIET_FOR"
  TREE_NOW=$(jabot_tree_hash) || die "could not hash the working tree"
  if [ "$TREE_NOW" != "$TREE_BEFORE" ]; then
    note "tree is still being written (${TREE_BEFORE:0:12} -> ${TREE_NOW:0:12}); not verifying yet"
    exit 5
  fi
  note "quiet"
fi

# --- 3. anything to do? ----------------------------------------------------
HEAD_TREE=$(jabot_head_tree_hash)
if [ "$TREE_BEFORE" = "$HEAD_TREE" ]; then
  note "working tree is identical to HEAD — nothing to commit"
  exit 3
fi

# --- 4. the gates ----------------------------------------------------------
step "verifying ${TREE_BEFORE:0:12} (scripts/verify.sh ${VERIFY_ARGS[*]-})"
VERIFY_OK=0
# An empty array must expand to *no* argument, not to one empty string —
# verify.sh rejects unknown flags, and "" is an unknown flag.
if [ ${#VERIFY_ARGS[@]} -eq 0 ]; then
  ./scripts/verify.sh || VERIFY_OK=$?
else
  ./scripts/verify.sh "${VERIFY_ARGS[@]}" || VERIFY_OK=$?
fi

# --- 5. did anything move while we did that? -------------------------------
TREE_AFTER=$(jabot_tree_hash) || die "could not hash the working tree"
HEAD_AFTER=$(git rev-parse --verify --quiet HEAD || printf 'none')

if [ "$TREE_AFTER" != "$TREE_BEFORE" ]; then
  step "result"
  moved "$TREE_BEFORE" "$TREE_AFTER"
  [ "$VERIFY_OK" -eq 0 ] || fail "(the gates also failed)"
  exit 2
fi
if [ "$HEAD_AFTER" != "$HEAD_BEFORE" ]; then
  step "result"
  fail "REFUSING TO COMMIT: HEAD moved during verification ($HEAD_BEFORE -> $HEAD_AFTER)."
  fail "Someone else committed, rebased or checked out here. Nothing was committed."
  exit 2
fi

if [ "$VERIFY_OK" -ne 0 ]; then
  step "result"
  fail "REFUSING TO COMMIT: scripts/verify.sh failed (exit $VERIFY_OK). Nothing was staged or committed."
  exit 1
fi

if [ "$DRY_RUN" -eq 1 ]; then
  step "result"
  note "gates passed and tree ${TREE_BEFORE:0:12} did not move — would commit (--dry-run, so nothing was)"
  exit 0
fi

# --- 6. commit the verified tree, and only that ----------------------------
step "committing ${TREE_BEFORE:0:12}"
git add -A || die "git add failed"
TREE_STAGED=$(git write-tree) || die "git write-tree failed"
if [ "$TREE_STAGED" != "$TREE_BEFORE" ]; then
  # A write landed between step 5 and here. The index is now something nobody
  # verified, so stop before it becomes a commit.
  moved "$TREE_BEFORE" "$TREE_STAGED"
  fail "(the index holds that unverified content now — \`git reset\` if you want it unstaged.)"
  exit 2
fi

git commit -q -m "$MESSAGE" || die "git commit failed"

# `git commit` builds from the index, so this should be impossible — which is
# why it is worth asserting. A commit hook that rewrites files, or another
# process staging in the same instant, would show up here and nowhere else.
TREE_COMMITTED=$(git rev-parse HEAD^{tree})
if [ "$TREE_COMMITTED" != "$TREE_BEFORE" ]; then
  fail "COMMITTED THE WRONG TREE: verified $TREE_BEFORE, committed $TREE_COMMITTED."
  fail "This commit was NOT verified. Inspect it before pushing:"
  fail "    git show --stat HEAD"
  exit 6
fi

note "committed $(git log --oneline -1)"
# scripts/verify.sh already noted this tree as verified (scripts/lib/tree-hash.sh),
# so .githooks/pre-push will let the push through without repeating the gates.

# --- 7. optional push ------------------------------------------------------
if [ "$PUSH" -eq 1 ]; then
  BRANCH=$(git rev-parse --abbrev-ref HEAD)
  step "pushing $BRANCH to $REMOTE"
  for attempt in 1 2 3; do
    if git push "$REMOTE" "$BRANCH"; then
      note "pushed"
      exit 0
    fi
    note "push attempt $attempt failed"
    [ "$attempt" -eq 3 ] || sleep 5
  done
  fail "push failed three times; the commit is local and verified"
  exit 1
fi
exit 0
