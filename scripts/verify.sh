#!/usr/bin/env bash
#
# One command that checks the whole product, front to back.
#
#   ./scripts/verify.sh                    # everything
#   ./scripts/verify.sh --fast             # skip the e2e project (no Rust binary build)
#   ./scripts/verify.sh --check-toolchain  # also ask rustup if stable moved (NETWORK)
#
# This is the only gate. CI's `verify` job is `npm ci` + this script, and the
# macOS `bundle` job does not run on pull requests (.github/workflows/ci.yml
# says why), so anything this misses reaches main. Everything below runs
# offline, needs no display, no GitHub token and no macOS — except
# --check-toolchain, which is opt-in for exactly that reason.
#
# Stages, cheapest first so failures surface early:
#   0. toolchain      — versions printed, MSRV floor enforced, drift from CI warned
#   1. lockfiles      — package-lock.json and Cargo.lock agree with their manifests
#   2. bundle-config  — what the macOS `bundle` job reads, checked without macOS
#   2b. commit guards — the checkpoint/pre-push guards still refuse a bad commit
#   3. tsc            — renderer types
#   4. vitest unit    — React components + host client (jsdom)
#   5. cargo fmt      — Rust formatting
#   6. cargo clippy   — Rust lints, warnings are errors
#   6b. cargo check   — the crate compiles WITHOUT dev-bins, i.e. what tauri build sees
#   7. cargo test     — Rust host unit + integration tests
#   8. build hostd    — the NDJSON stdio host the e2e suite drives
#   9. vitest e2e     — TypeScript client against the real Rust host
#  10. vite build     — the renderer bundle actually builds
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
# jabot_tree_hash / jabot_stamp_verified — see the end of this file for why a
# verification run has to know whether the thing it verified stayed still.
# shellcheck source=lib/tree-hash.sh
. "scripts/lib/tree-hash.sh"

FAST=0
CHECK_TOOLCHAIN=0
for arg in "$@"; do
  case "$arg" in
    --fast)            FAST=1 ;;
    --check-toolchain) CHECK_TOOLCHAIN=1 ;;
    -h|--help)         sed -n '3,7p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'unknown flag: %s (try --help)\n' "$arg" >&2; exit 2 ;;
  esac
done

MANIFEST=(--manifest-path src-tauri/Cargo.toml)
# `jabot-hostd` and `fake-acp-agent` are cargo targets only under the
# `dev-bins` feature, so that `tauri build` never sees them (src-tauri/Cargo.toml
# has the reason). Anything here that compiles, lints, or runs them has to ask.
DEV_BINS=(--features dev-bins)
# CI's cache hides a stale Cargo.lock; --locked makes a stale one a failure
# here instead of a surprise later. On the passing path it costs nothing and
# touches nothing: the lock already satisfies the manifest, so there is no
# resolution to do. On the failing path cargo says "cannot update the lock
# file ... because --locked was passed" and stops; a manifest edit that also
# needs a new *version* can send it to the index first, which is no more
# network than the build it is replacing would have used.
LOCKED=(--locked)
FAILED=()
WARNINGS=()

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

warn() {
  printf '\033[33m!! %s\033[0m\n' "$1"
  WARNINGS+=("$1")
}

# ---------------------------------------------------------------------------
# 0. toolchain
#
# Two CI failures came from this box being on an older stable than CI, which
# installs current stable on every run (dtolnay/rust-toolchain@stable). See
# DEVIATIONS.md D-014. Being older than CI cannot be detected offline with
# certainty, so: print every version unconditionally, so a discrepancy is
# visible in any pasted log; fail only on things that are locally provable
# (below the declared floor, mismatched clippy); warn on everything else.
# ---------------------------------------------------------------------------

# `sort -V` is not portable to the BSD userland on macOS; compare the way the
# compiler numbers itself instead. Returns 0 when $1 < $2.
ver_lt() {
  awk -v a="$1" -v b="$2" '
    BEGIN {
      na = split(a, x, "."); nb = split(b, y, ".");
      n = (na > nb ? na : nb);
      for (i = 1; i <= n; i++) {
        u = (i <= na ? x[i] + 0 : 0); v = (i <= nb ? y[i] + 0 : 0);
        if (u < v) exit 0;
        if (u > v) exit 1;
      }
      exit 1;
    }'
}

