#!/usr/bin/env python3
"""Checks a staged Windows runtime on Windows, with no model and no network.

Every bundled program starts; FFmpeg writes and reads the 30-second-style FLAC
segments capture produces, from raw PCM on stdin, in a folder with spaces and
non-ASCII letters; the bundled Python loads LiteRT and the worker modules.
Run python scripts/windows-runtime-smoke.py <folder with engines/ and python/>.
"""
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

root = Path(sys.argv[1]).resolve()
worker = Path(__file__).resolve().parent.parent / "worker"
engines, python = root / "engines", root / "python" / "python.exe"
env = dict(os.environ, PYTHONUTF8="1")
results = {}


def run(*argv, data=None):
    return subprocess.run([str(a) for a in argv], input=data, capture_output=True, timeout=300, env=env)


for name in ["whisper-cli", "whisper-cli-generic", "llama-completion", "llama-completion-generic", "ffmpeg", "ffprobe"]:
    assert (engines / f"{name}.exe").is_file(), f"missing {name}.exe"
for name, args in [("ffmpeg", ["-version"]), ("ffprobe", ["-version"]), ("whisper-cli", ["--help"]),
                   ("whisper-cli-generic", ["--help"]), ("llama-completion", ["--version"]),
                   ("llama-completion-generic", ["--version"])]:
    result = run(engines / f"{name}.exe", *args)
    assert result.returncode == 0, f"{name} {args} exited {result.returncode}: {result.stderr[-400:]!r}"
    results[name] = "starts"

with tempfile.TemporaryDirectory() as directory:
    folder = Path(directory) / "Zoë's café recordings"
    folder.mkdir()
    # Three seconds of a 440 Hz tone, the way Windows capture will feed FFmpeg.
    pcm = b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 16000))) for i in range(48000))
    result = run(engines / "ffmpeg.exe", "-hide_banner", "-loglevel", "info", "-y", "-f", "s16le", "-ar", "16000", "-ac", "1",
                 "-i", "pipe:0", "-af", "aresample=async=1:first_pts=0,astats=metadata=1:reset=1,ametadata@mic=print:key=lavfi.astats.Overall.RMS_level",
                 "-ac", "1", "-ar", "16000", "-c:a", "flac", "-f", "segment", "-segment_time", "1",
                 "-segment_format_options", "flush_packets=1", "-reset_timestamps", "1", folder / "s0000-mic-%06d.flac", data=pcm)
    assert result.returncode == 0, result.stderr[-600:]
    assert b"lavfi.astats.Overall.RMS_level=" in result.stderr, "no level metering on stderr"
    segments = sorted(folder.glob("s0000-mic-*.flac"))
    assert len(segments) >= 3, [p.name for p in segments]
    # Segment headers can lack a duration (the app counts decoded samples instead),
    # and cuts fall on FLAC frames (1152 samples at 16 kHz), so a closed segment
    # runs up to a frame past one second. Together they must hold every sample.
    samples = []
    for segment in segments:
        decoded = run(engines / "ffmpeg.exe", "-v", "error", "-i", segment, "-ar", "16000", "-ac", "1", "-f", "s16le", "pipe:1")
        assert decoded.returncode == 0, (segment.name, decoded.stderr[-400:])
        samples.append(len(decoded.stdout) // 2)
    assert sum(samples) == 48000 and all(16000 <= n < 17152 for n in samples[:2]), samples
    whole = folder / "whole.flac"
    result = run(engines / "ffmpeg.exe", "-v", "error", "-f", "s16le", "-ar", "16000", "-ac", "1", "-i", "pipe:0", "-c:a", "flac", whole, data=pcm)
    assert result.returncode == 0, result.stderr[-600:]
    probe = run(engines / "ffprobe.exe", "-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1", whole)
    assert probe.returncode == 0 and 2.9 < float(probe.stdout) < 3.1, (probe.returncode, probe.stdout, probe.stderr)
    results["ffmpeg_segments_unicode_path"] = len(segments)

result = run(python, "-I", "-c", "import importlib.metadata as m; assert m.version('litert-lm') == '0.18.0'; "
             "from litert_lm import Backend, EmbeddingEngine, Content; assert callable(EmbeddingEngine.compute_embedding)")
assert result.returncode == 0, result.stderr[-800:]
results["python_litert"] = "loads"
# The worker and Word export, started the way the app starts them.
with tempfile.TemporaryDirectory() as directory:
    request, response = Path(directory) / "request.json", Path(directory) / "response.jsonl"
    request.write_text(json.dumps({"kind": "answer", "evidence": []}), encoding="utf-8")
    result = run(python, worker / "engine.py", "--request", request, "--response", response)
    assert result.returncode == 0, result.stderr[-800:]
    assert "no supporting evidence" in response.read_text(encoding="utf-8"), response.read_text(encoding="utf-8")
    target = Path(directory) / "Zoë's notes.docx"
    result = run(python, worker / "export_docx.py", target, data=json.dumps({"title": "Café", "text": "## Overview\nZoë agreed."}).encode())
    assert result.returncode == 0 and target.read_bytes().startswith(b"PK"), result.stderr[-800:]
results["python_worker_and_word_export"] = "run"
print(json.dumps(results))
