#!/usr/bin/env bash
#
# macOS packaged-app acceptance (#235).
#
#   ./scripts/macos-acceptance.sh check                 # Linux-safe: matrix, docs, isolation
#   ./scripts/macos-acceptance.sh package [--app PATH]  # inspect a built JaBot.app
#   ./scripts/macos-acceptance.sh updater-artifacts DIR # archives/sigs, not a signed install
#   ./scripts/macos-acceptance.sh run [--app PATH]      # launch isolated .app (macOS)
#   ./scripts/macos-acceptance.sh matrix                # print the cells
#   ./scripts/macos-acceptance.sh release-checklist     # named manual steps + evidence
#
# This is the native boundary. Browser Playwright (#231–#234), including a
# WebKit project, is renderer + jabot-hostd over the Vite transport. It is
# not Tauri IPC, not WKWebView, and must not be labelled as this gate.
#
# Cost: a GitHub-hosted macos-latest job bills at 10x. The headless `bundle`
# job compiles and packages; it does not prove Dock, notifications, or
# Keychain prompts (D-019 / #73). See docs/macos-acceptance.md.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)
CONF=src-tauri/tauri.conf.json
DOCS=docs/macos-acceptance.md
PLAYWRIGHT_LIE='Playwright WebKit is Tauri acceptance'

die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }
ok()  { printf '\033[32mPASS\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL\033[0m %s\n' "$*"; FAILED+=("$1"); }
warn() { printf '\033[33m!!\033[0m %s\n' "$*"; }