# GNU date and BSD date do not share a spelling for "parse this".
to_epoch() {
  date -d "$1" +%s 2>/dev/null || date -j -f '%Y-%m-%d' "$1" +%s 2>/dev/null
}

toolchain() {
  local ok=0

  local rustc_v cargo_v clippy_v fmt_v node_v npm_v
  rustc_v=$(rustc --version 2>/dev/null) || { printf 'rustc not on PATH\n'; return 1; }
  cargo_v=$(cargo --version 2>/dev/null)
  clippy_v=$(cargo clippy --version 2>/dev/null)
  fmt_v=$(cargo fmt --version 2>/dev/null)
  node_v=$(node --version 2>/dev/null)
  npm_v=$(npm --version 2>/dev/null)

  printf '  %s\n  %s\n  %s\n  %s\n  node %s / npm %s\n' \
    "$rustc_v" "$cargo_v" "$clippy_v" "$fmt_v" "${node_v:-?}" "${npm_v:-?}"

  local rustc_rel rustc_date
  rustc_rel=$(rustc -vV | sed -n 's/^release: //p')
  rustc_date=$(rustc -vV | sed -n 's/^commit-date: //p')

  # The floor the project claims to support. clippy.toml pins the same number;
  # if these two ever disagree, clippy starts recommending APIs the declared
  # minimum compiler does not have (D-014).
  local floor clippy_msrv
  floor=$(sed -n 's/^[[:space:]]*min-version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)
  clippy_msrv=$(sed -n 's/^[[:space:]]*msrv[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' src-tauri/clippy.toml)
  if [[ -n "$floor" && -n "$clippy_msrv" && "$floor" != "$clippy_msrv" ]]; then
    printf '  rust-toolchain.toml min-version (%s) != clippy.toml msrv (%s)\n' "$floor" "$clippy_msrv"
    ok=1
  fi
  if [[ -n "$floor" ]] && ver_lt "$rustc_rel" "$floor"; then
    printf '  rustc %s is below the declared floor %s\n' "$rustc_rel" "$floor"
    ok=1
  fi

  # A clippy left behind by a partial update lints with different rules than
  # the rustc everything else uses — the exact shape of the D-014 failure.
  # clippy numbers itself 0.1.<rustc minor>, so compare on major.minor.
  local clippy_rel
  clippy_rel=$(printf '%s' "$clippy_v" | sed -n 's/^clippy 0\.[0-9]*\.\([0-9]*\).*/1.\1/p')
  if [[ -n "$clippy_rel" && "$clippy_rel" != "${rustc_rel%.*}" ]]; then
    printf '  clippy is built against rustc %s but rustc is %s — run `rustup update`\n' \
      "$clippy_rel" "$rustc_rel"
    ok=1
  fi

  # rust-toolchain.toml tracks `stable`; anything else lints differently to CI.
  local channel active
  channel=$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)
  if command -v rustup >/dev/null 2>&1; then
    active=$(rustup show active-toolchain 2>/dev/null | head -1)
    if [[ -n "$channel" && "$active" != "$channel"* ]]; then
      warn "active toolchain is '${active%% *}' but rust-toolchain.toml says '$channel'; CI uses $channel"
    fi
  fi

  # Offline staleness heuristic. Stable ships every 6 weeks and CI always takes
  # the newest, so a local build older than one cycle is probably behind the
  # compiler that will judge this commit. A warning, not a failure: this has to
  # keep working on a plane.
  if [[ -n "$rustc_date" && "$rustc_date" != unknown ]]; then
    local built now age
    built=$(to_epoch "$rustc_date")
    now=$(date +%s)
    if [[ -n "$built" ]]; then
      age=$(( (now - built) / 86400 ))
      printf '  stable was built %s (%d days ago)\n' "$rustc_date" "$age"
      if (( age > 42 )); then
        warn "local stable is ${age} days old; CI installs current stable and clippy gained lints since. Run \`rustup update stable\` (D-014)."
      fi
    fi
  fi

  # Node major is pinned in the workflow; vite/vitest behaviour is not
  # guaranteed identical across majors.
  local ci_node local_node
  ci_node=$(sed -n 's/^[[:space:]]*node-version:[[:space:]]*\([0-9]*\).*/\1/p' .github/workflows/ci.yml | head -1)
  local_node=$(printf '%s' "${node_v#v}" | cut -d. -f1)
  if [[ -n "$ci_node" && -n "$local_node" && "$ci_node" != "$local_node" ]]; then
    warn "node $local_node locally, CI uses node $ci_node (.github/workflows/ci.yml)"
  fi

  # Opt-in, because it is the only thing in this script that touches a network.
  if (( CHECK_TOOLCHAIN )) && command -v rustup >/dev/null 2>&1; then
    printf '  querying rustup for the current stable...\n'
    local check
    if check=$(rustup check 2>/dev/null); then
      printf '%s\n' "$check" | sed 's/^/  /'
      if printf '%s' "$check" | grep -q 'stable.*Update available'; then
        warn "rustup reports a newer stable than this box has — run \`rustup update stable\` before trusting clippy (D-014)"
      fi
    else
      warn "rustup check failed (offline?); could not compare against current stable"
    fi
  fi

  return $ok
}

