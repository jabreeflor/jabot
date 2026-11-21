#!/usr/bin/env bash
#
# Build JaBot Local from this checkout and put it in ~/Applications.
#
#   npm run local:install            # build + install "JaBot Local.app"
#   npm run local:install -- --to /Applications
#
# JaBot Local is the same app as the released JaBot ("Dev", the one that
# updates itself from main) under a different name and bundle identifier
# (src-tauri/tauri.local.conf.json). The two run side by side:
#
#   * separate app data — threads, bots, folders and the SQLite store live
#     under ~/Library/Application Support/com.jabot.app.local, so an
#     experiment here never touches Dev's history;
#   * shared harness credentials — both read the `com.jabot.app` Keychain
#     service, so you sign in to a harness once, not twice;
#   * no self-update — Local never replaces itself with what is on main
#     (src-tauri/src/updates.rs).
#
# For a quick loop without installing anything, `npm run local` is
# `tauri dev` with the same identity.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

DEST="$HOME/Applications"
while [ $# -gt 0 ]; do
  case "$1" in
    --to) DEST="$2"; shift 2 ;;
    --to=*) DEST="${1#*=}"; shift ;;
    -h|--help) sed -n '3,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; exit 2 ;;
  esac
done

[ "$(uname -s)" = "Darwin" ] || { printf 'local:install is macOS-only; use `npm run local` elsewhere.\n' >&2; exit 1; }

npx tauri build --config src-tauri/tauri.local.conf.json --bundles app

APP="src-tauri/target/release/bundle/macos/JaBot Local.app"
[ -d "$APP" ] || { printf 'build finished but %s is missing\n' "$APP" >&2; exit 1; }

mkdir -p "$DEST"
if pgrep -f "$DEST/JaBot Local.app/Contents/MacOS/" >/dev/null 2>&1; then
  printf 'JaBot Local is running from %s — quit it first.\n' "$DEST" >&2
  exit 1
fi
rm -rf "$DEST/JaBot Local.app"
ditto "$APP" "$DEST/JaBot Local.app"
printf 'Installed %s\n    open -a "JaBot Local"\n' "$DEST/JaBot Local.app"
