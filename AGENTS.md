# AGENTS.md: how to work on VoxLoom

This is the single source of truth for every coding agent (Codex, Claude, others) and human contributor. Read it before changing anything. If you change how VoxLoom is built, tested, stored or released, update this file in the same change. `CLAUDE.md` only points here.

## What VoxLoom is

A private Linux desktop app (Ubuntu 24.04, x86_64) that records microphone and/or system audio, transcribes it live, and offers local search and on-demand summaries. Everything runs on the user's own CPU; nothing is sent anywhere.

- **Desktop shell:** Tauri 2 hosting a React/TypeScript UI in the system WebKitGTK.
- **Rust host** (`src-tauri/src`): capture, SQLite library, job queue, worker supervision.
- **Python worker** (`worker/engine.py`): one fresh process per job. It drives:
  - `whisper-cli` (whisper.cpp v1.9.5) for transcripts
  - LiteRT-LM EmbeddingGemma 2 740M for search
  - `llama-completion` (llama.cpp v0.6.0, patched) for summaries
- **Target hardware:** ordinary business laptops with 4–8 CPU cores and 16 GB RAM, no GPU. Measure CPU and latency on that class of machine before making anything heavier.
- **Windows 11 (x64)** is being added on the same codebase, not a fork; see `docs/WINDOWS.md` for status and plan. The Windows package bundles every runtime (its own Python with LiteRT, FFmpeg, the engines) so users install nothing else; only models download, through first-run setup. Linux behaviour must not change while this happens.

## Ground rules (do not break these)

1. **Losing transcript content is unacceptable.** See "Transcription invariants" below. A transcript that is late or has extra words is acceptable; a missing one is not.
2. **Local only.** The app makes no network requests at runtime. Model/runtime downloads happen only after the user clicks Download in setup.
   - No telemetry, CDN, fonts or update plugins.
   - `scripts/privacy-check.py` (part of `npm run check`) enforces this.
3. **Private data stays out of command lines and logs.** Transcript text, audio and file names are never put on a command line, in a shell string or in routine logs. Pass them through files or stdin. This is why Whisper gets no `--prompt`.
4. **Models are never in Git or the package.** `models/manifest.json` pins repository, revision, size and SHA-256 for each model. Downloads verify hashes before use.
5. **Summaries run only when the user asks.** Recording and import never trigger one.
6. **Recording comes first.** Only one heavy job runs at a time, and live transcription outranks indexing and summaries. Each worker exits when its job ends; no model stays loaded.
7. **Several agents share this checkout.**
   - Run `git status` before you start.
   - Never delete build caches (`.runtimes/`, `src-tauri/target/`, `.toolchain/`, `.local-data/`) or `release/` contents without saying so to the user.
   - Do not commit unless the user asks.
8. **Never commit or package private recordings or transcripts.** If you analyse the user's real recordings, copy them to a temporary folder and delete the copies when done.

## Repository map

