//! Folder registration and GitHub auth (#16).
//!
//! A folder is **one registered local directory** — almost always the root of
//! a git checkout — and not a tag, a multi-repo group, or a GitHub org
//! (`docs/research/git-and-prs/folders-and-auth.md`). "New thread in jabot-app"
//! means: spawn a session whose cwd is that repository.
//!
//! Two rules run through everything here.
//!
//! **Probe once, at registration.** Git is asked what a directory is when the
//! user registers it, and the answer is written to `folders`. A sidebar that
//! shelled out per render would spend a subprocess per folder per repaint to
//! re-learn something that changes when the user runs `git remote set-url`.
//! `folder/update { refresh: true }` is how they ask again.
//!
//! **Stamp the thread at spawn.** [`HostSession::thread_repo_record`] resolves
//! repo root, `owner/name`, forge host, branch and host id when a thread is
//! opened, and `insert_thread` writes them in the same statement as the row
//! (setup-porting §19). Nothing later infers them. A thread that has to work
//! out its own cwd after a restart works it out from wherever the app happens
//! to be running; a thread whose folder the user has since forgotten would have
//! nothing left to infer from at all.
//!
//! GitHub auth is [`gh`] — the user's existing CLI login, read on demand, never
//! persisted. No JaBot GitHub App for MVP.

pub(super) mod exec;
pub mod gh;
pub mod git;
pub mod origin;

use std::path::{Path, PathBuf};

use super::protocol::error::RpcError;
use super::protocol::methods::{
    FoldPolicy, FolderForgetResult, FolderListResult, FolderOriginView, FolderRefParams,
    FolderRegisterParams, FolderThreadView, FolderUpdateParams, FolderView, GithubStatusParams,
    GithubStatusResult,
};
use super::store::{
    FolderPatch, FolderRepoPatch, FolderRow, NewFolder, Store, StoreError, ThreadRepo,
};
use super::HostSession;
use git::RepoProbe;

impl HostSession {
    /// Every registered folder with the threads the sidebar draws under it.
    ///
    /// One call rather than a folder list plus a thread list per folder: the
    /// sidebar is drawn as a unit, and the join is the host's to do (#11's
    /// `FolderWithThreads` names exactly this shape).
    pub fn folder_list(&mut self) -> Result<FolderListResult, RpcError> {
        let store = self.repo_store()?;
        let rows = store.list_folders().map_err(internal)?;
        let mut folders = Vec::with_capacity(rows.len());
        for row in rows {
            let threads = self.folder_threads(&row.id)?;
            folders.push(folder_view(row, threads));
        }
        Ok(FolderListResult { folders })
    }

    /// Register a directory the user picked.
    ///
    /// A directory that is not a git repository is accepted rather than
    /// refused: threads work there, and only the PR surface skips it. What is
    /// refused is a *second* row for a checkout that is already registered —
    /// including a subdirectory of one — because two sidebar folders pointing
    /// at one repo would each start worktrees the other does not know about.
    pub fn folder_register(
        &mut self,
        params: FolderRegisterParams,
    ) -> Result<FolderView, RpcError> {
        let path = canonical_dir(&params.path)?;
        let path_text = path.to_string_lossy().into_owned();
        let store = self.repo_store()?;
        if let Some(existing) = store.find_folder_by_path(&path_text).map_err(internal)? {
            return Err(RpcError::FolderExists {
                folder_id: existing.id,
                path: path_text,
            });
        }
        let probe = git::probe(&path);
        if let Some(root) = probe.repo_root.as_deref() {
            if let Some(existing) = store.find_folder_by_repo_root(root).map_err(internal)? {
                return Err(RpcError::FolderExists {
                    folder_id: existing.id,
                    path: existing.path,
                });
            }
        }
        let name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            // The repository's name, not the subdirectory the user happened to
            // pick — and never the directory renamed: the display name is ours,
            // the directory is theirs.
            .unwrap_or_else(|| default_name(&probe, &path));
        let new = NewFolder {
            name,
            path: path_text,
            sort_order: store.next_folder_sort_order().map_err(internal)?,
            repo_root: probe.repo_root.clone(),
            origin_url: probe.origin_url.clone(),
            forge_host: probe.origin.as_ref().map(|o| o.host.clone()),
            repo_owner: probe.origin.as_ref().map(|o| o.owner.clone()),
            repo_name: probe.origin.as_ref().map(|o| o.name.clone()),
            default_branch: probe.default_branch.clone(),
            setup_command: trimmed(params.setup_command.as_deref()),
            files_to_copy_json: files_to_copy_json(params.files_to_copy.as_deref())?,
        };
        let row = store.insert_folder(&new).map_err(store_error)?;
        Ok(folder_view(row, Vec::new()))
    }

