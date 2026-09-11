#!/usr/bin/env bash
#
# Windows smoke acceptance (#287). Smaller than macos-acceptance.sh.
#
#   ./scripts/windows-acceptance.sh check       # Linux-safe: docs, links, isolation
#   ./scripts/windows-acceptance.sh matrix      # print the five cells
#   ./scripts/windows-acceptance.sh checklist   # printable smoke list
#   ./scripts/windows-acceptance.sh run         # refuses — no packaged .exe yet
#
# This is not the macOS packaged-app gate and not Playwright. See
# docs/windows-acceptance.md and docs/windows.md. Parent: #280.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)
DOCS=docs/windows.md
ACCEPT=docs/windows-acceptance.md

die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }
ok()  { printf '\033[32mPASS\033[0m %s\n' "$*"; }

CMD=${1:-}
[[ -n "$CMD" ]] || { sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
shift || true

# ---------------------------------------------------------------------------
# Matrix. Five cells only. Growing this without updating the docs is a fail.
# ---------------------------------------------------------------------------

matrix() {
  cat <<'MATRIX'
cell	stage	owner	evidence
launch	run (Windows)	human on Win10/11 x64	native window + Tauri IPC. Not live.sh. Not Playwright.
create-open-bot-chat	run (Windows)	human	standing thread / composer screenshot
secret-round-trip	run (Windows)	human; host APIs on #283	Credential Manager item, isolated service. memory backend is not a pass.
adapter-spawn	run (Windows)	human	fake-acp-agent child pid or adapter stderr
quit-no-orphans	run (Windows)	human; blocked on #285	no JaBot.exe / adapter / leftover node after exit
glass-vibrancy	gap	#282	opaque until #282. Do not claim macOS glass.
dock-hide	gap	#280 / #282	close quits; no hide-to-Dock.
signing-smartscreen	gap	#281	unsigned; SmartScreen will warn.
notify	gap	#284	unsupported no-op; Inbox still records the card.
MATRIX
}

checklist() {
  cat <<'CHECK'
# Windows smoke checklist (#287)

Parent: #280. Smaller than macos-acceptance.md. This is not macOS parity.

- [ ] launch — native window, host reachable over Tauri IPC
- [ ] create/open bot chat — standing thread visible, composer works
- [ ] secret round-trip — isolated Credential Manager item (#283); memory backend does not count
- [ ] adapter spawn — fake-acp-agent (or a real harness) is a live child
- [ ] quit, no orphans — after closing the last window, no JaBot.exe / adapter / leftover node from that session

Gaps to record, not to paper over:

- [ ] glass/vibrancy — chrome is opaque, or #282 has landed
- [ ] Dock hide — close quit the app (no hide-to-Dock)
- [ ] signing/SmartScreen — unsigned warning noted, or a signed build exists
- [ ] notify — no toast (unsupported), or #284 has landed; Inbox card still present

Playwright is not this list. live.sh is not this list.
CHECK
}

# ---------------------------------------------------------------------------
# check — offline, no display, no Windows. The verify.sh stage.
# ---------------------------------------------------------------------------

check() {
  local ok=0
  [[ -f "$DOCS" ]] || { printf '  missing %s\n' "$DOCS"; return 1; }
  [[ -f "$ACCEPT" ]] || { printf '  missing %s\n' "$ACCEPT"; return 1; }
  [[ -x scripts/windows-acceptance.sh ]] || { printf '  script is not executable\n'; return 1; }

  node - <<'NODE' || ok=1
const fs = require('fs');
const errs = [];
const bad = (m) => errs.push(m);

const docs = fs.readFileSync('docs/windows.md', 'utf8');
const accept = fs.readFileSync('docs/windows-acceptance.md', 'utf8');
const readme = fs.readFileSync('README.md', 'utf8');
const packaging = fs.readFileSync('docs/packaging.md', 'utf8');
const contributing = fs.readFileSync('CONTRIBUTING.md', 'utf8');
const verify = fs.readFileSync('scripts/verify.sh', 'utf8');
const script = fs.readFileSync('scripts/windows-acceptance.sh', 'utf8');

for (const word of [
  'Windows 10',
  'Windows 11',
  'x64',
  'tauri dev',
  'WebView2',
  'SmartScreen',
  'glass',
  'vibrancy',
  'Dock',
  'notify',
  '#280',
  '#281',
  'packaging.md',
  'windows-acceptance.md',
  'NSIS',
  'JaBot_*_x64-setup.exe',
  'current-user',
]) {
  if (!docs.includes(word)) bad(`docs/windows.md is missing "${word}"`);
}

if (!readme.includes('JaBot_*_x64-setup.exe')) {
  bad('README.md does not name the NSIS artifact JaBot_*_x64-setup.exe');
}

if (/\[#281\]\([^)]*\/pull\//.test(docs) || /\[#281\]\([^)]*\/pull\//.test(accept)
    || /\[#281\]\([^)]*\/pull\//.test(readme) || /\[#281\]\([^)]*\/pull\//.test(packaging)) {
  bad('#281 must link to /issues/281, not a /pull/ URL (PR #291 is named separately)');
}

if (/File → Exit/.test(accept) || /File -> Exit/.test(accept)) {
  bad('docs/windows-acceptance.md must not invent a File → Exit menu');
}

if (/works on Windows the same way it does on Linux/.test(docs)) {
  bad('docs/windows.md must not claim live.sh Windows parity with Linux');
}

if (!/Git Bash/.test(docs) || !/bundle-adapters\.sh/.test(docs)) {
  bad('docs/windows.md must say bundle:adapters is bash / Git Bash, not a PowerShell copy-paste');
}

if (!docs.includes('does **not** claim macOS parity')
    && !docs.includes('does not claim macOS parity')) {
  bad('docs/windows.md must say it does not claim macOS parity');
}

if (/full macOS parity/i.test(docs) && !/not[^\n]{0,40}macOS parity/i.test(docs)) {
  bad('docs/windows.md looks like it claims full macOS parity');
}

const cells = [
  'launch',
  'create/open bot chat',
  'secret round-trip',
  'adapter spawn',
  'quit, no orphans',
];
for (const cell of cells) {
  if (!accept.toLowerCase().includes(cell.toLowerCase())) {
    bad(`docs/windows-acceptance.md does not name the "${cell}" cell`);
  }
}

for (const word of [
  'Playwright',
  'Credential Manager',
  'SmartScreen',
  'glass',
  'Dock',
  'notify',
  '#280',
  '#283',
  '#285',
]) {
  if (!accept.includes(word)) bad(`docs/windows-acceptance.md is missing "${word}"`);
}

if (/webkit[^\n]{0,80}(tauri|webview2) acceptance/i.test(accept)
    && !/not (?:proof of |labelled as )?(?:tauri|webview2)/i.test(accept)) {
  bad('docs must not label Playwright as Windows/WebView2 acceptance');
}

if (!script.includes('Playwright is not this list')
    && !script.includes('not Playwright')) {
  bad('scripts/windows-acceptance.sh lost the Playwright disclaimer');
}

if (!readme.includes('docs/windows.md')) {
  bad('README.md does not link docs/windows.md');
}
if (!packaging.includes('windows.md')) {
  bad('docs/packaging.md does not link the Windows install page');
}
if (!packaging.includes('#280')) {
  bad('docs/packaging.md does not mention tracking epic #280');
}
if (!contributing.includes('windows-acceptance')) {
  bad('CONTRIBUTING.md does not mention windows-acceptance');
}
if (!verify.includes('windows-acceptance') && !verify.includes('windows acceptance')) {
  bad('verify.sh no longer runs the windows-acceptance gate');
}

if (errs.length) {
  for (const e of errs) console.log('  ' + e);
  process.exit(1);
}
console.log('  windows docs, gap list, and smoke cells still agree');
NODE

  if JABOT_APP_DATA_DIR="$HOME/AppData/Roaming/com.jabot.app" \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
      "$0" _assert-isolation >/dev/null 2>&1; then
    printf '  isolation accepted the production AppData path\n'
    ok=1
  fi
  if JABOT_APP_DATA_DIR="$HOME/Library/Application Support/com.jabot.app" \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
      "$0" _assert-isolation >/dev/null 2>&1; then
    printf '  isolation accepted the production macOS app-support path\n'
    ok=1
  fi
  if JABOT_APP_DATA_DIR=/tmp/jabot-acceptance-x/data \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app \
      "$0" _assert-isolation >/dev/null 2>&1; then
    printf '  isolation accepted the production secret service\n'
    ok=1
  fi
  if JABOT_APP_DATA_DIR=/tmp/jabot-acceptance-x/data \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
      "$0" _assert-isolation; then
    :
  else
    printf '  isolation refused a valid temp dir\n'
    ok=1
  fi

  if [[ $ok -eq 0 ]]; then
    ok "check (docs, isolation, cross-links)"
  fi
  return $ok
}

_assert_isolation() {
  local data=${JABOT_APP_DATA_DIR:-}
  local service=${JABOT_KEYCHAIN_SERVICE:-}
  local norm=${data//\\//}
  case "$data" in
    "") die "data dir empty" ;;
  esac
  case "$norm" in
    *"/AppData/Roaming/com.jabot.app"|*"/AppData/Local/com.jabot.app")
      die "production app data: $data"
      ;;
    *"/Library/Application Support/com.jabot.app")
      die "production app data: $data"
      ;;
  esac
  case "$service" in
    com.jabot.app.acceptance.*) ;;
    *) die "secret service is not isolated: ${service:-unset}" ;;
  esac
}

# ---------------------------------------------------------------------------
# run — not wired. A packaged JaBot.exe does not exist until #281.
# ---------------------------------------------------------------------------

run() {
  die "run is not wired: there is no packaged JaBot.exe in this tree yet (#281). Walk docs/windows-acceptance.md on a Windows box. Linux verify cannot launch WebView2 (same honesty as D-019 on macOS notify)."
}

# ---------------------------------------------------------------------------

case "$CMD" in
  check) check ;;
  matrix|plan) matrix ;;
  checklist) checklist ;;
  run) run "$@" ;;
  _assert-isolation) _assert_isolation ;;
  -h|--help) sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) die "unknown command: $CMD (try --help)" ;;
esac
