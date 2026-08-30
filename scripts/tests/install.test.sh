#!/usr/bin/env bash
#
# Tests for scripts/install.sh — the `curl | bash` installer.
#
# The installer is the first thing a new user runs and the only code we ship
# that is not covered by tsc, clippy or vitest. It is also the piece with the
# least forgiving failure mode: everything it refuses to do (install an
# unnotarized app, install somebody else's bundle, overwrite a running copy) is
# a refusal nobody notices when it stops happening. So the refusals get tests.
#
# It is a macOS script, and this suite runs on Linux in CI. Every macOS tool it
# touches — codesign, spctl, hdiutil, ditto, sw_vers, uname, pgrep, osascript,
# and curl itself — is stubbed into a directory at the front of $PATH, and each
# stub is scriptable from the environment, so a case can say "spctl reports an
# unnotarized app" and assert that nothing lands in the target directory. The
# script under test is the real one, run as a user runs it.
#
#   ./scripts/tests/install.test.sh          # all cases
#   ./scripts/tests/install.test.sh refuses  # cases whose name contains refuses
set -uo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
INSTALLER="$REPO_ROOT/scripts/install.sh"
FILTER="${1:-}"
FAILURES=0
COUNT=0
CASE=""

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/jabot-install.XXXXXX") || exit 1
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

BIN="$SANDBOX/bin"
mkdir -p "$BIN"

pass() { printf '  \033[32mok\033[0m   %s\n' "$CASE"; }
fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$CASE"
  printf '       %s\n' "$@"
  FAILURES=$((FAILURES + 1))
}
assert_eq() { # expected actual label
  if [ "$1" != "$2" ]; then fail "$3: expected [$1], got [$2]"; return 1; fi
}
assert_contains() { # haystack needle label
  case "$1" in
    *"$2"*) return 0 ;;
    *) fail "$3: [$2] not found in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-500)"; return 1 ;;
  esac
}
assert_not_contains() { # haystack needle label
  case "$1" in
    *"$2"*) fail "$3: [$2] should not be in: $(printf '%s' "$1" | tr '\n' '|' | cut -c1-500)"; return 1 ;;
    *) return 0 ;;
  esac
}

# --- the stubbed macOS toolchain -------------------------------------------
# Each stub appends what it was asked to $STUB_LOG, so a case can assert on the
# order of operations (verify before install, detach after failure) and not
# just on the end state.

cat > "$BIN/uname" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "${STUB_UNAME:-Darwin}"
STUB

cat > "$BIN/sw_vers" <<'STUB'
#!/usr/bin/env bash
[ "${STUB_NO_SW_VERS:-0}" = "1" ] && exit 1
printf '%s\n' "${STUB_MACOS:-14.5}"
STUB

# curl in three modes, told apart the way the installer calls it: a redirect
# probe (-w %{url_effective}), an api.github.com GET, and a download (-o FILE).
cat > "$BIN/curl" <<'STUB'
#!/usr/bin/env bash
out=""; url=""; want_url_effective=0
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -w) case "$2" in *url_effective*) want_url_effective=1 ;; esac; shift 2 ;;
    -H) shift 2 ;;
    --retry|--retry-delay) shift 2 ;;
    -*) shift ;;
    *) url="$1"; shift ;;
  esac
done
printf 'curl %s\n' "$url" >> "$STUB_LOG"

if [ "$want_url_effective" = "1" ]; then
  [ "${STUB_NO_NETWORK:-0}" = "1" ] && exit 22
  printf '%s' "${STUB_LATEST_URL:-https://github.com/jabreeflor/jabot/releases/tag/${STUB_LATEST_TAG:-v9.9.9}}"
  exit 0
fi

case "$url" in
  https://api.github.com/*)
    [ "${STUB_API_FAIL:-0}" = "1" ] && exit 22
    printf '{"tag_name":"x","assets":[{"name":"latest.json","browser_download_url":"%s"},{"name":"d.dmg","browser_download_url":"%s"}]}' \
      "${STUB_API_JSON_URL:-https://github.com/jabreeflor/jabot/releases/download/v9.9.9/latest.json}" \
      "${STUB_API_DMG_URL:-https://github.com/jabreeflor/jabot/releases/download/v9.9.9/JaBot_9.9.9_universal.dmg}"
    exit 0 ;;
