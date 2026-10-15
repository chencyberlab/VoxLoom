#!/usr/bin/env python3
"""Fail on remote assets, runtime networking, permissive IPC or update plugins."""
import json
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
errors = []
config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
csp = config["app"]["security"]["csp"]
for required in ["connect-src ipc: http://ipc.localhost", "object-src 'none'", "frame-src 'none'"]:
    if required not in csp:
        errors.append(f"Missing CSP restriction: {required}")
if config["app"]["security"]["assetProtocol"]["scope"]:
    errors.append("Media scope must start empty and allow host-owned exact files only")
for folder in ["src", "worker", "src-tauri/src"]:
    for path in (root / folder).rglob("*"):
        if not path.is_file() or path.suffix not in [".ts", ".tsx", ".css", ".py", ".rs"] or path.name == "provision.py":
            continue
        text = path.read_text()
        if re.search(r'\b(fetch\s*\(|XMLHttpRequest|WebSocket\s*\(|requests\.(get|post)|urllib\.request|TcpListener|0\.0\.0\.0)', text):
            errors.append(f"Runtime network API in {path.relative_to(root)}")
        if re.search(r'@import\s+url\([\'"]?https?:|(?:src|href)=[\'"]https?://[^\'"]+\.(?:js|css|woff)', text):
            errors.append(f"Remote frontend asset in {path.relative_to(root)}")
for path in [root / "package.json", root / "src-tauri/Cargo.toml"]:
    if re.search(r'(tauri-plugin-updater|sentry|posthog|analytics)', path.read_text()):
        errors.append(f"Unexpected network integration in {path.name}")
print("\n".join(errors) if errors else "PASS: local assets, restricted CSP/media scope, no runtime networking or telemetry")
sys.exit(bool(errors))
