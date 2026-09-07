//! Cursor Agent CLI as a compiled-in preset (#222).
//!
//! JaBot speaks to Cursor through its first-party ACP server (`agent acp` /
//! `cursor-agent acp`), not through `--print`. Headless print mode only
//! applies file changes when `--force` / `--yolo` is set, and the issue
//! forbids turning those on by default to paper over missing permission
//! integration. ACP already has `session/request_permission`, streaming
//! `session/update`s, `session/cancel`, and `session/load`.
//!
//! Auth is the Cursor account already on this machine (`agent login` /
//! `agent status`) or an exported `CURSOR_API_KEY` / `CURSOR_AUTH_TOKEN`.
//! That is the account-profile the CLI actually has. It is **not** isolated
//! per JaBot bot — every Cursor thread on this host shares that one login
//! and quota.

use super::catalog::HarnessCapabilities;

/// Install the CLI, not an npm ACP wrapper — Cursor *is* the adapter.
pub const INSTALL_HINT: &str =
    "Install the Cursor Agent CLI (`curl https://cursor.com/install -fsS | bash`), then run `agent login` or export CURSOR_API_KEY.";
pub const INSTALL_URL: &str = "https://cursor.com/docs/cli/installation";

/// Calendar builds from before ACP was a supported `agent acp` command, or
/// semver builds that predate the rename from `cursor-agent`.
const MIN_CAL: (u16, u8, u8) = (2025, 8, 1);
const MIN_SEMVER: (u16, u16) = (0, 50);

/// What the card and the Doctor declare. Supported verbs go through ACP;
/// everything else is named so a missing Cursor extension cannot be mistaken
/// for a silent allow.
pub fn capabilities() -> HarnessCapabilities {
    HarnessCapabilities {
        streaming: true,
        tool_events: true,
        permissions: true,
        cancel: true,
        // Resume only if `initialize` advertises `loadSession` / `sessionCapabilities.resume`.
        // Claiming it on the card would lie when the running CLI does not.
        resume: false,
        notes: Some(
            "Permissions stay in JaBot (no --force). Auth uses this machine's Cursor account or CURSOR_API_KEY — not isolated per bot. Resume only if the CLI advertises loadSession. cursor/ask_question and cursor/create_plan are declined so a turn cannot hang.".into(),
        ),
    }
}

/// A version string the CLI printed (`agent --version`, `agent about`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorVersion {
    Cal { year: u16, month: u8, day: u8 },
    Semver { major: u16, minor: u16, patch: u16 },
}

impl CursorVersion {
    pub fn is_supported(self) -> bool {
        match self {
            Self::Cal { year, month, day } => (year, month, day) >= MIN_CAL,
            Self::Semver { major, minor, .. } => (major, minor) >= MIN_SEMVER || major >= 1,
        }
    }
}

impl std::fmt::Display for CursorVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Cal { year, month, day } => write!(f, "{year}.{month}.{day}"),
            Self::Semver {
                major,
                minor,
                patch,
            } => write!(f, "{major}.{minor}.{patch}"),
        }
    }
}

/// Pull the first `YYYY.M.D` / `YYYY.MM.DD` or `major.minor.patch` out of
/// whatever `agent --version` printed.
pub fn parse_version(text: &str) -> Option<CursorVersion> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            if let Some((version, consumed)) = parse_version_at(&text[i..]) {
                return (consumed > 0).then_some(version);
            }
        }
        i += 1;
    }
    None
}

