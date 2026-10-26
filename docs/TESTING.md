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

## Version 0.1.28: fixes from the review of 0.1.27 (issue #12) and a Windows test

Both findings of the review of 9d9d6f5 were confirmed and fixed, and a third problem turned up while checking them in WebKitGTK:
- **Play cancelled a jump that was waiting to start** (medium). A transcript row or search result asked the part to start but left the button on Play, and Play while the part was loading flipped that request off. Now the jump shows Pause at once, and while a part loads the button follows what it shows (Play asks, Pause cancels). WebKitGTK, built UI, jump while the bytes were 2.5 s away: on 0.1.27 the button still said Play 300 ms later; now it says Pause.
- **After a failed copy, Play showed Pause and never retried** (low). Play now asks the host again, clears the error and starts the part when the copy arrives. WebKitGTK: on 0.1.27 one `playback` call and Pause with nothing playing; now a second call, playing, no error.
- **A jump to a part still loading stalled in WebKitGTK** (found here; Chrome was fine). The jump seeked the element while it had nothing loaded; WebKitGTK kept that as a start position, applied it after play began, and playback from the Blob stopped at 1.0 s (`stalled`). The same sequence in a bare page, from an `https:` or a `tauri:` origin, played through, so only the app's sequence triggers it. Now only an element that has the part loaded is seeked; `onLoadedMetadata` seeks the part when it arrives. Jump while loading: playing from 1 s three runs out of three (1.31-1.32 s shortly after); jump within a part already playing: from 0.27 s to 1.67 s 0.7 s later, still playing.
- **Windows unit test** (CI on 9d9d6f5): `a_part_asked_for_twice_at_once_is_decoded_once_for_both` compared the returned path with one built as `cache/playback` as text, which fails on Windows only. The tests now build paths one part at a time and compare them as paths. The same CI run's Windows real-engine job ran Accurate on Windows for the first time: the workflow test's Accurate pass passed, and the recording pass (33.5 s, stopped after one chunk and resumed) covered every millisecond with Parakeet.

Validation: TypeScript/privacy/version (0.1.28), 55 Python checks, 24 browser checks (new: a jump while the part loads shows Pause and starts it; after a failed copy Play asks again, both failing on 0.1.27), 36 Rust unit checks plus the offline setup integration check, clippy.

## Version 0.1.27: fixes from the review of 0.1.26 (issue #11)

Both findings of the review of e0e4b3e were confirmed and fixed:
- **Play pressed before the part could play** was lost (medium). The click called `play()` on an element with no source yet, or with the `asset:` URL WebKitGTK refuses, showed the error and left the part paused once its Blob arrived. Now, until the selected part can play (its copy is still being made, or on Linux its bytes are on the way, or it has not been asked for yet), Play only asks for it to start when it loads, and a second press cancels that. WebKitGTK, VoxLoom's built UI, Play pressed the instant the recording opened, five runs each with the copy delayed 700 ms, the bytes delayed 900 ms, or neither: all 15 played with no error. The same check on the 0.1.26 UI failed all 9 runs (AbortError, part left paused). Browser tests for both delays failed before the fix and pass after.
- **Two decodes of the same part** (low): choosing a part also prepares the next one, so a quick jump to it could start a second FFmpeg into the same `.part` file; one caller failed, and the UI then played the capture file itself, whose clock is wrong in WebView2. Decodes are now serialised, and a failed copy is reported instead of falling back to the capture file. `a_part_asked_for_twice_at_once_is_decoded_once_for_both` (eight callers at once): failed three runs out of three before, and passed five out of five after, with every caller given the same WAV, about 6 s long whatever its place in the recording, and no partial file left.

Validation: TypeScript/privacy/version (0.1.27), 55 Python checks, 22 browser checks, 36 Rust unit checks plus the offline setup integration check, clippy for desktop and for the Windows target.

## Version 0.1.26: playback works on Linux

The user reported that Play showed "NotSupportedError: The operation is not supported." on 0.1.25 (Ubuntu 24.04, WebKitGTK 2.52.6, GStreamer 1.24.2). It was not caused by 0.1.25. A standalone WebKitGTK page served a capture-style FLAC segment over a custom `asset:` scheme the way Tauri does, and failed the same way (`MEDIA_ERR_SRC_NOT_SUPPORTED`). GStreamer's FLAC parser and decoder were installed.

Cause:
- WebKitGTK refuses media from schemes it does not know (log: `isProtocolAllowed: Requested protocol: asset (allowed: no)`). With `WEBKIT_GST_ALLOWED_URI_PROTOCOLS=asset` it gets past that check, then GStreamer reports `No URI handler implemented for "asset"`. Streaming from `asset:` cannot work in this WebKitGTK.
- A capture segment also cannot play as it is, in either webview. Its FLAC frames carry their position in the whole recording and its header no length. In WebKitGTK (from a Blob) its duration read 0 and a seek to 10 s was ignored. In both WebKitGTK and Chrome (the engine WebView2 uses) the second segment of a fixture ended at 33.5 s instead of 3.5 s, and the sixth file of a real recording at 152.9 s; the player adds that clock to the file's place in the recording.
- The browser UI tests run in Chrome against WAV fixtures, so they never saw either problem.

