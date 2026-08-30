//! Native notifications for the Inbox cards that deserve one (#27).
//!
//! Three rules shape this module, and none of them is negotiable.
//!
//! **Persist, then notify.** #15 and #25 write the `inbox_events` row inside a
//! transaction and *then* queue the host notification this module reads. The
//! banner is the second half of that order, never the first: a notification
//! that is refused, suppressed, or delivered to a machine with no Notification
//! Center loses nothing, because the card is already on disk and the in-app
//! Inbox still draws it. Nothing here reports failure upward, and nothing here
//! runs before the store write.
//!
//! **A budget, not a firehose.** Only `needs_you`, `done` and `failed` reach
//! Notification Center. `stuck` deliberately does not — it means the process is
//! still working and the honest ask is patience, which is a poor reason to
//! interrupt someone; the Inbox card says it just as well. `folded` is not an
//! event at all (D-006), and a cancel the user asked for never produces one
//! (`lifecycle::resurface`).
//!
//! **macOS only, and a genuine no-op everywhere else.** Delivery is
//! [`mac`], compiled only on macOS exactly like the hide-to-Dock branch and the
//! updater plugin in `lib.rs`. Every other platform links [`unsupported`],
//! which really does nothing — CI's verify job runs on Linux, so "compiles and
//! tests there" is a hard requirement, not a courtesy.
//!
//! The decision layer — which events notify, what the payload says, where a
//! click goes — is portable and unit-tested on every platform. What cannot be
//! tested off a Mac is delivery itself; see https://github.com/jabreeflor/jabot/issues/73.

use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;

use crate::host::{INBOX_EVENT, INBOX_RESURFACE};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as backend;

#[cfg(not(target_os = "macos"))]
mod unsupported;
#[cfg(not(target_os = "macos"))]
use unsupported as backend;

/// `userInfo` keys on the delivered notification. Namespaced because the
/// dictionary is shared with whatever else ever attaches to a JaBot alert, and
/// because a click that cannot find *our* key must route nowhere rather than
/// guess a thread id out of a stranger's payload.
pub const USER_INFO_THREAD_ID: &str = "jabot.threadId";
pub const USER_INFO_KIND: &str = "jabot.kind";

/// The Tauri event a click turns into. Mirrored in `src/host/protocol.ts`
/// beside `HOST_RPC_EVENT`, because it reaches the renderer the same way.
pub const ACTIVATED_EVENT: &str = "notification-activated";

/// Notification Center groups by this; one JaBot thread is one conversation.
pub const CATEGORY_IDENTIFIER: &str = "jabot.inbox";

/// The Inbox kinds worth interrupting someone for.
///
/// This is the whole noise budget. Adding a variant is a product decision, not
/// a refactor — the point of the enum is that `stuck`, `folded` and every kind
/// a later issue invents fall through to "no banner" by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyReason {
    NeedsYou,
    Done,
    Failed,
}

impl NotifyReason {
    pub const ALL: &'static [NotifyReason] = &[Self::NeedsYou, Self::Done, Self::Failed];

    /// Map an `inbox_events.kind` (or an `inbox/resurface` reason — they share
    /// the vocabulary) onto a banner. `None` means "record it, do not ring".
    pub fn from_inbox_kind(kind: &str) -> Option<Self> {
        match kind {
            "needs_you" => Some(Self::NeedsYou),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs_you",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// Copy for a card whose title the notification did not carry. The boot
    /// restate path in `supervisor::boot` re-announces an existing row without
    /// re-reading its title, so this is a real case rather than a defensive one.
    fn fallback_title(self) -> &'static str {
        match self {
            Self::NeedsYou => "A thread needs you",
            Self::Done => "A thread finished",
            Self::Failed => "A thread failed",
        }
    }

    /// Copy for a card with no summary. Says what the card means, never what
    /// the transcript said — the transcript is what opening the thread is for.
    fn fallback_body(self) -> &'static str {
        match self {
            Self::NeedsYou => "Waiting on your answer.",
            Self::Done => "Finished while you were away.",
            Self::Failed => "Stopped without finishing.",
        }
    }
}