    /// Rename, edit the setup script and files-to-copy, or re-probe git.
    pub fn folder_update(&mut self, params: FolderUpdateParams) -> Result<FolderView, RpcError> {
        let store = self.repo_store()?;
        let row = store
            .get_folder(&params.folder_id)
            .map_err(internal)?
            .ok_or_else(|| folder_not_found(&params.folder_id))?;
        let repo = if params.refresh.unwrap_or(false) {
            // Re-probe from the registered path, not from `repo_root`: the
            // point of a refresh is to notice that the directory has become a
            // repository, or stopped being one.
            let probe = git::probe(Path::new(&row.path));
            Some(FolderRepoPatch {
                repo_root: probe.repo_root.clone(),
                origin_url: probe.origin_url.clone(),
                forge_host: probe.origin.as_ref().map(|o| o.host.clone()),
                repo_owner: probe.origin.as_ref().map(|o| o.owner.clone()),
                repo_name: probe.origin.as_ref().map(|o| o.name.clone()),
                default_branch: probe.default_branch.clone(),
            })
        } else {
            None
        };
        let patch = FolderPatch {
            name: params.name.as_deref().map(str::trim).map(str::to_string),
            // An empty string is the clear; an absent field leaves it alone.
            setup_command: params
                .setup_command
                .as_deref()
                .map(|value| trimmed(Some(value))),
            files_to_copy_json: match params.files_to_copy.as_deref() {
                Some(files) => Some(files_to_copy_json(Some(files))?),
                None => None,
            },
            repo,
        };
        let updated = store
            .update_folder(&params.folder_id, &patch)
            .map_err(store_error)?;
        let threads = self.folder_threads(&updated.id)?;
        Ok(folder_view(updated, threads))
    }

    /// Remove the sidebar row. Never the directory, and never the threads.
    pub fn folder_forget(
        &mut self,
        params: FolderRefParams,
    ) -> Result<FolderForgetResult, RpcError> {
        let store = self.repo_store()?;
        let detached_threads = store
            .delete_folder(&params.folder_id)
            .map_err(|err| match err {
                StoreError::NotFound(_) => folder_not_found(&params.folder_id),
                other => internal(other),
            })?;
        Ok(FolderForgetResult {
            folder_id: params.folder_id,
            forgotten: true,
            detached_threads,
        })
    }

    /// Whether the host can act as the user on GitHub, and as whom.
    ///
    /// Never blocks New Chat: a folder with no GitHub login still runs threads,
    /// and this is the answer the PR surface (#28) gates on, not the app.
    pub fn github_status(
        &mut self,
        params: GithubStatusParams,
    ) -> Result<GithubStatusResult, RpcError> {
        let host = params.host.as_deref().unwrap_or(gh::DEFAULT_HOST);
        let auth = gh::status(host);
        Ok(GithubStatusResult {
            installed: auth.installed,
            authenticated: auth.authenticated,
            host: auth.host,
            account: auth.account,
            detail: auth.detail,
            remedy: auth.remedy,
            gh_path: auth.path,
        })
    }

