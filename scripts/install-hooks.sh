#!/usr/bin/env bash
#
# Point git at the hooks this repo tracks.
#
#   ./scripts/install-hooks.sh              # install (idempotent)
#   ./scripts/install-hooks.sh --uninstall  # back to .git/hooks
#   ./scripts/install-hooks.sh --check      # say whether they are installed; exit 1 if not
#   ./scripts/install-hooks.sh --quiet      # for the npm `prepare` hook
#
# Git hooks live in .git/hooks, which is not tracked and does not survive a
# clone, so a hook nobody installs protects nobody. This sets core.hooksPath to
# the tracked .githooks/ directory: one command, and every hook in there is
# live — including future ones, without a second install step.
#
# `npm install` runs this via the "prepare" script, so a fresh clone that
# follows the README gets the hooks without knowing they exist. It is a local
# git config setting, so it never travels to anyone else's clone.
set -uo pipefail

cd "$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)" || exit 1

HOOKS_DIR=.githooks
MODE=install
QUIET=0
for arg in "$@"; do
  case "$arg" in
    --uninstall) MODE=uninstall ;;
    --check)     MODE=check ;;
    --quiet)     QUIET=1 ;;
    -h|--help)   sed -n '3,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'unknown flag: %s (try --help)\n' "$arg" >&2; exit 2 ;;
  esac
done

say() { [ "$QUIET" -eq 1 ] || printf '%s\n' "$*"; }
err() { printf '\033[31m%s\033[0m\n' "$*" >&2; }

# `npm install` inside a tarball, a Docker build, or a vendored copy has no
# .git. That is not an error worth failing an install over.
if ! git rev-parse --git-dir >/dev/null 2>&1; then
  say "not a git repository — skipping hook installation"
  [ "$MODE" = check ] && exit 1
  exit 0
fi

current=$(git config --local --get core.hooksPath 2>/dev/null || printf '')

case "$MODE" in
  check)
    if [ "$current" = "$HOOKS_DIR" ]; then
      say "hooks installed (core.hooksPath = $HOOKS_DIR)"
      exit 0
    fi
    err "git hooks are NOT installed (core.hooksPath = '${current:-unset}')."
    err "Run ./scripts/install-hooks.sh — otherwise nothing stops an unverified push."
    exit 1
    ;;
  uninstall)
    if [ "$current" = "$HOOKS_DIR" ]; then
      git config --local --unset core.hooksPath
      say "uninstalled: core.hooksPath cleared, git is back on .git/hooks"
    else
      say "nothing to do (core.hooksPath = '${current:-unset}')"
    fi
    exit 0
    ;;
esac

if [ ! -d "$HOOKS_DIR" ]; then
  err "$HOOKS_DIR does not exist"
  exit 1
fi

# A hook that is not executable is silently skipped by git — the worst possible
# failure for a guard. Fix it here rather than discover it during a bad push.
installed=""
for hook in "$HOOKS_DIR"/*; do
  [ -f "$hook" ] || continue
  case "$hook" in *.md|*.sample) continue ;; esac
  [ -x "$hook" ] || chmod +x "$hook"
  installed="$installed ${hook#"$HOOKS_DIR"/}"
done

if [ -z "$installed" ]; then
  err "$HOOKS_DIR contains no hooks"
  exit 1
fi

if [ -n "$current" ] && [ "$current" != "$HOOKS_DIR" ]; then
  say "note: core.hooksPath was '$current'; changing it to $HOOKS_DIR"
fi
git config --local core.hooksPath "$HOOKS_DIR" || { err "could not set core.hooksPath"; exit 1; }

say "hooks installed (core.hooksPath = $HOOKS_DIR):$installed"
say "  pre-push runs ./scripts/verify.sh and refuses a failing push."
say "  Emergency escape hatch: git push --no-verify"
