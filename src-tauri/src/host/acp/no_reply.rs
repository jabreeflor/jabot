//! Why a harness turn produced no visible reply.
//!
//! #183 / #203 already turn a successful `end_turn` with no agent text into
//! `empty_response`. Claude Code (and any other adapter) can fail that way
//! for several different prerequisites — not signed in, CLI gone, a model the
//! adapter will not run, or the process exiting before it answers. The generic
//! "failed: no reply" line named every one of those and identified none.
//!
//! Classification is a best-effort read of the adapter's stderr log. Unknown
//! output stays `empty_response` or `adapter_exit` and still carries the
//! excerpt, so a retry after fixing sign-in or the adapter is pointed at the
//! same log a human would open.

use std::fs;
use std::path::Path;

/// How a no-reply turn is recorded on the run and the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoReplyDiagnosis {
    pub stop_reason: &'static str,
    pub summary: String,
}

impl NoReplyDiagnosis {
    /// `last_error` / run error: keep the stop-reason token so existing
    /// `empty_response` assertions still match, then the sentence a human
    /// can act on, then the adapter's own words when we have them.
    pub fn error_line(&self) -> String {
        format!("stopped: {} — {}", self.stop_reason, self.summary)
    }
}

/// Read the tail of an adapter stderr log. Missing or empty is `None`.
pub fn read_log_excerpt(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    excerpt(&raw)
}

pub fn diagnose(stderr: &str, process_exited: bool) -> NoReplyDiagnosis {
    let excerpt = excerpt(stderr);
    let kind = classify(stderr).unwrap_or(if process_exited {
        Kind::AdapterExit
    } else {
        Kind::Empty
    });
    let mut summary = kind.summary().to_string();
    if let Some(excerpt) = excerpt {
        summary.push_str("\n\nAdapter log:\n");
        summary.push_str(&excerpt);
    }
    NoReplyDiagnosis {
        stop_reason: kind.stop_reason(),
        summary,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    NotSignedIn,
    CliUnavailable,
    UnsupportedModel,
    AdapterLaunch,
    AdapterExit,
    Empty,
}

impl Kind {
    fn stop_reason(self) -> &'static str {
        match self {
            Self::NotSignedIn => "not_signed_in",
            Self::CliUnavailable => "cli_unavailable",
            Self::UnsupportedModel => "unsupported_model",
            Self::AdapterLaunch => "adapter_launch",
            Self::AdapterExit => "adapter_exit",
            Self::Empty => "empty_response",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::NotSignedIn => {
                "The harness is not signed in. Run `claude` once and sign in, or export ANTHROPIC_API_KEY, then retry."
            }
            Self::CliUnavailable => {
                "The harness CLI is not available. Install Claude Code (and its ACP adapter), then retry."
            }
            Self::UnsupportedModel => {
                "The harness rejected the model configuration. Check the selected model, then retry."
            }
            Self::AdapterLaunch => {
                "The harness adapter failed to start. Check the adapter install and retry."
            }
            Self::AdapterExit => {
                "The harness process exited before sending a reply. Check the adapter, sign-in, and model configuration, then retry."
            }
            Self::Empty => {
                "The harness ended without a reply. Check the harness’s adapter, sign-in, and model configuration, then retry."
            }
        }
    }
}

fn classify(stderr: &str) -> Option<Kind> {
    let lower = stderr.to_ascii_lowercase();
    if matches_any(
        &lower,
        &[
            "not logged in",
            "not authenticated",
            "logged out",
            "please run /login",
            "please run `claude`",
            "please run claude",
            "run `claude` once",
            "authentication failed",
            "invalid api key",
            "invalid x-api-key",
            "missing api key",
            "unauthorized",
            "auth token",
            "sign in",
            "signin required",
        ],
    ) {
        return Some(Kind::NotSignedIn);
    }
    if matches_any(
        &lower,
        &[
            "unsupported model",
            "unknown model",
            "invalid model",
            "model not found",
            "model is not available",
            "no such model",
            "could not resolve model",
        ],
    ) {
        return Some(Kind::UnsupportedModel);
    }
    if matches_any(
        &lower,
        &[
            "failed to spawn",
            "cannot start",
            "adapter failed to start",
            "eacces",
        ],
    ) {
        return Some(Kind::AdapterLaunch);
    }
    if matches_any(
        &lower,
        &[
            "command not found",
            "enoent",
            "no such file or directory",
            "not found on path",
            "is not recognized as",
        ],
    ) {
        return Some(Kind::CliUnavailable);
    }
    None
}

fn matches_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn excerpt(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let tail = if lines.len() > 20 {
        &lines[lines.len() - 20..]
    } else {
        &lines[..]
    };
    let text = tail.join("\n");
    if text.len() > 1200 {
        Some(text[text.len() - 1200..].to_string())
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sign_in_beats_a_generic_empty_turn() {
        let d = diagnose("not logged in. Run `claude` once and sign in.", false);
        assert_eq!(d.stop_reason, "not_signed_in");
        assert!(d.summary.contains("not signed in"));
        assert!(d.summary.contains("Adapter log:"));
        assert!(d.summary.contains("not logged in"));
        assert!(d.error_line().contains("not_signed_in"));
    }

    #[test]
    fn model_rejection_is_named() {
        let d = diagnose(
            "unsupported model: claude-opus-4-99 is not available",
            false,
        );
        assert_eq!(d.stop_reason, "unsupported_model");
        assert!(d.summary.contains("model configuration"));
    }

    #[test]
    fn missing_cli_is_named() {
        let d = diagnose("claude: command not found", false);
        assert_eq!(d.stop_reason, "cli_unavailable");
    }

    #[test]
    fn launch_failure_is_named() {
        let d = diagnose("failed to spawn claude-agent-acp: EACCES", false);
        assert_eq!(d.stop_reason, "adapter_launch");
    }

    #[test]
    fn a_quiet_empty_turn_stays_empty_response() {
        let d = diagnose("", false);
        assert_eq!(d.stop_reason, "empty_response");
        assert!(!d.summary.contains("Adapter log:"));
        assert!(d.error_line().contains("empty_response"));
    }

    #[test]
    fn a_process_that_exits_before_a_reply_is_adapter_exit() {
        let d = diagnose("adapter panicked: boom", true);
        assert_eq!(d.stop_reason, "adapter_exit");
        assert!(d.summary.contains("exited before sending a reply"));
        assert!(d.summary.contains("adapter panicked: boom"));
    }

    #[test]
    fn a_sign_in_error_wins_even_when_the_process_exits() {
        let d = diagnose(
            "Error: Claude Code is not authenticated. Please run /login.",
            true,
        );
        assert_eq!(d.stop_reason, "not_signed_in");
    }

    #[test]
    fn excerpt_keeps_the_tail_of_a_long_log() {
        let mut log = String::new();
        for i in 0..40 {
            log.push_str(&format!("line {i}\n"));
        }
        let taken = excerpt(&log).unwrap();
        assert!(!taken.contains("line 0"));
        assert!(taken.contains("line 39"));
    }

    #[test]
    fn missing_log_is_none() {
        assert!(read_log_excerpt(Path::new("/tmp/jabot-no-such-adapter.log")).is_none());
    }

    #[test]
    fn a_written_log_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.stderr.log");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "not logged in").unwrap();
        drop(file);
        assert_eq!(read_log_excerpt(&path).as_deref(), Some("not logged in"));
    }
}
