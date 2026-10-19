//! Child processes. A killed host must not leave a recorder or model resident,
//! and a desktop app must not flash console windows.
//!
//! Linux: every child leads its own process group and gets SIGTERM if the host
//! dies; `kill_tree` kills the group.
//! Windows: children start without a console window. `kill_tree` ends the
//! process and its descendants. Job Objects, which also cover a crashed host,
//! come with the bundled runtime.
use std::{ffi::OsStr, process::Command};

/// A supervised child: its whole tree can be killed with `kill_tree`.
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
    #[cfg(windows)]
    c.env("PYTHONUTF8", "1");
    c
}

/// Kills a child started with `command` and everything it started.
pub fn kill_tree(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = hidden("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(windows)]
fn hide_console(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}
