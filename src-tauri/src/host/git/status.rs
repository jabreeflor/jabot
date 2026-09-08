//! Live git facts for a conversation summary (#269).
//!
//! Registration already asked git once and wrote the answer down. That is the
//! right probe for "is this a repo" and "which remote". Change counts and the
//! branch an agent is on right now are the opposite: they move with every
//! edit, and a panel that showed yesterday's diffstat would be describing a
//! different tree. So this module shells out when the panel asks, against the
//! directory the thread is actually editing.

use std::path::{Path, PathBuf};

use super::super::repo::exec::{self, Output, RunError};
use super::worktree::{head_branch, GitFailure, GIT_TIMEOUT};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffStat {
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
}

fn run(args: &[&str]) -> Result<Output, GitFailure> {
    exec::run("git", args, GIT_TIMEOUT).map_err(|err| GitFailure {
        command: args.join(" "),
        detail: match err {
            RunError::NotInstalled(_) => "git is not installed on this machine".to_string(),
            RunError::TimedOut => format!("timed out after {}s", GIT_TIMEOUT.as_secs()),
            RunError::Failed(detail) => detail,
        },
    })
}

fn check(args: &[&str]) -> Result<Output, GitFailure> {
    let out = run(args)?;
    if out.ok() {
        return Ok(out);
    }
    let detail = out
        .stderr
        .trim()
        .lines()
        .next()
        .unwrap_or("failed")
        .to_string();
    Err(GitFailure {
        command: args.join(" "),
        detail,
    })
}

/// Whether this directory is a checkout git will answer about.
pub fn is_repository(path: &Path) -> bool {
    let dir = path.to_string_lossy().into_owned();
    run(&["-C", &dir, "rev-parse", "--is-inside-work-tree"])
        .ok()
        .and_then(|out| out.line())
        .is_some_and(|line| line == "true")
}

/// The commit this thread's changes are counted against.
///
/// Prefer the merge-base with the shared default branch so "Changes" is the
/// work on this conversation, not every commit since the repo was created.
/// Fall back to `HEAD` when there is no default, and to nothing when the
/// repository has no commits at all.
pub fn compare_base(path: &Path, default_branch: Option<&str>) -> Option<String> {
    let dir = path.to_string_lossy().into_owned();
    if let Some(branch) = default_branch.map(str::trim).filter(|b| !b.is_empty()) {
        for candidate in [
            format!("origin/{branch}"),
            branch.to_string(),
            format!("refs/heads/{branch}"),
        ] {
            if let Ok(out) = check(&["-C", &dir, "merge-base", "HEAD", &candidate]) {
                if let Some(sha) = out.line() {
                    return Some(sha);
                }
            }
        }
    }
    run(&["-C", &dir, "rev-parse", "--verify", "HEAD"])
        .ok()
        .and_then(|out| out.line())
}

pub fn current_branch(path: &Path) -> Option<String> {
    head_branch(path)
}

/// Added and removed lines versus `base`, including untracked files.
pub fn diffstat(path: &Path, base: Option<&str>) -> Result<DiffStat, GitFailure> {
    let files = changed_files(path, base)?;
    Ok(DiffStat {
        additions: files.iter().map(|f| f.additions).sum(),
        deletions: files.iter().map(|f| f.deletions).sum(),
    })
}

pub fn changed_files(path: &Path, base: Option<&str>) -> Result<Vec<ChangedFile>, GitFailure> {
    let dir = path.to_string_lossy().into_owned();
    let mut files = Vec::new();
    if let Some(base) = base {
        let numstat = check(&["-C", &dir, "diff", "--numstat", base])?;
        let names = check(&["-C", &dir, "diff", "--name-status", base])?;
        files.extend(merge_diff(&numstat.stdout, &names.stdout));
    } else {
        let numstat = check(&["-C", &dir, "diff", "--numstat"])?;
        let names = check(&["-C", &dir, "diff", "--name-status"])?;
        files.extend(merge_diff(&numstat.stdout, &names.stdout));
    }
    for untracked in untracked(&dir)? {
        if files.iter().any(|f| f.path == untracked) {
            continue;
        }
        files.push(ChangedFile {
            additions: line_count(&path.join(&untracked)),
            deletions: 0,
            path: untracked,
            status: "added".into(),
        });
    }
    Ok(files)
}

/// Unified diff versus `base`, capped so a huge tree cannot blow the frame.
pub fn unified_diff(path: &Path, base: Option<&str>) -> Result<String, GitFailure> {
    let dir = path.to_string_lossy().into_owned();
    let out = match base {
        Some(base) => check(&["-C", &dir, "diff", "--no-color", base])?,
        None => check(&["-C", &dir, "diff", "--no-color"])?,
    };
    let mut text = out.stdout;
    const CAP: usize = 100_000;
    if text.len() > CAP {
        text.truncate(CAP);
        text.push_str("\n…truncated\n");
    }
    Ok(text)
}

