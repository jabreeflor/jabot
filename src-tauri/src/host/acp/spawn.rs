//! Spawn an ACP adapter in its own process group; stderr goes to a log file.
//!
//! Kill the group, not just the parent PID — otherwise `claude` grandchildren
//! survive JaBot (`docs/research/app-shell/process-architecture.md`).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::runtime::HarnessRuntime;

#[derive(Debug)]
pub struct SpawnedAdapter {
    pub child: Child,
    pub stdin: std::process::ChildStdin,
    pub stdout: std::process::ChildStdout,
    pub log_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("failed to create adapter log {path}: {source}")]
    Log {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to spawn {command}: {source}")]
    Spawn {
        command: String,
        #[source]
        source: std::io::Error,
    },
}

pub fn spawn_adapter(
    runtime: &HarnessRuntime,
    cwd: Option<&Path>,
    log_path: &Path,
) -> Result<SpawnedAdapter, SpawnError> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|source| SpawnError::Log {
            path: log_path.display().to_string(),
            source,
        })?;
    }
    let log = File::create(log_path).map_err(|source| SpawnError::Log {
        path: log_path.display().to_string(),
        source,
    })?;

    let mut cmd = Command::new(&runtime.command);
    cmd.args(&runtime.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    for (key, value) in &runtime.env {
        cmd.env(key, value);
    }
    if let Some(cwd) = cwd {
        if cwd.is_dir() {
            cmd.current_dir(cwd);
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New process group; child's pid becomes the pgid.
        cmd.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    let mut child = cmd.spawn().map_err(|source| SpawnError::Spawn {
        command: runtime.command.clone(),
        source,
    })?;
    let stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    Ok(SpawnedAdapter {
        child,
        stdin,
        stdout,
        log_path: log_path.to_path_buf(),
    })
}

/// SIGTERM the process group, then SIGKILL if it is still alive.
pub fn terminate_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::acp::runtime::HarnessRuntime;
    use std::collections::BTreeMap;
    use std::process::Command;

    #[cfg(unix)]
    #[test]
    fn kill_group_reaps_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grand.pid");
        let log_path = dir.path().join("adapter.stderr.log");
        let script = format!(
            "sleep 120 & echo $! > {}; exec sleep 120",
            pidfile.display()
        );
        let runtime = HarnessRuntime {
            id: "sleep".into(),
            command: "sh".into(),
            args: vec!["-c".into(), script],
            env: BTreeMap::new(),
            install_hint: None,
        };
        let mut spawned = spawn_adapter(&runtime, None, &log_path).unwrap();
        drop(spawned.stdin);
        drop(spawned.stdout);

        let mut grandchild = None;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(raw) = std::fs::read_to_string(&pidfile) {
                if let Ok(pid) = raw.trim().parse::<i32>() {
                    grandchild = Some(pid);
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        let grandchild = grandchild.expect("grandchild pid file");
        assert!(
            process_alive(grandchild),
            "grandchild {grandchild} should be running before kill"
        );

        terminate_process_group(&mut spawned.child);
        thread::sleep(Duration::from_millis(100));
        assert!(
            !process_alive(grandchild),
            "grandchild {grandchild} survived process-group kill"
        );
    }

    #[cfg(unix)]
    fn process_alive(pid: i32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
