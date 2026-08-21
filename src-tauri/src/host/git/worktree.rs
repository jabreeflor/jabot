//! The git half of the worktree manager: add, inspect, save, remove.
//!
//! Everything here shells out to `git` through [`super::super::repo::exec`],
//! for the same two reasons folder registration does: the augmented PATH (a
//! `.app` launched from the Dock cannot see Homebrew's git) and a deadline
//! plus a process group on every call (git shells out further, to credential
//! helpers and hooks).
//!
//! Three rules the callers depend on.
//!
//! **A branch is checked out in exactly one tree.** That is git's rule, not
//! ours (`docs/research/git-and-prs/worktrees.md`), and it is why every thread
//! gets a fresh `jabot/<id>` branch instead of sharing the folder's. A name
//! already taken is resolved to a free one rather than reused: the collision
//! case is a thread deleted and re-opened under the same id, and its old
//! branch is still holding work.
//!
//! **A locked tree is a live tree.** `worktree add --lock` is what stops a
//! `git worktree prune` — the user's, or ours — from collecting a directory an
//! agent is currently writing into. Removal unlocks first, deliberately.
//!
//! **Nothing is removed before it is saved.** [`save_uncommitted`] is the whole
//! answer to "removing a worktree that has uncommitted work loses it": the work
//! becomes a commit on the thread's own branch, so the tree can go and the work
//! cannot. The branch outlives the thread on purpose. "On the thread's own
//! branch" is checked and not assumed — an agent that ran `git checkout <sha>`,
//! `git bisect`, or a rebase that stopped on a conflict leaves HEAD detached,
//! and a commit made there is reachable from no ref at all, so the removal that
//! follows would take the only thing pointing at it. [`save_uncommitted`] plants
//! a ref itself in that case and says which one, so the caller can record it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::repo::exec::{self, Output, RunError};

/// Long enough to check out a large repository, short enough that a git
/// waiting on an index lock does not read as a frozen app.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Branch names JaBot mints. Prefixed so a human reading `git branch` knows
/// which of these are ours, and so nothing collides with `worktree-*` (Claude)
/// or the user's own naming.
pub const BRANCH_PREFIX: &str = "jabot/";

/// How many `-2`, `-3`, … suffixes to try before giving up on a free branch.
const BRANCH_ATTEMPTS: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFailure {
    pub command: String,
    pub detail: String,
}

impl std::fmt::Display for GitFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "git {}: {}", self.command, self.detail)
    }
}

/// What one `git worktree add` was asked for.
#[derive(Debug, Clone)]
pub struct Plan {
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    /// The commit-ish the branch starts from, already resolved to something
    /// git can name. Never a dirty `HEAD` we merely hope is still there.
    pub base: String,
}

/// Run git and hand back what it said, whatever it said.
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

/// Run git and treat a non-zero exit as the failure it is.
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

/// The commit a new thread branch should start from.
///
/// `origin/<default>` first — the research is explicit that a thread starts
/// from the shared default branch and not from whatever the user happens to
/// have checked out and half-edited. The local default branch is the fallback
/// for a repo with no remote, and `HEAD` the last resort. `None` means the
/// repository has no commits at all, which is not an error: it is a repository
/// no worktree can be added to, and the caller falls back to the checkout.
pub fn base_commit(repo_root: &Path, default_branch: Option<&str>) -> Option<String> {
    let root = repo_root.to_string_lossy().into_owned();
    let mut candidates = Vec::new();
    if let Some(branch) = default_branch {
        candidates.push(format!("origin/{branch}"));
        candidates.push(branch.to_string());
    }
    candidates.push("HEAD".to_string());
    for candidate in candidates {
        if let Some(sha) = resolve(&root, &candidate) {
            return Some(sha);
        }
    }
    None
}

