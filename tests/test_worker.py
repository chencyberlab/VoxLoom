from array import array
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import wave
import shutil
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "worker"))
import engine
import provision


class InferenceUnits(unittest.TestCase):
    @unittest.skipUnless(shutil.which('ffmpeg'), 'SKIPPED: FFmpeg not installed')
    def test_segmented_flac_duration_and_seek_preserve_original_pcm(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            source=root/'source.wav'
            samples=b''.join(((n*17)%4000-2000).to_bytes(2,'little',signed=True) for n in range(51200))
            with wave.open(str(source),'wb') as output:
                output.setparams((1,2,16000,0,'NONE','not compressed'))
                output.writeframes(samples)
            engine.run(['ffmpeg','-nostdin','-v','error','-i',str(source),'-c:a','flac','-f','segment','-segment_time','1','-reset_timestamps','1',str(root/'s0000-system-%06d.flac')])
            tracks=[{'id':str(i),'path':str(p),'source':'system','offset_ms':i*1000} for i,p in enumerate(sorted(root.glob('*.flac')))]
            stream=engine.audio_streams(tracks,lambda **event:None)[0]
            self.assertEqual(stream['length'],3200)
            target=root/'joined.wav'
            engine.clip_stream(stream,target,500,2900,root)
            with wave.open(str(target)) as output: actual=output.readframes(output.getnframes())
            self.assertEqual(actual,samples[500*32:2900*32])

    def test_final_response_never_exposes_draft_or_unfinished_generation(self):
        good = "<voxloom_result>Budget approved [00:00:03].</voxloom_result>"
        self.assertEqual(engine.final_response(good), "Budget approved [00:00:03].")
        self.assertEqual(engine.final_response("<voxloom_result>Budget approved [00:00:03]. [end of text]"), "Budget approved [00:00:03].")
        self.assertEqual(engine.final_response("<|channel>thought\nprivate draft<channel|>" + good), "Budget approved [00:00:03].")
        self.assertEqual(engine.final_response("<think>private draft</think>" + good), "Budget approved [00:00:03].")
        for bad in ["<|channel>thought\nThinking Process: unfinished", "<think>unfinished", "<voxloom_result>cut off", "Thinking Process: draft", "<voxloom_result><|channel>thought draft</voxloom_result>"]:
            with self.assertRaisesRegex(ValueError, "finish"):
                engine.final_response(bad)

    @unittest.skipUnless(shutil.which('ffmpeg'), 'SKIPPED: FFmpeg not installed')
    def test_stream_clip_crosses_storage_boundary_without_dropping_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tracks=[]
            for i, value in enumerate([1000, -1000]):
                path=root/f's0000-system-{i:06}.flac'
                wav=root/f'{i}.wav'
                with wave.open(str(wav),'wb') as output:
                    output.setparams((1,2,16000,0,'NONE','not compressed'))
                    output.writeframes(value.to_bytes(2,'little',signed=True)*16000)
                engine.run(['ffmpeg','-nostdin','-v','error','-i',str(wav),str(path)])
                tracks.append({'id':str(i),'path':str(path),'source':'system','offset_ms':i*30000,'duration_ms':1000})
            stream=engine.audio_streams(tracks,lambda **event:None)[0]
            self.assertEqual(stream['length'],2000)  # Actual samples, not nominal file labels.
            target=root/'joined.wav'
            engine.clip_stream(stream,target,500,2000,root)
            with wave.open(str(target)) as result: samples=result.readframes(result.getnframes())
            self.assertEqual(samples, (1000).to_bytes(2,'little',signed=True)*8000+(-1000).to_bytes(2,'little',signed=True)*16000)

    @unittest.skipUnless(shutil.which('ffmpeg'), 'SKIPPED: FFmpeg not installed')
    def test_subsecond_asr_tail_is_padded_without_changing_original(self):
        with tempfile.TemporaryDirectory() as directory:
            source, target = Path(directory)/'short.wav', Path(directory)/'asr.wav'
            with wave.open(str(source),'wb') as output:
                output.setparams((1,2,16000,0,'NONE','not compressed'))
                output.writeframes(b'\0\0'*3200)
            engine.clip(source,target,0,200,minimum_ms=1100)
            with wave.open(str(target)) as result:
                self.assertGreaterEqual(result.getnframes(),17600)
            with wave.open(str(source)) as original:
                self.assertEqual(original.getnframes(),3200)
    def test_chunks_no_gaps_tail(self):
        self.assertEqual(list(engine.windows(65000)), [(0, 30000), (28000, 58000), (56000, 65000)])
        self.assertEqual(list(engine.windows(30000)), [(0, 30000)])
        self.assertEqual(list(engine.windows(0)), [])

    def test_prefixes_and_normalization(self):
        self.assertEqual(engine.prompt("  budget \n", True), "task: search query | text: budget")
        self.assertEqual(engine.prompt("  budget "), "task: search result | text: budget")
        v = engine.normalize([2] * 768)
        self.assertAlmostEqual(sum(x*x for x in v), 1)
        for invalid in [[0] * 768, [float("nan")] * 768, [1] * 767]:
            with self.assertRaises(ValueError):
                engine.normalize(invalid)

    def test_timestamp_over_an_hour(self):
        self.assertEqual(engine.stamp(3661001), "01:01:01")

    def test_retry_seam_drops_only_words_heard_twice(self):
        self.assertEqual(engine.drop_repeated_start("We approved the project budget.", "the project budget, and launch is Friday."), "and launch is Friday.")
        self.assertEqual(engine.drop_repeated_start("people from hurting themselves as they were.", "themselves as it were. Yeah, we did."), "Yeah, we did.")
        self.assertEqual(engine.drop_repeated_start("The budget is approved", "The garden needs water"), "The garden needs water")
        self.assertEqual(engine.drop_repeated_start("Yeah.", "Yeah. Yeah, we did."), "Yeah. Yeah, we did.")

    def test_missing_model_is_real_error(self):
        with self.assertRaisesRegex(ValueError, "not installed"):
            engine.ensure_model("/tmp/nonexistent-voxloom-test-model.bin")

    def test_pinned_models_are_hashed_once_until_they_change(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            body = b"fixture whisper weights"
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"models": [{"id": "whisper", "filename": "ggml-base.en.bin", "bytes": len(body), "sha256": hashlib.sha256(body).hexdigest()}]}), encoding="utf-8")
            model = root / "ggml-base.en.bin"
            model.write_bytes(body)
            cache = root / "cache" / "verified-models.json"
            hashed = []
            real_digest = provision.digest
            def counting_digest(path):
                hashed.append(path)
                return real_digest(path)
            with patch.object(provision, "MANIFEST", manifest), patch.object(provision, "digest", side_effect=counting_digest), \
                    patch.object(engine, "VERIFIED_MODELS", str(cache)):
                engine.ensure_model(model)
                engine.ensure_model(model)
                self.assertEqual(len(hashed), 1)
                self.assertEqual(json.loads(cache.read_text(encoding="utf-8"))[str(model.resolve())][0], len(body))
                # Same size, new content: the changed mtime forces a fresh hash, which fails.
                model.write_bytes(b"fixture whisper weighty")
                os.utime(model, ns=(model.stat().st_atime_ns, model.stat().st_mtime_ns + 1_000_000))
                with self.assertRaisesRegex(ValueError, "SHA-256"):
                    engine.ensure_model(model)
                self.assertEqual(len(hashed), 2)
                # Rewritten correctly: hashed once more, then trusted again.
                model.write_bytes(body)
                os.utime(model, ns=(model.stat().st_atime_ns, model.stat().st_mtime_ns + 2_000_000))
                engine.ensure_model(model)
                engine.ensure_model(model)
                self.assertEqual(len(hashed), 3)
                # A damaged cache file is ignored, not fatal.
                cache.write_text("not json", encoding="utf-8")
                engine.ensure_model(model)
                self.assertEqual(len(hashed), 4)
            # Without a cache location (tests, scripts) every call verifies.
            with patch.object(provision, "MANIFEST", manifest), patch.object(provision, "digest", side_effect=counting_digest), \
                    patch.object(engine, "VERIFIED_MODELS", None):
                engine.ensure_model(model)
                engine.ensure_model(model)
                self.assertEqual(len(hashed), 6)
            # Files the manifest does not name (custom GGUFs) are never hashed.
            custom = root / "my-own.gguf"
            custom.write_bytes(b"x")
            with patch.object(provision, "MANIFEST", manifest), patch.object(provision, "digest", side_effect=AssertionError("hashed")):
                engine.ensure_model(custom)

    def test_empty_evidence_does_not_load_model(self):
        rows = []
        engine.execute({"kind":"answer", "evidence":[]}, lambda **v: rows.append(v), Path("/tmp"))
        self.assertIn("no supporting evidence", rows[0]["text"])

    def test_scripts_run_without_their_folder_on_sys_path(self):
        # -P is what the embeddable Python bundled on Windows does: the
        # script's own folder is not on sys.path.
        import subprocess
        worker = Path(__file__).resolve().parents[1] / "worker"
        with tempfile.TemporaryDirectory() as directory:
            request, response = Path(directory) / "request.json", Path(directory) / "response.jsonl"
            request.write_text(json.dumps({"kind": "answer", "evidence": []}), encoding="utf-8")
            result = subprocess.run([sys.executable, "-P", str(worker / "engine.py"), "--request", str(request), "--response", str(response)],
                                    capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("no supporting evidence", response.read_text(encoding="utf-8"))
            target = Path(directory) / "Zoë notes.docx"
            result = subprocess.run([sys.executable, "-P", str(worker / "export_docx.py"), str(target)],
                                    input=json.dumps({"title": "Café", "text": "## Overview\nZoë agreed."}).encode(), capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(target.read_bytes().startswith(b"PK"))


def speech_audio(spans, total):
    """Fixture PCM: silence is 0; speech samples encode their stream time / 100 ms."""
    samples = array('h', bytes(total * 32))
    for a, b in spans:
        for ms in range(a, b):
            samples[ms * 16:(ms + 1) * 16] = array('h', [ms // 100 + 1] * 16)
    return samples


class LiveTranscription(unittest.TestCase):
    """Real chunk planner and sentence assembly with fixture audio and Whisper."""

    def run_live(self, spans, total, step=2500, skip_tail_once=False, endings=None, room=-120.0, decode=None):
        audio = speech_audio(spans, total)
        progress, rows, chunks, calls = {}, [], [], []
        endings = endings or {}

        def levels(samples):
            size = engine.FRAME_MS * 16
            return [-20.0 if any(samples[i:i + size]) else room for i in range(0, len(samples), size)]

        def whisper(request, model, samples, scratch):
            calls.append(len(samples) // 16)
            pieces, start = [], None
            for i in range(0, len(samples) + 16, 16):
                voiced = i < len(samples) and samples[i] != 0
                if voiced and start is None:
                    start = i
                elif not voiced and start is not None:
                    begin = (samples[start] - 1) * 100
                    pieces.append({'text': f"Speech at {begin // 1000}s" + endings.get(begin // 1000, '.'), 'start_ms': start // 16, 'end_ms': i // 16})
                    start = None
            if skip_tail_once and len(calls) == 1 and len(pieces) > 1:
                return pieces[:1]  # Whisper ending a window early.
            return pieces

        def receive(**event):
            if event['type'] == 'asr_batch':
                previous = progress.get(event['stream'], {}).get('covered_ms', 0)
                chunks.append((previous, event['covered_ms']))
                progress[event['stream']] = {'covered_ms': event['covered_ms'], 'overlap_text': event['overlap_text']}
                rows.extend(event['segments'])

        with tempfile.TemporaryDirectory() as directory, \
                patch.object(engine, 'ensure_model', return_value='fixture'), \
                patch.object(engine, 'stream_samples', side_effect=lambda stream, a, b: audio[a * 16:b * 16]), \
                patch.object(engine, 'voice_levels', side_effect=levels), \
                patch.object(engine, 'whisper_pieces', side_effect=decode or whisper):
            clock = 0
            while clock < total:
                clock = min(total, clock + step)
                track = {'id': 't', 'path': '/fixture/s0000-mic-000000.flac', 'source': 'mic', 'offset_ms': 0, 'duration_ms': clock}
                engine.transcribe({'kind': 'transcribe_live', 'models': {'whisper': 'fixture'}, 'whisper_cli': 'fixture', 'threads': 2,
                                   'tracks': [track], 'context_tracks': [track], 'asr_progress': json.loads(json.dumps(progress)),
                                   'finalize': clock == total}, receive, Path(directory))
        return rows, chunks, calls

    def test_every_millisecond_is_decoded_once_and_utterances_stay_whole(self):
        spans = [(1000, 6000), (7500, 12000), (13500, 50000), (52000, 55000)]
        rows, chunks, _ = self.run_live(spans, 60000)
        self.assertEqual(chunks[0][0], 0)
        self.assertEqual(chunks[-1][1], 60000)
        for (_, end), (start, _) in zip(chunks, chunks[1:]):
            self.assertEqual(end, start)  # Contiguous: no gap, no overlap.
        self.assertTrue(all(b - a <= engine.MAX_CHUNK_MS for a, b in chunks))
        # Cuts land only in pauses, except inside nonstop speech longer than a window.
        for _, end in chunks[:-1]:
            inside = [(a, b) for a, b in spans if a < end < b]
            self.assertTrue(not inside or inside[0][1] - inside[0][0] > engine.MAX_CHUNK_MS, (end, inside))
        texts = [r['text'] for r in rows]
        for start, _ in spans:
            self.assertIn(f"Speech at {start // 1000}s.", texts)
        self.assertEqual(len(texts), len(set(texts)))  # Nothing transcribed twice.

    def test_live_text_appears_after_a_pause_not_after_a_storage_file(self):
        rows, _, _ = self.run_live([(1000, 9000), (11000, 14000)], 30000)
        first = next(r for r in rows if r['text'] == 'Speech at 1s.')
        self.assertLessEqual(first['end_ms'], 9000)
        self.assertEqual(rows[-1]['source'], 'mic')

    def test_whisper_skipping_trailing_speech_is_retried(self):
        rows, _, calls = self.run_live([(1000, 4000), (6000, 9000)], 12000, step=12000, skip_tail_once=True)
        self.assertEqual([r['text'] for r in rows], ['Speech at 1s.', 'Speech at 6s.'])
        self.assertGreater(len(calls), 1)

    def music_bed(self, repeat_at_join=False):
        """Narration, a music bed, then speech: no pause anywhere, so cuts are forced."""
        script = [(1000, 5000, 'This video shows a session'), (5000, 9000, 'about attending skills.'),
                  (24000, 27000, 'Yeah, I guess I have to.'), (28000, 31000, 'Then more words.'), (40000, 44000, 'And the rest.')]
        windows = []

        def whisper(request, model, samples, scratch):
            a = (samples[0] - 1) * 100
            b = a + len(samples) // 16
            windows.append(a)
            pieces = [(s, e, t) for s, e, t in script if a <= s and e <= b]
            narration = [p for p in pieces if p[0] < 24000]
            if narration and b - 24000 < 6000:
                # Whisper's failure: the narration is stretched over the music and
                # the speech after it, whose words are never written.
                pieces = [*narration[:-1], (narration[-1][0], b, narration[-1][2])]
            elif repeat_at_join and 5000 < a < 9000:
                pieces = [(a, a + 800, 'attending skills.'), *pieces]  # The re-read starts early on purpose.
            return [{'text': t, 'start_ms': s - a, 'end_ms': e - a} for s, e, t in pieces]

        rows, chunks, _ = self.run_live([(0, 60000)], 60000, decode=whisper)
        return [r['text'] for r in rows], chunks, windows

    def test_speech_after_a_music_bed_is_reread_not_lost(self):
        texts, chunks, windows = self.music_bed()
        self.assertEqual(texts, ['This video shows a session about attending skills.', 'Yeah, I guess I have to.', 'Then more words.', 'And the rest.'])
        # The stretched segment is kept; decoding resumes where its words end.
        self.assertTrue(any(5000 < a < 9000 for a in windows), windows)
        for (_, end), (start, _) in zip(chunks, chunks[1:]):
            self.assertEqual(end, start)
        self.assertEqual(chunks[-1][1], 60000)

    def test_words_heard_twice_at_a_reread_join_are_dropped(self):
        texts, _, _ = self.music_bed(repeat_at_join=True)
        self.assertEqual(texts, ['This video shows a session about attending skills.', 'Yeah, I guess I have to.', 'Then more words.', 'And the rest.'])

    def test_unfinished_sentence_waits_for_its_ending_then_flushes_at_stop(self):
        rows, _, _ = self.run_live([(1000, 12000), (13500, 16000)], 18000, endings={1: ' and then', 13: ''})
        self.assertEqual([r['text'] for r in rows], ['Speech at 1s and then Speech at 13s'])
        self.assertEqual(rows[0]['start_ms'], 1000)

    def test_silence_is_skipped_without_calling_whisper(self):
        rows, chunks, calls = self.run_live([], 40000)
        self.assertEqual(rows, [])
        self.assertEqual(calls, [])
        self.assertEqual(chunks[-1][1], 40000)

    def test_pauses_in_a_noisy_room_still_divide_speech(self):
        rows, chunks, _ = self.run_live([(1000, 6000), (7500, 12000), (13500, 20000)], 30000, room=-50.0)
        self.assertEqual([r['text'] for r in rows], ['Speech at 1s.', 'Speech at 7s.', 'Speech at 13s.'])

    def test_audio_without_contrast_is_never_skipped(self):
        # Nothing stands out from the background: it is still transcribed,
        # in window-sized chunks, rather than discarded as silence.
        rows, chunks, calls = self.run_live([], 60000, room=-40.0)
        self.assertEqual(sum(calls), 60000)
        self.assertTrue(all(b - a <= engine.MAX_CHUNK_MS for a, b in chunks))

    def test_noisy_room_pauses_are_found_relative_to_its_floor(self):
        levels = ([-52.0] * 25 + [-30.0] * 150 + [-51.0] * 40 + [-31.0] * 200) * 2
        floor = engine.noise_floor(levels)
        self.assertLess(floor, -45)
        cut = engine.plan_chunk(levels, len(levels) * engine.FRAME_MS, floor + engine.VOICE_MARGIN_DB, final=False)
        self.assertIsNotNone(cut)
        self.assertLess(levels[cut // engine.FRAME_MS], floor + engine.VOICE_MARGIN_DB)
        # Muted audio: digital silence is a pause, never a reason to hide speech.
        muted = [-120.0] * 40 + [-25.0] * 200 + [-120.0] * 40 + [-25.0] * 200
        self.assertTrue(engine.has_contrast(muted, engine.noise_floor(muted)))

    def test_sentence_rows(self):
        rows, pending = engine.sentences([{'text': '- Yeah. - No, e.g. this works. And then we', 'start_ms': 0, 'end_ms': 4000}], flush=False)
        self.assertEqual([r['text'] for r in rows], ['Yeah.', 'No, e.g. this works.'])
        self.assertEqual(pending['text'], 'And then we')
        rows, pending = engine.sentences([pending, {'text': 'go home.', 'start_ms': 5000, 'end_ms': 6000}], flush=False)
        self.assertEqual([r['text'] for r in rows], ['And then we go home.'])
        self.assertIsNone(pending)
        long = ' '.join(['word'] * 120)
        rows, _ = engine.sentences([{'text': long, 'start_ms': 0, 'end_ms': 60000}], flush=True)
        self.assertEqual(' '.join(r['text'] for r in rows), long)
        self.assertTrue(all(len(r['text']) <= engine.PENDING_CHARS for r in rows))

    def test_old_checkpoint_text_is_not_reused_as_state(self):
        self.assertEqual(engine.carry_state('previous overlap words'), {})
        self.assertEqual(engine.carry_state('{"floor_db": -50}'), {'floor_db': -50})

    @unittest.skipUnless(shutil.which('ffmpeg'), 'SKIPPED: FFmpeg not installed')
    def test_file_still_being_recorded_is_read_up_to_its_last_whole_frame(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, complete = root / 'source.wav', root / 'complete.flac'
            samples = b''.join(((n * 37) % 6000 - 3000).to_bytes(2, 'little', signed=True) for n in range(48000))
            with wave.open(str(source), 'wb') as output:
                output.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
                output.writeframes(samples)
            engine.run(['ffmpeg', '-nostdin', '-v', 'error', '-i', str(source), '-c:a', 'flac', str(complete)])
            growing = root / 's0000-mic-000000.flac'
            data = complete.read_bytes()
            growing.write_bytes(data[:len(data) * 2 // 3])  # Cut mid-frame.
            partial = engine.decode_pcm(growing, growing=True)
            self.assertGreater(len(partial), 0)
            self.assertEqual(partial, samples[:len(partial)])
            growing.write_bytes(b'')
            self.assertEqual(engine.decode_pcm(growing, growing=True), b'')
            stream = engine.audio_streams([{'id': 'g', 'path': str(growing), 'source': 'mic', 'offset_ms': 0, 'growing': True}], lambda **e: None)
            self.assertEqual(stream[0]['length'], 0)


class Provisioning(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="voxloom test café ")
        self.root = Path(self.tmp.name)
        self.body = b"local fixture model"
        self.model = {"id":"fixture", "name":"Fixture", "filename":"model.bin", "bytes":len(self.body), "sha256":hashlib.sha256(self.body).hexdigest(),
                      "repo":"example/fixture", "revision":"0"*40}

    def tearDown(self):
        self.tmp.cleanup()

    def test_offline_import_and_integrity(self):
        source = self.root / "source.bin"
        source.write_bytes(self.body)
        with patch.object(socket, "socket", side_effect=AssertionError("network used")):
            provision.install(self.model, self.root / "models", source)
            self.assertTrue(provision.valid(self.root / "models/model.bin", self.model))
        target = self.root / "models/model.bin"
        if sys.platform != 'win32':  # Windows has no Unix modes; AppData ACLs keep files private
            self.assertEqual(target.stat().st_mode & 0o777, 0o600)
        target.write_bytes(b"corrupt")
        self.assertFalse(provision.valid(target, self.model))

    def test_corrupt_import_never_replaces_valid_data(self):
        source = self.root / "source.bin"
        source.write_bytes(b"not a valid model")
        target = self.root / "models"
        target.mkdir()
        with self.assertRaises(ValueError):
            provision.install(self.model, target, source)
        self.assertFalse((target / "model.bin").exists())

    def test_damaged_partial_download_is_removed_so_retry_can_work(self):
        import urllib.error
        import urllib.request
        models = self.root / "models"
        models.mkdir()
        part = models / "model.bin.part"
        refused = urllib.error.HTTPError("https://example.invalid", 416, "Range Not Satisfiable", {}, None)
        # Full length but wrong bytes: the server refuses the resume offset for
        # ever, so a retry can only succeed after the file is gone.
        part.write_bytes(b"xxxxx fixture model")
        with patch.object(urllib.request, "urlopen", side_effect=refused):
            with self.assertRaisesRegex(ValueError, "removed"):
                provision.install(self.model, models)
        self.assertFalse(part.exists())
        self.assertFalse((models / "model.bin").exists())
        # A complete, correct partial file is promoted without another byte.
        part.write_bytes(self.body)
        with patch.object(urllib.request, "urlopen", side_effect=refused):
            provision.install(self.model, models)
        self.assertTrue(provision.valid(models / "model.bin", self.model))

    def test_mismatched_download_is_removed_instead_of_left_for_the_user(self):
        import urllib.request
        models = self.root / "models"
        models.mkdir()
        class Response:
            status = 200
            headers = {}
            def __init__(self, data): self.data = data
            def __enter__(self): return self
            def __exit__(self, *args): return False
            def read(self, size):
                data, self.data = self.data, b""
                return data
        with patch.object(urllib.request, "urlopen", return_value=Response(b"local fixture mode!")):
            with self.assertRaisesRegex(ValueError, "removed"):
                provision.install(self.model, models)
        self.assertFalse((models / "model.bin.part").exists())
        self.assertFalse((models / "model.bin").exists())
        with patch.object(urllib.request, "urlopen", return_value=Response(self.body)):
            provision.install(self.model, models)
        self.assertTrue(provision.valid(models / "model.bin", self.model))

    def test_pinned_manifest_has_only_full_embedding_and_manual_gemma4(self):
        models = json.loads(provision.MANIFEST.read_text())["models"]
        embedding = next(m for m in models if m["id"] == "embedding")
        self.assertEqual(embedding["dimensions"], 768)
        self.assertIn("740m", embedding["filename"])
        # E2B is the standard summary model; E4B is an explicit, opt-in download.
        for model_id, filename, manual in [("summary_e2b", "gemma-4-E2B-it-Q4_0.gguf", False), ("summary_e4b", "gemma-4-E4B-it-Q4_0.gguf", True)]:
            model = next(m for m in models if m["id"] == model_id)
            self.assertTrue(model["optional"])
            self.assertEqual(model.get("manual_only", False), manual)
            self.assertEqual(model["filename"], filename)
            self.assertEqual(len(model["revision"]), 40)
        # Gemma 3 1B was removed in 0.1.8: never downloaded, only listed for removal.
        self.assertFalse(any("gemma-3" in m["filename"] for m in models))
        retired = json.loads(provision.MANIFEST.read_text())["retired"]
        self.assertEqual([r["filename"] for r in retired], ["gemma-3-1b-it-qat-Q4_0.gguf"])
        # The accurate transcription model is an explicit, opt-in download.
        accurate = next(m for m in models if m["id"] == "whisper_accurate")
        self.assertTrue(accurate["manual_only"] and accurate["optional"])
        self.assertEqual(accurate["filename"], "ggml-small.en-q8_0.bin")
        for m in models:
            self.assertEqual(len(m["sha256"]), 64)
            self.assertEqual(len(m["revision"]), 40)


if __name__ == "__main__":
    unittest.main()
