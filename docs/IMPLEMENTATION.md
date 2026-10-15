# Implementation checkpoints

The starting repository contained README.md and the user-supplied development brief only.

Plan: establish and exercise the Tauri/React/Rust shell and SQLite core; implement import, Whisper and provisioning; verify the exact 740M LiteRT backend before integrating native audio/text search; then add Linux capture and optional grounded generation. Keep each engine's measured acceptance separate from implemented control flow.

Environment discovered 2026-10-15: Ubuntu 24.04.5 x86_64, 15,822 MiB physical RAM, Node 22.23.1, Python 3.12.3. Rust, GTK/WebKit development libraries, FFmpeg and pactl were initially absent. sudo package installation requires an interactive password; native checks depend on the user installing the documented packages.

## Final checkpoints — 2026-10-15

The user installed native system dependencies. A project-local Rust toolchain and pinned CPU inference engines were then built. Source and original brief were preserved; no commits were requested.

Stage 0: Native shell and data layer
Implemented: Tauri/React studio and library, private SQLite migrations/FTS, local assets, diagnostics and privacy scanner.
Verified: production build; TypeScript/privacy check; five Rust unit tests; clippy with warnings denied; three browser UI tests.
Known limitations: native UI checks use isolated Xvfb, not physical GNOME Wayland.
Next: real import and ASR.

Stage 1: Import and transcribe
Implemented: local audio/video import, Whisper Base English, timestamp edits, persistence and transcript exports; explicit model downloads and verified offline imports.
Verified: real pinned Whisper fixture, full Rust restart/export/delete workflow, ten Python unit tests including sub-second tail padding and corrupt imports.
Known limitations: English default; imported jobs have no dedicated cancellation control.
Next: native audio/text retrieval.

Stage 2: Full 740M embeddings
Implemented: LiteRT CPU native audio and text embeddings, revisioned/resumable index, hybrid semantic/FTS search and timestamp navigation.
Verified: actual 768-dimensional finite vectors; relevant text and native audio outranked unrelated fixtures; repeat index did not duplicate vectors; real workflow passed in 19.35 seconds. Fixture measurements are in reports/.
Known limitations: full GUI-plus-worker memory pressure and large-library performance remain unverified.
Next: Linux dual-source capture.

Stage 3: Dual-source recording
Implemented: separate microphone/monitor FLAC tracks, meters, 30-second transcription batches, pause/resume, final-tail ASR and indexing, continuous per-source playback and concatenated source export.
Verified: actual PipeWire synthetic dual-source test passed in about 66 seconds; live transcript arrived before stop; all final tracks transcribed; defaults unchanged; shutdown flushed and released capture. Real FFmpeg export test covered quoted and Unicode paths.
Known limitations: physical microphones/headsets, Wayland behavior, drift recovery, disk-full and multi-hour reliability need manual testing. No synchronized mic+system playback mix.
Next: explicit summaries and Q&A.

Stage 4: Optional local generation
Implemented: explicit Summarise and grounded Q&A only, bounded hierarchical processing and citations; selectable 1B/E2B profiles; sequential model residency.
Verified: real 1B fixture retained the evidence timestamp; no summary appeared automatically; active-model shutdown verified workers were reaped. Inference network trace recorded no connect/send/receive calls.
Known limitations: business/counselling quality and E2B inference unverified; physical external networking was not disconnected.
Next: first-run UX and release preview.

Stage 5: Linux preview distribution
Implemented: one-click setup for private runtime and chosen models, optional summaries, progress/cancel/retry, offline import and collapsed advanced controls; bundled optimized/generic CPU engines; one resizable floating recorder.
Verified: fresh private runtime install/API import; existing-model setup orchestration; native extracted-release launch, 390×92 compact/expand, Quit exit 0 and four observed processes released. Sampled idle RSS 441,820 KiB (431.5 MiB). Final package release/voxloom_0.1.0_amd64.deb is 14,949,138 bytes and contains no AI model weights. Portable launcher: release/VoxLoom/voxloom.
Known limitations: a fresh OS installation and first-ever full GUI download remain unverified; this is a manual-testing preview.
Next: user first-run and real business/device testing.

