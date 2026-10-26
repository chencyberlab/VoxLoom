#!/usr/bin/env python3
"""Real local-engine regression, synthetic speech only; never downloads models."""
import argparse
from contextlib import nullcontext
import json
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import time
import wave

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "worker"))
import engine

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--models", type=Path, required=True)
parser.add_argument("--summaries", action="store_true")
parser.add_argument("--artifacts", type=Path)
parser.add_argument("--whisper-cli", default=str(ROOT / ".runtimes/whisper-build/bin/whisper-cli"))
parser.add_argument("--transcribe-cli", help="also check Accurate (Parakeet); its model must be in --models")
args = parser.parse_args()
whisper = args.models / "ggml-base.en.bin"
if not whisper.is_file():
    raise SystemExit("SKIPPED: model files not provisioned; choose a local --models folder")
passes = [("whisper", whisper)]
if args.transcribe_cli:
    parakeet = args.models / "parakeet-tdt-0.6b-v2-Q6_K.gguf"
    if not parakeet.is_file():
        raise SystemExit("SKIPPED: the Accurate model is not in --models")
    passes.append(("parakeet", parakeet))

if args.artifacts:
    args.artifacts.mkdir(parents=True, exist_ok=True)
with (nullcontext(str(args.artifacts)) if args.artifacts else tempfile.TemporaryDirectory(prefix="voxloom-continuity-")) as directory:
    scratch = Path(directory)
    full = scratch / "continuous.wav"
    phrases = [
        (28500, "The committee approved the community centre renovation budget and Sarah will send the final invoice on Friday morning."),
        (58500, "The emergency inspection will take place on Tuesday afternoon and David will bring the complete safety report to the meeting."),
    ]
    with wave.open(str(full), "wb") as output:
        output.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        written = 0
        for offset, text in phrases:
            raw, speech = scratch / "tts.wav", scratch / "speech.wav"
            subprocess.run(["espeak-ng", "-v", "en-us", "-s", "145", "-w", str(raw), text], check=True)
            engine.clip(raw, speech, 0, engine.duration(raw))
            with wave.open(str(speech)) as source:
                samples = source.readframes(source.getnframes())
            target = offset * 16
            output.writeframesraw(b"\0\0" * (target - written))
            output.writeframesraw(samples)
            written = target + len(samples) // 2
        output.writeframesraw(b"\0\0" * 16000)
    subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-i", str(full), "-c:a", "flac", "-f", "segment", "-segment_time", "30", "-reset_timestamps", "1", str(scratch / "s0000-system-%06d.flac")], check=True)
    tracks = [{"id": str(i), "path": str(path), "source": "system", "offset_ms": i * 30000} for i, path in enumerate(sorted(scratch.glob("s0000-system-*.flac")))]
    original_run = engine.run
    calls = []
    def traced_run(argv, **kwargs):
        result = original_run(argv, **kwargs)
        if Path(argv[0]).name == 'whisper-cli':
            calls.append(json.loads((scratch / 'asr.json').read_text()))
            if args.artifacts:
                (scratch / f'pass-{len(calls)}.json').write_text(json.dumps(calls[-1], indent=2))
                shutil.copyfile(scratch/'asr.wav', scratch/f'pass-{len(calls)}.wav')
        return result
    engine.run = traced_run
    for name, model in passes:
        checkpoints, lengths, segments = {}, {}, []

        def receive(**event):
            if event["type"] == "track_duration":
                lengths[event["track_id"]] = event["duration_ms"]
            elif event["type"] == "asr_batch":
                checkpoints[event["stream"]] = {"covered_ms": event["covered_ms"], "overlap_text": event.get("overlap_text", "")}
                segments.extend(event["segments"])

        started = time.monotonic()
        # The pass after Stop: every 30-second file at once, sentences crossing their edges.
        engine.transcribe({"kind": "transcribe", "models": {"whisper": str(model)}, "tracks": tracks, "asr_progress": {}, "asr_engine": name,
                           "whisper_cli": args.whisper_cli, "transcribe_cli": args.transcribe_cli, "threads": 2}, receive, scratch)
        assert checkpoints["system:s0000"]["covered_ms"] == sum(lengths.values())
        transcript = " ".join(segment["text"] for segment in segments)
        if args.artifacts:
            (scratch/f'segments-{name}.json').write_text(json.dumps(segments,indent=2))
        for word in ["committee", "approved", "community", "budget", "invoice", "Friday", "emergency", "inspection", "Tuesday", "safety", "report"]:
            assert word.lower() in transcript.lower(), (name, word, transcript)
        assert all(s["start_ms"] <= s["end_ms"] for s in segments)
        print(json.dumps({"continuous_asr": "PASS", "engine": name, "seconds": time.monotonic() - started, "audio_ms": sum(lengths.values()), "covered_ms": checkpoints["system:s0000"]["covered_ms"], "synthetic_transcript": transcript}, indent=2), flush=True)
    if args.summaries:
        for name in ["gemma-4-E2B-it-Q4_0.gguf"]:
            model = args.models / name
            if not model.is_file():
                print(json.dumps({"summary_model": name, "result": "SKIPPED: model files not provisioned"}), flush=True)
                continue
            started = time.monotonic()
            reply = engine.generate({"models": {"summary": str(model)}, "llama_cli": str(ROOT / ".runtimes/llama-build/bin/llama-completion"), "threads": 2}, "Summarize topics and explicit actions.\n<EVIDENCE>\n[00:00:03] The budget was approved. Sarah will send the final invoice on Friday.\n</EVIDENCE>", scratch)
            assert "<|channel>" not in reply and "Thinking Process" not in reply
            assert "[00:00:03]" in reply
            print(json.dumps({"summary_model": name, "result": "PASS", "seconds": time.monotonic() - started, "synthetic_reply": reply}, indent=2), flush=True)
