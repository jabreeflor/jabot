//! The non-desktop-notify backend: a real no-op, not a stub that pretends.
//!
//! Linked on Linux and any OS that is neither macOS nor Windows. CI's verify
//! job runs on Linux and `jabot-hostd` builds there, so this file is what
//! makes "no Notification Center here" a compile-time fact instead of a
//! runtime branch. Windows is **not** this file — see `win.rs` (#284).
//!
//! It reports `Unsupported` rather than `Denied` because the two mean
//! different things to the UI: denied is a permission the user can change,
//! unsupported is a platform that has nowhere to put a banner.

use super::{Authorization, NativeNotification};

pub fn supported() -> bool {
    false
}

pub fn authorization() -> Authorization {
    Authorization::Unsupported
}

pub fn install() {}

/// Deliberately reads its argument and drops it. The Inbox card is already on
/// disk (persist-then-notify), so there is nothing here to fail at.
pub fn deliver(_note: &NativeNotification) {}