Fresh-start handoff: after all real-engine validation, removed the three downloaded models (1,353,012,147 bytes total), the empty development library, private setup runtime and caches by deleting task-owned .local-data/. Verified no copies of the manifest's model weights remain in the workspace. No pre-existing default user VoxLoom data/cache existed. Build tools and source dependencies remain for development; the released app does not discover those development-only runtimes.


Version 0.1.1: First-run summary model choice
Implemented: an optional 1B/E2B picklist with download sizes, simple tradeoff explanations, selected-model progress and total setup estimate. One-click and offline setup install core models plus only the chosen summary model, then persist the successful choice. Settings can reopen the wizard for existing users.
Verified: TypeScript/privacy checks; six browser tests; five Rust unit tests and the offline provisioning regression covering both models, skipping summaries, missing files and an invalid profile; desktop clippy with warnings denied. Fixture provisioning uses tiny integrity-checked files and a documented runtime probe stub, without real model downloads.
Known limitations: E2B quality and memory are still unverified. This update does not reset user data or models while the user tests the installed 0.1.0 build.
Release result: release/voxloom_0.1.1_amd64.deb (14,955,318 bytes; no model weights) and release/VoxLoom-0.1.1/voxloom. The final extracted package passed native launch, compact/expand and Quit (exit 0; three observed processes released; sampled idle process-tree RSS 441,724 KiB). The setup action is fully visible at 900×720 for all three choices in browser checks.
Next: user installs 0.1.1 after the current manual trial, then opens Settings → Model setup to select a summary model without resetting the library.


Version 0.1.2: Continuous transcription and summary response handling
Implemented: source/session audio continuity across storage files; exact cached decoded FLAC durations and safe seek; pause-aware buffered ASR with bounded 120-second overlap fallback; atomic transcript/checkpoint batches, retry/restart and edit protection; completed summary extraction and hidden legacy drafts; pinned llama.cpp reasoning-option forwarding patch in both CPU binaries.
Verified: original cross-file PCM preserved byte-for-byte; real 67.574-second fixture retains both complete boundary-crossing sentences. Sixteen Python tests, six Rust unit tests plus setup integration, six browser tests and clippy pass. Real full workflow passed in 22.18 seconds and synthetic dual-source capture passed in 55.86 seconds. Short E2B generation produces finished cited notes; 1B safely falls back to evidence extracts when citation checks fail. User-installed models were reused read-only; no downloads or user-library changes.
Known limitations: word substitutions remain possible with Base English. Long nonstop speech has bounded-overlap unit coverage; multi-hour capture, business summary quality and full native memory pressure still require manual testing.
Release result: release/voxloom_0.1.2_amd64.deb is 14,964,076 bytes; portable app is release/VoxLoom-0.1.2/voxloom. Packaged source/binary hashes match, with no model weights included. Final native launch, compact/expand and Quit passed (exit 0; four observed processes released; idle sampled RSS 434,888 KiB).
Next: user installs 0.1.2, reruns Transcribe on the saved complete audio, waits for completion and requests a new summary. Existing user models, recordings and settings are preserved.


Version 0.1.3: meeting notes for ordinary document workflows
Implemented: topic-based plain meeting notes without source timestamps or visible Markdown; normal headings/paragraphs/bullets; Copy notes with rich HTML and plain text; Word (.docx) export for notes and transcripts. Transcript timestamps and cited Q&A remain available separately. Normal export choices are Word, text and subtitles; legacy developer formats remain in the backend.
Verified: 23 Python checks, seven Rust unit checks plus setup integration, seven browser checks, TypeScript/privacy and clippy. The full real workflow includes both Word exports and passed in 48.15 seconds. Both local model profiles generated source-timecode-free workshop notes; factual business quality remains a manual assessment. Native WebKit-to-GTK clipboard readback passed both HTML and plain text, with no visible markup. A generated DOCX opened in LibreOffice Writer and converted to PDF with headings, bullets and deadline intact.
Known limitations: 1B can repeat or misclassify facts; generated notes still need review. Word-format interoperability was exercised with LibreOffice, not Microsoft Word itself. Extended GNOME Wayland clipboard/device behavior remains part of manual acceptance.
Release: release/voxloom_0.1.3_amd64.deb (14,977,428 bytes) and release/VoxLoom-0.1.3/voxloom. Final native launch, compact/expand and Quit passed (exit 0; four observed processes released; idle sampled RSS 402,808 KiB). Bundled workers, note/export helpers, manifest and source notices match the tested source hashes; no model weights are included.
Next: user installs 0.1.3 and regenerates saved summaries. User models and recordings are preserved.


