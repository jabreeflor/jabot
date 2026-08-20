#!/usr/bin/env bash
#
# One command that checks the whole product, front to back.
#
#   ./scripts/verify.sh            # everything
#   ./scripts/verify.sh --fast     # skip the e2e project (no Rust binary build)
#
# Stages, cheapest first so failures surface early:
#   1. tsc            — renderer types
#   2. vitest unit    — React components + host client (jsdom)
#   3. cargo fmt      — Rust formatting
#   4. cargo clippy   — Rust lints, warnings are errors
#   5. cargo test     — Rust host unit + integration tests
#   6. build hostd    — the NDJSON stdio host the e2e suite drives
#   7. vitest e2e     — TypeScript client against the real Rust host
#   8. vite build     — the renderer bundle actually builds
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

FAST=0
[[ "${1:-}" == "--fast" ]] && FAST=1

MANIFEST=(--manifest-path src-tauri/Cargo.toml)
FAILED=()
run() {
  local name="$1"; shift
  printf '\n\033[1m> %s\033[0m\n' "$name"
  if "$@"; then
    printf '\033[32mPASS %s\033[0m\n' "$name"
  else
    printf '\033[31mFAIL %s\033[0m\n' "$name"
    FAILED+=("$name")
  fi
}

run "typecheck"      npx tsc --noEmit
run "unit tests"     npx vitest run --project unit
run "rust fmt"       cargo fmt "${MANIFEST[@]}" -- --check
run "rust clippy"    cargo clippy "${MANIFEST[@]}" --all-targets -- -D warnings
run "rust tests"     cargo test "${MANIFEST[@]}"

if [[ $FAST -eq 0 ]]; then
  run "build jabot-hostd" cargo build "${MANIFEST[@]}" --bin jabot-hostd
  # Only meaningful if the binary exists; a failed build would make every e2e
  # case fail with the same confusing spawn error.
  if [[ -x src-tauri/target/debug/jabot-hostd ]]; then
    run "e2e (ts to rust host)" npx vitest run --project e2e
  else
    printf '\033[31mFAIL e2e skipped - jabot-hostd did not build\033[0m\n'
    FAILED+=("e2e (not built)")
  fi
fi

run "renderer build" npx vite build

printf '\n'
if [[ ${#FAILED[@]} -eq 0 ]]; then
  printf '\033[32m=== all checks passed ===\033[0m\n'
  exit 0
fi
printf '\033[31m=== %d check(s) failed ===\033[0m\n' "${#FAILED[@]}"
printf '  - %s\n' "${FAILED[@]}"
exit 1