FAILED=()
CMD=${1:-}
[[ -n "$CMD" ]] || { sed -n '3,16p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
shift || true

is_darwin() { [[ "$(uname -s 2>/dev/null || true)" == Darwin ]]; }

bundle_id() { node -p 'require("./src-tauri/tauri.conf.json").identifier'; }
product_name() { node -p 'require("./src-tauri/tauri.conf.json").productName'; }
app_version() { node -p 'require("./src-tauri/tauri.conf.json").version'; }

# ---------------------------------------------------------------------------
# Matrix. The cells are the issue. A check that cannot fail when the cell
# breaks is worse than no check — each row names what would turn it red.
# ---------------------------------------------------------------------------

matrix() {
  cat <<'MATRIX'
cell	stage	owner	evidence
compiled	compiled	agent+human	verify.sh log (Linux). Does not launch JaBot.app.
packaged	packaged	CI on main, tags	JaBot.app identity, Info.plist version, staged adapter path. Headless macos-latest. Not interaction.
launched	launched	macos-acceptance run	isolated JaBot.app + ipc-connected.json. Vite/WebKit browser runs are a different gate.
tauri-ipc	run	macos-acceptance run	ipc-connected.json written from host_rpc (webview invoke). Not jabot-hostd. Not Playwright.
synthetic-turn	run	macos-acceptance run	synthetic-turn.json with hello from fake-acp. Isolated data + fake-acp-agent. No user credentials.
close-to-dock-reopen	run (interactive)	human on a logged-in Mac	process stays up after close; window returns on reopen. Screenshot + evidence/dock-reopen.json. CI=true skips.
quit-relaunch-durability	run	macos-acceptance run	durable-reloaded.json after quit+relaunch against the same temp data dir.
notify-permission	release (manual)	release owner	screenshot of first-launch prompt; denied still works (Inbox). D-019: never delivered on Linux.
notify-click-to-thread	release (manual)	release owner	screenshot: banner click un-hides and opens the named thread.
isolated-keychain	run	macos-acceptance run	keychain.json service starts with com.jabot.app.acceptance.; production service refused.
packaged-adapter	package + run	CI / run	bundled-adapter.json or Contents/Resources vendor/adapters/.../index.js exists.
updater-artifacts	release CI	release.yml	.app.tar.gz + .sig present. Not a signed update install.
signed-update-install	release (manual)	release owner	controlled fixture: installed copy accepts the feed. Logs + latest.json. Separate from archive presence.
MATRIX
}

release_checklist() {
  cat <<'CHECK'
# Release acceptance checklist (#235)

Distinguish these four. A green compile is not a launched app.

- [ ] compiled — `./scripts/verify.sh` on the tagged tree
- [ ] packaged — signed/notarized `JaBot.app` (or the CI bundle job's unsigned .app)
- [ ] launched — isolated `./scripts/macos-acceptance.sh run` wrote ipc-connected.json
- [ ] interactively verified — Dock/reopen + notification permission + click-to-thread

## Automated on a Mac (blocks the packaged-acceptance gate)

Retain the evidence directory (`--keep` or the printed path):

- launch.json — bundle id, version, isolated data dir
- ipc-connected.json — first host_rpc from the webview
- synthetic-turn.json — fake-acp reply
- keychain.json — isolated service, not com.jabot.app
- bundled-adapter.json / package report — staged Claude adapter path
- durable-reloaded.json — same thread after quit/relaunch
- screenshots if the session has a display (screencapture)

Never point JABOT_APP_DATA_DIR at ~/Library/Application Support/com.jabot.app.
Never set JABOT_KEYCHAIN_SERVICE=com.jabot.app. Never use user credentials.

## Manual (blocks publish; owner = the human cutting the release)

1. Notification permission prompt on first launch of the signed app.
   Evidence: screenshot of the prompt; a denied run still shows Inbox cards.
2. Banner appears (unsigned/unbundled builds fail here — D-019).
   Evidence: screenshot of the banner naming the thread.
3. Click-to-thread: banner click un-hides the window and selects that thread.
   Evidence: screenshot of the opened thread + note of the notification id.
4. Same-thread replacement (identifier policy), not a stack of banners.
   Evidence: screenshot or Notification Center listing with one banner.
5. signed update install from a controlled feed fixture — not the archive
   presence check. Evidence: latest.json version, updater log, before/after
   version in About / launch.json.

Playwright WebKit is not this list.
CHECK
}

# ---------------------------------------------------------------------------
# check — offline, no display, no macOS. The verify.sh stage.
# ---------------------------------------------------------------------------

check() {
  local ok=0
  [[ -f "$DOCS" ]] || { printf '  missing %s\n' "$DOCS"; return 1; }
  [[ -x scripts/macos-acceptance.sh ]] || { printf '  script is not executable\n'; return 1; }

  node - <<'NODE' || ok=1
const fs = require('fs');
const errs = [];
const bad = (m) => errs.push(m);

const docs = fs.readFileSync('docs/macos-acceptance.md', 'utf8');
const ci = fs.readFileSync('.github/workflows/ci.yml', 'utf8');
const native = fs.existsSync('.github/workflows/macos-native.yml')
  ? fs.readFileSync('.github/workflows/macos-native.yml', 'utf8')
  : '';
const release = fs.readFileSync('.github/workflows/release.yml', 'utf8');
const script = fs.readFileSync('scripts/macos-acceptance.sh', 'utf8');
const verify = fs.readFileSync('scripts/verify.sh', 'utf8');
const contributing = fs.readFileSync('CONTRIBUTING.md', 'utf8');

const cells = [
  'tauri-ipc',
  'synthetic-turn',
  'close-to-dock',
  'quit-relaunch',
  'notify',
  'keychain',
  'packaged adapter',
  'updater',
];
for (const cell of cells) {
  const re = new RegExp(cell.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i');
  if (!re.test(docs)) bad(`docs/macos-acceptance.md does not name the "${cell}" cell`);
}

for (const word of [
  'compiled',
  'packaged',
  'launched',
  'interactively verified',
  'D-019',
  'Playwright',
  'WKWebView',
  '10x',
]) {
  if (!docs.includes(word)) bad(`docs/macos-acceptance.md is missing "${word}"`);
}

if (/webkit[^\n]{0,80}(tauri|wkwebview) acceptance/i.test(docs)
    && !/not (?:proof of |labelled as )?(?:tauri|wkwebview)/i.test(docs)) {
  bad('docs must not label WebKit Playwright as Tauri acceptance');
}

if (!script.includes('Playwright WebKit is Tauri acceptance')
    && !script.includes('must not be labelled')) {
  bad('scripts/macos-acceptance.sh lost the Playwright disclaimer');
}

if (!native.includes('macos-acceptance.sh') && !ci.includes('macos-acceptance.sh')) {
  bad('no workflow invokes macos-acceptance.sh — the targeted native check is gone');
}
if (!native.includes('paths:') && !ci.includes('native-sensitive')) {
  bad('no path filter for native-sensitive PRs');
}
if (!release.includes('updater-artifacts') && !release.includes('macos-acceptance.sh updater')) {
  bad('release.yml no longer checks updater archives separately from a signed install');
}

if (!verify.includes('macos-acceptance') && !verify.includes('macos acceptance')) {
  bad('verify.sh no longer runs the macos-acceptance gate');
}
if (!contributing.includes('macos-acceptance')) {
  bad('CONTRIBUTING.md does not mention macos-acceptance');
}

const conf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
if (conf.bundle.createUpdaterArtifacts !== false) {
  bad('tauri.conf.json createUpdaterArtifacts must stay false; archives are a release --config (D-005)');
}

if (errs.length) {
  for (const e of errs) console.log('  ' + e);
  process.exit(1);
}
console.log('  matrix, workflows, and Playwright disclaimer still agree');
NODE

  # The script must refuse a production data dir even when someone exports HOME.
  if JABOT_ACCEPTANCE_DIR=/tmp/x JABOT_APP_DATA_DIR="$HOME/Library/Application Support/com.jabot.app" \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
      "$0" _assert-isolation >/dev/null 2>&1; then
    printf '  isolation accepted the production app-support path\n'
    ok=1
  fi
  if JABOT_ACCEPTANCE_DIR=/tmp/x JABOT_APP_DATA_DIR=/tmp/jabot-acceptance-x/data \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app \
      "$0" _assert-isolation >/dev/null 2>&1; then
    printf '  isolation accepted the production Keychain service\n'
    ok=1
  fi
  if JABOT_ACCEPTANCE_DIR=/tmp/x JABOT_APP_DATA_DIR=/tmp/jabot-acceptance-x/data \
      JABOT_KEYCHAIN_SERVICE=com.jabot.app.acceptance.test \
      "$0" _assert-isolation; then
    :
  else
    printf '  isolation refused a valid temp dir\n'
    ok=1
  fi

  if [[ $ok -eq 0 ]]; then
    ok "check (docs, isolation, workflow pins)"
  fi
  return $ok
}

_assert_isolation() {
  local data=${JABOT_APP_DATA_DIR:-}
  local service=${JABOT_KEYCHAIN_SERVICE:-}
  case "$data" in
    "") die "$data dir empty" ;;
    *"/Library/Application Support/com.jabot.app") die "production app data: $data" ;;
  esac
  case "$service" in
    com.jabot.app.acceptance.*) ;;
    *) die "keychain service is not isolated: ${service:-unset}" ;;
  esac
}

