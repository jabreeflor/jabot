//! What git says about a directory, asked once and then written down.
//!
//! Registration probes; nothing else does. A sidebar that shelled out to git
//! per render would spend a subprocess per folder per repaint, and the answers
//! it cares about — which checkout, which remote — change about as often as
//! the user runs `git remote set-url`. `folder/update { refresh: true }` is
//! how they get asked again.
//!
//! Every failure here is an answer, not an error: a directory git does not
//! claim is a legal folder (threads run, the PR view skips it), and a machine
//! with no `git` at all still gets to register directories.

use std::path::Path;
use std::time::Duration;

use super::exec::{self, PROBE_TIMEOUT};
use super::origin::{self, Origin};

const GIT: &str = "git";

/// What one directory turned out to be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoProbe {
    pub git_installed: bool,
    /// `git rev-parse --show-toplevel`. `None` means "not inside a repo".
    pub repo_root: Option<String>,
    pub origin_url: Option<String>,
    /// Parsed from `origin_url`. `None` with a `Some` url means a remote we
    /// could not attribute to a forge — a local path, or something exotic.
    pub origin: Option<Origin>,
    pub default_branch: Option<String>,
    /// The branch checked out in *this* directory. `None` when detached, or in
    /// a repository with no commits yet.
    pub branch: Option<String>,
}

impl RepoProbe {
    pub fn is_git(&self) -> bool {
        self.repo_root.is_some()
    }
}

pub fn probe(dir: &Path) -> RepoProbe {
    probe_with(dir, PROBE_TIMEOUT)
}

fn probe_with(dir: &Path, timeout: Duration) -> RepoProbe {
    let dir = dir.to_string_lossy().into_owned();
    let toplevel = match git(&["-C", &dir, "rev-parse", "--show-toplevel"], timeout) {
        Ok(output) => output.line(),
        Err(exec::RunError::NotInstalled(_)) => return RepoProbe::default(),
        Err(_) => None,
    };
    let mut probe = RepoProbe {
        git_installed: true,
        repo_root: toplevel,
        ..RepoProbe::default()
    };
    let Some(root) = probe.repo_root.clone() else {
        return probe;
    };
    probe.branch = branch_of(&dir, timeout);
    // `origin` specifically, not the first remote: it is the one `gh` resolves
    // against and the one a fork's `upstream` must not be mistaken for.
    probe.origin_url = git(&["-C", &root, "remote", "get-url", "origin"], timeout)
        .ok()
        .and_then(|output| output.line());
    probe.origin = probe.origin_url.as_deref().and_then(origin::parse);
    probe.default_branch = default_branch(&root, timeout);
    probe
}

/// `--show-current` rather than `rev-parse --abbrev-ref HEAD`, because the two
/// disagree exactly where it matters: a detached worktree and a repository with
/// no commits both print something usable-looking from `rev-parse`, and neither
/// is a branch anyone can push.
fn branch_of(dir: &str, timeout: Duration) -> Option<String> {
    git(&["-C", dir, "branch", "--show-current"], timeout)
        .ok()
        .and_then(|output| output.line())
}

/// `origin/HEAD` is what a clone records as the repository's default branch.
/// It is missing on repos created with `git init` and on some older clones; a
/// `None` here means "ask `gh repo view` when there is a reason to", not
/// "main".
fn default_branch(root: &str, timeout: Duration) -> Option<String> {
    let head = git(
        &[
            "-C",
            root,
            "symbolic-ref",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        timeout,
    )
    .ok()
    .and_then(|output| output.line())?;
    let branch = head.strip_prefix("origin/").unwrap_or(&head);
    (!branch.is_empty()).then(|| branch.to_string())
}

fn git(args: &[&str], timeout: Duration) -> Result<exec::Output, exec::RunError> {
    exec::run(GIT, args, timeout)
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;
    use std::process::Command;

    /// A real repository in a temp directory. Real git, because the whole point
    /// of this module is what git answers — a fake would only ever confirm what
    /// the fake was told.
    pub fn init_repo(dir: &Path, origin_url: Option<&str>) {
        run(dir, &["init", "--initial-branch=main"]);
        run(dir, &["config", "user.email", "test@example.com"]);
        run(dir, &["config", "user.name", "Test"]);
        run(dir, &["commit", "--allow-empty", "-m", "first"]);
        if let Some(url) = origin_url {
            run(dir, &["remote", "add", "origin", url]);
        }
    }

    pub fn run(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git is required to build this project");
        assert!(
            status.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_checkout_the_remote_and_the_branch() {
        let dir = tempfile::tempdir().unwrap();
        testing::init_repo(dir.path(), Some("git@github.com:jabreeflor/jabot.git"));

        let probe = probe(dir.path());
        assert!(probe.is_git());
        // Canonicalised on both sides: macOS temp dirs are symlinked through
        // /private, and git answers with the resolved path.
        assert_eq!(
            std::fs::canonicalize(probe.repo_root.as_ref().unwrap()).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
        assert_eq!(probe.origin.as_ref().unwrap().slug(), "jabreeflor/jabot");
        assert_eq!(probe.origin.as_ref().unwrap().host, "github.com");
        assert_eq!(probe.branch.as_deref(), Some("main"));
        // `git init` records no origin/HEAD, and inventing "main" here would be
        // a guess a worktree later branches from.
        assert_eq!(probe.default_branch, None);
    }

    #[test]
    fn a_subdirectory_reports_the_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        testing::init_repo(dir.path(), None);
        let nested = dir.path().join("src/host");
        std::fs::create_dir_all(&nested).unwrap();

        let probe = probe(&nested);
        assert_eq!(
            std::fs::canonicalize(probe.repo_root.as_ref().unwrap()).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
        // A repository with no remote is still a repository.
        assert_eq!(probe.origin_url, None);
        assert_eq!(probe.origin, None);
    }

    #[test]
    fn a_plain_directory_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let probe = probe(dir.path());
        assert!(probe.git_installed);
        assert!(!probe.is_git());
        assert_eq!(probe.repo_root, None);
        assert_eq!(probe.branch, None);
    }

    #[test]
    fn a_detached_head_has_no_branch_to_report() {
        let dir = tempfile::tempdir().unwrap();
        testing::init_repo(dir.path(), None);
        testing::run(dir.path(), &["checkout", "--detach"]);

        let probe = probe(dir.path());
        assert!(probe.is_git());
        assert_eq!(probe.branch, None);
    }

    #[test]
    fn a_missing_directory_answers_rather_than_failing() {
        let probe = probe(Path::new("/jabot-does-not-exist-xyz"));
        assert!(!probe.is_git());
    }
}
