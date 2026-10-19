#!/usr/bin/env python3
"""Installs, launches and uninstalls the Windows installer in both install
modes, and checks that nothing is left but what the user chose to keep.

For each mode it installs silently and checks the files, shortcut and
uninstall entry. It launches the installed app (throwaway data) and runs the
bundled runtime from the install folder, which leaves Python caches there.
It then puts what a user's data folder holds in place (models, a recording,
the library, caches, browser data, Windows' microphone record) and
uninstalls: once keeping recordings, once with /DELETEDATA. Changes the
machine's VoxLoom install and data folder: run it on a disposable Windows
machine (CI), as an administrator.
Run python scripts/installer-smoke-windows.py <setup.exe> [programs list].
With a programs list path, it writes every program file the installer put
down (.exe, .dll, .pyd) with its SHA-256, for IT allow-listing.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import winreg

setup = Path(sys.argv[1]).resolve()
programs = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else None
scripts = Path(__file__).resolve().parent
local, roaming = Path(os.environ["LOCALAPPDATA"]), Path(os.environ["APPDATA"])
desktops = [Path(os.environ["USERPROFILE"]) / "Desktop" / "VoxLoom.lnk", Path(os.environ["PUBLIC"]) / "Desktop" / "VoxLoom.lnk"]
data = local / "org.voxloom.desktop"
UNINSTALL = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\VoxLoom"
REMEMBERED = r"Software\VoxLoom\VoxLoom"
MICROPHONE = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged"
MODES = [
    ("/CurrentUser", local / "VoxLoom", winreg.HKEY_CURRENT_USER, roaming / r"Microsoft\Windows\Start Menu\Programs"),
    ("/AllUsers", Path(os.environ["ProgramW6432"]) / "VoxLoom", winreg.HKEY_LOCAL_MACHINE,
     Path(os.environ["ProgramData"]) / r"Microsoft\Windows\Start Menu\Programs"),
]
INSTALLED = ["voxloom.exe", "uninstall.exe", "engines/whisper-cli.exe", "engines/llama-completion.exe", "engines/ffmpeg.exe",
             "python/python.exe", "worker/engine.py", "worker/notes.py", "models/manifest.json", "licenses/README.txt",
             "licenses/VERSIONS.txt", "LICENSE.txt"]
KEPT = ["library.db", "recordings/r1/s0000-mic-000000.flac"]
REMOVED = ["models/ggml-base.en.bin", "cache/job/request.json", "EBWebView/Default/Preferences"]


def key_exists(root, path):
    try:
        winreg.CloseKey(winreg.OpenKey(root, path))
        return True
    except OSError:
        return False


def until(what, left, seconds=180):
    deadline = time.time() + seconds
    while remaining := left():
        assert time.time() < deadline, f"{what}: left behind: {remaining}"
        time.sleep(1)


def run(*argv, timeout=600):
    result = subprocess.run([str(a) for a in argv], timeout=timeout)
    assert result.returncode == 0, f"{argv[0]} exited {result.returncode}"


results = {}
for mode, folder, root, start_menu in MODES:
    for delete_data in (False, True):
        case = f"{mode} {'/DELETEDATA' if delete_data else 'keeping recordings'}"
        run(setup, "/S", mode)
        missing = [part for part in INSTALLED if not (folder / part).is_file()]
        assert not missing, f"{case}: missing after install: {missing}"
        assert (start_menu / "VoxLoom.lnk").is_file(), f"{case}: no Start menu shortcut"
        assert key_exists(root, UNINSTALL), f"{case}: no uninstall entry"
        if programs and not programs.exists():
            files = sorted(p for p in folder.rglob("*") if p.suffix.lower() in {".exe", ".dll", ".pyd"})
            programs.write_text(
                f"Every program file {setup.name} installs, relative to the VoxLoom folder\n"
                "(C:\\Program Files\\VoxLoom or %LOCALAPPDATA%\\VoxLoom), with its SHA-256.\n\n"
                + "".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(folder)}\n" for p in files),
                encoding="utf-8", newline="\r\n")
        app = folder / "voxloom.exe"
        run(sys.executable, scripts / "native-smoke-windows.py", app, timeout=300)
        run(sys.executable, scripts / "windows-runtime-smoke.py", folder)
        for part in KEPT + REMOVED:
            (data / part).parent.mkdir(parents=True, exist_ok=True)
            (data / part).write_bytes(b"VoxLoom test")
        microphone = MICROPHONE + "\\" + str(app).replace("\\", "#")
        winreg.CloseKey(winreg.CreateKey(winreg.HKEY_CURRENT_USER, microphone))
        run(folder / "uninstall.exe", "/S", *(["/DELETEDATA"] if delete_data else []), timeout=300)

        def left():
            paths = [folder, start_menu / "VoxLoom.lnk", *desktops, roaming / "org.voxloom.desktop"]
            paths += [data] if delete_data else [data / part for part in REMOVED]
            keys = {"uninstall entry": (root, UNINSTALL), "remembered location": (root, REMEMBERED),
                    "installer language": (winreg.HKEY_CURRENT_USER, REMEMBERED),
                    "microphone record": (winreg.HKEY_CURRENT_USER, microphone)}
            return [str(p) for p in paths if p.exists()] + [name for name, (hive, key) in keys.items() if key_exists(hive, key)]
        # The uninstaller copies itself elsewhere and returns at once, so wait
        # for it to finish its work.
        until(case, left)
        if not delete_data:
            assert all((data / part).is_file() for part in KEPT), f"{case}: recordings or library not kept"
            assert sorted(p.name for p in data.iterdir()) == ["library.db", "recordings"], list(data.iterdir())
        results[case] = "clean"
print(json.dumps(results))
