#!/usr/bin/env python3
"""Windows launch smoke with throwaway data; no private screen read.

Starts the app, waits for the library and the VoxLoom window, closes the
window as a user would, then checks the app exits with code 0 and every
process it started (WebView2, probes, workers) is gone.
Run python scripts/native-smoke-windows.py <voxloom.exe>.
"""
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
WM_CLOSE = 0x0010
STILL_ACTIVE = 259


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD),
                ("cntThreads", wintypes.DWORD), ("th32ParentProcessID", wintypes.DWORD),
                ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD), ("szExeFile", ctypes.c_wchar * 260)]


kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
kernel32.OpenProcess.restype = wintypes.HANDLE
user32.FindWindowW.restype = wintypes.HWND


def processes():
    """{pid: (parent pid, executable name)} for every running process."""
    snapshot = kernel32.CreateToolhelp32Snapshot(0x2, 0)
    entry = PROCESSENTRY32W(dwSize=ctypes.sizeof(PROCESSENTRY32W))
    found = {}
    ok = kernel32.Process32FirstW(snapshot, ctypes.byref(entry))
    while ok:
        found[entry.th32ProcessID] = (entry.th32ParentProcessID, entry.szExeFile)
        ok = kernel32.Process32NextW(snapshot, ctypes.byref(entry))
    kernel32.CloseHandle(snapshot)
    return found


def descendants(root):
    table = processes()
    found = {root}
    for _ in range(8):
        found |= {pid for pid, (parent, _) in table.items() if parent in found}
    return {pid: table[pid][1] for pid in found if pid in table}


def alive(pid):
    handle = kernel32.OpenProcess(0x1000, False, pid)
    if not handle:
        return False
    code = wintypes.DWORD()
    kernel32.GetExitCodeProcess(handle, ctypes.byref(code))
    kernel32.CloseHandle(handle)
    return code.value == STILL_ACTIVE


with tempfile.TemporaryDirectory(prefix="voxloom-native-", ignore_cleanup_errors=True) as directory:
    env = dict(os.environ, VOXLOOM_DATA_DIR=directory, VOXLOOM_CACHE_DIR=str(Path(directory) / "cache"))
    log_path = Path(directory).parent / "voxloom-native-smoke.log"
    with open(log_path, "w", encoding="utf-8") as log:
        app = subprocess.Popen([sys.argv[1]], env=env, stdout=log, stderr=log)
    observed = {}
    try:
        window = None
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline and not (window and (Path(directory) / "library.db").is_file()):
            assert app.poll() is None, f"App exited before its window appeared (code {app.returncode}). Log: {log_path}"
            window = user32.FindWindowW(None, "VoxLoom")
            observed |= descendants(app.pid)
            time.sleep(0.25)
        assert window, f"No VoxLoom window within 90 s. Log: {log_path}"
        assert (Path(directory) / "library.db").is_file(), "The library was not created"
        # Let the UI finish its first-run calls (runtime probe, settings, devices).
        for _ in range(40):
            assert app.poll() is None, f"App exited while idle (code {app.returncode}). Log: {log_path}"
            observed |= descendants(app.pid)
            time.sleep(0.25)
        user32.PostMessageW(window, WM_CLOSE, 0, 0)
        code = app.wait(timeout=30)
        assert code == 0, f"Close exited with code {code}. Log: {log_path}"
        remaining = {}
        for _ in range(60):
            remaining = {pid: name for pid, name in observed.items() if pid != app.pid and alive(pid)}
            if not remaining:
                break
            time.sleep(0.25)
        assert not remaining, f"Processes remained after close: {remaining}"
        print({"native_launch": "PASS", "window": "PASS", "close_exit_code": code,
               "observed_processes": sorted(set(observed.values())), "released": "PASS"})
    finally:
        if app.poll() is None:
            app.kill()
            app.wait(timeout=10)
