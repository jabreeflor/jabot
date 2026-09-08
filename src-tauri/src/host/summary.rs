//! Code conversation summary: repositories, Git state, and sources (#269).
//!
//! The transcript does not know which checkout the agent is editing, what is
//! dirty, or which files the person attached. Those facts live on the thread
//! row, extra `thread_repos` attachments, and live git — so the panel asks
//! here rather than reconstructing them from chat.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::git::status;
use super::protocol::error::RpcError;
use super::protocol::methods::{
    ThreadGitCommitParams, ThreadGitDiffResult, ThreadGitFile, ThreadGitParams,
    ThreadGitPushResult, ThreadRepoChoice, ThreadRepoParams, ThreadRepoSummary,
    ThreadSourceAddParams, ThreadSourceOpenResult, ThreadSourceRefParams, ThreadSourceView,
    ThreadSummaryResult,
};
use super::store::{FolderRow, StoreError, ThreadRow};
use super::HostSession;

impl HostSession {
    pub fn thread_summary(&mut self, thread_id: &str) -> Result<ThreadSummaryResult, RpcError> {
        let row = self
            .lifecycle_thread(thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(thread_id.to_string()))?;
        let store = self.store_or_err()?;
        let extra = store
            .list_thread_repos(thread_id)
            .map_err(summary_store_error)?;
        let sources = store
            .list_thread_sources(thread_id)
            .map_err(summary_store_error)?;
        let folders = store.list_folders().map_err(summary_store_error)?;
        let prs = super::pr::thread_prs(store, thread_id);

        let mut repositories = Vec::new();
        if let Some(primary) = primary_repo(&row, &folders) {
            repositories.push(primary);
        }
        for link in extra {
            if row.folder_id.as_deref() == Some(link.folder_id.as_str()) {
                continue;
            }
            let Some(folder) = folders.iter().find(|f| f.id == link.folder_id) else {
                continue;
            };
            repositories.push(repo_from_folder(folder, false, None, &prs));
        }

        let selected_repo_id = repositories
            .iter()
            .find(|repo| repo.primary)
            .or_else(|| repositories.first())
            .map(|repo| repo.id.clone())
            .unwrap_or_default();

        let attached: Vec<&str> = repositories.iter().map(|r| r.id.as_str()).collect();
        let available_folders = folders
            .iter()
            .filter(|folder| !attached.contains(&folder.id.as_str()))
            .map(|folder| ThreadRepoChoice {
                folder_id: folder.id.clone(),
                name: folder.name.clone(),
                path: folder.path.clone(),
                is_git: folder.repo_root.is_some(),
            })
            .collect();

        Ok(ThreadSummaryResult {
            thread_id: thread_id.to_string(),
            selected_repo_id,
            repositories,
            sources: sources
                .into_iter()
                .map(|row| ThreadSourceView {
                    available: Path::new(&row.path).exists(),
                    id: row.id,
                    name: row.name,
                    kind: row.kind,
                    path: row.path,
                    mime: row.mime,
                })
                .collect(),
            available_folders,
        })
    }

    pub fn thread_repo_attach(
        &mut self,
        params: ThreadRepoParams,
    ) -> Result<ThreadSummaryResult, RpcError> {
        let row = self
            .lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        if row.folder_id.as_deref() == Some(params.folder_id.as_str()) {
            return Err(RpcError::InvalidParams(
                "that folder is already this conversation's primary repository".into(),
            ));
        }
        let store = self.store_or_err()?;
        store
            .get_folder(&params.folder_id)
            .map_err(summary_store_error)?
            .ok_or_else(|| RpcError::InvalidParams("no such folder".into()))?;
        store
            .attach_thread_repo(&params.thread_id, &params.folder_id)
            .map_err(summary_store_error)?;
        self.thread_summary(&params.thread_id)
    }

    pub fn thread_repo_detach(
        &mut self,
        params: ThreadRepoParams,
    ) -> Result<ThreadSummaryResult, RpcError> {
        self.lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        self.store_or_err()?
            .detach_thread_repo(&params.thread_id, &params.folder_id)
            .map_err(summary_store_error)?;
        self.thread_summary(&params.thread_id)
    }