/// Resolve any ref the caller named — a branch, a tag, a sha, `origin/main` —
/// to a commit. Refs are resolved before `worktree add` rather than handed to
/// it, so "that ref does not exist" is a message about the ref and not a
/// half-created tree.
pub fn resolve(repo_root: &str, refname: &str) -> Option<String> {
    let spec = format!("{refname}^{{commit}}");
    run(&["-C", repo_root, "rev-parse", "--verify", "--quiet", &spec])
        .ok()
        .and_then(|out| out.line())
}

/// A branch name for this thread that nothing else is holding.
pub fn free_branch(repo_root: &Path, slug: &str) -> String {
    let root = repo_root.to_string_lossy().into_owned();
    let first = format!("{BRANCH_PREFIX}{slug}");
    if !branch_exists(&root, &first) {
        return first;
    }
    for n in 2..=BRANCH_ATTEMPTS {
        let candidate = format!("{BRANCH_PREFIX}{slug}-{n}");
        if !branch_exists(&root, &candidate) {
            return candidate;
        }
    }
    // Twenty live branches for one thread id means something is very wrong;
    // a name with the clock in it still lets this spawn succeed.
    format!(
        "{BRANCH_PREFIX}{slug}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    )
}

fn branch_exists(repo_root: &str, branch: &str) -> bool {
    let refname = format!("refs/heads/{branch}");
    run(&["-C", repo_root, "show-ref", "--verify", "--quiet", &refname])
        .map(|out| out.ok())
        .unwrap_or(false)
}

/// `git worktree add --lock -b <branch> <path> <base>`.
///
/// The lock is taken by `add` rather than after it: between the two there is a
/// window in which a `git worktree prune` — the user's, or the sweep on our own
/// next boot — would collect a directory that is about to hold an agent.
pub fn add(plan: &Plan) -> Result<(), GitFailure> {
    if let Some(parent) = plan.path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| GitFailure {
            command: format!("mkdir {}", parent.display()),
            detail: err.to_string(),
        })?;
    }
    let root = plan.repo_root.to_string_lossy().into_owned();
    let path = plan.path.to_string_lossy().into_owned();
    let reason = format!("jabot worktree for {}", plan.branch);
    let with_reason: Vec<&str> = vec![
        "-C",
        &root,
        "worktree",
        "add",
        "--lock",
        "--reason",
        &reason,
        "-b",
        &plan.branch,
        &path,
        &plan.base,
    ];
    match check(&with_reason) {
        Ok(_) => Ok(()),
        Err(err) if mentions_unknown_reason(&err) => {
            // `worktree add --reason` is newer than `--lock`. The reason is a
            // courtesy to a human reading `git worktree list`; the lock is the
            // part that matters, so an older git gets the lock without it.
            check(&[
                "-C",
                &root,
                "worktree",
                "add",
                "--lock",
                "-b",
                &plan.branch,
                &path,
                &plan.base,
            ])
            .map(|_| ())
        }
        Err(err) => Err(err),
    }
}

/// Put a tree back at the path a thread already believes is its `cwd`, on the
/// branch that thread already owns.
///
/// The pair to archive's removal: `archived → active` is a legal transition, and
/// without this the thread would come back pointing its adapter at a directory
/// that no longer exists. No `-b`: the branch is the one archive committed the
/// work onto, so the restored tree opens holding it.
pub fn restore(repo_root: &Path, path: &Path, branch: &str) -> Result<(), GitFailure> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| GitFailure {
            command: format!("mkdir {}", parent.display()),
            detail: err.to_string(),
        })?;
    }
    let root = repo_root.to_string_lossy().into_owned();
    let dir = path.to_string_lossy().into_owned();
    // Stale metadata from the removal is what would otherwise make this fail
    // with "already registered" at a path that is not there.
    let _ = run(&["-C", &root, "worktree", "prune"]);
    check(&["-C", &root, "worktree", "add", "--lock", &dir, branch]).map(|_| ())
}

fn mentions_unknown_reason(err: &GitFailure) -> bool {
    let detail = err.detail.to_ascii_lowercase();
    detail.contains("--reason") || detail.contains("unknown option")
}

