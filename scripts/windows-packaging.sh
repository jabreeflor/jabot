#!/usr/bin/env bash
#
# Windows NSIS packaging gate (#281).
#
#   ./scripts/windows-packaging.sh check              # Linux-safe: config, workflow, docs
#   ./scripts/windows-packaging.sh artifacts [DIR]    # require a *-setup.exe (release CI)
#
# This is the packaging contract, not a launched Windows app. Window chrome
# (#282), Credential Manager (#283), toasts (#284), and Job Objects (#285)
# are separate. A green check here means `tauri build` on windows-latest is
# still aimed at NSIS, writes the same draft notes as macos, and cannot
# clobber the macOS updater feed (uploadUpdaterJson is false; no
# TAURI_SIGNING_*).
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)
CONF=src-tauri/tauri.conf.json
WIN_CONF=src-tauri/tauri.windows.conf.json
DOCS=docs/packaging.md

die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }
ok()  { printf '\033[32mPASS\033[0m %s\n' "$*"; }

CMD=${1:-}
[[ -n "$CMD" ]] || { sed -n '3,7p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
shift || true

# ---------------------------------------------------------------------------
# check — offline, no display, no Windows. The verify.sh stage.
# ---------------------------------------------------------------------------

check() {
  local ok=0
  [[ -f "$CONF" ]] || { printf '  missing %s\n' "$CONF"; return 1; }
  [[ -f "$WIN_CONF" ]] || { printf '  missing %s\n' "$WIN_CONF"; return 1; }
  [[ -f "$DOCS" ]] || { printf '  missing %s\n' "$DOCS"; return 1; }

  node - <<'NODE' || ok=1
const fs = require('fs');
const path = require('path');
const errs = [];
const bad = (m) => errs.push(m);

const T = 'src-tauri/tauri.conf.json';
const W = 'src-tauri/tauri.windows.conf.json';
const Y = '.github/workflows/release.yml';
const P = 'package.json';
const R = 'README.md';
const D = 'docs/packaging.md';

const conf = JSON.parse(fs.readFileSync(T, 'utf8'));
const win = JSON.parse(fs.readFileSync(W, 'utf8'));
const wf = fs.readFileSync(Y, 'utf8');
const pkg = JSON.parse(fs.readFileSync(P, 'utf8'));

if (conf.productName !== 'JaBot') {
  bad(`${T}: productName must stay "JaBot" (got ${JSON.stringify(conf.productName)})`);
}
if (conf.identifier !== 'com.jabot.app') {
  bad(`${T}: identifier must stay "com.jabot.app" (got ${JSON.stringify(conf.identifier)})`);
}
if (win.productName && win.productName !== conf.productName) {
  bad(`${W}: productName ${JSON.stringify(win.productName)} disagrees with ${T}`);
}
if (win.identifier && win.identifier !== conf.identifier) {
  bad(`${W}: identifier ${JSON.stringify(win.identifier)} disagrees with ${T}`);
}

const macTargets = conf.bundle && conf.bundle.targets;
if (macTargets !== 'all' && !(Array.isArray(macTargets) && macTargets.includes('app') && macTargets.includes('dmg'))) {
  bad(`${T}: bundle.targets must still contain "app" and "dmg" (got ${JSON.stringify(macTargets)}) — Windows packaging must not drop the macOS installers (D-005)`);
}
if (Array.isArray(macTargets) && macTargets.includes('nsis')) {
  bad(`${T}: bundle.targets lists "nsis" — that belongs in ${W} so a macOS tauri build never sees a Windows target`);
}

const winTargets = win.bundle && win.bundle.targets;
if (!(Array.isArray(winTargets) && winTargets.includes('nsis'))) {
  bad(`${W}: bundle.targets must contain "nsis" (got ${JSON.stringify(winTargets)}) — that is the primary Windows installer`);
}
if (Array.isArray(winTargets) && winTargets.includes('msi')) {
  bad(`${W}: bundle.targets also lists "msi" — MVP ships one installer (NSIS); do not add WiX unless that is a deliberate second artifact`);
}
if (Array.isArray(winTargets) && (winTargets.includes('app') || winTargets.includes('dmg'))) {
  bad(`${W}: bundle.targets still lists a macOS target (got ${JSON.stringify(winTargets)}) — on Windows those names do not produce an installer`);
}

const publisher = (win.bundle && win.bundle.publisher) || (conf.bundle && conf.bundle.publisher);
if (!publisher) {
  bad(`${T} / ${W}: bundle.publisher is empty — NSIS manufacturer would fall back to a slice of the identifier`);
}

const nsis = (win.bundle && win.bundle.windows && win.bundle.windows.nsis) || {};
if (nsis.installMode !== 'currentUser') {
  bad(`${W}: bundle.windows.nsis.installMode must be "currentUser" (got ${JSON.stringify(nsis.installMode)}) — per-machine needs elevation the unsigned MVP should not require`);
}

const icons = (conf.bundle && conf.bundle.icon) || [];
if (!icons.includes('icons/icon.ico')) {
  bad(`${T}: bundle.icon must include icons/icon.ico — the NSIS installer uses it`);
}
const ico = path.join('src-tauri', 'icons/icon.ico');
if (!fs.existsSync(ico) || fs.statSync(ico).size === 0) {
  bad(`${ico} is missing or empty`);
}

const adapters = pkg.scripts && pkg.scripts['bundle:adapters'];
if (!adapters || !/\bbash\b/.test(adapters) || !/bundle-adapters\.sh/.test(adapters)) {
  bad(`${P}: scripts.bundle:adapters must invoke the script through bash (got ${JSON.stringify(adapters)}) — npm on Windows will not execute a bare ./scripts/*.sh`);
}

function jobBody(src, name) {
  const start = src.search(new RegExp(`^  ${name}:`, 'm'));
  if (start < 0) return null;
  const afterHeader = src.indexOf('\n', start);
  if (afterHeader < 0) return '';
  const rest = src.slice(afterHeader + 1);
  const next = rest.search(/^  [A-Za-z0-9_-]+:/m);
  return next < 0 ? rest : rest.slice(0, next);
}

if (!/^jobs:\n/m.test(wf)) bad(`${Y}: has no jobs: block`);
const macos = jobBody(wf, 'macos');
const windows = jobBody(wf, 'windows');
if (!macos) bad(`${Y}: no macos: job — the universal-apple-darwin release path is gone`);
if (!windows) bad(`${Y}: no windows: job — nothing uploads the NSIS installer`);

if (!macos.includes('macos-latest')) {
  bad(`${Y}: macos job is not on macos-latest`);
}
if (!macos.includes('universal-apple-darwin')) {
  bad(`${Y}: macos job lost --target universal-apple-darwin`);
}
if (!macos.includes('createUpdaterArtifacts')) {
  bad(`${Y}: macos job no longer merges createUpdaterArtifacts — the feed would ship without archives (D-005)`);
}
if (!/gh release upload[^\n]*install\.sh|install\.sh[^\n]*gh release upload/.test(macos.replace(/\\\n/g, ' '))) {
  bad(`${Y}: macos job no longer uploads scripts/install.sh`);
}

if (!windows.includes('windows-latest')) {
  bad(`${Y}: windows job is not on windows-latest`);
}
if (!windows.includes('--bundles nsis') && !windows.includes('--bundles=nsis')) {
  bad(`${Y}: windows job must pass --bundles nsis so the installer is explicit even if ${W} is ignored`);
}
if (!windows.includes('x86_64-pc-windows-msvc')) {
  bad(`${Y}: windows job must pin --target x86_64-pc-windows-msvc (MVP is one x64 installer)`);
}
const windowsCode = windows.replace(/^\s*#.*$/gm, '').replace(/\\\n/g, ' ');
if (/createUpdaterArtifacts/.test(windowsCode)) {
  bad(`${Y}: windows job merges createUpdaterArtifacts — it would race the macOS job for latest.json, which the updater plugin looks up by darwin-* only`);
}
// Pinned tauri-action defaults uploadUpdaterJson to true and will rewrite
// latest.json if it sees a .sig on the draft. createUpdaterArtifacts being
// off is not that write path. The value must be the literal false.
if (!/^\s+uploadUpdaterJson:\s*false\s*$/m.test(windows)) {
  bad(`${Y}: windows job must set uploadUpdaterJson: false — tauri-action defaults to true and would rewrite latest.json if a .sig is on the draft`);
}
if (/^\s+uploadUpdaterJson:\s*true\s*$/m.test(windows)) {
  bad(`${Y}: windows job sets uploadUpdaterJson: true — that job must not touch latest.json`);
}
// Pinned tauri-action does not update name/body on an existing draft. If
// windows creates the draft first, macos leaves the body alone. Both jobs
// must carry the full notes, including the unsigned / SmartScreen warning.
if (!/^\s+releaseBody:\s*\|?\s*$/m.test(windows)) {
  bad(`${Y}: windows job has no releaseBody — pinned tauri-action will not update an existing draft; if windows creates it first the notes (and SmartScreen warning) stay empty`);
}
if (!/SmartScreen/.test(windows) || !/Authenticode|unsigned/i.test(windows)) {
  bad(`${Y}: windows releaseBody must carry the unsigned / SmartScreen warning (same notes as macos)`);
}
if (!/script-shell/.test(windows) || !/command -v bash/.test(windows)) {
  bad(`${Y}: windows job must run npm config set script-shell "$(command -v bash)" before tauri-action — beforeBuildCommand is spawned by the Tauri CLI, not defaults.run.shell, and npm's script-shell on windows-latest is cmd.exe`);
}
const assigned = (body, prefix) => new RegExp(`^\\s+${prefix}[A-Z0-9_]*:\\s*\\$\\{\\{`, 'm').test(body);
if (assigned(windows, 'APPLE_')) {
  bad(`${Y}: windows job lists an APPLE_* secret — those belong only on the macOS runner`);
}
if (assigned(windows, 'TAURI_SIGNING_')) {
  bad(`${Y}: windows job lists TAURI_SIGNING_* — that is the updater minisign key, not Authenticode; keep it off this runner`);
}

const readme = fs.readFileSync(R, 'utf8');
const docs = fs.readFileSync(D, 'utf8');
for (const [file, text] of [[R, readme], [D, docs]]) {
  if (!/NSIS|setup\.exe/i.test(text)) {
    bad(`${file}: does not tell a Windows user about the NSIS installer`);
  }
  if (!/SmartScreen|Authenticode|unsigned/i.test(text)) {
    bad(`${file}: does not say the Windows installer is unsigned / SmartScreen will warn`);
  }
}
if (!/tauri\.windows\.conf\.json/.test(docs)) {
  bad(`${D}: does not name ${W}`);
}
if (!/npm run tauri build/.test(docs)) {
  bad(`${D}: does not document npm run tauri build on Windows`);
}

if (errs.length) {
  for (const e of errs) console.log('  ' + e);
  process.exit(1);
}
console.log(`  ${W}: targets=${JSON.stringify(winTargets)}, installMode=${nsis.installMode}, publisher=${publisher}`);
console.log(`  ${Y}: macos + windows sibling jobs; uploadUpdaterJson false; latest.json stays on macos`);
NODE
  [[ $ok -eq 0 ]] || return 1
  ok "windows packaging config"
}

# ---------------------------------------------------------------------------
# artifacts — presence of the NSIS setup exe. Not a signed install.
# ---------------------------------------------------------------------------

artifacts() {
  local dir="${1:-}"
  if [[ -z "$dir" ]]; then
    for candidate in \
      src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis \
      src-tauri/target/release/bundle/nsis
    do
      if [[ -d "$candidate" ]]; then
        dir="$candidate"
        break
      fi
    done
  fi
  [[ -n "$dir" && -d "$dir" ]] || die "no NSIS bundle directory — cannot check Windows installer artifacts"
  local setup=""
  setup=$(find "$dir" -type f \( -name '*-setup.exe' -o -name '*setup.exe' \) | head -n 1 || true)
  [[ -n "$setup" ]] || die "no *-setup.exe under $dir — tauri build did not emit the NSIS installer"
  [[ -s "$setup" ]] || die "$setup is empty"
  ok "NSIS installer $setup (unsigned artifact check — not Authenticode / SmartScreen)"
}

case "$CMD" in
  check)     check ;;
  artifacts) artifacts "${1:-}" ;;
  *)         die "unknown command: $CMD" ;;
esac
