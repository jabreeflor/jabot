#!/usr/bin/env bash
#
# JaBot installer — download the signed, notarized release DMG and put
# JaBot.app in /Applications.
#
#   curl -fsSL https://github.com/jabreeflor/jabot/releases/latest/download/install.sh | bash
#
# With options, `bash -s --` is how you get arguments past the pipe:
#
#   curl -fsSL .../install.sh | bash -s -- --version v0.2.0
#   curl -fsSL .../install.sh | bash -s -- --to ~/Applications --force
#
#   --version <v0.2.0>  install this release instead of the latest one
#   --to <dir>          install into <dir> instead of /Applications
#   --force             quit a running JaBot and replace it
#   --dry-run           resolve and print what would happen, download nothing
#   --team-id <TEAMID>  require this Apple Developer team on the signature
#   -h, --help          this text
#
# Environment: JABOT_VERSION, JABOT_INSTALL_DIR, JABOT_REPO, JABOT_TEAM_ID,
# JABOT_GITHUB_TOKEN (or GITHUB_TOKEN, only ever sent to api.github.com).
#
# This script is a release asset, uploaded by .github/workflows/release.yml, so
# the URL above always serves the installer that shipped with the latest
# published release rather than whatever is on main.
#
# What it refuses to do: install anything that is not signed by a Developer ID
# certificate, notarized by Apple, and carrying our bundle identifier. A
# downloaded-over-TLS-from-github.com DMG is not by itself evidence of
# anything — a redirected download or a compromised release asset would look
# identical to this script — so the signature on the .app inside is the actual
# gate, and it is checked before a single byte is copied into /Applications.
# There is deliberately no flag to skip that.
#
# macOS only: the release is a universal .app in a .dmg, and every verification
# below is codesign/spctl. Tested on Linux with a stubbed macOS toolchain by
# scripts/tests/install.test.sh, which is what scripts/verify.sh runs.
set -uo pipefail

# These four are pinned to src-tauri/tauri.conf.json. The "install script"
# stage of scripts/verify.sh fails if they drift apart, because every one of
# them is a check that silently stops checking when it goes stale: an APP_NAME
# that no longer matches the bundle installs nothing, and a BUNDLE_ID that no
# longer matches accepts any notarized app in the world.
REPO_DEFAULT="jabreeflor/jabot"
APP_NAME="JaBot.app"          # bundle.productName + ".app"
BUNDLE_ID="com.jabot.app"     # identifier
MIN_MACOS="13.0"              # bundle.macOS.minimumSystemVersion

# Empty until the first release is signed for real: the Developer ID team is
# not in this repo (docs/packaging.md keeps it in Actions secrets), and a
# fabricated value here would refuse every legitimate install. Once a release
# exists, read it off the shipped app — `codesign -dv --verbose=4
# /Applications/JaBot.app 2>&1 | grep TeamIdentifier` — and paste it here; it
# is public in every signature we ship, and pinning it narrows "any notarized
# Developer ID app" to "ours". Until then --team-id / JABOT_TEAM_ID lets a
# caller pin it themselves. See docs/packaging.md.
TEAM_ID_PIN=""

REPO="${JABOT_REPO:-$REPO_DEFAULT}"
INSTALL_DIR="${JABOT_INSTALL_DIR:-/Applications}"
VERSION="${JABOT_VERSION:-}"
TEAM_ID="${JABOT_TEAM_ID:-$TEAM_ID_PIN}"
FORCE=0
DRY_RUN=0

WORK=""
MOUNTED=""
STAGING=""

say()  { printf '%s\n' "$*"; }
step() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33m!!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

