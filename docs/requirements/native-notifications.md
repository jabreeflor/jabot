# Native notifications

**Issue:** #27, #284
**Status:** Implemented — `src-tauri/src/notify/mac.rs` (macOS), `src-tauri/src/notify/win.rs` (Windows), `src-tauri/src/notify/unsupported.rs` (Linux and other hosts), `src-tauri/src/notify/mod.rs`

## What it is

OS banners fired when an Inbox event is worth interrupting the user for, so a
folded thread's result reaches them even when JaBot isn't the focused app.

- **macOS:** `UNUserNotificationCenter` (#27).
- **Windows:** Action Center toasts via WinRT (#284).
- **Anything else:** a compile-time no-op. Linux CI is this path on purpose.

## Why

Folding a thread only pays off if "needs you" actually reaches the user
somewhere they'll see it — the Inbox view alone requires JaBot to be open
and visible.

## Requirements

1. Notifications are driven by the same Inbox event feed as the in-app
   Inbox (see [inbox.md](inbox.md)) — no separate notification-worthy
   condition is computed independently.
2. `mac.rs` implements native delivery via `UNUserNotificationCenter` on
   macOS, including requesting notification permission from the OS
   before first use.
3. `win.rs` implements native delivery via WinRT toasts on Windows. Title
   and body come from the same `notify::plan` payload macOS uses. A WinRT
   or AUMID failure is logged and dropped — the host never panics.
4. `unsupported.rs` provides a no-op/fallback implementation for
   platforms without native notification support (Linux CI builds, and
   any OS that is neither macOS nor Windows), so the rest of the host
   doesn't need to branch on platform. Windows is not this file.
5. Not every Inbox event fires a notification — routine "still sleeping"
   states never notify; the policy for which run states/fold policies
   notify is explicit and testable, not implicit in call-site behavior.
6. Clicking a notification brings JaBot to the foreground and navigates
   to the corresponding thread (consistent with requirement 3 of
   [inbox.md](inbox.md)), **while the process is running**. See
   [Click focus](#click-focus) for the Windows caveat.
7. Notification delivery failures (permission denied, OS API error, missing
   AppUserModelID) are logged but never block or roll back the underlying
   Inbox event/store write (see requirement 8 of
   [data-layer-persistence.md](data-layer-persistence.md) — the write
   happens first, regardless of notification outcome).

`mac.rs` is `cfg(macos)` and is not seen by the default Linux Clippy. The
before-merge lint is the existing scratch-crate cross-check
(`scripts/check-mac-notify.sh`), run automatically on relevant PRs; see
[macos-lint.md](../macos-lint.md). Delivery on a signed Mac is still the
runtime checklist on decision [#73](https://github.com/jabreeflor/jabot/issues/73).

`win.rs` is `cfg(windows)` and is likewise invisible on Linux CI. The
decision layer in `mod.rs` still runs there. Delivery on a Windows
desktop is the smoke list below, not a Linux compile.

## Click focus

The portable click sink (`notify::on_click` / `dispatch_click`) is
shared. `lib.rs` focuses the `main` window and emits
`notification-activated` with `{ threadId, kind }`.

| Platform | Click while JaBot is running | Click after JaBot has quit |
| --- | --- | --- |
| macOS | Un-hides from the Dock and opens that thread | Relaunches via the app bundle, then the same route if the payload is still there |
| Windows | Focuses the window and opens that thread. `win.rs` keeps each `ToastNotification` and its `Activated` handler alive after `Show` so the click still reaches `dispatch_click`. | Does **not** relaunch. A Start Menu shortcut / COM activator is installer work (#281). The Inbox card is already on disk. |
| Linux / other | No banner, so no click | — |

Unpackaged `tauri dev` on Windows may toast under PowerShell's
AppUserModelID (the log says so) until NSIS/MSI registers `com.jabot.app`.
A PowerShell success does not skip `com.jabot.app` on later toasts.
Same-thread replacement (one banner, not a stack) is macOS-only for now;
Windows may show two toasts for two cards on one thread.

Windows never reports `denied`. There is no permission prompt; Settings
can still suppress the banner. `authorization` stays `notDetermined`
until a `Show` is accepted, then `granted`. InboxView's "notifications
are turned off" line is macOS-only in practice (it keys on `denied`).

## Windows notify smoke (acceptance)

Run on a logged-in Windows 10/11 session — a headless runner cannot claim
this, for the same honesty reason as D-019 on macOS.

1. Launch a Windows JaBot build. Fold a thread (or wait for a `needs_you` /
   `done` / `failed` Inbox card).
2. **Toast appears** with that card's title and summary (or the reason
   fallback copy). Evidence: screenshot of the Action Center toast.
3. **Click while JaBot is still running** focuses the window and opens
   that thread. Evidence: screenshot of the opened thread.
4. **Failure is soft.** Turning notifications off in Windows Settings, or
   running an unpackaged binary whose AUMID WinRT rejects, must not crash
   the host. Inbox still lists the card. Evidence: the process is still
   up; stderr has `could not post a Windows toast` if delivery failed.
   Windows does **not** flip `notify/status` to `denied` when Settings
   hide banners — that copy and the Inbox line are the macOS permission
   path. A quiet `notDetermined` (or `granted` after a hidden `Show`)
   with a live process is the Windows success criterion.
5. macOS behavior is unchanged — do not treat a Windows smoke run as a
   substitute for the #73 Mac checklist.