Fix and validation:
- The `playback` action decodes a capture segment to WAV in the private cache. In WebKitGTK, from a Blob, a WAV segment reported 30.024 s, a seek to 10 s played on from there (11.19 s after 1.2 s), and the 3.5 s second segment ended at 3.48 s; an imported `original.flac` (one ordinary file) reported 33.51 s and seeked the same way, so imports play as they are.
- When `<audio>` reports the source unsupported, the UI asks the host for the bytes (`playback_bytes`, raw IPC) and plays a Blob URL. `media-src` gained `blob:`; `connect-src` is unchanged, and the UI makes no `fetch` (the privacy check forbids it).
- VoxLoom's built UI (`dist/`) in WebKitGTK, served at `tauri://localhost` with recordings on a real `asset:` scheme and only the host calls stubbed: Play played the part from a Blob with its 30.02 s duration and the clock moving (1.29 s after 1.2 s). Without the bytes fallback it showed the user's error.
- Moving on to the next part (reported by the user on the first 0.1.26 build): at the end of a 30-second part playback went silent while the button still showed Pause. The next part's file now arrives a moment after the track changes, and the effect that resumes playback ran before it, on the finished part: it used up the autoplay and started the old part again, and when the new file loaded the browser stopped it without a pause event. Now that effect touches the element only once it holds the selected part (the new part's `loadedmetadata` seeks and resumes), and an `emptied` event shows Play. WebKitGTK, built UI, two 3-second parts: before, the second part never started (paused, time 0) and the button showed Pause; after, three runs out of three, the second part played on with 35-37 ms of silence between the parts, and the button showed Play after the last one. The next part is also decoded as soon as a part is chosen; decoding a 30-second part takes 30-40 ms here, longer where antivirus delays starting FFmpeg. The Chrome playback test now checks that the next part actually plays and that the button returns to Play; it failed before the fix.
- The button no longer flickers at each boundary (also reported by the user): a part that ends, or a file swapped while playback moves on by itself, leaves it on Pause; a part that cannot be read or playback that cannot start shows Play. WebKitGTK, built UI: labels from Play to the end went Pause, Play, Pause, Play before and Pause, Play after (three runs), with the second part playing and 28-31 ms between parts; when the second part's bytes could not be read, the button showed Play. The Chrome playback test records every label and expects exactly Pause, Play; on the 0.1.25 code it got four.
- Rust: `capture_segments_play_from_a_decoded_copy_that_goes_with_the_recording` (FFmpeg-made capture segments: a WAV copy that is decoded once; imports play their own file; opening another recording removes the first one's copies; deleting a recording removes its copies; an unknown track is refused). Browser: the player asks for the host's copy, and after an unreadable response plays from the host's bytes.
- Fast set: TypeScript/privacy/version (0.1.26), 55 Python checks, 20 browser checks, 35 Rust unit checks plus the offline setup integration check, clippy for desktop and for the Windows target.
- Package: `release/voxloom_0.1.26_amd64.deb` (SHA-256 e639499a…bd5b04) passed the native smoke check: launch, floating recorder and back, Quit with exit code 0, all 4 observed processes released.
- Read, not run: playback on Windows. WebView2 streams the WAV copies from `asset:`; the bundled FFmpeg has the WAV muxer and PCM encoder.

## Version 0.1.25: Parakeet for Accurate transcription

Tested on the Surface Laptop Studio (i7-11370H, 4 cores, Ubuntu 24.04) with the desktop in use.

Validation:
- Fast set: TypeScript/privacy/version (0.1.25), 55 Python checks (new: Parakeet word grouping, text kept whole when its word list is missing or disagrees, empty chunks, times past a chunk's end, engine choice, and the skipped-speech retry through Parakeet), 19 browser checks (setup offers Accurate at +600 MB and 4.2 GB in total; Settings downloads model `parakeet`), 34 Rust unit checks plus the offline setup integration check (Accurate chooses the Parakeet file and `asr_engine` `parakeet`, never the retired Whisper Small file), clippy for desktop and for the Windows target. The visual tour was captured in light and dark; the settings page lists "Transcription · more accurate, 600 MB".
- Engines: `setup-runtimes.sh --engines-only` built transcribe-cli from its pinned archive (SHA-256 96a3993c…cab4a5) as an AVX2 build (5,024,560 bytes) and a generic one (4,543,856 bytes); both start.
- Provisioning: `provision.py --download --model parakeet` downloaded 600,408,576 bytes and verified the pinned SHA-256.
- `test-transcript-continuity.py --transcribe-cli`: both engines covered all 67,574 ms and wrote the same two sentences across the 30- and 60-second file edges.
- `transcribe-recording.py --engine parakeet` on copies of three real recordings, deleted afterwards: 84cc8615 40 rows at 0.15 of real time, 76e99b52 20 rows at 0.16, d34b2474 61 rows. Every millisecond was covered, and the row counts equal the evaluation's below. Stopped after three chunks and resumed, 76e99b52 gave the same 20 rows.
- Real-engine workflow test (`real_offline_import…`, Linux): passed in 54.5 s with its new Accurate pass. Transcribe started over with Parakeet and heard "budget". On that 10-second fixture Standard took 1.9 s with a 0.28 GiB engine peak, and Accurate 2.2 s with 0.93 GiB.
- CI's new Accurate step, run here: the long fixture as 30-second FLAC files in a folder named "recording Zoë accurate", stopped after the first of two chunks and resumed: 33,507 ms covered, "budget" six times.
- Windows: `build-windows-runtime.sh --parts engines` with Ubuntu 24.04's mingw-w64 packages unpacked locally built all six engines. `transcribe-cli.exe` (7,811,499 bytes) and its generic build import only ADVAPI32, KERNEL32 and msvcrt and carry the UTF-8 code-page manifest. `licenses/` gained `transcribe.cpp-LICENSE.txt` and `transcribe.cpp-THIRD-PARTY-LICENSES.md`, and `VERSIONS.txt` lists transcribe.cpp v0.4.1. They were not run: there is no Windows machine here.
- Package: `build-app.sh` reused the engines and made `release/voxloom_0.1.25_amd64.deb` (19,148,454 bytes, SHA-256 0ae08b1a…dd9cd5), with transcribe-cli and its generic build in `/usr/lib/voxloom/engines`. Native smoke (Xvfb) on the extracted package: launch, floating recorder and back, Quit with exit code 0, all 4 observed processes released; idle single-webview peak 446,888 KiB.
- Read, not run: the Windows CI steps (runtime smoke, Accurate workflow pass, Accurate recording pass) and the installer check, which now expects `engines/transcribe-cli.exe`.

## Parakeet TDT 0.6B v2 as a transcription model (evaluated after 0.1.24)

Question: should NVIDIA's Parakeet TDT 0.6B v2 (English only, CC-BY-4.0) replace Standard or Accurate? Windows 11 work laptops are short of RAM and already slow with company monitoring software, so memory was weighed as heavily as accuracy. Published Open ASR Leaderboard average: 6.05%, against 10.32% for base.en and 7.44% for large-v3. Published LibriSpeech test-clean figures for the GGUF files: Q8_0 1.69%, Q6_K 1.70%, Q4_K_M 1.72%.

Setup, all in a temporary folder outside the repository:
- Engine: transcribe.cpp v0.4.1 (github.com/handy-computer/transcribe.cpp, commit 4d7c1cc, MIT, released 2026-10-22), `transcribe-cli`, CPU only. It was built with the same SIMD flags as VoxLoom's whisper-cli (AVX, AVX2, FMA, F16C, BMI2, no AVX-512), with `TRANSCRIBE_USE_SYSTEM_BLAS=OFF` (its official-wheel setting; a Windows package would bundle no BLAS) and its default thread pool, not OpenMP.
- Output: `--timestamps word` prints word times on stdout. `-o` writes only the plain text, and `--batch-jsonl` only segment times.
- Models: handy-computer/parakeet-tdt-0.6b-v2-gguf, revision 58239cf6d0ecb0af921e7936c1e5e0a8dba91dbb. These are conversions of nvidia/parakeet-tdt-0.6b-v2 at commit 1b149a3, not retrained.
  - Q8_0: 729,574,912 bytes, SHA-256 f0d0e99cebb6d3b83f1f7069b82b5d3c2e39a54545b0da039cb4bafd9c4e5caa.
  - Q6_K: 600,408,576 bytes, 44c7c363438e03b613cf8ced8b7e6fd1beb2b3805008136fe5e922ab3c79eea9.
  - Q4_K_M: 475,491,840 bytes, 4853f9653f641d376e6f7de65d73c7a34a73677704a606727bf51acc83f999f3.
  - Licence CC-BY-4.0: shipping them needs attribution to NVIDIA and a note of the conversion in the app's notices.
- What transcribe.cpp does at load, which a release build cannot turn off:
  - It runs the full-attention checkpoint with local attention [256, 256], NVIDIA's long-audio mode. That is exact up to 20.5 s, so it changes only the far ends of a 28 s chunk.
  - It converts 48 F16 convolution weights to F32 for the CPU, about 300 MB of private memory at every quantisation.

Method:
- `scripts/transcribe-recording.py` ran unchanged from a harness that replaced `engine.whisper_pieces` in memory with transcribe-cli. Parakeet's word times were grouped into Whisper-style pieces that end at a sentence end or at a pause of 1 s or more. So the pause-planned 28 s chunks, the skipped-speech retry and the music-bed re-read all stayed on.
- Each chunk started a fresh engine process, as with whisper-cli, and each engine process ran under `/usr/bin/time`.
- Runs were pinned with `taskset` to two cores with two threads, and repeated on all four cores with four threads. The desktop stayed in use (Chrome, about a third of a core).

References:
- The Moonshine trial's references no longer exist, so 76e99b52, 9337ef83 and d34b2474 lost their video-transcript references. All recordings were scored against whisper.cpp large-v3-turbo q5_0 run over each whole stream: the same pinned file (revision 5359861c, 574,041,195 bytes, SHA-256 394221709c…ffa7e2 checked), four threads.
- Check on the new references: on 76e99b52, Standard and Accurate scored 5.7% and 4.6% against turbo, and 5.8% and 4.7% against the video in the Moonshine table.
- The microphone takes of the parent-relationship video (d34b2474, 86eb3f3e, 78822941) were also scored against turbo's transcript of the clean system-audio capture of the same video (9337ef83), with free ends. Every model had 45-49 extra words at d34b2474's edges, and the other two recordings started 74 reference words in.
- Text rules as in the Moonshine trial. Sound tags were removed from the references only, so a model's own tags count as extra words.
- Three more recordings were scored: 86eb3f3e and 78822941 (the same video recorded by microphone at +10 dB boost, without and with WebRTC noise suppression), and 632f6500 (an imported student English listening test, copied as one capture file).

| Recording | Standard | Accurate | Parakeet Q8_0 | Parakeet Q6_K | Parakeet Q4_K_M |
|---|---|---|---|---|---|
| 76e99b52 counselling role-play, system audio, music bed (2 min 33 s) | 5.7% | 4.6% | 2.9% | 2.9% | 2.9% |
| 9337ef83 parent relationship role-play, system audio (4 min 13 s) | 6.3% | 4.6% | 2.5% | 2.7% | 3.6% |
| d34b2474 same video, clipped laptop microphone (4 min 45 s) | 39.3% | 30.0% | 20.9% | 21.7% | 21.1% |
| d34b2474 against the clean system-audio copy | 39.3% | 28.5% | 18.6% | 19.0% | 18.6% |
| 84cc8615 counselling podcast, noisy laptop microphone (4 min 24 s) | 21.6% | 12.1% | 8.0% | 8.8% | 8.8% |
| 7735714c listening test, system audio (2 min 2 s) | 1.2% | 0.8% | 0.8% | 0.8% | 0.8% |
| ef1e2714 listening test, system audio (4 min 31 s) | 4.7% | 5.1% | 4.9% | 4.9% | 5.1% |
| 86eb3f3e parent role-play, microphone at +10 dB (4 min 2 s), against the clean copy | 8.3% | 5.4% | 4.7% | 4.5% | 5.4% |
| 78822941 the same with WebRTC noise suppression (4 min 1 s), against the clean copy | 7.6% | 7.0% | 5.0% | 4.5% | 5.6% |
| 632f6500 student English listening test, imported (2 min 21 s) | 5.8% | 2.5% | 4.9% | 4.9% | 4.4% |
| Longest run of missing words | 7 | 3 | 4 | 4 | 4 |
| Time / audio, two cores | 0.12-0.26 | 0.29-0.63 | 0.12-0.21 | 0.11-0.19 | 0.11-0.20 |
| Time / audio, four cores | | 0.20-0.43 | 0.09-0.18 | 0.08-0.19 | 0.08-0.14 |
| Peak memory, mapped weights (default): whole / private | 0.28-0.30 GiB | 0.52-0.54 GiB | 1.15-1.19 / 0.51 GiB | 1.03-1.07 / 0.51 GiB | 0.92-0.95 / 0.51 GiB |
| Peak memory, weights copied (`--no-mmap`), all private | | | 1.19 GiB | 1.07 GiB | 0.95 GiB |
| Time under a memory cap of 80% / 60% of peak (84cc8615, cold cache) | | 1.35× / 1.40× | 1.7× / 2.3× | 1.7× / 2.4× | 1.6× / 2.3× |
| The same with weights copied | | | 3.0× / 3.5× | 2.6× / 3.4× | 2.0× / 2.9× |

How the rows were measured:
- Time is the whole pass (Python, FFmpeg and the engine) divided by the audio. d34b2474's is per microphone minute: its dual-capture system track is digital silence and produced no rows.
- Peak memory is the engine process. "Whole" is its peak RSS, which counts the model file's pages mapped from disk. "Private" is peak RssAnon on a real 28 s chunk, sampled every 5 ms. Whisper reads its model into private memory: Standard 0.27 GiB, Accurate 0.52 GiB.
- All 143 passes covered every captured millisecond. Parakeet's transcripts were identical at two and four threads, with copied weights, and under every memory cap.

Load against decode (one process, two threads, warm page cache, median of three):
- Parakeet loads in 0.22-0.23 s mapped and 0.37-0.47 s copied. On 1.1 s of silence its whole process takes 0.4 s, against 1.9 s for Standard and 5.5 s for Accurate, because whisper.cpp always encodes a full 30 s window.
- On a real 28 s chunk: Standard 7.2 s, Accurate 13.6 s, Q8_0 4.2 s, Q6_K and Q4_K_M 3.2 s. The K-quants encode faster than Q8_0 on this CPU.
- Private memory is 0.33 GiB after load and 0.51 GiB on the chunk, the same at every size. The size changes only the mapped part: 702 / 579 / 460 MiB.

Retries and re-reads:
- The skipped-speech retry fired on 39-40 of 102 Parakeet chunks (69-70 extra decodes), against 15-17 of 103 for Whisper (30-33). Parakeet returns no words for fillers, laughter and repeated starts, which Whisper often covers with a segment. The retries are included in the times above.
- The music-bed re-read fired once for Standard and twice for Accurate (76e99b52, d34b2474), and never for Parakeet. Its word times end with the words, so no piece spreads over music. It kept the "yeah" after 76e99b52's music bed, which Accurate dropped.

Losses and invented words:
- **Parakeet:** its longest losses are speakers repeating themselves ("I, I wasn't, I wasn't" written once) and a 4-word "and I mean like" on the clipped microphone. Its longest insertions are the reverse: repeats the reference left out, numbers as words ("eleven to sixteen" for "11 to 16"), "oh yeah oh" on the clipped microphone, and one "warren" on 84cc8615. It writes British spellings ("counselling"), as Accurate does, and emits no sound tags.
- **Accurate:** it invented an 11-word phrase on d34b2474 that is in neither reference, and sound tags ("car engine revving", "breathing heavily", "water splashing").
- **632f6500:** Parakeet's deficit is mostly formatting. It wrote "Eton" (most likely right) five times where turbo and Accurate wrote "Eaton", and wrote numbers as words. With spelling and number formats normalised, it scores 3.8-4.0% against Accurate's 1.6%. Elsewhere normalising put Parakeet ahead on ef1e2714 (4.1-4.3%, against 4.7% for Standard and 5.1% for Accurate) and changed no other ranking.

Memory caps:
- Each pass over 84cc8615 ran in a `systemd-run --user --scope -p MemoryMax=…` with the whole job inside, and 4 GiB of swap available. The model file was dropped from the page cache first (`posix_fadvise(DONTNEED)`, no root), so its pages were charged to the capped scope. A first attempt with the file already cached never pressed: Linux charges cached pages to the group that read them first, so the scope peaked at 0.51 GiB under every cap.
- Caps: Accurate 427 / 320 MiB, Q8_0 961 / 721, Q6_K 862 / 647, Q4_K_M 767 / 575.
- No pass failed or was killed. Accurate swapped private memory (swap peak 139 / 267 MiB). Mapped Parakeet re-read its weights from the NVMe disk instead: 2.1-3.0 million page refaults at 60%, about 8-12 GB read per pass. A slower disk would cost more.
- Even at 60%, Q6_K took 99 s (0.37 of real time), faster than Accurate with no cap (113 s). Copied weights have to go to swap, which made them 1.5-2.0 times slower than mapped weights under the same cap.

Windows build: transcribe-cli cross-compiled at the first attempt.
- Toolchain: Ubuntu 24.04's mingw-w64 11 / GCC 13 posix, the packages `build-windows-runtime.sh` uses, unpacked locally with `dpkg-deb` rather than installed. The build used that script's toolchain file, its `mingw-compat.h`, `-static -static-libgcc -static-libstdc++` and `GGML_OPENMP=OFF`.
- Result: a 7.8 MB `transcribe-cli.exe` that imports only ADVAPI32, KERNEL32 and msvcrt. Its 280 warnings are mostly `-Wformat` about `%zu` in log messages and `-Wshadow` in ggml.
- It was not run: this machine has no Windows or Wine.

Decision: replace Accurate with Parakeet Q6_K, memory-mapped, and keep Standard (Whisper base.en) as the low-memory model that setup always installs and the app falls back to. Implemented in 0.1.25 (see "Version 0.1.25").
- **Against the bar:** it beats Accurate on both laptop microphones (18.6-19.0% against 28.5% on d34b2474, 8.8% against 12.1% on 84cc8615). Its longest missing run is 3-4 words, against Whisper's 3-7. It runs at 0.11-0.19 of real time on two cores. Its private memory, 0.51 GiB, matches Accurate's 0.52 GiB; its whole peak, 1.03-1.07 GiB, does not, but the operating system can drop the mapped part under pressure, which the caps show to be survivable.
- **Q6_K over the others:** it is within 0.8 points of Q8_0 everywhere (worse on three recordings, better on two), maps 120 MiB less and encodes faster. Q4_K_M saves another 120 MiB of mapped memory but no private memory, and is 0.9-1.1 points worse than Q6_K on 9337ef83, 86eb3f3e and 78822941.
- **Still to check:** the real-engine workflow on GitHub's Windows machines, and a Windows 11 work laptop with its monitoring software.
- **To note for users:** Parakeet drops fillers and repeated words, so transcripts are tidier but less verbatim than Whisper's.

Recordings, references, transcripts, models and builds were in a temporary folder and were deleted afterwards.

## Version 0.1.24: fix from the review of 0.1.23

Tested on a 4-core cloud VM (Ubuntu 24.04, no audio devices).

Issue #10 (review of bbcc741) and the first Apps run:
- Start-up repair (`interrupted_recordings_resume_transcription_and_regain_their_length`): an `interrupted` recording whose files are all read and whose checkpoint holds no unfinished sentence, the row 0.1.23 leaves after a recovery pass, is `ready` after reopening and gets no new transcription. One with an unfinished sentence stays `interrupted` with its pass queued; one whose folder holds no audio stays `interrupted` with nothing queued. Without the repair the test fails.
- The first Apps run on bbcc741 failed in the Windows installer check: after closing the per-machine install, `native-smoke-windows.py` reported `vctip.exe` still running. That is the Visual C++ build tools' telemetry helper, left by the compile step (GitHub's job cleanup ends it as an orphan in every Windows job), not a VoxLoom process. Windows keeps an exited parent's id in its children and reuses ids, so the orphan of an exited linker looked like a child of a VoxLoom process that got the linker's old id. The check now counts a process only if it started after the app, and after its parent while that parent still runs.
- The second Apps run (cd76a1b) passed both launches, then failed in the same step: after the uninstall that keeps recordings, the data folder still listed `EBWebView`. The check had waited only for the file it planted there (`EBWebView/Default/Preferences`), while the uninstaller, which runs on in the background, was still deleting the real WebView2 profile the launch had left in that folder. It now waits for the `models`, `cache` and `EBWebView` folders themselves to go.

Validation:
- Host (`an_interrupted_recording_is_ready_once_its_transcription_completes`): a library holds an `interrupted` recording with two registered files and a queued transcription. A stand-in worker in the test's resources folder reports each file as 12,000 ms long and exits well, so the scheduler runs the whole path: start, ingest, completion. The job completes, the recording's length becomes 42,000 ms (30,000 ms offset plus the second file) and its status `ready`. A second interrupted recording whose transcription was cancelled stays `interrupted` at 0 ms. On 0.1.23 the same test fails with `interrupted 42000`: the length was restored but the status never changed.
- Recovery of interrupted recordings (`interrupted_recordings_resume_transcription_and_regain_their_length`), the stranded-recording query, start-over and retry, the capture scheduler and the queue cap keep their tests.
- Native (Xvfb, debug build, `scripts/native-smoke.py`): launch, floating recorder and back, Quit with exit code 0, all 4 observed processes released; idle single-webview peak 672,692 KiB. The sidebar shows 0.1.24.
- Fast set (with the repair): 50 Python checks, 34 Rust unit checks plus the offline setup integration check, 19 browser checks (the tour skipped without `TOUR_OUT`), TypeScript/privacy/version, and clippy for desktop and the Windows target.
- Read, not run: Windows recording (`audio/wasapi.rs`, `audio/pcm.rs`), the installer hooks and the update check against GitHub; this environment has no Windows machine, audio device or desktop session.

## Version 0.1.23: one notes page; fixes from the review of 0.1.22 (issue #8)

Tested on a 4-core cloud VM (Ubuntu 24.04, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- Worker (`tests/test_notes.py`, mocked generator): the person's own notes reach every summarising pass as a NOTES block with the guidance that they steer topics but are not evidence; the evidence block is unchanged and an empty note sends no block. `own_notes` leaves out every `# Summary` section (whatever follows it up to the next `# ` title), keeps `## ` headings inside the person's notes, and cuts at a line break before 1,500 characters.
- Host:
  - A finished summary is appended under `# Summary` below notes that are kept byte for byte; a second summary adds another section; an empty note gets no leading blank lines; an empty reply fails instead of adding a bare title. A save sent with an older `notes_base` keeps the summary that arrived after it, below the edit and on lines of its own even when the edit has no final line break, and returns the merged text; a save from the current text replaces it.
  - Migration 004 on a library from 0.1.22: a finished summary moves below the notes once (opening again changes nothing), surrounding whitespace is trimmed, an unfinished draft (`<|channel>`, `<think>`, `Thinking Process:`) stays in the unused column, and the library opens at version 4.
  - Issue #8, finding 1: Transcribe cancelled before it starts leaves the rows, the checkpoint and the index as they were; once its job starts, the unedited rows and the checkpoint are cleared and the edited row stays. With the clear moved back to `enqueue`, the test fails.
  - Issue #8, finding 2: start-up queues a resuming transcription for a finished recording with a checkpoint and an unread file or an unfinished sentence and no transcription job (the state the 0.1.22 upgrade left after removing a failed live pass); the queued pass reads the kept checkpoint (28,000 ms). One whose transcription was cancelled is not queued again; recordings fully transcribed or never started are left alone. Without the new query, the test fails.
  - Issue #8, finding 3: `docs/ARCHITECTURE.md` now describes the pass after Stop.
- Browser checks:
  - Notes typed in the studio during a recording are saved (with `notes_base`) and are what the recording opens on after Stop. Summarise saves an unsaved edit before it queues the summary. A summary that arrives while the person is still typing is added below their new lines and neither is lost. Done shows the notes rendered, with the person's lines first and the Summary title.
  - Rendering keeps typed line breaks and times as typed; recording times are cleaned only inside the summary. Copy notes gives formatted HTML and plain text starting with the person's lines; Copy Markdown gives the whole page with `# Summary`.
  - Typing into a recording with no notes keeps the field open and focused past the first save, and the page then offers Copy notes, Done and Edit.
  - The library marks a recording whose notes hold a summary, and only that one.
  - An old unfinished draft is never shown. The tour shows the notes page and the studio notes in light and dark.
- CI's Windows real-engine workflow (`real_offline_import_transcribe_index_search_restart_export_delete`) now types a note before Summarise and checks, with the real E2B model, that the note is kept exactly as typed and the summary (mentioning the budget) is added below it under `# Summary`. Its first run on 0.1.23 failed because it still read the old `summary` field; the library's Summary tag had the same leftover. Fixed in 5730445, where it passed: the whole workflow took 54.7 s, the summary 26.4 s at a host-tree peak of 2.60 GiB (2,729,356 KiB, the Job Object working set, as in the Phase 2 table) on GitHub's Windows Server 2025 machine, and the transcribe-recording step again covered 33.5 s in 2 chunks, resumed, with all 6 repetitions.
- Native (Xvfb, debug build, `scripts/test-native-notes.py`): a 0.1.22-format library with notes and a summary opens on the notes page with the summary below them; Copy notes puts HTML (`<h2>`, `<h3>`, `<strong>`) and plain text starting with the person's line on the GTK clipboard; Quit exits with code 0. The script now waits for the window instead of a fixed 5 s, which a debug build needs. `scripts/native-smoke.py` on the same build: launch, floating recorder and back, Quit, all 3 processes released.
- Fast set: 50 Python checks, 33 Rust unit checks plus the offline setup integration check, 19 browser checks plus the light and dark tour, TypeScript/privacy/version, and clippy for desktop and the Windows target.

## Version 0.1.22: recordings are transcribed after Stop

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- Worker (`Transcription` in `tests/test_worker.py`, fixture Whisper, real chunk planner and sentence assembly): every former live-path test now runs as one pass over the finished recording. Every millisecond is decoded once with cuts only in pauses; skipped trailing speech is re-read; speech after a music bed is re-read and the words heard twice are dropped; an unfinished sentence is finished by the next chunk; silence is skipped; audio without contrast is never skipped.
- New: a pass stopped after its first, second or third chunk and started again from its checkpoint gives exactly the same cuts and rows as an uninterrupted pass.
- Host:
  - While a recording is open (and while it is paused), the scheduler registers closed segments and starts nothing: no job is queued for it, and an earlier recording's queued transcription stays pending. Stop queues one transcription, which reads every track of the recording from an empty checkpoint.
  - Transcribe (`restart`) clears unedited rows and the checkpoint and keeps the user's edits; Retry keeps both, so it resumes.
  - Migration 003 turns a queued pass of the old live kind into an ordinary transcription, keeps one per recording, drops old failures, and the library opens at version 3.
  - Recovery of interrupted recordings, atomic checkpoints and the queue cap (transcriptions never turned away) keep their tests.
- `scripts/transcribe-recording.py` with a stand-in Whisper on a 70 s FFmpeg-made recording in three 30-second files: every millisecond covered; stopped after 2 chunks and resumed, the rows were identical to an uninterrupted run.
- CI's Windows real-engine job now also runs `transcribe-recording.py` with the real engine and model on `voxloom-long.wav` cut into 30-second files, stopped after one chunk and resumed. A local run with the real engine was not possible: this environment's network policy refused the whisper.cpp source download (403). On the 0.1.22 commit that job passed: 33.5 s covered in 2 chunks, stopped after the first and resumed, with all 6 repetitions of the fixture sentence in the rows.
- Browser check: while recording, the top bar says "Waits until you stop recording" with no spinner or progress, and the studio says the transcript is written when you stop (also when paused). After Stop, the recording's pass shows "Transcribing · 40%", and its page shows "Writing the transcript… 40%" with a progress bar instead of the Transcribe button.
- The opt-in real-capture test (`real_pulse_dual_capture_transcribed_after_stop_pause_resume_shutdown`) now checks that no rows or jobs appear while recording and that the fixture's words arrive after Stop. It needs PipeWire and was not run here.
- Fast set: 48 Python checks, 31 Rust unit checks plus the offline setup integration check, 17 browser checks, TypeScript/privacy/version, and clippy for desktop and the Windows target (with tests).

## Version 0.1.21: Check for updates

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- `tests/test_updates.py`, with GitHub replaced by fixed answers:
  - Only a strictly newer published version is offered, and versions compare as numbers (0.1.10 is newer than 0.1.9).
  - The one request is a GET to `api.github.com/repos/chencyberlab/VoxLoom/releases/latest` with no body, and with only `Accept` and `User-Agent: VoxLoom/<version>` headers.
  - 404, 403/429, other HTTP errors, no connection, a timeout, unreadable JSON and unexpected tags each give a plain message.
- Live, from this VM: `updates.py` reported 0.1.20 as newer for 0.1.19 and as up to date for 0.1.20 and 0.2.0. The `check_updates` action through the real service returned `{"current":"0.1.20","latest":"0.1.20","newer":false}`.
- `release_page` accepts only `N.N.N`. Empty, two-part, four-part, prefixed, non-numeric, path and argument-like versions are refused, so only this repository's release page can reach the browser.
- `privacy-check.py` fails when `updates.py` names any other address (checked by changing it to another URL) and passes with the real one.
- Browser check:
  - Settings makes no update request on load.
  - Check for updates shows "VoxLoom 0.9.0 is available", and Get VoxLoom 0.9.0 asks the host to open that release only.
  - The next check says up to date and hides the button; an offline answer shows its message and the button can be pressed again.
- Settings checked in light and dark with the visual tour. `worker/updates.py` is in the package resources, and the Windows installer check now expects it in the install folder.
- Fast set: 48 Python checks, 30 Rust unit checks plus the offline setup integration check, 17 browser checks, TypeScript/privacy/version, and clippy for desktop and the Windows target (with tests).

## Version 0.1.20: transcription choice in setup, upgrades keep models

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- The offline setup integration test (real `provision.py`, tiny SHA-256-checked files) now also covers transcription:
  - Accurate with or without summaries installs Standard and Accurate and saves Accurate.
  - Standard saves Standard over a saved Accurate.
  - No choice keeps the saved one.
  - Accurate missing from the offline folder fails, keeps the saved choice and installs no summary model.
  - An unknown choice installs nothing.
- On the 0.1.19 backend the new test fails at the first Accurate case: the Accurate model is not installed.
- Browser checks:
  - First-run setup shows Standard selected, Accurate adds 264 MB to the total (3.6 → 3.9 GB) and to the Transcription row (412 MB), and the request carries `transcription_profile`.
  - The Download button stays fully visible at 900 × 720 with the longer Accurate text.
  - Model setup opened from Settings shows the saved Accurate choice as Ready.
- The setup window was checked in light and dark with the visual tour.
- `scripts/installer-smoke-windows.py` adds an upgrade before the first uninstall in each mode: the old uninstaller run in place (`/S /DELETEDATA _?=<folder>`, as a newer installer runs it), then a fresh silent install. It fails if any model, cache, browser data, library, recording, shortcut, remembered location or microphone record is gone.
  - Result (Apps run on ee699e4): the upgrade kept everything in both modes, and all four uninstall cases were clean.
- Fast set: 45 Python checks, 29 Rust unit checks plus the offline setup integration check, 16 browser checks, TypeScript/privacy/version, and clippy for desktop and the Windows target (with tests).

## Version 0.1.19: Windows installer (Windows phase 4)

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- `scripts/installer-smoke-windows.py`, run by the Apps workflow on the setup it has just built. For each install mode (`/CurrentUser` into `%LOCALAPPDATA%\Programs\VoxLoom`, `/AllUsers` into `C:\Program Files\VoxLoom`) it installs twice, silently:
  - It checks the program, uninstaller, engines, FFmpeg, Python, worker, model manifest and licences are in place, with a Start menu shortcut and an Apps entry.
  - It launches the installed app and closes it (`native-smoke-windows.py`), and runs every bundled program from the install folder (`windows-runtime-smoke.py`), which also leaves Python caches there.
  - It puts a library, a recording, a model, a cache file, browser data and Windows' microphone record in place, then uninstalls: once keeping recordings, once with `/DELETEDATA`.
  - It fails if the program folder, a shortcut, the Apps entry, the remembered install location, the installer language, the microphone record, the models, the cache, the browser data or anything in `%APPDATA%\org.voxloom.desktop` is left. Kept: only `library.db` and `recordings`; with `/DELETEDATA`, nothing.
  - Result (Apps run on 51d8b7f): all four cases clean. The installer is 41.5 MiB; the installed app launched and closed with nothing left running in each case, and every bundled program started from both install folders.
- `scripts/native-smoke.py` (Linux launch check) waited fixed times between key presses and clicks; on a cold GitHub machine the first Apps build of 0.1.19 clicked before the page had loaded and failed. It now waits for each window change and tries a lost click again. Three local runs passed, and so did a run with the first float and Quit clicks deliberately left out.
- The uninstall hooks compiled with warnings as errors in a stub uninstaller (`makensis -WX`) before the first Windows build.
- `package-windows-preview.py` now takes the bundled files from the Windows Tauri settings, as the installer does; with stand-in files it assembles the same folder as before.
- Fast set: 45 Python checks, 29 Rust unit checks plus the offline setup integration check, 14 browser checks, TypeScript/privacy/version, and clippy for desktop and the Windows target (with tests).

## Version 0.1.18: fix from the review of 0.1.17 (issue #7)

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- `saved_tool_paths_follow_the_running_copy` was rewritten around the preview's layout: the running copy has its own `voxloom` executable beside `engines/`.
- Each case checks what Settings shows and what the transcribe request runs, then saves every setting as shown (as both Save buttons do) and checks again. The cases:
  - Nothing saved uses the default and stores nothing.
  - The running copy's other engine build, and a tool placed inside it, are used and survive the save.
  - An engine of another copy, on disk or deleted, is not used.
  - A custom path that is missing is not used. It survives saves and is used once the file exists.
  - Choosing the default again stores nothing.
- On the 0.1.17 code, the new test fails: choosing the running copy's generic build left Settings and the request on the default `whisper-cli`.
- Fast set: 45 Python checks, 29 Rust unit checks plus the offline setup integration check, 14 browser checks, TypeScript/privacy/version, and clippy for desktop and the Windows target (with tests).

## Version 0.1.17: Windows setup works after moving to a newer preview

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines.

Validation:
- Regression test `saved_tool_paths_follow_the_running_copy`:
  - A saved engine path from an older copy that is still on disk is not used: Settings shows the running copy's engine.
  - Nor is a path from a copy that was deleted.
  - A custom path the user chose is kept.
  - Saving the default stores nothing.
  - With the fix disabled, the test fails at the first stale path.
- Fast set: 45 Python checks, 29 Rust unit checks plus the offline setup integration check, 14 browser checks, TypeScript/privacy/version, and clippy for desktop and the Windows target.

## Version 0.1.16: recording on Windows (Windows phase 3)

Tested on a 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices) and on GitHub's Windows Server 2025 machines (bundled FFmpeg 7.1.5, no audio devices).

Validation:
- Fast set here: 45 Python checks, 28 Rust unit checks plus the offline setup integration check, 14 browser checks, TypeScript/privacy/version, desktop clippy, and clippy for the Windows target (the WASAPI backend and its tests) with warnings denied.
- Timeline (`audio/pcm.rs`): packets jittered by up to ±4 ms join without a gap; a source that hears nothing until 1.0 s is silent until then and its sound lands at exactly 1.0 s; a device that comes back after 500 ms starts where the clock says; audio that arrives after its place was written out is kept, right after it, not dropped.
- FFmpeg graph, run for real: 3.5 s of two-channel PCM on stdin (a tone on the microphone, silence then a tone on the system channel) becomes one `-mic-` and one `-system-` segment of 56,000 samples each (±64), and both sources print level lines. Passed with FFmpeg 6.1.1 here and with the bundled FFmpeg 7.1.5 on Windows CI.
- Windows CI: device listing works without audio hardware; a device that no longer exists is refused with a plain message before FFmpeg starts, leaving no files and no registered capture; 33 Rust checks in all; the runtime smoke check; the UI check in Edge; launch and close with every process released.
- UI: a start refused by Windows' microphone switch shows the message with "Open microphone settings", which calls `open_microphone_settings`; a device notice during recording shows in the studio and goes when the backend stops reporting it.
- Linux launch test (`native-smoke.py`, Xvfb) on the debug build: three of four runs passed (launch, compact/expand, Quit with exit 0, every observed process released). In the first run, straight after the build, the test's Quit click came before the window had restored and the app was still open after 10 s; no capture was involved.

Not yet tested: real microphones, loopback of real playback, headset switching, the privacy switch and long recordings on Windows. They need a Windows 11 laptop.

## Version 0.1.15: fix from the review of 0.1.14 (issue #6)

Tested on the same 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices, no models, no LiteRT).