    /// The spawn record for a new thread (setup-porting §19).
    ///
    /// The folder is asked first — it was probed at registration, so this costs
    /// nothing and it is the answer the user's sidebar is already showing.
    /// Without a folder the cwd itself is probed, which is what makes a scratch
    /// thread started in a checkout still know which repository it is in. The
    /// branch always comes from the cwd: the folder's checkout and this
    /// thread's worktree are different trees on different branches (#23).
    pub(crate) fn thread_repo_record(&self, folder_id: Option<&str>, cwd: &str) -> ThreadRepo {
        let host_id = Some(self.identity.host_id.clone());
        let folder = folder_id
            .and_then(|id| self.store.as_ref()?.get_folder(id).ok().flatten())
            .filter(|row| row.repo_root.is_some());
        if let Some(folder) = folder {
            return ThreadRepo {
                repo_root: folder.repo_root.clone(),
                repo: slug(&folder),
                forge_host: folder.forge_host.clone(),
                branch: git::probe(Path::new(cwd)).branch,
                host_id,
            };
        }
        let probe = git::probe(Path::new(cwd));
        ThreadRepo {
            repo_root: probe.repo_root.clone(),
            repo: probe.origin.as_ref().map(|origin| origin.slug()),
            forge_host: probe.origin.as_ref().map(|origin| origin.host.clone()),
            branch: probe.branch.clone(),
            host_id,
        }
    }

    /// The rows the sidebar draws under one folder, each with the state of its
    /// latest run — what `ThreadSummary` (#11) needs and nothing more.
    fn folder_threads(&self, folder_id: &str) -> Result<Vec<FolderThreadView>, RpcError> {
        let store = self.repo_store()?;
        let rows = store.list_folder_threads(folder_id).map_err(internal)?;
        let mut views = Vec::with_capacity(rows.len());
        for thread in rows {
            let run_state = store
                .latest_run(&thread.id)
                .map_err(internal)?
                .map(|run| run.state);
            views.push(FolderThreadView {
                thread_id: thread.id,
                folder_id: thread.folder_id,
                bot_id: thread.bot_id,
                harness_id: thread.harness_id,
                title: thread.title,
                state: thread.state,
                fold_policy: FoldPolicy::parse(&thread.fold_policy),
                run_state,
                preview: thread.preview,
            });
        }
        Ok(views)
    }

    fn repo_store(&self) -> Result<&Store, RpcError> {
        self.store.as_ref().ok_or(RpcError::StoreUnavailable)
    }
}

fn folder_view(row: FolderRow, threads: Vec<FolderThreadView>) -> FolderView {
    let files_to_copy =
        serde_json::from_str::<Vec<String>>(&row.files_to_copy_json).unwrap_or_default();
    let origin = match (
        &row.origin_url,
        &row.forge_host,
        &row.repo_owner,
        &row.repo_name,
    ) {
        (Some(url), Some(host), Some(owner), Some(name)) => Some(FolderOriginView {
            url: url.clone(),
            host: host.clone(),
            owner: owner.clone(),
            name: name.clone(),
            repo: format!("{owner}/{name}"),
        }),
        _ => None,
    };
    FolderView {
        folder_id: row.id,
        name: row.name,
        cwd: row.repo_root.clone().unwrap_or_else(|| row.path.clone()),
        path: row.path,
        is_git: row.repo_root.is_some(),
        repo_root: row.repo_root,
        origin,
        default_branch: row.default_branch,
        setup_command: row.setup_command,
        files_to_copy,
        sort_order: row.sort_order,
        threads,
    }
}

fn slug(row: &FolderRow) -> Option<String> {
    match (&row.repo_owner, &row.repo_name) {
        (Some(owner), Some(name)) => Some(format!("{owner}/{name}")),
        _ => None,
    }
}

/// The repository's directory name when git knows one, else the directory the
/// user picked. Registering `~/code/jabot/src` names the folder `jabot`.
fn default_name(probe: &RepoProbe, path: &Path) -> String {
    let source = probe.repo_root.as_deref().map(Path::new).unwrap_or(path);
    source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_string_lossy().into_owned())
}

