//! Spawn an ACP adapter in its own process group; stderr goes to a log file.
//!
//! Kill the group, not just the parent PID — otherwise `claude` grandchildren
//! survive JaBot (`docs/research/app-shell/process-architecture.md`).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use super::super::harness;
use super::super::procgroup;
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
    /// Asked to start an adapter in a directory that is not there.
    #[error("the working directory {path} does not exist")]
    Cwd { path: String },
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
    // A Finder-launched .app inherits launchd's PATH, so an adapter that
    // shells out to `node`, `git`, or its own vendor CLI would not find them.
    // Hand the child the same augmented PATH the catalog probed with (#13).
    cmd.env("PATH", harness::path::joined());
    // Applied as written, not as a default. By the time a runtime reaches the
    // spawner its env is the thread's snapshot: the catalog's floor was
    // already resolved when the spec was built (`HarnessDescriptor::
    // runtime_spec`), and everything else came from the client's `thread/open`
    // or a tier-3 file. Letting the host's own environment win here would mean
    // an exported `HERMES_HOME` quietly redirecting every profile-scoped
    // thread onto one state directory (`setup-porting/hermes.md`).
    for (key, value) in &runtime.env {
        cmd.env(key, value);
    }
    if let Some(cwd) = cwd {
        // Refuse rather than fall through. A child given no `current_dir`
        // inherits the host's, so a thread whose checkout was unmounted or
        // moved would run the agent's shell and edit tools against whatever
        // folder JaBot was launched from. #21 catches this earlier and with a
        // better error; this is the backstop for every other caller.
        if !cwd.is_dir() {
            return Err(SpawnError::Cwd {
                path: cwd.display().to_string(),
            });
        }
        cmd.current_dir(cwd);
    }

    procgroup::own_group(&mut cmd);

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
    procgroup::terminate(child);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::acp::runtime::HarnessRuntime;
    use std::collections::BTreeMap;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::host::procgroup::process_alive;

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

    /// A thread's snapshotted env is the runtime it recorded, so it has to
    /// reach the child even when the host process exports the same key. `HOME`
    /// is the one key every machine running this test already has, which makes
    /// it the only honest way to ask the question without mutating the test
    /// process's own environment.
    #[cfg(unix)]
    #[test]
    fn snapshotted_env_beats_the_hosts_own() {
        let Some(host_home) = std::env::var_os("HOME") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("home.txt");
        let runtime = HarnessRuntime {
            id: "env".into(),
            command: "sh".into(),
            args: vec![
                "-c".into(),
                format!("printf %s \"$HOME\" > {}", seen.display()),
            ],
            env: BTreeMap::from([("HOME".to_string(), "/jabot/from-thread".to_string())]),
            install_hint: None,
        };
        let mut spawned = spawn_adapter(&runtime, None, &dir.path().join("stderr.log")).unwrap();
        drop(spawned.stdin);
        drop(spawned.stdout);
        spawned.child.wait().unwrap();

        assert_ne!(host_home, std::ffi::OsString::from("/jabot/from-thread"));
        assert_eq!(
            std::fs::read_to_string(&seen).unwrap(),
            "/jabot/from-thread"
        );
    }
}
