#!/usr/bin/env python3
"""Sample summed Linux RSS for a command and its full process tree.

RSS includes shared pages counted once per process, intentionally conservative.
No application content, environment secrets or command arguments are collected.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time


def sample(root):
    processes = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            status = (entry / "status").read_text()
            fields = dict(line.split(":", 1) for line in status.splitlines() if ":" in line)
            processes[int(entry.name)] = (int(fields["PPid"]), int(fields.get("VmRSS", "0 kB").split()[0]))
        except (OSError, ValueError, KeyError):
            pass
    selected = {root}
    changed = True
    while changed:
        new = {pid for pid, (parent, _) in processes.items() if parent in selected}
        changed = bool(new - selected)
        selected |= new
    return sum(processes.get(pid, (0, 0))[1] for pid in selected), len(selected)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--label", default="run")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    process = subprocess.Popen(command)
    peak = 0
    maximum_processes = 0
    start = time.monotonic()
    while process.poll() is None:
        rss, count = sample(process.pid)
        peak = max(peak, rss)
        maximum_processes = max(count, maximum_processes)
        time.sleep(.05)
    result = {"label":args.label,"peak_process_tree_rss_kib":peak,"maximum_processes":maximum_processes,
              "seconds":time.monotonic()-start,"exit_code":process.returncode,"sample_interval_ms":50}
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    raise SystemExit(process.returncode)


if __name__ == "__main__":
    main()
