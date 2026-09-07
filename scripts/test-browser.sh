#!/usr/bin/env bash
#
# Playwright visual + browser accessibility + smoke suite (#234 / #256).
#
#   ./scripts/test-browser.sh                  # all projects
#   ./scripts/test-browser.sh --smoke          # Chromium @smoke (journey + axe + keyboard)
#   ./scripts/test-browser.sh --visual         # Chromium screenshots
#   ./scripts/test-browser.sh --ui             # Playwright UI
#   ./scripts/test-browser.sh --update-snapshots
#
# Builds jabot-hostd / fake-acp-agent when missing. Does not call live.sh.
# Does not update snapshots unless --update-snapshots is passed — CI must
# never pass that flag. Playwright 1.63+ (1.56 hangs on Node 26).
#
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

MODE=all
EXTRA=()
for arg in "$@"; do
  case "$arg" in
    --smoke) EXTRA+=(--project=chromium --grep @smoke) ;;
    --visual) EXTRA+=(--project=chromium-visual) ;;
    --ui) MODE=ui ;;
    --update-snapshots)
      EXTRA+=(--update-snapshots --project=chromium-visual --project=webkit-visual)
      ;;
    -h|--help)
      sed -n '3,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf 'unknown flag: %s (try --help)\n' "$arg" >&2
      exit 2
      ;;
  esac
done

if [[ ! -x src-tauri/target/debug/jabot-hostd || ! -x src-tauri/target/debug/fake-acp-agent ]]; then
  printf '> building jabot-hostd and fake-acp-agent\n'
  cargo build --manifest-path src-tauri/Cargo.toml --features dev-bins --bins \
    || { printf 'test-browser: host binaries failed to build\n' >&2; exit 1; }
fi

if ! node -e 'require.resolve("@playwright/test")' 2>/dev/null; then
  printf 'test-browser: @playwright/test is not installed (npm ci)\n' >&2
  exit 1
fi

if [[ "$MODE" == "ui" ]]; then
  exec npx playwright test --ui
fi

exec npx playwright test "${EXTRA[@]}"
