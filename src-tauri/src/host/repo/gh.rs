//! GitHub auth: the user's existing `gh` login, read on demand.
//!
//! Decision (`docs/research/git-and-prs/folders-and-auth.md`): **no JaBot
//! GitHub App and no OAuth app for MVP.** Coding agents on this machine already
//! run `gh pr create`; reusing that login means one identity, one org SSO
//! dance, and no client secret to hide in a public native binary.
//!
//! The token is never persisted. `gh` holds it (keychain or its own config),
//! this module shells out for it at the moment of use, and it is dropped when
//! the call returns. Nothing here writes to SQLite, and nothing here is
//! reachable from the renderer: [`status`] answers *whether* the host can
//! authenticate and as whom, never with what. If JaBot ever needs to cache a
//! credential it goes in the vault (#9) — never in the store, in plaintext or
//! otherwise (data-and-persistence).
//!
//! Two consumers, one login: the **agent** in the worktree runs `gh` itself
//! with the user's environment, and the **host** (the PR view, #28) uses
//! [`token`] for GraphQL as the same user.

use std::time::Duration;

use super::exec::{self, PROBE_TIMEOUT};

const GH: &str = "gh";

/// Signing in is a network round trip — `gh` verifies the token against the
/// host before it stores it — so it gets its own, longer deadline than the
/// probes. Short enough that a captive portal still ends in a message.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(20);

/// The forge host `gh` defaults to. GHES folders carry their own, parsed from
/// `origin`, and `gh` is addressed per host (`--hostname`).
pub const DEFAULT_HOST: &str = "github.com";

/// What the PR surface can expect, in the user's terms. Deliberately three
/// facts rather than one boolean: "GitHub is not connected" sends a user who
/// has never installed `gh` to a login page, and a user who is logged out to
/// an install page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhAuth {
    pub installed: bool,
    pub authenticated: bool,
    pub host: String,
    pub account: Option<String>,
    pub detail: String,
    pub remedy: Option<String>,
    /// Where `gh` resolved from, so "it works in my terminal" is comparable.
    pub path: Option<String>,
}

pub fn status(host: &str) -> GhAuth {
    let host = normalize_host(host);
    let Some(path) = super::super::harness::resolve_command(GH) else {
        return GhAuth {
            installed: false,
            authenticated: false,
            host,
            account: None,
            detail: "GitHub CLI (gh) is not installed.".into(),
            remedy: Some("brew install gh".into()),
            path: None,
        };
    };
    let path = Some(path.to_string_lossy().into_owned());

    // Asking for the token is the only probe that answers the question the PR
    // view actually has — `gh auth status` can report a stored login whose
    // token this process cannot read. The value is dropped here; only the
    // verdict travels.
    let has_token = token(&host).is_some();
    if !has_token {
        return GhAuth {
            installed: true,
            authenticated: false,
            host: host.clone(),
            account: None,
            detail: format!("Not logged in to {host}."),
            remedy: Some(format!("gh auth login --hostname {host}")),
            path,
        };
    }

    let account = run(&["auth", "status", "--hostname", &host])
        .ok()
        .and_then(|output| parse_account(&format!("{}\n{}", output.stdout, output.stderr), &host));
    let detail = match &account {
        Some(account) => format!("Logged in to {host} as {account}."),
        None => format!("Logged in to {host}."),
    };
    GhAuth {
        installed: true,
        authenticated: true,
        host,
        account,
        detail,
        remedy: None,
        path,
    }
}

/// Why a sign-in did not happen. Each case has a different way forward, which
/// is the whole reason this is not a bool: a malformed paste is the user's to
/// fix here, a refusal is GitHub's to explain, and a missing `gh` is an
/// install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    NotInstalled,
    /// The paste never left this process. Nothing that looks like a token can
    /// reach argv, a log line, or a child.
    Malformed,
    /// `gh` ran and said no — a revoked token, a wrong host, an SSO block.
    /// Carries `gh`'s own first line, which is the most useful sentence
    /// anybody has about it.
    Refused(String),
    TimedOut,
}

