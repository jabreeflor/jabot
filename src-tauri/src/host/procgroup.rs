//! Child processes JaBot starts are killed as a group, never by pid.
//!
//! Almost everything this host spawns is a wrapper that forks work of its own:
//! `claude` and `codex-acp` are node scripts, `hermes acp --check` talks to a
//! runtime, and a login shell runs the user's rc files. Killing the pid we
//! spawned leaves that subtree running with no parent and nothing that will
//! ever reap it (`docs/research/app-shell/process-architecture.md`).
//!
//! **Unix:** `process_group(0)` so the child's pid becomes its pgid, then
//! `SIGTERM` the negative pgid. If the direct child exits, return — the rest
//! of the group already got SIGTERM and may finish. `SIGKILL` only if the
//! parent is still alive after grace. Same as the original adapter layer
//! (#10); #285 must not harden this into an immediate group SIGKILL.
//!
//! **Windows (#285):** a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! plus `CREATE_NEW_PROCESS_GROUP` so a `CTRL_BREAK` can be the grace signal.
//! The child is created `CREATE_SUSPENDED`, assigned, then `ResumeThread` —
//! birth-into-job. `std::process::Command` does not expose
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST` / the primary thread, so assign-after-a-
//! *running* spawn is not used: a `.cmd` → `cmd` → `node` race would leave
//! the grandchild outside the job. Closing / `TerminateJobObject` takes every
//! descendant that did not break away. If the process cannot be assigned
//! (a parent job that forbids nesting), teardown falls back to `taskkill /T /F`
//! even when the wrapper pid has already exited. Path / `PATHEXT` footguns live
//! in [`super::harness::resolve_command`], not here.

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
    /// True when `AssignProcessToJobObject` succeeded. Tests that claim to
    /// prove the Job Object path must assert this — `taskkill /T` is not
    /// that proof.
    #[cfg(windows)]
    pub(crate) fn job_assigned(&self) -> bool {
        self.job.is_assigned()
    }

    /// Why assign failed, when it did. For honest skip/fail messages.
    #[cfg(windows)]
    pub(crate) fn job_assign_error(&self) -> Option<&str> {
        self.job.assign_error()
    }

    /// True when `pid` is listed on our Job Object right now.
    #[cfg(all(test, windows))]
    pub(crate) fn job_contains(&self, pid: u32) -> bool {
        self.job.contains_pid(pid)
    }

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
        // Suspended until assigned: no user-mode code, so no `.cmd` → cmd →
        // node grandchild can be born outside the job. `Command` does not
        // expose the primary thread or `PROC_THREAD_ATTRIBUTE_JOB_LIST`.
        const CREATE_SUSPENDED: u32 = 0x00000004;
        // Leave a parent job (GHA "orphan cleanup") when that job allows
        // breakaway, so ours is not a nested assign that CI forbids.
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED | CREATE_BREAKAWAY_FROM_JOB);
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
        return spawn_windows(cmd, true);
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
                // Parent already gone: SIGTERM hit the group. Do not SIGKILL
                // the rest — that was the pre-Windows Unix behavior (#293).
                Ok(Some(_)) => return,
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

/// Windows spawn: create the job, spawn *suspended*, assign, then resume.
/// `assign_to_job` is false in tests that prove the `taskkill /T` fallback.
#[cfg(windows)]
fn spawn_windows(cmd: &mut Command, assign_to_job: bool) -> io::Result<GroupedChild> {
    let mut job = if assign_to_job {
        windows_job::Job::create().unwrap_or_else(|_| windows_job::Job::empty())
    } else {
        windows_job::Job::empty()
    };
    let mut child = cmd.spawn()?;
    job.set_pid(child.id());
    if assign_to_job && !job.is_empty() {
        if let Err(err) = job.assign(&child) {
            eprintln!(
                "jabot: AssignProcessToJobObject failed ({err}); teardown will use taskkill /T"
            );
        }
    }
    if let Err(err) = windows_job::resume_primary_thread(child.id()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    Ok(GroupedChild { child, job })
}

/// Spawn with an empty job so tests can prove the `taskkill /T` fallback,
/// including after the wrapper pid has already exited.
#[cfg(all(test, windows))]
pub(crate) fn spawn_unassigned_for_test(cmd: &mut Command) -> io::Result<GroupedChild> {
    own_group(cmd);
    spawn_windows(cmd, false)
}

#[cfg(windows)]
mod windows_job {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, FALSE, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next,
        PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, OpenThread, ResumeThread,
        PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
    };

    pub(super) struct Job {
        handle: HANDLE,
        assigned: bool,
        /// Direct child pid, for the unassigned `taskkill /T` fallback (and
        /// Drop). 0 means unset — never pass it to taskkill (pid 0 is Idle).
        pid: u32,
        assign_error: Option<String>,
        /// After an explicit [`Job::terminate`], Drop must not taskkill again
        /// on a pid that `wait` already reaped — harmless, but skip it.
        terminated: bool,
    }

    // SAFETY: `HANDLE` is an integer-sized kernel object id; the Job is moved
    // with the child and never shared across threads without the child.
    unsafe impl Send for Job {}

    impl Job {
        pub(super) fn empty() -> Self {
            Self {
                handle: std::ptr::null_mut(),
                assigned: false,
                pid: 0,
                assign_error: None,
                terminated: false,
            }
        }

        pub(super) fn is_empty(&self) -> bool {
            self.handle.is_null() || self.handle == INVALID_HANDLE_VALUE
        }

        pub(super) fn is_assigned(&self) -> bool {
            self.assigned
        }

        pub(super) fn assign_error(&self) -> Option<&str> {
            self.assign_error.as_deref()
        }

        pub(super) fn set_pid(&mut self, pid: u32) {
            self.pid = pid;
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
                    pid: 0,
                    assign_error: None,
                    terminated: false,
                })
            }
        }

        pub(super) fn assign(&mut self, child: &Child) -> io::Result<()> {
            if self.is_empty() {
                let err = io::Error::other("job object was not created");
                self.assign_error = Some(err.to_string());
                return Err(err);
            }
            // SAFETY: `child` is a live process we just spawned (still
            // suspended); the handle is valid for the lifetime of `Child`.
            unsafe {
                let process = child.as_raw_handle() as HANDLE;
                let ok = AssignProcessToJobObject(self.handle, process);
                if ok == FALSE {
                    let err = io::Error::from_raw_os_error(GetLastError() as i32);
                    self.assign_error = Some(err.to_string());
                    return Err(err);
                }
            }
            self.assigned = true;
            Ok(())
        }

        #[cfg(test)]
        pub(super) fn contains_pid(&self, pid: u32) -> bool {
            if !self.assigned || self.is_empty() {
                return false;
            }
            #[repr(C)]
            struct PidList {
                assigned: u32,
                in_list: u32,
                pids: [usize; 64],
            }
            // SAFETY: query-only; buffer is ours and sized for a typical
            // adapter tree (shim + node + a few helpers).
            unsafe {
                let mut list = PidList {
                    assigned: 0,
                    in_list: 0,
                    pids: [0; 64],
                };
                let mut needed = 0u32;
                let ok = QueryInformationJobObject(
                    self.handle,
                    JobObjectBasicProcessIdList,
                    (&mut list as *mut PidList).cast(),
                    std::mem::size_of::<PidList>() as u32,
                    &mut needed,
                );
                if ok == FALSE {
                    return false;
                }
                let n = list.in_list.min(64) as usize;
                list.pids[..n].iter().any(|p| *p as u32 == pid)
            }
        }

        /// CTRL_BREAK (grace), then `TerminateJobObject` or `taskkill /T /F`.
        pub(super) fn terminate(&mut self, child: &mut Child) {
            const GRACE: Duration = Duration::from_millis(400);
            let pid = child.id();

            // SAFETY: `CREATE_NEW_PROCESS_GROUP` made `pid` a process-group
            // id. Failure is fine — many adapters have no console (GUI Tauri).
            let signaled = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) };

            let mut exited = false;
            if signaled != FALSE {
                let deadline = Instant::now() + GRACE;
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
            }

            if self.assigned && !self.is_empty() {
                // SAFETY: we own the job and assigned this tree to it.
                // Terminates every descendant that did not break away, even
                // if the parent already exited during the grace window.
                unsafe {
                    let _ = TerminateJobObject(self.handle, 1);
                }
            } else {
                // Restricted parent job (no nesting), or job create failed.
                // Must run even when the wrapper pid is already gone —
                // `.cmd` dying in the grace window would otherwise leak
                // grandchildren. `taskkill /T` on a dead pid is not enough
                // by itself; walk ParentProcessId as well.
                taskkill_tree(pid);
            }

            if !exited {
                let _ = child.kill();
            }
            let _ = child.wait();
            self.terminated = true;
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // Unassigned job + forgotten terminate: Child::drop is one PID.
            // Sweep the tree before closing anything.
            if !self.assigned && !self.terminated && self.pid != 0 {
                taskkill_tree(self.pid);
            }
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

    /// Resume the primary thread of a `CREATE_SUSPENDED` child.
    pub(super) fn resume_primary_thread(pid: u32) -> io::Result<()> {
        let mut last_err = io::Error::other(format!(
            "could not find primary thread of suspended pid {pid}"
        ));
        for _ in 0..20 {
            match primary_thread_id(pid) {
                Ok(tid) => return resume_thread(tid),
                Err(err) => last_err = err,
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err(last_err)
    }

    fn primary_thread_id(pid: u32) -> io::Result<u32> {
        // SAFETY: snapshot of all threads; we close the handle before return.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap.is_null() || snap == INVALID_HANDLE_VALUE {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut tid = None;
            if Thread32First(snap, &mut entry) != FALSE {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        tid = Some(entry.th32ThreadID);
                        break;
                    }
                    if Thread32Next(snap, &mut entry) == FALSE {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            tid.ok_or_else(|| {
                io::Error::other(format!("no thread for suspended pid {pid} in snapshot"))
            })
        }
    }

    fn resume_thread(tid: u32) -> io::Result<()> {
        // SAFETY: query/resume the thread we just found; close before return.
        unsafe {
            let handle = OpenThread(THREAD_SUSPEND_RESUME, FALSE, tid);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            let prev = ResumeThread(handle);
            CloseHandle(handle);
            if prev == u32::MAX {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            Ok(())
        }
    }

    /// `taskkill /T` on `pid`, then a ParentProcessId walk for descendants
    /// that `taskkill` cannot see once the wrapper has already exited.
    fn taskkill_tree(pid: u32) {
        if pid == 0 {
            return;
        }
        taskkill_pid(pid);
        for descendant in descendant_pids(pid) {
            taskkill_pid(descendant);
        }
    }

    fn taskkill_pid(pid: u32) {
        if pid == 0 {
            return;
        }
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    fn descendant_pids(root: u32) -> Vec<u32> {
        let children_of = match process_parent_map() {
            Some(map) => map,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut seen = HashSet::from([root]);
        let mut queue = VecDeque::from([root]);
        while let Some(parent) = queue.pop_front() {
            let Some(children) = children_of.get(&parent) else {
                continue;
            };
            for &child in children {
                if seen.insert(child) {
                    out.push(child);
                    queue.push_back(child);
                }
            }
        }
        out
    }

    fn process_parent_map() -> Option<HashMap<u32, Vec<u32>>> {
        // SAFETY: process snapshot; we close the handle before return.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
            if Process32FirstW(snap, &mut entry) != FALSE {
                loop {
                    map.entry(entry.th32ParentProcessID)
                        .or_default()
                        .push(entry.th32ProcessID);
                    if Process32NextW(snap, &mut entry) == FALSE {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            Some(map)
        }
    }

    #[cfg(test)]
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
