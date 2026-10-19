# VoxLoom

A Linux desktop recorder, transcript library, and private audio/text search tool. All inference runs locally. Summaries are optional and generated only when requested.

## For everyday use

VoxLoom records a conversation on your own computer and keeps the words so you can find them later. The recording, the transcript and any notes stay on this machine. Nothing is uploaded, and there is no account.

The pictures below use sample titles so you can see the layout. Your own library starts empty.

![Home, light theme](docs/images/home-light.png)

![Home, dark theme](docs/images/home-dark.png)

![A transcript you can read and correct](docs/images/transcript.png)

**Open it.** After VoxLoom is installed, it appears in the applications menu.

**First time.** Choose **Download & set up** if you want live text, search and notes. That download happens once, and only when you ask. You can skip it and still record; the text can be prepared later.

**Record.** Choose **New recording**, then **Microphone** (people in the room), **System audio** (a call or a video playing on this computer), or **Both**. People should know you are recording. Press **Stop** when you are done. Words appear a few seconds after someone pauses.

**Read it back.** Open the recording. Click a time to hear that moment, and correct a line if a word is wrong.

**Notes, only if you ask.** **Summarise** writes notes from the transcript. Copy them formatted for Word or your case notes, or as Markdown. Starting or importing a recording never writes notes by itself.

**Find something later.** **Search** looks through what was said, on this computer.

**Light or dark.** The icon beside **Quit** in the sidebar switches the theme. **Settings** has the same choice, including **System**, which follows the desktop.

Current build: functional development preview on Ubuntu 24.04 x86_64. Real Whisper, native-audio EmbeddingGemma 2 740M retrieval, on-demand Gemma 4 E2B summaries and synthetic PipeWire dual capture have been exercised. E2B summaries have been checked on a real counselling recording. Business meeting quality, physical-device behavior and multi-hour reliability still need testing. See [testing and measurements](docs/TESTING.md).

## Install or run the preview

Released versions come with ready-made builds on this repository's GitHub Releases page: the Linux `.deb` and a Windows 11 preview zip (no recording yet; unzip it and open `voxloom.exe`).

Install `release/voxloom_0.1.14_amd64.deb` to add VoxLoom to the applications menu. On this prepared Ubuntu computer, you can also open `release/VoxLoom-0.1.14/voxloom` directly; keep its `usr/` folder beside it. Both run the compiled native app without a development server.

The downloaded development models and setup data have been removed for a fresh first-run test. Choose **Download & set up** when the app opens. Models are separate from the package and download only when requested. This preview has passed native launch, compact/expand and Quit checks; physical devices and business recordings still need manual testing.

## Build the app yourself (step by step)

You need an Ubuntu 24.04 (or newer) x86_64 computer, an internet connection for the first build, and about 6 GB of free disk space. No programming knowledge is needed.

1. **Get the code.** Download or clone this repository and open a terminal in its folder.
2. **Build.** Run:

   ```bash
   bash scripts/build-app.sh --install-deps
   ```

   The script does everything in order and stops with a clear message if something is missing:
   - installs the Ubuntu packages listed below (it asks for your password once)
   - installs private copies of Node.js 24 and Rust into `.toolchain/` if your computer doesn't have them; your system and shell settings are not changed
   - downloads the JavaScript libraries
   - compiles the speech and summary engines from pinned, checksum-verified sources. This happens only the first time and takes about 5–15 minutes; the whole first build took about 9 minutes on a 4-core laptop.
   - builds the app and copies the result into `release/`

   Add `--check` to run the automated tests before building.
3. **Install.** The script prints the exact command at the end, for example:

   ```bash
   sudo apt install ./release/voxloom_0.1.14_amd64.deb
   ```

   Or run it without installing: `release/VoxLoom-0.1.14/voxloom`.
4. **First run.** Open VoxLoom from the applications menu and choose **Download & set up** to fetch the AI models (about 3.6 GB with the default summary model). Models are never part of the build.

To build again after pulling changes, run `bash scripts/build-app.sh`. Later builds take a few minutes. Upgrading keeps your recordings, models and settings.

System packages the script installs or checks (to install them yourself):

```bash
sudo apt-get install -y --no-install-recommends build-essential cmake pkg-config curl libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf ffmpeg pulseaudio-utils python3 python3-venv
```

Developers and coding agents: see [AGENTS.md](AGENTS.md) for the project layout, where data is stored, the test suite and the release checklist.

## Run this checkout for development

```bash
bash scripts/run-local.sh
```

This uses the Git-ignored `.local-data/` folder for this checkout's private library and model setup. The development test models were deliberately removed; provision them again before running real-engine checks. It starts the actual native application, with no dev server. Close the main window or click Quit to flush capture, stop inference and release workers. The compact-recorder button shrinks the same window into a floating bar that stays on top; drag it anywhere by the bar. Expand returns to the transcription studio.

## Development prerequisites