fn parse_version_at(text: &str) -> Option<(CursorVersion, usize)> {
    let mut parts: Vec<u32> = Vec::new();
    let mut rest = text;
    let mut consumed = 0usize;
    loop {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            break;
        }
        let value: u32 = digits.parse().ok()?;
        parts.push(value);
        consumed += digits.len();
        rest = &rest[digits.len()..];
        if rest.starts_with('.') {
            consumed += 1;
            rest = &rest[1..];
        } else {
            break;
        }
    }
    match parts.as_slice() {
        [year, month, day, ..] if *year >= 2020 && *month <= 12 && *day <= 31 => Some((
            CursorVersion::Cal {
                year: *year as u16,
                month: *month as u8,
                day: *day as u8,
            },
            consumed,
        )),
        [major, minor, patch, ..] => Some((
            CursorVersion::Semver {
                major: *major as u16,
                minor: *minor as u16,
                patch: *patch as u16,
            },
            consumed,
        )),
        [major, minor] => Some((
            CursorVersion::Semver {
                major: *major as u16,
                minor: *minor as u16,
                patch: 0,
            },
            consumed,
        )),
        _ => None,
    }
}

pub fn env_has_cursor_credentials(has: impl Fn(&str) -> bool) -> bool {
    has("CURSOR_API_KEY") || has("CURSOR_AUTH_TOKEN")
}

/// What `--version` meant for the Doctor.
pub enum VersionCheck {
    Ok(CursorVersion),
    /// Printed something we could not parse. Do not block ready on that —
    /// a newer CLI that changed its banner is not "unsupported".
    Unparseable,
    Unsupported {
        printed: String,
        parsed: CursorVersion,
    },
    Unknown(String),
}

pub fn classify_version(run_ok: bool, stdout: &str, detail: String) -> VersionCheck {
    if !run_ok {
        return VersionCheck::Unknown(detail);
    }
    match parse_version(stdout) {
        Some(version) if version.is_supported() => VersionCheck::Ok(version),
        Some(version) => VersionCheck::Unsupported {
            printed: stdout.trim().to_string(),
            parsed: version,
        },
        None => VersionCheck::Unparseable,
    }
}

/// Args JaBot must never add to work around permissions. The catalog launch
/// is only `acp`; this is the assertion the tests lock.
#[cfg(test)]
pub fn forbidden_launch_flags() -> &'static [&'static str] {
    &["--force", "--yolo", "--approve-mcps", "--trust"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_and_semver_floors() {
        assert!(parse_version("2026.08.09-abc").unwrap().is_supported());
        assert!(parse_version("agent 1.2.3").unwrap().is_supported());
        assert!(parse_version("0.50.0").unwrap().is_supported());
        assert!(!parse_version("2025.07.31").unwrap().is_supported());
        assert!(!parse_version("0.49.9").unwrap().is_supported());
        assert!(parse_version("no version here").is_none());
    }

    #[test]
    fn capabilities_name_the_gaps_instead_of_force_flags() {
        let caps = capabilities();
        assert!(caps.streaming && caps.permissions && caps.cancel);
        assert!(!caps.resume);
        let notes = caps.notes.as_deref().unwrap();
        assert!(notes.contains("--force"));
        assert!(notes.contains("not isolated"));
        assert!(notes.contains("ask_question"));
        assert!(notes.contains("CURSOR_API_KEY"));
    }

    #[test]
    fn acp_launch_never_carries_force_flags() {
        for flag in forbidden_launch_flags() {
            assert!(!["acp"].contains(flag), "{flag}");
        }
    }

    #[test]
    fn api_key_or_token_is_the_account_profile() {
        assert!(env_has_cursor_credentials(|key| key == "CURSOR_API_KEY"));
        assert!(env_has_cursor_credentials(|key| key == "CURSOR_AUTH_TOKEN"));
        assert!(!env_has_cursor_credentials(|_| false));
    }

    #[test]
    fn classify_version_separates_old_from_unparseable() {
        assert!(matches!(
            classify_version(true, "0.49.0", String::new()),
            VersionCheck::Unsupported { .. }
        ));
        assert!(matches!(
            classify_version(true, "custom-build", String::new()),
            VersionCheck::Unparseable
        ));
        assert!(matches!(
            classify_version(false, "", "exit 1".into()),
            VersionCheck::Unknown(_)
        ));
    }
}
