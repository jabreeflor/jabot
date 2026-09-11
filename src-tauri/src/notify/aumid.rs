//! Windows AppUserModelID order. Portable so Linux verify can test it.
//!
//! Packaged JaBot should toast as [`APP_USER_MODEL_ID`].
//! `CreateToastNotifierWithId` can return Ok for an unregistered id and
//! still show nothing, so the input is "is there a Start Menu shortcut?"
//! — not "did Show return Ok?". Last-success is not sticky: each toast
//! re-asks, so a mid-session installer registration is picked up.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

/// Must match `identifier` in `src-tauri/tauri.conf.json`.
pub const APP_USER_MODEL_ID: &str = "com.jabot.app";

/// PowerShell's well-known AppUserModelID.
pub const POWERSHELL_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// Which AUMID to post with.
///
/// A registered Start Menu shortcut (#281) means the real id can show a
/// toast that names JaBot. Without one, skip `com.jabot.app` entirely —
/// a silent Ok would hide the PowerShell fallback that actually appears.
pub fn app_id_candidates(jabot_registered: bool) -> Vec<&'static str> {
    if jabot_registered {
        vec![APP_USER_MODEL_ID]
    } else {
        vec![POWERSHELL_APP_ID]
    }
}

/// Action Center `Tag` is 16 characters. Prefer the tail of the thread id
/// so two UUIDs that share a prefix still replace rather than collide.
pub fn toast_tag(thread_id: &str) -> String {
    let stripped: String = thread_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if stripped.len() <= 16 {
        stripped
    } else {
        stripped[stripped.len() - 16..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_jabot_uses_the_real_id_only() {
        assert_eq!(app_id_candidates(true), vec![APP_USER_MODEL_ID]);
    }

    #[test]
    fn unpackaged_uses_powershell_instead_of_a_silent_ok() {
        assert_eq!(app_id_candidates(false), vec![POWERSHELL_APP_ID]);
    }

    #[test]
    fn toast_tag_fits_the_sixteen_char_limit() {
        assert_eq!(toast_tag("thread-7"), "thread7");
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let tag = toast_tag(uuid);
        assert_eq!(tag.len(), 16);
        assert!(tag.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