# ---------------------------------------------------------------------------
# 1. lockfiles
#
# CI runs `npm ci`, which refuses to install when package-lock.json and
# package.json disagree; nothing local ever noticed, so drift shipped. The
# dry run is the same check without the install (~0.7s, no network, and it
# does not touch node_modules).
#
# Cargo.lock has no equivalent guard at all: CI's rust-cache restores a lock
# that happens to work, so a manifest edit with no lock update fails nowhere.
# `cargo metadata --locked` is the resolution step on its own.
# ---------------------------------------------------------------------------
lockfiles() {
  local ok=0
  if ! npm ci --dry-run --no-audit --no-fund >/dev/null 2>&1; then
    printf '  package-lock.json disagrees with package.json — CI `npm ci` would refuse to install:\n'
    npm ci --dry-run --no-audit --no-fund 2>&1 | sed 's/^/    /' | head -20
    ok=1
  else
    printf '  package-lock.json satisfies package.json\n'
  fi

  local host err
  host=$(rustc -vV | sed -n 's/^host: //p')
  err=/tmp/jabot-cargo-lock-check.$$
  # --filter-platform keeps the resolve to this machine's target, so a box that
  # has built once does not need to fetch android/windows-only crates to answer
  # the question. --offline first so the common case provably cannot reach the
  # network; a stale lock sends cargo to the index, and a machine that has
  # never fetched would fail offline for a reason that is not the lock, so the
  # verdict is taken from the second attempt.
  if cargo metadata "${MANIFEST[@]}" "${LOCKED[@]}" --offline \
       --filter-platform "$host" --format-version 1 >/dev/null 2>/dev/null; then
    printf '  Cargo.lock satisfies src-tauri/Cargo.toml\n'
  elif cargo metadata "${MANIFEST[@]}" "${LOCKED[@]}" \
         --filter-platform "$host" --format-version 1 >/dev/null 2>"$err"; then
    printf '  Cargo.lock satisfies src-tauri/Cargo.toml (after a fetch)\n'
  else
    printf '  Cargo.lock does not satisfy src-tauri/Cargo.toml:\n'
    sed 's/^/    /' "$err" | head -20
    ok=1
  fi
  rm -f "$err"
  return $ok
}