esac

# a download
[ "${STUB_DOWNLOAD_FAIL:-0}" = "1" ] && exit 22
[ -n "$out" ] || exit 22
printf 'not-a-real-disk-image\n' > "$out"
exit 0
STUB

cat > "$BIN/hdiutil" <<'STUB'
#!/usr/bin/env bash
verb="$1"; shift
case "$verb" in
  attach)
    mnt=""
    while [ $# -gt 0 ]; do
      case "$1" in -mountpoint) mnt="$2"; shift 2 ;; *) shift ;; esac
    done
    printf 'hdiutil attach %s\n' "$mnt" >> "$STUB_LOG"
    [ "${STUB_ATTACH_FAIL:-0}" = "1" ] && exit 1
    app="$mnt/${STUB_DMG_APP_NAME:-JaBot.app}"
    mkdir -p "$app/Contents/MacOS"
    printf 'binary\n' > "$app/Contents/MacOS/JaBot"
    printf 'plist\n' > "$app/Contents/Info.plist"
    : > "$STUB_STATE/mounted"
    ;;
  detach)
    printf 'hdiutil detach %s\n' "$1" >> "$STUB_LOG"
    rm -f "$STUB_STATE/mounted"
    ;;
esac
exit 0
STUB

cat > "$BIN/codesign" <<'STUB'
#!/usr/bin/env bash
args="$*"
target=""
for a in "$@"; do case "$a" in -*) ;; *) target="$a" ;; esac; done
printf 'codesign %s\n' "$args" >> "$STUB_LOG"
case "$target" in
  *.dmg)
    [ "${STUB_DMG_SIGNED:-1}" = "1" ] || exit 1
    case "$args" in *--verify*) [ "${STUB_DMG_VERIFY_FAIL:-0}" = "1" ] && exit 1 ;; esac
    exit 0 ;;
esac
case "$args" in
  *--verify*)
    [ "${STUB_APP_VERIFY_FAIL:-0}" = "1" ] && { printf 'a sealed resource is missing or invalid\n' >&2; exit 1; }
    exit 0 ;;
  *-dv*)
    # Real codesign writes this block to stderr.
    {
      printf 'Executable=%s/Contents/MacOS/JaBot\n' "$target"
      printf 'Identifier=%s\n' "${STUB_IDENT:-com.jabot.app}"
      printf 'TeamIdentifier=%s\n' "${STUB_TEAM:-AB12CD34EF}"
    } >&2
    exit 0 ;;
esac
exit 0
STUB

cat > "$BIN/spctl" <<'STUB'
#!/usr/bin/env bash
target=""
for a in "$@"; do case "$a" in -*) ;; install) ;; *) target="$a" ;; esac; done
printf 'spctl %s\n' "$target" >> "$STUB_LOG"
case "${STUB_SPCTL:-notarized}" in
  notarized)   printf '%s: accepted\nsource=Notarized Developer ID\norigin=Developer ID Application: Someone (AB12CD34EF)\n' "$target"; exit 0 ;;
  unnotarized) printf '%s: accepted\nsource=Unnotarized Developer ID\n' "$target"; exit 0 ;;
  rejected)    printf '%s: rejected\nsource=no usable signature\n' "$target"; exit 3 ;;
esac
STUB

cat > "$BIN/ditto" <<'STUB'
#!/usr/bin/env bash
printf 'ditto %s -> %s\n' "$1" "$2" >> "$STUB_LOG"
[ "${STUB_DITTO_FAIL:-0}" = "1" ] && exit 1
mkdir -p "$(dirname "$2")" && cp -R "$1" "$2"
STUB

cat > "$BIN/pgrep" <<'STUB'
#!/usr/bin/env bash
[ -f "$STUB_STATE/running" ] && exit 0
exit 1
STUB

