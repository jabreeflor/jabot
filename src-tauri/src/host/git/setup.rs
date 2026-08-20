//! Making a fresh worktree runnable: copy the ignored files in, run the
//! folder's setup command.
//!
//! A `git worktree add` produces a **tracked-files** checkout. `node_modules`,
//! `.env`, and the local SQLite the app needs to boot are all gitignored, so
//! they are all missing, and an agent whose first act is `npm test` in that
//! tree fails for a reason that has nothing to do with the task. Every mature
//! product solves this the same way — Claude and Codex with `.worktreeinclude`,
//! Cursor with `.cursor/worktrees.json`, Conductor with Files to copy plus a
//! setup script (`docs/research/git-and-prs/worktrees.md`) — and #16 already
//! records both halves on the folder.
//!
//! Two rules.
//!
//! **The repo's own `.worktreeinclude` is honoured.** A project already set up
//! for Claude, Codex or Conductor should not need to be set up again for JaBot,
//! and the file is the one piece of this configuration that lives with the code
//! rather than in one user's app database.
//!
//! **Setup failing is not spawn failing.** A missing `.env`, a `npm ci` that
//! cannot reach the network — those make a worktree less useful, not unusable,
//! and the thread is still the user's to prompt. What is reported is what
//! happened, so a Doctor or a thread view can say why the tests are failing.
//! Copying *never* follows a path out of the repository, which is a boundary
//! and not a nicety: `files_to_copy` is user-editable text.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::repo::exec::{self, RunError, Spawn};

/// What a `npm ci` in a large monorepo needs, and no more. The host request
/// thread is blocked for the duration — see the module docs on `git/mod.rs` —
/// so this is a deliberate ceiling, not a guess at how long setup takes.
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(300);

/// The file Claude Code, Codex and Conductor all read.
pub const WORKTREE_INCLUDE: &str = ".worktreeinclude";

/// What the folder (#16) says this repository needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub files: Vec<String>,
    pub command: Option<String>,
}

/// What actually happened, for the log and for anyone who asks later why the
/// tree is missing something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub copied: Vec<String>,
    /// Listed, but not present in the source checkout. Normal: a folder that
    /// suggests `.env.local` is right to, even in a repo that has none.
    pub missing: Vec<String>,
    /// Listed, and refused — a path that leaves the repository.
    pub refused: Vec<String>,
    pub failed: Vec<String>,
    /// `None` when the folder has no setup command.
    pub command_ok: Option<bool>,
    pub command_detail: Option<String>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty() && self.failed.is_empty() && self.command_ok != Some(false)
    }
}

/// Copy the ignored files in, then run the setup command in the new tree.
pub fn apply(repo_root: &Path, worktree: &Path, plan: &Plan, thread_id: &str) -> Report {
    let mut report = Report::default();
    for entry in entries(repo_root, plan) {
        let Some(relative) = safe_relative(&entry) else {
            report.refused.push(entry);
            continue;
        };
        let from = repo_root.join(&relative);
        if !from.exists() {
            report.missing.push(entry);
            continue;
        }
        match copy(&from, &worktree.join(&relative)) {
            Ok(()) => report.copied.push(entry),
            Err(err) => report.failed.push(format!("{entry}: {err}")),
        }
    }
    if let Some(command) = plan
        .command
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        let (ok, detail) = run_command(repo_root, worktree, command, thread_id);
        report.command_ok = Some(ok);
        report.command_detail = detail;
    }
    report
}

/// The folder's list first, then the repository's own `.worktreeinclude`, with
/// duplicates dropped. The folder wins on ordering because it is the setting
/// the user edited most recently and the one they can see.
fn entries(repo_root: &Path, plan: &Plan) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |value: &str| {
        let value = value.trim();
        if value.is_empty() || out.iter().any(|seen| seen == value) {
            return;
        }
        out.push(value.to_string());
    };
    for file in &plan.files {
        push(file);
    }
    if let Ok(text) = std::fs::read_to_string(repo_root.join(WORKTREE_INCLUDE)) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            push(line);
        }
    }
    out
}

