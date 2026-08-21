//! Delivery through `UNUserNotificationCenter` (#27). macOS only.
//!
//! Two things about this framework decide the shape of the file.
//!
//! **It needs a bundle.** `UNUserNotificationCenter` reads the running app's
//! bundle identifier to decide who is asking, and raises an Objective-C
//! exception — which is an abort, not an error — when there is not one. A
//! `cargo tauri dev` build runs the bare binary, not `JaBot.app`, so every
//! entry point here is guarded by [`bundled`] and degrades to silence.
//! Silence is the correct outcome: the Inbox card was written before this
//! module was ever called.
//!
//! **It is asynchronous.** Authorization and settings arrive in completion
//! blocks, so [`authorization`] reports the last answer the OS gave rather than
//! blocking a JSON-RPC handler on a system call. "Not asked yet" is a real,
//! reportable state, not a placeholder.
//!
//! There is deliberately no `willPresentNotification:` delegate method. Without
//! one macOS suppresses banners while JaBot is frontmost, which is exactly
//! right — someone looking at the app has the Inbox in front of them, and #27
//! is about the times they are somewhere else.

use std::sync::atomic::{AtomicU8, Ordering};

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSBundle, NSDictionary, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSettings, UNNotificationSound,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::{Authorization, NativeNotification};

const UNSUPPORTED: u8 = 0;
const NOT_DETERMINED: u8 = 1;
const GRANTED: u8 = 2;
const DENIED: u8 = 3;

/// The last thing the OS said. Starts `UNSUPPORTED` so a build that never gets
/// as far as `install` — or one running outside an app bundle — reports the
/// truth rather than an optimistic "not asked yet".
static AUTHORIZATION: AtomicU8 = AtomicU8::new(UNSUPPORTED);

pub fn supported() -> bool {
    bundled()
}

pub fn authorization() -> Authorization {
    match AUTHORIZATION.load(Ordering::Relaxed) {
        GRANTED => Authorization::Granted,
        DENIED => Authorization::Denied,
        NOT_DETERMINED => Authorization::NotDetermined,
        _ => Authorization::Unsupported,
    }
}

fn store(status: u8) {
    AUTHORIZATION.store(status, Ordering::Relaxed);
}

/// Whether this process is a real app bundle. See the module docs: without a
/// bundle identifier, touching the notification center aborts the process.
fn bundled() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

pub fn install() {
    if !bundled() {
        return;
    }
    store(NOT_DETERMINED);
    let center = UNUserNotificationCenter::currentNotificationCenter();

    // `setDelegate:` is a *weak* property and the delegate has to outlive the
    // app, so the single retain is leaked on purpose. One object, once, at
    // startup — cheaper and far clearer than a static that has to prove itself
    // `Sync` to hold an Objective-C pointer.
    let delegate = ClickDelegate::new();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    std::mem::forget(delegate);

    // Alert and sound only. No badge: the Inbox count is the app's own badge
    // (#22) and a second, divergent number on the Dock icon would be worse
    // than none.
    let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
    let answered = RcBlock::new(|granted: Bool, error: *mut NSError| {
        if let Some(error) = unsafe { error.as_ref() } {
            // A refused permission is not an error path; this is the OS
            // failing to answer at all. Either way the Inbox still works.
            eprintln!(
                "notification authorization failed: {}",
                error.localizedDescription()
            );
        }
        store(if granted.as_bool() { GRANTED } else { DENIED });
    });
    center.requestAuthorizationWithOptions_completionHandler(options, &answered);

    // The user may have changed their mind in System Settings since the grant;
    // the settings query is the only thing that knows.
    refresh(&center);
}

