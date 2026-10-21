#!/usr/bin/env python3
"""Transcribe a real recording exactly as the app does after Stop.

Reads every capture file in a recording folder (sNNNN-mic-NNNNNN.flac,
sNNNN-system-NNNNNN.flac, with each session's offset from session-NNNN.json),
runs one transcription pass over all of it, and writes the rows and a summary
as JSON. With --stop-after N the pass is stopped after N chunks and started
again from its checkpoint, as when a new recording or Quit interrupts it.
Never downloads anything.

  python3 scripts/transcribe-recording.py --recording DIR --model ggml-base.en.bin --whisper-cli whisper-cli --out rows.json
"""
import argparse
import json
from pathlib import Path
import re
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "worker"))
import engine

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--recording", type=Path, required=True, help="folder with sNNNN-<source>-NNNNNN.flac files")
parser.add_argument("--model", type=Path, required=True)
parser.add_argument("--whisper-cli", default=str(ROOT / ".runtimes/whisper-build/bin/whisper-cli"))
parser.add_argument("--threads", type=int, default=2)
parser.add_argument("--stop-after", type=int, help="stop after this many chunks, then resume from the checkpoint")
parser.add_argument("--out", type=Path, required=True)
args = parser.parse_args()

tracks = []
for path in sorted(args.recording.glob("s[0-9][0-9][0-9][0-9]-*-[0-9]*.flac")):
    session, source, index = re.fullmatch(r"s(\d+)-(mic|system)-(\d+)\.flac", path.name).groups()
    offset_file = args.recording / f"session-{session}.json"
    offset = json.loads(offset_file.read_text(encoding="utf-8")).get("offset_ms", 0) if offset_file.is_file() else 0
    tracks.append({"id": path.name, "path": str(path), "source": source, "offset_ms": offset + int(index) * 30000})
if not tracks:
    raise SystemExit("No capture files found")


class Stopped(Exception):
    pass


progress, rows, chunks, whisper_seconds = {}, [], 0, 0.0
real_run = engine.run


def timed_run(argv, **kwargs):
    global whisper_seconds
    started = time.monotonic()
    try:
        return real_run(argv, **kwargs)
    finally:
        if Path(argv[0]).name.startswith("whisper"):
            whisper_seconds += time.monotonic() - started


engine.run = timed_run


def receive(**event):
    global chunks
    if event["type"] == "track_duration":
        next(t for t in tracks if t["id"] == event["track_id"])["duration_ms"] = event["duration_ms"]
    elif event["type"] == "asr_batch":
        progress[event["stream"]] = {"covered_ms": event["covered_ms"], "overlap_text": event["overlap_text"]}
        rows.extend(event["segments"])
        chunks += 1
        if chunks == args.stop_after:
            raise Stopped


started = time.monotonic()
with tempfile.TemporaryDirectory(prefix="voxloom-transcribe-") as directory:
    request = {"kind": "transcribe", "models": {"whisper": str(args.model)}, "whisper_cli": args.whisper_cli,
               "threads": args.threads, "language": "en"}
    for _ in range(2):
        try:
            engine.transcribe(dict(request, tracks=tracks, asr_progress=json.loads(json.dumps(progress))), receive, Path(directory))
            break
        except Stopped:
            pass
elapsed = time.monotonic() - started

streams = {s["key"]: s["length"] for s in engine.audio_streams(tracks, lambda **event: None)}
audio = sum(streams.values())
summary = {"audio_ms": audio, "streams": streams, "covered_ms": {k: progress.get(k, {}).get("covered_ms", 0) for k in streams},
           "rows": len(rows), "chunks": chunks, "words": sum(len(r["text"].split()) for r in rows),
           "elapsed_s": round(elapsed, 1), "whisper_s": round(whisper_seconds, 1),
           "real_time_factor": round(elapsed / max(1, audio / 1000), 3), "resumed": bool(args.stop_after)}
args.out.write_text(json.dumps({"summary": summary, "rows": rows}, indent=1), encoding="utf-8")
print(json.dumps(summary))
assert summary["covered_ms"] == streams, "every recorded millisecond must be covered"