# ---------------------------------------------------------------------------
# 2. bundle-config
#
# `npm run tauri build` cannot run here — it is macOS-only and bills at 10x, so
# it no longer runs on pull requests. The config that step reads can still be
# checked, and the ci.yml comment promises this gate by name. Everything below
# is a way packaging has broken or would break silently:
#
#   - bundle.targets losing "app" publishes a release with no updater archive.
#     The build succeeds and logs one warning (DEVIATIONS.md D-005).
#   - createUpdaterArtifacts=true hard-errors every unsigned build, CI's
#     included; it is merged in at release time with --config (D-005).
#   - a missing icon or an unparseable entitlements.plist fails at package or
#     codesign time, i.e. on macOS, i.e. now only on main.
#   - an undeclared src/bin/*.rs is auto-discovered by cargo, copied into
#     JaBot.app by the bundler, and kills the universal build after both full
#     compiles have finished (src-tauri/Cargo.toml explains it at length).
#   - default-run missing => "failed to find main binary".
# ---------------------------------------------------------------------------
bundle_config() {
  local ok=0

  node - <<'NODE' || ok=1
const fs = require('fs');
const path = require('path');

const errs = [];
const bad = (m) => errs.push(m);
const T = 'src-tauri/tauri.conf.json';

let conf;
try {
  conf = JSON.parse(fs.readFileSync(T, 'utf8'));
} catch (e) {
  console.log(`  ${T} does not parse: ${e.message}`);
  process.exit(1);
}

const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
const bundle = conf.bundle || {};

if (bundle.active !== true) bad(`${T}: bundle.active is ${JSON.stringify(bundle.active)}, expected true`);

// D-005: with ["dmg"] alone the build succeeds and emits no .app.tar.gz, so
// the update feed has nothing for installed copies to update from.
const targets = bundle.targets;
if (targets !== 'all' && !(Array.isArray(targets) && targets.includes('app'))) {
  bad(`${T}: bundle.targets must contain "app" (got ${JSON.stringify(targets)}) — `
    + 'without it tauri-bundler emits no updater archive and the release is unupdatable (D-005)');
}
if (targets !== 'all' && !(Array.isArray(targets) && targets.includes('dmg'))) {
  bad(`${T}: bundle.targets must contain "dmg" (got ${JSON.stringify(targets)}) — that is the installer users download`);
}

// D-005: true here makes `tauri build` fail unless TAURI_SIGNING_PRIVATE_KEY
// is set, which would break CI's bundle job and every laptop build.
if (bundle.createUpdaterArtifacts !== false) {
  bad(`${T}: bundle.createUpdaterArtifacts must stay false (got ${JSON.stringify(bundle.createUpdaterArtifacts)}); `
    + 'it is merged in at release time with --config (D-005)');
}

const icons = bundle.icon || [];
if (!Array.isArray(icons) || icons.length === 0) bad(`${T}: bundle.icon is empty`);
for (const icon of icons) {
  const p = path.join('src-tauri', icon);
  if (!fs.existsSync(p)) bad(`${T}: bundle.icon names ${icon}, which does not exist (${p})`);
  else if (fs.statSync(p).size === 0) bad(`${p} is empty`);
}

const mac = bundle.macOS || {};
if (mac.hardenedRuntime !== true) {
  bad(`${T}: bundle.macOS.hardenedRuntime must be true — notarization rejects the build without it`);
}
if (!mac.entitlements) bad(`${T}: bundle.macOS.entitlements is not set`);
else {
  const ep = path.join('src-tauri', mac.entitlements);
  if (!fs.existsSync(ep)) bad(`${T}: bundle.macOS.entitlements names ${mac.entitlements}, which does not exist (${ep})`);
}
if (!mac.minimumSystemVersion) bad(`${T}: bundle.macOS.minimumSystemVersion is not set`);

// The bundler runs beforeBuildCommand and then copies frontendDist. A typo in
// either is a macOS-only failure today.
const before = conf.build && conf.build.beforeBuildCommand;
const m = /^npm run ([\w:-]+)$/.exec(before || '');
if (!m) bad(`${T}: build.beforeBuildCommand ${JSON.stringify(before)} is not an "npm run <script>"`);
else if (!(pkg.scripts && pkg.scripts[m[1]])) bad(`${T}: build.beforeBuildCommand runs "npm run ${m[1]}", which package.json does not define`);
const dist = conf.build && conf.build.frontendDist;
if (!dist) bad(`${T}: build.frontendDist is not set`);
else if (!fs.existsSync(path.dirname(path.join('src-tauri', dist)))) {
  bad(`${T}: build.frontendDist ${dist} resolves outside the repo`);
}

// This version is what lands in Info.plist and what the updater compares
// against the release feed; package.json's is what everyone bumps.
if (conf.version !== pkg.version) {
  bad(`${T} version ${conf.version} != package.json version ${pkg.version} — the shipped app would report the wrong one`);
}
for (const k of ['productName', 'identifier']) {
  if (!conf[k]) bad(`${T}: ${k} is not set`);
}

// ---- src-tauri/Cargo.toml -------------------------------------------------
const C = 'src-tauri/Cargo.toml';
const cargo = fs.readFileSync(C, 'utf8');

if (!/^\s*default-run\s*=\s*"jabot"\s*$/m.test(cargo)) {
  bad(`${C}: default-run = "jabot" is missing — \`tauri build\` fails with "failed to find main binary"`);
}

// Every explicitly declared binary must be gated, and every file cargo would
// auto-discover must be one of those declarations.
const sections = cargo.split(/^\s*\[\[bin\]\]\s*$/m).slice(1);
const declared = new Map();
for (const sec of sections) {
  const body = sec.split(/^\s*\[/m)[0];
  const name = (/^\s*name\s*=\s*"([^"]+)"/m.exec(body) || [])[1];
  const bin = (/^\s*path\s*=\s*"([^"]+)"/m.exec(body) || [])[1];
  const feats = (/^\s*required-features\s*=\s*\[([^\]]*)\]/m.exec(body) || [])[1] || '';
  if (!name) { bad(`${C}: a [[bin]] section has no name`); continue; }
  if (!/"dev-bins"/.test(feats)) {
    bad(`${C}: [[bin]] ${name} is not gated behind required-features = ["dev-bins"] — `
      + 'the bundler copies every binary cargo reports into JaBot.app and the universal build then dies');
  }
  if (bin) declared.set(path.normalize(bin), name);
}
for (const want of ['jabot-hostd', 'fake-acp-agent']) {
  if (![...declared.values()].includes(want)) bad(`${C}: [[bin]] ${want} is gone; scripts/verify.sh builds it`);
}
const binDir = 'src-tauri/src/bin';
if (fs.existsSync(binDir)) {
  for (const f of fs.readdirSync(binDir)) {
    if (!f.endsWith('.rs')) continue;
    const rel = path.normalize(`src/bin/${f}`);
    if (!declared.has(rel)) {
      bad(`${binDir}/${f} is not declared as a gated [[bin]] in ${C} — cargo auto-discovers it, `
        + 'so `tauri build` would copy it into the app and fail the universal lipo');
    }
  }
}

