# Validation and remaining work

Tested 2026-10-15 on an Ubuntu 24.04.5 x86_64 Surface Laptop Studio with 15,822 MiB physical RAM, an existing browser/desktop workload (about 7 GiB already used during initial inspection), Node 22.23.1, Python 3.12.3 and Rust 1.99.0. No GPU inference was used. Native screenshots and lifecycle actions use an isolated Xvfb display; PipeWire fixtures use the actual user's local Pulse compatibility socket with temporary synthetic endpoints, without recording the physical microphone or changing defaults. This is not a GNOME Wayland physical-device certification.

## Reproducible checks

```bash
npm run check
npm test
npm run test:ui
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --offline
cargo clippy --manifest-path src-tauri/Cargo.toml --offline -- -D warnings
```

The project-local Rust launcher is `scripts/desktop.sh`; for direct cargo commands with that toolchain, export `RUSTUP_HOME="$PWD/.toolchain/rustup"`, `CARGO_HOME="$PWD/.toolchain/cargo"`, and add `.toolchain/cargo/bin` to PATH. No cargo clean is needed. Native dependencies and compiler prerequisites are in README. Playwright uses the available Chrome binary at `/opt/google/chrome/chrome`; adjust the config on a different development machine. No cloud API keys or external test service is required.

Meaningful unit checks cover pause-aligned chunk planning, skipped-speech retries, sentence carry-over, chunk tail/time offsets, asymmetric task prefixes, 768-d normalization/finite checks, FTS insert/edit/delete triggers, SQLite reopen/migration, vector serialization, nearby-result deduplication, integrity validation, private imports with spaces/non-ASCII paths, corrupt imports, and empty-evidence behavior. The UI tests check empty/error states, normal navigation, onboarding's one-click action, hidden technical details, optional summaries, responsive overflow, local asset requests and compact control labels. Browser preview is explicitly labeled and cannot access user files; it has no simulated recordings.

Version 0.1.1 added a first-run 1B/E2B summary selector. Six browser tests pass, including the request payload, displayed estimate and locked selection during setup for 1B, E2B and no summaries. These use an explicitly mocked bridge, without downloading models. A separate Rust regression executes the real offline provisioning script with tiny SHA-256-checked fixture files and a stubbed runtime readiness probe. It verifies core plus only the chosen summary model, saved E2B selection after success, core-only setup, missing-summary failure without changing the saved selection, and rejection of an unknown profile. Native checks on the final 0.1.1 package also passed launch, compact/expand and Quit, with all three observed processes released. The setup action remains fully visible at 900×720 in the browser checks. No user library or installed app data is reset during updates.

## Version 0.1.2: continuous audio and finished summaries

The user reported complete audio but missing transcript words, plus an E2B summary containing an unfinished thought channel. Regression work reproduced three app/runtime issues: independently decoding storage chunks, time-based rejection of valid recognized text, and seeking into segmented FLAC that skipped requested audio. Some FLAC segments also inherited the full recording duration in their header. Transcription now joins each source/session logically, counts/caches actual decoded samples, decodes segmented files before seeking, waits for natural pauses, and retains whole recognized segments. Nonstop speech uses bounded 120-second passes with six seconds of audio overlap and conservative text deduplication. Capture runs independently, and Stop/Pause flushes pending speech. Quiet audio is retained; only digital quiet is trimmed from inference copies.

Real engine regression, using already user-provisioned weights read-only and synthetic speech only:

```bash
python3 scripts/test-transcript-continuity.py --models /path/to/existing/models --summaries
```

The 67,574 ms fixture places sentences across the physical 30- and 60-second file boundaries. Decoded coverage exactly matched its original duration; the complete committee/budget/invoice/Friday and emergency/inspection/Tuesday/safety-report sentences survived. A separate real FFmpeg regression compares PCM bytes across segmented-file boundaries and verifies exact sample preservation. The earlier failing fixture omitted opening words and misreported the duration as 127,478 ms, exposing the seek/header defects. These fixtures include silence and are not continuous-speech throughput benchmarks.

