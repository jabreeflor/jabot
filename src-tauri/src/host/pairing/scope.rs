//! What a role may call. Default-deny, evaluated on the host, every call.
//!
//! `pairing-security-mobile.md` is specific about the phone: "`approver` role:
//! Inbox, permission reply, read transcript, maybe cancel. Not: delete a
//! thread, change crew tools, pair *another* device, dump host logs, set
//! Always-allow globally." The desktop stays the admin console.
//!
//! Two properties matter more than the exact list.
//!
//! **It is an allowlist.** A method that does not appear here is refused for
//! an `approver`, so a host method added next week is closed to phones until
//! somebody decides otherwise. The alternative — a denylist — silently opens
//! every new surface to the least trusted device on the account.
//!
//! **The role is never read off the wire.** `host/hello` may carry whatever a
//! client likes; the role used here comes from the `paired_devices` row, read
//! fresh on each call, so revoking or narrowing a device takes effect on its
//! very next request rather than at its next reconnect.

use super::super::protocol::methods::{
    DeviceRole, HOST_HEALTH, HOST_HELLO, INBOX_LIST, PAIRING_CLAIM, PAIRING_CONFIRM,
    PERMISSION_PENDING, PERMISSION_REPLY, SESSION_CANCEL, SYNC_RESUME_FROM, THREAD_STATE,
    THREAD_TRANSCRIPT,
};

/// Everything a phone may do: see what needs it, answer it, read enough of the
/// thread to know what it is answering, and stop a turn it does not like.
pub const APPROVER_METHODS: &[&str] = &[
    HOST_HELLO,
    HOST_HEALTH,
    INBOX_LIST,
    PERMISSION_PENDING,
    PERMISSION_REPLY,
    THREAD_STATE,
    THREAD_TRANSCRIPT,
    SESSION_CANCEL,
    SYNC_RESUME_FROM,
];

/// The two methods a device that is not paired *yet* has to be able to reach.
///
/// Everything else goes through `require_hello`, and an unpaired device cannot
/// say hello — that is the point of `hello_rejects_unknown_device`. Claiming
/// an offer and confirming a safety number are the handshake that *earns* the
/// hello, so they are answered before one has happened. Both are useless
/// without the out-of-band secret, and both burn attempts on the offer.
pub const UNAUTHENTICATED_METHODS: &[&str] = &[PAIRING_CLAIM, PAIRING_CONFIRM];

pub fn allows(role: DeviceRole, method: &str) -> bool {
    match role {
        // The desktop, and any second Mac the user deliberately trusted.
        DeviceRole::Full => true,
        DeviceRole::Approver => APPROVER_METHODS.contains(&method),
    }
}

pub fn is_unauthenticated(method: &str) -> bool {
    UNAUTHENTICATED_METHODS.contains(&method)
}

#[cfg(test)]
mod tests {
    use super::super::super::protocol::methods::{
        CREW_UPDATE, DEVICE_REVOKE, PAIRING_START, SESSION_PROMPT, THREAD_DELETE, TOOLS_CONNECT,
    };
    use super::*;

    #[test]
    fn an_approver_can_answer_and_read() {
        for method in [
            PERMISSION_REPLY,
            PERMISSION_PENDING,
            INBOX_LIST,
            THREAD_TRANSCRIPT,
            SESSION_CANCEL,
        ] {
            assert!(allows(DeviceRole::Approver, method), "{method}");
        }
    }

    /// The list from the research, one assertion each. A phone that can do any
    /// of these is a phone that is the admin console.
    #[test]
    fn an_approver_cannot_administer_the_host() {
        for method in [
            THREAD_DELETE,
            CREW_UPDATE,
            TOOLS_CONNECT,
            PAIRING_START,
            DEVICE_REVOKE,
            SESSION_PROMPT,
        ] {
            assert!(!allows(DeviceRole::Approver, method), "{method}");
            assert!(allows(DeviceRole::Full, method), "{method}");
        }
    }

    /// The property that matters more than the list: a method nobody has
    /// thought about is closed, not open.
    #[test]
    fn a_method_nobody_scoped_yet_is_denied() {
        assert!(!allows(DeviceRole::Approver, "some/methodAddedLater"));
    }

    #[test]
    fn only_the_handshake_runs_before_a_hello() {
        assert!(is_unauthenticated(PAIRING_CLAIM));
        assert!(is_unauthenticated(PAIRING_CONFIRM));
        assert!(!is_unauthenticated(PAIRING_START));
        assert!(!is_unauthenticated(PERMISSION_REPLY));
    }
}
