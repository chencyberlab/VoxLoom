#!/usr/bin/env python3
"""Real Tauri/WebKit smoke in an isolated Xvfb display; no private screen read.

Tests main -> floating bubble (drag, bar opening away from the screen edge) -> main -> Quit and asserts all observed descendants exit.
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


def drag(start, end):
    # About 16 px per event, as a hand moves a mouse.
    steps = max(4, max(abs(end[0] - start[0]), abs(end[1] - start[1])) // 16)
    xtst.XTestFakeMotionEvent(display, -1, start[0], start[1], 0)
    xtst.XTestFakeButtonEvent(display, 1, 1, 0)
    x11.XFlush(display)
    time.sleep(.3)
    for step in range(1, steps + 1):
        x = start[0] + (end[0] - start[0]) * step // steps
        y = start[1] + (end[1] - start[1]) * step // steps
        xtst.XTestFakeMotionEvent(display, -1, x, y, 0)
        x11.XFlush(display)
        time.sleep(.03)
    xtst.XTestFakeButtonEvent(display, 1, 0, 0)
    x11.XFlush(display)
    time.sleep(1.2)


def windows():
    return subprocess.check_output(["xwininfo", "-root", "-tree"], text=True)


def shows(*texts, seconds=10):
    deadline = time.time() + seconds
    while not all(text in windows() for text in texts):
        if time.time() > deadline:
            return False
        time.sleep(.2)
    return True


def origin(title):
    """Top-left corner of the window whose line in `windows()` names `title`."""
    for line in windows().splitlines():
        if title in line:
            found = re.search(r"\d+x\d+\+(-?\d+)\+(-?\d+)", line)
            if found:
                return int(found.group(1)), int(found.group(2))
    raise AssertionError(windows())


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
        # On a cold machine the page can still be loading, and the key or the
        # click is lost: try again, but only once the window has had time to
        # float, since a second click could land on the floating recorder.
        key = x11.XKeysymToKeycode(display, 0xff1b)
        for attempt in range(3):
            xtst.XTestFakeKeyEvent(display, key, 1, 0)
            xtst.XTestFakeKeyEvent(display, key, 0, 0)
            x11.XFlush(display)
            time.sleep(.7)
            if attempt == 0:
                subprocess.run(["import", "-window", "root", "/tmp/voxloom-native-home.png"], check=True)
            click(1185, 34)
            if shows('VoxLoom · Recorder', '64x64'):
                break
        assert 'VoxLoom · Recorder' in windows(), windows()
        assert '64x64' in windows(), windows()
        subprocess.run(["import", "-window", "root", "/tmp/voxloom-native-mini.png"], check=True)
        # The floating bubble moves with a drag, which opens nothing.
        drag((32, 32), (232, 132))
        assert shows('64x64+200+100'), windows()
        # Near the right edge its bar (200x64) opens leftwards, so the bubble's
        # tile stays put; closing the bar puts the bubble back where it was.
        drag((232, 132), (1232, 132))
        assert shows('64x64+1200+100'), windows()
        click(1232, 132)
        assert shows('200x64+1064+100'), windows()
        subprocess.run(["import", "-window", "root", "/tmp/voxloom-native-bar.png"], check=True)
        click(1232, 132)
        assert shows('64x64+1200+100'), windows()
        click(1232, 132)
        assert shows('200x64+1064+100'), windows()
        # The leftward bar is mirrored: Expand sits 52 px from its left edge.
        click(1064 + 52, 132)
        assert shows('"VoxLoom"', '1240x820'), windows()
        # Back from the screen edge, the studio opens centred and whole.
        left, top = origin('"VoxLoom"')
        assert (left, top) == (20, 102), windows()
        peak = max(peak, measure.sample(app.pid)[0])
        # Main restored; explicit Quit in the sidebar.
        for attempt in range(2):
            click(left + 179, top + 791)
            try:
                app.wait(timeout=10)
                break
            except subprocess.TimeoutExpired:
                pass
        assert app.poll() is not None, "Quit did not close the app"
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