/// Commit everything in this tree. Uses the user's own git identity so the
/// commit is theirs, not JaBot's automatic save.
pub fn commit_all(path: &Path, message: &str) -> Result<String, GitFailure> {
    let dir = path.to_string_lossy().into_owned();
    check(&["-C", &dir, "add", "--all"])?;
    check(&["-C", &dir, "commit", "-m", message])?;
    Ok(run(&["-C", &dir, "rev-parse", "--short", "HEAD"])
        .ok()
        .and_then(|out| out.line())
        .unwrap_or_else(|| "HEAD".into()))
}

/// Push the current branch to `origin`.
pub fn push_head(path: &Path) -> Result<String, GitFailure> {
    let dir = path.to_string_lossy().into_owned();
    let out = check(&["-C", &dir, "push", "-u", "origin", "HEAD"])?;
    Ok(out.stderr.trim().to_string())
}

fn untracked(dir: &str) -> Result<Vec<String>, GitFailure> {
    let out = check(&["-C", dir, "ls-files", "--others", "--exclude-standard"])?;
    Ok(out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn merge_diff(numstat: &str, name_status: &str) -> Vec<ChangedFile> {
    let mut by_path: Vec<ChangedFile> = numstat
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let additions = parse_count(parts.next()?)?;
            let deletions = parse_count(parts.next()?)?;
            let path = parts.next()?.to_string();
            if path.is_empty() {
                return None;
            }
            Some(ChangedFile {
                path,
                status: "modified".into(),
                additions,
                deletions,
            })
        })
        .collect();
    for line in name_status.lines() {
        let mut parts = line.split('\t');
        let code = parts.next().unwrap_or("");
        let status = status_word(code);
        let path = if code.starts_with('R') || code.starts_with('C') {
            parts.nth(1).unwrap_or("").to_string()
        } else {
            parts.next().unwrap_or("").to_string()
        };
        if path.is_empty() {
            continue;
        }
        if let Some(file) = by_path.iter_mut().find(|f| f.path == path) {
            file.status = status;
        } else {
            by_path.push(ChangedFile {
                path,
                status,
                additions: 0,
                deletions: 0,
            });
        }
    }
    by_path
}

fn status_word(code: &str) -> String {
    match code.chars().next().unwrap_or('M') {
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        _ => "modified",
    }
    .into()
}

fn parse_count(raw: &str) -> Option<i64> {
    if raw == "-" {
        return Some(0);
    }
    raw.parse().ok()
}

fn line_count(path: &PathBuf) -> i64 {
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    if bytes.contains(&0) || bytes.is_empty() {
        return 0;
    }
    let lines = bytecount_newlines(&bytes);
    if bytes.last() == Some(&b'\n') {
        lines
    } else {
        lines + 1
    }
}

fn bytecount_newlines(bytes: &[u8]) -> i64 {
    bytes.iter().filter(|b| **b == b'\n').count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::repo::git::testing;
    use tempfile::tempdir;

    #[test]
    fn counts_committed_and_untracked_work() {
        let dir = tempdir().unwrap();
        testing::init_repo(dir.path(), None);
        std::fs::write(dir.path().join("README.md"), "# hi\nsecond\n").unwrap();
        testing::run(dir.path(), &["add", "README.md"]);
        testing::run(dir.path(), &["commit", "-m", "readme"]);
        std::fs::write(dir.path().join("README.md"), "# hi\nsecond\nthird\n").unwrap();
        std::fs::write(dir.path().join("new.rs"), "fn main() {}\n").unwrap();

        let base = compare_base(dir.path(), Some("main")).expect("a base");
        let files = changed_files(dir.path(), Some(&base)).unwrap();
        let readme = files.iter().find(|f| f.path == "README.md").unwrap();
        assert_eq!(readme.additions, 1);
        assert_eq!(readme.status, "modified");
        let fresh = files.iter().find(|f| f.path == "new.rs").unwrap();
        assert_eq!(fresh.additions, 1);
        assert_eq!(fresh.status, "added");
        let stat = diffstat(dir.path(), Some(&base)).unwrap();
        assert_eq!(stat.additions, 2);
        assert_eq!(stat.deletions, 0);
        assert_eq!(current_branch(dir.path()).as_deref(), Some("main"));
    }

    #[test]
    fn a_plain_directory_is_not_a_repository() {
        let dir = tempdir().unwrap();
        assert!(!is_repository(dir.path()));
        assert_eq!(compare_base(dir.path(), None), None);
    }

    #[test]
    fn commit_all_records_the_message() {
        let dir = tempdir().unwrap();
        testing::init_repo(dir.path(), None);
        std::fs::write(dir.path().join("scratch.txt"), "notes\n").unwrap();
        let sha = commit_all(dir.path(), "save the notes").unwrap();
        assert!(!sha.is_empty());
        let log = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["log", "-1", "--pretty=%s"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim(),
            "save the notes"
        );
    }
}
