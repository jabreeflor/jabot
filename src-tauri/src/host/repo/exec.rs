//! Running `git` and `gh` as children of the host.
//!
//! Two rules the rest of this module depends on.
//!
//! **The augmented PATH, not the process PATH.** A `.app` launched from the
//! Dock inherits launchd's `PATH`, where Homebrew's `gh` does not exist
//! (`harness/path.rs`). Resolving through the same list the harness Doctor
//! searches is what stops "GitHub CLI not installed" from being a lie told to
//! a user who has it in their terminal.
//!
//! **Every call has a deadline and a process group.** `gh auth status` talks to
//! the network and `git` can block on a lock or a credential helper prompt; a
//! folder registration that hangs takes the whole host request thread with it.
//! The group is because both shell out further — `git` to credential helpers,
//! `gh` to `git` — and killing the pid alone leaves that subtree behind.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::super::harness::{path, resolve_command};
use super::super::procgroup;

/// How long a probe gets before it is killed. Generous enough for `gh` to make
/// one network round trip, short enough that a hung helper does not read as a
/// frozen app.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    /// Trimmed stdout, but only from a command that succeeded. A failed `git`
    /// still prints things, and treating that as an answer is how a folder
    /// ends up registered with an error message as its origin URL.
    pub fn line(&self) -> Option<String> {
        if !self.ok() {
            return None;
        }
        let line = self.stdout.trim();
        (!line.is_empty()).then(|| line.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The binary is not on the augmented PATH at all.
    NotInstalled(String),
    TimedOut,
    Failed(String),
}

/// One child process: what to run, where, and with what added to its
/// environment. `cwd` and `env` exist for #23's setup command — a folder's
/// `npm ci` has to run *in* the new worktree and be told where it is — and are
/// empty for the probes this module was written for.
#[derive(Debug)]
pub struct Spawn<'a> {
    pub command: &'a str,
    pub args: &'a [&'a str],
    pub cwd: Option<&'a Path>,
    /// Added to the child's environment, on top of the host's.
    pub env: &'a [(&'a str, String)],
    pub timeout: Duration,
}

impl<'a> Spawn<'a> {
    pub fn new(command: &'a str, args: &'a [&'a str], timeout: Duration) -> Self {
        Self {
            command,
            args,
            cwd: None,
            env: &[],
            timeout,
        }
    }

    pub fn in_dir(mut self, dir: &'a Path) -> Self {
        self.cwd = Some(dir);
        self
    }

    pub fn with_env(mut self, env: &'a [(&'a str, String)]) -> Self {
        self.env = env;
        self
    }
}

pub fn run(command: &str, args: &[&str], timeout: Duration) -> Result<Output, RunError> {
    spawn(Spawn::new(command, args, timeout))
}

pub fn spawn(spec: Spawn<'_>) -> Result<Output, RunError> {
    let Spawn {
        command,
        args,
        cwd,
        env,
        timeout,
    } = spec;
    let Some(resolved) = resolve_command(command) else {
        return Err(RunError::NotInstalled(command.to_string()));
    };
    let mut cmd = Command::new(resolved);
    cmd.args(args)
        .env("PATH", path::joined())
        // A credential helper that decides to ask the terminal a question must
        // find no terminal: the answer would never arrive and the deadline
        // would be the only thing that ended the call.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    procgroup::own_group(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| RunError::Failed(e.to_string()))?;
    // Drained on threads rather than after the wait: a command that fills the
    // 64 KiB pipe buffer blocks forever waiting for a reader that is itself
    // waiting for the command to exit.
    let mut out = child.stdout.take().expect("stdout is piped");
    let mut err = child.stderr.take().expect("stderr is piped");
    let out_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out.read_to_string(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            Ok(None) => {
                procgroup::terminate(&mut child);
                break None;
            }
            Err(err) => return Err(RunError::Failed(err.to_string())),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    match status {
        Some(status) => Ok(Output {
            code: status.code(),
            stdout,
            stderr,
        }),
        None => Err(RunError::TimedOut),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_binary_rather_than_failing_to_spawn() {
        let err = run("jabot-not-a-real-binary-xyz", &[], PROBE_TIMEOUT).unwrap_err();
        assert!(matches!(err, RunError::NotInstalled(_)), "{err:?}");
    }

    #[test]
    fn captures_stdout_and_the_exit_code() {
        let out = run("git", &["--version"], PROBE_TIMEOUT).expect("git is required to build");
        assert!(out.ok(), "{out:?}");
        assert!(out.line().unwrap().starts_with("git version"));
    }

    #[test]
    fn a_failed_command_has_no_line_to_offer() {
        let out = run("git", &["rev-parse", "--show-toplevel"], PROBE_TIMEOUT)
            .expect("git is required to build");
        // Run from an arbitrary cwd this may succeed or fail; what must hold is
        // that a non-zero exit never hands its output back as an answer.
        if !out.ok() {
            assert!(out.line().is_none());
        }
    }

    #[test]
    fn a_child_runs_where_it_is_told_and_sees_what_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let env = [("JABOT_PROBE_ECHO".to_string(), "worktree".to_string())];
        let env: Vec<(&str, String)> = env.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let out = spawn(
            Spawn::new("sh", &["-c", "pwd; echo $JABOT_PROBE_ECHO"], PROBE_TIMEOUT)
                .in_dir(dir.path())
                .with_env(&env),
        )
        .expect("sh is required to build");
        assert!(out.ok(), "{out:?}");
        let canonical = std::fs::canonicalize(dir.path()).unwrap();
        // Both halves matter: a setup command that runs in the wrong directory
        // installs the wrong project's dependencies, and one that cannot see
        // where the worktree is cannot copy anything into it.
        assert!(
            out.stdout
                .contains(&canonical.to_string_lossy().into_owned()),
            "{out:?}"
        );
        assert!(out.stdout.contains("worktree"), "{out:?}");
    }

    #[test]
    fn a_command_that_never_exits_is_killed_at_the_deadline() {
        let err = run("sleep", &["30"], Duration::from_millis(150)).unwrap_err();
        assert_eq!(err, RunError::TimedOut);
    }
}