Validation:
- Fast set: 45 Python checks (one extended), 21 Rust unit checks plus the offline setup integration check, 13 browser checks, TypeScript/privacy/version, desktop clippy with warnings denied, and clippy for `x86_64-pc-windows-gnu` with warnings denied.
- Worker (`test_segment_cut_before_its_first_frame_counts_as_empty`, extended): with the 2-second segment's length cached (as the host sends it after the recovery pass) and no length for the 200-byte stub or the empty file, `track_lengths()` returns `[2000, 0, 0]` and `windows()` over those yields one audio window for the real file and none for the others. The expression `index()` used in 0.1.14 on the same list raises `Saved audio could not be decoded`.

Native smoke (`npx tauri build --debug --no-bundle`, then `xvfb-run -a python3 scripts/native-smoke.py src-tauri/target/debug/voxloom`): launch, shrink to 390×92, expand, Quit (exit 0; three observed processes released; peak sampled RSS 670,116 KiB for the unoptimised debug build). The sidebar shows 0.1.15. No `.deb` was built on this machine; the Apps workflow builds it from main.

## Version 0.1.14: fixes from the review of 0.1.13 (issue #5)

Tested on the same 4-core cloud VM (Ubuntu 24.04, FFmpeg 6.1.1, no audio devices, no models).