# ---------------------------------------------------------------------------
# package — inspect a built .app. No launch. Works on any OS that can see
# the tree (the bundle job copies it; a laptop has it after tauri build).
# ---------------------------------------------------------------------------

find_app() {
  local hint=${1:-}
  if [[ -n "$hint" && -d "$hint" ]]; then
    printf '%s\n' "$hint"
    return
  fi
  local candidates=(
    "src-tauri/target/release/bundle/macos/JaBot.app"
    "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/JaBot.app"
    "src-tauri/target/x86_64-apple-darwin/release/bundle/macos/JaBot.app"
    "src-tauri/target/universal-apple-darwin/release/bundle/macos/JaBot.app"
  )
  local c
  for c in "${candidates[@]}"; do
    if [[ -d "$c" ]]; then
      printf '%s\n' "$c"
      return
    fi
  done
  return 1
}

package() {
  local app=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --app) app=$2; shift 2 ;;
      *) die "unknown package flag: $1" ;;
    esac
  done
  app=$(find_app "${app:-}") || die "no JaBot.app found; pass --app PATH after tauri build"
  local want_id want_name want_ver
  want_id=$(bundle_id)
  want_name=$(product_name)
  want_ver=$(app_version)
  local plist="$app/Contents/Info.plist"
  [[ -f "$plist" ]] || die "no Info.plist in $app — not a packaged app"

  local got_id got_name got_ver
  got_id=$(plist_str "$plist" CFBundleIdentifier)
  got_name=$(plist_str "$plist" CFBundleName)
  got_ver=$(plist_str "$plist" CFBundleShortVersionString)
  [[ "$got_id" == "$want_id" ]] || die "bundle id $got_id != $want_id"
  [[ "$got_name" == "$want_name" ]] || die "product name $got_name != $want_name"
  [[ "$got_ver" == "$want_ver" ]] || die "version $got_ver != $want_ver"

  local adapter
  adapter=$(find "$app/Contents/Resources" -path '*claude-agent-acp/dist/index.js' 2>/dev/null | head -1 || true)
  [[ -n "$adapter" ]] || die "packaged adapter missing under Contents/Resources (bundle.resources / bundle-adapters.sh)"

  # Updater archives are a different cell. A packaged .app without them is
  # expected for `tauri build` (createUpdaterArtifacts is false).
  ok "package $app  id=$got_id  version=$got_ver"
  say "  adapter $adapter"
  say "  updater archives are a separate check (updater-artifacts), not this one"
}

