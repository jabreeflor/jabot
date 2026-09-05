//! Desktop workspace selection. Network and filesystem work never holds the host lock.
use super::exec;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::Manager;

#[derive(Deserialize, Serialize)]
pub struct Repository {
    full_name: String,
    description: Option<String>,
    private: bool,
}

fn gh(args: &[&str], seconds: u64) -> Result<String, String> {
    let output = exec::run("gh", args, Duration::from_secs(seconds))
        .map_err(|err| format!("GitHub request failed: {err:?}"))?;
    if !output.ok() {
        return Err(output.stderr.trim().to_string());
    }
    Ok(output.stdout)
}

#[tauri::command]
pub async fn pick_workspace() -> Result<Option<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .set_title("Open a folder for this session")
        .pick_folder()
        .await
        .map(|file| file.path().to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn github_repositories(host: String, page: u32) -> Result<Vec<Repository>, String> {
    if !super::super::pr::github::is_hostname(&host) || page == 0 {
        return Err("Invalid GitHub host or page.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let endpoint = format!("user/repos?sort=pushed&per_page=50&page={page}");
        let body = gh(&["api", "--hostname", &host, &endpoint], 30)?;
        serde_json::from_str(&body).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

fn valid_repo(repo: &str) -> bool {
    let parts: Vec<_> = repo.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
}

#[tauri::command]
pub async fn clone_repository(
    app: tauri::AppHandle,
    host: String,
    repo: String,
) -> Result<String, String> {
    if !super::super::pr::github::is_hostname(&host) || !valid_repo(&repo) {
        return Err("Invalid GitHub repository.".into());
    }
    let root = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("repositories")
        .join(uuid::Uuid::new_v4().to_string());
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&root).map_err(|err| err.to_string())?;
        let target = root.join(repo.split('/').next_back().unwrap());
        let path = target.to_string_lossy().into_owned();
        let url = format!("https://{host}/{repo}");
        if let Err(err) = gh(&["repo", "clone", &url, &path], 300) {
            // This UUID directory belongs only to this attempt.
            let _ = std::fs::remove_dir_all(&root);
            return Err(err);
        }
        Ok(path)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn scratch_workspace(app: tauri::AppHandle) -> Result<String, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("sessions")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::valid_repo;
    #[test]
    fn accepts_repo_names_and_rejects_paths_and_options() {
        assert!(valid_repo("owner/repo.name-1"));
        for value in [
            "../repo",
            "owner/..",
            "owner/repo/extra",
            "owner/",
            "--flag",
            "o/r;evil",
        ] {
            assert!(!valid_repo(value));
        }
    }
}