/// ACP refuses a relative `cwd`, so a folder's path is absolute from the moment
/// it is registered rather than at the moment a session fails to start.
/// Canonicalising also collapses `..`, symlinks and a trailing slash, which is
/// what makes "already registered" a question with one answer.
fn canonical_dir(raw: &str) -> Result<PathBuf, RpcError> {
    let expanded = expand_home(raw.trim());
    let path = std::fs::canonicalize(&expanded).map_err(|err| {
        RpcError::InvalidParams(format!("{} cannot be opened: {err}", expanded.display()))
    })?;
    if !path.is_dir() {
        return Err(RpcError::InvalidParams(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    Ok(path)
}

/// `~` is what a person types and what a dropped path from a shell often
/// carries; nothing below this line would know what to do with it.
fn expand_home(raw: &str) -> PathBuf {
    let Some(rest) = raw.strip_prefix('~') else {
        return PathBuf::from(raw);
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return PathBuf::from(raw);
    };
    match rest.strip_prefix('/') {
        Some(tail) => home.join(tail),
        None if rest.is_empty() => home,
        // `~other-user/...` is a shell expansion we do not implement; leaving it
        // alone makes it fail as a path rather than silently as the wrong one.
        None => PathBuf::from(raw),
    }
}

fn files_to_copy_json(files: Option<&[String]>) -> Result<String, RpcError> {
    let files: Vec<String> = files
        .unwrap_or(&[])
        .iter()
        .map(|file| file.trim().to_string())
        .filter(|file| !file.is_empty())
        .collect();
    serde_json::to_string(&files).map_err(|err| RpcError::Internal(err.to_string()))
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn folder_not_found(folder_id: &str) -> RpcError {
    RpcError::InvalidParams(format!("no such folder: {folder_id}"))
}

fn store_error(err: StoreError) -> RpcError {
    match err {
        StoreError::Invalid(detail) => RpcError::InvalidParams(detail),
        other => internal(other),
    }
}

fn internal(err: StoreError) -> RpcError {
    RpcError::Internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{
        FOLDER_FORGET, FOLDER_LIST, FOLDER_REGISTER, FOLDER_UPDATE, GITHUB_STATUS, HOST_HELLO,
        THREAD_OPEN,
    };
    use crate::host::HostSession;
    use serde_json::{json, Value};

    fn host() -> (HostSession, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(&dir.path().join("data"));
        session
            .handle_request(req(1, HOST_HELLO, None))
            .result
            .expect("hello");
        (session, dir)
    }

    fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(RequestId::Number(id), method, params)
    }

    fn ok(session: &mut HostSession, method: &str, params: Value) -> Value {
        let response = session.handle_request(req(7, method, Some(params)));
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        response.result.expect("result")
    }

    fn err(session: &mut HostSession, method: &str, params: Value) -> crate::host::JsonRpcError {
        session
            .handle_request(req(8, method, Some(params)))
            .error
            .unwrap_or_else(|| panic!("{method} was expected to fail"))
    }

    fn repo_at(dir: &Path, origin: Option<&str>) -> String {
        git::testing::init_repo(dir, origin);
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn registering_a_repo_records_its_origin_and_lists_it() {
        let (mut session, _dir) = host();
        let repo = tempfile::tempdir().unwrap();
        let path = repo_at(repo.path(), Some("git@github.com:jabreeflor/jabot.git"));

        let folder = ok(&mut session, FOLDER_REGISTER, json!({ "path": path }));
        assert_eq!(folder["isGit"], true);
        assert_eq!(folder["origin"]["repo"], "jabreeflor/jabot");
        assert_eq!(folder["origin"]["host"], "github.com");
        assert_eq!(folder["cwd"], folder["repoRoot"]);
        assert_eq!(folder["threads"].as_array().unwrap().len(), 0);

        let listed = ok(&mut session, FOLDER_LIST, json!({}));
        assert_eq!(listed["folders"].as_array().unwrap().len(), 1);
        assert_eq!(listed["folders"][0]["folderId"], folder["folderId"]);
    }

    #[test]
    fn a_directory_that_is_not_a_repo_is_still_a_folder() {
        let (mut session, _dir) = host();
        let plain = tempfile::tempdir().unwrap();

        let folder = ok(
            &mut session,
            FOLDER_REGISTER,
            json!({ "path": plain.path().to_string_lossy() }),
        );
        assert_eq!(folder["isGit"], false);
        assert!(folder["origin"].is_null());
        // With no repository root, threads start in the directory itself.
        assert_eq!(folder["cwd"], folder["path"]);
    }

    #[test]
    fn one_repo_is_one_folder_however_it_is_named() {
        let (mut session, _dir) = host();
        let repo = tempfile::tempdir().unwrap();
        let path = repo_at(repo.path(), None);
        let nested = repo.path().join("src");
        std::fs::create_dir_all(&nested).unwrap();

        let first = ok(&mut session, FOLDER_REGISTER, json!({ "path": path }));
        // A subdirectory of a registered checkout is the same project, and the
        // error names the folder that already has it so the UI can select it.
        let clash = err(
            &mut session,
            FOLDER_REGISTER,
            json!({ "path": nested.to_string_lossy() }),
        );
        assert_eq!(clash.code, crate::host::protocol::error::FOLDER_EXISTS);
        assert_eq!(clash.data.unwrap()["folderId"], first["folderId"]);
        assert_eq!(
            ok(&mut session, FOLDER_LIST, json!({}))["folders"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_path_that_is_not_a_directory_is_refused_before_anything_is_written() {
        let (mut session, _dir) = host();
        let missing = err(
            &mut session,
            FOLDER_REGISTER,
            json!({ "path": "/jabot-nowhere-xyz" }),
        );
        assert_eq!(missing.code, crate::host::protocol::error::INVALID_PARAMS);
        assert_eq!(
            ok(&mut session, FOLDER_LIST, json!({}))["folders"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn the_setup_script_and_files_to_copy_survive_a_rename() {
        let (mut session, _dir) = host();
        let repo = tempfile::tempdir().unwrap();
        let path = repo_at(repo.path(), None);

        let folder = ok(
            &mut session,
            FOLDER_REGISTER,
            json!({ "path": path, "setupCommand": "npm ci", "filesToCopy": [".env", "  "] }),
        );
        assert_eq!(folder["setupCommand"], "npm ci");
        // Blank entries are dropped rather than handed to #23 to copy.
        assert_eq!(folder["filesToCopy"], json!([".env"]));

        let renamed = ok(
            &mut session,
            FOLDER_UPDATE,
            json!({ "folderId": folder["folderId"], "name": "JABOT-APP" }),
        );
        assert_eq!(renamed["name"], "JABOT-APP");
        assert_eq!(renamed["setupCommand"], "npm ci");
        assert_eq!(renamed["filesToCopy"], json!([".env"]));

        // An explicit empty string is how the user clears it.
        let cleared = ok(
            &mut session,
            FOLDER_UPDATE,
            json!({ "folderId": folder["folderId"], "setupCommand": "" }),
        );
        assert!(cleared["setupCommand"].is_null());
    }

    #[test]
    fn refresh_picks_up_a_remote_added_after_registration() {
        let (mut session, _dir) = host();
        let repo = tempfile::tempdir().unwrap();
        let path = repo_at(repo.path(), None);

        let folder = ok(&mut session, FOLDER_REGISTER, json!({ "path": path }));
        assert!(folder["origin"].is_null());

        git::testing::run(
            repo.path(),
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        );
        let refreshed = ok(
            &mut session,
            FOLDER_UPDATE,
            json!({ "folderId": folder["folderId"], "refresh": true }),
        );
        assert_eq!(refreshed["origin"]["repo"], "o/r");
    }

    #[test]
    fn a_thread_is_stamped_with_its_repo_and_keeps_it_when_the_folder_goes() {
        let (mut session, _dir) = host();
        let repo = tempfile::tempdir().unwrap();
        let path = repo_at(repo.path(), Some("git@github.com:jabreeflor/jabot.git"));
        let folder = ok(&mut session, FOLDER_REGISTER, json!({ "path": path }));
        let folder_id = folder["folderId"].as_str().unwrap().to_string();

        let thread = ok(
            &mut session,
            THREAD_OPEN,
            json!({
                "threadId": "t-repo",
                "title": "Auth migration",
                "cwd": folder["cwd"],
                "harnessId": "claude",
                "folderId": folder_id,
            }),
        );
        assert_eq!(thread["repo"], "jabreeflor/jabot");
        assert_eq!(thread["forgeHost"], "github.com");
        assert_eq!(thread["repoRoot"], folder["repoRoot"]);
        assert_eq!(thread["hostId"], session.identity.host_id);
        // `repoRoot` is the user's checkout; `cwd` is the thread's own worktree
        // on its own branch (#23). The two must not be the same directory, or
        // the next thread in this folder would be editing these files.
        assert_eq!(thread["branch"], "jabot/t-repo");
        assert_eq!(thread["cwd"], thread["worktreePath"]);
        assert_ne!(thread["cwd"], folder["cwd"]);
        assert_eq!(
            std::fs::canonicalize(thread["repoRoot"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(repo.path()).unwrap()
        );

        // The folder lists the thread it now owns.
        let listed = ok(&mut session, FOLDER_LIST, json!({}));
        assert_eq!(listed["folders"][0]["threads"][0]["threadId"], "t-repo");
        assert_eq!(
            listed["folders"][0]["threads"][0]["title"],
            "Auth migration"
        );

        let forgotten = ok(
            &mut session,
            FOLDER_FORGET,
            json!({ "folderId": folder_id }),
        );
        assert_eq!(forgotten["detachedThreads"], 1);
        assert!(ok(&mut session, FOLDER_LIST, json!({}))["folders"]
            .as_array()
            .unwrap()
            .is_empty());

        // The directory is untouched, and so is what the thread knows about it.
        assert!(repo.path().join(".git").exists());
        let after = ok(
            &mut session,
            "thread/state",
            json!({ "threadId": "t-repo" }),
        );
        assert!(after["folderId"].is_null());
        assert_eq!(after["repo"], "jabreeflor/jabot");
        // Forgetting the folder does not move the thread's cwd, and does not
        // take its worktree: the spawn record is the thread's, not the row's.
        assert_eq!(after["cwd"], thread["cwd"]);
        assert_eq!(after["worktreePath"], thread["worktreePath"]);
    }

    #[test]
    fn a_thread_with_no_folder_is_stamped_from_its_own_cwd() {
        let (mut session, _dir) = host();
        let repo = tempfile::tempdir().unwrap();
        let path = repo_at(repo.path(), Some("https://gitlab.com/group/thing.git"));

        let thread = ok(
            &mut session,
            THREAD_OPEN,
            json!({
                "threadId": "t-scratch",
                "title": "Scratch",
                "cwd": path,
                "harnessId": "claude",
            }),
        );
        // Not GitHub, and that is fine: the thread runs, the PR view skips it.
        assert_eq!(thread["repo"], "group/thing");
        assert_eq!(thread["forgeHost"], "gitlab.com");
    }

    #[test]
    fn github_status_answers_without_ever_carrying_a_token() {
        let (mut session, _dir) = host();
        let status = ok(&mut session, GITHUB_STATUS, json!({}));
        assert_eq!(status["host"], "github.com");
        assert!(status["installed"].is_boolean());
        assert!(status["detail"].as_str().unwrap().len() > 4);
        // Whatever this machine's state, a report that cannot act has a remedy
        // and one that can carries no secret.
        if status["authenticated"] == json!(true) {
            assert!(status["remedy"].is_null());
        } else {
            assert!(status["remedy"].as_str().unwrap().len() > 4);
        }
        let encoded = status.to_string();
        assert!(!encoded.contains("gho_"), "{encoded}");
        assert!(!encoded.contains("token"), "{encoded}");
    }
}
