#!/usr/bin/env python3
"""Replay a finished recording through live transcription, as capture wrote it.

Every pass sees the 30-second files that had closed by then plus the file still
being written (truncated to the replay clock), exactly like a live recording.
Writes the resulting transcript rows as JSON; never downloads anything.

  python3 scripts/replay-live-asr.py --recording DIR --model ggml-base.en.bin --out rows.json
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "worker"))
import engine

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--recording", type=Path, required=True, help="folder with s0000-<source>-NNNNNN.flac files")
parser.add_argument("--model", type=Path, required=True)
parser.add_argument("--whisper-cli", default=str(ROOT / ".runtimes/whisper-build/bin/whisper-cli"))
parser.add_argument("--step", type=float, default=2.5, help="seconds of capture between live passes")
parser.add_argument("--threads", type=int, default=2)
parser.add_argument("--out", type=Path, required=True)
parser.add_argument("--until", type=float, help="stop the replay after this many seconds of capture")
args = parser.parse_args()

files = sorted(args.recording.glob("s0000-*-*.flac"))
if not files:
    raise SystemExit("No s0000 capture files found")
source = "mic" if "-mic-" in files[0].name else "system"
lengths = [engine.duration(f) for f in files]
total = sum(lengths)
if args.until:
    total = min(total, int(args.until * 1000))
progress, rows, chunks, passes, whisper_seconds = {}, [], [], 0, 0.0
real_run = engine.run
real_pieces = engine.transcribe_chunk


def logged_pieces(request, model, samples, levels, threshold, scratch):
    pieces = real_pieces(request, model, samples, levels, threshold, scratch)
    chunks.append({"start_ms": progress.get(f"{source}:s0000", {}).get("covered_ms", 0), "length_ms": len(samples) // 16,
                   "at_ms": clock, "pieces": pieces})
    return pieces


engine.transcribe_chunk = logged_pieces


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
    if event["type"] == "asr_batch":
        progress[event["stream"]] = {"covered_ms": event["covered_ms"], "overlap_text": event["overlap_text"]}
        rows.extend(dict(s, committed_at_ms=clock) for s in event["segments"])


with tempfile.TemporaryDirectory(prefix="voxloom-replay-") as directory:
    scratch = Path(directory)
    request = {"kind": "transcribe_live", "models": {"whisper": str(args.model)}, "whisper_cli": args.whisper_cli,
               "threads": args.threads, "language": "en"}
    clock, started = 0, time.monotonic()
    while True:
        clock = min(total, clock + int(args.step * 1000))
        final = clock >= total
        closed, growing, offset = [], [], 0
        for i, (path, length) in enumerate(zip(files, lengths)):
            # A file closes when the next one starts; the newest closes at Stop.
            if offset + length <= clock and i + 1 < len(files):
                closed.append({"id": str(i), "path": str(path), "source": source, "offset_ms": i * 30000, "duration_ms": length})
            elif offset < clock:
                partial = scratch / "live" / path.name
                partial.parent.mkdir(exist_ok=True)
                subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(path), "-t", str((clock - offset) / 1000),
                                "-c:a", "flac", str(partial)], check=True)
                growing.append({"path": str(partial), "source": source, "offset_ms": i * 30000})
                break
            offset += length
        if final and not args.until:
            closed = [{"id": str(i), "path": str(p), "source": source, "offset_ms": i * 30000, "duration_ms": n}
                      for i, (p, n) in enumerate(zip(files, lengths))]
            growing = []
        work = scratch / f"pass-{passes}"
        work.mkdir()
        engine.transcribe(dict(request, tracks=closed, context_tracks=closed, growing=growing,
                               asr_progress=json.loads(json.dumps(progress)), finalize=final), receive, work)
        passes += 1
        if final:
            break
    elapsed = time.monotonic() - started

covered = progress.get(f"{source}:s0000", {}).get("covered_ms", 0)
lag = [r["committed_at_ms"] - r["end_ms"] for r in rows]
summary = {"audio_ms": total, "covered_ms": covered, "rows": len(rows), "passes": passes,
           "words": sum(len(r["text"].split()) for r in rows), "elapsed_s": round(elapsed, 1),
           "whisper_s": round(whisper_seconds, 1),
           "median_lag_s": round(sorted(lag)[len(lag) // 2] / 1000, 1) if lag else None,
           "max_lag_s": round(max(lag) / 1000, 1) if lag else None}
args.out.write_text(json.dumps({"summary": summary, "rows": rows, "chunks": chunks}, indent=1))
print(json.dumps(summary))
assert covered == total, "every captured millisecond must be covered"