    pub fn thread_source_add(
        &mut self,
        params: ThreadSourceAddParams,
    ) -> Result<ThreadSummaryResult, RpcError> {
        self.lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        let path = PathBuf::from(&params.path);
        if !path.is_absolute() {
            return Err(RpcError::InvalidParams("path must be absolute".into()));
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| params.path.clone());
        let kind = if is_image(&name) { "image" } else { "file" };
        let mime = mime_for(&name);
        self.store_or_err()?
            .add_thread_source(
                &params.thread_id,
                &name,
                kind,
                &params.path,
                mime.as_deref(),
            )
            .map_err(summary_store_error)?;
        self.thread_summary(&params.thread_id)
    }

    pub fn thread_source_remove(
        &mut self,
        params: ThreadSourceRefParams,
    ) -> Result<ThreadSummaryResult, RpcError> {
        self.lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        let source = self
            .store_or_err()?
            .get_thread_source(&params.source_id)
            .map_err(summary_store_error)?
            .ok_or_else(|| RpcError::InvalidParams("no such source".into()))?;
        if source.thread_id != params.thread_id {
            return Err(RpcError::InvalidParams(
                "that source does not belong to this conversation".into(),
            ));
        }
        self.store_or_err()?
            .remove_thread_source(&params.source_id)
            .map_err(summary_store_error)?;
        self.thread_summary(&params.thread_id)
    }

    pub fn thread_source_open(
        &mut self,
        params: ThreadSourceRefParams,
    ) -> Result<ThreadSourceOpenResult, RpcError> {
        self.lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        let source = self
            .store_or_err()?
            .get_thread_source(&params.source_id)
            .map_err(summary_store_error)?
            .ok_or_else(|| RpcError::InvalidParams("no such source".into()))?;
        if source.thread_id != params.thread_id {
            return Err(RpcError::InvalidParams(
                "that source does not belong to this conversation".into(),
            ));
        }
        let path = PathBuf::from(&source.path);
        if !path.exists() {
            return Err(RpcError::InvalidParams(
                "that file is no longer on disk".into(),
            ));
        }
        Ok(ThreadSourceOpenResult {
            opened: open_path(&path),
        })
    }

    pub fn thread_git_diff(
        &mut self,
        params: ThreadGitParams,
    ) -> Result<ThreadGitDiffResult, RpcError> {
        let summary = self.thread_summary(&params.thread_id)?;
        let repo = named_repo(&summary, params.repo_id.as_deref())?;
        let Some(path) = repo.path.as_deref().filter(|p| Path::new(p).exists()) else {
            return Err(RpcError::InvalidParams(
                "that repository is not available on disk".into(),
            ));
        };
        if !repo.is_git {
            return Err(RpcError::InvalidParams(
                "that folder is not a Git repository".into(),
            ));
        }
        let root = Path::new(path);
        let base = status::compare_base(root, repo.default_branch.as_deref());
        let files = status::changed_files(root, base.as_deref()).map_err(git_error)?;
        let patch = status::unified_diff(root, base.as_deref()).ok();
        Ok(ThreadGitDiffResult {
            repo_id: repo.id.clone(),
            additions: files.iter().map(|f| f.additions).sum(),
            deletions: files.iter().map(|f| f.deletions).sum(),
            files: files
                .into_iter()
                .map(|file| ThreadGitFile {
                    path: file.path,
                    status: file.status,
                    additions: file.additions,
                    deletions: file.deletions,
                })
                .collect(),
            patch,
            compare_url: repo.compare_url.clone(),
        })
    }

    pub fn thread_git_commit(
        &mut self,
        params: ThreadGitCommitParams,
    ) -> Result<ThreadSummaryResult, RpcError> {
        let summary = self.thread_summary(&params.thread_id)?;
        let repo = named_repo(&summary, params.repo_id.as_deref())?;
        let path = git_path(repo)?;
        status::commit_all(path, params.message.trim()).map_err(git_error)?;
        self.thread_summary(&params.thread_id)
    }