How FFmpeg treats damaged capture segments, measured here with a 30-second pink-noise segment (1,225,490 bytes) and a 2-second tone segment: a 200-byte or 6,000-byte prefix (no complete frame) cannot be opened at all (`Error opening input: End of file`, exit 187, no samples); a 15,000-byte prefix decodes 144 ms and a 100,000-byte prefix 2,232 ms, both with exit 0 and a logged `decode_frame() failed` for the half frame at the end; 400 bytes overwritten in the middle lose 216 ms and exit 0. So a segment that `duration()` rejects holds no audio FFmpeg can read, which is what the worker now relies on. Audio export (`ffconcat`) still succeeds when such a file is last in the list (2.0 s out of a 2 s segment plus a stub) and fails only when it comes first, which a crash cannot produce.

Validation:
- Fast set: 45 Python checks (2 new), 21 Rust unit checks (2 new) plus the offline setup integration check, 13 browser checks, TypeScript/privacy/version, desktop clippy with warnings denied, and clippy for `x86_64-pc-windows-gnu` with warnings denied.
- Worker (`test_segment_cut_before_its_first_frame_counts_as_empty`): one 2-second segment, a 200-byte prefix named `s0000-mic-000001.flac` and an empty `s0001-mic-000000.flac`: `audio_streams` returns the first segment's stream (2,000 ms), emits one `track_duration` (for the real file only) and no error; a 200-byte prefix with an import's name still raises. Against the 0.1.13 worker the same test fails with `Saved audio could not be decoded`.
- Worker, end to end (`test_recovery_pass_transcribes_the_segments_before_a_crash_stub`): the same files through `transcribe()` with a fake Whisper: the pass commits `covered_ms` 2,000, one row, and one `track_duration`. Fails on the 0.1.13 worker with the same error.
- Host (`a_full_disk_refuses_recording_but_an_unknown_one_does_not`, `free_space_is_unknown_for_a_missing_volume_and_measured_for_a_real_one`): `Some(0)` and `Some(256 MiB − 1)` refuse, `Some(256 MiB)` and `None` do not; a path on no volume reports `None`, the temp folder reports a figure.