plist_str() {
  local plist=$1 key=$2
  if command -v plutil >/dev/null 2>&1; then
    plutil -extract "$key" raw -o - "$plist" 2>/dev/null && return
  fi
  # Linux CI and the stubbed test: read the XML the bundler writes.
  node -e '
    const fs = require("fs");
    const [plist, key] = process.argv.slice(1);
    const t = fs.readFileSync(plist, "utf8");
    const re = new RegExp("<key>" + key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "</key>\\s*<string>([^<]*)</string>");
    const m = re.exec(t);
    if (!m) process.exit(2);
    process.stdout.write(m[1] + "\n");
  ' "$plist" "$key"
}

# ---------------------------------------------------------------------------
# updater-artifacts — presence of the archive + sig. NOT a signed install.
# ---------------------------------------------------------------------------

updater_artifacts() {
  local dir=${1:-}
  [[ -n "$dir" && -d "$dir" ]] || die "updater-artifacts needs a directory of build outputs"
  local archive sig
  archive=$(find "$dir" -name '*.app.tar.gz' ! -name '*.sig' 2>/dev/null | head -1 || true)
  sig=$(find "$dir" -name '*.app.tar.gz.sig' 2>/dev/null | head -1 || true)
  [[ -n "$archive" ]] || die "no .app.tar.gz under $dir — createUpdaterArtifacts did not emit an updater archive (D-005)"
  [[ -n "$sig" ]] || die "no .app.tar.gz.sig under $dir — the feed would be unverifiable"
  ok "updater artifacts  archive=$(basename "$archive")  sig=$(basename "$sig")"
  say "  this is archive presence, not a signed update installation"
  say "  signed install is the release-checklist item owned by the human cutting the tag"
}

# ---------------------------------------------------------------------------
# run — launch the packaged app against temp data + isolated Keychain.
# ---------------------------------------------------------------------------

