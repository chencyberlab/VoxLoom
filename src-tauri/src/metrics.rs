//! Memory of the host's process tree, for job metrics. Linux reads /proc;
//! Windows sums the working sets of every process in the app's Job Object.
//! Shared pages are conservatively counted per process on both.
#[cfg(target_os = "linux")]
use std::{collections::HashSet, fs};
#[cfg(target_os = "linux")]
pub fn tree_pids(root: u32) -> Vec<u32> {
    let mut seen = HashSet::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            for task in tasks.flatten() {
                if let Ok(children) = fs::read_to_string(task.path().join("children")) {
                    queue.extend(
                        children
                            .split_whitespace()
                            .filter_map(|v| v.parse::<u32>().ok()),
                    );
                }
            }
        }
    }
    seen.into_iter().collect()
}
#[cfg(target_os = "linux")]
pub fn tree_rss_kib(root: u32) -> u64 {
    tree_pids(root)
        .into_iter()
        .filter_map(|pid| fs::read_to_string(format!("/proc/{pid}/status")).ok())
        .map(|status| {
            status
                .lines()
                .find_map(|line| {
                    line.strip_prefix("VmRSS:")
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|s| s.parse::<u64>().ok())
                })
                .unwrap_or(0)
        })
        .sum()
}
#[cfg(windows)]
pub fn tree_rss_kib(root: u32) -> u64 {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
        },
    };
    let pids = crate::process::contained_pids()
        .filter(|_| root == std::process::id())
        .unwrap_or_else(|| vec![root]);
    pids.into_iter()
        .map(|pid| unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if process.is_null() {
                return 0;
            }
            let mut counters = PROCESS_MEMORY_COUNTERS {
                cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            let ok = K32GetProcessMemoryInfo(process, &mut counters, counters.cb) != 0;
            CloseHandle(process);
            if ok {
                counters.WorkingSetSize as u64 / 1024
            } else {
                0
            }
        })
        .sum()
}
#[cfg(not(any(target_os = "linux", windows)))]
pub fn tree_rss_kib(_root: u32) -> u64 {
    0
}
