//! Delivery through Windows Action Center toasts (#284). Windows only.
//!
//! **It must never panic.** WinRT can refuse an unpackaged `tauri dev`
//! binary, a machine with notifications off, or a host without a registered
//! AppUserModelID. Every entry point here logs and returns. The Inbox card
//! was written before this module was called.
//!
//! **AUMID, then a documented fallback.** Packaged JaBot should toast as
//! [`APP_USER_MODEL_ID`] (`com.jabot.app`, the Tauri identifier). Until the
//! installer registers that id (#281), WinRT often rejects it; we then retry
//! once with PowerShell's well-known id so a toast still appears. That retry
//! *looks* like PowerShell. The log says so. Do not treat the fallback as
//! the shipping identity. Always try the real id first — a PowerShell
//! success does not sticky-lock the rest of the process.
//!
//! **Clicks, while this process is alive.** Each toast's `ToastNotification`
//! and `Activated` handler are retained for the life of the banner (a
//! process-global list). A click then calls
//! [`dispatch_click`](super::dispatch_click), which is the same sink macOS
//! uses: focus the window, tell the renderer which thread to open. Dropping
//! those objects after `Show` — what `tauri-winrt-notification` 0.8.1
//! `show()` does after a 10 ms sleep — is why an earlier path never fired.
//! A click after JaBot has quit does not relaunch the app — that needs a
//! Start Menu shortcut / COM activator the NSIS work will own. The card is
//! already on disk either way.
//!
//! **No permission prompt.** Windows 10+ has no `UNUserNotificationCenter`
//! equivalent we have to call first. Settings can still suppress the banner;
//! we do not query that (cheap MVP), so [`authorization`] reports
//! `NotDetermined` until a toast is accepted and `Granted` after. Windows
//! never reports [`Authorization::Denied`](super::Authorization::Denied): a
//! Settings-off machine looks like a quiet `NotDetermined`, or `Granted`
//! after a `Show` that Action Center then hid. InboxView's "notifications
//! are turned off" line is therefore macOS-only in practice.
//!
//! **Replace-not-stack is macOS-only for now.** We do not set Tag/Group on
//! this path, so two cards on one thread may sit together in Action Center.
//! Same noise budget as macOS; louder shelf.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use windows::core::{h, IInspectable, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

use super::{
    app_id_candidates, Authorization, NativeNotification, APP_USER_MODEL_ID, POWERSHELL_APP_ID,
};

const UNSUPPORTED: u8 = 0;
const NOT_DETERMINED: u8 = 1;
const GRANTED: u8 = 2;

/// How many live banners we keep handlers for. Action Center can still show
/// an older toast; past this cap its in-process click is the one we drop.
const LIVE_CAP: usize = 64;

/// The last delivery outcome we learned. Starts `NOT_DETERMINED` after
/// [`install`]: Windows can toast, we just have not proved it yet.
static AUTHORIZATION: AtomicU8 = AtomicU8::new(UNSUPPORTED);

/// WinRT drops `Activated` when the `ToastNotification` (and its handler) are
/// released. Keep both for the banner's life so a click after `post` returns
/// still reaches [`dispatch_click`](super::dispatch_click).
struct LiveToast {
    _notification: ToastNotification,
    _activated: TypedEventHandler<ToastNotification, IInspectable>,
}

static LIVE_TOASTS: Mutex<Vec<LiveToast>> = Mutex::new(Vec::new());

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
            if app_id == POWERSHELL_APP_ID {
                eprintln!(
                    "Windows toast posted via the PowerShell AppUserModelID; \
                     it will look like PowerShell until the installer \
                     registers {APP_USER_MODEL_ID} (#281)"
                );
            }
            store(GRANTED);
        }
        Err(err) => {
            // Logged and dropped. The card is already in the Inbox (#5).
            eprintln!("could not post a Windows toast: {err}");
        }
    }
}

fn show_toast(note: &NativeNotification) -> Result<String, String> {
    let mut last_err = String::from("no AppUserModelID left to try");

    for app_id in app_id_candidates() {
        match post(app_id, note) {
            Ok(()) => return Ok(app_id.to_string()),
            Err(err) => last_err = format!("{app_id}: {err}"),
        }
    }
    Err(last_err)
}

fn post(app_id: &str, note: &NativeNotification) -> windows::core::Result<()> {
    let xml = toast_xml(note)?;
    let notification = ToastNotification::CreateToastNotification(&xml)?;

    let thread_id = note.thread_id.clone();
    let kind = note.reason.as_str().to_string();
    let activated =
        TypedEventHandler::<ToastNotification, IInspectable>::new(move |_sender, _args| {
            super::dispatch_click(Some(&thread_id), Some(&kind));
            Ok(())
        });
    notification.Activated(&activated)?;

    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?;
    notifier.Show(&notification)?;

    retain(LiveToast {
        _notification: notification,
        _activated: activated,
    });
    Ok(())
}

fn toast_xml(note: &NativeNotification) -> windows::core::Result<XmlDocument> {
    let xml = XmlDocument::new()?;
    let toast = xml.CreateElement(h!("toast"))?;
    let visual = xml.CreateElement(h!("visual"))?;
    let binding = xml.CreateElement(h!("binding"))?;
    binding.SetAttribute(h!("template"), h!("ToastGeneric"))?;

    let title = xml.CreateElement(h!("text"))?;
    title.SetAttribute(h!("id"), h!("1"))?;
    title.SetInnerText(&HSTRING::from(note.title.as_str()))?;
    binding.AppendChild(&title)?;

    let body = xml.CreateElement(h!("text"))?;
    body.SetAttribute(h!("id"), h!("2"))?;
    body.SetInnerText(&HSTRING::from(note.body.as_str()))?;
    binding.AppendChild(&body)?;

    visual.AppendChild(&binding)?;
    toast.AppendChild(&visual)?;
    xml.AppendChild(&toast)?;
    Ok(xml)
}

fn retain(toast: LiveToast) {
    let mut live = LIVE_TOASTS.lock().unwrap_or_else(|e| e.into_inner());
    if live.len() >= LIVE_CAP {
        live.remove(0);
    }
    live.push(toast);
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
    fn denied_is_not_a_windows_authorization_state() {
        install();
        assert_ne!(authorization(), Authorization::Denied);
        deliver(&sample());
        assert_ne!(authorization(), Authorization::Denied);
    }
}