/// Every kind that produces a banner, as the wire spells them. `notify/status`
/// hands this to the UI so "why did nothing ping me?" has an answer that does
/// not require reading this file.
pub fn notifying_kinds() -> Vec<String> {
    NotifyReason::ALL
        .iter()
        .map(|reason| reason.as_str().to_string())
        .collect()
}

/// One banner, fully decided, before any platform is involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeNotification {
    /// Stable per thread on purpose. UNUserNotificationCenter replaces a
    /// delivered notification that carries an identifier it already has, so a
    /// thread that finishes after asking for permission updates its one banner
    /// instead of stacking two. The durable record is the Inbox; this is a
    /// nudge, and five nudges about one thread is worse than one current one.
    pub identifier: String,
    pub thread_id: String,
    pub reason: NotifyReason,
    pub title: String,
    pub body: String,
}

impl NativeNotification {
    /// What travels in `userInfo` and comes back on a click. Only the thread
    /// id is load-bearing; the kind rides along so the renderer can tell an
    /// opened-from-`needs_you` from an opened-from-`done` without a lookup.
    pub fn user_info(&self) -> [(&'static str, String); 2] {
        [
            (USER_INFO_THREAD_ID, self.thread_id.clone()),
            (USER_INFO_KIND, self.reason.as_str().to_string()),
        ]
    }
}

/// Decide whether a host → client notification also deserves an OS banner.
///
/// Takes the wire frame rather than a typed struct because that is what the
/// Tauri layer has in hand, and because a params shape that grows a field must
/// not be able to break the banner. Anything unrecognised returns `None`.
pub fn plan(method: &str, params: &Value) -> Option<NativeNotification> {
    let thread_id = non_empty(params.get("threadId").and_then(Value::as_str))?;

    let (reason, title, body) = match method {
        // #25: a card on a thread that did not move. Carries its own copy.
        INBOX_EVENT => {
            let reason = NotifyReason::from_inbox_kind(params.get("kind")?.as_str()?)?;
            (
                reason,
                non_empty(params.get("title").and_then(Value::as_str)),
                non_empty(params.get("summary").and_then(Value::as_str)),
            )
        }
        // #15: a folded thread came back. `title` / `summary` are optional on
        // the wire, so a client (or a host path) that omits them still notifies
        // — with reason-shaped copy rather than nothing at all.
        INBOX_RESURFACE => {
            let reason = NotifyReason::from_inbox_kind(params.get("reason")?.as_str()?)?;
            (
                reason,
                non_empty(params.get("title").and_then(Value::as_str)),
                non_empty(params.get("summary").and_then(Value::as_str)),
            )
        }
        _ => return None,
    };

    Some(NativeNotification {
        identifier: identifier_for(thread_id),
        thread_id: thread_id.to_string(),
        reason,
        title: title.unwrap_or(reason.fallback_title()).to_string(),
        body: body.unwrap_or(reason.fallback_body()).to_string(),
    })
}

fn identifier_for(thread_id: &str) -> String {
    format!("{CATEGORY_IDENTIFIER}.{thread_id}")
}

fn non_empty(raw: Option<&str>) -> Option<&str> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// What the renderer is told when a banner is clicked.
///
/// Serialized straight onto the Tauri event bus, so the field names are the
/// ones `src/host/protocol.ts` mirrors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationOpen {
    pub thread_id: String,
    pub kind: String,
}

impl NotificationOpen {
    /// Rebuild the click target from a delivered notification's `userInfo`.
    ///
    /// Returns `None` when the thread id is missing or blank: a banner we
    /// cannot attribute to a thread must open nothing rather than guess. The
    /// kind is decoration and defaults rather than refusing.
    pub fn from_user_info(thread_id: Option<&str>, kind: Option<&str>) -> Option<Self> {
        Some(Self {
            thread_id: non_empty(thread_id)?.to_string(),
            kind: non_empty(kind).unwrap_or("done").to_string(),
        })
    }
}

type ClickSink = Box<dyn Fn(&NotificationOpen) + Send + 'static>;

