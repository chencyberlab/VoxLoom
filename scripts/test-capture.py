#!/usr/bin/env python3
"""Explicit synthetic Pulse/PipeWire fixture, no physical mic or speaker audio.

Creates temporary test endpoints, never changes a default route, then removes
them. Must be invoked explicitly; never runs when the application launches.
"""
import os
from pathlib import Path
import subprocess
import shutil

root = Path(__file__).resolve().parents[1]
modules = []
default_sink = subprocess.check_output(["pactl", "get-default-sink"], text=True).strip()
default_source = subprocess.check_output(["pactl", "get-default-source"], text=True).strip()
try:
    modules.append(subprocess.check_output(["pactl", "load-module", "module-null-sink", "sink_name=voxloom_fixture", "sink_properties=device.description=VoxLoom_Test_Only"], text=True).strip())
    modules.append(subprocess.check_output(["pactl", "load-module", "module-remap-source", "master=voxloom_fixture.monitor", "source_name=voxloom_fixture_mic", "source_properties=device.description=VoxLoom_Test_Mic"], text=True).strip())
    # Fixtures cannot accidentally become the user's output or input.
    assert subprocess.check_output(["pactl", "get-default-sink"], text=True).strip() == default_sink
    assert subprocess.check_output(["pactl", "get-default-source"], text=True).strip() == default_source
    env = dict(os.environ, VOXLOOM_TEST_ROOT=str(root), VOXLOOM_TEST_MIC="voxloom_fixture_mic", VOXLOOM_TEST_MONITOR="voxloom_fixture.monitor")
    subprocess.run([str(root / ".toolchain/cargo/bin/cargo") if (root / ".toolchain/cargo/bin/cargo").exists() else (shutil.which("cargo") or "cargo"), "test", "--manifest-path", "src-tauri/Cargo.toml", "--no-default-features", "--offline", "--test", "workflow", "real_pulse", "--", "--ignored", "--nocapture"], cwd=root, env=env, check=True)
finally:
    for module in reversed(modules):
        subprocess.run(["pactl", "unload-module", module], check=False)