impl LoginError {
    pub fn detail(&self) -> String {
        match self {
            Self::NotInstalled => "GitHub CLI (gh) is not installed.".to_string(),
            Self::Malformed => {
                "That does not look like a GitHub token. Paste the whole token, with no spaces."
                    .to_string()
            }
            Self::Refused(detail) => detail.clone(),
            Self::TimedOut => format!("gh did not answer within {}s.", LOGIN_TIMEOUT.as_secs()),
        }
    }

    pub fn remedy(&self, host: &str) -> Option<String> {
        match self {
            Self::NotInstalled => Some("brew install gh".to_string()),
            Self::Refused(_) => Some(format!("gh auth login --hostname {host}")),
            _ => None,
        }
    }
}

/// Hand `gh` a token to hold, and report who it makes us.
///
/// The token goes in on **stdin** (`--with-token`), never in argv: this host
/// runs children whose command lines any process on the Mac can read. It is
/// not stored here, not written to SQLite, and not echoed back — the result is
/// the same [`GhAuth`] the PR surface already gates on, so a successful login
/// and a probe are indistinguishable to every caller downstream. `gh` keeps
/// the credential where it keeps its own (the Keychain), which is the point of
/// reusing its login rather than inventing a second one (#16).
pub fn login(host: &str, token: &str) -> Result<GhAuth, LoginError> {
    let host = normalize_host(host);
    let token = token.trim();
    if !is_plausible_token(token) {
        return Err(LoginError::Malformed);
    }
    let args = ["auth", "login", "--hostname", &host, "--with-token"];
    // `gh` reads to EOF, so the newline is what ends the paste.
    let payload = format!("{token}\n");
    let spawn = exec::Spawn::new(GH, &args, LOGIN_TIMEOUT).with_stdin(&payload);
    let output = exec::spawn(spawn).map_err(|err| match err {
        exec::RunError::NotInstalled(_) => LoginError::NotInstalled,
        exec::RunError::TimedOut => LoginError::TimedOut,
        exec::RunError::Failed(detail) => LoginError::Refused(detail),
    })?;
    if !output.ok() {
        // `gh`'s own words. It never echoes the token back, and the trim keeps
        // this to the one line that says why.
        return Err(LoginError::Refused(first_line(&output.stderr)));
    }
    Ok(status(&host))
}

/// The shape of a GitHub credential, checked before it is handed to a child.
///
/// Deliberately about *shape* and not about prefixes: GitHub has minted
/// `ghp_`, `github_pat_`, `gho_` and bare 40-hex tokens, and a checker that
/// knew the list would reject next year's. What matters is that a paste with
/// whitespace, a newline in the middle, or a leading `-` never becomes a child
/// process's stdin — that is how a stray paste turns into an argument.
pub fn is_plausible_token(token: &str) -> bool {
    let len = token.len();
    (8..=512).contains(&len)
        && !token.starts_with('-')
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn first_line(text: &str) -> String {
    text.trim()
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("gh refused the sign-in")
        .trim()
        .to_string()
}

/// The active token for one host, for host-side GraphQL (#28).
///
/// Never store the result, never log it, never put it in an RPC result, and
/// never inject it into a git remote — agents use the user's own credential
/// helper (`folders-and-auth.md`).
pub fn token(host: &str) -> Option<String> {
    let host = normalize_host(host);
    let output = run(&["auth", "token", "--hostname", &host]).ok()?;
    output.line()
}

/// `gh auth status` has printed the account two ways across versions:
///
/// ```text
/// ✓ Logged in to github.com account octocat (keyring)
/// ✓ Logged in to github.com as octocat (oauth_token)
/// ```
///
/// Both are parsed, and only on the line naming the host we asked about — a
/// machine logged in to `github.com` and to a GHES host prints both.
pub fn parse_account(output: &str, host: &str) -> Option<String> {
    let needle = format!("Logged in to {host}");
    for line in output.lines() {
        let Some(rest) = line.split_once(&needle).map(|(_, rest)| rest.trim()) else {
            continue;
        };
        let account = rest
            .strip_prefix("account ")
            .or_else(|| rest.strip_prefix("as "))?;
        let account = account.split_whitespace().next()?;
        if !account.is_empty() {
            return Some(account.to_string());
        }
    }
    None
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        DEFAULT_HOST.to_string()
    } else {
        host
    }
}

