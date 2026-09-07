//! Explicit onboarding installs. Catalog hints are never executable input.
//! Only these pinned adapters can be installed, into the user's local prefix.
use super::super::protocol::error::RpcError;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallParams {
    pub harness_id: String,
    #[serde(default)]
    pub start: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    pub running: bool,
    pub error: Option<String>,
}
static JOBS: OnceLock<Mutex<HashMap<String, InstallStatus>>> = OnceLock::new();

fn package(id: &str) -> Result<&'static str, RpcError> {
    match id {
        "claude" => Ok("@agentclientprotocol/claude-agent-acp@0.75.1"),
        "codex" => Ok("@zed-industries/codex-acp@0.16.0"),
        "pi" => Ok("pi-acp@0.0.33"),
        _ => Err(RpcError::InvalidParams(
            "Automatic installation is available only for Claude, Codex and Pi adapters.".into(),
        )),
    }
}
pub fn install(params: InstallParams) -> Result<InstallStatus, RpcError> {
    let package = package(&params.harness_id)?;
    let mut jobs = JOBS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| RpcError::Internal("Installer unavailable; restart JaBot.".into()))?;
    if let Some(job) = jobs.get(&params.harness_id) {
        if job.running || !params.start {
            return Ok(job.clone());
        }
    }
    if !params.start {
        return Ok(InstallStatus {
            running: false,
            error: None,
        });
    }
    if jobs.values().any(|job| job.running) {
        return Err(RpcError::InvalidParams(
            "Another adapter is installing. Wait for it to finish, then retry.".into(),
        ));
    }
    let npm = super::resolve_command("npm")
        .ok_or_else(|| RpcError::InvalidParams("Install Node.js and npm, then retry.".into()))?;
    let prefix = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| RpcError::Internal("Home directory unavailable.".into()))?
        .join(".local");
    let status = InstallStatus {
        running: true,
        error: None,
    };
    jobs.insert(params.harness_id.clone(), status.clone());
    std::thread::spawn(move || {
        let result = run(npm, prefix, package);
        if let Ok(mut jobs) = JOBS.get_or_init(Default::default).lock() {
            jobs.insert(
                params.harness_id,
                InstallStatus {
                    running: false,
                    error: result.err(),
                },
            );
        }
    });
    Ok(status)
}
fn run(npm: PathBuf, prefix: PathBuf, package: &str) -> Result<(), String> {
    // No shell, no sudo and no caller-supplied package/command. Discard output
    // rather than risk a filled pipe blocking the installer or exposing tokens.
    let mut command = Command::new(npm);
    super::super::procgroup::own_group(&mut command);
    let mut child = command
        .args(["install", "--global", "--prefix"])
        .arg(prefix)
        .args(["--no-audit", "--no-fund", package])
        .env("PATH", super::path::joined())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start npm: {e}. Retry after installing Node.js."))?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("Adapter installation failed. Check your network and ~/.local permissions, then retry or skip.".into()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => { super::super::procgroup::terminate(&mut child); return Err("Adapter installation timed out or stopped. Retry or skip.".into()); }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_catalog_commands_and_unknown_harnesses() {
        for id in ["custom", "hermes", "claude; touch /tmp/no", ""] {
            assert!(package(id).is_err());
        }
        assert_eq!(
            package("codex").unwrap(),
            "@zed-industries/codex-acp@0.16.0"
        );
    }
    #[test]
    fn polling_does_not_start_an_install() {
        let status = install(InstallParams {
            harness_id: "pi".into(),
            start: false,
        })
        .unwrap();
        assert!(!status.running);
        assert!(status.error.is_none());
    }
}
