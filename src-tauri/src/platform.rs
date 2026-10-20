//! Operating-system differences outside audio capture and child processes.
use crate::{err, Result};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

/// The library and cache folders. VOXLOOM_DATA_DIR / VOXLOOM_CACHE_DIR win.
/// Windows uses Local AppData (Roaming can be synced to a server on company
/// networks) under the app's identifier: the per-user installer puts the
/// program in %LOCALAPPDATA%\Programs\VoxLoom, and its uninstaller keeps the
/// library and recordings here unless the user asks (windows/hooks.nsh).
pub fn data_dirs() -> Result<(PathBuf, PathBuf)> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    #[cfg(unix)]
    let (data, cache) = {
        let home = var("HOME").ok_or("HOME is not set")?;
        (
            var("XDG_DATA_HOME")
                .unwrap_or_else(|| home.join(".local/share"))
                .join("voxloom"),
            var("XDG_CACHE_HOME")
                .unwrap_or_else(|| home.join(".cache"))
                .join("voxloom"),
        )
    };
    #[cfg(windows)]
    let (data, cache) = {
        let local = var("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
        let library = local.join("org.voxloom.desktop");
        (library.clone(), library.join("cache"))
    };
    Ok((
        var("VOXLOOM_DATA_DIR").unwrap_or(data),
        var("VOXLOOM_CACHE_DIR").unwrap_or(cache),
    ))
}

/// Files the app creates are readable only by the user. On Windows the
/// per-user AppData folders are already private to the account.
pub fn restrict_new_files() {
    #[cfg(unix)]
    unsafe {
        libc::umask(0o077);
    }
}

pub fn private_dir(p: &Path) -> Result<()> {
    fs::create_dir_all(p).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    Ok(())
}

pub fn private_file(p: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o600)).map_err(err)?;
    }
    #[cfg(windows)]
    let _ = p;
    Ok(())
}

/// Bytes available to the user on the volume holding `path`, or `None` when
/// the volume could not be queried. A full disk is `Some(0)`, so callers can
/// tell "no room" from "unknown" and refuse to record on the former.
pub fn free_disk_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        #[allow(clippy::unnecessary_cast)]
        Some((info.f_bavail as u64).saturating_mul(info.f_frsize as u64))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(available)
    }
}

/// Whether this CPU runs the AVX2-optimised engine builds.
pub fn optimized_cpu() -> bool {
    #[cfg(target_os = "linux")]
    {
        fs::read_to_string("/proc/cpuinfo")
            .map(|s| {
                ["avx2", "fma", "f16c", "bmi2"]
                    .iter()
                    .all(|feature| s.split_whitespace().any(|word| word == *feature))
            })
            .unwrap_or(false)
    }
    #[cfg(all(not(target_os = "linux"), target_arch = "x86_64"))]
    {
        std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("f16c")
            && std::arch::is_x86_feature_detected!("bmi2")
    }
    #[cfg(all(not(target_os = "linux"), not(target_arch = "x86_64")))]
    {
        false
    }
}

/// An executable's file name on this platform.
pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The interpreter inside a Python virtual environment.
pub fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// Windows: puts the bundled FFmpeg and engines first on PATH, for the app and
/// every worker it starts. `resources/engines` is the installed layout;
/// `src-tauri/engines` is a checkout's staged copy (debug builds).
/// Linux keeps the system FFmpeg.
pub fn use_bundled_tools(resources: &Path) {
    #[cfg(windows)]
    {
        let bundled = [
            resources.join("engines"),
            resources.join("src-tauri").join("engines"),
        ];
        let mut paths: Vec<PathBuf> = bundled.into_iter().filter(|p| p.is_dir()).collect();
        if paths.is_empty() {
            return;
        }
        paths.extend(
            std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default(),
        );
        if let Ok(joined) = std::env::join_paths(paths) {
            // Before any worker starts; Windows environment calls are thread-safe.
            std::env::set_var("PATH", joined);
        }
    }
    #[cfg(not(windows))]
    let _ = resources;
}

/// Opens a web page in the user's browser. Windows: Explorer hands the page
/// to the running shell, so the browser does not start inside the app's Job
/// Object and close with VoxLoom. Linux: xdg-open runs without the process
/// group and parent-death signal of supervised children, for the same reason.
pub fn open_in_browser(url: &str) -> Result<()> {
    let mut child = crate::process::hidden(if cfg!(windows) { "explorer" } else { "xdg-open" })
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Your web browser could not be opened.".to_string())?;
    // xdg-open can wait for the browser; reap it without holding up the app.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Whether `program` can be started by name from PATH.
pub fn on_path(program: impl Into<OsString>) -> bool {
    crate::process::hidden(if cfg!(windows) { "where" } else { "which" })
        .arg(program.into())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_is_unknown_for_a_missing_volume_and_measured_for_a_real_one() {
        assert_eq!(
            free_disk_bytes(Path::new("/definitely/not/a/voxloom/volume")),
            None
        );
        assert!(free_disk_bytes(&std::env::temp_dir()).is_some());
    }
}
