# Packaging: Developer ID signing, notarization, updater

Runbook for [#12](https://github.com/jabreeflor/jabot/issues/12). What ships,
what secrets it needs, how to cut a release, and how to prove the result is
actually notarized rather than merely built.

**Channel:** direct download of a signed and notarized universal `.dmg`. Not
the Mac App Store — the store requires App Sandbox, and a sandboxed app cannot
exec a harness from the user's PATH or supervise its process group. The
reasoning is in
[app-shell/process-architecture](research/app-shell/process-architecture.md#packaging).

| Piece | Where it lives |
|---|---|
| Release pipeline | [`.github/workflows/release.yml`](../.github/workflows/release.yml) |
| Bundle + updater config | [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json) |
| Entitlements (and the audit) | [`src-tauri/entitlements.plist`](../src-tauri/entitlements.plist) |
| Update feed | `https://github.com/jabreeflor/jabot/releases/latest/download/latest.json` |
| Installer (`curl \| bash`) | [`scripts/install.sh`](../scripts/install.sh), uploaded as a release asset |
| Packaged-app acceptance (#235) | [`docs/macos-acceptance.md`](macos-acceptance.md), [`scripts/macos-acceptance.sh`](../scripts/macos-acceptance.sh) |
| Windows install + gaps (#287) | [`docs/windows.md`](windows.md), smoke checklist [`windows-acceptance.md`](windows-acceptance.md). Tracking epic [#280](https://github.com/jabreeflor/jabot/issues/280). |
| Windows CI compile / optional NSIS (#286) | [`docs/windows-ci.md`](windows-ci.md), [`.github/workflows/windows.yml`](../.github/workflows/windows.yml) |

---

## Secrets

All eight are repository secrets (**Settings → Secrets and variables →
Actions**). Nothing in this repo contains a key, a certificate, a password, or
a team ID, and nothing should.

| Secret | What it is | How to get it |
|---|---|---|
| `APPLE_CERTIFICATE` | Base64 of the **Developer ID Application** certificate + private key, as a `.p12` | below |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting that `.p12` | you choose it at export time |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: NAME (TEAMID)` | `security find-identity -v -p codesigning` |
| `APPLE_ID` | Apple ID of an Apple Developer Program account | the account itself |
| `APPLE_TEAM_ID` | 10-character team identifier | [developer.apple.com/account](https://developer.apple.com/account) → Membership details; it is also the string in parentheses in the signing identity |
| `APPLE_PASSWORD` | **App-specific** password, not the account password | [appleid.apple.com](https://appleid.apple.com) → Sign-In and Security → App-Specific Passwords |
| `TAURI_SIGNING_PRIVATE_KEY` | Updater's Ed25519 private key (base64, one line) | below |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for that key | you choose it at generation time |

The Apple signing and the updater signing are two unrelated keypairs. Apple's
proves the app came from us to Gatekeeper; the updater's proves an update
archive came from us to an already-installed copy. Tauri does not let the
second one be disabled.

### Developer ID certificate → `APPLE_CERTIFICATE`

Requires a paid Apple Developer Program membership.

1. Xcode → Settings → Accounts → Manage Certificates → **+** → **Developer ID
   Application**. (Or create a CSR in Keychain Access and request the
   certificate at [developer.apple.com](https://developer.apple.com/account/resources/certificates).)
2. Keychain Access → **My Certificates** → right-click the *Developer ID
   Application* entry → Export → `.p12`. Set a password; that is
   `APPLE_CERTIFICATE_PASSWORD`.
3. Encode it:

   ```sh
   base64 -i DeveloperID.p12 | pbcopy
   ```

   Paste as `APPLE_CERTIFICATE`, then delete the `.p12`.

Export the certificate **with** its private key — a `.p12` exported from the
*Certificates* category instead of *My Certificates* has no key in it and the
build fails at import with no useful message.

### Updater keypair → `TAURI_SIGNING_PRIVATE_KEY` + `pubkey`

```sh
npm run tauri signer generate -- -w ~/.tauri/jabot.key
```

That writes two single-line base64 files. Neither needs further encoding.

| File | Goes to |
|---|---|
| `~/.tauri/jabot.key` | secret `TAURI_SIGNING_PRIVATE_KEY` |
| `~/.tauri/jabot.key.pub` | `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`, committed |

`tauri.conf.json` currently holds `REPLACE_ME__run__npm_run_tauri_signer_generate__see_docs_packaging.md`
rather than a fabricated key. The release workflow's preflight step fails fast
on that placeholder, so the first real release cannot ship an unverifiable
feed by accident.

**Back this key up somewhere that is not a laptop.** Losing it does not break
new installs — it breaks *updates for every existing install*, permanently.
Recovery is telling users to re-download by hand.

---

## What ships inside the bundle: the Claude ACP adapter

`bundle.resources` in `tauri.conf.json` copies
`src-tauri/vendor/adapters/node_modules/**/*` into `Contents/Resources`.
That is the ACP adapter for the Claude card,
[`@agentclientprotocol/claude-agent-acp`](https://github.com/agentclientprotocol/claude-agent-acp),
pinned in `src-tauri/vendor/adapters/package.json` and installed from the lock
next to it by [`scripts/bundle-adapters.sh`](../scripts/bundle-adapters.sh).
`beforeBuildCommand` is `npm run build:app`, which runs that script before the
renderer build, so a `tauri build` always packages a staged tree.

Why it is in the bundle at all: installing JaBot used to leave a second install
to do by hand, and a user with Claude Code working got `Harness unavailable:
claude-agent-acp` on every launch with no way out of it from inside the app.
`src-tauri/src/host/harness/bundled.rs` finds this copy and offers it as the
Claude card's **last** launch candidate — an adapter the user installed
themselves still wins.

Three things about it matter here:

- **No native code.** The staged tree is JavaScript and JSON only, so nothing
  in `Contents/Resources` needs its own signature and none of it changes the
  notarization story. Keep it that way: a dependency that ships a `.node`, a
  `.dylib` or a platform binary would have to be signed with the app, and
  `codesign --verify --deep` will say so at release time rather than at build
  time.
- **`--omit=optional` is load-bearing.** `@anthropic-ai/claude-agent-sdk`
  publishes the Claude Code binary itself as per-platform optional
  dependencies, ~200 MB each. We omit them and point the adapter at the user's
  own `claude` with `CLAUDE_CODE_EXECUTABLE`, which is also why the bundled
  candidate is only offered on a machine where `claude` resolves.
- **Node is not bundled.** The adapter runs on the user's Node (20+). Without
  one the card falls back to the install hint, exactly as before.
- **An unstaged tree fails early and loudly.** `tauri-build` reads
  `bundle.resources` from the build script and errors when a glob matches
  nothing, so a clone that never staged the adapters fails `cargo check`, not
  just `tauri build`. `scripts/verify.sh` catches it first, in `bundle-config`,
  with the command to run.

To change the pinned version: edit `src-tauri/vendor/adapters/package.json`,
run `npm install` in that directory, commit both files, and re-run
`./scripts/bundle-adapters.sh`. `./scripts/verify.sh` fails if the two disagree.

---

## What consumes the feed

`tauri.conf.json` names the endpoint and carries the `pubkey`, but both are
read by the *bundler*. Nothing checks for an update unless the plugin is
registered at runtime, which happens in the `Builder` setup in
`src-tauri/src/lib.rs`:

```rust
#[cfg(target_os = "macos")]
app.handle()
    .plugin(tauri_plugin_updater::Builder::new().build())?;
```

Checking from Rust needs no capability entry. If the renderer is ever the one
to trigger a check, it also needs `updater:default` in
`src-tauri/capabilities/`.

A green release run is not by itself evidence that updates work: it proves the
artifacts were signed and published, not that an installed copy accepts them.
That is what the feed verification below is for. Archive *presence*
(`.app.tar.gz` + `.sig`) is checked automatically by
`./scripts/macos-acceptance.sh updater-artifacts` at the end of
`release.yml`. Installing a signed update is a named manual release step —
see [macos-acceptance.md](macos-acceptance.md) (#235). Do not treat the
headless macOS bundle job, or a Playwright WebKit run, as that step.

---

## Cutting a release

1. Bump `version` in `src-tauri/tauri.conf.json`. The workflow refuses to build
   if it disagrees with the tag, because a mismatch silently produces a
   `latest.json` no installed copy will ever accept.
2. Merge to `main`.
3. Tag and push:

   ```sh
   git tag v0.2.0 && git push origin v0.2.0
   ```

4. The `Release` workflow builds `universal-apple-darwin`, signs with hardened
   runtime and `entitlements.plist`, notarizes through `notarytool`, staples
   the ticket, and uploads to a **draft** GitHub Release: the `.dmg`, the
   `.app.tar.gz` + `.sig`, `latest.json`, and `install.sh`.
5. Download the `.dmg` and run the verification below.
6. **Publish the draft.** Publishing is the act of shipping: it is what makes
   `releases/latest/download/latest.json` point here, and every installed copy
   starts offering the update immediately.

Notarization is the slow part — usually a few minutes, occasionally hours on a
brand new team. The build waits for it.

To retry a failed release, delete the draft release and the tag, then push the
tag again; the workflow is not re-runnable from the Actions UI because
`workflow_dispatch` cannot target a tag ref.

### Why every action in `release.yml` is a 40-character SHA

This job has all eight secrets in its environment and every step of it shares
one runner, so any third-party action in it can reach them. A tag can be
force-moved and `dtolnay/rust-toolchain@stable` is a *branch*, which moves by
design — either would let an upstream compromise change what runs here with no
change to this repo, and a stolen `TAURI_SIGNING_PRIVATE_KEY` cannot be
rotated without stranding every install. So the pins are deliberate, not
noise; the trailing comment names the human-readable version.

To bump one, resolve the ref and paste the SHA:

```sh
git ls-remote https://github.com/tauri-apps/tauri-action refs/tags/v1
```

Read the diff between the old pin and the new one before taking it. `ci.yml`
holds no secrets and is left on floating tags on purpose — it is not worth the
maintenance there.

### Why `createUpdaterArtifacts` is set in the workflow, not the config

With it enabled, `tauri build` hard-errors unless `TAURI_SIGNING_PRIVATE_KEY`
is set — which would break every unsigned build, including the `bundle` job in
`ci.yml` and anyone running `npm run tauri build` on a laptop. So the base
config leaves it off and `release.yml` merges it in with `--config`. If
`ci.yml`'s bundle job ever grows a `--no-sign` flag, this can move back into
`tauri.conf.json` where it reads more naturally.

`bundle.targets` is `["app", "dmg"]` for a related reason: on macOS the
bundler only emits the updater archive when the plain `app` target is in the
list. With `["dmg"]` alone the build succeeds, logs one warning, and publishes
a release with no `.app.tar.gz` — a feed nobody can update from. The raw
`.app` is not uploaded twice; tauri-action drops it once the signed
`.app.tar.gz` exists.

### Why the test binaries are behind a cargo feature

`src-tauri` has three binaries; only `jabot` is the app. The bundler copies
*every* binary cargo reports into `JaBot.app/Contents/MacOS`, and for
`--target universal-apple-darwin` the tooling lipos only the main one into the
universal output directory — so an ungated build shipped the test binaries on
a plain `tauri build` and hard-failed on the universal one, after both full
release compiles had finished. `jabot-hostd` and `fake-acp-agent` therefore
carry `required-features = ["dev-bins"]`, which makes them cargo targets only
when asked for. `scripts/verify.sh` asks; `tauri build` does not. Anything
that builds or runs them by hand needs `--features dev-bins` too.

---

## The installer

New users have nothing of ours on their machine yet, so the entry point is one
line:

```sh
curl -fsSL https://github.com/jabreeflor/jabot/releases/latest/download/install.sh | bash
```

`scripts/install.sh` resolves the latest tag, downloads that release's `.dmg`,
mounts it, and — before it copies anything — asks the same three questions the
["Verifying notarization" section](#verifying-notarization-succeeded) asks by
hand: does the signature verify, is the bundle identifier `com.jabot.app`, and
does `spctl -a -t install` say `source=Notarized Developer ID`. Any one of
those failing aborts the install with the app still on the disk image. There
is no flag to skip them, because a flag to skip them is the flag an attacker
tells the user to pass.

It then `ditto`s the app to a staging path inside the target directory and
`mv`s it into place, so the moment where neither the old nor the new app
exists is a rename rather than a multi-second copy. Arguments go after `bash -s
--`: `--version`, `--to`, `--force`, `--team-id`, `--dry-run`.

**It is served from the release, not from `main`.** The last step of
`release.yml` uploads `scripts/install.sh` to the draft release, which is what
makes `releases/latest/download/install.sh` resolve — and it means the
installer a new user runs is the one that shipped with the release it installs,
not whatever landed on the default branch this morning. A
`raw.githubusercontent.com/.../main/scripts/install.sh` URL would give away
both of those properties; do not publish one.

### Pin the team ID after the first real release

`TEAM_ID_PIN` at the top of `scripts/install.sh` is empty. Without it the
notarization check proves the app was notarized by *somebody* with a Developer
ID, and the identifier check is what narrows that to us. Once a release has
been signed for real, read the team off it and commit it:

```sh
codesign -dv --verbose=4 /Applications/JaBot.app 2>&1 | grep TeamIdentifier
```

That value is public — it is in the signature of every copy we ship — so it
belongs in the script, unlike `APPLE_TEAM_ID` the secret, which exists so that
notarytool can authenticate as us. Until then, a cautious installer can pass
`--team-id`.

### Changing the installer

`scripts/verify.sh` has an `install script` stage, so this is covered by the
normal gate:

- `scripts/tests/install.test.sh` runs the real script on Linux against a
  stubbed macOS toolchain (`codesign`, `spctl`, `hdiutil`, `ditto`, `curl`,
  ...) and asserts every refusal above, that nothing is copied before
  verification, and that the disk image is detached even when a check fails.
- The four constants the script pins — repo, app name, bundle identifier,
  minimum macOS — are checked against `tauri.conf.json`, and the workflow is
  checked for the upload step, so a rename on either side fails the gate rather
  than the release.

Anything added to the script that macOS-only tooling can reach needs a stub and
a case, or it is untested: CI has no Mac.

Windows packaging on PRs is a separate, opt-in job (`windows-package` label
or `workflow_dispatch` with `package=true`). It builds an **unsigned**
NSIS installer so the port can be exercised without paying for NSIS on every
PR. It does not change `bundle.targets` (`app` / `dmg`), does not upload a
release asset, and does not replace a human smoke on a real PC. Tag-time
Windows NSIS belongs to `release.yml` (#281). What the PR job guarantees
versus what it does not is
[docs/windows-ci.md](windows-ci.md) (#286 / #281).

---

## Verifying notarization succeeded

A green workflow is not proof. Run these against the app from the downloaded
`.dmg` — a locally built copy is not carrying the quarantine flag that
Gatekeeper actually reacts to.

```sh
# Signed by us, with hardened runtime? Look for
#   Authority=Developer ID Application: ...
#   flags=0x10000(runtime)
codesign -dv --verbose=4 /Volumes/JaBot/JaBot.app

# Seals intact, including everything nested in the bundle.
codesign --verify --deep --strict --verbose=2 /Volumes/JaBot/JaBot.app

# The one that matters. Expect: "accepted" + "source=Notarized Developer ID".
# "rejected" or "source=Unnotarized Developer ID" means signing worked and
# notarization did not.
spctl -a -vvv -t install /Volumes/JaBot/JaBot.app

# Ticket stapled into the bundle, so first launch works offline.
xcrun stapler validate /Volumes/JaBot/JaBot.app
```

Confirm the entitlements that shipped are the ones we intended — an empty
dict, no keys at all:

```sh
codesign -d --entitlements - --xml /Volumes/JaBot/JaBot.app | plutil -convert xml1 -o - -
```

If `spctl` rejects it, pull the actual reason from Apple rather than guessing:

```sh
xcrun notarytool history \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_PASSWORD"
xcrun notarytool log <submission-id> \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_PASSWORD"
```

The log names the offending path, which for us will most often be a nested
executable that was added to the bundle without being signed.

Then the real test, once: mount the downloaded `.dmg`, drag to
`/Applications`, launch, and start a thread. The packaged-app acceptance
matrix ([macos-acceptance.md](macos-acceptance.md)) is the rest of that
story: Tauri IPC, a synthetic `fake-acp` turn on temp data, quit/relaunch,
and the notification / signed-update cells that stay manual (D-019). Gatekeeper problems and
hardened-runtime problems look nothing alike — the first blocks the app, the
second lets the app open and then kills the adapter subprocess. Adapters are
`fork`/`exec`ed, so macOS judges each one by its own signature and no
entitlement of ours changes that; if one dies at spawn in a notarized build
that works unsigned, capture the real error first — `log stream --predicate
'sender == "kernel"'` while launching, plus the adapter's teed stderr — and
record it in `entitlements.plist` alongside whatever key turns out to fix it.
Do not add an entitlement against a predicted failure.

### Verifying the feed

After publishing:

```sh
curl -sL https://github.com/jabreeflor/jabot/releases/latest/download/latest.json
```

`version` must match the tag without the `v`, and `platforms` must contain
**both** `darwin-aarch64` and `darwin-x86_64`. tauri-action writes both from
the single universal build, because the updater plugin looks up
`{os}-{arch}` and has no `darwin-universal` fallback — a feed listing only
`darwin-universal` resolves for nobody.

Each entry's `signature` is the base64 `.sig` produced by
`TAURI_SIGNING_PRIVATE_KEY`. If it was signed by a key that does not match the
committed `pubkey`, the build only *warns*; the failure surfaces later as
installs rejecting the update. Rotating the updater key means committing the
new `pubkey` **and** shipping at least one release users can still verify with
the old one.

---

## The DMG container is signed but not notarized

Tauri notarizes and staples the `.app`, then signs the `.dmg` around it. The
`.dmg` itself is not submitted separately. That is fine for Gatekeeper — the
quarantine flag follows the app out of the disk image, and the app carries its
own stapled ticket — and it is why the verification commands above target the
`.app` inside the mounted volume.

Stapling the container too would need `xcrun notarytool submit --wait` and
`xcrun stapler staple` against the `.dmg` *before* it is uploaded, which is
inside `tauri-action`'s step and not reachable from the workflow. If we ever
want it, the honest fix is to build and upload in two steps rather than to
bolt a post-upload staple onto a file that is already published.

---

## Windows

The `.dmg` / `install.sh` path above is unchanged. Windows MVP is **one
NSIS installer** (`JaBot_*_x64-setup.exe`, current-user, unsigned). MSI is
not built. The overlay, sibling `windows` release job, and artifact checks
are [#281](https://github.com/jabreeflor/jabot/issues/281) /
[PR #291](https://github.com/jabreeflor/jabot/pull/291) — that PR extends
this runbook; do not fork a second copy here.

User-facing install, the gap list vs macOS (glass, Dock hide,
signing/SmartScreen, notify), `tauri dev` prerequisites, and the five-cell
smoke checklist live in [`docs/windows.md`](windows.md) (#287) under
tracking epic [#280](https://github.com/jabreeflor/jabot/issues/280).
Sibling Windows PRs should update that page rather than contradict it.

A green macOS release is not a Windows ship. Base `bundle.targets` stays
`["app", "dmg"]`; NSIS is the Windows overlay, not a second entry in that
array (D-005).

## Entitlements

The full audit is in the comments of
[`src-tauri/entitlements.plist`](../src-tauri/entitlements.plist), which
declares nothing: every entitlement JaBot could plausibly want was considered
and left out, each with its reason. The short version:

| Need | Entitlement | Why |
|---|---|---|
| Spawn ACP adapters from PATH | none | An exec'd child is signature-checked on its own; `disable-library-validation` governs what loads *into* JaBot's process, not what it spawns, so it would not help — and it would let any foreign-team dylib into the process holding the keychain secrets |
| Outgoing network to harness APIs | none | Sandbox-only key; a non-sandboxed Developer ID app already has network access |
| Keychain secrets vault ([#9](https://github.com/jabreeflor/jabot/issues/9)) | none | Only needed to *share* keychain items with another of our apps |
| Camera, mic, location, App Sandbox | none | JaBot uses none of them, and App Sandbox would make PATH adapters illegal |
