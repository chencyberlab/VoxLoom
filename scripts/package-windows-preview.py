#!/usr/bin/env python3
"""Assembles the Windows preview: a folder that runs without installing.

It holds the release app and every file the installer bundles: bundle.resources
in src-tauri/tauri.conf.json and tauri.windows.conf.json, so the zip, the
installer and what the app expects next to voxloom.exe never drift apart. That
includes the runtime staged in src-tauri (engines/, python/, licenses/).
Run on Windows after `npx tauri build`, with the runtime staged:
  python scripts/package-windows-preview.py <output folder>
It writes <output>/VoxLoom-<version>-windows-x64-preview/ and a .zip of it.
"""
import json
from pathlib import Path
import shutil
import sys

repo = Path(__file__).resolve().parent.parent
tauri = repo / "src-tauri"
version = (repo / "VERSION").read_text(encoding="utf-8").strip()
name = f"VoxLoom-{version}-windows-x64-preview"
out = Path(sys.argv[1]).resolve()
folder = out / name
shutil.rmtree(folder, ignore_errors=True)
folder.mkdir(parents=True)

shutil.copy2(tauri / "target" / "release" / "voxloom.exe", folder / "voxloom.exe")
def resources(config):
    return json.loads((tauri / config).read_text(encoding="utf-8"))["bundle"]["resources"]


resources = {**resources("tauri.conf.json"), **resources("tauri.windows.conf.json")}
for source, target in resources.items():
    source, target = tauri / source, folder / target
    if source.is_dir():
        shutil.copytree(source, target, dirs_exist_ok=True)
    else:
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
(folder / "START HERE.txt").write_text(f"""VoxLoom {version}, Windows preview

Open voxloom.exe in this folder. Keep the other files beside it.

The first time, VoxLoom asks to download its speech and search models (a few GB).
Your recordings and models are kept in %LOCALAPPDATA%\\org.voxloom.desktop,
not in this folder.

VoxLoom records your microphone and, for calls and videos, the sound your
computer plays. Windows lets desktop apps use the microphone only while
Settings > Privacy & security > Microphone > "Let desktop apps access your
microphone" is on.

To install VoxLoom instead, with a Start menu entry and an uninstaller, use
VoxLoom-{version}-windows-x64-setup.exe from the same release. Windows may say
the app is from an unknown publisher because it is not signed yet: choose More
info, then Run anyway.

Licences for the software included are in the licenses folder.
""", encoding="utf-8", newline="\r\n")

archive = shutil.make_archive(str(out / name), "zip", root_dir=out, base_dir=name)
print(folder)
print(archive)
