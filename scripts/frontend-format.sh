#!/usr/bin/env bash
#
# Apply or check Prettier on first-party frontend files.
#
#   ./scripts/frontend-format.sh --check    # read-only; used by verify.sh
#   ./scripts/frontend-format.sh --write    # rewrite in place
#
# npm run format:check / npm run format are the documented names for these.
# PRETTIER_ROOT overrides the tree (scripts/tests/format.test.sh uses it).
# Offline after `npm install`: the binary is node_modules/.bin/prettier.
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
ROOT="${PRETTIER_ROOT:-$REPO_ROOT}"
PRETTIER="${PRETTIER_BIN:-$REPO_ROOT/node_modules/.bin/prettier}"

usage() {
  sed -n '3,10p' "$0" | sed 's/^# \{0,1\}//'
}

MODE=""
case "${1:-}" in
  --check|--write) MODE="$1"; shift ;;
  -h|--help) usage; exit 0 ;;
  *)
    printf 'usage: %s --check|--write\n' "$0" >&2
    exit 2
    ;;
esac

if [[ ! -x "$PRETTIER" ]]; then
  printf 'prettier is not installed at %s — run npm install\n' "$PRETTIER" >&2
  exit 1
fi

if [[ ! -f "$ROOT/prettier.config.mjs" || ! -f "$ROOT/.prettierignore" ]]; then
  printf 'prettier.config.mjs or .prettierignore missing under %s\n' "$ROOT" >&2
  exit 1
fi

cd "$ROOT"

# --ignore-unknown keeps a stray extension from expanding the contract.
# No --cache: a stale cache can mark a file clean that --check then
# rejects. Nested `.then()` chains can take two --write passes to
# stabilize (tests/e2e/lifecycle.test.ts); --write runs twice so
# format:check is a no-op afterwards. The glob is the contract:
# TS/TSX, JS/MJS/CJS, CSS, JSON.
ARGS=(
  --ignore-unknown
  --config "$ROOT/prettier.config.mjs"
  --ignore-path "$ROOT/.prettierignore"
  "$@"
  "**/*.{ts,tsx,js,mjs,cjs,css,json}"
)

if [[ "$MODE" == "--write" ]]; then
  "$PRETTIER" --write "${ARGS[@]}"
  exec "$PRETTIER" --write "${ARGS[@]}"
fi
exec "$PRETTIER" --check "${ARGS[@]}"