Ubuntu 24.04+: Node 24 (22 still works), Rust stable (1.85+ declaration; validated with 1.99), a C/C++ toolchain and the packages above. `scripts/build-app.sh` sets all of this up; to do it by hand:

```bash
npm ci
```

For source development or publishing only, build the CPU engines and prepare a development runtime in this checkout's `.local-data/` (these commands are not shown to end users):

```bash
VOXLOOM_DATA_DIR="$PWD/.local-data" bash scripts/setup-runtimes.sh
npm run desktop
```

The development launcher also finds the project-local `.toolchain/` Rust installation when present. Source-local `.venv/` and `.runtimes/` are discovered automatically in development; an installed build finds the XDG runtime paths created by the setup script. Settings allows executable/model path overrides. The optional E2B profile requires a current compatible llama.cpp; the pinned runtime contains Gemma 4 support, and short E2B generation has been exercised with user-provisioned local weights.

Build commands:

```bash
npm run build
bash scripts/desktop.sh build --debug --no-bundle
npm run desktop:build
```

The final command builds a Linux `.deb` under `src-tauri/target/release/bundle/deb/`. The package includes optimized CPU audio/summary executables plus generic CPU fallbacks. AI weights remain separate. The first-run **Download & set up** button installs its private search support and selected models together; ordinary users do not compile engines or run terminal commands. It is a preview package until installation/cold-start is tested on a fresh machine. No AppImage, Windows or macOS package is provided.

## Model setup and offline use

Use the first-run wizard or Settings → Model setup. Choose optional summaries and select **Gemma 4 E2B** (recommended, 2.84 GB) or **Gemma 4 E4B** (largest, 4.59 GB) before setup. The wizard explains the tradeoff and updates the total download estimate. **Download & set up** handles local runtime support, the core models and only your selected summary model automatically, with plain-language progress and cancellation. The summary selection is saved after successful setup. Technical paths and runtime diagnostics are collapsed under Advanced settings. **Use files from this device** accepts an offline setup folder (`models/` plus `wheels/` when search support is not already installed). Recording is available if setup is skipped. Download buttons are explicit network approval; app startup never downloads anything. Offline import verifies the exact model file against the pinned manifest. Gemma terms are linked before download/import. No credentials are requested; gated access must be completed through the model repository before importing a file.

For developers or offline administrators, equivalent model-only CLI actions (standard weights, including Gemma 4 E2B, total about 3.47 GB; core-only about 633 MB):

```bash
bash scripts/provision-models.sh --download
bash scripts/provision-models.sh --download --core-only
bash scripts/provision-models.sh --check
bash scripts/provision-models.sh --offline-import /path/to/model-files --core-only
bash scripts/provision-models.sh --offline-import /path/to/ggml-base.en.bin --model whisper
```

Optional E4B (4.59 GB) is an independent, explicit download/import; E2B alone is `--model summary_e2b`:

```bash
bash scripts/provision-models.sh --download --model summary_e4b
```

**More accurate transcription (optional, 264 MB):** in Settings → Model setup, download **Transcription · more accurate** (Whisper Small English), then choose **Settings → Transcription → Accurate** and **Save settings**. It mishears fewer words, especially on noisy microphones, but uses about three times more processor time and text appears a few seconds later; on a 4-core laptop it is best for one audio source at a time. Until it is downloaded, Standard is used. Re-run **Transcribe** on a recording to redo it with the selected model. CLI: `bash scripts/provision-models.sh --download --model whisper_accurate`.

Select **Gemma 4 E2B** (default) or **Gemma 4 E4B** in Settings → Summary model, then **Save settings**. Neither is loaded until Summarise or Q&A is requested. To compare, export the first summary before switching and regenerating. Gemma 3 1B was removed in 0.1.8 because it listed next steps nobody had agreed; a library that had it selected now uses E2B, and an existing 1B file appears in Settings as "No longer used by VoxLoom" with **Remove**. One real E2B summary of a 2.5-minute transcript took 58 s and peaked at about 4 GB of memory with two threads. E4B is the same model family at about twice the size: expect about 6 GB of free memory and roughly twice the time. It has not been run in VoxLoom yet. Google's AI Edge Gallery offers a 3.41 GB LiteRT build of the same E4B model; VoxLoom's summary engine (llama.cpp) needs the GGUF file instead.

**Free up space:** Settings → Local intelligence shows **Remove** next to every downloaded model, and **Free up space → Remove all models** deletes them all at once. Removal also deletes unfinished downloads and the search model's compiled caches (about 370 MB). Recordings, transcripts, summaries and settings stay. VoxLoom refuses while recording, while a job is queued or running, or during setup. Download again at any time from Settings → Model setup. A custom GGUF chosen in Settings is your own file and is never deleted.

