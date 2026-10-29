#!/usr/bin/env python3
"""The window's close button, in the real app (issue #16).

Usage: xvfb-run -a python3 scripts/native-close-check.py <path to voxloom>

Throwaway data. A pending summary job whose "worker" only sleeps keeps VoxLoom
busy from start-up. Checks: an idle close quits at once; a busy close keeps the
window open behind "Quit VoxLoom?" and "Quit anyway" stops the worker, leaving
the job to run again; a busy close while the page is still starting (it cannot
answer yet) quits by itself after a few seconds instead of being lost.
"""
import ctypes
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time

x11 = ctypes.CDLL("libX11.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x11.XInternAtom.restype = ctypes.c_ulong
x11.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.c_void_p]
x11.XFlush.argtypes = [ctypes.c_void_p]
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XKeysymToKeycode.restype = ctypes.c_uint
xtst = ctypes.CDLL("libXtst.so.6")
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
display = x11.XOpenDisplay(None)
assert display, "No isolated X display"


class ClientMessage(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
                ("format", ctypes.c_int), ("data", ctypes.c_long * 5), ("pad", ctypes.c_long * 12)]


def main_window():
    tree = subprocess.check_output(["xwininfo", "-root", "-tree"], text=True)
    return next((int(line.split()[0], 16) for line in tree.splitlines() if '"VoxLoom": (' in line), None)


def close():
    """What a window manager sends for the title bar's close button."""
    event = ClientMessage(type=33, window=main_window(), format=32,
                          message_type=x11.XInternAtom(display, b"WM_PROTOCOLS", 0))
    event.data[0] = x11.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
    x11.XSendEvent(display, event.window, 0, 0, ctypes.byref(event))
    x11.XFlush(display)


def key(sym):
    code = x11.XKeysymToKeycode(display, sym)
    xtst.XTestFakeKeyEvent(display, code, 1, 0)
    xtst.XTestFakeKeyEvent(display, code, 0, 0)
    x11.XFlush(display)
    time.sleep(.5)


def worker_running():
    return subprocess.run(["pgrep", "-fx", "sleep 117"], stdout=subprocess.DEVNULL).returncode == 0


def job_status(directory):
    with sqlite3.connect(Path(directory, "library.db")) as db:
        return db.execute("SELECT status FROM jobs WHERE id='j'").fetchone()[0]


assert not worker_running(), "Stop the other 'sleep 117' first"
result = {}
with tempfile.TemporaryDirectory(prefix="voxloom-close-") as directory:
    env = dict(os.environ, VOXLOOM_DATA_DIR=directory, VOXLOOM_CACHE_DIR=directory + "/cache")
    log = open(Path(directory, "log"), "w")
    launch = lambda: subprocess.Popen([sys.argv[1]], env=env, stdout=log, stderr=log)
    app = launch()
    try:
        time.sleep(6)
        started = time.time()
        close()
        app.wait(timeout=10)
        result["idle_close_quit_s"] = round(time.time() - started, 1)
        # Stands in for Python only for the worker, so start-up checks still answer.
        worker = Path(directory, "fake-python")
        worker.write_text('#!/bin/sh\ncase "$1" in */engine.py) exec sleep 117;; esac\nexec python3 "$@"\n')
        worker.chmod(0o700)
        with sqlite3.connect(Path(directory, "library.db")) as db:
            db.execute("INSERT OR REPLACE INTO settings VALUES ('python', ?)", [str(worker)])
            db.execute("INSERT INTO meetings (id,title,created_at) VALUES ('m','Session',1)")
            db.execute("INSERT INTO jobs (id,meeting_id,kind,status,created_at) VALUES ('j','m','summarize','pending',1)")

        app = launch()
        time.sleep(8)
        assert worker_running(), "The busy job did not start"
        close()
        time.sleep(5)
        assert app.poll() is None, "A busy close whose question showed must keep VoxLoom open"
        subprocess.run(["import", "-window", "root", "/tmp/voxloom-close-dialog.png"], check=True)
        key(0xff09)  # Tab from "Keep VoxLoom open" to "Quit anyway"
        key(0xff0d)
        app.wait(timeout=10)
        result["busy_close_kept_open"] = "PASS"
        result["quit_anyway_stopped_worker"] = not worker_running()
        result["job_after_quit"] = job_status(directory)

        app = launch()
        while not (worker_running() and main_window()):
            assert app.poll() is None, "VoxLoom exited at start-up"
            time.sleep(.02)
        started = time.time()
        close()
        app.wait(timeout=10)
        result["starting_page_close_quit_s"] = round(time.time() - started, 1)
        result["starting_page_close_stopped_worker"] = not worker_running()
        result["job_after_start_up_close"] = job_status(directory)
    finally:
        if app.poll() is None:
            app.kill()
        subprocess.run(["pkill", "-fx", "sleep 117"])
print(result)
assert result["quit_anyway_stopped_worker"] and result["starting_page_close_stopped_worker"], result
assert result["job_after_quit"] == result["job_after_start_up_close"] == "pending", result
