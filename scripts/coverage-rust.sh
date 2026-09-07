#!/usr/bin/env bash
#
# Collect Rust coverage for the host crate. Reporting only — no threshold.
#
# This script never installs tools and never touches a network. CI puts
# `cargo-llvm-cov` and `llvm-tools-preview` on PATH in the workflow setup
# (see .github/workflows/ci.yml); a laptop that wants the same report does
# the same install itself. `./scripts/verify.sh` stays offline either way.
#
#   JABOT_RUST_COVERAGE=1 ./scripts/verify.sh   # CI: replace `cargo test`
#   ./scripts/coverage-rust.sh                  # the report, on its own
#
# Writes coverage/rust/{lcov.info,coverage.json,html/,SCOPE.md}.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  printf '%s\n' \
    'cargo-llvm-cov is not on PATH.' \
    'Install it in CI setup (.github/workflows/ci.yml), or locally with:' \
    '  rustup component add llvm-tools-preview' \
    '  cargo install cargo-llvm-cov --locked' \
    'This script will not install either.'
  exit 2
fi

OUT=coverage/rust
mkdir -p "$OUT"

cat > "$OUT/SCOPE.md" <<'EOF'
# Rust coverage scope

This report is **Rust unit + crate integration tests** (`cargo test` under
`src-tauri/`, `dev-bins` on). It is not:

- frontend Vitest coverage (`coverage/frontend/`)
- the TypeScript-to-`jabot-hostd` e2e project
- macOS-only code (`src-tauri/src/notify/mac.rs`), which is `cfg`'d out on
  the Linux CI runner and so does not appear here

There is **no Rust coverage threshold** yet. High-risk gaps are listed in
`docs/coverage.md`; a floor comes after those are understood, not before.

Machine-readable: `lcov.info`, `coverage.json`. Human: `html/index.html`.
EOF

# One instrumented `cargo test`. `--no-fail-fast` still exits non-zero when
# tests fail, but finishes the suite so a red run still leaves a report
# (CI uploads those). Further formats are `report` against the same
# profdata, so the suite is not executed twice.
cargo llvm-cov --manifest-path src-tauri/Cargo.toml \
  --features dev-bins \
  --locked \
  --no-fail-fast \
  --lcov --output-path "$OUT/lcov.info"

cargo llvm-cov report --manifest-path src-tauri/Cargo.toml \
  --json --output-path "$OUT/coverage.json"

cargo llvm-cov report --manifest-path src-tauri/Cargo.toml \
  --html --output-dir "$OUT/html"

# Text summary for the log and for the artifact (no extra test run).
cargo llvm-cov report --manifest-path src-tauri/Cargo.toml \
  > "$OUT/summary.txt"

cat "$OUT/summary.txt"
