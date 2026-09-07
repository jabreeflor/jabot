#!/usr/bin/env bash
#
# Tests for the coverage policy: scoped frontend include/exclude, thresholds
# that actually fail, CI installing Rust coverage tools rather than verify.sh.
#
# The fixture cases spin up a throwaway Vitest project. They do not run this
# repo's suite — they prove the *mechanism* (an unimported production file
# counts; a threshold miss is a non-zero exit). The repo's own numbers live
# in docs/coverage.md and are enforced by vitest.config.ts.
#
#   ./scripts/tests/coverage.test.sh
#   ./scripts/tests/coverage.test.sh threshold
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-coverage.XXXXXX") || exit 1
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

pass() { printf '  \033[32mok\033[0m   %s\n' "$CASE"; }
fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$CASE"
  printf '       %s\n' "$@"
  FAILURES=$((FAILURES + 1))
}
assert_eq() {
  if [ "$1" != "$2" ]; then fail "$3: expected [$1], got [$2]"; return 1; fi
}
assert_contains() {
  case "$1" in
    *"$2"*) return 0 ;;
    *) fail "$3: [$2] not found in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-500)"; return 1 ;;
  esac
}
assert_not_contains() {
  case "$1" in
    *"$2"*) fail "$3: [$2] should not be in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-500)"; return 1 ;;
    *) return 0 ;;
  esac
}

run_case() {
  local fn="$1"
  CASE=${fn#case_}
  if [ -n "$FILTER" ] && ! printf '%s' "$CASE" | grep -q "$FILTER"; then
    return 0
  fi
  COUNT=$((COUNT + 1))
  if "$fn"; then
    # A case that already called fail() still returns 0 unless it `return 1`s.
    # Honour FAILURES so a helper's fail() is enough.
    return 0
  fi
}

# --- fixture: one covered file, one unimported file, a test we must ignore --
write_fixture() { # dir
  local d="$1"
  mkdir -p "$d/src" "$d/src/__tests__" "$d/plugins/vendored" "$d/worktrees/other"
  cat > "$d/src/used.ts" <<'TS'
export function covered(): number {
  return 1;
}
TS
  cat > "$d/src/unimported.ts" <<'TS'
export function neverImported(): number {
  return 99;
}
TS
  cat > "$d/src/used.test.ts" <<'TS'
import { describe, expect, it } from "vitest";
import { covered } from "./used";
describe("used", () => {
  it("returns 1", () => {
    expect(covered()).toBe(1);
  });
});
TS
  # A test file that would inflate numbers if it counted as production.
  cat > "$d/src/__tests__/pad.test.ts" <<'TS'
import { describe, expect, it } from "vitest";
describe("pad", () => {
  it("is a test, not production", () => {
    expect(true).toBe(true);
  });
});
TS
  cat > "$d/plugins/vendored/secret.ts" <<'TS'
export const vendor = "should not be scored";
TS
  cat > "$d/worktrees/other/src.ts" <<'TS'
export const nested = "should not be scored";
TS
  cat > "$d/package.json" <<'JSON'
{ "type": "module" }
JSON
  # ESM config resolution does not honor NODE_PATH; the fixture must see this
  # repo's vitest / coverage-v8 without npm-installing (offline gate).
  ln -s "$REPO_ROOT/node_modules" "$d/node_modules"
  cat > "$d/vitest.config.ts" <<'TS'
import { defineConfig } from "vitest/config";
export default defineConfig({
  test: {
    coverage: {
      provider: "v8",
      all: true,
      reportsDirectory: "./coverage/frontend",
      reporter: ["json-summary", "json"],
      reportOnFailure: true,
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.test.{ts,tsx}",
        "src/**/__tests__/**",
        "src/**/*.d.ts",
        "plugins/**",
        "worktrees/**",
      ],
      thresholds: {
        lines: 90,
        branches: 85,
        functions: 80,
        statements: 90,
      },
    },
  },
});
TS
}

