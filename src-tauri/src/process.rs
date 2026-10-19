//! Child processes. A killed host must not leave a recorder or model resident,
//! and a desktop app must not flash console windows.
//!
//! Linux: every child leads its own process group and gets SIGTERM if the host
//! dies; `kill_tree` kills the group.
//! Windows: the app runs inside a Job Object that Windows closes when the app
//! exits for any reason, ending everything it started (`contain_children`).
//! Children started with `spawn` also get a job of their own, so `kill_tree`
//! ends a worker together with the engines it started. No console windows.
use std::{
    ffi::OsStr,
    process::{Child, Command},
};

/// A supervised child: start it with `spawn`, end its tree with `kill_tree`.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut c = Command::new(program);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // All descendants join a dedicated process group; orderly shutdown
        // also kills that group.
        unsafe {
            c.pre_exec(|| {
                libc::setpgid(0, 0);
                #[cfg(target_os = "linux")]
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    hide_console(&mut c);
    c
}

/// A short-lived helper (lookups, probes) that needs no supervision.
pub fn hidden(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    hide_console(&mut c);
    c
}

/// A Python interpreter. Windows Python reads and writes files in the ANSI
/// code page unless UTF-8 mode is on, which would corrupt transcript text.
pub fn python(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = command(program);
    // No bytecode in the app's folder either: the uninstaller removes the
    // files it installed, and Python's caches would stay behind.
    #[cfg(windows)]
    c.env("PYTHONUTF8", "1").env("PYTHONDONTWRITEBYTECODE", "1");
    c
}

/// Starts a child that `kill_tree` may have to end, with everything it starts.
pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    #[cfg(windows)]
    return jobs::spawn(cmd);
    #[cfg(not(windows))]
    cmd.spawn()
}

/// Kills a child started with `spawn` and everything it started.
pub fn kill_tree(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    jobs::kill(pid);
}

/// A child started with `spawn` has exited and been waited for. Windows ends
/// anything it left running and releases its job. Linux needs nothing: a
/// child's own descendants die with it.
pub fn finished(pid: u32) {
    #[cfg(windows)]
    jobs::release(pid);
    #[cfg(not(windows))]
    let _ = pid;
}

/// Puts the app itself in a Job Object (Windows), so a crash or a forced
/// close still ends every worker, engine and recorder. Call once, at startup.
pub fn contain_children() {
    #[cfg(windows)]
    jobs::contain_app();
}

/// Process IDs of the app and everything it started, once contained (Windows).
#[cfg(windows)]
pub fn contained_pids() -> Option<Vec<u32>> {
    jobs::app_pids()
}

#[cfg(windows)]
fn hide_console(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
mod jobs {
    use std::{
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Child, Command, Stdio},
        sync::{Mutex, OnceLock},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{
                GetCurrentProcess, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
                THREAD_SUSPEND_RESUME,
            },
        },
    };

    /// A Job Object whose processes end when its last handle closes.
    struct Job(HANDLE);
    // A job handle may be used and closed from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}
    impl Job {
        fn new() -> Option<Job> {
            let job = Job(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
            if job.0.is_null() {
                return None;
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            (set != 0).then_some(job)
        }
        fn assign(&self, process: HANDLE) -> bool {
            unsafe { AssignProcessToJobObject(self.0, process) != 0 }
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    static APP: OnceLock<Job> = OnceLock::new();
    static CHILDREN: Mutex<Vec<(u32, Job)>> = Mutex::new(Vec::new());

    pub fn contain_app() {
        if let Some(job) = Job::new() {
            // Never closed: Windows closes it as the app exits, ending the rest.
            if job.assign(unsafe { GetCurrentProcess() }) {
                let _ = APP.set(job);
            }
        }
    }

    pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
        // Suspended until it is in its job, so nothing it starts can escape.
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        let mut child = cmd.spawn()?;
        let job = Job::new().filter(|job| job.assign(child.as_raw_handle() as HANDLE));
        if resume(child.id()) == 0 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("The process could not be started"));
        }
        if let Some(job) = job {
            CHILDREN.lock().unwrap().push((child.id(), job));
        }
        Ok(child)
    }

    fn take(pid: u32) -> Option<Job> {
        let mut children = CHILDREN.lock().unwrap();
        let index = children.iter().position(|(p, _)| *p == pid)?;
        Some(children.swap_remove(index).1)
    }

    pub fn kill(pid: u32) {
        match take(pid) {
            Some(job) => unsafe {
                TerminateJobObject(job.0, 1);
            },
            // Not started by `spawn`, or already released: end the tree by parentage.
            None => {
                let _ = super::hidden("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    pub fn release(pid: u32) {
        drop(take(pid));
    }

    /// Resumes the threads of a process started suspended; returns how many.
    fn resume(pid: u32) -> usize {
        let mut resumed = 0;
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return 0;
            }
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        if ResumeThread(thread) != u32::MAX {
                            resumed += 1;
                        }
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        resumed
    }

    pub fn app_pids() -> Option<Vec<u32>> {
        let job = APP.get()?;
        #[repr(C)]
        struct List {
            assigned: u32,
            listed: u32,
            ids: [usize; 1024],
        }
        let mut list = List {
            assigned: 0,
            listed: 0,
            ids: [0; 1024],
        };
        let ok = unsafe {
            QueryInformationJobObject(
                job.0,
                JobObjectBasicProcessIdList,
                &mut list as *mut _ as *mut _,
                size_of::<List>() as u32,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then(|| {
            list.ids[..list.listed.min(1024) as usize]
                .iter()
                .map(|&id| id as u32)
                .collect()
        })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                GetProcessTimes, OpenProcess, WaitForSingleObject,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            },
        },
    };

    /// A process a test watches. Holding its handle stops Windows from giving
    /// its PID to a new process while the test waits for it to end.
    struct Watched {
        pid: u32,
        name: String,
        handle: HANDLE,
        created: u64,
    }
    impl Watched {
        fn open(pid: u32, name: &str) -> Option<Watched> {
            unsafe {
                let handle = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                );
                if handle.is_null() {
                    return None;
                }
                let (mut created, mut exited, mut kernel, mut user) = (
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                );
                GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
                Some(Watched {
                    pid,
                    name: name.to_string(),
                    handle,
                    created: (created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64,
                })
            }
        }
        fn running(&self) -> bool {
            unsafe { WaitForSingleObject(self.handle, 0) == WAIT_TIMEOUT }
        }
        fn is(&self, name: &str) -> bool {
            self.name.eq_ignore_ascii_case(name)
        }
    }
    impl Drop for Watched {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    /// Everything `root` started, directly or not. A recorded parent PID can
    /// belong to a process that died long ago and whose PID `root` now has,
    /// so a child must also be younger than its parent.
    fn descendants(root: &Watched) -> Vec<Watched> {
        let mut entries = vec![];
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            assert_ne!(snapshot, INVALID_HANDLE_VALUE);
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let name = &entry.szExeFile;
                let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                entries.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    String::from_utf16_lossy(&name[..length]),
                ));
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        let mut found: Vec<Watched> = vec![];
        loop {
            let before = found.len();
            for (pid, parent, name) in &entries {
                if *pid == root.pid || found.iter().any(|p| p.pid == *pid) {
                    continue;
                }
                let Some(born_after) = std::iter::once(root)
                    .chain(found.iter())
                    .find(|p| p.pid == *parent)
                    .map(|p| p.created)
                else {
                    continue;
                };
                if let Some(child) = Watched::open(*pid, name) {
                    if child.created >= born_after {
                        found.push(child);
                    }
                }
            }
            if found.len() == before {
                return found;
            }
        }
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn until_ended(what: &str, processes: &[Watched]) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let running: Vec<String> = processes
                .iter()
                .filter(|p| p.running())
                .map(|p| format!("{} {}", p.name, p.pid))
                .collect();
            if running.is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out: {what}; still running: {running:?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// cmd.exe that starts ping.exe, which runs for a minute unless killed.
    fn shell_with_grandchild(detach: bool) -> Command {
        let mut c = command("cmd");
        c.args(if detach {
            ["/C", "start", "/B", "ping", "-n", "60", "127.0.0.1"].as_slice()
        } else {
            ["/C", "ping", "-n", "60", "127.0.0.1"].as_slice()
        })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        c
    }

    #[test]
    fn kill_tree_ends_a_child_and_what_it_started() {
        let mut child = spawn(&mut shell_with_grandchild(false)).unwrap();
        let shell = Watched::open(child.id(), "cmd.exe").unwrap();
        let mut started = vec![];
        until("ping to start", || {
            started = descendants(&shell);
            started.iter().any(|p| p.is("ping.exe"))
        });
        kill_tree(child.id());
        child.wait().unwrap();
        until_ended("every process to end", &started);
    }

    #[test]
    fn finished_ends_what_an_exited_child_left_running() {
        let mut child = spawn(&mut shell_with_grandchild(true)).unwrap();
        let shell = Watched::open(child.id(), "cmd.exe").unwrap();
        let mut started = vec![];
        until("ping to start", || {
            started = descendants(&shell);
            started.iter().any(|p| p.is("ping.exe"))
        });
        child.wait().unwrap();
        assert!(
            started.iter().any(|p| p.is("ping.exe") && p.running()),
            "ping should outlive its shell"
        );
        finished(child.id());
        until_ended("the orphan to end", &started);
    }

    /// Run by `a_force_killed_app_leaves_nothing_running` as a stand-in app.
    #[test]
    fn stand_in_app() {
        // The harness may capture stdout, so the PIDs go to a file.
        let Some(report) = std::env::var_os("VOXLOOM_STAND_IN_APP") else {
            return;
        };
        contain_children();
        let mut supervised = spawn(&mut shell_with_grandchild(false)).unwrap();
        // Not started with `spawn`: the app's own job must still cover it.
        let mut plain = shell_with_grandchild(false).spawn().unwrap();
        let partial = std::path::PathBuf::from(&report).with_extension("part");
        std::fs::write(&partial, format!("{} {}", supervised.id(), plain.id())).unwrap();
        std::fs::rename(partial, report).unwrap();
        std::thread::sleep(Duration::from_secs(60));
        let _ = (
            supervised.kill(),
            plain.kill(),
            supervised.wait(),
            plain.wait(),
        );
    }

    #[test]
    fn a_force_killed_app_leaves_nothing_running() {
        let temp = tempfile::tempdir().unwrap();
        let report = temp.path().join("started.txt");
        let mut app = Command::new(std::env::current_exe().unwrap())
            .args([
                "process::tests::stand_in_app",
                "--exact",
                "--test-threads=1",
            ])
            .env("VOXLOOM_STAND_IN_APP", &report)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        until("the stand-in app to start its children", || {
            report.is_file()
        });
        // Both shells run ping for a minute, so their PIDs are still theirs.
        let mut shells: Vec<Watched> = std::fs::read_to_string(&report)
            .unwrap()
            .split(' ')
            .map(|pid| Watched::open(pid.parse().unwrap(), "cmd.exe").unwrap())
            .collect();
        let mut started = vec![];
        until("both pings to start", || {
            started = shells.iter().flat_map(descendants).collect();
            started.iter().filter(|p| p.is("ping.exe")).count() == 2
        });
        // TerminateProcess: no shutdown code runs, as when the app crashes.
        app.kill().unwrap();
        app.wait().unwrap();
        started.append(&mut shells);
        until_ended("everything the app started to end", &started);
    }
}
