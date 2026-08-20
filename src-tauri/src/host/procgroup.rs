//! Child processes JaBot starts are killed as a group, never by pid.
//!
//! Almost everything this host spawns is a wrapper that forks work of its own:
//! `claude` and `codex-acp` are node scripts, `hermes acp --check` talks to a
//! runtime, and a login shell runs the user's rc files. Killing the pid we
//! spawned leaves that subtree running with no parent and nothing that will
//! ever reap it (`docs/research/app-shell/process-architecture.md`). Signal the
//! group and the whole tree goes.
//!
//! Adapters were already doing this; readiness probes are the case that
//! matters most, because a user runs the Doctor precisely when a vendor CLI is
//! hanging, and that is the run whose timeout would leak.

use std::process::{Child, Command};

/// Give the child its own process group, so it can be signalled as one.
pub(crate) fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 0 means "the child's own pid becomes the pgid".
        cmd.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// SIGTERM the process group, then SIGKILL if it is still alive.
pub(crate) fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        use std::thread;
        use std::time::{Duration, Instant};

        /// How long a signalled group gets to exit before it is killed outright.
        const GRACE: Duration = Duration::from_millis(400);

        let pid = child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + GRACE;
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

/// True only if the pid is a *running* process.
///
/// `kill -0` is not enough: it succeeds for a zombie, and a group-killed
/// grandchild is exactly that until its reparented init gets around to reaping
/// it. Under a PID 1 that does not reap promptly — most containers, including
/// CI — `kill -0` would report a corpse as alive and fail these tests against
/// correct code. Ask for the process state instead and treat `Z` as gone.
#[cfg(all(test, unix))]
pub(crate) fn process_alive(pid: i32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output();
    match output {
        Ok(output) => {
            let state = String::from_utf8_lossy(&output.stdout);
            let state = state.trim();
            !state.is_empty() && !state.starts_with('Z')
        }
        Err(_) => false,
    }
}
