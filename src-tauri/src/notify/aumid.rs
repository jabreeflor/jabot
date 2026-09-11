//! Windows AppUserModelID order. Portable so Linux verify can test it.
//!
//! Packaged JaBot should toast as [`APP_USER_MODEL_ID`]. Until the installer
//! registers that id (#281), WinRT often rejects it; [`app_id_candidates`]
//! then offers PowerShell's well-known id so a toast still appears. The
//! PowerShell path *looks* like PowerShell. Do not treat it as the shipping
//! identity.
//!
//! Last-success is not sticky. Always try the real id first so a mid-session
//! installer registration is picked up without restarting JaBot.

/// Must match `identifier` in `src-tauri/tauri.conf.json`.
pub const APP_USER_MODEL_ID: &str = "com.jabot.app";

/// PowerShell's well-known AppUserModelID.
pub const POWERSHELL_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// Real id first, PowerShell last. Last-success is not an input: a previous
/// PowerShell success must not skip `com.jabot.app` forever.
pub fn app_id_candidates() -> Vec<&'static str> {
    vec![APP_USER_MODEL_ID, POWERSHELL_APP_ID]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_aumid_always_tries_jabot_then_powershell() {
        assert_eq!(
            app_id_candidates(),
            vec![APP_USER_MODEL_ID, POWERSHELL_APP_ID]
        );
    }
}