Native smoke (`npx tauri build --debug --no-bundle`, then `xvfb-run -a python3 scripts/native-smoke.py src-tauri/target/debug/voxloom`): launch, shrink to 390×92, expand, Quit (exit 0; four observed processes released; peak sampled RSS 662,704 KiB for the unoptimised debug build). The sidebar shows 0.1.14. No `.deb` was built on this machine; the Apps workflow builds it from main.

## Version 0.1.13: code review fixes

Tested on a 4-core cloud VM (Ubuntu 24.04, 15 GiB, no audio devices, no models), so the real-engine and real-capture checks were not run; each fix has a unit test on the host or the worker.

Validation:
- Fast set: 42 Python checks (4 new), 19 Rust unit checks (5 new) plus the offline setup integration check, 13 browser checks, TypeScript/privacy/version, desktop clippy with warnings denied, and clippy for `x86_64-pc-windows-gnu` (mingw-w64) with warnings denied.
- Live stream gap (`live_request_registers_segments_closed_since_the_last_tick`): with the scheduler stopped, two segment files on disk and a capture that has registered nothing, one `transcribe_live` request registers the closed file and lists it in `context_tracks`; a second request does not register it twice.
- Pause (`pause_and_stop_both_queue_the_final_live_pass`): Pause registers the open segment and queues one `transcribe_live` job, whose request finalises with no growing file; Stop afterwards leaves exactly one job (the duplicate is refused).
- Recovery (`interrupted_recordings_resume_transcription_and_regain_their_length`): a library closed with a `recording` meeting and two files on disk reopens with both registered (offsets 0 and 30,000 ms) and one live pass queued; a meeting whose files are all transcribed but whose checkpoint carries an unfinished sentence is queued too; one with nothing owed, and a `ready` meeting, are not. Two `track_duration` events raise the interrupted meeting's length from 0 to 42,000 ms and leave a `ready` meeting's length alone.
- Queue cap (`full_queue_never_turns_away_a_live_pass`): with eight summaries pending, a ninth summary and an index are refused and a live pass is accepted; its duplicate is still refused.
- Search (`exact_search_filters_before_the_row_cap`): 600 short matching rows in one recording and one longer matching row in another. Filtering on the second recording returned 0 rows before the fix and 1 after; the unfiltered search still returns 80.
- Worker (`test_pinned_models_are_hashed_once_until_they_change`): a manifest-named file is hashed on the first call only; a rewrite of the same size with a later timestamp is hashed again and fails; the corrected file is hashed once more and then trusted; a damaged cache file is ignored; with no cache location every call hashes; files the manifest does not name are never hashed.
- Summaries (`test_odd_leftover_note_waits_for_a_partner_instead_of_a_solo_pass`): three batches now take five model passes instead of six, and the third note reaches the final combine unchanged.
- Setup (`test_damaged_partial_download_is_removed_so_retry_can_work`, `test_mismatched_download_is_removed_instead_of_left_for_the_user`): an HTTP 416 on a wrong full-length `.part` removes it and the next attempt downloads; a correct `.part` is promoted without another byte; a download whose hash fails is removed and the retry succeeds.

