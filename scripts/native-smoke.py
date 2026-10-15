#!/usr/bin/env python3
"""Real Tauri/WebKit smoke in an isolated Xvfb display; no private screen read.

Tests main -> floating -> main -> Quit and asserts all observed descendants exit.
Run xvfb-run -a python3 scripts/native-smoke.py <native executable>.
"""
import ctypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import re

sys.path.insert(0, str(Path(__file__).resolve().parent))
from importlib.machinery import SourceFileLoader
measure = SourceFileLoader("measure", str(Path(__file__).with_name("measure-tree.py"))).load_module()

x11 = ctypes.CDLL("libX11.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XFlush.argtypes = [ctypes.c_void_p]
xtst = ctypes.CDLL("libXtst.so.6")
xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XKeysymToKeycode.restype = ctypes.c_uint
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
display = x11.XOpenDisplay(None)
assert display, "No isolated X display"


def click(x, y):
    xtst.XTestFakeMotionEvent(display, -1, x, y, 0)
    xtst.XTestFakeButtonEvent(display, 1, 1, 0)
    xtst.XTestFakeButtonEvent(display, 1, 0, 0)
    x11.XFlush(display)
    time.sleep(1.2)


def windows():
    return subprocess.check_output(["xwininfo", "-root", "-tree"], text=True)


def descendants(root):
    processes = {}
    for entry in Path('/proc').iterdir():
        if entry.name.isdigit():
            try:
                fields = dict(line.split(':', 1) for line in (entry/'status').read_text().splitlines() if ':' in line)
                processes[int(entry.name)] = int(fields['PPid'])
            except (OSError, KeyError, ValueError):
                pass
    found = {root}
    for _ in range(8):
        found |= {pid for pid, parent in processes.items() if parent in found}
    return found


with tempfile.TemporaryDirectory(prefix="voxloom-native-") as directory:
    env = dict(os.environ, VOXLOOM_DATA_DIR=directory, VOXLOOM_CACHE_DIR=directory + "/cache")
    log = open("/tmp/voxloom-native-smoke.log", "w")
    app = subprocess.Popen([sys.argv[1]], env=env, stdout=log, stderr=log)
    peak = 0
    observed = set()
    try:
        for _ in range(60):
            assert app.poll() is None, "Native app exited before rendering"
            peak = max(peak, measure.sample(app.pid)[0])
            observed |= descendants(app.pid)
            time.sleep(.1)
        assert '"VoxLoom"' in windows(), windows()
        subprocess.run(["import", "-window", "root", "/tmp/voxloom-native-setup.png"], check=True)
        # First run opens model setup. Dismiss without downloading anything.
        key = x11.XKeysymToKeycode(display, 0xff1b)
        xtst.XTestFakeKeyEvent(display, key, 1, 0)
        xtst.XTestFakeKeyEvent(display, key, 0, 0)
        x11.XFlush(display)
        time.sleep(.7)
        subprocess.run(["import", "-window", "root", "/tmp/voxloom-native-home.png"], check=True)
        click(1185, 34)
        assert 'VoxLoom · Recorder' in windows(), windows()
        assert '390x92' in windows(), windows()
        subprocess.run(["import", "-window", "root", "/tmp/voxloom-native-mini.png"], check=True)
        click(335, 46)
        peak = max(peak, measure.sample(app.pid)[0])
        # Main restored; explicit Quit in the sidebar.
        click(179, 791)
        app.wait(timeout=10)
        for _ in range(40):
            alive = []
            for pid in observed:
                try:
                    status = Path(f'/proc/{pid}/status').read_text()
                    if not re.search(r'^State:\s+Z', status, re.M):
                        alive.append(pid)
                except FileNotFoundError:
                    pass
            if not alive:
                break
            time.sleep(.1)
        assert not alive, f'Native processes remained after Quit: {alive}'
        print({"native_launch": "PASS", "floating_expand_shrink": "PASS", "quit_exit_code": app.returncode,
               "idle_single_webview_peak_sample_rss_kib": peak, "observed_processes_released": len(observed)})
    finally:
        if app.poll() is None:
            app.terminate()
            app.wait(timeout=10)
