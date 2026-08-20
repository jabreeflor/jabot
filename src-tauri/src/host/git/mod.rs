//! Host-owned git worktrees: one checkout per concurrent code thread (#23).
//!
//! The prototype shows two threads running in `jabot-app` at once. Without
//! this module they would both be `cd`'d into the same directory, editing the
//! same files, running `git checkout` against each other and against the human
//! who also has the project open. Git's answer is a linked working tree — its
//! own files, index and `HEAD`, sharing one object store — and
//! `docs/research/git-and-prs/worktrees.md` settles that **JaBot** creates it:
//! not `claude --worktree`, not `codex --worktree`, not the agent. ACP already
//! demands an absolute `cwd`; isolation is a host concern for the same reason
//! process supervision is.
//!
//! Four rules.
//!
//! **Only code threads.** Decision #6: every non-code bot has one standing
//! thread whose `cwd` is its memory directory, no repo and no worktree. The
//! test here is the thread's *folder* — a thread opened in a registered folder
//! that git calls a repository is a code thread, and nothing else is. A folder
//! that is not a repo still runs threads, in the folder itself.
//!
//! **The tree is decided before the row exists.** `cwd`, `repo`, `branch` and
//! `worktree_path` are written by the same INSERT (#16, setup-porting §19), so
//! the worktree is created first and the thread is stamped with it. If the
//! insert then fails, the tree is removed again: a worktree with no row is a
//! worktree nothing will ever clean up.
//!
//! **Nothing is removed until the work in it is saved.** Archive and delete
//! both commit whatever is uncommitted onto the thread's own `jabot/<id>`
//! branch before the tree goes ([`worktree::save_uncommitted`]). JaBot never
//! deletes that branch. So the honest answer to "what happens to my
//! uncommitted changes when I archive" is: they become a commit you can check
//! out, and if that commit cannot be made, the tree is kept instead. Fold keeps
//! the tree untouched — resume needs the same `cwd`.
//!
//! **A tree left behind is a bug, so boot sweeps.** A host killed between
//! `worktree add` and the INSERT, or between archive and removal, leaves a
//! directory under our root that no thread claims. [`HostSession::sweep_worktrees`]
//! runs at startup, saves anything uncommitted in such a tree, and collects it.
//!
//! One thing to know about the cost: [`setup::apply`] runs the folder's setup
//! command synchronously, so `thread/open` on a folder configured with
//! `npm ci` takes as long as `npm ci` does. That is deliberate — an agent must
//! not start work in a half-built tree — and it is bounded by
//! [`setup::SETUP_TIMEOUT`]. Making it asynchronous means telling the renderer
//! about a "preparing" state that nothing draws yet.

pub mod setup;
pub mod worktree;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::protocol::error::RpcError;
use super::protocol::methods::ThreadOpenParams;
use super::store::FolderRow;
use super::HostSession;

/// Where every JaBot-owned tree lives, under the app data directory and
/// **outside** the user's checkout. Same placement as Codex's
/// `$CODEX_HOME/worktrees` and Conductor's `~/conductor/workspaces`: dropping
/// trees into the repo means the user has to gitignore us, and means a
/// `rm -rf` of their project silently takes our state with it.
pub const WORKTREE_DIR: &str = "worktrees";

/// The tree a thread was given, as it is about to be written onto the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadWorktree {
    pub path: PathBuf,
    pub branch: String,
    pub repo_root: PathBuf,
}

/// Why a tree is being released. The difference is what happens when git
/// refuses: archive keeps the tree rather than risk the work, delete forces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    Archived,
    Deleted,
}

impl Release {
    fn force(self) -> bool {
        matches!(self, Self::Deleted)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Archived => "archived",
            Self::Deleted => "deleted",
        }
    }
}

