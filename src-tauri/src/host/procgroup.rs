//! Child processes JaBot starts are killed as a group, never by pid.
//!
//! Almost everything this host spawns is a wrapper that forks work of its own:
//! `claude` and `codex-acp` are node scripts, `hermes acp --check` talks to a
//! runtime, and a login shell runs the user's rc files. Killing the pid we
//! spawned leaves that subtree running with no parent and nothing that will
//! ever reap it (`docs/research/app-shell/process-architecture.md`).
//!
//! **Unix:** `process_group(0)` so the child's pid becomes its pgid, then
//! `SIGTERM` / `SIGKILL` the negative pgid. Unchanged from the original
//! adapter layer (#10).
//!
//! **Windows (#285):** a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! plus `CREATE_NEW_PROCESS_GROUP` so a `CTRL_BREAK` can be the grace signal.
//! Closing / `TerminateJobObject` takes every descendant that did not break
//! away. If the process cannot be assigned (a parent job that forbids nesting),
//! teardown falls back to `taskkill /T /F`. Path / `PATHEXT` footguns live in
//! [`super::harness::resolve_command`], not here.

use std::io;
use std::ops::{Deref, DerefMut};
use std::process::{Child, Command};

/// A child spawned by [`spawn`]: Unix process group or Windows Job Object.
pub(crate) struct GroupedChild {
    child: Child,
    #[cfg(windows)]
    job: windows_job::Job,
}

impl Deref for GroupedChild {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for GroupedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl std::fmt::Debug for GroupedChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupedChild")
            .field("pid", &self.child.id())
            .finish_non_exhaustive()
    }
}

impl GroupedChild {
    /// Consume the child after it has exited, keeping the Windows job
    /// handle alive until `wait` returns so `KILL_ON_JOB_CLOSE` cannot
    /// race the reaper.
    pub(crate) fn wait_with_output(self) -> io::Result<std::process::Output> {
        #[cfg(windows)]
        {
            let GroupedChild { child, job } = self;
            let output = child.wait_with_output();
            drop(job);
            output
        }
        #[cfg(not(windows))]
        {
            self.child.wait_with_output()
        }
    }
}

/// Give the child its own process group / console group, so it can be
/// signalled as one. Windows also needs the Job Object created in [`spawn`];
/// these flags alone do not kill grandchildren.
fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 0 means "the child's own pid becomes the pgid".
        cmd.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Console-ctrl analogue of a new pgid. The kill-tree is the Job Object,
        // not this flag: `CREATE_NEW_PROCESS_GROUP` only affects Ctrl+Break.
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// Spawn `cmd` in a group that [`terminate`] can reap as a tree.
pub(crate) fn spawn(cmd: &mut Command) -> io::Result<GroupedChild> {
    own_group(cmd);

    #[cfg(windows)]
    {
        let mut job = windows_job::Job::create().unwrap_or_else(|_| windows_job::Job::empty());
        let child = cmd.spawn()?;
        if !job.is_empty() {
            let _ = job.assign(&child);
        }
        return Ok(GroupedChild { child, job });
    }

    #[cfg(not(windows))]
    {
        Ok(GroupedChild {
            child: cmd.spawn()?,
        })
    }
}

/// SIGTERM the process group (Unix) or CTRL_BREAK + job/taskkill tree (Windows),
/// then a hard kill if it is still alive.
pub(crate) fn terminate(child: &mut GroupedChild) {
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
                Ok(Some(_)) => break,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(windows)]
    {
        child.job.terminate(&mut child.child);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// True only if the pid is a *running* process.
///
/// Unix: `kill -0` is not enough — it succeeds for a zombie, and a group-killed
/// grandchild is exactly that until its reparented init gets around to reaping
/// it. Under a PID 1 that does not reap promptly — most containers, including
/// CI — `kill -0` would report a corpse as alive and fail these tests against
/// correct code. Ask for the process state instead and treat `Z` as gone.
///
/// Windows: `OpenProcess` + `GetExitCodeProcess`, treating `STILL_ACTIVE` as
/// alive. No zombie equivalent we have to special-case.
#[cfg(test)]
pub(crate) fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
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

    #[cfg(windows)]
    {
        windows_job::process_alive(pid)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(windows)]
mod windows_job {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, FALSE, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub(super) struct Job {
        handle: HANDLE,
        assigned: bool,
    }

    // SAFETY: `HANDLE` is an integer-sized kernel object id; the Job is moved
    // with the child and never shared across threads without the child.
    unsafe impl Send for Job {}

    impl Job {
        pub(super) fn empty() -> Self {
            Self {
                handle: std::ptr::null_mut(),
                assigned: false,
            }
        }

        pub(super) fn is_empty(&self) -> bool {
            self.handle.is_null() || self.handle == INVALID_HANDLE_VALUE
        }

        pub(super) fn create() -> io::Result<Self> {
            // SAFETY: a nameless job; we own the handle and set limits before
            // any process is assigned.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                    return Err(io::Error::from_raw_os_error(GetLastError() as i32));
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == FALSE {
                    let err = io::Error::from_raw_os_error(GetLastError() as i32);
                    CloseHandle(handle);
                    return Err(err);
                }
                Ok(Self {
                    handle,
                    assigned: false,
                })
            }
        }

        pub(super) fn assign(&mut self, child: &Child) -> io::Result<()> {
            if self.is_empty() {
                return Err(io::Error::other("job object was not created"));
            }
            // SAFETY: `child` is a live process we just spawned; the handle is
            // valid for the lifetime of `Child`.
            unsafe {
                let process = child.as_raw_handle() as HANDLE;
                let ok = AssignProcessToJobObject(self.handle, process);
                if ok == FALSE {
                    return Err(io::Error::from_raw_os_error(GetLastError() as i32));
                }
            }
            self.assigned = true;
            Ok(())
        }

        /// CTRL_BREAK (grace), then `TerminateJobObject` or `taskkill /T /F`.
        pub(super) fn terminate(&mut self, child: &mut Child) {
            const GRACE: Duration = Duration::from_millis(400);
            let pid = child.id();

            // SAFETY: `CREATE_NEW_PROCESS_GROUP` made `pid` a process-group
            // id. Failure is fine — many adapters have no console.
            unsafe {
                let _ = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid);
            }

            let deadline = Instant::now() + GRACE;
            let mut exited = false;
            while Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }

            if self.assigned && !self.is_empty() {
                // SAFETY: we own the job and assigned this tree to it.
                // Terminates every descendant that did not break away, even
                // if the parent already exited during the grace window.
                unsafe {
                    let _ = TerminateJobObject(self.handle, 1);
                }
            } else if !exited {
                // Restricted parent job (no nesting): documented fallback.
                let _ = Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }

            if !exited {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if self.is_empty() {
                return;
            }
            // SAFETY: we own the handle. `KILL_ON_JOB_CLOSE` reaps anyone
            // still in the job — the leak-prevention the issue asked for.
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = std::ptr::null_mut();
        }
    }

    pub(super) fn process_alive(pid: u32) -> bool {
        // SAFETY: query-only; we close the handle before returning.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != FALSE && code == STILL_ACTIVE as u32
        }
    }
}
