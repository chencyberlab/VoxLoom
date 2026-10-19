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
Open issue (addressed in 0.1.7, see below): with Accurate (small.en), speech that starts right after a music bed ("Yeah, I guess I have to" at about 26 s of the counselling video) can be lost. The chunk is forced (no pause in the music), and Whisper stretches its last segment over the music and the new speech, so the skipped-speech retry thinks it is covered. A fix that ended such chunks early when the last segment carries under 10 characters per second (re-reading the tail with the following speech) recovered the phrase (missing words 9 → 5) but duplicated a re-read line (WER 5.8% → 8.0%). It is not shipped. Next: on retreat, drop the stretched segment from the commit instead of keeping it (let the next chunk re-read it whole), or cut forced chunks before speech onsets using a speech/music VAD (whisper.cpp's Silero model). Test with scripts/replay-live-asr.py on a recording of the video plus the user's YouTube transcript (not in the repo).


Version 0.1.6: E4B summaries, freeing disk space, always-visible progress
Implemented: Gemma 4 E4B Q4_0 (4.59 GB, ggml-org GGUF at a pinned revision and SHA-256) as a third summary choice in Settings and first-run setup; manual download only, never part of the default. Settings → Local intelligence: **Remove** on every downloaded model, plus **Free up space → Remove all models**. Removal deletes manifest files, unfinished `.part` downloads and the search model's LiteRT caches, never a custom GGUF, recordings, transcripts or settings. It refuses while recording, during setup or while any job is queued or running. Job progress moved from a panel at the bottom of each page to an indicator in a sticky top bar, with a drop-down for every job (Cancel), failures (Retry/Dismiss) and a brief "All done". Live passes during an active recording restart every 2.5 s, so they are shown as one steady "Transcribing live" item with a moving bar (only when a transcription model is installed) instead of a flickering percentage. The sidebar version now comes from package.json (it had shown 0.1.3 since that release).
Verified: see TESTING.md, "Version 0.1.6". E4B was deliberately not downloaded or run, at the user's request.
Known limitations: E4B quality, speed and memory are unmeasured. Estimated from one real E2B summary (58 s, about 4 GB peak for a 2.5-minute transcript at two threads): about 6 GB and twice the time, which a 16 GB laptop with other apps open may not manage. Google's AI Edge Gallery uses a 3.41 GB LiteRT build of E4B; using that format would need a second summary engine. A removed selected summary model shows as unavailable rather than switching profiles automatically.
Release result: release/voxloom_0.1.6_amd64.deb (14,999,418 bytes; no model weights) and release/VoxLoom-0.1.6/voxloom. Packaged manifest and provision.py match source. Native launch, compact/expand and Quit passed in Xvfb (exit 0; four observed processes released; idle sampled RSS 445,188 KiB).
Next: the open Accurate-after-music issue above. If E4B is tried on a larger machine, measure it with `scripts/test-meeting-notes.py --model e4b` and the summary metrics.


Version 0.1.7: counselling summaries, automatic titles, speech after music
Implemented: Settings → Summary style: Counselling or support session (new default), Service conversation, Meeting (the previous prompt, unchanged) and Custom instructions (up to 2,000 characters, wrapped in the same safety rules, passed to the worker in the request file). Counselling notes use Overview, Main concerns, Feelings and experiences, Strengths and supports and Agreed next steps, and forbid diagnosis, clinical labels, risk assessment and advice that was not said; the transcript has no speaker labels, so attribution is only made when clear. Saved notes mark titles with `## `; "Title: text" lines and sections that only say nothing was mentioned are tidied. The Summary tab heading follows the style. The last summarising pass also returns a short title that replaces only the default "Untitled recording". Live and file transcription: when a cut has no pause near it and Whisper's last segment is stretched over a music bed, the segment is kept, the chunk ends where its words must have ended, and the rest is decoded again with the following audio.
Verified: see TESTING.md, "Version 0.1.7". Counselling video (Standard / Accurate): word error rate 5.8% / 5.8% before, 6.2% / 5.1% after; missing words 8 / 9 → 6 / 6; "I guess I have to figure out things for myself" recovered. Three other real recordings produce word-for-word identical transcripts.
Known limitations: the first word after the music ("Yeah") is still missed. Gemma 3 1B can list "agreed next steps" that nobody agreed; E2B correctly reported none on the same session. Summary styles were checked on one real counselling recording; Service and Custom were exercised with unit and UI tests only.
Release result: release/voxloom_0.1.7_amd64.deb (15,003,420 bytes; no model weights) and release/VoxLoom-0.1.7/voxloom. Packaged engine.py, notes.py, provision.py and manifest match source. Native launch, compact/expand and Quit passed in Xvfb (exit 0; four observed processes released; idle sampled RSS 446,708 KiB).
Next: real Service and Custom summaries from the user's own conversations; a speech/music detector if music-to-speech losses remain.


Version 0.1.8: Gemma 4 E2B as the default summary model
Implemented: Gemma 3 1B removed. On the counselling recording it listed three "agreed next steps" nobody had agreed, where E2B correctly found none. Gemma 4 E2B is now the default and part of the standard download (no longer manual-only); E4B stays optional. A saved `1b` choice resolves to E2B. The manifest gains a `retired` list: a Gemma 3 1B file still on disk appears in Settings as "No longer used by VoxLoom" with Remove only, and nothing downloads it. Test downloads from the Gemma 4 transcription evaluation (6.4 GB in `.local-data/experiments/`, 3 GB of LiteRT caches in the session scratch folder) and the test-only `llama-mtmd-cli` binary were deleted; its 8.6 MB of object files remain in the engine build folder.
Verified: see TESTING.md, "Version 0.1.8".
Known limitations: first-run setup grows from about 1.5 GB to 3.6 GB. E2B needs about 4 GB of free memory while summarising.
Release result: release/voxloom_0.1.8_amd64.deb (15,003,902 bytes; no model weights) and release/VoxLoom-0.1.8/voxloom. Packaged engine.py, notes.py, provision.py and manifest match source; the package holds only whisper-cli and llama-completion (plus generic builds). Native launch, compact/expand and Quit passed in Xvfb (exit 0; three observed processes released; idle sampled RSS 434,488 KiB).
Next: user installs 0.1.8, then removes the old 1B file from Settings → Local intelligence.


Version 0.1.9: appearance, readability, single version file
Implemented: light/dark/System appearance on colour tokens (user's theme work), stored in settings and in local storage, applied before first paint by `public/theme-boot.js` (a file because the CSP allows only `script-src 'self'`). Review fixes: the sidebar no longer pushes Settings/Quit off screen at 900×720 (it scrolls, and the privacy card hides below 800 px tall); icons no longer shrink to dots in narrow flex rows; one focus ring around the search box. Readability: no text under 11 px (labels 12-13 px), consistent button heights, control borders at 3:1 contrast (`--control-line`), visible focus rings, card shadow in light mode. Settings split into Transcription & summaries (with Save), Local intelligence and Appearance; Advanced settings have their own Save. The sidebar's three-way switch became one button beside Quit. `VERSION` is the single version source (`scripts/version.py`).
Verified: contrast of every text/background token pair computed (all at least 4.5:1 in both themes); visual tour of every page in both themes at 1280×860 and 900×720; 15 browser checks including appearance cycling, persistence and the first paint after reload; `version.py set` changed exactly the six version fields on a copy.
Release result: release/voxloom_0.1.9_amd64.deb (15,005,652 bytes; no model weights; package version 0.1.9 read through tauri.conf.json → package.json) and release/VoxLoom-0.1.9/voxloom. Version bumped with `python3 scripts/version.py set 0.1.9`. Packaged engine.py, notes.py, provision.py and manifest match source; theme-boot.js is embedded. Native launch, compact/expand and Quit passed in Xvfb (exit 0; four observed processes released; idle sampled RSS 446,844 KiB).
Next: user installs 0.1.9 and checks both themes on the real desktop.


Version 0.1.10: queued-job message
Implemented: queuing a summary, transcription or search indexing said "You can cancel it below", left over from the progress panel that moved to the top bar in 0.1.6. It now says "Its progress shows at the top; click it to cancel."
Verified: a browser check clicks Summarise and reads the new message; full fast set and 13 browser checks pass.
Release result: release/voxloom_0.1.10_amd64.deb (15,005,634 bytes; no model weights; package version 0.1.10) and release/VoxLoom-0.1.10/voxloom, built with `python3 scripts/version.py set 0.1.10` and `build-app.sh --check`. The packaged web build has the new message and not the old one. Native launch, compact/expand and Quit passed in Xvfb (exit 0; three observed processes released; idle sampled RSS 434,876 KiB).
Next: user installs 0.1.10.


Version 0.1.11: floating recorder drags and has rounded corners
Implemented: the floating recorder could hardly be moved. Only its grip handle carried `data-tauri-drag-region`, and Tauri 2.12 starts a drag only on the marked element itself, so pressing the grip icon inside it did nothing. Now the whole bar is a drag region (`data-tauri-drag-region="deep"`), and buttons still click. The "VoxLoom / Ready when you are" text is part of the drag area; Expand still opens the studio. The bar's rounded corners showed square ones behind them because the window was opaque. The window is now created transparent (`tauri.conf.json`) and turns see-through only in compact mode (`set_background_color`), with a transparent page background behind the bar (`:root:has(.mini)`). The studio and startup keep the configured opaque colour, set again at setup: a transparent webview with tao's background paint would otherwise flash before the first page paint. Evaluated and not adopted: Moonshine Streaming as a transcription model (see TESTING.md).
Verified: see TESTING.md, "Version 0.1.11".
Release result: release/voxloom_0.1.11_amd64.deb (15,007,136 bytes; no model weights; package version 0.1.11; SHA-256 910a7642…68530f) and release/VoxLoom-0.1.11/voxloom, built with `python3 scripts/version.py set 0.1.11` and `build-app.sh --check`. The packaged web build has the deep drag region and the transparent compact background. Native launch, compact/expand and Quit passed in Xvfb (exit 0; four observed processes released; idle sampled RSS 457,136 KiB).
Limitations: dragging needs a window manager and was not exercised in Xvfb; it is left to the user's GNOME X11 desktop. Not tested on Wayland. Without a compositor, the corners show black instead of the desktop.
Next: user installs 0.1.11 and drags the floating recorder on the real desktop.


Version 0.1.12: Markdown copy, Windows preview, apps built by GitHub
Implemented: the summary page copies notes as rich text (Copy notes) or as Markdown (Copy Markdown); its Word document button is gone, and transcripts still export to Word. The app icon is a rounded tile. Windows 11 joins as a second platform of the same code (docs/WINDOWS.md, phases 1-2): a bundled runtime (engines, FFmpeg, Python with LiteRT, all pinned), Job Objects so a crash leaves nothing running, and a preview zip that imports, transcribes, searches and summarises but does not record yet. Builds use Node 24 LTS. GitHub's Apps workflow now builds both apps when VERSION changes on main and attaches them to a draft release. Linux behaviour is otherwise unchanged.
Verified: see TESTING.md, "Version 0.1.12".
Release result: built by the Apps workflow on main, not on this computer: voxloom_0.1.12_amd64.deb and VoxLoom-0.1.12-windows-x64-preview.zip on the draft release v0.1.12, with their SHA-256 in its notes and SHA256SUMS.txt. The draft is made only after both launch checks in that run pass.
Limitations: the Windows preview cannot record (phase 3), is not signed, has no installer (phase 4) and has not yet run on a real Windows 11 laptop. Upgrading the Linux package from 0.1.11 keeps recordings, models and settings.
Next: user installs 0.1.12 on Linux and tries the Windows preview on a Windows 11 laptop.


Version 0.1.13: code review fixes (reliability of live transcription, recovery, setup and search)
Implemented, from a review of the whole codebase rather than a user report:
- Live stream gap: `request()` read the growing file under a short capture lock and the registered tracks under a later library lock. A 30-second segment that closed between `tick`'s registration and that read was neither, so the live pass saw a stream with a hole and its words were lost, and the file registered one tick later was transcribed against a checkpoint that had moved past it. Registration and both reads now happen under the same two locks.
- Pause flushes the tail: only Stop queued the final live pass. A Pause taken while no pass was running left the unfinished sentence in the checkpoint until Resume; now Pause queues it too.
- Interrupted recordings: after a crash or power loss, start-up registered the segment files of a `recording` meeting but never transcribed them, and the meeting kept the length of its last Pause (or 0). Start-up now queues a final pass when any file is untranscribed or the last checkpoint carries an unfinished sentence, and decoded segment lengths extend the recording's length.
- Starting a recording no longer kills a running setup (pip into the LiteRT venv, or a model download); live passes wait for it instead. Recording refuses to start with under 256 MB free on the data volume (unknown free space is not treated as 0).
- A full job queue (8 pending) no longer turns away the live pass, which is the only way a recording's words reach the transcript.
- The worker time limit is max(2 h, twice the recording's length), so a long recording's summary on a slow laptop is not stopped half-way. The 300-character title limit counts characters, not bytes.
- Exact search applied the recording and source filters after the 500-row rank cap, so a common word with many hits in other recordings could hide every hit in the chosen one. The filters are now part of the query.
- `pactl` probes go through `process::hidden`, as the conventions require.
- Pinned models were hashed in full by every worker (each summary batch read the 2.8 GB GGUF again). A model whose size and modification time are unchanged since it last passed is trusted, recorded in `<cache>/verified-models.json`; any change hashes it again.
- Summary reduction no longer sends an odd leftover note through the model alone; it waits for a partner in the next round (one model pass fewer on three- and five-batch recordings). Indexing reuses the segment lengths transcription measured instead of decoding every file again.
- Setup: a `.part` download the server refuses to resume (HTTP 416) or that fails its hash is removed, so Retry works instead of failing the same way for ever and telling the user to delete a hidden file.
- UI: the title box is cleared once a recording starts, so the next recording does not inherit the name; "Save correction" is disabled for empty text, and the host rejects an empty correction.
Verified: see TESTING.md, "Version 0.1.13".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.13_amd64.deb and VoxLoom-0.1.13-windows-x64-preview.zip on the draft release v0.1.13, with their SHA-256 in its notes. Upgrading keeps recordings, models and settings; no database migration.
Known limitations: no real-engine or real-capture run in this pass (the development machine had no models); the new behaviour is covered by unit tests on the host and worker. Trusting size and modification time means a model file replaced by one of the same size with its timestamp preserved is not re-hashed; downloads and imports are still verified in full.
Next: user installs 0.1.13.


Version 0.1.14: fixes from the review of 0.1.13 (GitHub issue #5)
Implemented:
- Recovery after a crash no longer stops at a segment cut off before its first frame. A crash or power loss in the first fraction of a second of a new 30-second segment leaves a file FFmpeg cannot open at all (a 30-second segment first opens at about 11 KB; a file with at least one complete frame decodes fine, truncated or not). Start-up registered that file like any other, and `audio_streams()` raised on it before any transcript row was produced, so the segments recorded before it were never transcribed, Retry failed the same way, and every launch queued another failing pass. `stored_length()` now counts such a capture segment as empty and the pass continues over the other files. Its length is not cached, so a cached 0 can never hide audio. An imported file that cannot be read still fails with its error.
- A full disk refuses to record. `free_disk_bytes()` returned 0 both for a failed query and for a volume with nothing left, and the 0.1.13 guard skipped 0 as unknown, so a full volume reached `Capture::start` and showed the generic capture error. It now returns `None` for a failed query and `Some(0)` for a full volume, and `disk_too_full()` refuses the latter. The UI omits the free-space sentence when it is unknown instead of showing "0.0 GB".
Verified: see TESTING.md, "Version 0.1.14".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.14_amd64.deb and VoxLoom-0.1.14-windows-x64-preview.zip on the draft release v0.1.14, with their SHA-256 in its notes. Upgrading keeps recordings, models and settings; no database migration.
Known limitations: no real-engine or real-capture run in this pass; the stub case is covered by worker tests with real FLAC files and a fake Whisper. A segment with no complete frame stays registered (it is harmless: audio export tolerates a trailing file it cannot open, and the file is measured again by each pass over that recording, one failed open each time).
Next: user installs 0.1.14.


Version 0.1.15: fix from the review of 0.1.14 (GitHub issue #6)
Implemented:
- Search preparation no longer fails on a crash stub. 0.1.14 made the recovery pass count a segment FFmpeg cannot open as empty and, on purpose, cached no length for it. `index()` still measured any file without a cached length with `duration()`, so on a machine with the embedding model installed the "Preparing search" job that follows a successful recovery pass failed with "Saved audio could not be decoded" before emitting any embedding, and Retry, Prepare search and Rebuild index failed the same way. Exact word search still worked (rows were in FTS); meaning search had no vectors for that recording. `index()` now takes its lengths from `track_lengths()`, which uses `stored_length()`; the stub's 0 adds no audio windows and the text embeddings run. No other job measures files directly.
Verified: see TESTING.md, "Version 0.1.15".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.15_amd64.deb and VoxLoom-0.1.15-windows-x64-preview.zip on the draft release v0.1.15, with their SHA-256 in its notes. Upgrading keeps recordings, models and settings; no database migration.
Known limitations: `index()` itself was not run (no LiteRT on the development machine); the length list it uses is tested with the real files, and the rest of `index()` is unchanged.
Next: user installs 0.1.15.


Version 0.1.16: recording on Windows (Windows phase 3)
Implemented: the Windows preview records the microphone and system audio through WASAPI (`audio/wasapi.rs`), into the same `sNNNN-mic-` / `sNNNN-system-` FLAC segments, with the same level meters, as Linux. FFmpeg cannot open WASAPI devices, so a capture thread reads them and `audio/pcm.rs` puts both sources on one clock: it joins jittery packets, writes silence where a source heard nothing (loopback sends nothing while nothing plays), and never drops captured audio. One FFmpeg reads the sources as channels on stdin and splits them (`channelsplit`, newly in the bundled FFmpeg). A source chosen as the default device follows the default, so a headset plugged in mid-meeting takes over. A disconnected device is replaced within a second by the same one or the default, and the studio shows a notice until then. When Windows' "Let desktop apps access your microphone" switch blocks the microphone, the message names it and offers "Open microphone settings". Linux records exactly as before: ending capture moved behind `backend::finish`, which still sends "q", and the status gains a `warning` only when a backend reports one. The `windows` crate is the version Tauri already uses, so nothing new is downloaded.
Verified: see TESTING.md, "Version 0.1.16".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.16_amd64.deb and VoxLoom-0.1.16-windows-x64-preview.zip on the draft release v0.1.16, with their SHA-256 in its notes and SHA256SUMS.txt.
Known limitations: recording on Windows has run only on machines without audio devices. Real microphones, loopback, headset switching and the privacy switch need a Windows 11 laptop. The preview is unsigned and has no installer (phase 4).
Next: user records on a Windows 11 laptop with the 0.1.16 preview.


Version 0.1.17: Windows setup works after moving to a newer preview
Implemented: on Windows 0.1.16, model setup failed with "The system cannot find the path specified" and the downloaded models looked unreadable. Cause: Settings' Save button sends back every setting it shows, including the full paths of the bundled Python and engines, and those were stored. They pointed into the folder the preview was unzipped to. Once a newer preview ran from another folder and the old one was deleted, setup, the model check and transcription started a Python that no longer existed. Recording was unaffected because it uses only FFmpeg. Now `Service::tool` uses a saved Python, Whisper or llama path only while it exists and does not belong to another copy of VoxLoom (a folder with its own `voxloom.exe` beside `python/` or `engines/`); otherwise it uses the running copy's own. Saving a path equal to the default stores nothing, so the default keeps following the app. A custom path chosen under Advanced settings is kept. Libraries that already hold stale paths recover without the user doing anything. Linux paths never move, so Linux behaves as before.
Verified: see TESTING.md, "Version 0.1.17".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.17_amd64.deb and VoxLoom-0.1.17-windows-x64-preview.zip on the draft release v0.1.17, with their SHA-256 in its notes and SHA256SUMS.txt.
Known limitations: the fix was checked with a regression test and on GitHub's Windows machines, not yet on the user's laptop.
Next: user reruns model setup on Windows with 0.1.17, then tests transcription of recordings.


Version 0.1.18: fix from the review of 0.1.17 (GitHub issue #7)
Implemented: 0.1.17 ignored every saved tool path two folders below a folder holding a `voxloom` executable. That included the running Windows preview's own folder: choosing its generic engine build (or any tool placed inside it) under Advanced settings was ignored, and the next save of Settings erased the choice. A custom path that was missing for the moment was also erased by any save, because Settings showed the default and sent it back. Now `Service::other_copy` ignores only a path in the `engines` or `python` folder beside the app of a copy other than the running one, and `save_settings` changes a tool path only when the user edited it: saving what Settings shows keeps the stored value, and choosing the default stores nothing.
Verified: see TESTING.md, "Version 0.1.18".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.18_amd64.deb and VoxLoom-0.1.18-windows-x64-preview.zip on the draft release v0.1.18, with their SHA-256 in its notes and SHA256SUMS.txt.
Known limitations: checked with the regression test and on GitHub's Windows machines, not yet by choosing an engine in the packaged Windows preview.
Next: user tests model setup and transcription on Windows with 0.1.18.


Version 0.1.19: Windows installer (Windows phase 4)
Implemented: `VoxLoom-<version>-windows-x64-setup.exe`, Tauri's NSIS installer in both modes: for the current user (`%LOCALAPPDATA%\Programs\VoxLoom`, no administrator) or for everyone (`C:\Program Files\VoxLoom`), with silent switches `/S /CurrentUser` and `/S /AllUsers`. It bundles what the preview zip does (engines, FFmpeg, Python with LiteRT, licences); WebView2 comes with Windows 11, so the installer does not download it. Uninstalling removes everything VoxLoom put on the computer: the program folder whole (with whatever was written into it later) when it is VoxLoom's own, shortcuts, the Apps entry, the remembered install location, Windows' microphone record, and the downloaded models, cache and browser data in `%LOCALAPPDATA%\org.voxloom.desktop`. The library and recordings stay unless the user ticks "Also delete my recordings, transcripts and settings" or passes `/DELETEDATA`. Updates remove nothing. Bundled Python no longer writes bytecode caches into the program folder. The Apps workflow builds the installer, installs and uninstalls it in both modes, and attaches it, the zip and a list of every program file with its SHA-256 (for IT allow-listing) to the draft release.
Verified: see TESTING.md, "Version 0.1.19".
Release result: built by the Apps workflow when this version reaches main: voxloom_0.1.19_amd64.deb, VoxLoom-0.1.19-windows-x64-setup.exe, VoxLoom-0.1.19-windows-x64-preview.zip and VoxLoom-0.1.19-windows-x64-programs-SHA256.txt on the draft release v0.1.19, with their SHA-256 in its notes and SHA256SUMS.txt.
Known limitations: not code-signed, so SmartScreen warns and managed laptops may block it. Uninstalling a per-machine install cleans only the data of the account that runs it. A models folder chosen under Advanced settings is never deleted. Checked on GitHub's Windows Server machines, not yet on a Windows 11 laptop.
Next: user installs 0.1.19 on a Windows 11 laptop (per user and per machine), records, and uninstalls.
