# Packaging: signing, notarization, updater

**Issue:** #12
**Status:** Partially implemented — `src-tauri/entitlements.plist`, `src-tauri/tauri.conf.json`, `src-tauri/icons/`, `docs/packaging.md`, `scripts/install.sh`

## What it is

Turning the Tauri build into a distributable, trusted macOS app: Developer
ID code signing, Apple notarization, and an update mechanism so users get
new builds without reinstalling by hand.

## Why

An unsigned, unnotarized macOS binary is blocked by Gatekeeper by default;
without an updater, every fix requires a manual reinstall. This is the
last mile between "builds locally" and "someone else can run it."

## Requirements

1. Release builds are signed with a Developer ID Application certificate;
   the signing identity and entitlements come from
   `src-tauri/entitlements.plist` and must not request more capability
   than the app uses (hardened runtime, no unnecessary entitlements).
2. Signed builds are submitted for Apple notarization and stapled before
   distribution; an unnotarized build is not shipped to users.
3. `src-tauri/tauri.conf.json` declares the bundle identifier, version,
   and updater configuration consistently with the signing identity.
4. App icons for every required macOS size live under
   `src-tauri/icons/` and are wired into the bundle config.
5. An update channel lets an installed app detect and install a newer
   signed, notarized build without the user re-downloading a DMG by
   hand.
6. Packaging steps are documented in [`docs/packaging.md`](../packaging.md)
   so a release can be reproduced by someone other than the original
   author — that document, not this file, is the source of truth for
   exact commands/secrets handling.
7. Installing the first copy is one command that needs nothing installed
   first (`curl -fsSL .../install.sh | bash`), and that installer refuses
   any build that is not signed, notarized, and carrying our bundle
   identifier — the check happens before anything is written to
   `/Applications`. It is served from the release itself, so the script a
   user runs is the one that shipped with the build it installs.
8. CI's macOS bundle job is explicitly **not** the gate for this
   (see the "Working on it" section of the top-level
   [`README.md`](../../README.md)) — packaging must be verifiable via
   local scripts/tooling, not assumed to run on every PR.