fn run(args: &[&str]) -> Result<exec::Output, exec::RunError> {
    exec::run(GH, args, PROBE_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_spellings_gh_has_used() {
        let modern = "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n";
        let older = "github.com\n  ✓ Logged in to github.com as octocat (oauth_token)\n";
        assert_eq!(
            parse_account(modern, "github.com").as_deref(),
            Some("octocat")
        );
        assert_eq!(
            parse_account(older, "github.com").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn reads_the_host_that_was_asked_about() {
        let both = "github.com\n  ✓ Logged in to github.com account personal (keyring)\n\
                    git.corp.example.com\n  ✓ Logged in to git.corp.example.com account work (keyring)\n";
        assert_eq!(
            parse_account(both, "git.corp.example.com").as_deref(),
            Some("work")
        );
        assert_eq!(
            parse_account(both, "github.com").as_deref(),
            Some("personal")
        );
        assert_eq!(parse_account(both, "gitlab.com"), None);
    }

    #[test]
    fn a_logged_out_report_names_nobody() {
        let out = "github.com\n  X Failed to log in to github.com account (keyring)\n";
        assert_eq!(parse_account(out, "github.com"), None);
    }

    /// The probe has to answer on a machine with no `gh` — CI is one — and the
    /// answer has to point at installing it rather than at logging in.
    #[test]
    fn status_distinguishes_missing_from_logged_out() {
        let report = status("github.com");
        assert_eq!(report.host, "github.com");
        if report.installed {
            // Whatever this machine's login state is, the two fields must agree
            // and an unauthenticated report must carry a way forward.
            assert_eq!(report.authenticated, report.remedy.is_none());
        } else {
            assert!(!report.authenticated);
            assert_eq!(report.remedy.as_deref(), Some("brew install gh"));
            assert!(report.path.is_none());
        }
    }

    /// The check is about *shape*, and the shapes that matter are the ones
    /// that would turn a paste into something other than one token on stdin.
    #[test]
    fn a_token_is_checked_for_shape_and_never_for_a_prefix() {
        // Every spelling GitHub has minted passes, because the check knows
        // none of them by name.
        assert!(is_plausible_token("ghp_16CharactersOfBase62xyz"));
        assert!(is_plausible_token(
            "github_pat_11ABCDE0000aBcDeFgHiJk_lMnOpQrStUvWxYz"
        ));
        assert!(is_plausible_token(&"a".repeat(40)));

        assert!(!is_plausible_token(""));
        assert!(!is_plausible_token("short"));
        // A paste that brought a newline, a space or a second word with it
        // would reach `gh` as something other than one token.
        assert!(!is_plausible_token("ghp_first ghp_second"));
        assert!(!is_plausible_token("ghp_with\nnewline_inside_it"));
        // And a value that could pose as a flag never becomes one.
        assert!(!is_plausible_token("--hostname=evil.example"));
        assert!(!is_plausible_token(&"a".repeat(513)));
    }

    /// A refusal has to be sayable. The one thing it must never say is the
    /// token, which is why nothing here formats one.
    #[test]
    fn a_login_refusal_says_what_to_do_next() {
        assert_eq!(
            LoginError::NotInstalled.remedy("github.com").as_deref(),
            Some("brew install gh")
        );
        assert_eq!(LoginError::Malformed.remedy("github.com"), None);
        assert!(LoginError::Malformed
            .detail()
            .contains("does not look like"));
        let refused = LoginError::Refused("HTTP 401: Bad credentials".into());
        assert_eq!(refused.detail(), "HTTP 401: Bad credentials");
        assert_eq!(
            refused.remedy("git.corp.example.com").as_deref(),
            Some("gh auth login --hostname git.corp.example.com")
        );
    }

    /// A malformed paste is refused *before* anything is spawned, so a token
    /// that was never going to work cannot leave this process at all.
    #[test]
    fn a_malformed_token_never_reaches_a_child() {
        assert_eq!(
            login("github.com", "not a token"),
            Err(LoginError::Malformed)
        );
    }

    #[test]
    fn an_empty_host_means_github_dot_com() {
        assert_eq!(status("  ").host, DEFAULT_HOST);
    }
}
