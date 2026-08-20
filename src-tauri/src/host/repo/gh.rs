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

use super::exec::{self, PROBE_TIMEOUT};

const GH: &str = "gh";

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

    #[test]
    fn an_empty_host_means_github_dot_com() {
        assert_eq!(status("  ").host, DEFAULT_HOST);
    }
}