fn refresh(center: &UNUserNotificationCenter) {
    let settled = RcBlock::new(|settings: core::ptr::NonNull<UNNotificationSettings>| {
        // SAFETY: the framework hands the block a live, non-null settings
        // object and keeps it alive for the duration of the call.
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        store(match status {
            UNAuthorizationStatus::Denied => DENIED,
            UNAuthorizationStatus::NotDetermined => NOT_DETERMINED,
            // Authorized, Provisional and Ephemeral all mean "a banner can
            // reach the user"; the differences are about how loudly.
            _ => GRANTED,
        });
    });
    center.getNotificationSettingsWithCompletionHandler(&settled);
}

pub fn deliver(note: &NativeNotification) {
    if !bundled() {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();

    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&note.title));
    content.setBody(&NSString::from_str(&note.body));
    // Groups every banner about one thread into one conversation in
    // Notification Center, the way a messenger groups by chat.
    content.setThreadIdentifier(&NSString::from_str(&note.thread_id));
    content.setCategoryIdentifier(&NSString::from_str(super::CATEGORY_IDENTIFIER));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    attach_user_info(&content, note);

    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&note.identifier),
        &content,
        // No trigger: deliver now. The work already happened.
        None,
    );
    let delivered = RcBlock::new(|error: *mut NSError| {
        if let Some(error) = unsafe { error.as_ref() } {
            // Logged and dropped. The card is already in the Inbox, which is
            // the whole reason persist-then-notify is the rule (#5).
            eprintln!(
                "could not post a notification: {}",
                error.localizedDescription()
            );
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&delivered));
}

/// Attach the thread id a click routes on. Split out because the generic
/// erasure in the middle needs a reason next to it.
fn attach_user_info(content: &UNMutableNotificationContent, note: &NativeNotification) {
    let pairs = note.user_info();
    let keys: Vec<Retained<NSString>> = pairs
        .iter()
        .map(|(key, _)| NSString::from_str(key))
        .collect();
    let values: Vec<Retained<NSString>> = pairs
        .iter()
        .map(|(_, value)| NSString::from_str(value))
        .collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
    let value_refs: Vec<&NSString> = values.iter().map(|value| &**value).collect();

    let typed = NSDictionary::<NSString, NSString>::from_slices(&key_refs, &value_refs);
    // SAFETY: this erases the Rust-side generic parameters only. The object is
    // the same dictionary of `NSString`s either way, and `setUserInfo:` copies
    // it, so nothing outlives the cast.
    let erased: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(typed) };
    // SAFETY: the values really are the `NSString`s the key type promises.
    unsafe { content.setUserInfo(&erased) };
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - `ClickDelegate` holds no ivars and does not implement `Drop`.
    #[unsafe(super(NSObject))]
    // Named so a crash report says which delegate, rather than a mangled
    // module path with a version number in it.
    #[name = "JaBotNotificationDelegate"]
    struct ClickDelegate;

    unsafe impl NSObjectProtocol for ClickDelegate {}

    // SAFETY: every method of this protocol is optional, and the one
    // implemented below matches the framework's signature.
    unsafe impl UNUserNotificationCenterDelegate for ClickDelegate {
        /// A banner was clicked. Open the thread it names — and nothing else:
        /// the notification is a shortcut into the Inbox, not an action on the
        /// thread.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            let content = response.notification().request().content();
            let info = content.userInfo();
            let thread_id = string_for_key(&info, super::USER_INFO_THREAD_ID);
            let kind = string_for_key(&info, super::USER_INFO_KIND);
            super::dispatch_click(thread_id.as_deref(), kind.as_deref());
            // Must be called, and must be called even when the payload was
            // unreadable: an unanswered completion block leaves the system
            // waiting on us.
            completion.call(());
        }
    }
);

impl ClickDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// One `userInfo` string, or `None` if the key is absent or holds something
/// that is not a string. A malformed payload routes nowhere; it never guesses.
fn string_for_key(info: &NSDictionary<AnyObject, AnyObject>, key: &str) -> Option<String> {
    let key = NSString::from_str(key);
    let value = info.objectForKey(&key)?;
    let value: Retained<NSString> = value.downcast().ok()?;
    Some(value.to_string())
}