The llama.cpp completion frontend accepted but failed to forward its reasoning setting. The documented source patch now applies `--reasoning off` to its native chat template; both CPU variants are rebuilt. Responses require a completed result wrapper or native end-of-generation marker, never an unfinished thought channel. The short E2B fixture returned normal cited notes. The 1B fixture completed without a reasoning leak but omitted reliable citations and correctly used the explicitly labeled evidence-extract fallback; this does not demonstrate high-quality 1B summarization. No new model weights were downloaded for these checks. E2B business quality and full-native peak memory remain unverified.

Validation for this update: 16 Python checks, six Rust unit checks plus the offline setup integration check, six browser checks (including hiding an old unfinished summary), desktop clippy with warnings denied, the real ASR/index/search/summary/restart/export/delete/shutdown workflow (22.18 s), and synthetic dual capture with live text, pause/resume, final-tail processing and shutdown (55.86 s). Cursor advancement and transcript rows commit atomically; a deliberately invalid batch rolls back both, retries do not duplicate rows, edited text survives, progress resumes after reopening SQLite, and meeting deletion cascades through the new checkpoint table. The database migrates to schema version 2. Tests use temporary libraries and never reset the installed user's data. The final extracted 0.1.2 package also passed native launch, compact/expand and Quit (exit 0; four observed processes released; sampled idle tree 434,888 KiB). Packaged workers, manifest, source notices and both patched completion binaries matched the source/staging hashes; no model weights are bundled.

## Version 0.1.5: optional accurate transcription

The user captured a public counselling role-play from YouTube (system audio, 2 min 31 s, music bed under the opening narration) and supplied the video's own transcript as ground truth. Live replays with two threads, scored with word alignment against the first 277 words:

| Model | Word error rate | Missing words | Whisper time / audio | Notes |
|---|---|---|---|---|
| base.en (Standard) | 4.7% | 4 (an opening "Yeah", stammers "who, who", "I'm, I'm", "Hmm") | 15% | Misheard "one year already" as "when you're ready" |
| small.en q8_0 (Accurate) | 5.8% | 9 | 40% | Heard "one year already" correctly; dropped "Yeah, I guess I have to" where speech starts under the end of the music bed |
| small.en full / q5_1 | 6.1% / 6.5% | 10 | 51% / 59% | Same drop; no accuracy gain over q8_0 |

On the earlier noisy laptop-microphone recording, small.en scored 15.2% against base.en's 19.8%. Accurate is therefore an opt-in choice, not the default: it helps most with distant or noisy speakers and costs about three times the CPU. The ground-truth comparison also showed that a repeated phrase at a chunk join ("I have to … I have to") was real speech ("I have to have to"), so repeated words at joins are deliberately not removed.

DTW word-level timing was tried for skipped-speech detection and rejected (see AGENTS.md): it raised base.en to 8.3% and small.en to 11.6% through music-bed re-reads and hallucinations.

Validation: 31 Python checks (manifest pins Accurate as optional and manual-only), 8 Rust unit checks including Accurate→Standard fallback and profile validation, offline setup integration, 8 browser checks including choosing Accurate, the "download first" hint, the saved setting and the per-model download request. Provisioning of the new model passed offline import, integrity check and a real pinned download (SHA-256 67a179f6…7958067); the default setup check ignores it.

## Version 0.1.4: live transcription without cut or lost sentences

The user reported cut sentences and lost words in long conversations. Two real recordings (6.8 min noisy laptop microphone, 3.9 min system audio) showed the cause: every boundary fell exactly at 114, 228 and 342 seconds. The fixed −60 dB silence detector never fired on real audio (band-limited noise floors were about −32 dBFS raw on the microphone and −55 dBFS on system audio), so every pass waited for the 120-second fallback, cut mid-word and relied on six seconds of overlapping text deduplication. That produced garbled or duplicated lines at each boundary, Whisper line fragments as rows and roughly two minutes of lag. Long 120-second passes also lost punctuation.

Research basis: WhisperX "VAD cut & merge" (pre-segment at pauses, merge to at most one 30-second Whisper window, cut nonstop speech at its quietest point), whisper_streaming/SimulStreaming (commit only stable text), and whisper.cpp's own Silero VAD. VoxLoom now applies cut-and-merge to live capture with an adaptive speech-band level detector, adds a retry for speech Whisper skips, and reads the file still being recorded. See ARCHITECTURE.md for the rules.

Replay on the real recordings, exactly as capture wrote them (closed 30-second files plus the growing file, one live pass per 2.5 s of capture):