/// Process-global because its other end is an Objective-C delegate object that
/// the system owns and calls on its own schedule; there is no `self` to hang it
/// off. Installed once, from `lib.rs`'s `setup`.
static CLICK_SINK: Mutex<Option<ClickSink>> = Mutex::new(None);

/// Route clicks to `sink` — in practice, "focus the window and tell the
/// renderer which thread to open".
pub fn on_click(sink: impl Fn(&NotificationOpen) + Send + 'static) {
    let mut slot = CLICK_SINK.lock().unwrap_or_else(|e| e.into_inner());
    *slot = Some(Box::new(sink));
}

/// A banner was clicked. Called by the macOS delegate with the two `userInfo`
/// strings it found; returns what it routed so the decision is testable without
/// a delegate, a display, or a Mac.
pub fn dispatch_click(thread_id: Option<&str>, kind: Option<&str>) -> Option<NotificationOpen> {
    let open = NotificationOpen::from_user_info(thread_id, kind)?;
    let slot = CLICK_SINK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(sink) = slot.as_ref() {
        sink(&open);
    }
    Some(open)
}

/// Ask the OS for permission and start listening for clicks. Safe to call on
/// any platform; on a machine or a build without Notification Center it marks
/// the feature unsupported and returns.
pub fn install() {
    backend::install();
}

/// The whole outbound path: decide, then hand off. Deliberately returns
/// nothing — a caller that could react to a delivery failure would be tempted
/// to undo the store write that already happened.
pub fn announce(method: &str, params: &Value) {
    if let Some(note) = plan(method, params) {
        backend::deliver(&note);
    }
}

/// Where the OS permission stands. `Unsupported` is not a failure: it is a
/// Linux CI box, a dev build running outside an app bundle, or any build that
/// was not compiled with the macOS backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
    Unsupported,
    NotDetermined,
    Granted,
    Denied,
}

impl Authorization {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::NotDetermined => "notDetermined",
            Self::Granted => "granted",
            Self::Denied => "denied",
        }
    }
}

/// Whether this build can deliver a banner at all.
pub fn supported() -> bool {
    backend::supported()
}