impl HostSession {
    /// The worktree a new thread should work in, created and set up, or `None`
    /// when this thread is not a code thread.
    ///
    /// Called from `thread/open` before the row is inserted, because the row
    /// records the answer.
    pub(crate) fn provision_worktree(
        &self,
        thread_id: &str,
        params: &ThreadOpenParams,
    ) -> Result<Option<ThreadWorktree>, RpcError> {
        // The advanced opt-out from the research: one thread at a time may work
        // in the user's own checkout. Not the default, at any concurrency —
        // sharing the main tree is the footgun this module exists to remove.
        if params.use_checkout.unwrap_or(false) {
            return Ok(None);
        }
        let Some(folder) = self.thread_folder(params.folder_id.as_deref()) else {
            return Ok(None);
        };
        let Some(repo_root) = folder.repo_root.clone().map(PathBuf::from) else {
            return Ok(None);
        };
        let Some(root) = self.worktree_root() else {
            // No data directory means no store either, so in practice nothing
            // reaches here; a host with nowhere to put a tree runs in the
            // folder rather than refusing to open the thread.
            return Ok(None);
        };
        let base = match params
            .base_ref
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
        {
            Some(refname) => Some(
                worktree::resolve(&repo_root.to_string_lossy(), refname).ok_or(
                    RpcError::WorktreeFailed {
                        thread_id: thread_id.to_string(),
                        path: None,
                        detail: format!("{refname} is not a commit in this repository"),
                    },
                )?,
            ),
            None => worktree::base_commit(&repo_root, folder.default_branch.as_deref()),
        };
        let Some(base) = base else {
            // A repository with no commits has no branch to fork and nothing to
            // collide over. The thread runs in the folder itself.
            return Ok(None);
        };

        let slug = worktree::slug(thread_id);
        let path = root.join(worktree::slug(&folder.id)).join(&slug);
        // A directory left by a crash would make `worktree add` fail with
        // "already exists"; it is ours, under our root, so collect it first.
        if path.exists() {
            self.reclaim_path(&repo_root, &path, thread_id);
        }
        let plan = worktree::Plan {
            repo_root: repo_root.clone(),
            path: path.clone(),
            branch: worktree::free_branch(&repo_root, &slug),
            base,
        };
        worktree::add(&plan).map_err(|err| RpcError::WorktreeFailed {
            thread_id: thread_id.to_string(),
            path: Some(path.to_string_lossy().into_owned()),
            detail: err.to_string(),
        })?;

        let report = setup::apply(
            &repo_root,
            &path,
            &setup::Plan {
                files: folder.files_to_copy(),
                command: folder.setup_command.clone(),
            },
            thread_id,
        );
        if !report.is_clean() {
            // Not a spawn failure: the tree exists and the thread is the user's
            // to prompt. Said out loud because "the tests fail in this thread
            // and not in my terminal" is otherwise a mystery.
            eprintln!("worktree setup for {thread_id} was incomplete: {report:?}");
        }
        Ok(Some(ThreadWorktree {
            path,
            branch: plan.branch,
            repo_root,
        }))
    }