```bash
python3 scripts/replay-live-asr.py --recording <recording folder> --model <models>/ggml-base.en.bin \
  --whisper-cli <whisper-cli> --out /tmp/rows.json
```

Scored against a large-v3-turbo transcript of the complete audio (word alignment; the reference also has errors):

| Recording | Build | WER | Content words missing | Duplicated boundary text | Planner lag (median / max) |
|---|---|---|---|---|---|
| Microphone, 407 s, noisy | 0.1.3 | 21.8% | 49 | yes, at each 114 s boundary | about 2 minutes |
| Microphone, 407 s, noisy | 0.1.4 base.en | 19.8% | 45 | none | 6.5 s / 28.8 s |
| Microphone, 407 s, noisy | 0.1.4 small.en (not shipped) | 15.2% | 43 | none | 5.0 s / 20.2 s |
| System audio, 237 s | 0.1.3 | 3.2% | 4 words total | yes | about 2 minutes |
| System audio, 237 s | 0.1.4 base.en | 1.8–3.6% (run variation) | 0–3 words total | none | 3.8–11 s / 22–27 s |

Planner lag is measured on the recording clock (end of a sentence until its chunk can be committed); add Whisper processing (about 1–4 s per chunk with base.en at two threads) and up to 2.5 s of polling. 100% of captured audio was covered in every run. The remaining microphone deletions are mostly fillers (um/uh/yeah) that Whisper omits by style. A 6.7-second passage that Whisper skipped at the end of one chunk ("single timestamp ending") during development is now recovered by the voiced-gap retry.

Measured CPU speed on the reference laptop (i7-11370H, 4 cores, AVX2 build), one 28-second chunk: base.en 3.6 s (2 threads) / 2.5 s (4); small.en 10.7 s / 8.8 s; large-v3-turbo q5_0 35 s / 23 s, slower than real time and unsuitable for live use on business laptops.

Growing-file reads: with `-segment_format_options flush_packets=1`, 58 snapshots of a file being written decoded as exact byte prefixes of the final audio with no decoder errors; without it the segment muxer buffers 256 KB (about 16 s of speech) before anything is readable.

Validation for this update: 31 Python checks (pause planner coverage with no gaps or overlap, cuts only in pauses, retry of skipped trailing speech, unfinished-sentence carry and flush, silence and no-contrast audio, noisy-room and muted-audio floors, growing-file decode), Rust unit and setup checks, the real continuity regression (both boundary-crossing sentences complete), and the synthetic PipeWire dual-capture test: first live text after 24.8 s, before the first 30-second file closed; pause/resume, final tail after Stop, indexing and shutdown during capture passed. The temporary test sink was removed and default devices were unchanged.

## Real local engines and full workflow

Generate public synthetic fixtures; no real/private recording is committed:

```bash
bash scripts/make-fixtures.sh
.venv/bin/python scripts/probe-engines.py --models .local-data/models \
  --speech /tmp/voxloom-budget.wav --unrelated /tmp/voxloom-garden.wav \
  --whisper-cli "$PWD/.runtimes/whisper-build/bin/whisper-cli" \
  --llama-cli "$PWD/.runtimes/llama-build/bin/llama-completion"
VOXLOOM_TEST_ROOT="$PWD" VOXLOOM_TEST_REPORT=/tmp/voxloom-stage-memory.json \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --offline \
  real_offline -- --ignored --nocapture
```

`probe-engines.py` prints `SKIPPED: model files not provisioned` with setup instructions when weights are absent. The Rust engine/device tests are explicitly ignored by the default unit run; opt in only with the supplied fixtures and weights. A skipped real engine is never evidence of successful inference. After the successful runs below, all development model weights and `.local-data/` setup state were deliberately removed at the user's request. Reprovision before repeating engine acceptance.

Observed passes with actual pinned weights:

- Whisper recognized the budget/invoice/Friday sentence from synthesized speech, persisted timestamped segments and exported TXT/MD/SRT/JSON.
- LiteRT 0.18.0 produced finite 768-dimensional query, document and native audio vectors from the full 740M checkpoint. The budget document scored 0.872 versus 0.616 for unrelated garden text. Native budget speech scored 0.772 versus 0.558 for garden speech. Audio inference consumed WAV directly, independently of transcript matching.
- The Rust integration test imported audio, ran ASR, indexed both modalities, reran/resumed index without increasing vector rows, retrieved a native-audio hit, generated an explicitly requested summary and grounded answer, reopened SQLite, exported, and cascaded deletion through FTS and derived data.
- The same workflow checked the one-click setup orchestration against already-installed verified files, with no downloads. The new search runtime installer was separately exercised in a fresh private venv through environment creation, pinned package install, dependency check and actual LiteRT API import. First-ever complete GUI provisioning on a clean OS is still unverified.
- No summary existed after transcription/indexing. Only the explicit summarize job created one. A short 1B response correctly retained `[00:00:03]`; quality on real counselling or business conversations is unverified.
- A shutdown test waited for a real resident model (process tree above 200,000 KiB RSS), closed the service, and checked observed worker descendants were gone or reaped. Native-window Quit separately checked WebKit helper exit. At the initial checkpoint no E2B weights were downloaded. The later 0.1.2 short generation regression reuses user-installed E2B weights; full business quality remains unverified.

On this CPU with two threads, a short Whisper fixture took approximately 2.1 s; embedding load plus three text calls approximately 1.6 s; two native audio calls approximately 1.3 s; the one-sentence summary probe approximately 3.3 s. These are tiny fixture results, not six-minute/multi-hour throughput guarantees. Generic CPU fallback binaries were also exercised on this host and were substantially slower (about 24 s for the short ASR fixture and 17 s for the short generation fixture). The app selects the optimized AVX2/FMA/F16C/BMI2 executable when supported. Performance of the generic fallback on actual older hardware is unverified.

## Real synthetic dual-source recording

```bash
python3 scripts/test-capture.py
```

This creates a temporary null output and remapped synthetic microphone, plays the fixture specifically into that test output, and invokes the real production FFmpeg capture path with both sources. It verifies live transcript arrival before stop, separate mic/system FLAC tracks, pause without timer advance, resume, and shutdown. Defaults are asserted unchanged and test endpoints are unloaded in finally. Latest observed full pass: approximately 66 seconds, including the 30-second capture window, final-tail transcription/indexing after Stop, and a second capture closed through shutdown. No physical microphone or speaker playback was recorded. Regression tests also exposed sub-second ASR/audio-encoder input failures. Inference now pads those tiny tails with silence to 1.1 seconds while preserving the original file and original timestamp end. Real audio-limit probes succeeded at 1, 2, 10, 20, 25 and 30 seconds. The first attempt exposed unknown-duration segmented FLAC headers; the initial implementation fell back to a null-muxer probe. Version 0.1.2 instead counts decoded PCM for capture segments, including inherited but non-`N/A` header durations.

## Native UI, offline boundary and memory

```bash
bash scripts/desktop.sh build --debug --no-bundle
xvfb-run -a python3 scripts/native-smoke.py src-tauri/target/debug/voxloom
python3 scripts/measure-tree.py --label check --output /tmp/tree.json -- <command>
```

Native checks launch the actual Tauri/WebKit app, dismiss first-run setup without downloading, shrink to a 390×92 recorder, expand to the studio, Quit, and check all observed app/helper processes exit. The same check passed on `release/VoxLoom/voxloom`, extracted from the final `.deb`, with fresh temporary data: exit code 0, four observed processes released, 441,820 KiB sampled idle process-tree RSS. The single-window design replaced a measured approximately 613 MiB two-webview prototype. Subsequent single-window samples were approximately 431–439 MiB combined RSS, without models loaded. Reclaimable Linux filesystem cache after model unloading is not a lingering inference process.

Runtime inference and the full Rust workflow were exercised in the restricted-network execution environment. A separate `strace -f -e trace=network` run observed no connect/send/receive network operations for Whisper, LiteRT text/audio and Gemma generation; LiteRT created one unconnected IPv4 datagram socket. UI asset requests were local only. The machine's real external network interface was **not** disconnected, and an installed fresh-machine offline launch/capture test is still required. Explicit onboarding downloads are intentionally outside the inference-only offline trace.