Version 0.1.4: live transcription without cut or lost sentences
Implemented: pause-aligned ASR chunks of at most 28 seconds using an adaptive speech-band level detector (replacing the fixed −60 dB detector that never fired on real audio and the 120-second overlap fallback); no overlapping audio or text deduplication; retry of voiced stretches Whisper returned no words for; one sentence per row with unfinished sentences carried in the checkpoint; live passes every 2.5 seconds that read the FLAC file still being recorded (`flush_packets=1`); a final pass whenever a provisional pass finishes after Stop/Pause; completed live-pass job rows are removed. Replay tool: scripts/replay-live-asr.py.
Verified: see TESTING.md, "Version 0.1.4". Real recordings: 100% coverage, no duplicated boundary text, microphone WER 21.8% → 19.8% against a large-v3-turbo reference, live text after pauses instead of after about two minutes. Real PipeWire capture showed live text before the first storage file closed.
Known limitations: accuracy is bounded by Whisper Base English, especially on distant or noisy microphones; small.en measured 15.2% versus 19.8% WER on the same audio at about three times the CPU cost and is not yet offered. During continuous talk, text arrives in bursts every 10–30 seconds. Multi-hour capture still needs manual testing.
Release result: release/voxloom_0.1.4_amd64.deb (14,985,880 bytes; no model weights) and release/VoxLoom-0.1.4/voxloom. Packaged worker and manifest match source; engines are byte-identical to the pinned 0.1.3 builds. Native launch, compact/expand and Quit passed in Xvfb (exit 0; four observed processes released; idle sampled RSS 401,016 KiB). Seven browser checks, TypeScript/privacy and desktop clippy pass.
Next: user installs 0.1.4 and records a long real conversation (mic and/or system audio). Existing recordings can be re-run with Transcribe to use the new segmentation.


Version 0.1.5: optional accurate transcription
Implemented: Settings → Transcription (Standard / Accurate). Accurate uses Whisper Small English Q8_0 (264 MB, pinned revision and SHA-256), an optional manual download in Model setup, never part of first-run setup. The host falls back to Standard until it is downloaded. Applies to live transcription and to Transcribe. Build tooling: scripts/build-app.sh, AGENTS.md, CLAUDE.md.
Verified: see TESTING.md, "Version 0.1.5". Ground truth (YouTube transcript): Standard 4.7% WER, Accurate 5.8% on clean system audio with a music intro; on a noisy microphone Accurate was 15.2% versus 19.8%.
Known limitations: speech that begins under music can be skipped by Whisper at the music-to-speech transition, more often with small.en. Accurate costs about three times the CPU.
Open issue (next session): with Accurate (small.en), speech that starts right after a music bed ("Yeah, I guess I have to" at about 26 s of the counselling video) can be lost. The chunk is forced (no pause in the music), and Whisper stretches its last segment over the music and the new speech, so the skipped-speech retry thinks it is covered. A fix that ended such chunks early when the last segment carries under 10 characters per second (re-reading the tail with the following speech) recovered the phrase (missing words 9 → 5) but duplicated a re-read line (WER 5.8% → 8.0%). It is not shipped. Next: on retreat, drop the stretched segment from the commit instead of keeping it (let the next chunk re-read it whole), or cut forced chunks before speech onsets using a speech/music VAD (whisper.cpp's Silero model). Test with scripts/replay-live-asr.py on a recording of the video plus the user's YouTube transcript (not in the repo).