    pub fn thread_git_push(
        &mut self,
        params: ThreadGitParams,
    ) -> Result<ThreadGitPushResult, RpcError> {
        let summary = self.thread_summary(&params.thread_id)?;
        let repo = named_repo(&summary, params.repo_id.as_deref())?;
        let path = git_path(repo)?;
        match status::push_head(path) {
            Ok(detail) => Ok(ThreadGitPushResult {
                ok: true,
                detail: (!detail.is_empty()).then_some(detail),
                compare_url: repo.compare_url.clone(),
            }),
            Err(err) => Ok(ThreadGitPushResult {
                ok: false,
                detail: Some(err.to_string()),
                compare_url: repo.compare_url.clone(),
            }),
        }
    }
}

fn primary_repo(row: &ThreadRow, folders: &[FolderRow]) -> Option<ThreadRepoSummary> {
    let folder = row
        .folder_id
        .as_deref()
        .and_then(|id| folders.iter().find(|f| f.id == id));
    let checkout = row
        .worktree_path
        .clone()
        .or_else(|| (!row.cwd.is_empty()).then(|| row.cwd.clone()))
        .map(PathBuf::from);
    let path = checkout.filter(|p| !p.as_os_str().is_empty());
    if folder.is_none() && path.is_none() {
        return None;
    }
    let name = folder
        .map(|f| f.name.clone())
        .or_else(|| {
            row.repo
                .as_deref()
                .and_then(|slug| slug.rsplit('/').next())
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            path.as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Repository".into());
    let id = folder
        .map(|f| f.id.clone())
        .unwrap_or_else(|| "primary".into());
    let default_branch = folder.and_then(|f| f.default_branch.clone());
    Some(probe_repo(
        ThreadRepoSummary {
            id,
            name,
            primary: true,
            environment: "Local".into(),
            is_git: false,
            available: path.as_ref().is_some_and(|p| p.exists()),
            status: "empty".into(),
            branch: row.branch.clone(),
            additions: None,
            deletions: None,
            path: path.map(|p| p.to_string_lossy().into_owned()),
            repo: row.repo.clone().or_else(|| {
                folder.and_then(|f| match (&f.repo_owner, &f.repo_name) {
                    (Some(owner), Some(name)) => Some(format!("{owner}/{name}")),
                    _ => None,
                })
            }),
            forge_host: row
                .forge_host
                .clone()
                .or_else(|| folder.and_then(|f| f.forge_host.clone())),
            default_branch,
            compare_url: None,
            pull_request_url: None,
            pull_request_number: None,
        },
        &[],
    ))
}

fn repo_from_folder(
    folder: &FolderRow,
    primary: bool,
    override_path: Option<String>,
    prs: &[super::protocol::methods::PullRequestView],
) -> ThreadRepoSummary {
    let path = override_path.unwrap_or_else(|| folder.path.clone());
    let slug = match (&folder.repo_owner, &folder.repo_name) {
        (Some(owner), Some(name)) => Some(format!("{owner}/{name}")),
        _ => None,
    };
    probe_repo(
        ThreadRepoSummary {
            id: folder.id.clone(),
            name: folder.name.clone(),
            primary,
            environment: "Local".into(),
            is_git: folder.repo_root.is_some(),
            available: Path::new(&path).exists(),
            status: "empty".into(),
            branch: None,
            additions: None,
            deletions: None,
            path: Some(path),
            repo: slug,
            forge_host: folder.forge_host.clone(),
            default_branch: folder.default_branch.clone(),
            compare_url: None,
            pull_request_url: None,
            pull_request_number: None,
        },
        prs,
    )
}

fn probe_repo(
    mut repo: ThreadRepoSummary,
    prs: &[super::protocol::methods::PullRequestView],
) -> ThreadRepoSummary {
    let Some(path) = repo.path.clone() else {
        repo.status = "unavailable".into();
        return repo;
    };
    let root = Path::new(&path);
    if !root.exists() {
        repo.available = false;
        repo.status = "unavailable".into();
        return repo;
    }
    if !status::is_repository(root) {
        repo.is_git = false;
        repo.status = "not_git".into();
        return repo;
    }
    repo.is_git = true;
    repo.available = true;
    if let Some(branch) = status::current_branch(root) {
        repo.branch = Some(branch);
    }
    let base = status::compare_base(root, repo.default_branch.as_deref());
    match status::diffstat(root, base.as_deref()) {
        Ok(stat) => {
            repo.additions = Some(stat.additions);
            repo.deletions = Some(stat.deletions);
            repo.status = "ok".into();
        }
        Err(_) => {
            repo.status = "unavailable".into();
        }
    }
    if let (Some(host), Some(slug), Some(branch), Some(base_branch)) = (
        repo.forge_host.as_deref(),
        repo.repo.as_deref(),
        repo.branch.as_deref(),
        repo.default_branch.as_deref(),
    ) {
        if branch != base_branch {
            repo.compare_url = Some(format!(
                "https://{host}/{slug}/compare/{base_branch}...{branch}"
            ));
        }
    }
    if let Some(pr) = repo
        .repo
        .as_deref()
        .and_then(|slug| prs.iter().find(|pr| pr.repo == slug))
        .or_else(|| prs.first().filter(|_| repo.primary))
    {
        repo.pull_request_url = Some(pr.url.clone());
        repo.pull_request_number = Some(pr.number);
    }
    repo
}

fn named_repo<'a>(
    summary: &'a ThreadSummaryResult,
    repo_id: Option<&str>,
) -> Result<&'a ThreadRepoSummary, RpcError> {
    let id = repo_id
        .filter(|id| !id.is_empty())
        .unwrap_or(summary.selected_repo_id.as_str());
    summary
        .repositories
        .iter()
        .find(|repo| repo.id == id)
        .ok_or_else(|| RpcError::InvalidParams("no such repository on this conversation".into()))
}