    /// Give a tree back after archive or delete. Never called for fold.
    pub(crate) fn release_worktree(&mut self, thread_id: &str, mode: Release) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Ok(Some(row)) = store.get_thread(thread_id) else {
            return;
        };
        let Some(path) = row.worktree_path.clone().map(PathBuf::from) else {
            return;
        };
        let repo_root = row
            .repo_root
            .clone()
            .map(PathBuf::from)
            .or_else(|| worktree::repo_root_of(&path));
        match release_at(repo_root.as_deref(), &path, thread_id, mode) {
            Ok(()) => {
                // The column means "a host-owned tree exists here". Clearing it
                // is what stops the next boot's sweep, and every later reader,
                // from believing in a directory that is gone.
                if let Err(err) = store.set_thread_worktree(thread_id, None) {
                    eprintln!("failed to clear the worktree path for {thread_id}: {err}");
                }
            }
            Err(detail) => eprintln!(
                "kept the worktree at {} for {thread_id} ({}): {detail}",
                path.display(),
                mode.as_str()
            ),
        }
    }

    /// Put an archived thread's worktree back where its `cwd` says it is.
    ///
    /// `archived → active` is a legal transition (the state machine keeps the
    /// edge for #21), and archive removed the tree — so without this the thread
    /// comes back pointing an adapter at a directory that is not there. The
    /// path and the branch both come off the row, so nothing is inferred: this
    /// re-creates what archive took away, on the branch archive committed to.
    pub(crate) fn restore_worktree(&mut self, thread_id: &str) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let Ok(Some(row)) = store.get_thread(thread_id) else {
            return;
        };
        // A thread whose tree is still there — every fold, and every ordinary
        // reopen — has nothing to restore.
        if row.worktree_path.is_some() {
            return;
        }
        let (Some(root), Some(repo_root), Some(branch)) = (
            self.worktree_root(),
            row.repo_root.clone(),
            row.branch.clone(),
        ) else {
            return;
        };
        let path = PathBuf::from(&row.cwd);
        // Three guards, and each one is the difference between restoring a tree
        // and creating something nobody asked for: the cwd has to be one of
        // ours, it has to be missing, and the branch has to be the one we mint.
        if !path.starts_with(&root) || path.exists() || !branch.starts_with(worktree::BRANCH_PREFIX)
        {
            return;
        }
        let repo_root = PathBuf::from(repo_root);
        if let Err(err) = worktree::restore(&repo_root, &path, &branch) {
            eprintln!("could not restore the worktree for {thread_id}: {err}");
            return;
        }
        // A restored tree is as bare as a new one: tracked files only.
        let plan = self
            .thread_folder(row.folder_id.as_deref())
            .map(|folder| setup::Plan {
                files: folder.files_to_copy(),
                command: folder.setup_command.clone(),
            })
            .unwrap_or_default();
        let report = setup::apply(&repo_root, &path, &plan, thread_id);
        if !report.is_clean() {
            eprintln!("worktree setup for {thread_id} was incomplete: {report:?}");
        }
        if let Some(store) = self.store.as_ref() {
            if let Err(err) = store.set_thread_worktree(thread_id, Some(&row.cwd)) {
                eprintln!("failed to record the restored worktree for {thread_id}: {err}");
            }
        }
    }

    /// Undo a `provision_worktree` whose thread never made it into the store.
    pub(crate) fn discard_worktree(&self, tree: &ThreadWorktree, thread_id: &str) {
        if let Err(detail) = release_at(
            Some(&tree.repo_root),
            &tree.path,
            thread_id,
            Release::Deleted,
        ) {
            eprintln!(
                "failed to remove the orphaned worktree at {}: {detail}",
                tree.path.display()
            );
        }
    }

    /// Collect every tree under our root that no live thread claims.
    ///
    /// Runs at startup, next to the ledger reconciliation, because the case it
    /// exists for is a host that was killed: between `worktree add` and the
    /// INSERT, or between the archive and the removal. Anything uncommitted in
    /// such a tree is committed to its branch first — a sweep must not be the
    /// thing that loses work either.
    pub(crate) fn sweep_worktrees(&mut self) {
        let Some(root) = self.worktree_root() else {
            return;
        };
        if !root.is_dir() {
            return;
        }
        let live = self.live_worktree_paths();
        for folder_dir in sub_dirs(&root) {
            for tree in sub_dirs(&folder_dir) {
                if live.contains(&canonical(&tree)) {
                    continue;
                }
                let thread_id = tree
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let repo_root = worktree::repo_root_of(&tree);
                match release_at(repo_root.as_deref(), &tree, &thread_id, Release::Archived) {
                    Ok(()) => eprintln!("swept an unclaimed worktree at {}", tree.display()),
                    Err(detail) => {
                        eprintln!(
                            "could not sweep the worktree at {}: {detail}",
                            tree.display()
                        )
                    }
                }
            }
            // Only succeeds when the folder has no trees left, which is exactly
            // when it should go.
            let _ = std::fs::remove_dir(&folder_dir);
        }
    }

    /// Every path a thread still claims. Archived and deleted rows are absent
    /// on purpose: their trees are the sweep's to collect if a release failed.
    fn live_worktree_paths(&self) -> HashSet<PathBuf> {
        let Some(store) = self.store.as_ref() else {
            return HashSet::new();
        };
        match store.list_worktree_threads() {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|row| row.worktree_path)
                .map(|path| canonical(Path::new(&path)))
                .collect(),
            Err(err) => {
                // A sweep that cannot read the store must not decide every tree
                // is an orphan.
                eprintln!("skipping the worktree sweep: {err}");
                HashSet::new()
            }
        }
    }

    fn reclaim_path(&self, repo_root: &Path, path: &Path, thread_id: &str) {
        if let Err(detail) = release_at(Some(repo_root), path, thread_id, Release::Archived) {
            eprintln!(
                "a directory is already at {} and could not be collected: {detail}",
                path.display()
            );
        }
    }

    fn thread_folder(&self, folder_id: Option<&str>) -> Option<FolderRow> {
        self.store.as_ref()?.get_folder(folder_id?).ok().flatten()
    }

    pub(crate) fn worktree_root(&self) -> Option<PathBuf> {
        Some(self.data_dir.as_ref()?.join(WORKTREE_DIR))
    }
}