/// A relative path that stays inside the repository, or nothing.
///
/// `files_to_copy` and `.worktreeinclude` are both text a person types. An
/// absolute path or a `..` would make "copy the ignored files this project
/// needs" into "copy whatever this string points at", and the destination side
/// of the same join would write outside the worktree.
fn safe_relative(entry: &str) -> Option<PathBuf> {
    let path = Path::new(entry);
    if path.is_absolute() {
        return None;
    }
    let mut relative = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::Normal(name) => relative.push(name),
            // `./foo` is a person writing a relative path, and harmless.
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

/// Copy a file, or a directory and everything under it.
///
/// Never a symlink to somewhere else on disk: `node_modules` symlinked from the
/// main checkout is the one thing Cursor's docs explicitly warn against, since
/// a `npm install` in the worktree then rewrites the user's own tree.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to copy a symlink into a worktree",
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// `sh -c <command>` in the worktree, with the paths a setup script needs.
///
/// `$JABOT_REPO_ROOT` is the Cursor `$ROOT_WORKTREE_PATH` idea: the common
/// setup script is "copy something out of the main checkout", and a script that
/// has to guess where that is cannot be written once and reused.
fn run_command(
    repo_root: &Path,
    worktree: &Path,
    command: &str,
    thread_id: &str,
) -> (bool, Option<String>) {
    let env = [
        (
            "JABOT_WORKTREE_PATH",
            worktree.to_string_lossy().into_owned(),
        ),
        ("JABOT_REPO_ROOT", repo_root.to_string_lossy().into_owned()),
        ("JABOT_THREAD_ID", thread_id.to_string()),
    ];
    let args = ["-c", command];
    match exec::spawn(
        Spawn::new("sh", &args, SETUP_TIMEOUT)
            .in_dir(worktree)
            .with_env(&env),
    ) {
        Ok(out) if out.ok() => (true, None),
        Ok(out) => (
            false,
            Some(tail(&format!(
                "{}\n{}",
                out.stdout.trim(),
                out.stderr.trim()
            ))),
        ),
        Err(RunError::TimedOut) => (
            false,
            Some(format!(
                "setup command timed out after {}s",
                SETUP_TIMEOUT.as_secs()
            )),
        ),
        Err(RunError::NotInstalled(cmd)) => (false, Some(format!("{cmd} is not installed"))),
        Err(RunError::Failed(detail)) => (false, Some(detail)),
    }
}

/// The last few lines of a failed build, not all of it: this ends up in a log
/// line and, later, in a card.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(5);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trees() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&tree).unwrap();
        (dir, repo, tree)
    }

    #[test]
    fn the_ignored_files_a_fresh_tree_needs_are_copied_in() {
        let (_dir, repo, tree) = trees();
        std::fs::write(repo.join(".env"), "TOKEN=1").unwrap();
        std::fs::create_dir_all(repo.join("config/local")).unwrap();
        std::fs::write(repo.join("config/local/db.json"), "{}").unwrap();

        let plan = Plan {
            files: vec![".env".into(), "config".into(), ".env.local".into()],
            command: None,
        };
        let report = apply(&repo, &tree, &plan, "t-1");

        assert_eq!(
            std::fs::read_to_string(tree.join(".env")).unwrap(),
            "TOKEN=1"
        );
        assert!(tree.join("config/local/db.json").exists());
        // A listed file the repo does not have is normal, not a failure.
        assert_eq!(report.missing, vec![".env.local".to_string()]);
        assert!(report.is_clean(), "{report:?}");
    }

    #[test]
    fn a_repos_own_worktreeinclude_is_honoured_alongside_the_folder_setting() {
        let (_dir, repo, tree) = trees();
        std::fs::write(
            repo.join(WORKTREE_INCLUDE),
            "# ignored files\n\n.env\nlocal.db\n",
        )
        .unwrap();
        std::fs::write(repo.join(".env"), "TOKEN=1").unwrap();
        std::fs::write(repo.join("local.db"), "rows").unwrap();

        // `.env` is in both lists; it must not be copied twice or reported twice.
        let plan = Plan {
            files: vec![".env".into()],
            command: None,
        };
        let report = apply(&repo, &tree, &plan, "t-1");
        assert_eq!(
            report.copied,
            vec![".env".to_string(), "local.db".to_string()]
        );
        assert_eq!(
            std::fs::read_to_string(tree.join("local.db")).unwrap(),
            "rows"
        );
    }

    #[test]
    fn a_path_that_leaves_the_repository_is_refused_rather_than_followed() {
        let (dir, repo, tree) = trees();
        std::fs::write(dir.path().join("secret"), "not yours").unwrap();

        let plan = Plan {
            files: vec![
                "../secret".into(),
                "/etc/hosts".into(),
                "nested/../../secret".into(),
            ],
            command: None,
        };
        let report = apply(&repo, &tree, &plan, "t-1");

        assert_eq!(report.refused.len(), 3, "{report:?}");
        assert!(report.copied.is_empty());
        assert!(!tree.join("secret").exists());
        assert!(!dir.path().join("tree").join("..").join("secret2").exists());
    }

    #[test]
    fn a_symlink_is_not_followed_into_the_users_checkout() {
        let (_dir, repo, tree) = trees();
        std::fs::create_dir_all(repo.join("node_modules")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(repo.join("node_modules"), repo.join("modules")).unwrap();

        let plan = Plan {
            files: vec!["modules".into()],
            command: None,
        };
        let report = apply(&repo, &tree, &plan, "t-1");
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert!(!tree.join("modules").exists());
    }

    #[test]
    fn the_setup_command_runs_in_the_tree_and_is_told_where_the_repo_is() {
        let (_dir, repo, tree) = trees();
        let plan = Plan {
            files: Vec::new(),
            command: Some("pwd > where.txt && echo $JABOT_REPO_ROOT >> where.txt".into()),
        };
        let report = apply(&repo, &tree, &plan, "t-setup");

        assert_eq!(report.command_ok, Some(true), "{report:?}");
        let written = std::fs::read_to_string(tree.join("where.txt")).unwrap();
        assert!(
            written.contains(&tree.to_string_lossy().into_owned()),
            "{written}"
        );
        assert!(
            written.contains(&repo.to_string_lossy().into_owned()),
            "{written}"
        );
    }

    #[test]
    fn a_failing_setup_command_is_reported_and_does_not_stop_anything() {
        let (_dir, repo, tree) = trees();
        let plan = Plan {
            files: Vec::new(),
            command: Some("echo 'no lockfile' >&2; exit 1".into()),
        };
        let report = apply(&repo, &tree, &plan, "t-setup");
        assert_eq!(report.command_ok, Some(false));
        assert!(!report.is_clean());
        assert!(report.command_detail.unwrap().contains("no lockfile"));
    }
}
