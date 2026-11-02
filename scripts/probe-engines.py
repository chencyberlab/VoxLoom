#!/usr/bin/env python3
"""Real, CPU-only engine acceptance. Never download; absent assets are SKIPPED.
Runs with the system python3. Fixtures can be generated using espeak-ng.
"""
import argparse
import json
import os
os.umask(0o077)
from pathlib import Path
import resource
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "worker"))
import engine


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--speech", type=Path, required=True)
    parser.add_argument("--whisper-cli", default="whisper-cli")
    parser.add_argument("--llama-cli", default="llama-completion")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = {}
    with tempfile.TemporaryDirectory(prefix="voxloom-probe-") as directory:
        scratch = Path(directory)
        request = {"models": {"whisper": str((args.models / "ggml-base.en.bin").resolve()),
                              "summary": str((args.models / "gemma-4-E2B-it-Q4_0.gguf").resolve())},
                   "cache_dir": directory, "threads": 2, "whisper_cli": args.whisper_cli, "llama_cli": args.llama_cli,
                   "tracks": [{"id": "fixture", "path": str(args.speech.resolve()), "source": "imported", "offset_ms": 0}]}
        if Path(request["models"]["whisper"]).is_file():
            rows = []
            started = time.monotonic()
            engine.transcribe(request, lambda **v: rows.append(v), scratch)
            text = " ".join(v["text"] for v in rows if v["type"] == "segment")
            assert "budget" in text.lower(), f"Expected fixture word budget: {text}"
            report["whisper"] = {"passed": True, "seconds": time.monotonic() - started, "fixture_transcript": text,
                                 "child_peak_rss_kib": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss}
        else:
            report["whisper"] = {"status": "SKIPPED: model files not provisioned"}
        if Path(request["models"]["summary"]).is_file():
            started = time.monotonic()
            reply = engine.generate(request, "Summarize this evidence in one sentence.\n<EVIDENCE>[00:00:03] The budget was approved.</EVIDENCE>", scratch)
            assert "budget" in reply.lower() and "00:00:03" in reply
            report["summary"] = {"passed": True, "seconds": time.monotonic() - started, "fixture_reply": reply,
                                 "child_peak_rss_kib": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss}
        else:
            report["summary"] = {"status": "SKIPPED: model files not provisioned; use explicit model setup or offline import"}
    if args.output:
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
