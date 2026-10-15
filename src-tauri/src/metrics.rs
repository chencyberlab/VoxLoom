//! Linux RSS sampling; shared pages are conservatively counted per process.
use std::{collections::HashSet, fs};
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