/// Save, unlock, remove, prune — the whole cleanup, without a `HostSession`.
///
/// `Err` means the tree is still there, and the caller must keep believing in
/// it. That is the safe direction: a retained tree costs disk and shows up in
/// the next sweep, while a forgotten one is a leak nothing will ever collect.
fn release_at(
    repo_root: Option<&Path>,
    path: &Path,
    thread_id: &str,
    mode: Release,
) -> Result<(), String> {
    if !path.exists() {
        // Already gone — the user deleted it by hand, most likely. Prune the
        // stale `$GIT_DIR/worktrees` metadata so a later `add` at the same path
        // is not refused, and call it done.
        if let Some(root) = repo_root {
            let _ = worktree::remove(root, path, true);
        }
        return Ok(());
    }
    let Some(root) = repo_root else {
        return Err("the repository this worktree belongs to could not be found".to_string());
    };
    match worktree::save_uncommitted(path, thread_id) {
        Ok(Some(sha)) => eprintln!("saved uncommitted work in {} as {sha}", path.display()),
        Ok(None) => {}
        Err(err) => {
            if !mode.force() {
                // Archive is not a destructive gesture. If the work cannot be
                // committed, the tree stays and the user still has it.
                return Err(format!("uncommitted work could not be saved: {err}"));
            }
            eprintln!(
                "deleting {} with work that could not be saved: {err}",
                path.display()
            );
        }
    }
    worktree::remove(root, path, mode.force()).map_err(|err| err.to_string())?;
    // `git worktree remove` takes the directory with it; anything left is a
    // file git did not consider its own.
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
    Ok(())
}