# Unmount before deleting the mountpoint, and delete the staging copy before
# leaving: a failed run must not leave a mounted volume in Finder or a
# half-written .app next to the real one.
cleanup() {
  if [ -n "$MOUNTED" ]; then
    hdiutil detach "$MOUNTED" -quiet 2>/dev/null || hdiutil detach "$MOUNTED" -force -quiet 2>/dev/null || true
    MOUNTED=""
  fi
  [ -n "$STAGING" ] && rm -rf "$STAGING"
  [ -n "$WORK" ] && rm -rf "$WORK"
  return 0
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || { printf 'error: --version needs a value\n' >&2; exit 2; }; VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --to)      [ $# -ge 2 ] || { printf 'error: --to needs a value\n' >&2; exit 2; }; INSTALL_DIR="$2"; shift 2 ;;
    --to=*)    INSTALL_DIR="${1#*=}"; shift ;;
    --team-id) [ $# -ge 2 ] || { printf 'error: --team-id needs a value\n' >&2; exit 2; }; TEAM_ID="$2"; shift 2 ;;
    --team-id=*) TEAM_ID="${1#*=}"; shift ;;
    --force)   FORCE=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'error: unknown option: %s (try --help)\n' "$1" >&2; exit 2 ;;
  esac
done

# ---------------------------------------------------------------------------
# Preflight
#
# Everything here is cheap and everything here is fatal later: a Linux box, a
# macOS older than the deployment target, or a missing tool all fail *after*
# the download otherwise, which is the worst place to find out.
# ---------------------------------------------------------------------------
have() { command -v "$1" >/dev/null 2>&1; }

# `sort -V` is not in the BSD userland; compare the fields.
ver_lt() { # a b -> 0 when a < b
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

preflight() {
  local os; os="$(uname -s 2>/dev/null || printf 'unknown')"
  [ "$os" = "Darwin" ] || die "JaBot is a macOS app; this is $os. There is no Linux or Windows build."

  local macos; macos="$(sw_vers -productVersion 2>/dev/null || printf '')"
  if [ -z "$macos" ]; then
    warn "could not read the macOS version (sw_vers); continuing, but JaBot needs macOS $MIN_MACOS or newer"
  elif ver_lt "$macos" "$MIN_MACOS"; then
    die "JaBot needs macOS $MIN_MACOS or newer; this is $macos."
  fi

  local missing="" t
  for t in curl hdiutil ditto codesign spctl; do
    have "$t" || missing="$missing $t"
  done
  [ -z "$missing" ] || die "missing required tool(s):$missing"
}

# ---------------------------------------------------------------------------
# Which release
#
# The tag comes from the /releases/latest redirect rather than the API: it does
# not count against the API's 60-an-hour-per-IP anonymous rate limit, which a
# `curl | bash` line behind an office NAT will find.
# ---------------------------------------------------------------------------
resolve_tag() {
  if [ -n "$VERSION" ]; then
    # Accept 0.2.0 and v0.2.0 both; tags are v-prefixed.
    case "$VERSION" in v*) printf '%s' "$VERSION" ;; *) printf 'v%s' "$VERSION" ;; esac
    return 0
  fi

  local url=""
  url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' --retry 3 --retry-delay 1 \
        "https://github.com/$REPO/releases/latest" 2>/dev/null)"
  # Some proxies refuse HEAD outright. One GET of the release page is a fine
  # price for the fallback, and it follows the same redirect.
  if [ -z "$url" ]; then
    url="$(curl -fsSL -o /dev/null -w '%{url_effective}' --retry 3 --retry-delay 1 \
          "https://github.com/$REPO/releases/latest" 2>/dev/null)"
  fi
  [ -n "$url" ] || die "could not reach github.com to find the latest JaBot release. Check your network, or pass --version."

  local tag="${url##*/}"
  # No published release at all redirects to .../releases — and a *draft*
  # release is invisible here, which is exactly right: docs/packaging.md makes
  # publishing the draft the act of shipping.
  case "$tag" in
    v[0-9]*) printf '%s' "$tag" ;;
    *) die "$REPO has no published release yet (github.com sent us to $url)." ;;
  esac
}

