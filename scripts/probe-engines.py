#!/usr/bin/env python3
"""Real, CPU-only engine acceptance. Never download; absent assets are SKIPPED.
Run with the LiteRT venv Python. Fixtures can be generated using espeak-ng.
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
    parser.add_argument("--unrelated", type=Path, required=True)
    parser.add_argument("--whisper-cli", default="whisper-cli")
    parser.add_argument("--llama-cli", default="llama-completion")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = {}
    with tempfile.TemporaryDirectory(prefix="voxloom-probe-") as directory:
        scratch = Path(directory)
        request = {"models": {"whisper": str((args.models / "ggml-base.en.bin").resolve()),
                              "embedding": str((args.models / "embeddinggemma-2-740m.litertlm").resolve()),
                              "summary": str((args.models / "gemma-3-1b-it-qat-Q4_0.gguf").resolve())},
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
        if Path(request["models"]["embedding"]).is_file():
            from litert_lm import Content
            started = time.monotonic()
            with engine.embedding_engine(request, audio=True) as runtime:
                query = engine.normalize(runtime.compute_embedding(engine.prompt("the project budget and invoice", True)).embedding)
                texts = [engine.normalize(runtime.compute_embedding(engine.prompt(t)).embedding) for t in
                         ["The project budget is approved. Sarah will send the invoice.", "Red roses grow in the garden."]]
                text_seconds = time.monotonic() - started
                wavs = []
                for i, source in enumerate([args.speech, args.unrelated]):
                    target = scratch / f"fixture-{i}.wav"
                    engine.clip(source, target, 0, engine.duration(source))
                    wavs.append(target)
                started_audio = time.monotonic()
                audios = [engine.normalize(runtime.compute_embedding(Content.AudioFile(str(p))).embedding) for p in wavs]
                dot = lambda a, b: sum(x * y for x, y in zip(a, b))
                text_scores = [dot(query, v) for v in texts]
                audio_scores = [dot(query, v) for v in audios]
                assert text_scores[0] > text_scores[1], text_scores
                assert audio_scores[0] > audio_scores[1], audio_scores
                report["embedding"] = {"passed": True, "dimensions": len(query), "backend": "CPU x86_64, 2 threads",
                                       "text_scores": text_scores, "native_audio_scores": audio_scores,
                                       "load_plus_three_text_seconds": text_seconds,
                                       "two_audio_seconds": time.monotonic() - started_audio,
                                       "worker_peak_rss_kib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
        else:
            report["embedding"] = {"status": "SKIPPED: model files not provisioned"}
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