fn sub_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{
        FOLDER_REGISTER, HOST_HELLO, THREAD_ARCHIVE, THREAD_DELETE, THREAD_FOLD, THREAD_OPEN,
        THREAD_REOPEN, THREAD_STATE,
    };
    use crate::host::repo::git::testing;
    use serde_json::{json, Value};

    struct Fixture {
        session: HostSession,
        /// The user's checkout.
        repo: tempfile::TempDir,
        /// The app data directory the worktree root lives under.
        _data: tempfile::TempDir,
        data_dir: PathBuf,
        folder_id: String,
    }

    impl Fixture {
        fn open(&mut self, thread_id: &str, extra: Value) -> Value {
            let mut params = json!({
                "threadId": thread_id,
                "title": "Auth migration",
                "cwd": self.repo.path().to_string_lossy(),
                "harnessId": "claude",
                "folderId": self.folder_id,
            });
            merge(&mut params, extra);
            ok(&mut self.session, THREAD_OPEN, params)
        }

        fn state(&mut self, thread_id: &str) -> Value {
            ok(
                &mut self.session,
                THREAD_STATE,
                json!({ "threadId": thread_id }),
            )
        }

        /// Reopen the host over the same data directory — a quit and relaunch,
        /// which is what makes the boot sweep observable.
        fn relaunch(&mut self) {
            self.session = HostSession::load(&self.data_dir);
            self.session
                .handle_request(req(1, HOST_HELLO, None))
                .result
                .expect("hello");
        }
    }

    fn merge(into: &mut Value, extra: Value) {
        let (Some(target), Some(source)) = (into.as_object_mut(), extra.as_object()) else {
            return;
        };
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }

    fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(RequestId::Number(id), method, params)
    }

    fn ok(session: &mut HostSession, method: &str, params: Value) -> Value {
        let response = session.handle_request(req(7, method, Some(params)));
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        response.result.expect("result")
    }

    /// A host with one registered repository, ready to open threads in.
    fn fixture(setup_command: Option<&str>, files_to_copy: Value) -> Fixture {
        let data = tempfile::tempdir().unwrap();
        let data_dir = data.path().join("data");
        let mut session = HostSession::load(&data_dir);
        session
            .handle_request(req(1, HOST_HELLO, None))
            .result
            .expect("hello");
        let repo = tempfile::tempdir().unwrap();
        testing::init_repo(repo.path(), Some("git@github.com:jabreeflor/jabot.git"));
        let folder = ok(
            &mut session,
            FOLDER_REGISTER,
            json!({
                "path": repo.path().to_string_lossy(),
                "setupCommand": setup_command,
                "filesToCopy": files_to_copy,
            }),
        );
        let folder_id = folder["folderId"].as_str().unwrap().to_string();
        Fixture {
            session,
            repo,
            _data: data,
            data_dir,
            folder_id,
        }
    }

    fn path_of(value: &Value, key: &str) -> PathBuf {
        PathBuf::from(
            value[key]
                .as_str()
                .unwrap_or_else(|| panic!("{key} is set")),
        )
    }

    #[test]
    fn two_threads_in_one_folder_never_share_a_directory_or_a_branch() {
        let mut fx = fixture(None, json!([]));
        let first = fx.open("t-one", json!({}));
        let second = fx.open("t-two", json!({}));

        let one = path_of(&first, "worktreePath");
        let two = path_of(&second, "worktreePath");
        assert_ne!(one, two);
        assert_eq!(first["cwd"].as_str().unwrap(), one.to_string_lossy());
        assert_eq!(second["cwd"].as_str().unwrap(), two.to_string_lossy());
        assert_ne!(first["branch"], second["branch"]);
        // Neither of them is the user's checkout, which is the whole point:
        // the prototype shows two threads live in one folder at once.
        assert!(one.starts_with(fx.data_dir.join(WORKTREE_DIR)));
        assert!(two.starts_with(fx.data_dir.join(WORKTREE_DIR)));
        assert_ne!(one.as_path(), fx.repo.path());
        // Both trees are real checkouts of the same repository.
        assert!(one.join(".git").exists() && two.join(".git").exists());
        assert_eq!(first["repoRoot"], second["repoRoot"]);
    }

    #[test]
    fn only_code_threads_get_a_worktree() {
        let mut fx = fixture(None, json!([]));

        // A bot's standing thread: no folder, so no repo and no tree (#6).
        let standing = ok(
            &mut fx.session,
            THREAD_OPEN,
            json!({
                "threadId": "t-chief",
                "title": "Chief",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "harnessId": "claude",
            }),
        );
        assert!(standing["worktreePath"].is_null());

        // A folder that is not a checkout still runs threads, in itself.
        let plain = tempfile::tempdir().unwrap();
        let folder = ok(
            &mut fx.session,
            FOLDER_REGISTER,
            json!({ "path": plain.path().to_string_lossy() }),
        );
        let notes = ok(
            &mut fx.session,
            THREAD_OPEN,
            json!({
                "threadId": "t-notes",
                "title": "Notes",
                "cwd": folder["cwd"],
                "harnessId": "claude",
                "folderId": folder["folderId"],
            }),
        );
        assert!(notes["worktreePath"].is_null());
        assert_eq!(notes["cwd"], folder["cwd"]);

        // And the advanced opt-out: work in my own checkout.
        let shared = fx.open("t-shared", json!({ "useCheckout": true }));
        assert!(shared["worktreePath"].is_null());
        assert_eq!(
            std::fs::canonicalize(shared["cwd"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(fx.repo.path()).unwrap()
        );
    }

    #[test]
    fn a_fresh_tree_gets_the_ignored_files_and_the_setup_command() {
        let mut fx = fixture(Some("echo ready > setup-ran.txt"), json!([".env"]));
        std::fs::write(fx.repo.path().join(".env"), "TOKEN=1").unwrap();

        let thread = fx.open("t-setup", json!({}));
        let tree = path_of(&thread, "worktreePath");
        // Neither file is tracked, so `git worktree add` alone leaves both out
        // and the agent's first command fails for the wrong reason.
        assert_eq!(
            std::fs::read_to_string(tree.join(".env")).unwrap(),
            "TOKEN=1"
        );
        assert_eq!(
            std::fs::read_to_string(tree.join("setup-ran.txt"))
                .unwrap()
                .trim(),
            "ready"
        );
    }

    #[test]
    fn folding_keeps_the_tree_because_resume_needs_the_same_cwd() {
        let mut fx = fixture(None, json!([]));
        let thread = fx.open("t-fold", json!({}));
        let tree = path_of(&thread, "worktreePath");

        let folded = ok(
            &mut fx.session,
            THREAD_FOLD,
            json!({ "threadId": "t-fold" }),
        );
        assert_eq!(folded["state"], "folded");
        assert!(tree.exists());
        assert_eq!(path_of(&fx.state("t-fold"), "worktreePath"), tree);
    }

    #[test]
    fn archiving_saves_uncommitted_work_to_the_branch_before_removing_the_tree() {
        let mut fx = fixture(None, json!([]));
        let thread = fx.open("t-archive", json!({}));
        let tree = path_of(&thread, "worktreePath");
        let branch = thread["branch"].as_str().unwrap().to_string();
        std::fs::write(tree.join("half-done.rs"), "fn main() {}").unwrap();

        let archived = ok(
            &mut fx.session,
            THREAD_ARCHIVE,
            json!({ "threadId": "t-archive" }),
        );
        assert_eq!(archived["state"], "archived");
        // The tree is gone — a worktree left behind is a bug — and the row no
        // longer claims one.
        assert!(!tree.exists());
        assert!(archived["worktreePath"].is_null());
        // And the work is not gone: it is a commit on the thread's own branch.
        let root = fx.repo.path().to_string_lossy().into_owned();
        let show = worktree::testing::show(&root, &branch, "half-done.rs");
        assert_eq!(show.trim(), "fn main() {}");
    }

    #[test]
    fn reopening_an_archived_thread_puts_its_tree_back_with_the_work_in_it() {
        let mut fx = fixture(None, json!([]));
        let thread = fx.open("t-back", json!({}));
        let tree = path_of(&thread, "worktreePath");
        std::fs::write(tree.join("half-done.rs"), "fn main() {}").unwrap();
        ok(
            &mut fx.session,
            THREAD_ARCHIVE,
            json!({ "threadId": "t-back" }),
        );
        assert!(!tree.exists());

        let reopened = ok(
            &mut fx.session,
            THREAD_REOPEN,
            json!({ "threadId": "t-back" }),
        );
        // `archived → active` is a legal move, so the cwd this thread has been
        // carrying since it was opened has to be a directory again.
        assert_eq!(reopened["state"], "active");
        assert_eq!(path_of(&reopened, "worktreePath"), tree);
        assert_eq!(reopened["cwd"].as_str().unwrap(), tree.to_string_lossy());
        assert_eq!(
            std::fs::read_to_string(tree.join("half-done.rs")).unwrap(),
            "fn main() {}"
        );
    }

    #[test]
    fn deleting_removes_the_tree_and_still_keeps_the_branch() {
        let mut fx = fixture(None, json!([]));
        let thread = fx.open("t-gone", json!({}));
        let tree = path_of(&thread, "worktreePath");
        let branch = thread["branch"].as_str().unwrap().to_string();
        std::fs::write(tree.join("scratch.txt"), "unpushed").unwrap();

        let deleted = ok(
            &mut fx.session,
            THREAD_DELETE,
            json!({ "threadId": "t-gone" }),
        );
        assert!(deleted["deletedAt"].is_string());
        assert!(!tree.exists());
        // Delete is the user saying they meant it, and the tree goes without a
        // confirmation the host has no way to ask for. The branch stays: it
        // costs nothing and it is the only copy of anything never pushed.
        let root = fx.repo.path().to_string_lossy().into_owned();
        let show = worktree::testing::show(&root, &branch, "scratch.txt");
        assert_eq!(show.trim(), "unpushed");
    }

    #[test]
    fn a_tree_left_by_a_crash_is_collected_on_the_next_boot() {
        let mut fx = fixture(None, json!([]));
        let thread = fx.open("t-crash", json!({}));
        let tree = path_of(&thread, "worktreePath");
        let branch = thread["branch"].as_str().unwrap().to_string();

        // A host killed between the archive and the removal: the row says
        // archived, the directory is still on disk.
        ok(
            &mut fx.session,
            THREAD_DELETE,
            json!({ "threadId": "t-crash" }),
        );
        let plan = worktree::Plan {
            repo_root: fx.repo.path().to_path_buf(),
            path: tree.clone(),
            branch: format!("{branch}-orphan"),
            base: worktree::base_commit(fx.repo.path(), None).unwrap(),
        };
        worktree::add(&plan).expect("re-create the leftover directory");
        std::fs::write(tree.join("unsaved.txt"), "mid-flight").unwrap();
        assert!(tree.exists());

        fx.relaunch();

        // No thread claims it, so boot collects it — after committing what was
        // in it, because a sweep must not be the thing that loses work either.
        assert!(!tree.exists(), "the orphan tree survived the sweep");
        let root = fx.repo.path().to_string_lossy().into_owned();
        let show = worktree::testing::show(&root, &plan.branch, "unsaved.txt");
        assert_eq!(show.trim(), "mid-flight");
    }

    #[test]
    fn a_live_threads_tree_survives_every_boot() {
        let mut fx = fixture(None, json!([]));
        let active = path_of(&fx.open("t-active", json!({})), "worktreePath");
        let folded = path_of(&fx.open("t-folded", json!({})), "worktreePath");
        ok(
            &mut fx.session,
            THREAD_FOLD,
            json!({ "threadId": "t-folded" }),
        );

        fx.relaunch();
        fx.relaunch();

        // Folded is the state that means "disappeared and still working"; a
        // sweep that collected it would delete a running agent's checkout.
        assert!(active.exists());
        assert!(folded.exists());
        assert_eq!(path_of(&fx.state("t-folded"), "worktreePath"), folded);
    }

    #[test]
    fn a_named_base_ref_that_does_not_exist_refuses_the_spawn() {
        let mut fx = fixture(None, json!([]));
        let response = fx.session.handle_request(req(
            9,
            THREAD_OPEN,
            Some(json!({
                "threadId": "t-bad-base",
                "title": "Auth migration",
                "cwd": fx.repo.path().to_string_lossy(),
                "harnessId": "claude",
                "folderId": fx.folder_id,
                "baseRef": "origin/does-not-exist",
            })),
        ));
        let err = response.error.expect("a bad base ref is refused");
        assert_eq!(err.code, crate::host::protocol::error::WORKTREE_FAILED);
        // Nothing half-made: no row, and no directory under the worktree root.
        assert!(fx
            .session
            .handle_request(req(
                10,
                THREAD_STATE,
                Some(json!({ "threadId": "t-bad-base" }))
            ))
            .error
            .is_some());
        assert!(!fx
            .data_dir
            .join(WORKTREE_DIR)
            .join(worktree::slug(&fx.folder_id))
            .join("t-bad-base")
            .exists());
    }
}