# The asset list is the truth about the DMG's name; the conventional name is
# the fallback for when the API says 403 because the whole office shares an IP.
resolve_dmg_url() { # tag -> url
  local tag="$1" version="${1#v}" api="" dmg="" auth=()
  local token="${JABOT_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}"
  [ -n "$token" ] && auth=(-H "Authorization: Bearer $token")

  api="$(curl -fsSL --retry 2 --retry-delay 1 \
        -H 'Accept: application/vnd.github+json' "${auth[@]+"${auth[@]}"}" \
        "https://api.github.com/repos/$REPO/releases/tags/$tag" 2>/dev/null)"
  if [ -n "$api" ]; then
    dmg="$(printf '%s' "$api" | tr ',' '\n' \
          | sed -n 's|.*"browser_download_url"[[:space:]]*:[[:space:]]*"\(https://[^"]*\.dmg\)".*|\1|p' \
          | head -1)"
  fi

  if [ -z "$dmg" ]; then
    # tauri-bundler names it <productName>_<version>_<arch>.dmg, and the
    # release builds --target universal-apple-darwin.
    dmg="https://github.com/$REPO/releases/download/$tag/${APP_NAME%.app}_${version}_universal.dmg"
  fi
  printf '%s' "$dmg"
}

# ---------------------------------------------------------------------------
# Is this actually ours
#
# Three questions, in the order that a failure is most informative: are the
# seals intact, is it our bundle, and did Apple notarize it. The third is the
# one that cannot be faked without Apple, and `spctl` is what Gatekeeper itself
# would say at first launch — asking it here means a rejected app never reaches
# /Applications rather than reaching it and refusing to open.
# ---------------------------------------------------------------------------
verify_app() { # path
  local app="$1" out=""

  out="$(codesign --verify --deep --strict --verbose=2 "$app" 2>&1)" || {
    say "$out" >&2
    die "the downloaded app's code signature does not verify. Do not install it; report this on the release."
  }

  out="$(codesign -dv --verbose=4 "$app" 2>&1)" || die "could not read the downloaded app's signature."

  local ident; ident="$(printf '%s' "$out" | sed -n 's/^Identifier=//p' | head -1)"
  [ "$ident" = "$BUNDLE_ID" ] || die "the downloaded app identifies itself as '${ident:-<none>}', not $BUNDLE_ID. Refusing to install it."

  local team; team="$(printf '%s' "$out" | sed -n 's/^TeamIdentifier=//p' | head -1)"
  if [ -n "$TEAM_ID" ]; then
    [ "$team" = "$TEAM_ID" ] || die "the downloaded app is signed by team '${team:-<none>}', not $TEAM_ID. Refusing to install it."
  fi

  # `-t install` is the assessment Gatekeeper runs for a downloaded app.
  # "accepted" alone is not enough: an ad-hoc or unnotarized Developer ID build
  # can be accepted on the machine that built it, and says so in `source=`.
  out="$(spctl -a -vvv -t install "$app" 2>&1)"
  case "$out" in
    *"source=Notarized Developer ID"*) ;;
    *) say "$out" >&2
       die "the downloaded app is not notarized by Apple. Refusing to install it." ;;
  esac

  printf '%s' "$team"
}

# ---------------------------------------------------------------------------
# Replacing a copy that is open
#
# Copying over a running .app leaves the running process with a bundle that no
# longer matches it, which is how "it updated and then crashed on the next
# click" happens. Refuse by default; --force quits it first, and quitting is
# something the user should have said yes to rather than something an install
# script decides on its own — there may be a thread mid-run in there.
# ---------------------------------------------------------------------------
app_is_running() { # path
  pgrep -f "$1/Contents/MacOS/" >/dev/null 2>&1
}

quit_app() { # path
  local app="$1" name i
  name="$(basename "$app" .app)"
  osascript -e "quit app \"$name\"" >/dev/null 2>&1 || true
  i=0
  while [ "$i" -lt 20 ]; do
    app_is_running "$app" || return 0
    sleep 0.5
    i=$((i + 1))
  done
  return 1
}

