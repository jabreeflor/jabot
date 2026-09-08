#!/usr/bin/env bash
#
# Prove the Playwright smoke test fails when composer send is broken.
# Restores the file afterwards. Exit 0 only if the test failed as intended.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

FILE=src/components/Composer.tsx
if ! grep -q 'onSend(trimmed);' "$FILE"; then
  printf 'browser-break-demo: could not find onSend(trimmed); in %s\n' "$FILE" >&2
  exit 2
fi

restore() { git checkout -- "$FILE"; }
trap restore EXIT

# Swallow the send — the composer still clears, so the UI looks like it worked.
sed -i 's/onSend(trimmed);/\/\/ broken: onSend(trimmed);/' "$FILE"

printf '> deliberate break: composer onSend is a no-op\n'
set +e
npx playwright test --project=chromium --grep @smoke
status=$?
set -e

if [[ $status -eq 0 ]]; then
  printf 'browser-break-demo: smoke still passed after breaking send — the test is not watching the composer\n' >&2
  exit 1
fi

printf 'browser-break-demo: smoke failed after the send-path break, as required\n'
exit 0