Sampling reads the whole process tree at about 150 ms intervals during real jobs (50 ms in the external measurement tool). Summed RSS counts shared pages per process, conservatively. The backend integration process and all inference descendants measured about 311 MiB for ASR, 542 MiB for indexing and 1,194 MiB for 1B summary generation. These stage measurements exclude the native WebKit UI, which is measured separately. Do **not** present their arithmetic sum as a measured simultaneous app total. The full GUI plus all workers under recording/index/summary load and explicit 16 GB memory pressure remain unverified. App diagnostics now save the actual host process-tree peak for each native inference job, so real business trials can collect that evidence.

See `docs/reports/` for representative fixture metrics and `PERFORMANCE.md` for disk/package measurements. No GPU, Arm/mobile or estimated model-size-based RAM claim is substituted for measured x86_64 results.

## Manual acceptance still required

Before calling this a stable distribution, test GNOME Wayland with a real mic and monitor, Teams/Zoom headset routing, output mute versus player mute, Bluetooth disconnect/profile changes, device unplug while recording, disk exhaustion, worker crash, multi-hour recording/seek and disk growth. Validate continuous speech around 30-second boundaries, overlap corrections, accidental echo, source alignment and audio intelligibility. Per-source playback advances through stored chunks on a global timeline, and source export concatenates the chunks; a synchronized microphone+system mix is not implemented. Browser tests exercise chunk advance/seek with real PCM through an explicitly mocked bridge; extended native playback still needs manual testing. A user-triggered import is streamed but does not yet have its own cancellation control. Automatic source-switch recovery and calibrated cross-device drift handling remain unfinished.

For the planned counselling-video comparison: capture System audio, review and correct the transcript, then Summarise with 1B. Export it before switching to an independently installed E2B model and regenerating. Compare factual accuracy, attribution, unsupported claims, missing topics, timestamp usefulness, latency and measured native process-tree RSS. No action was taken on the user's supplied YouTube URL.


## Version 0.1.3: meeting notes, copying and Word export

Summaries are now topic-based meeting notes: Overview, relevant discussion points, supported decisions/actions and next steps. Generation receives plain transcript text without source timestamp prefixes; Q&A retains its citation behavior. Stored notes are plain section titles and bullet text, with no Markdown syntax. Existing Markdown-style notes are rendered into normal headings/lists and their source codes are hidden. Actual times such as a Friday 09:30 deadline are preserved. The Transcript tab still provides recording timestamps.

Twenty-three Python checks cover uncited note generation, long-input reduction, completed native outputs without wrappers, omitted empty/unraised question sections, DOCX structure/XML escaping, styled headings and bullets, run spacing, file permissions and retained transcript timestamps. Seven Rust unit checks plus setup integration pass, as do clippy and seven browser checks. Browser checks read both HTML and plain clipboard formats, verify bold text/headings and the Word export request, and keep developer export formats out of the normal dropdown. The full real engine workflow including transcript and notes Word export passed in 48.15 seconds. It uses temporary libraries; user recordings/models remain untouched.

Both real local 1B and E2B checkpoints generated timestamp-free meeting notes from the same synthetic workshop-planning facts. The 1B model remains prone to repetition and classification errors; the app omits a generated Open questions section when the transcript contains no explicit question signal. Business/counselling factual quality is not certified by the format/fixture checks. Native end-of-generation markers allow a complete plain answer when a model omits the requested wrapper; incomplete drafts remain rejected. See reports/meeting-notes-0.1.3.json for the observed synthetic outputs.

The generated DOCX was opened by an isolated LibreOffice Writer profile and converted to PDF; extracted PDF text retained the title, section headings, bullets, names and deadline. This is an actual office-suite interoperability check, not a claim to have run Microsoft Word on Linux. No office suite is required by VoxLoom itself. The native WebKit app also passed an actual GTK clipboard readback of both text/plain and text/html on an isolated Xvfb display; the HTML retained heading and bold-name elements, and Quit returned 0. The native clipboard fixture never uses the user's library or desktop clipboard.

```bash
python3 scripts/test-meeting-notes.py --models /path/to/existing/models
xvfb-run -a python3 scripts/test-native-notes.py /path/to/voxloom
```

Final 0.1.3 package: native launch, compact/expand and Quit all passed (exit 0; four observed processes released; sampled idle process tree 402,808 KiB). All packaged note-generation/export helper files matched source hashes.
