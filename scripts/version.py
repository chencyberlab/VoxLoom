#!/usr/bin/env python3
"""VoxLoom's version lives in VERSION. Everything else is a copy tools require.

  python3 scripts/version.py              check every copy matches VERSION (part of npm run check)
  python3 scripts/version.py set 0.1.9    write VERSION and every copy

The app's UI reads VERSION directly and tauri.conf.json reads package.json, so
the copies kept here are only those npm and Cargo insist on: package.json,
package-lock.json (two fields) and the voxloom package in Cargo.toml/Cargo.lock.
"""
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"\d+\.\d+\.\d+")

# (file, pattern whose group 2 is the version). Each must match exactly once.
COPIES = [
    ("package.json", r'(\A\{\s*"name": "voxloom",\s*"private": true,\s*"version": ")([^"]+)(")'),
    ("package-lock.json", r'(\A\{\s*"name": "voxloom",\s*"version": ")([^"]+)(")'),
    ("package-lock.json", r'("packages": \{\s*"": \{\s*"name": "voxloom",\s*"version": ")([^"]+)(")'),
    ("src-tauri/Cargo.toml", r'(\A\[package\]\s*name = "voxloom"\s*version = ")([^"]+)(")'),
    ("src-tauri/Cargo.lock", r'(\[\[package\]\]\s*name = "voxloom"\s*version = ")([^"]+)(")'),
]


def current():
    return (ROOT / "VERSION").read_text().strip()


def copies():
    for name, pattern in COPIES:
        text = (ROOT / name).read_text()
        found = list(re.finditer(pattern, text))
        if len(found) != 1:
            raise SystemExit(f"{name}: expected one version field, found {len(found)}. Update scripts/version.py.")
        yield name, pattern, text, found[0]


def check():
    version, problems = current(), []
    if not SEMVER.fullmatch(version):
        problems.append(f"VERSION: {version!r} is not X.Y.Z")
    for name, _, _, match in copies():
        if match[2] != version:
            problems.append(f"{name}: {match[2]} (VERSION says {version})")
    tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text()).get("version")
    if tauri != "../package.json":
        problems.append(f'src-tauri/tauri.conf.json: version is {tauri!r}; it should be "../package.json"')
    if problems:
        print("Version mismatch. Run: python3 scripts/version.py set " + version + "\n  " + "\n  ".join(problems))
        return 1
    print(f"PASS: version {version} everywhere")
    return 0


def set_version(version):
    if not SEMVER.fullmatch(version):
        raise SystemExit(f"{version!r} is not X.Y.Z")
    edits = {}
    for name, pattern, _, _ in copies():
        text = edits.get(name, (ROOT / name).read_text())
        edits[name] = re.sub(pattern, lambda m: m[1] + version + m[3], text, count=1)
    for name, text in edits.items():
        (ROOT / name).write_text(text)
    (ROOT / "VERSION").write_text(version + "\n")
    return check()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "set":
        sys.exit(set_version(sys.argv[2]))
    if len(sys.argv) == 1:
        sys.exit(check())
    raise SystemExit(__doc__)
