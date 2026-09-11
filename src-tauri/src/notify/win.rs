//! Delivery through Windows Action Center toasts (#284). Windows only.
//!
//! **It must never panic.** `Toast::show` talks to WinRT, which can refuse an
//! unpackaged `tauri dev` binary, a machine with notifications off, or a host
//! without a registered AppUserModelID. Every entry point here logs and
//! returns. The Inbox card was written before this module was called.
//!
//! **AUMID, then a documented fallback.** Packaged JaBot should toast as
//! [`APP_USER_MODEL_ID`] (`com.jabot.app`, the Tauri identifier). Until the
//! installer registers that id (#281), WinRT often rejects it; we then retry
//! once with PowerShell's well-known id so a toast still appears. That retry
//! *looks* like PowerShell. The log says so. Do not treat the fallback as
//! the shipping identity.
//!
//! **Clicks, while this process is alive.** `on_activated` calls
//! [`dispatch_click`](super::dispatch_click), which is the same sink macOS
//! uses: focus the window, tell the renderer which thread to open. A click
//! after JaBot has quit does not relaunch the app — that needs a Start Menu
//! shortcut / COM activator the NSIS work will own. The card is already on
//! disk either way.
//!
//! **No permission prompt.** Windows 10+ has no `UNUserNotificationCenter`
//! equivalent we have to call first. Settings can still suppress the banner;
//! we do not query that (cheap MVP), so [`authorization`] reports
//! `NotDetermined` until a toast is accepted and `Granted` after.
//!
//! **Replace-not-stack is macOS-only for now.** The toast builder we use has
//! no Tag/Group on the happy path, so two cards on one thread may sit
//! together in Action Center. Same noise budget as macOS; louder shelf.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use tauri_winrt_notification::{Sound, Toast};

use super::{Authorization, NativeNotification};

const UNSUPPORTED: u8 = 0;
const NOT_DETERMINED: u8 = 1;
const GRANTED: u8 = 2;

/// Must match `identifier` in `src-tauri/tauri.conf.json`. The installer
/// (#281) is what makes WinRT treat this as a real app rather than a guest.
pub const APP_USER_MODEL_ID: &str = "com.jabot.app";

/// The last delivery outcome we learned. Starts `NOT_DETERMINED` after
/// [`install`]: Windows can toast, we just have not proved it yet.
static AUTHORIZATION: AtomicU8 = AtomicU8::new(UNSUPPORTED);

/// Which AUMID last succeeded. `None` means "try the real id first".
static WORKING_APP_ID: Mutex<Option<String>> = Mutex::new(None);

pub fn supported() -> bool {
    true
}

pub fn authorization() -> Authorization {
    match AUTHORIZATION.load(Ordering::Relaxed) {
        GRANTED => Authorization::Granted,
        NOT_DETERMINED => Authorization::NotDetermined,
        _ => Authorization::Unsupported,
    }
}

fn store(status: u8) {
    AUTHORIZATION.store(status, Ordering::Relaxed);
}

pub fn install() {
    store(NOT_DETERMINED);
}

pub fn deliver(note: &NativeNotification) {
    match show_toast(note) {
        Ok(app_id) => {
            remember_app_id(app_id);
            store(GRANTED);
        }
        Err(err) => {
            // Logged and dropped. The card is already in the Inbox (#5).
            eprintln!("could not post a Windows toast: {err}");
        }
    }
}

fn show_toast(note: &NativeNotification) -> Result<String, String> {
    let remembered = working_app_id();
    let candidates = app_id_candidates(remembered.as_deref());
    let mut last_err = String::from("no AppUserModelID left to try");

    for app_id in candidates {
        match post(app_id, note) {
            Ok(()) => {
                if app_id == Toast::POWERSHELL_APP_ID {
                    eprintln!(
                        "Windows toast posted via the PowerShell AppUserModelID; \
                         it will look like PowerShell until the installer \
                         registers {APP_USER_MODEL_ID} (#281)"
                    );
                }
                return Ok(app_id.to_string());
            }
            Err(err) => last_err = format!("{app_id}: {err}"),
        }
    }
    Err(last_err)
}

/// Real id first, PowerShell last, and skip a remembered failure path.
fn app_id_candidates(remembered: Option<&str>) -> Vec<&'static str> {
    match remembered {
        Some(id) if id == APP_USER_MODEL_ID => {
            vec![APP_USER_MODEL_ID, Toast::POWERSHELL_APP_ID]
        }
        Some(id) if id == Toast::POWERSHELL_APP_ID => {
            vec![Toast::POWERSHELL_APP_ID]
        }
        _ => vec![APP_USER_MODEL_ID, Toast::POWERSHELL_APP_ID],
    }
}

fn post(app_id: &str, note: &NativeNotification) -> tauri_winrt_notification::Result<()> {
    let thread_id = note.thread_id.clone();
    let kind = note.reason.as_str().to_string();
    Toast::new(app_id)
        .title(&note.title)
        .text1(&note.body)
        .sound(Some(Sound::Default))
        .on_activated(move |_action| {
            super::dispatch_click(Some(&thread_id), Some(&kind));
            Ok(())
        })
        .show()
}

fn working_app_id() -> Option<String> {
    WORKING_APP_ID
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn remember_app_id(app_id: String) {
    *WORKING_APP_ID.lock().unwrap_or_else(|e| e.into_inner()) = Some(app_id);
}

#[cfg(test)]
mod tests {
    use super::super::NotifyReason;
    use super::*;

    fn sample() -> NativeNotification {
        NativeNotification {
            identifier: "jabot.inbox.thread-win".into(),
            thread_id: "thread-win".into(),
            reason: NotifyReason::Done,
            title: "A thread finished".into(),
            body: "Finished while you were away.".into(),
        }
    }

    #[test]
    fn windows_is_a_supported_toast_host() {
        assert!(supported());
        install();
        assert_eq!(authorization(), Authorization::NotDetermined);
    }

    /// Headless CI (and this unit test) may have no Action Center. The
    /// contract is "do not abort", not "the banner appeared".
    #[test]
    fn deliver_is_safe_when_winrt_refuses() {
        install();
        deliver(&sample());
    }

    #[test]
    fn aumid_order_prefers_jabot_then_powershell() {
        assert_eq!(
            app_id_candidates(None),
            vec![APP_USER_MODEL_ID, Toast::POWERSHELL_APP_ID]
        );
        assert_eq!(
            app_id_candidates(Some(Toast::POWERSHELL_APP_ID)),
            vec![Toast::POWERSHELL_APP_ID]
        );
    }
}