# Reuse this repo's vitest / coverage-v8 so the fixture does not npm-install.
VITEST="$REPO_ROOT/node_modules/.bin/vitest"
if [ ! -x "$VITEST" ]; then
  printf 'vitest is not installed in this clone; run npm ci first\n' >&2
  exit 1
fi

case_threshold_violation_fails() {
  local d="$SANDBOX/threshold"
  write_fixture "$d"
  local out rc
  out=$(cd "$d" && "$VITEST" run --coverage --config "$d/vitest.config.ts" 2>&1) || rc=$?
  rc=${rc:-0}
  if [ "$rc" -eq 0 ]; then
    fail "an unimported production file under a 90% floor should fail, got 0"
    printf '%s\n' "$out" | sed 's/^/       /' | tail -20
    return 1
  fi
  assert_contains "$out" "does not meet global threshold" \
    "failure must be the coverage floor, not a test error" || return 1
  assert_contains "$out" "50%" "exactly one of two included files is uncovered" || return 1
  pass
}

case_unimported_file_is_in_the_json() {
  local d="$SANDBOX/json"
  write_fixture "$d"
  (cd "$d" && "$VITEST" run --coverage --config "$d/vitest.config.ts" >/dev/null 2>&1) || true
  local json="$d/coverage/frontend/coverage-final.json"
  if [ ! -f "$json" ]; then
    # vitest 3 json reporter writes coverage-final.json via istanbul
    json="$d/coverage/frontend/coverage-summary.json"
  fi
  if [ ! -f "$json" ]; then
    fail "coverage json was not written (reportOnFailure should still emit it)"
    return 1
  fi
  local body
  body=$(cat "$json")
  assert_contains "$body" "unimported" "unimported production file must appear in the report" || return 1
  assert_not_contains "$body" "pad.test" "test files must not appear as production" || return 1
  assert_not_contains "$body" "vendored" "vendored plugins must not appear" || return 1
  assert_not_contains "$body" "worktrees" "nested worktrees must not appear" || return 1
  pass
}

case_above_threshold_passes() {
  local d="$SANDBOX/pass"
  write_fixture "$d"
  # Import the second file so the floor is met — proves the fail above was
  # the uncovered file, not a broken fixture.
  cat > "$d/src/used.test.ts" <<'TS'
import { describe, expect, it } from "vitest";
import { covered } from "./used";
import { neverImported } from "./unimported";
describe("used", () => {
  it("covers both", () => {
    expect(covered()).toBe(1);
    expect(neverImported()).toBe(99);
  });
});
TS
  local rc=0
  (cd "$d" && "$VITEST" run --coverage --config "$d/vitest.config.ts" >/dev/null 2>&1) || rc=$?
  assert_eq 0 "$rc" "covering every included file should satisfy 90/85/80" || return 1
  pass
}

case_repo_vitest_config_scopes_src() {
  local cfg
  cfg=$(cat "$REPO_ROOT/vitest.config.ts")
  assert_contains "$cfg" 'include: ["src/**/*.{ts,tsx}"]' "production include" || return 1
  assert_contains "$cfg" 'src/**/*.test.{ts,tsx}' "exclude tests" || return 1
  assert_contains "$cfg" 'src/**/__tests__/**' "exclude __tests__" || return 1
  assert_contains "$cfg" 'plugins/**' "exclude vendored plugins" || return 1
  assert_contains "$cfg" 'worktrees/**' "exclude nested worktrees" || return 1
  assert_contains "$cfg" 'all: true' "unimported files must count" || return 1
  assert_contains "$cfg" 'reportOnFailure: true' "failed runs still write reports" || return 1
  assert_contains "$cfg" 'lines: 90' "line floor" || return 1
  assert_contains "$cfg" 'branches: 85' "branch floor" || return 1
  assert_contains "$cfg" 'functions: 80' "function floor" || return 1
  assert_contains "$cfg" 'statements: 90' "statement floor" || return 1
  assert_contains "$cfg" 'json-summary' "machine-readable summary" || return 1
  assert_contains "$cfg" 'lcov' "LCOV reporter" || return 1
  assert_contains "$cfg" 'html' "HTML reporter" || return 1
  pass
}