Files live in `${XDG_DATA_HOME:-~/.local/share}/voxloom/`, with cache in `${XDG_CACHE_HOME:-~/.cache}/voxloom/`. `VOXLOOM_DATA_DIR` and `VOXLOOM_CACHE_DIR` override these roots; Settings can select another model directory. Paths with spaces and non-ASCII characters are supported. `--directory` overrides the CLI model folder. Nothing requires network after runtime and model provisioning. Recording/playback works with no models; transcript needs Whisper; semantic search needs the real full 740M LiteRT checkpoint; summary/Q&A needs the selected GGUF.

## Recording, video and summaries

- **Microphone:** face-to-face conversations and your own speech.
- **System audio:** select the monitor of the actual speakers/headphones used by Teams, Zoom, a browser or training player.
- **Both:** separate mic and system tracks. Headphones reduce acoustic echo. Recording is always manually started, visibly indicated, and subject to participant consent.
- Audio is saved continuously. Live transcription reads the file still being recorded and transcribes each stretch of speech once it ends in a real pause, so words usually appear a few seconds after someone stops talking (nonstop speech appears within about 30 seconds). Each line is one sentence; a sentence interrupted by a pause is completed when the speaker continues. Stop/Pause flushes the final speech. This preview does not provide word-by-word streaming. Completed tracks and transcription checkpoints survive interruptions; pause/resume excludes paused wall time.
- Import WAV/MP3/FLAC/OGG/M4A/OPUS or local MP4/MKV/WebM/MOV. FFmpeg stores only the extracted audio as FLAC. No YouTube URL downloader is included. You can capture a video's browser playback instead.
- Review/edit the saved transcript, then click **Summarise** if wanted. Settings → **Summary style** chooses what the notes focus on: **Counselling or support session** (default: main concerns, feelings and experiences, strengths and supports, agreed next steps; no diagnosis, labels or advice that was not said), **Service conversation** (reason for contact, information shared, support discussed, agreed follow-up), **Meeting** (discussion points, decisions, actions), or **Custom instructions** you write yourself (up to 2,000 characters; VoxLoom still keeps to what was said and adds no timestamps). Notes are grouped by topic, without source timestamps. **Copy notes** pastes formatted headings, paragraphs and bullets into Word, Outlook or a case-notes system; **Copy Markdown** copies the same notes as Markdown (`## ` headings, `- ` bullets) for note apps and wikis. If the recording is still called "Untitled recording", the same Summarise run also gives it a short title (for example "Staff Induction Workshop Planning"); a title you typed or an imported file's name is never changed. Stopping/importing never generates summaries automatically. Transcription and search preparation start automatically when their local tools are installed. Corrections invalidate the old search data; Prepare search updates it after review. Exact search remains available without embeddings.
- **Progress:** transcription, search preparation and summaries show their progress in the top bar, which stays in place while you scroll a long transcript. Click it to see every job, its recording and **Cancel**. It shows **All done** when work finishes, and turns red with **Retry** if a job fails. While you record, it shows a steady **Transcribing live** instead of the short passes that restart every 2.5 seconds; words appear in the studio a few seconds after each pause.
- **Appearance:** System (follows the desktop), Light or Dark in Settings → Appearance; the small button beside Quit in the sidebar cycles through them.
- Timestamp buttons seek the appropriate source on the recording timeline. Playback advances through that source's chunks; audio export joins its chunks into one FLAC. The normal export choices are Word documents, plain text and subtitles. Transcript timestamps remain in the Transcript tab. A synchronized microphone+system playback/export mix remains future work.
- Delete removes the managed recording and derived data. User-created exports are independent files.

## Audio troubleshooting

```bash
pactl --format=json list sources
pactl get-default-sink
ffmpeg -devices
```

PipeWire must expose its `pipewire-pulse` compatibility socket in the user's desktop session. Install `pulseaudio-utils` for pactl and FFmpeg with Pulse input support. If a headset disconnects or changes profile, stop, refresh sources and select the new monitor/input. The app never silently changes default routes or creates a virtual sink. System capture includes any sounds sent to that output, including notifications. PipeWire normally exposes a monitor before output volume changes, but configurations vary: watch the real input meter. Keep the meeting app/browser tab/player itself unmuted. Physical-device mute behavior needs validation on your output.

## Checks

```bash
npm run check
npm test
npm run test:ui
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --offline
```

Real-engine acceptance requires local fixtures/weights; see [TESTING.md](docs/TESTING.md). Architecture and limitations: [ARCHITECTURE.md](docs/ARCHITECTURE.md). Runtime/model provenance: [MODEL_SOURCES.md](docs/MODEL_SOURCES.md).

## License

VoxLoom's source code is released under the [MIT License](LICENSE). The engines it builds (whisper.cpp, llama.cpp) are also MIT. The models you download during setup keep their own terms, listed in [MODEL_SOURCES.md](docs/MODEL_SOURCES.md); Gemma models follow the [Gemma Terms of Use](https://ai.google.dev/gemma/terms).