| Path | What it is |
|---|---|
| `src/` | React UI. `App.tsx` holds most of it; `api.ts` sends every call through one Tauri command, `dispatch(action, payload)`. |
| `src-tauri/src/service.rs` | All `dispatch` actions, the job queue (`tick`), worker launch (`run_worker`), result ingestion and the live-transcription scheduler. |
| `src-tauri/src/audio.rs` | Capture sessions, 30-second FLAC segment bookkeeping (`sNNNN-mic-NNNNNN.flac`, `sNNNN-system-NNNNNN.flac`), durations and export. The platform backends find devices, start capture and end it: `audio/pulse.rs` (Linux: FFmpeg reads Pulse/PipeWire) and `audio/wasapi.rs` (Windows: WASAPI microphones and output loopback, fed to FFmpeg on stdin through `audio/pcm.rs`, which keeps the sources on one clock and writes silence where a source heard nothing). |
| `src-tauri/src/process.rs`, `src-tauri/src/platform.rs` | Everything else that differs by OS. `process`: build commands (`command` supervised, `hidden` for quick probes, `python` adds UTF-8 mode on Windows), `spawn` children `kill_tree` may have to end, `finished` once one has been waited for, and `contain_children` (Windows: the app's own kill-on-close Job Object, so a crash still ends every worker and engine). `platform`: data folders, private permissions, free disk space, CPU features, executable and venv names, bundled tools on PATH, PATH lookup. |
| `src-tauri/src/db.rs`, `src-tauri/migrations/` | SQLite (WAL, FTS5). Migrations are idempotent SQL files applied in order on every open. |
| `src-tauri/src/search.rs` | Vector storage, exact/semantic/combined search. |
| `src-tauri/tests/` | Integration tests. The real-engine and real-capture tests are `#[ignore]`d and opt-in. |
| `worker/engine.py` | Transcription, indexing, summaries, Q&A. Uses only the Python standard library, except LiteRT for embeddings. |
| `worker/provision.py`, `worker/setup_runtime.py` | Model download/import with hash checks; private Python venv for LiteRT. |
| `models/manifest.json` | Pinned model sources. Changes must keep the SHA-256 and revision pinning. |
| `VERSION` | The app version, the only place to change it. `scripts/version.py set X.Y.Z` writes the copies npm and Cargo need; the UI reads `VERSION`, `tauri.conf.json` reads `package.json`. |
| `scripts/` | Build, run and test tools (listed below). |
| `tests/` | Python unit tests (`test_worker.py`, `test_notes.py`) and Playwright UI tests (`tests/ui`). |
| `docs/` | `ARCHITECTURE.md` (how it works), `TESTING.md` (what was verified, with numbers), `IMPLEMENTATION.md` (per-version log), `PERFORMANCE.md`, `MODEL_SOURCES.md`, `WINDOWS.md` (Windows port status and plan), `reports/` (JSON measurement reports). |
| `.github/workflows/ci.yml` | Every push that changes more than documentation (`*.md`, `docs/`): Windows and Linux run the fast checks; Windows also checks the bundled runtime, the Job Objects, a debug build and its launch; a Windows real-engine job runs the whole import-to-delete workflow with cached pinned models. Pull requests run it only from forks (this repository's branches are checked by their pushes). |
| `.github/workflows/windows-runtime.yml` | Shared by CI and Apps: cross-builds the Windows runtime on Linux, caches it until its recipe changes, and makes the speech fixtures when asked. |
| `src-tauri/tauri.windows.conf.json`, `src-tauri/windows/` | Windows packaging: the NSIS installer (per-user `%LOCALAPPDATA%\Programs\VoxLoom` or per-machine `C:\Program Files\VoxLoom`, chosen at install) and what it bundles. `hooks.nsh` makes the uninstaller remove everything VoxLoom put on the computer except the user's recordings and library, which go only when asked, and makes an upgrade remove nothing (a newer installer runs the old uninstaller in place, `_?=`, which every real uninstall never does); `English.nsh` is Tauri's English text with VoxLoom's wording for that choice. |
| `.github/workflows/apps.yml`, `scripts/package-windows-preview.py`, `scripts/installer-smoke-windows.py` | Apps: builds the Linux `.deb`, the Windows installer and the Windows preview zip (runs without installing) from main, launches each, installs and uninstalls the Windows installer in both modes checking nothing is left behind, and attaches them to a draft GitHub release for the version. Runs only when a push to main changes `VERSION` or this recipe, or from Actions → Apps → Run workflow. |
| `scripts/build-windows-runtime.sh`, `worker/requirements.windows.lock.txt` | Cross-builds on Linux everything the Windows package bundles, from pinned sources with SHA-256: the engines (same pins as `setup-runtimes.sh`), a minimal LGPL FFmpeg, and embeddable CPython with LiteRT installed with `--require-hashes`. `scripts/windows-runtime-smoke.py` checks the result on Windows. |

## Where things are stored

| Location | Contents | In Git? |
|---|---|---|
| `~/.local/share/voxloom/` (`VOXLOOM_DATA_DIR`) | Installed app's library: `library.db`, `recordings/<id>/`, `models/`, `runtimes/inference` (LiteRT venv). Private (0700). | Never |
| `~/.cache/voxloom/` (`VOXLOOM_CACHE_DIR`) | Temporary job folders, the last inference metrics and `verified-models.json` (size, mtime and pinned hash of each model that passed its SHA-256 check, so workers do not re-hash unchanged files). | Never |
| `%LOCALAPPDATA%\org.voxloom.desktop\` and `…\cache\` | The same library and cache on Windows. Local, not Roaming (Roaming can sync to a server on company networks), and not the per-user program folder `%LOCALAPPDATA%\Programs\VoxLoom`. The WebView2 profile (`EBWebView`) lives here too. The uninstaller always deletes `models`, `cache` and `EBWebView`; the library and recordings only if the user ticks "Also delete my recordings, transcripts and settings" (or runs `uninstall.exe /S /DELETEDATA`). Updates delete nothing. | Never |
| `.local-data/windows-runtime/` | `build-windows-runtime.sh` output: `engines/`, `python/`, `licenses/` (bundled software's licences and sources), `VERSIONS.txt`, and `build/` (sources and objects, reusable). | Ignored |
| `src-tauri/engines/`, `src-tauri/python/`, `src-tauri/licenses/`, `src-tauri/VERSIONS.txt` on Windows | The Windows runtime staged for debug builds (found through `resources/src-tauri/…`) and packaging. | Ignored |
| `.local-data/` | This checkout's own library, models and compiled engines (`.local-data/runtimes/{whisper,llama}{,-generic}/bin`). Used by `run-local.sh` and `build-app.sh`, so development never touches the installed app's data. | Ignored |
| `.toolchain/` | Private Rust (`rustup`, `cargo`) and, if needed, Node (`node/`). Created by `build-app.sh`. | Ignored |
| `.venv/` | Development Python with LiteRT, for real-engine tests. | Ignored |
| `.runtimes/` | Older layout for engine builds (`whisper-build/bin`, `llama-build/bin`, `generic/`). Still accepted by `desktop.sh`. | Ignored |
| `src-tauri/engines/` | Engines staged by `desktop.sh` just before packaging. | Ignored |
| `src-tauri/target/` | Cargo build output; the package lands in `target/release/bundle/deb/`. | Ignored |
| `dist/` | Built UI (`npm run build`). | Ignored |
| `release/` | Hand-off artifacts: `voxloom_<version>_amd64.deb` plus `VoxLoom-<version>/` (the extracted package with `voxloom` symlink and `START HERE.txt`). Keep the previous version until the user says otherwise. | Ignored |
| `/tmp/voxloom-*.wav` | Synthetic speech fixtures from `scripts/make-fixtures.sh`. | No |

Audio, video, Word/SRT exports, models and databases match `.gitignore` patterns (`*.flac`, `*.wav`, `*.m4a`, `*.mp4`, `*.docx`, `*.srt`, `*.bin`, `*.gguf`, `*.litertlm`, `*.db*`, and the other formats import accepts). Never force-add them.

## Set up a development machine

Easiest: `bash scripts/build-app.sh --install-deps`. It installs missing Ubuntu packages (with sudo), private Node/Rust, the npm dependencies and the compiled engines, then builds the package.

Node is only a build tool; nothing in the app runs on it. CI and the private copy use Node 24 LTS (`build-app.sh` pins version and SHA-256); a system Node 22 still works until its support ends in April 2027. npm 11 (Node 24) runs a dependency's install script only if `allowScripts` in `package.json` allows it. None is allowed: esbuild works from its platform package without its own. Allow a new one only with a reason.

Manual equivalent:

```bash
sudo apt-get install -y --no-install-recommends build-essential cmake pkg-config curl \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf \
  ffmpeg pulseaudio-utils python3 python3-venv
# Optional, for tests: espeak-ng xvfb, and Google Chrome or `npx playwright install chromium`
npm ci
VOXLOOM_DATA_DIR="$PWD/.local-data" bash scripts/setup-runtimes.sh   # engines + LiteRT venv
```

For direct `cargo` commands with the private toolchain:

```bash
export RUSTUP_HOME="$PWD/.toolchain/rustup" CARGO_HOME="$PWD/.toolchain/cargo" PATH="$PWD/.toolchain/cargo/bin:$PATH"
```

`scripts/desktop.sh` does this by itself.

## Run the app from source

```bash
bash scripts/desktop.sh build --debug --no-bundle   # compile a debug app
bash scripts/run-local.sh                           # run it with .local-data/ (not your installed library)
```

Use the app's first-run "Download & set up", or `bash scripts/provision-models.sh --download --directory .local-data/models`, to get models for this checkout.

## Tests

Run the fast set before every hand-off:

```bash
npm run check                 # TypeScript + privacy rules
npm test                      # Python unit tests (unittest discover -s tests, via scripts/python.mjs)
npm run test:ui               # Playwright UI tests (PLAYWRIGHT_CHROME=/path/to/chrome to choose a browser)
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --offline
cargo clippy --manifest-path src-tauri/Cargo.toml --offline -- -D warnings
```

`cargo clippy --all-targets` currently reports `items_after_test_module` in `service.rs`, an older layout issue. The documented check is without `--all-targets`. Clippy and the desktop build need `src-tauri/engines/` to exist (empty is fine); `desktop.sh` creates it. Windows builds also need the rest of the staged runtime the installer bundles: `src-tauri/python/`, `src-tauri/licenses/` and `src-tauri/VERSIONS.txt` (empty placeholders are fine for clippy).

Windows-specific code must also compile. From Linux, with `mingw-w64` installed and `rustup target add x86_64-pc-windows-gnu`:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu -- -D warnings
```

Every push that changes more than documentation also runs `.github/workflows/ci.yml` on GitHub's Windows and Linux machines: the fast set above, plus on Windows `scripts/windows-runtime-smoke.py` (every bundled program starts, FFmpeg writes capture-style segments in a non-ASCII folder, the bundled Python loads LiteRT), the Job Object tests, the capture graph through the bundled FFmpeg, device listing without audio hardware, a debug build, `scripts/native-smoke-windows.py` (launch with throwaway data, close the window, nothing left running), and the real-engine workflow test through a model folder with non-ASCII letters. GitHub's Windows machines are Windows Server with no audio devices, so recording and speed still need a real Windows 11 laptop.

For visual work, `TOUR_OUT=/tmp/voxloom-tour npx playwright test tour` captures every page in light and dark with sample data (skipped otherwise).

Real-engine checks are opt-in. They need models and fixtures, and they never download anything. Point `--models` at an existing models folder; `~/.local/share/voxloom/models` is fine read-only.

| Check | Command |
|---|---|
| Speech fixtures | `bash scripts/make-fixtures.sh` |
| Live transcription, sentences across 30 s file boundaries | `python3 scripts/test-transcript-continuity.py --models <models> --whisper-cli <whisper-cli>` |
| **Replay a real recording exactly as captured** (closed files + growing file, one live pass per 2.5 s); prints coverage, lag and rows | `python3 scripts/replay-live-asr.py --recording ~/.local/share/voxloom/recordings/<id> --model <models>/ggml-base.en.bin --whisper-cli <whisper-cli> --out /tmp/rows.json` |
| Real capture through a temporary PipeWire test sink (removed afterwards; defaults unchanged) | `VOXLOOM_TEST_MODELS=<models> VOXLOOM_TEST_WHISPER_CLI=<whisper-cli> python3 scripts/test-capture.py` |
| Full import → transcribe → index → search → summary → export → delete | see `docs/TESTING.md`, "Real local engines and full workflow" |
| Packaged app launches, floats/expands and quits cleanly (hidden Xvfb display, throwaway data) | `xvfb-run -a python3 scripts/native-smoke.py <path>/usr/bin/voxloom` |
| Engines, summaries, clipboard | `scripts/probe-engines.py`, `scripts/test-meeting-notes.py`, `scripts/test-native-notes.py` |

`<whisper-cli>` can be `.local-data/runtimes/whisper/bin/whisper-cli` or the installed `/usr/lib/voxloom/engines/whisper-cli`.

To judge transcription quality on real audio, compare against a stronger reference transcript (e.g. large-v3-turbo over the whole file). Report word error rate and runs of missing words, not impressions. Record the numbers in `docs/TESTING.md`.

## Build and release

```bash
bash scripts/build-app.sh            # add --check to run the fast tests first
```

The Windows runtime is built on Linux (mingw-w64, cmake, make, nasm, python3-venv) and needs github.com, ffmpeg.org, python.org and PyPI: `bash scripts/build-windows-runtime.sh [--parts engines,ffmpeg,python] [output]`. The first full build takes about 20 minutes; CI caches it. With it staged in `src-tauri/`, `npx tauri build` on Windows makes the installer (`src-tauri/target/release/bundle/nsis/VoxLoom_<version>_x64-setup.exe`); `scripts/package-windows-preview.py` then makes the zip from the same build. Installing and uninstalling is described in `docs/WINDOWS.md`.

GitHub builds the apps too. The Apps workflow (`.github/workflows/apps.yml`) runs when a push to main changes `VERSION`, or from Actions → Apps → Run workflow. Branches never build apps. It makes `voxloom_<version>_amd64.deb` with `build-app.sh`, `VoxLoom-<version>-windows-x64-setup.exe` with `npx tauri build`, and `VoxLoom-<version>-windows-x64-preview.zip` with `scripts/package-windows-preview.py`, and runs each one's launch check. `scripts/installer-smoke-windows.py` installs the setup per user and per machine, launches the installed app, uninstalls it (keeping recordings, then with `/DELETEDATA`) and fails if anything is left behind; it also writes `VoxLoom-<version>-windows-x64-programs-SHA256.txt`, every program file the installer puts down with its SHA-256, for IT allow-listing. On main it attaches all of them, with their SHA-256, to a draft release `v<version>`. A draft is visible only to people with access to the repository; publishing it is a person's decision, and the workflow never changes a published release. So releasing is: bump the version, push to main, check the draft, publish.

The script checks system packages, installs private Node/Rust when needed, runs `npm ci`, and compiles the engines once into `.local-data/runtimes`. It then runs `desktop.sh build --bundles deb` and copies the `.deb` plus the run-in-place folder into `release/`. Use `--release-dir DIR` to build somewhere else, for example while the user is installing the current release. The first build spends most of its time compiling the engines; it took about 9 minutes in total on a 4-core laptop. `VOXLOOM_BUILD_JOBS` limits the number of parallel compile jobs, which defaults to all cores. Later builds reuse the engines and take 1–2 minutes.

When releasing a new version:

1. Bump the version: `python3 scripts/version.py set X.Y.Z`. It writes `VERSION` and the copies in `package.json`, `package-lock.json` (root entries), `src-tauri/Cargo.toml` and the `voxloom` entry of `src-tauri/Cargo.lock`. `npm run check` and `build-app.sh` fail if any copy differs from `VERSION`. Never edit those copies by hand.
2. Run the fast tests and the real checks relevant to the change.
3. Build, then run the native smoke test on `release/VoxLoom-<version>/usr/bin/voxloom`. Or merge to main and let the Apps workflow build both apps and run their launch checks; the draft release then holds the files and SHA-256.
4. Add a version section to `docs/IMPLEMENTATION.md` (implemented / verified / limitations / release result / next) and to `docs/TESTING.md` (with measured numbers). Update `README.md` paths and user-visible behaviour.
5. Tell the user the install command (`sudo apt install ./release/voxloom_<version>_amd64.deb`) and the SHA-256. Upgrades keep the user's recordings, models and settings.

## Transcription invariants

The live path: `service.rs::tick` polls every 2.5 s → `request("transcribe_live")` → `engine.py::transcribe`. Rules:

- **Every millisecond belongs to exactly one chunk.** Chunks never overlap and never leave gaps. Each chunk is at most 28 s, one Whisper window.
- **Cuts happen only inside pauses** found by `plan_chunk`. Pauses are speech-band (250–3800 Hz) levels relative to an adaptive noise floor (`noise_floor`). Never use a fixed dB threshold: real microphones sit around −32 dBFS and system audio around −55 dBFS. Only nonstop speech longer than a window is cut, at its quietest moment.
- **Never skip audio on a doubtful estimate.** Skip only digital silence, or audio with clear voice/pause contrast (`has_contrast`) and no voiced frames. Without contrast (music bed, nonstop talk), transcribe everything.
- **Never drop Whisper text because of timestamps.** Timestamps only place rows on the timeline.
- **Whisper can silently skip trailing speech** ("single timestamp ending"). `transcribe_chunk` re-decodes voiced stretches that returned no words. Keep that safety net.
- **Whisper stretches a segment over a music bed** and skips the speech after the music (`stretched`). When a cut has no pause near it and the chunk's last segment runs to its end at under 8 characters a second, `transcribe` keeps that segment, ends the chunk where its words must have ended (18 characters a second), and decodes the rest again with the following audio. Words heard twice at that join are dropped with `drop_repeated_start`. Committed chunks still never overlap.
- **Rows are sentences.** An unfinished sentence is carried in the checkpoint and finished by the next chunk, or flushed after silence, at 300 characters / 30 s, or at Stop/Pause.
- **Checkpoints are atomic.** Coverage (`covered_ms`), rows and carry state commit in one transaction. The state is stored as JSON in `asr_progress.overlap_text`; the column name is historical.
- **The file still being recorded** is read up to its last CRC-valid frame (`decode_pcm(growing=True)`). It is never registered or cached. This depends on FFmpeg's `-segment_format_options flush_packets=1` in `audio.rs`; without it the newest file is unreadable for about 16 s.
- **A provisional (non-final) pass that ends after Stop/Pause** must trigger a final pass. `tick` does this. Stop and Pause both queue one themselves (`finish_capture`), and start-up queues one for an interrupted recording that still has unread files or an unfinished sentence (`recover_tracks`).
- **A capture segment FFmpeg cannot open holds no audio.** A crash inside the first fraction of a second of a new segment leaves a file with no complete frame; a file with one complete frame decodes however it was cut. `stored_length` counts such a segment as empty so the pass over the earlier segments still runs, and never caches that 0. Because that 0 is never cached, every job that measures a recording's files must use `track_lengths` / `stored_length`, never `duration` directly (indexing did, and failed on every retry). Imported files that cannot be read still fail loudly.
- **A live pass registers closed segments itself.** `request()` calls `closed_tracks()` under the capture and library locks before it reads the growing file, so a segment that closed since `tick`'s last poll is never missing from the stream. Keep registration and both reads under the same locks.
- **Before claiming a transcription improvement,** run the unit tests (`LiveTranscription` in `tests/test_worker.py`), the continuity script and a replay of a real recording.

Summary style: Settings → Summary style (`summary_style`: `counselling` (default), `service`, `meeting`, `custom` with `summary_prompt`, at most 2,000 characters). The prompts live in `engine.py::STYLES`; the Meeting prompt is the pre-0.1.7 text unchanged. Custom instructions are wrapped in the same safety rules and reach the worker in the request file, never on a command line. Saved notes mark section titles with `## `; `notes.py::heading_text` also reads older plain titles. The final summarising pass also returns a title, which replaces only the default name "Untitled recording".

Transcription model choice: first-run setup (and Settings → Model setup) and Settings → Transcription (`transcription_profile`: `standard` = `ggml-base.en.bin`, `accurate` = optional `ggml-small.en-q8_0.bin`). Setup always installs Standard, the fallback; choosing Accurate adds `whisper_accurate`, and the choice is saved only once its model is installed. A setup request without `transcription_profile` keeps the saved choice. `Service::whisper_model` is the only place that picks the file; it falls back to Standard until Accurate is downloaded. Never hard-code a model filename elsewhere.

Tried and rejected (0.1.7): Gemma 4 E2B/E4B as the transcription model (Google AI Edge Foresight's approach, LiteRT-LM `.litertlm`, Google's ASR prompt as a system message, at most 30 s of audio per request). On the counselling recording it missed fewer words than Whisper (E2B 4, E4B 5, Whisper 6) and kept stammers, but misheard more (E2B 8.7%, E4B 7.6% WER against Accurate 5.1%), used 1.6 / 3.3 GB and 0.36 / 0.77 of real time on two threads, and gives no timestamps. With the instructions after the audio it once appended the prompt to the transcript. Through llama.cpp (`llama-mtmd-cli` + `mmproj`) it could not turn off thinking: 75-87 s per 28 s clip. See docs/TESTING.md, "Gemma 4 as a transcription model".

Tried and rejected (after 0.1.10): Moonshine Streaming Small and Medium (`moonshine-voice` 0.1.5, int8, 142 / 269 MB) as extra transcription choices, fed VoxLoom's own chunks with Moonshine's VAD off. Six recordings were scored against the videos' transcripts or large-v3-turbo. Medium was less accurate than Accurate on four of the six (noisy microphone 19.1% vs 11.5%) at the same CPU cost (0.38-0.54 of real time on two cores) and 1.0 GB. Small was slower than Standard and worse on five of the six. Both lost speech silently on difficult audio and invented words: Small wrote 20 s of speech as "♪♪♪", and the longest missing runs were 20 / 11 words against Whisper's 5. Its published speed advantage assumes Whisper re-decodes a window for every streaming update, which VoxLoom does not. See docs/TESTING.md, "Moonshine Streaming as a transcription model".

Tried and rejected (0.1.7): dropping the stretched segment and re-reading it from its start. Accurate improved, but Standard stretched it again over the whole re-read and lost the speech entirely (8 → 16 missing words).

Tried and rejected (0.1.5): DTW word timestamps (`-dtw`, needs `-nfa`) to judge skipped speech. They triggered re-reads over music beds, producing "[MUSIC PLAYING]" rows, repeated phrases and Whisper's "transcription by ESO" hallucination, which raised the word error rate from 4.7% to 8.3%. Segment-span coverage stays.

Performance reference (i7-11370H, 4 cores), one 28 s chunk:

| Model | 2 threads | 4 threads | Notes |
|---|---|---|---|
| base.en (default) | 3.6 s | 2.5 s | |
| small.en (full size) | 10.7 s | 8.8 s | About 4 points lower word error rate on a noisy mic |
| small.en q8_0 ("Accurate") | about 40% of real time at 2 threads | | Fastest small variant on CPU; q5_1 is slower here |
| large-v3-turbo q5_0 | 35 s | 23 s | Too slow for live use |

## Database changes

Add `src-tauri/migrations/00N.sql`. Make it idempotent (`IF NOT EXISTS`) and end it with `PRAGMA user_version = N`. Include it in `Db::open` in `db.rs`, and raise the "created by a newer VoxLoom version" guard there. Never rewrite an existing migration. User libraries must upgrade in place.

## Code conventions

- Colours come only from the tokens at the top of `src/theme.css`, defined for light and dark. Keep text at 4.5:1 contrast or more, form-control borders (`--control-line`) and icons at 3:1, and no text smaller than 11 px. Check both themes with the visual tour.
- Match the surrounding code. `App.tsx` and parts of `service.rs` are deliberately dense; don't reformat files you aren't changing.
- Comments explain why (constraints, failure modes found in testing), not what.
- The worker stays standard-library Python apart from LiteRT. FFmpeg does audio work; pure Python handles small per-frame maths.
- OS differences live in `process.rs`, `platform.rs` and the `audio/` backends, behind `cfg`. Start child processes with `process::command` / `hidden` / `python`, never a bare `Command::new`, so Windows gets no console windows and Linux keeps its process groups. A child that `kill_tree` may have to end starts with `process::spawn`; call `process::finished(pid)` after waiting for one that exited on its own. In the worker, start engines with `child_options()`, and give every text read or write `encoding="utf-8"` (Windows Python defaults to the ANSI code page). A worker script that imports another worker module must first put its own folder on `sys.path`: the embeddable Python bundled on Windows leaves it off.
- New dependencies, runtimes or model downloads need a clear reason and the user's agreement. Anything downloaded must be pinned (version + SHA-256).
- User-facing text is plain and short; technical detail belongs under Advanced settings.
- The app icon's source is `src-tauri/icons/icon.svg` (a rounded tile). To change it, render it at 1024 px (`rsvg-convert -w 1024`), run `npx tauri icon <png> -o <temp>`, and copy only `icon.png` and `icon.ico` into `src-tauri/icons/`.

## Cursor Cloud specific instructions

The Cloud Agent environment installs the Ubuntu packages from "Set up a development machine", plus Xvfb, X11 utilities and ImageMagick for the native launch check. It updates Rust to stable (`rust-version` is 1.85; the base image's 1.83 is too old) and runs `npm ci`. Rust lives in `/usr/local/rustup` and `/usr/local/cargo`. Login shells export `RUSTUP_HOME` and `CARGO_HOME` from `/etc/profile.d/voxloom-rust.sh`. Node 22 is also linked at `/usr/local/bin`.

Playwright uses `/opt/google/chrome/chrome`. The fast checks (`npm run check`, `npm test`, `npm run test:ui`, `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features`, and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` after `mkdir -p src-tauri/engines`) do not need models.

Whisper, llama.cpp and the model files are not installed. `scripts/desktop.sh build` stops until `scripts/setup-runtimes.sh` has compiled the engines. A debug desktop binary does not need them: `npx tauri build --debug --no-bundle`, then `xvfb-run -a python3 scripts/native-smoke.py src-tauri/target/debug/voxloom`. Do not download models unless the task is a real-engine check.

## Known gotchas

- FFmpeg's segment muxer can leave the full recording length in a later file's FLAC header. Count decoded samples (`duration()`), and decode segmented files before seeking.
- Whisper output varies with exact chunk boundaries: the same audio cut a few milliseconds differently can gain or lose words. Compare runs, not single results.
- whisper.cpp always encodes a full 30 s window, so short chunks cost almost as much as long ones.
- Playwright uses `/opt/google/chrome/chrome` if present. Set `PLAYWRIGHT_CHROME`, or install Playwright's Chromium, on other machines.
- Never store a path inside the app's resources in Settings or the library: the Windows preview runs from wherever it was unzipped. Read the Python, Whisper and llama paths through `Service::tool`, which ignores a saved path that is gone or is in another copy's `engines` or `python` folder, and keeps the running copy's own files and custom paths. Settings sends back every value it shows, so `save_settings` changes a tool path only when it differs from what Settings showed.
- `desktop.sh build` needs the engines in `.runtimes/…` or `$VOXLOOM_DATA_DIR/runtimes/…`. A cleanup that removes them breaks packaging until `build-app.sh` (or `setup-runtimes.sh --engines-only`) rebuilds them.