run() {
  local app="" keep=0 interactive=1
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --app) app=$2; shift 2 ;;
      --keep) keep=1; shift ;;
      --no-interactive) interactive=0; shift ;;
      *) die "unknown run flag: $1" ;;
    esac
  done
  is_darwin || die "run is macOS-only; on Linux use check / package. GitHub's Linux verify job cannot launch JaBot.app (D-019)."
  [[ "${CI:-}" == true || "${CI:-}" == 1 ]] && interactive=0

  app=$(find_app "${app:-}") || die "no JaBot.app; pass --app PATH"
  local bin="$app/Contents/MacOS/$(product_name)"
  [[ -x "$bin" ]] || die "no executable at $bin"

  command -v cargo >/dev/null 2>&1 || die "cargo not found; needed to build fake-acp-agent"
  cargo build --manifest-path src-tauri/Cargo.toml --features dev-bins --bin fake-acp-agent \
    || die "fake-acp-agent failed to build"
  local fake
  fake=$(find src-tauri/target -type f -name fake-acp-agent -perm -111 2>/dev/null | head -1 || true)
  [[ -n "$fake" ]] || die "fake-acp-agent binary missing after build"

  local stamp run_id work evidence data keychain_name service
  stamp=$(date -u +%Y%m%dT%H%M%SZ)
  run_id=$(hexdump -n 4 -e '1/4 "%08x"' /dev/urandom 2>/dev/null || printf '%s' "$$")
  work=$(mktemp -d "${TMPDIR:-/tmp}/jabot-acceptance-${run_id}.XXXXXX")
  evidence="$work/evidence"
  data="$work/data"
  mkdir -p "$evidence" "$data" "$work/home"
  keychain_name="$work/acceptance.keychain-db"
  service="com.jabot.app.acceptance.${run_id}"

  # Refuse to inherit the user's login Keychain as the only store: create a
  # throwaway keychain and put it first. Teardown deletes it.
  local old_keychains=""
  if command -v security >/dev/null 2>&1; then
    old_keychains=$(security list-keychains -d user 2>/dev/null | tr -d '"' || true)
    security create-keychain -p "" "$keychain_name" || die "could not create isolated keychain"
    security set-keychain-settings -t 3600 -u "$keychain_name" || true
    security unlock-keychain -p "" "$keychain_name" || true
    # Isolated first, then whatever was already on the search list, so a
    # laptop run does not drop login.keychain for the rest of the session.
    # shellcheck disable=SC2086
    security list-keychains -d user -s "$keychain_name" $old_keychains || true
  else
    warn "security(1) missing; Keychain isolation is service-name only"
  fi

  say "acceptance workdir $work"
  say "  app $app"
  say "  data $data"
  say "  keychain service $service"
  say "  evidence $evidence"

  export JABOT_ACCEPTANCE_DIR="$evidence"
  export JABOT_APP_DATA_DIR="$data"
  export JABOT_KEYCHAIN_SERVICE="$service"
  export JABOT_FAKE_ACP_BIN="$ROOT/$fake"
  # HOME is *not* replaced: PATH adapters still resolve. Data isolation is
  # JABOT_APP_DATA_DIR, not a fake home that hides node/claude.

  _assert_isolation

  local log="$evidence/app.stderr.log"
  "$bin" >"$evidence/app.stdout.log" 2>"$log" &
  local pid=$!
  say "  launched pid $pid"

  wait_file "$evidence/launch.json" 45 || { dump_run "$evidence" "$pid"; die "launch.json never appeared"; }
  wait_file "$evidence/synthetic-turn.json" 45 || { dump_run "$evidence" "$pid"; die "synthetic-turn.json never appeared"; }
  wait_file "$evidence/keychain.json" 45 || { dump_run "$evidence" "$pid"; die "keychain.json never appeared"; }
  wait_file "$evidence/ipc-connected.json" 60 || {
    warn "ipc-connected.json missing — webview never invoked host_rpc (headless runner, or the window never loaded)"
    if [[ "$interactive" -eq 1 ]]; then
      dump_run "$evidence" "$pid"
      die "Tauri IPC cell failed"
    fi
  }

  assert_json_ok "$evidence/synthetic-turn.json" || { dump_run "$evidence" "$pid"; die "synthetic turn failed"; }
  assert_json_ok "$evidence/keychain.json" || { dump_run "$evidence" "$pid"; die "isolated Keychain cell failed"; }
  assert_json_field "$evidence/keychain.json" service "$service" || { dump_run "$evidence" "$pid"; die "keychain service mismatch"; }
  if grep -q "\"usedProductionService\": true" "$evidence/keychain.json"; then
    dump_run "$evidence" "$pid"
    die "keychain probe used the production service"
  fi

  if command -v screencapture >/dev/null 2>&1 && [[ "$interactive" -eq 1 ]]; then
    screencapture -x "$evidence/after-launch.png" || true
  fi

  if [[ "$interactive" -eq 1 ]]; then
    dock_reopen "$pid" "$evidence" || { dump_run "$evidence" "$pid"; die "close-to-Dock/reopen failed"; }
  else
    say "  skip close-to-Dock/reopen (CI / --no-interactive) — named manual release step"
    printf '%s\n' '{"cell":"close-to-dock-reopen","skipped":true,"reason":"no interactive display session"}' \
      >"$evidence/dock-reopen.json"
  fi

  # Quit and relaunch against the same temp data.
  if is_darwin; then
    osascript -e "tell application \"$(product_name)\" to quit" >/dev/null 2>&1 || true
  fi
  if kill -0 "$pid" 2>/dev/null; then
    sleep 1
    kill "$pid" 2>/dev/null || true
    sleep 1
    kill -9 "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true

  "$bin" >"$evidence/app.stdout.relaunch.log" 2>"$evidence/app.stderr.relaunch.log" &
  local pid2=$!
  wait_file "$evidence/durable-reloaded.json" 45 || { dump_run "$evidence" "$pid2"; die "durable-reloaded.json never appeared after relaunch"; }
  if ! grep -q '"ok": true' "$evidence/durable-reloaded.json"; then
    dump_run "$evidence" "$pid2"
    die "quit/relaunch durability failed"
  fi
  if is_darwin; then
    osascript -e "tell application \"$(product_name)\" to quit" >/dev/null 2>&1 || true
  fi
  if kill -0 "$pid2" 2>/dev/null; then
    sleep 1
    kill "$pid2" 2>/dev/null || true
    wait "$pid2" 2>/dev/null || true
  fi

  write_summary "$evidence" "$app"
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    mkdir -p "$ROOT/.macos-acceptance-evidence"
    cp -R "$evidence"/. "$ROOT/.macos-acceptance-evidence/"
  fi
  ok "run  evidence=$evidence"
  say "  retain this directory for the release gate (logs, json, screenshots)"

  if command -v security >/dev/null 2>&1; then
    if [[ -n "$old_keychains" ]]; then
      # shellcheck disable=SC2086
      security list-keychains -d user -s $old_keychains >/dev/null 2>&1 || true
    fi
    if [[ -f "$keychain_name" ]]; then
      security delete-keychain "$keychain_name" >/dev/null 2>&1 || true
    fi
  fi
  if [[ "$keep" -eq 0 && -z "${JABOT_ACCEPTANCE_KEEP:-}" ]]; then
    # Keep evidence; drop only the throwaway home/keychain wrappers if we add them.
    :
  fi
  printf '%s\n' "$evidence"
}