Measured hashing cost on this VM (SHA-256 in Python, 1 MiB reads): a 2.85 GB file, the size of the E2B summary model, takes 2.9 s from disk and 2.3 s from the page cache. Every `generate` call verified the model, so a summary with 19 passes spent 45-55 s hashing before the change; on a laptop SATA SSD (about 500 MB/s) each pass would be nearer 6 s. The whisper model (148 MB) cost 0.10 s per live pass.

Native smoke (`npx tauri build --debug --no-bundle`, then `xvfb-run -a python3 scripts/native-smoke.py src-tauri/target/debug/voxloom`; the VM has no engines or models, which the smoke test does not use): launch, shrink to 390×92, expand, Quit (exit 0; three observed processes released; peak sampled RSS 664,808 KiB for the unoptimised debug build without GPU acceleration). The sidebar shows the new version. No `.deb` was built on this machine; the Apps workflow builds it from main.

## Version 0.1.12: Markdown copy, Windows preview, apps built by GitHub

Validation:
- Fast set on this Ubuntu computer: TypeScript/privacy/version (0.1.12 everywhere), 39 Python checks, 13 browser checks (Chromium) including both copy buttons, 14 Rust checks plus offline setup integration, desktop clippy, and clippy for the Windows target. The browser checks also pass on Node 24.22.0.
- CI on GitHub's Windows and Linux machines: the fast set on both, and on Windows the runtime, Job Object, launch and real-engine checks recorded under "Windows port, Phase 2" below.
- Packages: built and checked by the Apps workflow on main. Linux: `build-app.sh`, then launch, compact/expand and Quit in Xvfb. Windows: the release app with every bundled file and the runtime, zipped, unzipped into a folder with spaces and accents, the runtime check, then launch and close with nothing left running. The draft release is made only when both pass.

## Windows port, Phase 2: bundled runtime on GitHub's Windows machines (2026-10-19)