fn git_path(repo: &ThreadRepoSummary) -> Result<&Path, RpcError> {
    if !repo.is_git {
        return Err(RpcError::InvalidParams(
            "that folder is not a Git repository".into(),
        ));
    }
    let path = repo.path.as_deref().ok_or_else(|| {
        RpcError::InvalidParams("that repository has no checkout to act on".into())
    })?;
    let root = Path::new(path);
    if !root.exists() {
        return Err(RpcError::InvalidParams(
            "that repository is not available on disk".into(),
        ));
    }
    Ok(root)
}

fn is_image(name: &str) -> bool {
    matches!(
        Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "avif")
    )
}

fn mime_for(name: &str) -> Option<String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())?;
    Some(
        match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "md" => "text/markdown",
            "json" => "application/json",
            "ts" | "tsx" | "js" | "jsx" => "text/plain",
            _ => return None,
        }
        .into(),
    )
}

fn open_path(path: &Path) -> bool {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        Command::new("explorer")
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_error(err: super::git::worktree::GitFailure) -> RpcError {
    RpcError::Internal(err.to_string())
}

fn summary_store_error(err: StoreError) -> RpcError {
    match err {
        StoreError::NotFound(id) => RpcError::ThreadNotFound(id),
        StoreError::Invalid(message) => RpcError::InvalidParams(message),
        other => RpcError::Internal(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{
        FOLDER_REGISTER, HOST_HELLO, THREAD_GIT_COMMIT, THREAD_GIT_DIFF, THREAD_OPEN,
        THREAD_REPO_ATTACH, THREAD_SOURCE_ADD, THREAD_SUMMARY,
    };
    use crate::host::repo::git::testing;
    use serde_json::{json, Value};

    struct Fixture {
        session: HostSession,
        repo: tempfile::TempDir,
        extra: tempfile::TempDir,
        _data: tempfile::TempDir,
        folder_id: String,
        extra_id: String,
    }

    impl Fixture {
        fn call(&mut self, method: &str, params: Value) -> Value {
            let response = self.session.handle_request(JsonRpcRequest::new(
                RequestId::Number(1),
                method,
                Some(params),
            ));
            assert!(response.error.is_none(), "{method}: {:?}", response.error);
            response.result.expect("result")
        }
    }

    fn fixture() -> Fixture {
        let data = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(&data.path().join("data"));
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        let repo = tempfile::tempdir().unwrap();
        testing::init_repo(repo.path(), Some("git@github.com:jabreeflor/jabot.git"));
        let extra = tempfile::tempdir().unwrap();
        testing::init_repo(
            extra.path(),
            Some("git@github.com:jabreeflor/jabot-frontend.git"),
        );
        let folder = ok(
            &mut session,
            FOLDER_REGISTER,
            json!({ "path": repo.path().to_string_lossy(), "name": "jabot" }),
        );
        let extra_folder = ok(
            &mut session,
            FOLDER_REGISTER,
            json!({
                "path": extra.path().to_string_lossy(),
                "name": "jabot-frontend"
            }),
        );
        Fixture {
            session,
            repo,
            extra,
            _data: data,
            folder_id: folder["folderId"].as_str().unwrap().to_string(),
            extra_id: extra_folder["folderId"].as_str().unwrap().to_string(),
        }
    }

    fn ok(session: &mut HostSession, method: &str, params: Value) -> Value {
        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(7),
            method,
            Some(params),
        ));
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        response.result.expect("result")
    }

    #[test]
    fn summary_reads_the_worktree_and_extra_repo() {
        let mut fx = fixture();
        let opened = fx.call(
            THREAD_OPEN,
            json!({
                "threadId": "t-sum",
                "title": "Auth",
                "cwd": fx.repo.path().to_string_lossy(),
                "harnessId": "claude",
                "folderId": fx.folder_id,
            }),
        );
        let tree = PathBuf::from(opened["worktreePath"].as_str().unwrap());
        std::fs::write(tree.join("added.rs"), "fn main() {}\n").unwrap();

        fx.call(
            THREAD_REPO_ATTACH,
            json!({ "threadId": "t-sum", "folderId": fx.extra_id }),
        );
        let summary = fx.call(THREAD_SUMMARY, json!({ "threadId": "t-sum" }));
        assert_eq!(summary["repositories"].as_array().unwrap().len(), 2);
        let primary = &summary["repositories"][0];
        assert_eq!(primary["name"], "jabot");
        assert_eq!(primary["primary"], true);
        assert_eq!(primary["environment"], "Local");
        assert_eq!(primary["status"], "ok");
        assert!(primary["additions"].as_i64().unwrap() >= 1);
        assert_eq!(summary["repositories"][1]["name"], "jabot-frontend");
        assert_eq!(summary["repositories"][1]["primary"], false);
        assert!(fx.extra.path().exists());

        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), "notes").unwrap();
        let after = fx.call(
            THREAD_SOURCE_ADD,
            json!({
                "threadId": "t-sum",
                "path": source.path().to_string_lossy(),
            }),
        );
        assert_eq!(after["sources"].as_array().unwrap().len(), 1);

        let diff = fx.call(THREAD_GIT_DIFF, json!({ "threadId": "t-sum" }));
        assert!(diff["additions"].as_i64().unwrap() >= 1);
        assert!(diff["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| { f["path"].as_str() == Some("added.rs") }));

        fx.call(
            THREAD_GIT_COMMIT,
            json!({
                "threadId": "t-sum",
                "message": "add the entry point",
            }),
        );
        let committed = fx.call(THREAD_SUMMARY, json!({ "threadId": "t-sum" }));
        assert_eq!(committed["repositories"][0]["additions"], 0);
    }

    #[test]
    fn a_non_git_folder_is_said_not_a_repo() {
        let data = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(&data.path().join("data"));
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        let plain = tempfile::tempdir().unwrap();
        let folder = ok(
            &mut session,
            FOLDER_REGISTER,
            json!({ "path": plain.path().to_string_lossy(), "name": "notes" }),
        );
        ok(
            &mut session,
            THREAD_OPEN,
            json!({
                "threadId": "t-notes",
                "title": "Notes",
                "cwd": folder["cwd"],
                "harnessId": "claude",
                "folderId": folder["folderId"],
            }),
        );
        let summary = ok(
            &mut session,
            THREAD_SUMMARY,
            json!({ "threadId": "t-notes" }),
        );
        assert_eq!(summary["repositories"][0]["status"], "not_git");
        assert_eq!(summary["repositories"][0]["isGit"], false);
    }
}