case_verify_runs_unit_coverage_offline() {
  local v
  v=$(cat "$REPO_ROOT/scripts/verify.sh")
  assert_contains "$v" '--coverage' "unit stage must collect frontend coverage" || return 1
  assert_not_contains "$v" 'cargo install' "verify must not cargo-install" || return 1
  assert_not_contains "$v" 'curl -' "verify must not curl-install coverage tools" || return 1
  pass
}

case_rust_script_does_not_install() {
  local bin="$SANDBOX/no-install-bin"
  mkdir -p "$bin"
  cat > "$bin/cargo" <<'STUB'
#!/usr/bin/env bash
printf 'cargo %s\n' "$*" >> "${STUB_CARGO_LOG:?}"
exit 1
STUB
  chmod +x "$bin/cargo"
  : > "$SANDBOX/cargo.log"
  local rc=0
  STUB_CARGO_LOG="$SANDBOX/cargo.log" PATH="$bin:/usr/bin:/bin" \
    "$REPO_ROOT/scripts/coverage-rust.sh" >/dev/null 2>&1 || rc=$?
  assert_eq 2 "$rc" "missing cargo-llvm-cov must exit 2, not try to install" || return 1
  if [ -s "$SANDBOX/cargo.log" ]; then
    fail "coverage-rust.sh invoked cargo (would have installed): $(cat "$SANDBOX/cargo.log")"
    return 1
  fi
  pass
}

case_ci_installs_rust_coverage_and_uploads_on_failure() {
  local wf
  wf=$(cat "$REPO_ROOT/.github/workflows/ci.yml")
  assert_contains "$wf" 'llvm-tools-preview' "CI rust-toolchain must request llvm-tools" || return 1
  assert_contains "$wf" 'cargo-llvm-cov' "CI must install cargo-llvm-cov" || return 1
  assert_contains "$wf" 'JABOT_RUST_COVERAGE' "CI must opt verify into rust coverage" || return 1
  assert_contains "$wf" 'upload-artifact' "CI must upload coverage artifacts" || return 1
  assert_contains "$wf" 'if: always()' "uploads must run after a failed job when possible" || return 1
  assert_contains "$wf" 'coverage/frontend' "frontend reports in the artifact" || return 1
  assert_contains "$wf" 'coverage/rust' "rust reports in the artifact" || return 1
  pass
}

case_reports_are_gitignored() {
  assert_contains "$(cat "$REPO_ROOT/.gitignore")" $'/coverage\n' \
    "root /coverage must be gitignored so reports do not move the tree hash" || return 1
  pass
}

case_rust_script_refuses_to_install() {
  assert_eq 1 "$(grep -c 'will not install' "$REPO_ROOT/scripts/coverage-rust.sh" || true)" \
    "coverage-rust.sh must say it will not install" || return 1
  if ! head -n 5 "$REPO_ROOT/scripts/coverage-rust.sh" | grep -q 'never installs'; then
    fail "coverage-rust.sh header should say it never installs tools"
    return 1
  fi
  pass
}

printf '\ncoverage policy\n'
run_case case_repo_vitest_config_scopes_src
run_case case_verify_runs_unit_coverage_offline
run_case case_rust_script_does_not_install
run_case case_ci_installs_rust_coverage_and_uploads_on_failure
run_case case_reports_are_gitignored
run_case case_rust_script_refuses_to_install
run_case case_threshold_violation_fails
run_case case_unimported_file_is_in_the_json
run_case case_above_threshold_passes

printf '\n%d cases, %d failed\n' "$COUNT" "$FAILURES"
if [ "$FAILURES" -ne 0 ]; then
  exit 1
fi
exit 0