# ---------------------------------------------------------------------------
main() {
  preflight

  local tag; tag="$(resolve_tag)" || exit 1
  local dmg_url; dmg_url="$(resolve_dmg_url "$tag")"
  local target="$INSTALL_DIR/$APP_NAME"

  step "JaBot $tag"
  say "    from     $dmg_url"
  say "    into     $target"

  if [ "$DRY_RUN" -eq 1 ]; then
    say ""
    say "--dry-run: nothing was downloaded or installed."
    return 0
  fi

  if [ ! -d "$INSTALL_DIR" ]; then
    mkdir -p "$INSTALL_DIR" || die "$INSTALL_DIR does not exist and could not be created."
  fi
  [ -w "$INSTALL_DIR" ] || die "$INSTALL_DIR is not writable by $(id -un). Re-run with sudo, or install somewhere you own: --to \"\$HOME/Applications\"."

  if [ -d "$target" ] && app_is_running "$target"; then
    if [ "$FORCE" -eq 1 ]; then
      step "Quitting the running JaBot"
      quit_app "$target" || die "JaBot is still running and would not quit. Quit it and re-run."
    else
      die "JaBot is running from $target. Quit it first, or re-run with --force to have this script quit it."
    fi
  fi

  WORK="$(mktemp -d "${TMPDIR:-/tmp}/jabot-install.XXXXXX")" || die "could not create a temporary directory."
  local dmg="$WORK/jabot.dmg"

  step "Downloading"
  curl -fL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 --progress-bar -o "$dmg" "$dmg_url" \
    || die "download failed: $dmg_url"$'\n'"       If this release has no .dmg asset yet, the release is still building or still a draft."

  # The container's own signature is checked when it has one. Tauri signs the
  # .dmg but does not notarize it separately (docs/packaging.md says why), so
  # an unsigned container is a warning and the .app inside is the real gate —
  # a hard failure here would break the day the bundler stops signing it.
  if codesign -dv "$dmg" >/dev/null 2>&1; then
    codesign --verify --strict "$dmg" >/dev/null 2>&1 || die "the downloaded disk image is signed, but its signature does not verify. Refusing to open it."
  else
    warn "the downloaded disk image is not signed; the app inside it is still verified below."
  fi

  step "Mounting"
  local mnt="$WORK/mnt"
  mkdir -p "$mnt"
  hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mnt" >/dev/null \
    || die "could not mount $dmg"
  MOUNTED="$mnt"

  local src="$mnt/$APP_NAME"
  [ -d "$src" ] || die "the disk image does not contain $APP_NAME (found: $(ls "$mnt" 2>/dev/null | tr '\n' ' '))."

  step "Verifying signature and notarization"
  local team; team="$(verify_app "$src")" || exit 1
  say "    Developer ID, notarized by Apple${team:+, team $team}"

  # Copy out of the read-only image first, then swap: `ditto` preserves the
  # extended attributes and symlinks a plain `cp -R` drops, and staging inside
  # $INSTALL_DIR keeps the final move on one filesystem, so the window where
  # neither the old nor the new app is in place is a rename rather than a
  # multi-second copy.
  step "Installing"
  STAGING="$INSTALL_DIR/.jabot-install-$$.app"
  rm -rf "$STAGING"
  ditto "$src" "$STAGING" || die "could not copy the app into $INSTALL_DIR."

  if [ -d "$target" ]; then
    rm -rf "$target" || die "could not remove the existing $target."
  fi
  mv "$STAGING" "$target" || die "could not move the new app into place at $target."
  STAGING=""

  # No `xattr -dr com.apple.quarantine` here on purpose: curl does not set the
  # quarantine flag (LaunchServices does, for browser downloads), so there is
  # nothing to strip — and stripping it would be a way to launder an app that
  # the checks above are meant to reject.

  step "Installed $tag to $target"
  say ""
  say "    open -a \"${APP_NAME%.app}\""
  say ""
  say "    It updates itself from now on: new releases arrive through the"
  say "    in-app updater, so this script is only needed once."
}

main "$@"