CI run 39196514916 on commit 7dfdf22, image `windows-2025` (Windows Server 2025 Datacenter 10.0.26100, GitHub's standard 4-vCPU, 16 GB machine, no audio devices). `scripts/build-windows-runtime.sh` cross-built the runtime on Ubuntu 24.04 and the Windows jobs received it as an artifact. Apart from CI's own build tools, nothing was installed on the Windows machine: no Python, FFmpeg or Visual C++ runtime for VoxLoom.

The real-engine workflow (`cargo test --test workflow real_offline_import -- --ignored`) ran with the bundled Python and LiteRT, `whisper-cli.exe` and `llama-completion.exe`. The pinned models were opened through `%RUNNER_TEMP%\modèles Zoë`, a path with a space and non-ASCII letters. It did the following, and passed in 42.6 s:

- ran setup against the cached, hash-checked models, with no download;
- imported `voxloom-budget.wav`, transcribed it, and indexed it as text and audio vectors;
- re-ran indexing without duplicating vectors;
- found a native-audio hit and an exact hit;
- made the requested E2B summary and a grounded answer;
- exported SRT, TXT, Markdown, JSON and both Word files;
- reopened the library and shut it down while a model was loaded (no worker or engine was still running five seconds later);
- deleted the recording with all derived data.

| Stage | Time | Peak memory, app and everything it started |
|---|---|---|
| Transcribe (base.en) | 1.6 s | 345 MiB |
| Index (EmbeddingGemma 740M) | 2.3 s | 683 MiB |
| Summary (Gemma 4 E2B) | 17.1 s | 2.6 GiB |

Memory is the sum over the app's Job Object, and that sum includes the test process. On Windows the worker reports no peak of its own, because Python's `resource` module doesn't exist there. These are times for a short synthetic sentence on a server CPU, not laptop throughput; Phase 5 measures a real Windows 11 laptop.

The other Windows checks, on every push:

- Python unit tests, with the bundled FFmpeg on PATH.
- Playwright UI tests in Edge (13 passed).
- Rust unit tests, which include three Job Object tests:
  - a killed worker's grandchildren end with it;
  - a child that exits on its own leaves nothing behind;
  - a force-killed stand-in app leaves nothing running, including a child it did not start through `process::spawn`.
- Clippy and a debug build.
- `native-smoke-windows.py`: the window opens and closes with exit code 0, and WebView2 is released.
- `windows-runtime-smoke.py`:
  - every bundled program starts;
  - FFmpeg writes 1-second capture-style segments from raw PCM in a folder named `Zoë's café recordings`, with level metering;
  - the segments decode to exactly the 48,000 input samples, cut on FLAC frames (16,128 / 16,128 / 15,744 samples);
  - the bundled Python loads LiteRT and runs the worker and Word export with a non-ASCII file name.

The first run of the real-engine job found the one Windows-only bug in Phase 2: the embeddable Python's `._pth` file leaves the script's folder off `sys.path`, so the worker could not import `notes.py` and stopped at once. A `-P` unit test now covers it on every platform.

## Version 0.1.2: continuous audio and finished summaries

The user reported complete audio but missing transcript words, plus an E2B summary containing an unfinished thought channel. Regression work reproduced three app/runtime issues: independently decoding storage chunks, time-based rejection of valid recognized text, and seeking into segmented FLAC that skipped requested audio. Some FLAC segments also inherited the full recording duration in their header. Transcription now joins each source/session logically, counts/caches actual decoded samples, decodes segmented files before seeking, waits for natural pauses, and retains whole recognized segments. Nonstop speech uses bounded 120-second passes with six seconds of audio overlap and conservative text deduplication. Capture runs independently, and Stop/Pause flushes pending speech. Quiet audio is retained; only digital quiet is trimmed from inference copies.

Real engine regression, using already user-provisioned weights read-only and synthetic speech only:

```bash
python3 scripts/test-transcript-continuity.py --models /path/to/existing/models --summaries
```

The 67,574 ms fixture places sentences across the physical 30- and 60-second file boundaries. Decoded coverage exactly matched its original duration; the complete committee/budget/invoice/Friday and emergency/inspection/Tuesday/safety-report sentences survived. A separate real FFmpeg regression compares PCM bytes across segmented-file boundaries and verifies exact sample preservation. The earlier failing fixture omitted opening words and misreported the duration as 127,478 ms, exposing the seek/header defects. These fixtures include silence and are not continuous-speech throughput benchmarks.

The llama.cpp completion frontend accepted but failed to forward its reasoning setting. The documented source patch now applies `--reasoning off` to its native chat template; both CPU variants are rebuilt. Responses require a completed result wrapper or native end-of-generation marker, never an unfinished thought channel. The short E2B fixture returned normal cited notes. The 1B fixture completed without a reasoning leak but omitted reliable citations and correctly used the explicitly labeled evidence-extract fallback; this does not demonstrate high-quality 1B summarization. No new model weights were downloaded for these checks. E2B business quality and full-native peak memory remain unverified.

Validation for this update: 16 Python checks, six Rust unit checks plus the offline setup integration check, six browser checks (including hiding an old unfinished summary), desktop clippy with warnings denied, the real ASR/index/search/summary/restart/export/delete/shutdown workflow (22.18 s), and synthetic dual capture with live text, pause/resume, final-tail processing and shutdown (55.86 s). Cursor advancement and transcript rows commit atomically; a deliberately invalid batch rolls back both, retries do not duplicate rows, edited text survives, progress resumes after reopening SQLite, and meeting deletion cascades through the new checkpoint table. The database migrates to schema version 2. Tests use temporary libraries and never reset the installed user's data. The final extracted 0.1.2 package also passed native launch, compact/expand and Quit (exit 0; four observed processes released; sampled idle tree 434,888 KiB). Packaged workers, manifest, source notices and both patched completion binaries matched the source/staging hashes; no model weights are bundled.

## Microphone gain and noise suppression (measured after 0.1.11)

Setup:
- The development laptop's internal microphone (Realtek ALC274) was at the maximum ALSA gain: Capture +30 dB and Internal Mic Boost +30 dB.
- The parent-relationship role-play (the source of 9337ef83 and d34b2474) was played from a phone about 1 m away and recorded mic-only with Accurate live transcription, once with the boost lowered to +10 dB and once through a PipeWire WebRTC source (`libpipewire-module-echo-cancel`: high-pass filter, noise suppression, echo cancellation, gain control off).
- d34b2474 was recorded on 15 October at maximum gain; its playback setup was not noted.
- Scoring: each transcript was aligned against the part of the video's transcript it covers (reference words 8-530 for the new takes), ignoring case, punctuation and fillers. d34b2474's replay figures come from the Moonshine evaluation above, which used the same text rules.
- Contrast is the 90th minus the 10th percentile of the 100 ms speech-band (300-3400 Hz) levels.

| Take | Mic | Overall / below 250 Hz / 300-3400 Hz | Full-scale samples | Contrast | Accurate live | Accurate replay | Standard replay |
|---|---|---|---|---|---|---|---|
| d34b2474 | boost +30 dB | −5.2 / −8.4 / −10.3 dBFS | 330,869 | 19.5 dB | 32.6% | 32.4% | 44.1% |
| 86eb3f3e | boost +10 dB | −23.0 / −24.1 / −30.4 dBFS | 4,572 | 29.6 dB | 7.3% | 7.7% | 10.5% |
| 78822941 | boost +10 dB, WebRTC processing | −31.3 / −41.6 / −32.4 dBFS | 2 | 43.6 dB | 9.7% | 7.1% | 9.3% |

Findings:
- **Gain:** lowering the boost by 20 dB removed the clipping and cut Accurate's WER from 32.6% to 7.3%, close to the clean system-audio capture of the same video (4.6%, 9337ef83).
- **Noise suppression:** it removed 17 dB of rumble and made pauses 14 dB quieter, but did not change WER reliably. It was better in both replays and worse live. The live difference is mostly one short reply ("Yeah. I guess so.") that Whisper dropped at a chunk start and kept in the replay of the same audio. Rows continue across that point, so no audio was skipped as idle; it is the chunk-boundary variance noted under Known gotchas.
- **Echo cancellation:** it removes whatever the laptop itself plays, so voices on the laptop speakers disappear from a Microphone-only recording.
- **Sound tags:** Whisper's tags ("soft music", "keyboard clicking") count as extra words in these scores.

Every replay covered all captured audio. Recordings and transcripts were copied to a temporary folder and deleted afterwards.

## Version 0.1.11: floating recorder drags and has rounded corners

Validation:
- Fast set: 38 Python checks, 12 Rust checks plus offline setup integration, 13 browser checks (the compact recorder's Expand and Quit buttons), TypeScript/privacy/version and desktop clippy pass.
- Native smoke (`scripts/native-smoke.py`, Xvfb 1280×900, depth 24 with a 32-bit visual) on the debug build and on `release/VoxLoom-0.1.11/usr/bin/voxloom`: launch, shrink to 390×92, expand, Quit (exit 0; four observed processes released; idle sampled RSS 457,136 KiB).
- Corners: in the compact screenshot, pixels outside the 12 px radius read (0,0,0) in both runs, which is how an alpha-0 window shows on a display without a compositor. The old opaque page background would read about (19,20,18), the bar body reads (25,26,23) (`--sidebar`), and the first-run screen is dark as before, with no flash.
- Dragging needs a window manager and is not covered by Xvfb.

## Moonshine Streaming as a transcription model (evaluated after 0.1.10, not adopted)

Question: should Moonshine Streaming Small or Medium (Moonshine AI, MIT, English only) be offered next to Standard and Accurate? Published Open ASR Leaderboard averages: Small 7.84%, Medium 6.65%, against 10.32% for base.en and 7.44% for large-v3. Tested with the `moonshine-voice` 0.1.5 Python package (ONNX Runtime) and its int8 English models from download.moonshine.ai (`quantized_26_08_21`): Small, 123M parameters, 142,300,462 bytes in 7 files; Medium, 245M parameters, 269,141,110 bytes. Its catalog publishes CRC32C checksums only, no SHA-256.

Method: `scripts/replay-live-asr.py` unchanged, with `whisper_pieces` swapped for Moonshine's batch call, so all four models decode the same pause-planned chunks of at most 28 s. Moonshine settings come from its own evaluation script: VAD off (`vad_threshold` 0, `vad_max_segment_duration` 100000), so no audio is skipped, and `max_tokens_per_second` 6.5. Moonshine returns one untimed line per chunk. The stretched-segment re-read was therefore off, as for Gemma 4, and the skipped-speech retry could not fire. The model loaded once per live pass, as a fresh worker would. Every run was pinned to the same two cores (`taskset`); Whisper used two threads.

References: three recordings were scored against the videos' own transcripts: 76e99b52 (first 274 words), and 9337ef83 and d34b2474 (from "So Joe", 582 and 624 words). The other three were scored against whisper.cpp large-v3-turbo q5_0 run over each whole file (ggerganov/whisper.cpp revision 5359861c, 574,041,195 bytes, SHA-256 39422170…ffa7e2). Against the video transcripts, turbo itself scored 3.6%, 4.1% and 15.1%. On those three recordings, turbo-based and transcript-based WER for the four models agreed within 0.6 points on system audio and 3.7 points on the clipped microphone, with the same ranking. Scoring: word alignment, case and punctuation ignored, and fillers (um, uh, hmm…) removed on both sides. The 76e99b52 figures are therefore 0.4 points lower than in the 0.1.7 table, which counted fillers.

| Recording | Reference | Standard | Accurate | Moonshine Small | Moonshine Medium |
|---|---|---|---|---|---|
| 76e99b52 counselling role-play, system audio, music bed (2 min 33 s) | video | 5.8% | 4.7% | 6.9% | 7.3% |
| 9337ef83 parent relationship role-play, system audio (4 min 13 s) | video | 5.8% | 4.6% | 6.7% | 5.8% |
| d34b2474 same video, clipped laptop microphone (4 min 45 s) | video | 44.1% | 32.4% | 62.0% | 53.8% |
| 84cc8615 counselling podcast, noisy laptop microphone (4 min 24 s) | turbo | 20.9% | 11.5% | 32.6% | 19.1% |
| 7735714c listening test, system audio (2 min 2 s) | turbo | 1.2% | 1.2% | 2.0% | 0.8% |
| ef1e2714 listening test, system audio (4 min 31 s) | turbo | 4.1% | 3.9% | 2.6% | 3.6% |
| Longest run of missing words | | 5 | 3 | 20 | 11 |
| Time / audio, two cores | | 0.16-0.28 | 0.45-0.62 | 0.26-0.38 | 0.38-0.54 |
| Peak memory | | 0.28-0.31 GB | 0.53-0.55 GB | 0.58-0.68 GB | 0.99-1.03 GB |

Peak memory is the `whisper-cli` process for Whisper, and the whole worker process for Moonshine (Python started at about 15 MB). All 24 runs covered every captured millisecond, so the losses below come from the models, not the chunking:

- **Small:** wrote the opening 20 s of d34b2474 (speech under music and microphone rumble, about 20 words) as "♪♪♪…??", and the 6 s opening sentence of ef1e2714 as "1.". Its 2.6% on ef1e2714 hides that loss.
- **Medium:** lost runs of up to 11 words on d34b2474. It also prefixed "150,000" to ef1e2714's opening sentence and wrote "So I was going to fucking laugh" in a passage of d34b2474 where nothing like it is said.
- **Both:** lost "Yeah, I guess" after the music bed in 76e99b52. Whisper recovers "I guess" through the re-read. Moonshine output is punctuated and capitalised, but Small once returned a lower-case run-on paragraph.

The d34b2474 microphone was recorded at the maximum ALSA gain (Capture +30 dB, Internal Mic Boost +30 dB). Its signal averages −5 dBFS, with 20,000-40,000 clipped samples per 30 s file, mostly below 300 Hz. Its dual-capture system track was digital silence and was left out.

Moonshine's published speed advantage (43.7× over large-v3 on an Apple M3) compares against Whisper re-decoding a 30 s window for every streaming update. VoxLoom decodes each chunk once.

Decision: keep Whisper.
- Medium costs Accurate's CPU and twice its memory. It is less accurate than Accurate on four of the six recordings and ahead by at most two words on the two listening tests.
- Small is slower than Standard and worse than it on five of the six.
- Both lose speech silently on difficult audio and invent words.

On microphone audio, Accurate removes about half of Standard's errors (20.9% → 11.5%). Recordings and transcripts were copied to a temporary folder; they, the models and the test environment were deleted afterwards.

## Version 0.1.10: queued-job message

The summary-style browser check now clicks Summarise and expects "Summary queued. Its progress shows at the top; click it to cancel." 38 Python checks, 12 Rust checks, offline setup integration, 13 browser checks, TypeScript/privacy/version and desktop clippy pass.

## Version 0.1.9: appearance, readability, single version file

Contrast of every text/background pairing in both themes computed from the tokens: all text at least 4.5:1 (lowest: light muted text on a hovered row, 4.56); form-control borders 3.05-3.54:1 (light) and 3.31-3.72:1 (dark) against their backgrounds. Visual tour of every page in both themes at 1280×860 and 900×720 (`TOUR_OUT=… npx playwright test tour`): sidebar, Settings and Quit stay visible at 900×720; first-run setup keeps "Download & set up" fully visible at 900×720 for E2B, E4B and no summaries. Validation: 38 Python checks; 12 Rust checks including appearance settings; offline setup integration; 13 browser checks, including System → Light → Dark cycling, saving to settings, the right theme after reload, and the Settings switch. `scripts/version.py set` changed exactly the six version fields on a copy; a hand-edited copy is reported by `npm run check`. TypeScript/privacy and desktop clippy pass.

## Version 0.1.8: Gemma 4 E2B as the default summary model

Gemma 3 1B removed after it invented three "agreed next steps" on the counselling recording (see Version 0.1.7). Validation: 38 Python checks (manifest: E2B standard, E4B manual-only, no Gemma 3 download, a `retired` entry for the 1B file); 12 Rust checks including a saved `1b` choice resolving to E2B, `1b` rejected when saving, and a retired file listed while on disk and removable; offline setup integration including a rejected `1b` setup request and E2B as the saved default; 12 browser checks including E2B/E4B in first-run setup (3.6 / 5.4 GB) and removing a retired 1B file. TypeScript/privacy and desktop clippy pass.

## Gemma 4 as a transcription model (evaluated after 0.1.7, not adopted)

Question: could Gemma 4 replace Whisper, as in Google AI Edge Foresight (Gemma 4 + EmbeddingGemma 2, no separate speech model)? Google documents Gemma 4 E2B/E4B speech recognition with at most 30 s of audio per request (25 tokens a second). Tested with LiteRT-LM 0.18.0 (already VoxLoom's search runtime) and the pinned `litert-community` files: E2B `gemma-4-E2B-it.litertlm` (revision b3ca0d2f, 2,588,147,712 bytes, SHA-256 18193810…9a63c) and E4B `gemma-4-E4B-it.litertlm` (revision 2eee7ac3, 3,659,530,240 bytes, SHA-256 0b2a8980…d45e2). Google's ASR prompt, greedy decoding, two threads, the counselling recording replayed live exactly as captured (Whisper's stretched-segment re-read disabled, since Gemma returns one untimed block per chunk):

| Model | WER | Missing words | Time / audio | Peak memory |
|---|---|---|---|---|
| Whisper Standard (0.1.7) | 6.2% | 6 | 0.19 | about 0.3 GB |
| Whisper Accurate (0.1.7) | 5.1% | 6 | 0.46 | about 0.55 GB |
| Gemma 4 E2B | 8.7% (7.6% in fixed 28 s clips) | 4 (3) | 0.36 | 1.6 GB |
| Gemma 4 E4B | 7.6% (7.3%) | 5 (6) | 0.77 | 3.3 GB |
| Foresight on the user's 64 GB Mac (same video) | 5.3% from where its transcript begins | 2 | — | — |

Gemma kept stammers and fillers ("I I guess", "who who", "um") and heard the "Yeah" after the music, but misheard more ("kids" → "gets", "counseling" → "listening"; E4B once "sounds like you" → "sam… saikyu"). With the instructions placed after the audio (Google's example order), E2B appended "and to transcribe the following speech in English into English text." to a chunk ending mid-sentence; instructions before the audio or as a system message fixed that. On the noisy microphone recording E2B ran at 0.51 of real time and differed from Whisper Accurate in 98 spans, mostly fillers and repeats, with misheard phrases on both sides. llama.cpp v0.6.0 does support Gemma 4 audio (`llama-mtmd-cli` with `mmproj-gemma-4-E2B-it-Q8_0.gguf`, 557,368,064 bytes), but it cannot turn thinking off: 75-87 s per 28 s clip and 4.75 GB. Decision: keep Whisper. Summaries through LiteRT-LM E2B took 19.7 s against 33 s through llama.cpp on the same transcript, with comparable notes: a possible later speed-up, not adopted.

## Version 0.1.7: counselling summaries, titles, speech after music

**Speech after a music bed.** Recording 76e99b52 (the counselling role-play captured from system audio, 2 min 33 s) replayed live exactly as captured (`scripts/replay-live-asr.py`, two threads), scored against the video's own transcript (first 277 words). Both models lost "Yeah, I guess I have to" at about 24 s. Trace: no pause in the music, so the chunk was cut at a fixed point (29.0 s), and Whisper's last segment ("skills of attending behavior, paraphrasing and summarizing.", 60 characters) spanned 17.6 s of music and speech (3.4 characters a second).

| Run | Standard WER / missing | Accurate WER / missing |
|---|---|---|
| Before (0.1.6) | 5.8% / 8 | 5.8% / 9 |
| A: drop the stretched segment, re-read from its start (rejected) | 9.5% / 16 | 4.0% / 5 |
| B: keep it, re-read from where its words end (shipped) | 6.2% / 6 | 5.1% / 6 |
| whisper.cpp on the whole file at once, no VoxLoom chunking | 5.5% / 5 | 6.2% / 14 |

With B both models recover "I guess I have to figure out things for myself"; the first word after the music ("Yeah") is still missed. Remaining errors are the model's: "one year already" → "when you're ready" (Standard), "came" → "come", "friends" → "friend", and stammers or fillers ("I'm, I'm", "Hmm") that Whisper leaves out by design. Live chunking is as accurate as whisper.cpp's own whole-file decoding on this recording.

Regression: recordings 84cc8615 (4 min 24 s noisy microphone), ef1e2714 (4 min 31 s system audio) and 7735714c (2 min 2 s system audio) replayed with Standard before and after: word-for-word identical transcripts, same Whisper time. The real continuity check (`scripts/test-transcript-continuity.py`) passes. Recordings were copied to a temporary folder and deleted afterwards.

**Summary styles and titles**, real engines on the counselling recording (copied, then deleted), two threads:

| Model | Title | Counselling notes |
|---|---|---|
| Gemma 3 1B (16 s) | Initial Assessment & Support | All five sections; three "agreed next steps" that were not said |
| Gemma 4 E2B (33 s) | Student Challenges and Responsibilities | Main concerns, feelings, strengths; correctly no next steps (empty section now removed) |

Titles on a second real recording: "Initial Counselling Sessions – A Memory" (1B), "Reflections on First Counselling Sessions" (E2B). On the synthetic workshop fixture: "Staff Induction - Customer Privacy & Accessible Communication" (1B), "Staff Induction Workshop Planning" (E2B). Asking for a title first made 1B close its answer with a misspelt tag (`</voiceloom_result>`); the stricter prompt and tolerant parsing fixed it.

Validation: 38 Python checks (music-bed re-read and seam de-duplication, which fail with the re-read disabled; title extraction; style prompts, the Counselling default and fallbacks; `## ` titles, inline titles and empty sections). 12 Rust checks, including title replacement only over "Untitled recording" and style/prompt settings validation. 13 browser checks, including the Counselling default, the Session notes heading and saving custom instructions. TypeScript/privacy and desktop clippy pass.

## Version 0.1.6: E4B option, model removal, progress indicator

E4B pinning was checked against Hugging Face repository metadata and the `resolve` headers of the pinned revision (`x-linked-size` 4,590,807,392, `x-linked-etag` equal to the manifest SHA-256). The weights were not downloaded or run.

Validation: 31 Python checks (the manifest pins E2B and E4B as optional, manual-only Q4_0 GGUFs at 40-character revisions). 10 Rust unit checks, including:
- every summary profile picking its pinned file, with unknown profiles rejected;
- model removal with real files: it refuses an empty or unknown id, a busy inference lock and a running job; it deletes the model, a `.part` download and the matching `*.xnnpack_cache`; it keeps a custom GGUF and other cache files; the reported freed bytes and `disk_bytes` are correct.

The offline setup integration check now includes installing only E4B and saving the `e4b` choice. 12 browser checks add:
- E4B in first-run setup (5.4 GB total estimate, memory warning);
- per-model and remove-all confirmation dialogs with the exact request payloads, a "Keep" that sends nothing, and a refreshed "No models are stored" state;
- the progress indicator staying fully in view after scrolling to the end of a 120-row transcript, with its drop-down, Cancel, "All done" timeout, a failed job's error and Retry (re-queue, then dismiss the failure);
- a steady "Transcribing live" item while recording (no percentage, waiting jobs listed), the real pass with its percentage once paused, and no live claim when no transcription model is installed.

TypeScript/privacy and desktop clippy pass. The 0.1.6 package passed the native smoke test (see IMPLEMENTATION.md).

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