cat > "$BIN/osascript" <<'STUB'
#!/usr/bin/env bash
printf 'osascript %s\n' "$*" >> "$STUB_LOG"
[ "${STUB_WONT_QUIT:-0}" = "1" ] && exit 0
rm -f "$STUB_STATE/running"
exit 0
STUB

chmod +x "$BIN"/*

# --- running the installer under the stubs ---------------------------------
# Sets OUT and RC. Each case gets its own state dir, log and target directory.
run_install() { # case-name [args...]
  local name="$1"; shift
  export STUB_STATE="$SANDBOX/state-$name"
  export STUB_LOG="$SANDBOX/log-$name"
  APPS="$SANDBOX/apps-$name"
  mkdir -p "$STUB_STATE" "$APPS"
  : > "$STUB_LOG"
  OUT=$(PATH="$BIN:$PATH" JABOT_INSTALL_DIR="$APPS" "$INSTALLER" "$@" 2>&1)
  RC=$?
}
logged() { grep -c "$1" "$STUB_LOG" 2>/dev/null | tr -d ' '; }
unset_stubs() {
  unset STUB_UNAME STUB_MACOS STUB_NO_SW_VERS STUB_NO_NETWORK STUB_LATEST_URL \
        STUB_LATEST_TAG STUB_API_FAIL STUB_API_DMG_URL STUB_DOWNLOAD_FAIL \
        STUB_ATTACH_FAIL STUB_DMG_APP_NAME STUB_DMG_SIGNED STUB_DMG_VERIFY_FAIL \
        STUB_APP_VERIFY_FAIL STUB_IDENT STUB_TEAM STUB_SPCTL STUB_DITTO_FAIL \
        STUB_WONT_QUIT JABOT_TEAM_ID JABOT_VERSION 2>/dev/null || true
}

# ===========================================================================

case_help_exits_zero() {
  unset_stubs
  run_install help --help
  assert_eq 0 "$RC" "--help must exit 0" || return
  assert_contains "$OUT" "curl -fsSL" "help should show the one-liner" || return
  pass
}

case_unknown_flag_is_a_usage_error() {
  unset_stubs
  run_install badflag --wat
  assert_eq 2 "$RC" "an unknown flag must exit 2, not 1" || return
  assert_contains "$OUT" "unknown option" "should say what was wrong" || return
  pass
}

case_refuses_on_a_non_mac() {
  unset_stubs
  export STUB_UNAME=Linux
  run_install linux --dry-run
  assert_eq 1 "$RC" "a non-Darwin box must fail" || return
  assert_contains "$OUT" "macOS app" "should explain there is no Linux build" || return
  assert_eq 0 "$(logged curl)" "must not touch the network before the OS check" || return
  pass
}

case_refuses_an_old_macos() {
  unset_stubs
  export STUB_MACOS=12.7
  run_install oldmac --dry-run
  assert_eq 1 "$RC" "macOS below the deployment target must fail" || return
  assert_contains "$OUT" "13.0 or newer" "should name the minimum" || return
  pass
}

case_dry_run_resolves_the_latest_tag_and_downloads_nothing() {
  unset_stubs
  export STUB_LATEST_TAG=v1.2.3
  run_install dryrun --dry-run
  assert_eq 0 "$RC" "--dry-run should succeed" || { fail "$OUT"; return; }
  assert_contains "$OUT" "JaBot v1.2.3" "should report the resolved tag" || return
  assert_eq 0 "$(logged 'hdiutil attach')" "--dry-run must not mount anything" || return
  [ -e "$APPS/JaBot.app" ] && { fail "--dry-run installed something"; return; }
  pass
}

case_explicit_version_is_normalised_to_a_tag() {
  unset_stubs
  run_install version --dry-run --version 0.4.0
  assert_eq 0 "$RC" "should succeed" || { fail "$OUT"; return; }
  assert_contains "$OUT" "JaBot v0.4.0" "a bare 0.4.0 should become the tag v0.4.0" || return
  assert_eq 0 "$(logged 'releases/latest')" "an explicit version must not ask which release is latest" || return
  pass
}

case_refuses_when_no_release_is_published() {
  unset_stubs
  # What github.com actually does when a repo has only draft releases.
  export STUB_LATEST_URL="https://github.com/jabreeflor/jabot/releases"
  run_install norelease --dry-run
  assert_eq 1 "$RC" "no published release must fail" || return
  assert_contains "$OUT" "no published release" "should say the release is not published" || return
  pass
}

case_reports_an_unreachable_github() {
  unset_stubs
  export STUB_NO_NETWORK=1
  run_install offline --dry-run
  assert_eq 1 "$RC" "an unreachable github must fail" || return
  assert_contains "$OUT" "could not reach github.com" "should say what could not be reached" || return
  pass
}

case_a_rate_limited_api_falls_back_to_the_conventional_asset() {
  unset_stubs
  export STUB_API_FAIL=1 STUB_LATEST_TAG=v2.0.0
  run_install ratelimit --dry-run
  assert_eq 0 "$RC" "a 403 from the API must not be fatal" || { fail "$OUT"; return; }
  assert_contains "$OUT" "releases/download/v2.0.0/JaBot_2.0.0_universal.dmg" \
    "should fall back to the name tauri-bundler produces" || return
  pass
}

case_the_api_asset_url_wins_when_the_api_answers() {
  unset_stubs
  export STUB_API_DMG_URL="https://github.com/jabreeflor/jabot/releases/download/v2.0.0/JaBot_renamed.dmg"
  run_install apiwins --dry-run --version v2.0.0
  assert_eq 0 "$RC" "should succeed" || { fail "$OUT"; return; }
  assert_contains "$OUT" "JaBot_renamed.dmg" "the release's own asset list is the truth about the name" || return
  pass
}

case_installs_a_verified_app() {
  unset_stubs
  export STUB_LATEST_TAG=v1.0.0
  run_install install
  assert_eq 0 "$RC" "a notarized app should install" || { fail "$OUT"; return; }
  [ -d "$APPS/JaBot.app" ] || { fail "JaBot.app is not in the target dir"; return; }
  [ -f "$APPS/JaBot.app/Contents/MacOS/JaBot" ] || { fail "the bundle was not copied whole"; return; }
  assert_contains "$OUT" "Installed v1.0.0" "should report what it installed" || return
  # Verification has to happen before anything is copied, or the refusals below
  # are only cleaning up after themselves.
  local order; order=$(grep -n 'spctl\|ditto' "$STUB_LOG" | head -2 | cut -d: -f2- | tr '\n' ' ')
  assert_contains "$order" "spctl" "spctl must run" || return
  case "$order" in spctl*) ;; *) fail "the app was copied before it was verified: $order"; return ;; esac
  [ -f "$STUB_STATE/mounted" ] && { fail "the disk image was left mounted"; return; }
  [ -z "$(ls -A "$APPS" | grep '^\.jabot-install')" ] || { fail "a staging copy was left behind"; return; }
  pass
}

case_refuses_an_unnotarized_app() {
  unset_stubs
  export STUB_SPCTL=unnotarized
  run_install unnotarized
  assert_eq 1 "$RC" "an unnotarized app must be refused" || return
  assert_contains "$OUT" "not notarized" "should say why" || return
  [ -e "$APPS/JaBot.app" ] && { fail "an unnotarized app was installed"; return; }
  assert_eq 0 "$(logged ditto)" "nothing may be copied once verification fails" || return
  [ -f "$STUB_STATE/mounted" ] && { fail "the disk image was left mounted after a refusal"; return; }
  pass
}

case_refuses_a_rejected_signature() {
  unset_stubs
  export STUB_SPCTL=rejected
  run_install rejected
  assert_eq 1 "$RC" "a Gatekeeper rejection must be refused" || return
  [ -e "$APPS/JaBot.app" ] && { fail "a rejected app was installed"; return; }
  pass
}

case_refuses_a_broken_seal() {
  unset_stubs
  export STUB_APP_VERIFY_FAIL=1
  run_install brokenseal
  assert_eq 1 "$RC" "a failing codesign --verify must be refused" || return
  assert_contains "$OUT" "signature does not verify" "should say why" || return
  assert_eq 0 "$(logged spctl)" "a broken seal should fail before the notarization check" || return
  [ -e "$APPS/JaBot.app" ] && { fail "an app with a broken seal was installed"; return; }
  pass
}

case_refuses_a_foreign_bundle_identifier() {
  unset_stubs
  export STUB_IDENT="com.attacker.app"
  run_install foreignid
  assert_eq 1 "$RC" "another team's notarized app must be refused" || return
  assert_contains "$OUT" "com.attacker.app" "should name what it found" || return
  assert_contains "$OUT" "com.jabot.app" "should name what it wanted" || return
  [ -e "$APPS/JaBot.app" ] && { fail "a foreign bundle was installed"; return; }
  pass
}

case_refuses_a_team_that_does_not_match_the_pin() {
  unset_stubs
  export STUB_TEAM="ZZ99YY88XX" JABOT_TEAM_ID="AB12CD34EF"
  run_install teampin
  assert_eq 1 "$RC" "a team mismatch must be refused" || return
  assert_contains "$OUT" "signed by team" "should say why" || return
  [ -e "$APPS/JaBot.app" ] && { fail "a mismatched team was installed"; return; }
  pass
}

case_accepts_a_team_that_matches_the_pin() {
  unset_stubs
  export STUB_TEAM="AB12CD34EF" JABOT_TEAM_ID="AB12CD34EF"
  run_install teampinok
  assert_eq 0 "$RC" "the pinned team should install" || { fail "$OUT"; return; }
  [ -d "$APPS/JaBot.app" ] || { fail "nothing was installed"; return; }
  pass
}

case_refuses_a_disk_image_without_our_app_in_it() {
  unset_stubs
  export STUB_DMG_APP_NAME="SomethingElse.app"
  run_install wrongapp
  assert_eq 1 "$RC" "a disk image without JaBot.app must fail" || return
  assert_contains "$OUT" "does not contain JaBot.app" "should say what was missing" || return
  [ -f "$STUB_STATE/mounted" ] && { fail "the disk image was left mounted"; return; }
  pass
}

case_an_unsigned_disk_image_warns_but_the_app_is_still_checked() {
  unset_stubs
  export STUB_DMG_SIGNED=0
  run_install unsigneddmg
  assert_eq 0 "$RC" "an unsigned container is not fatal on its own" || { fail "$OUT"; return; }
  assert_contains "$OUT" "disk image is not signed" "should warn" || return
  assert_eq 1 "$(logged spctl)" "the app inside must still be assessed" || return
  pass
}

case_a_download_failure_is_reported() {
  unset_stubs
  export STUB_DOWNLOAD_FAIL=1
  run_install dlfail
  assert_eq 1 "$RC" "a failed download must fail" || return
  assert_contains "$OUT" "download failed" "should say so" || return
  assert_eq 0 "$(logged 'hdiutil attach')" "nothing to mount after a failed download" || return
  pass
}

case_refuses_to_replace_a_running_app() {
  unset_stubs
  run_install running
  : > "$STUB_STATE/running"
  mkdir -p "$APPS/JaBot.app"
  OUT=$(PATH="$BIN:$PATH" JABOT_INSTALL_DIR="$APPS" "$INSTALLER" 2>&1); RC=$?
  assert_eq 1 "$RC" "replacing a running app must be refused" || return
  assert_contains "$OUT" "--force" "should point at the flag that would do it" || return
  assert_eq 0 "$(logged osascript)" "must not quit anything without --force" || return
  pass
}

case_force_quits_a_running_app_and_replaces_it() {
  unset_stubs
  run_install forcequit
  : > "$STUB_STATE/running"
  mkdir -p "$APPS/JaBot.app/Contents"
  printf 'old\n' > "$APPS/JaBot.app/Contents/old-marker"
  OUT=$(PATH="$BIN:$PATH" JABOT_INSTALL_DIR="$APPS" "$INSTALLER" --force 2>&1); RC=$?
  assert_eq 0 "$RC" "--force should install over a running app" || { fail "$OUT"; return; }
  [ -e "$APPS/JaBot.app/Contents/old-marker" ] && { fail "the old bundle was merged into, not replaced"; return; }
  [ -f "$APPS/JaBot.app/Contents/MacOS/JaBot" ] || { fail "the new bundle is not there"; return; }
  pass
}

case_force_gives_up_when_the_app_will_not_quit() {
  unset_stubs
  export STUB_WONT_QUIT=1
  run_install wontquit
  : > "$STUB_STATE/running"
  mkdir -p "$APPS/JaBot.app"
  OUT=$(PATH="$BIN:$PATH" JABOT_INSTALL_DIR="$APPS" "$INSTALLER" --force 2>&1); RC=$?
  assert_eq 1 "$RC" "an app that will not quit must stop the install" || return
  assert_contains "$OUT" "would not quit" "should say what happened" || return
  pass
}

case_an_upgrade_replaces_the_old_bundle_wholesale() {
  unset_stubs
  run_install upgrade
  mkdir -p "$APPS/JaBot.app/Contents"
  printf 'stale\n' > "$APPS/JaBot.app/Contents/stale-file"
  OUT=$(PATH="$BIN:$PATH" JABOT_INSTALL_DIR="$APPS" "$INSTALLER" 2>&1); RC=$?
  assert_eq 0 "$RC" "an upgrade should succeed" || { fail "$OUT"; return; }
  [ -e "$APPS/JaBot.app/Contents/stale-file" ] && { fail "a file from the previous version survived the upgrade"; return; }
  pass
}

case_refuses_an_unwritable_target_directory() {
  unset_stubs
  run_install unwritable --dry-run   # sets up $APPS
  chmod 555 "$APPS"
  OUT=$(PATH="$BIN:$PATH" JABOT_INSTALL_DIR="$APPS" "$INSTALLER" 2>&1); RC=$?
  chmod 755 "$APPS"
  if [ "$(id -u)" = "0" ]; then
    # root ignores the write bit, so there is nothing to refuse.
    assert_eq 0 "$RC" "root can write anywhere" || { fail "$OUT"; return; }
    pass
    return
  fi
  assert_eq 1 "$RC" "an unwritable target must fail" || return
  assert_contains "$OUT" "not writable" "should say why" || return
  assert_contains "$OUT" "--to" "should suggest a directory the user owns" || return
  pass
}

case_installs_into_a_directory_given_with_to() {
  unset_stubs
  run_install toflag --dry-run
  local elsewhere="$SANDBOX/elsewhere/Applications"
  OUT=$(PATH="$BIN:$PATH" "$INSTALLER" --to "$elsewhere" 2>&1); RC=$?
  assert_eq 0 "$RC" "--to should create and use the directory" || { fail "$OUT"; return; }
  [ -d "$elsewhere/JaBot.app" ] || { fail "nothing was installed into $elsewhere"; return; }
  pass
}

# ===========================================================================
CASES=$(grep -o '^case_[a-z0-9_]*' "${BASH_SOURCE[0]}" | sort -u)
printf '\n\033[1minstall script\033[0m (%s)\n' "$SANDBOX"
for c in $CASES; do
  case "$c" in *"$FILTER"*) ;; *) continue ;; esac
  CASE="${c#case_}"
  COUNT=$((COUNT + 1))
  "$c"
done

printf '\n'
if [ "$FAILURES" -eq 0 ]; then
  printf '\033[32m%d installer case(s) passed\033[0m\n' "$COUNT"
  exit 0
fi
printf '\033[31m%d of %d installer case(s) failed\033[0m\n' "$FAILURES" "$COUNT"
exit 1
