## Summary

<!-- What changed and why. -->

## Screenshot evidence

<!-- Required for anything visual (see CLAUDE.md). Commit under docs/img/<feature>/ and embed here,
     or state why it was skipped, e.g. "no visual surface — backend-only change". -->

## Artifact

<!-- Required on every PR: run /create-pr-artifact (from vendored plugins/jabstack)
     and let it fill this section with the explainer link and screenshots.
     Do not delete this heading. -->

## Verification

- [ ] `./scripts/verify.sh` passed on the pushed tree
- [ ] Native-sensitive change (`notify/`, Keychain, Tauri window/IPC, bundled adapters, updater): `./scripts/verify.sh --check-mac` and/or label `macos-acceptance` for a packaged run — [docs/macos-acceptance.md](docs/macos-acceptance.md). Playwright WebKit is not Tauri acceptance.