/// Whether the tree holds anything a removal would destroy.
///
/// `--porcelain` covers modified tracked files *and* untracked ones; both are
/// work. Ignored files (`node_modules`, the copied `.env`) are deliberately not
/// counted: they were put there by setup, not by the agent, and git will not
/// let them block a removal either.
/// The branch checked out in a directory right now, or `None` for a detached
/// HEAD — and for a directory that is not a checkout at all.
///
/// Read rather than remembered. `threads.branch` is what the thread was *given*
/// at spawn (#23); an agent that rebased, or the `jabot/<id>-rescue` branch a
/// detached HEAD earns on save, has moved on from it. PR linkage asks "which
/// branch would a pull request have been opened from", and only the tree can
/// answer that (#28).
pub fn head_branch(dir: &Path) -> Option<String> {
    let dir = dir.to_string_lossy().into_owned();
    run(&["-C", &dir, "branch", "--show-current"])
        .ok()
        .and_then(|out| out.line())
}

pub fn is_dirty(path: &Path) -> bool {
    let dir = path.to_string_lossy().into_owned();
    match run(&["-C", &dir, "status", "--porcelain"]) {
        Ok(out) if out.ok() => !out.stdout.trim().is_empty(),
        // A tree git cannot read is a tree we must not assume is empty.
        _ => true,
    }
}

/// A commit [`save_uncommitted`] made, and the branch that holds it.
///
/// Both halves matter to the caller: the sha is what gets logged, and the
/// branch is what the thread's row has to point at afterwards, because a
/// removal is about to take the worktree's own `HEAD` and reflog with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub sha: String,
    /// Always a `jabot/` branch, and not necessarily the one the row was
    /// stamped with: the agent may have moved HEAD since the tree was made.
    pub branch: String,
}

/// Commit whatever is in the tree onto the thread's own branch.
///
/// This is the policy that makes cleanup safe: archive and delete both go
/// through here first, so the tree can be removed without the work in it going
/// anywhere. The commit lands on `jabot/<id>`, which JaBot never deletes — the
/// user gets it back with `git worktree add <path> jabot/<id>` or plain
/// `git checkout`.
///
/// The commit alone is not the guarantee, though: `git commit` moves whatever
/// HEAD happens to be, and an agent leaves HEAD detached every time it runs
/// `git checkout <sha>` or stops mid-rebase. A commit on a detached HEAD is
/// held by nothing but the worktree's own `HEAD` file, which the removal
/// deletes — so the work would be a dangling object waiting for `gc`, with the
/// host having logged that it saved it. A branch of the agent's own is the same
/// defect one step quieter: the work is reachable, but not from the branch the
/// row names, so a reopen restores the wrong one. Both are answered the same
/// way — plant a `jabot/` ref at the new commit and tell the caller its name.
///
/// Identity, hooks and signing are all overridden: a machine with no
/// `user.email`, a `pre-commit` that fails, or a GPG key that wants a
/// passphrase must not be able to turn "save this work" into "lose this work".
pub fn save_uncommitted(path: &Path, thread_id: &str) -> Result<Option<Saved>, GitFailure> {
    if !is_dirty(path) {
        return Ok(None);
    }
    let dir = path.to_string_lossy().into_owned();
    check(&["-C", &dir, "add", "--all"])?;
    let subject = format!("jabot: uncommitted work from thread {thread_id}");
    let body = "Saved automatically so the thread's worktree could be removed. \
                The branch is kept; nothing here has been pushed.";
    check(&[
        "-C",
        &dir,
        "-c",
        "user.name=JaBot",
        "-c",
        "user.email=jabot@localhost",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--no-verify",
        "--no-gpg-sign",
        "-m",
        &subject,
        "-m",
        body,
    ])?;
    // `check`, not a best-effort `run`: a rev-parse that fails right after a
    // successful commit means the repository is in a state we cannot reason
    // about, and reporting "nothing was saved" would let the caller remove the
    // tree anyway.
    let sha = check(&["-C", &dir, "rev-parse", "HEAD"])?
        .line()
        .ok_or_else(|| GitFailure {
            command: "rev-parse HEAD".to_string(),
            detail: "the commit just made has no sha".to_string(),
        })?;
    let branch = anchor(&dir, thread_id, &sha)?;
    Ok(Some(Saved { sha, branch }))
}

