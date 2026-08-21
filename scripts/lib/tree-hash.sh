# shellcheck shell=bash
#
# jabot_tree_hash — the git tree OID of exactly what a commit would capture.
#
# Why not `git status --porcelain | sha256sum`, which is the obvious thing:
# porcelain lines are (status, path). Editing a tracked file in place changes
# nothing about that line — same path, same ` M`. So a hash of porcelain output
# cannot see the most common way a working tree moves, which is the exact thing
# a "did the tree move while we were verifying?" guard has to see. It also
# cannot tell you *what* moved.
#
# This asks git instead. It stages the whole worktree into a throwaway index —
# never the real one, so a refusal leaves the caller's staging area untouched —
# and writes the tree object. Two identical OIDs mean byte-identical content
# under every committable path; two different ones can be diffed by name
# (`git diff --name-status $a $b`) because both trees are real objects.
#
# Scope, stated plainly: this covers what git would commit. Files matched by
# .gitignore (node_modules/, dist/, src-tauri/target/) are invisible to it, so
# a gate that depends on those can still be fed something the guard did not
# watch. Everything that can reach a commit is covered, which is what the guard
# is for.
#
# Cost: the copied index carries git's stat cache, so unchanged files are not
# re-read. Under 100ms on this repo.
#
# Usage:  oid=$(jabot_tree_hash) || die "not a git worktree"
# Works from any directory inside the worktree.

jabot_tree_hash() {
  local idx real top oid

  top=$(git rev-parse --show-toplevel 2>/dev/null) || return 1
  idx=$(mktemp "${TMPDIR:-/tmp}/jabot-index.XXXXXX") || return 1
  rm -f "$idx"

  # Seeding from the real index keeps the stat cache (fast) and inherits any
  # skip-worktree bits, so the hash matches what a commit here would produce.
  # A repo mid-clone or with no index yet falls back to an empty one.
  real=$(git rev-parse --git-path index 2>/dev/null)
  if [ -z "$real" ] || ! cp "$real" "$idx" 2>/dev/null; then
    GIT_INDEX_FILE="$idx" git read-tree --empty 2>/dev/null || { rm -f "$idx"; return 1; }
  fi

  if ! GIT_INDEX_FILE="$idx" git add -A -- "$top" >/dev/null 2>&1; then
    rm -f "$idx"; return 1
  fi
  oid=$(GIT_INDEX_FILE="$idx" git write-tree 2>/dev/null) || { rm -f "$idx"; return 1; }
  rm -f "$idx"

  [ -n "$oid" ] || return 1
  printf '%s\n' "$oid"
}

# The tree OID already recorded at HEAD, or the empty-tree OID on a repo with
# no commits yet. Comparing this against jabot_tree_hash answers "is there
# anything to commit?" by content rather than by `git status` output, which
# also reports paths whose content did not actually change.
jabot_head_tree_hash() {
  git rev-parse --verify --quiet HEAD^{tree} 2>/dev/null && return 0
  git hash-object -t tree /dev/null 2>/dev/null
}

# ---------------------------------------------------------------------------
# The "this exact content passed the gates" note.
#
# scripts/verify.sh writes it when every gate passed AND the tree did not move
# while they ran. .githooks/pre-push reads it, and skips re-running a 90-second
# gate when the content it is about to push is byte-identical to content that
# has just been through it. Content-addressed by tree OID, so it can never
# vouch for anything but the exact bytes that were checked.
#
# It lives in .git/ — never tracked, never pushed, never shared. One clone's
# note about one clone's run.
#
#   tree <oid> <epoch-seconds> <mode>
#
# `mode` is full or fast; a fast run skipped the e2e suite, so it cannot stand
# in for a full one.
jabot_stamp_path() { git rev-parse --git-path jabot-verified 2>/dev/null; }

jabot_stamp_verified() { # <tree-oid> <full|fast>
  local p
  p=$(jabot_stamp_path) || return 1
  [ -n "$p" ] || return 1
  printf 'tree %s %s %s\n' "$1" "$(date -u +%s)" "$2" > "$p" 2>/dev/null || return 1
}

# Sets JABOT_STAMP_TREE / JABOT_STAMP_AGE / JABOT_STAMP_MODE, or returns 1.
jabot_read_stamp() {
  local p kind oid at mode now
  JABOT_STAMP_TREE=''; JABOT_STAMP_AGE=''; JABOT_STAMP_MODE=''
  p=$(jabot_stamp_path) || return 1
  [ -n "$p" ] && [ -f "$p" ] || return 1
  read -r kind oid at mode _rest < "$p" || return 1
  [ "${kind:-}" = tree ] || return 1
  case "${oid:-}" in ([0-9a-f][0-9a-f]*) ;; (*) return 1 ;; esac
  case "${at:-}" in (*[!0-9]*|'') return 1 ;; esac
  case "${mode:-}" in (full|fast) ;; (*) return 1 ;; esac
  now=$(date -u +%s)
  JABOT_STAMP_TREE=$oid
  JABOT_STAMP_AGE=$(( now - at ))
  JABOT_STAMP_MODE=$mode
  [ "$JABOT_STAMP_AGE" -ge 0 ] || return 1
}
