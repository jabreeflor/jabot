# Native notifications

**Issue:** #27
**Status:** Implemented — `src-tauri/src/notify/mac.rs`, `src-tauri/src/notify/unsupported.rs`, `src-tauri/src/notify/mod.rs`

## What it is

macOS notifications (via `UNUserNotificationCenter`) fired when an Inbox
event is worth interrupting the user for, so a folded thread's result
reaches them even when JaBot isn't the focused app.

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
3. `unsupported.rs` provides a no-op/fallback implementation for
   platforms without native notification support (e.g. Linux CI builds),
   so the rest of the host doesn't need to branch on platform.
4. Not every Inbox event fires a notification — routine "still sleeping"
   states never notify; the policy for which run states/fold policies
   notify is explicit and testable, not implicit in call-site behavior.
5. Clicking a notification brings JaBot to the foreground and navigates
   to the corresponding thread (consistent with requirement 3 of
   [inbox.md](inbox.md)).
6. Notification delivery failures (permission denied, OS API error) are
   logged but never block or roll back the underlying Inbox event/store
   write (see requirement 8 of
   [data-layer-persistence.md](data-layer-persistence.md) — the write
   happens first, regardless of notification outcome).
