//! Spawn an ACP adapter in its own process group / Job Object; stderr goes
//! to a log file.
//!
//! Kill the tree, not just the parent PID — otherwise `claude` grandchildren
//! survive JaBot (`docs/research/app-shell/process-architecture.md`). Unix
//! is still `process_group(0)` + signals. Windows (#285) is a Job Object
//! with `KILL_ON_JOB_CLOSE` (see `procgroup.rs`).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::harness;
use super::super::procgroup::{self, GroupedChild};
use super::runtime::HarnessRuntime;

#[derive(Debug)]
pub struct SpawnedAdapter {
    pub child: GroupedChild,
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

    // Resolve first so Windows `PATHEXT` (`.cmd` npm shims, `.exe`) and
    // backslash paths are the same answer the Doctor probed. Fall back to the
    // name as written so a test runtime that is itself an absolute path still
    // starts when the augmented PATH has not seen it.
    let program = harness::resolve_command(&runtime.command)
        .unwrap_or_else(|| std::path::PathBuf::from(&runtime.command));
    let mut cmd = Command::new(program);
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
    // OpenCode reads model from config, not from `session/new` on most
    // builds. Inline config is the documented override that does not rewrite
    // the user's `opencode.json`.
    if runtime.id == "opencode" {
        if let Some(model) = runtime.model.as_deref() {
            if !runtime.env.contains_key("OPENCODE_CONFIG_CONTENT") {
                cmd.env(
                    "OPENCODE_CONFIG_CONTENT",
                    serde_json::json!({ "model": model }).to_string(),
                );
            }
        }
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

    let mut child = procgroup::spawn(&mut cmd).map_err(|source| SpawnError::Spawn {
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

/// SIGTERM the process group (Unix) or Job Object / taskkill tree (Windows).
pub fn terminate_process_group(child: &mut GroupedChild) {
    procgroup::terminate(child);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::host::acp::runtime::HarnessRuntime;
    use crate::host::procgroup::process_alive;
    use std::collections::BTreeMap;
    use std::thread;
    use std::time::{Duration, Instant};

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
            model: None,
        };
        let mut spawned = spawn_adapter(&runtime, None, &log_path).unwrap();
        drop(spawned.stdin);
        drop(spawned.stdout);

        let mut grandchild = None;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(raw) = std::fs::read_to_string(&pidfile) {
                if let Ok(pid) = raw.trim().parse::<u32>() {
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
            model: None,
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

    /// Job Object + `KILL_ON_JOB_CLOSE`: a grandchild started with
    /// `UseShellExecute = $false` (CreateProcess, stays in the job) dies when
    /// we terminate the adapter. `Start-Process` without that flag can break
    /// away — that is the footgun this test is aimed at.
    ///
    /// Asserts `job_assigned` so a nested-job assign failure cannot hide
    /// behind `taskkill /T` and still look like Job Object proof.
    #[cfg(windows)]
    #[test]
    fn kill_job_reaps_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grand.pid");
        let script = dir.path().join("grand.ps1");
        let log_path = dir.path().join("adapter.stderr.log");
        std::fs::write(
            &script,
            format!(
                "$info = New-Object System.Diagnostics.ProcessStartInfo\n\
                 $info.FileName = 'ping.exe'\n\
                 $info.Arguments = '-n 120 127.0.0.1'\n\
                 $info.UseShellExecute = $false\n\
                 $info.CreateNoWindow = $true\n\
                 $p = [System.Diagnostics.Process]::Start($info)\n\
                 Set-Content -LiteralPath '{}' -Value $p.Id\n\
                 Start-Sleep -Seconds 120\n",
                pidfile.display()
            ),
        )
        .unwrap();
        let runtime = HarnessRuntime {
            id: "sleep".into(),
            command: "powershell".into(),
            args: vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script.display().to_string(),
            ],
            env: BTreeMap::new(),
            install_hint: None,
            model: None,
        };
        let mut spawned = spawn_adapter(&runtime, None, &log_path).unwrap();
        drop(spawned.stdin);
        drop(spawned.stdout);

        let mut grandchild = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(raw) = std::fs::read_to_string(&pidfile) {
                if let Ok(pid) = raw.trim().parse::<u32>() {
                    grandchild = Some(pid);
                    break;
                }
            }
            thread::sleep(Duration::from_millis(40));
        }
        let grandchild = grandchild.expect("grandchild pid file");
        assert!(
            process_alive(grandchild),
            "grandchild {grandchild} should be running before kill"
        );
        assert!(
            spawned.child.job_assigned(),
            "Job Object assign failed ({}); parent job likely forbids nesting. \
             taskkill /T is not Job Object proof — this test must not pass via fallback.",
            spawned
                .child
                .job_assign_error()
                .unwrap_or("no error recorded")
        );
        assert!(
            spawned.child.job_contains(grandchild),
            "grandchild {grandchild} was not in the Job Object — \
             spawn-then-assign lost it, or it broke away"
        );

        terminate_process_group(&mut spawned.child);
        thread::sleep(Duration::from_millis(200));
        assert!(
            !process_alive(grandchild),
            "grandchild {grandchild} survived Job Object kill"
        );
    }

    /// Nested-job / CI-sandbox path: the wrapper exits during the grace
    /// window and the job was never assigned. `taskkill /T` must still reap
    /// the grandchild — the leak #293 called out.
    #[cfg(windows)]
    #[test]
    fn taskkill_fallback_reaps_grandchild_after_wrapper_exits() {
        use crate::host::procgroup::spawn_unassigned_for_test;

        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grand.pid");
        let script = dir.path().join("grand.ps1");
        std::fs::write(
            &script,
            format!(
                "$info = New-Object System.Diagnostics.ProcessStartInfo\n\
                 $info.FileName = 'ping.exe'\n\
                 $info.Arguments = '-n 120 127.0.0.1'\n\
                 $info.UseShellExecute = $false\n\
                 $info.CreateNoWindow = $true\n\
                 $p = [System.Diagnostics.Process]::Start($info)\n\
                 Set-Content -LiteralPath '{}' -Value $p.Id\n\
                 exit 0\n",
                pidfile.display()
            ),
        )
        .unwrap();
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.display().to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        let mut child = spawn_unassigned_for_test(&mut cmd).unwrap();
        assert!(
            !child.job_assigned(),
            "fixture must be the unassigned taskkill path"
        );

        let mut grandchild = None;
        let mut wrapper_exited = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if grandchild.is_none() {
                if let Ok(raw) = std::fs::read_to_string(&pidfile) {
                    if let Ok(pid) = raw.trim().parse::<u32>() {
                        grandchild = Some(pid);
                    }
                }
            }
            if !wrapper_exited {
                match child.try_wait() {
                    Ok(Some(_)) => wrapper_exited = true,
                    Ok(None) => {}
                    Err(_) => wrapper_exited = true,
                }
            }
            if grandchild.is_some() && wrapper_exited {
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
        let grandchild = grandchild.expect("grandchild pid file");
        assert!(
            wrapper_exited,
            "wrapper must have exited before terminate so this is the leak case"
        );
        assert!(
            process_alive(grandchild),
            "grandchild {grandchild} should be running after wrapper exit"
        );

        crate::host::procgroup::terminate(&mut child);
        thread::sleep(Duration::from_millis(200));
        assert!(
            !process_alive(grandchild),
            "grandchild {grandchild} survived taskkill fallback after wrapper exit"
        );
    }

    /// Stderr is a real file path (PathBuf, not a `/`-joined string) and
    /// readers already treat `\r\n` as a line break. The adapter writes what
    /// it writes; we prove the host can open the path and read CRLF back.
    #[test]
    fn stderr_log_accepts_windows_paths_and_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("thread-one").join("adapter.stderr.log");
        let runtime = linger_runtime();
        let mut spawned = spawn_adapter(&runtime, None, &log_path).unwrap();
        drop(spawned.stdin);
        drop(spawned.stdout);
        assert!(log_path.is_file(), "{}", log_path.display());
        std::fs::write(&log_path, "not logged in\r\nplease run /login\r\n").unwrap();
        let raw = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines, ["not logged in", "please run /login"]);
        terminate_process_group(&mut spawned.child);
    }

    fn linger_runtime() -> HarnessRuntime {
        #[cfg(windows)]
        {
            HarnessRuntime {
                id: "sleep".into(),
                command: "ping".into(),
                args: vec!["-n".into(), "30".into(), "127.0.0.1".into()],
                env: BTreeMap::new(),
                install_hint: None,
                model: None,
            }
        }
        #[cfg(not(windows))]
        {
            HarnessRuntime {
                id: "sleep".into(),
                command: "sleep".into(),
                args: vec!["30".into()],
                env: BTreeMap::new(),
                install_hint: None,
                model: None,
            }
        }
    }
}