if (errs.length) {
  for (const e of errs) console.log(`  ${e}`);
  process.exit(1);
}
console.log(`  ${T}: targets=${JSON.stringify(targets)}, ${icons.length} icons present, entitlements ${mac.entitlements}`);
console.log(`  ${C}: default-run set, ${sections.length} dev-bins-gated [[bin]] target(s)`);
NODE

  # A plist that does not parse fails at codesign time, which is macOS, which
  # is now only main. Prefer the real parsers; every one of these ships with
  # either macOS or a normal Linux box.
  local ent='src-tauri/entitlements.plist'
  if [[ ! -f "$ent" ]]; then
    printf '  %s is missing\n' "$ent"
    return 1
  fi
  if command -v plutil >/dev/null 2>&1; then
    if plutil -lint "$ent" >/tmp/jabot-plist.$$ 2>&1; then
      printf '  %s parses (plutil)\n' "$ent"
    else
      printf '  %s is not a valid plist:\n' "$ent"; sed 's/^/    /' /tmp/jabot-plist.$$ | tail -3; ok=1
    fi
    rm -f /tmp/jabot-plist.$$
  elif command -v python3 >/dev/null 2>&1; then
    if python3 -c 'import plistlib,sys; plistlib.load(open(sys.argv[1],"rb"))' "$ent" 2>/tmp/jabot-plist.$$; then
      printf '  %s parses (plistlib)\n' "$ent"
    else
      printf '  %s is not a valid plist:\n' "$ent"; sed 's/^/    /' /tmp/jabot-plist.$$ | tail -3; ok=1
    fi
    rm -f /tmp/jabot-plist.$$
  elif command -v xmllint >/dev/null 2>&1; then
    if xmllint --noout "$ent" 2>/tmp/jabot-plist.$$; then
      printf '  %s is well-formed XML (xmllint)\n' "$ent"
    else
      printf '  %s is not well-formed:\n' "$ent"; sed 's/^/    /' /tmp/jabot-plist.$$ | tail -3; ok=1
    fi
    rm -f /tmp/jabot-plist.$$
  else
    warn "no plutil/python3/xmllint on PATH; $ent was only checked for its plist envelope"
    grep -q '<plist' "$ent" && grep -q '</plist>' "$ent" || { printf '  %s has no <plist> envelope\n' "$ent"; ok=1; }
  fi

  return $ok
}

# ---------------------------------------------------------------------------
# 2b. commit guards
#
# scripts/checkpoint.sh and .githooks/pre-push are what stops an unverified
# tree becoming a commit now that CI is not the safety net. They are shell, so
# nothing else here would ever notice them breaking — and a guard that has
# quietly stopped guarding is worse than no guard, because everyone still
# believes it.
#
# scripts/tests/guards.test.sh builds throwaway repos with a stubbed gate and
# checks the refusals actually happen: a tree that moves mid-verification, a
# red gate, a push with a failing gate, --no-verify still working. ~5s, no
# network, no display. It never touches this repo.
# ---------------------------------------------------------------------------
guards() {
  local ok=0
  ./scripts/tests/guards.test.sh || ok=1

  # Hooks live in git config, not in the tree, so a fresh clone has none until
  # someone runs the installer (`npm install` does it). Warn rather than fail:
  # a CI checkout is allowed to be unhooked, a laptop really is not.
  if ! ./scripts/install-hooks.sh --check >/dev/null 2>&1; then
    warn "git hooks are not installed in this clone — nothing would stop an unverified push. Run ./scripts/install-hooks.sh (CONTRIBUTING.md)."
  fi
  return $ok
}

