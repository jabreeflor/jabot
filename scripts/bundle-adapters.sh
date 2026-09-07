#!/usr/bin/env bash
#
# Stage the ACP adapters JaBot ships inside its own app bundle.
#
#   ./scripts/bundle-adapters.sh          # install + prune, idempotent
#   ./scripts/bundle-adapters.sh --check  # is the staged tree present and current?
#   ./scripts/bundle-adapters.sh --clean  # delete it
#
# Why this exists: the Claude card's adapter is an npm package, and until now
# the app only ever *told* people to install it. A user with Claude Code
# already working got `Harness unavailable: claude-agent-acp` on every launch
# and a hint to run npm, which is not something a desktop app should ask for.
# The adapter is small, pure JavaScript, and versioned with us — so it ships
# with the app, staged here at build time and copied into
# `JaBot.app/Contents/Resources` by `bundle.resources` in tauri.conf.json.
#
# What is deliberately NOT staged: `@anthropic-ai/claude-agent-sdk`'s
# platform-specific optional dependencies. Each one is a ~200 MB copy of the
# Claude Code binary, and the whole point of the Claude card is to drive the
# Claude Code the user already has (`cli: Some("claude")` in the catalog). So
# `--omit=optional` here, and `harness/bundled.rs` points the adapter at the
# resolved `claude` with `CLAUDE_CODE_EXECUTABLE`. Without that env var the
# SDK raises "Native CLI binary not found" — which is why the bundled launch
# is only offered when `claude` itself resolves.
#
# This runs before anything cargo: `tauri-build` reads bundle.resources at
# build-script time and fails when a glob matches nothing, so an unstaged tree
# breaks `cargo check` and not only `tauri build`. `scripts/live.sh setup` and
# CI's verify job both run this first for that reason.
#
# Codex and Pi are not staged. `@agentclientprotocol/codex-acp` depends on
# `@openai/codex`, i.e. on shipping OpenAI's CLI and its per-platform native
# binaries inside JaBot; Pi's card already carries an `npx -y pi-acp` fallback
# so it never reports unavailable on a machine with Node.
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

VENDOR=src-tauri/vendor/adapters
MODULES="$VENDOR/node_modules"
STAMP="$MODULES/.jabot-bundle-stamp"
# The entry point `harness/bundled.rs` looks for. If pruning ever removed it,
# the app would ship a resource directory that resolves to nothing.
ENTRY="$MODULES/@agentclientprotocol/claude-agent-acp/dist/index.js"

say()  { printf '\033[1m> %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m  %s\033[0m\n' "$*"; }
die()  { printf '\033[31mbundle-adapters: %s\033[0m\n' "$*" >&2; exit 1; }

MODE=install
case "${1:-}" in
  "")        ;;
  --check)   MODE=check ;;
  --clean)   MODE=clean ;;
  -h|--help) sed -n '3,7p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *)         die "unknown option: $1 (try --help)" ;;
esac

[ -f "$VENDOR/package.json" ]      || die "$VENDOR/package.json is missing"
[ -f "$VENDOR/package-lock.json" ] || die "$VENDOR/package-lock.json is missing — run \`npm install\` in $VENDOR and commit the lock"

# The stamp is the lockfile's hash plus the prune revision, so a version bump
# or a change to what we strip re-stages, and nothing else does. A build that
# re-ran `npm ci` on every `tauri build` would add ten seconds to every one.
PRUNE_REV=2
lock_hash() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$VENDOR/package-lock.json" | cut -d' ' -f1
  else sha256sum "$VENDOR/package-lock.json" | cut -d' ' -f1
  fi
}
want_stamp() { printf '%s prune=%s\n' "$(lock_hash)" "$PRUNE_REV"; }

current() {
  [ -f "$ENTRY" ] || return 1
  [ -f "$STAMP" ] || return 1
  [ "$(cat "$STAMP")" = "$(want_stamp)" ]
}

if [ "$MODE" = clean ]; then
  rm -rf "$MODULES"
  ok "removed $MODULES"
  exit 0
fi

if [ "$MODE" = check ]; then
  if current; then
    ok "bundled adapters staged ($(du -sh "$MODULES" 2>/dev/null | cut -f1))"
    exit 0
  fi
  die "the bundled adapters are missing or stale — run ./scripts/bundle-adapters.sh"
fi

if current; then
  ok "bundled adapters up to date ($(du -sh "$MODULES" | cut -f1))"
  exit 0
fi

command -v npm >/dev/null 2>&1 || die "npm is required to stage the bundled adapters"

say "staging bundled adapters into $MODULES"
rm -rf "$MODULES"
# `npm ci` so the staged tree is exactly the committed lock; `--omit=optional`
# for the 200 MB reason above; `--ignore-scripts` because nothing in this tree
# should be running install hooks on a build machine.
npm ci --prefix "$VENDOR" --omit=dev --omit=optional --ignore-scripts --no-audit --no-fund \
  || die "npm ci failed in $VENDOR"

[ -f "$ENTRY" ] || die "npm ci finished but $ENTRY is not there"

# Prune what Node never reads at run time — more than half the tree, and every
# megabyte here is a megabyte in the DMG, in the updater archive, and in what a
# user downloads.
#
# TypeScript of any kind, because Node loads none of it: `.d.ts` declarations
# and the `src/` trees packages publish next to their build (zod's is 6 MB of
# tests and locales on its own). Source maps likewise — nothing in a shipped
# app bundle is ever opened in a debugger. And `.bin`, which is a directory of
# symlinks the bundler would not copy anyway: `bundled.rs` spawns
# `node <dist/index.js>` by absolute path, never through a shim.
before=$(du -sk "$MODULES" | cut -f1)
find "$MODULES" -type f \( \
     -name '*.ts' -o -name '*.mts' -o -name '*.cts' -o -name '*.tsx' \
  -o -name '*.map' \
  -o -iname '*.md' -o -iname '*.markdown' \
  \) -delete 2>/dev/null
rm -rf "$MODULES/.bin"
after=$(du -sk "$MODULES" | cut -f1)

# Prove the tree still runs before stamping it: a prune that broke the adapter
# would otherwise be found by a user, on a machine, after a release.
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false}}}}\n' \
  | CLAUDE_CODE_EXECUTABLE=/nonexistent/claude node "$ENTRY" 2>/dev/null \
  | grep -q '"protocolVersion"' \
  || die "the staged adapter did not answer ACP initialize — the prune list above is wrong"

want_stamp > "$STAMP"
ok "staged $(( before / 1024 )) MB -> $(( after / 1024 )) MB at $MODULES"