/// The last authorization answer the OS gave. Never blocks: the real query is
/// asynchronous, so this reports what `install` and the last delivery learned.
pub fn authorization() -> Authorization {
    backend::authorization()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resurface(reason: &str) -> Value {
        json!({
            "hostId": "host-1",
            "threadId": "thread-7",
            "seq": 3,
            "reason": reason,
            "title": "Auth migration finished",
            "summary": "12 files changed",
        })
    }

    /// The noise budget, stated as a test so widening it is a deliberate edit.
    #[test]
    fn only_needs_you_done_and_failed_ring() {
        for reason in ["needs_you", "done", "failed"] {
            assert!(
                plan(INBOX_RESURFACE, &resurface(reason)).is_some(),
                "{reason} should notify"
            );
        }
        // Stuck means the process is *still working*; the card says so and a
        // banner would be an interruption asking for patience.
        assert_eq!(plan(INBOX_RESURFACE, &resurface("stuck")), None);
        // Nothing writes a `folded` event (D-006), but if something ever does
        // it must not ring: folding is the user asking not to be told.
        assert_eq!(plan(INBOX_RESURFACE, &resurface("folded")), None);
    }

    /// Every other host → client notification is traffic, not news.
    #[test]
    fn ordinary_stream_traffic_never_rings() {
        let update = json!({
            "hostId": "host-1",
            "threadId": "thread-7",
            "seq": 4,
            "acp": { "sessionUpdate": "agent_message_chunk" },
        });
        assert_eq!(plan("session/update", &update), None);
        assert_eq!(plan("permission/ask", &update), None);
        assert_eq!(plan("permission/resolved", &update), None);
    }

    #[test]
    fn a_schedule_card_carries_its_own_copy() {
        let event = json!({
            "hostId": "host-1",
            "threadId": "thread-9",
            "seq": 11,
            "kind": "done",
            "title": "Morning triage finished",
            "summary": "3 emails filed",
        });

        let note = plan(INBOX_EVENT, &event).expect("a done card notifies");
        assert_eq!(note.thread_id, "thread-9");
        assert_eq!(note.reason, NotifyReason::Done);
        assert_eq!(note.title, "Morning triage finished");
        assert_eq!(note.body, "3 emails filed");
    }

    /// `supervisor::boot` re-announces an existing card without its title. A
    /// banner with an empty title is worse than one that says what happened.
    #[test]
    fn a_resurface_without_copy_still_says_something() {
        let bare = json!({
            "hostId": "host-1",
            "threadId": "thread-7",
            "seq": 3,
            "reason": "needs_you",
        });

        let note = plan(INBOX_RESURFACE, &bare).expect("needs_you notifies");
        assert_eq!(note.title, "A thread needs you");
        assert_eq!(note.body, "Waiting on your answer.");
    }

    /// A banner nobody can attribute to a thread would open nothing on click,
    /// so it is never sent in the first place.
    #[test]
    fn a_card_without_a_thread_is_not_a_banner() {
        for params in [
            json!({ "hostId": "h", "seq": 1, "kind": "done", "title": "t" }),
            json!({ "hostId": "h", "threadId": "", "seq": 1, "kind": "done" }),
            json!({ "hostId": "h", "threadId": "   ", "seq": 1, "kind": "done" }),
        ] {
            assert_eq!(plan(INBOX_EVENT, &params), None, "{params}");
        }
    }

    /// Two cards on one thread share an identifier so the second *replaces*
    /// the first; two threads never collide.
    #[test]
    fn one_thread_owns_one_banner() {
        let needs = plan(INBOX_RESURFACE, &resurface("needs_you")).unwrap();
        let done = plan(INBOX_RESURFACE, &resurface("done")).unwrap();
        assert_eq!(needs.identifier, done.identifier);

        let other = plan(
            INBOX_EVENT,
            &json!({ "threadId": "thread-8", "seq": 1, "kind": "done" }),
        )
        .unwrap();
        assert_ne!(needs.identifier, other.identifier);
    }

    /// The click path, end to end minus the OS: what we attach on the way out
    /// is what routes a click on the way back in.
    #[test]
    fn a_click_routes_to_the_thread_the_banner_named() {
        let note = plan(INBOX_RESURFACE, &resurface("needs_you")).unwrap();
        let info = note.user_info();
        let thread = info
            .iter()
            .find(|(key, _)| *key == USER_INFO_THREAD_ID)
            .map(|(_, value)| value.as_str());
        let kind = info
            .iter()
            .find(|(key, _)| *key == USER_INFO_KIND)
            .map(|(_, value)| value.as_str());

        let open = NotificationOpen::from_user_info(thread, kind).expect("routable");
        assert_eq!(open.thread_id, "thread-7");
        assert_eq!(open.kind, "needs_you");
    }

    /// A stranger's notification, or one whose payload we cannot read, opens
    /// nothing. Guessing a thread would open somebody's transcript at random.
    #[test]
    fn an_unattributable_click_opens_nothing() {
        assert_eq!(NotificationOpen::from_user_info(None, Some("done")), None);
        assert_eq!(NotificationOpen::from_user_info(Some(" "), None), None);
    }

    /// The sink is a process global; this is the one test that installs one,
    /// and it asserts on its own thread id rather than on the recorded count.
    #[test]
    fn an_installed_sink_receives_the_click() {
        let seen: std::sync::Arc<Mutex<Vec<String>>> = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        on_click(move |open| {
            sink.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(open.thread_id.clone())
        });

        dispatch_click(Some("thread-sink"), Some("failed")).expect("routable");

        let recorded = seen.lock().unwrap_or_else(|e| e.into_inner());
        assert!(recorded.contains(&"thread-sink".to_string()));
    }

    /// Off macOS the whole thing is inert, and `announce` must still be safe to
    /// call — the host emits the same notifications on every platform.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_non_macos_build_is_a_real_no_op() {
        assert!(!supported());
        assert_eq!(authorization(), Authorization::Unsupported);
        install();
        announce(INBOX_RESURFACE, &resurface("done"));
    }

    #[test]
    fn the_budget_is_reportable() {
        assert_eq!(notifying_kinds(), vec!["needs_you", "done", "failed"]);
    }
}