/// The `jabot/` branch that holds `sha`, planting one if nothing does.
///
/// The `?` on `update-ref` is deliberate: archive's non-forced path turns an
/// error here into "the tree is kept", which is the only safe answer when the
/// work cannot be made reachable.
fn anchor(dir: &str, thread_id: &str, sha: &str) -> Result<String, GitFailure> {
    // `--quiet` because a detached HEAD is an expected answer here, not an
    // error worth printing; `--short` to get `jabot/x` rather than the full
    // `refs/heads/jabot/x`.
    let head = run(&["-C", dir, "symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .and_then(|out| out.line());
    if let Some(branch) = head {
        if branch.starts_with(BRANCH_PREFIX) {
            return Ok(branch);
        }
    }
    // A free name rather than a fixed one: a thread can be archived from a
    // detached HEAD more than once, and moving an existing rescue branch would
    // make the *previous* rescue the dangling commit this exists to prevent.
    let rescue = free_branch(Path::new(dir), &format!("{}-rescue", slug(thread_id)));
    check(&[
        "-C",
        dir,
        "update-ref",
        &format!("refs/heads/{rescue}"),
        sha,
    ])?;
    Ok(rescue)
}

/// Unlock, remove, prune. `force` is the difference between Archive and Delete.
pub fn remove(repo_root: &Path, path: &Path, force: bool) -> Result<(), GitFailure> {
    let root = repo_root.to_string_lossy().into_owned();
    let dir = path.to_string_lossy().into_owned();
    // An unlock failure is not fatal: a tree the user already unlocked, or one
    // whose metadata is gone, still has to be removable.
    let _ = run(&["-C", &root, "worktree", "unlock", &dir]);
    let mut args = vec!["-C", root.as_str(), "worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(dir.as_str());
    let removed = check(&args);
    // Prune whatever the removal left, and whatever an earlier crash left:
    // stale `$GIT_DIR/worktrees` metadata is what makes a later `worktree add`
    // at the same path fail with "already registered".
    let _ = run(&["-C", &root, "worktree", "prune"]);
    removed.map(|_| ())
}

/// The main checkout a worktree belongs to, asked of the tree itself.
///
/// The sweep needs this: a directory left behind by a crash may have no thread
/// row and no folder row left to look the repository up in, and `git` knows
/// anyway.
pub fn repo_root_of(path: &Path) -> Option<PathBuf> {
    let dir = path.to_string_lossy().into_owned();
    let common = run(&[
        "-C",
        &dir,
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ])
    .ok()
    .and_then(|out| out.line())?;
    // `<repo>/.git` for a normal checkout; a bare repo has no working tree to
    // be the parent of, and JaBot never registers one.
    Path::new(&common).parent().map(Path::to_path_buf)
}

/// A thread id reduced to something a directory and a git ref can both be
/// named. Ids come from clients, so this is a boundary and not a nicety: a
/// `../` in a thread id would otherwise pick the directory to delete.
pub fn slug(thread_id: &str) -> String {
    let mut out = String::with_capacity(thread_id.len());
    for ch in thread_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(['-', '_'].as_slice());
    let capped: String = trimmed.chars().take(40).collect();
    if capped.is_empty() {
        "thread".to_string()
    } else {
        capped
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::check;

    /// What a file looks like at a ref, straight from git.
    ///
    /// The question every cleanup test has to be able to ask: the tree is gone,
    /// so is the work still there?
    pub fn show(repo_root: &str, refname: &str, file: &str) -> String {
        let spec = format!("{refname}:{file}");
        check(&["-C", repo_root, "show", &spec])
            .unwrap_or_else(|err| panic!("git show {spec}: {err}"))
            .stdout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::repo::git::testing;

    /// The branch checked out in a tree — what "each thread gets its own
    /// branch" means, asked of git rather than of our own bookkeeping. The
    /// production reader is the same question, so it is the same function.
    use super::head_branch as branch_of;

    /// Every path git believes is a worktree of this repository, main tree
    /// included.
    fn list(repo_root: &Path) -> Vec<PathBuf> {
        let root = repo_root.to_string_lossy().into_owned();
        let Ok(out) = run(&["-C", &root, "worktree", "list", "--porcelain"]) else {
            return Vec::new();
        };
        out.stdout
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(PathBuf::from)
            .collect()
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        testing::init_repo(dir.path(), None);
        dir
    }

    fn plan(repo: &Path, at: &Path, branch: &str) -> Plan {
        Plan {
            repo_root: repo.to_path_buf(),
            path: at.to_path_buf(),
            branch: branch.to_string(),
            base: base_commit(repo, None).expect("the repo has a commit"),
        }
    }

    #[test]
    fn a_worktree_is_its_own_checkout_on_its_own_branch() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-1");
        add(&plan(repo.path(), &at, "jabot/t-1")).expect("worktree add");

        assert!(at.join(".git").exists());
        assert_eq!(branch_of(&at).as_deref(), Some("jabot/t-1"));
        // The user's checkout is untouched: same branch, still there.
        assert_eq!(branch_of(repo.path()).as_deref(), Some("main"));
        assert_eq!(
            std::fs::canonicalize(repo_root_of(&at).unwrap()).unwrap(),
            std::fs::canonicalize(repo.path()).unwrap()
        );
    }

    #[test]
    fn two_threads_in_one_repo_get_two_trees_and_never_share_a_branch() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let first = trees.path().join("t-1");
        let second = trees.path().join("t-2");
        add(&plan(repo.path(), &first, &free_branch(repo.path(), "t"))).unwrap();
        // The same slug twice — a thread deleted and re-opened under its own id
        // is exactly this case, and git refuses a branch checked out elsewhere.
        let branch = free_branch(repo.path(), "t");
        assert_eq!(branch, "jabot/t-2");
        add(&plan(repo.path(), &second, &branch)).expect("second worktree");

        assert_eq!(branch_of(&first).as_deref(), Some("jabot/t"));
        assert_eq!(branch_of(&second).as_deref(), Some("jabot/t-2"));
        let listed = list(repo.path());
        assert_eq!(listed.len(), 3, "{listed:?}");
    }

    #[test]
    fn a_live_tree_is_locked_and_removal_unlocks_it_first() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-lock");
        add(&plan(repo.path(), &at, "jabot/t-lock")).unwrap();

        let root = repo.path().to_string_lossy().into_owned();
        let listed = check(&["-C", &root, "worktree", "list", "--porcelain"]).unwrap();
        assert!(listed.stdout.contains("locked"), "{listed:?}");

        // The lock is load-bearing in both directions: git itself refuses to
        // remove a locked tree, which is what protects a running agent — so
        // `remove` has to unlock, and this is the proof it does.
        let dir = at.to_string_lossy().into_owned();
        let refused = check(&["-C", &root, "worktree", "remove", &dir]).unwrap_err();
        assert!(
            refused.detail.to_lowercase().contains("locked"),
            "{refused}"
        );

        remove(repo.path(), &at, false).expect("remove unlocks first");
        assert!(!at.exists());
        assert_eq!(list(repo.path()).len(), 1);
    }

    #[test]
    fn uncommitted_work_becomes_a_commit_on_the_thread_branch() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-dirty");
        add(&plan(repo.path(), &at, "jabot/t-dirty")).unwrap();
        std::fs::write(at.join("notes.md"), "half-finished work").unwrap();
        assert!(is_dirty(&at));

        let saved = save_uncommitted(&at, "t-dirty")
            .expect("save")
            .expect("something was saved");
        assert_eq!(saved.branch, "jabot/t-dirty");
        assert!(!is_dirty(&at));
        remove(repo.path(), &at, false).expect("a clean tree removes without force");
        assert!(!at.exists());

        // The tree is gone and the work is not: the branch still has it, which
        // is the whole of the archive policy.
        let root = repo.path().to_string_lossy().into_owned();
        let file = check(&["-C", &root, "show", "jabot/t-dirty:notes.md"]).unwrap();
        assert_eq!(file.stdout.trim(), "half-finished work");
        let head = resolve(&root, "jabot/t-dirty").unwrap();
        assert_eq!(head, saved.sha);
    }

    /// Every branch that contains a commit, asked of git. The question a save
    /// has to be able to answer *after* the tree is gone: is anything at all
    /// still pointing at this?
    fn branches_containing(repo_root: &Path, sha: &str) -> Vec<String> {
        let root = repo_root.to_string_lossy().into_owned();
        let out = check(&[
            "-C",
            &root,
            "branch",
            "--format=%(refname:short)",
            "--contains",
            sha,
        ])
        .expect("git branch --contains");
        out.stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn work_saved_from_a_detached_head_is_still_reachable_once_the_tree_is_gone() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-detached");
        add(&plan(repo.path(), &at, "jabot/t-detached")).unwrap();
        // What an agent leaves behind after `git checkout <sha>`, `git bisect`,
        // or a rebase that stopped on a conflict. `git commit` here moves
        // nothing but the tree's own HEAD, and the removal deletes that.
        let dir = at.to_string_lossy().into_owned();
        check(&["-C", &dir, "checkout", "--detach", "HEAD"]).unwrap();
        std::fs::write(at.join("important.txt"), "an hour of work").unwrap();

        let saved = save_uncommitted(&at, "t-detached")
            .expect("save")
            .expect("something was saved");
        remove(repo.path(), &at, false).expect("remove");
        assert!(!at.exists());

        // The claim the module doc makes, checked the only way that means
        // anything: a ref still holds it, so `gc` cannot take it.
        let holders = branches_containing(repo.path(), &saved.sha);
        assert!(
            holders.contains(&saved.branch),
            "{:?} does not hold the save",
            holders
        );
        assert!(saved.branch.starts_with(BRANCH_PREFIX), "{}", saved.branch);
        let root = repo.path().to_string_lossy().into_owned();
        assert_eq!(
            super::testing::show(&root, &saved.branch, "important.txt").trim(),
            "an hour of work"
        );
    }

    #[test]
    fn a_branch_the_agent_made_does_not_become_the_branch_the_thread_reopens_on() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-own-branch");
        add(&plan(repo.path(), &at, "jabot/t-own-branch")).unwrap();
        let dir = at.to_string_lossy().into_owned();
        check(&["-C", &dir, "checkout", "-b", "feature/x"]).unwrap();
        std::fs::write(at.join("important.txt"), "on the agent's branch").unwrap();

        let saved = save_uncommitted(&at, "t-own-branch")
            .expect("save")
            .expect("something was saved");
        // Reachable, but not from anything restore is willing to check out —
        // so the save reports a `jabot/` branch of its own rather than letting
        // the row keep naming a branch that does not have the work.
        assert!(saved.branch.starts_with(BRANCH_PREFIX), "{}", saved.branch);
        assert!(branches_containing(repo.path(), &saved.sha).contains(&saved.branch));
    }

    #[test]
    fn a_second_rescue_does_not_overwrite_the_first() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-twice");
        let spawn = plan(repo.path(), &at, "jabot/t-twice");
        let base = spawn.base.clone();
        add(&spawn).unwrap();
        let dir = at.to_string_lossy().into_owned();

        check(&["-C", &dir, "checkout", "--detach", "HEAD"]).unwrap();
        std::fs::write(at.join("first.txt"), "first").unwrap();
        let first = save_uncommitted(&at, "t-twice").unwrap().unwrap();
        // Detached again, off the first save this time — a second bisect, or a
        // reopened thread the agent moved again. A rescue ref reused by name
        // would drag the first save's only holder along with it.
        check(&["-C", &dir, "checkout", "--detach", &base]).unwrap();
        std::fs::write(at.join("second.txt"), "second").unwrap();
        let second = save_uncommitted(&at, "t-twice").unwrap().unwrap();

        assert_ne!(first.branch, second.branch);
        let root = repo.path().to_string_lossy().into_owned();
        assert_eq!(
            super::testing::show(&root, &first.branch, "first.txt").trim(),
            "first"
        );
        assert_eq!(
            super::testing::show(&root, &second.branch, "second.txt").trim(),
            "second"
        );
    }

    #[test]
    fn removing_a_dirty_tree_without_force_fails_rather_than_discarding_it() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let at = trees.path().join("t-refuse");
        add(&plan(repo.path(), &at, "jabot/t-refuse")).unwrap();
        std::fs::write(at.join("notes.md"), "work").unwrap();

        let err = remove(repo.path(), &at, false).unwrap_err();
        assert!(err.detail.to_lowercase().contains("untracked"), "{err}");
        assert!(at.join("notes.md").exists());
        // Delete says the user meant it; the caller saves first, then forces.
        remove(repo.path(), &at, true).expect("force");
        assert!(!at.exists());
    }

    #[test]
    fn the_base_is_the_default_branch_when_there_is_one_and_never_a_dirty_head() {
        let repo = repo();
        testing::run(repo.path(), &["branch", "release"]);
        std::fs::write(repo.path().join("dirty.txt"), "user's own edits").unwrap();
        testing::run(repo.path(), &["add", "-A"]);
        testing::run(repo.path(), &["commit", "-m", "on main only"]);

        let release = resolve(&repo.path().to_string_lossy(), "release").unwrap();
        assert_eq!(base_commit(repo.path(), Some("release")).unwrap(), release);
        // No such branch: fall through to HEAD rather than refusing to spawn.
        assert_eq!(
            base_commit(repo.path(), Some("no-such-branch")).unwrap(),
            resolve(&repo.path().to_string_lossy(), "HEAD").unwrap()
        );
    }

    #[test]
    fn a_repository_with_no_commits_has_no_base_to_branch_from() {
        let dir = tempfile::tempdir().unwrap();
        testing::run(dir.path(), &["init", "--initial-branch=main"]);
        assert_eq!(base_commit(dir.path(), None), None);
    }

    #[test]
    fn a_thread_id_cannot_name_a_directory_outside_the_worktree_root() {
        assert_eq!(slug("../../etc"), "etc");
        assert_eq!(slug("t/../x"), "t----x");
        assert_eq!(slug("....."), "thread");
        assert_eq!(slug("t-repo"), "t-repo");
    }

    /// PR linkage asks this of a directory it has no other knowledge of, so
    /// both "not a checkout" and "detached HEAD" have to come back as "no
    /// branch" rather than as a wrong one (#28).
    #[test]
    fn a_detached_head_and_a_plain_directory_have_no_branch() {
        let repo = repo();
        assert_eq!(head_branch(repo.path()).as_deref(), Some("main"));

        let head = check(&["-C", &repo.path().to_string_lossy(), "rev-parse", "HEAD"])
            .expect("rev-parse")
            .line()
            .expect("a commit");
        check(&[
            "-C",
            &repo.path().to_string_lossy(),
            "checkout",
            "--detach",
            &head,
        ])
        .expect("detach");
        assert_eq!(head_branch(repo.path()), None);

        let plain = tempfile::tempdir().unwrap();
        assert_eq!(head_branch(plain.path()), None);
    }
}