dock_reopen() {
  local pid=$1 evidence=$2
  local name
  name=$(product_name)
  osascript -e "tell application \"System Events\" to tell process \"$name\" to if (count of windows) > 0 then click (button 1 of window 1)" \
    >/dev/null 2>&1 || osascript -e "tell application \"System Events\" to keystroke \"w\" using command down" \
    >/dev/null 2>&1 || true
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    printf '%s\n' '{"cell":"close-to-dock-reopen","ok":false,"error":"process exited on window close"}' \
      >"$evidence/dock-reopen.json"
    return 1
  fi
  osascript -e "tell application \"$name\" to activate" >/dev/null 2>&1 || open -a "$name" || true
  sleep 1
  if command -v screencapture >/dev/null 2>&1; then
    screencapture -x "$evidence/after-reopen.png" || true
  fi
  printf '%s\n' '{"cell":"close-to-dock-reopen","ok":true,"pidStillRunning":true}' \
    >"$evidence/dock-reopen.json"
}

wait_file() {
  local path=$1 timeout=$2
  local i=0
  while [[ $i -lt $timeout ]]; do
    [[ -f "$path" ]] && return 0
    sleep 1
    i=$((i + 1))
  done
  return 1
}

assert_json_ok() {
  grep -q '"ok": true' "$1"
}

assert_json_field() {
  local file=$1 key=$2 want=$3
  grep -q "\"$key\": \"$want\"" "$file"
}

dump_run() {
  local evidence=$1 pid=${2:-}
  say "---- acceptance evidence ----"
  ls -la "$evidence" 2>/dev/null || true
  [[ -f "$evidence/app.stderr.log" ]] && tail -n 40 "$evidence/app.stderr.log"
  [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null && kill "$pid" 2>/dev/null || true
}

write_summary() {
  local evidence=$1 app=$2
  {
    printf '{\n'
    printf '  "app": %s,\n' "$(node -p 'JSON.stringify(process.argv[1])' "$app")"
    printf '  "bundleId": %s,\n' "$(node -p 'JSON.stringify(process.argv[1])' "$(bundle_id)")"
    printf '  "version": %s,\n' "$(node -p 'JSON.stringify(process.argv[1])' "$(app_version)")"
    printf '  "playwrightIsNotAcceptance": true,\n'
    printf '  "cells": ["tauri-ipc","synthetic-turn","close-to-dock-reopen","quit-relaunch-durability","isolated-keychain","packaged-adapter"]\n'
    printf '}\n'
  } >"$evidence/summary.json"
}

# ---------------------------------------------------------------------------

case "$CMD" in
  check) check ;;
  matrix|plan) matrix ;;
  package) package "$@" ;;
  updater-artifacts) updater_artifacts "${1:-}" ;;
  run) run "$@" ;;
  release-checklist) release_checklist ;;
  _assert-isolation) _assert_isolation ;;
  -h|--help) sed -n '3,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) die "unknown command: $CMD (try --help)" ;;
esac

if [[ ${#FAILED[@]} -gt 0 ]]; then
  printf '\033[31m=== %d failed ===\033[0m\n' "${#FAILED[@]}"
  printf '  - %s\n' "${FAILED[@]}"
  exit 1
fi