# ---------------------------------------------------------------------------
# The tree this run is about to describe.
#
# Empty when this is not a git worktree (a tarball, a vendored copy); every
# use below is guarded.
TREE_AT_START=$(jabot_tree_hash 2>/dev/null || printf '')

run "toolchain"      toolchain
run "lockfiles"      lockfiles
run "bundle-config"  bundle_config
run "commit guards"  guards
run "typecheck"      npx tsc --noEmit
run "unit tests"     npx vitest run --project unit
run "rust fmt"       cargo fmt "${MANIFEST[@]}" -- --check
run "rust clippy"    cargo clippy "${MANIFEST[@]}" "${LOCKED[@]}" "${DEV_BINS[@]}" --all-targets -- -D warnings
# Everything else in this script compiles the crate with `dev-bins` on. That is
# not the configuration `tauri build` compiles, and a cfg mistake between the
# two is invisible until the macOS job runs — which is now only on main. This
# is the one stage that compiles what actually ships. `check`, not `build`:
# type and cfg errors are the class this is for, and codegen would double the
# run. Costs ~1s when the Rust sources have not moved, ~20s when they have.
run "default-features check" cargo check "${MANIFEST[@]}" "${LOCKED[@]}"
run "rust tests"     cargo test "${MANIFEST[@]}" "${LOCKED[@]}" "${DEV_BINS[@]}"

if [[ $FAST -eq 0 ]]; then
  run "build jabot-hostd" cargo build "${MANIFEST[@]}" "${LOCKED[@]}" "${DEV_BINS[@]}" --bin jabot-hostd
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

# ---------------------------------------------------------------------------
# Did the thing being checked hold still while it was checked?
#
# This run takes about 90 seconds. An agent, a watch task, a formatter-on-save
# or a second terminal can write into the tree during them, and then "verify
# passed" is a statement about a tree that no longer exists. That is exactly
# how `error TS6133: 'client' is declared but its value is never read` reached
# CI from a green local run — the file was written after the check.
#
# So: say so. And when nothing moved and nothing failed, leave a note naming
# the tree that passed, which .githooks/pre-push uses to let the push straight
# through instead of spending another 90 seconds on identical bytes.
# scripts/checkpoint.sh is the strict version of this — it refuses to commit a
# tree that moved, where this only reports it.
# ---------------------------------------------------------------------------
if [[ -n "$TREE_AT_START" ]]; then
  TREE_AT_END=$(jabot_tree_hash 2>/dev/null || printf '')
  if [[ -n "$TREE_AT_END" && "$TREE_AT_END" != "$TREE_AT_START" ]]; then
    printf '\n'
    warn "the working tree changed WHILE these checks ran; the result above describes neither tree. Re-run, or use ./scripts/checkpoint.sh, which refuses to commit in this situation."
    git diff --name-status "$TREE_AT_START" "$TREE_AT_END" 2>/dev/null | sed 's/^/     /'
  elif [[ -n "$TREE_AT_END" && ${#FAILED[@]} -eq 0 ]]; then
    if [[ $FAST -eq 1 ]]; then STAMP_MODE=fast; else STAMP_MODE=full; fi
    jabot_stamp_verified "$TREE_AT_START" "$STAMP_MODE" || true
  fi
fi

printf '\n'
if [[ ${#WARNINGS[@]} -gt 0 ]]; then
  printf '\033[33m=== %d warning(s) ===\033[0m\n' "${#WARNINGS[@]}"
  printf '\033[33m  !! %s\033[0m\n' "${WARNINGS[@]}"
  printf '\n'
fi
if [[ ${#FAILED[@]} -eq 0 ]]; then
  printf '\033[32m=== all checks passed ===\033[0m\n'
  exit 0
fi
printf '\033[31m=== %d check(s) failed ===\033[0m\n' "${#FAILED[@]}"
printf '  - %s\n' "${FAILED[@]}"
exit 1
