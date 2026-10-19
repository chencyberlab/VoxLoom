# VoxLoom on Windows

Windows 11 x64 is a second platform of the same codebase. Linux stays fully supported and its behaviour must not change. Windows on Arm, Windows 10 and macOS are out of scope for now.

## What a Windows user gets

One installer with everything the app needs: VoxLoom, its own Python with LiteRT, FFmpeg, and the speech and summary engines. Nothing else to install: no Python, no FFmpeg, no Visual C++ runtime prompts, and WebView2 is part of Windows 11. Only the models are downloaded, through the existing first-run setup (or imported offline), because they are several GB and never ship in the package.

Library and cache: `%LOCALAPPDATA%\org.voxloom.desktop` and its `cache` folder. Local, not Roaming (Roaming can sync to a server on company networks), and not `%LOCALAPPDATA%\VoxLoom`, which is where the per-user installer puts the program. The WebView2 profile (`EBWebView`) and the downloaded models live in the same folder. Uninstalling is described below.

## Status

| Phase | What | State |
|---|---|---|
| 1 | Platform seams; the app compiles, launches and passes the fast tests on Windows | Done |
| 2 | Bundled runtime (engines, FFmpeg, Python with LiteRT, all pinned); Job Objects so a crash leaves nothing running; import → transcribe → index → search → summary → export → delete with the real engines on Windows | Done; verified on GitHub's Windows machines (numbers in `docs/TESTING.md`) |
| 3 | Live capture with WASAPI: microphones and loopback of an output device, same FLAC segments as Linux | Built; checked on GitHub's Windows machines, which have no audio devices. Needs a real Windows 11 laptop |
| 4 | NSIS installer (per-user and per-machine), models-only first-run setup, third-party notices, list of every bundled executable with SHA-256 for IT allow-listing | Built (0.1.19); installed, launched and uninstalled in both modes on GitHub's Windows machines. Not code-signed yet |
| 5 | Measurements on a real Windows 11 laptop, transcription quality checks, docs | |

Recording on Windows has not yet run on real audio hardware; that is the next check.

## The bundled runtime

`scripts/build-windows-runtime.sh` cross-builds it on Linux from pinned sources. Every executable is static (it needs only DLLs that ship with Windows; the build fails otherwise) and carries a manifest that sets its ANSI code page to UTF-8, so paths like `C:\Users\Zoë\…` reach the engines intact.

| Part | Source | Notes |
|---|---|---|
| `engines/whisper-cli.exe`, `-generic.exe` | whisper.cpp, the pin in `setup-runtimes.sh` | AVX2 and generic builds, as on Linux. MinGW-w64 GCC, no OpenMP (ggml's own thread pool; OpenMP would be another DLL). |
| `engines/llama-completion.exe`, `-generic.exe` | llama.cpp, the pin in `setup-runtimes.sh`, patched by `patch-llama-completion.py` | Same build options. |
| `engines/ffmpeg.exe`, `ffprobe.exe` | FFmpeg 7.1.5, SHA-256 `de668509…5d558f`, release signature checked against FFmpeg's key `FCF9 86EA … D676 58D8` | LGPL: no `--enable-gpl`, no external libraries. Only the demuxers, decoders and filters VoxLoom uses (WAV, FLAC, MP3, OGG/Opus/Vorbis, M4A/MP4/MOV, MKV/WebM, AAC; segment and FLAC output; raw PCM in and `channelsplit` for capture). About 3 MB each. |
| `python/` | CPython 3.13.16 embeddable package, SHA-256 `97dae527…821961297` (as published on python.org) | Its `._pth` adds `Lib\site-packages`. LiteRT and its dependencies are installed from `worker/requirements.windows.lock.txt` with `--require-hashes`; the same pins as Linux. LiteRT's CPU library needs only Windows' own DLLs. |

In the package, the runtime sits next to the app (`resources/engines`, `resources/python`). A checkout stages it in `src-tauri/engines` and `src-tauri/python` for debug builds. At startup the app puts the bundled `engines` folder first on PATH, so the host and every worker use the bundled FFmpeg.

## How the code is split

- `src-tauri/src/process.rs`: starting and ending child processes. Linux: process groups and `PR_SET_PDEATHSIG`, unchanged. Windows: the app contains itself in a kill-on-close Job Object at startup, so a crash or a forced close ends everything it started. Children started with `process::spawn` get a job of their own (started suspended, assigned, resumed, so nothing escapes); `kill_tree` terminates it and `finished` releases it. No console windows.
- `src-tauri/src/platform.rs`: data folders, permissions (Windows AppData is already private to the account), free disk space, CPU features, `.exe` and venv names, bundled tools on PATH, PATH lookup.
- `src-tauri/src/metrics.rs`: process-tree memory from the app's Job Object on Windows.
- `src-tauri/src/audio/pulse.rs` / `audio/wasapi.rs`: device discovery, starting and finishing capture. Everything after the FLAC segments exist is shared.
- Windows capture: FFmpeg cannot open WASAPI devices, so `audio/wasapi.rs` does. A capture thread reads the chosen microphone and, for system audio, the loopback of the chosen output device. Windows converts each to 16-bit mono at 48 kHz. `audio/pcm.rs` places every packet on one clock by its timestamp: jitter of up to 20 ms joins packets without gaps, silence fills every gap (loopback sends nothing while nothing plays), and nothing captured is dropped. Frames are written 300 ms behind the clock, as one channel per source, to a single FFmpeg on stdin. FFmpeg splits them with `channelsplit` into the same `sNNNN-mic-` / `sNNNN-system-` segments and level lines as on Linux. Stop and Pause close FFmpeg's input instead of sending "q", so it finishes the files itself.
- Devices on Windows: a source chosen as the default device follows the default, so plugging in a headset mid-meeting moves the recording to it. A device that disconnects is replaced, checked every second, by the same device or the default. Until then that source records silence and the studio says so. COM runs on the capture thread and on a short-lived thread for listing devices, never on the app's threads.
- Microphone access: Windows gives desktop apps no prompt. The microphone works only while Settings → Privacy & security → Microphone → "Let desktop apps access your microphone" is on. VoxLoom appears in that list after it has used the microphone. When Windows refuses, VoxLoom says which switch to turn on and offers "Open microphone settings" (`ms-settings:privacy-microphone`).
- `worker/engine.py`: `child_options()` for engine processes, explicit UTF-8 for all text files. The host also starts every Python with `PYTHONUTF8=1` on Windows.
- `src-tauri/tauri.windows.conf.json`: Windows-only Tauri settings: the `.ico`, the installer and the runtime it bundles (`python/`, `licenses/`, `VERSIONS.txt` and `LICENSE` beside the engines). `src-tauri/windows/hooks.nsh` adds VoxLoom's clean-up to Tauri's uninstaller; `src-tauri/windows/English.nsh` is Tauri's English text with VoxLoom's wording for deleting recordings. The Linux `tauri.conf.json` is unchanged.
- Bundled Python runs with `PYTHONDONTWRITEBYTECODE=1`: bytecode caches in the program folder would be files the uninstaller never installed.
- Setup on Windows never builds a venv: if the bundled LiteRT is missing, it asks the user to reinstall.
- Deleting a recording that is still open (for example, playing) says so instead of failing with a system error; folders a delete could not remove are removed at the next start.

## Installing and uninstalling

`VoxLoom-<version>-windows-x64-setup.exe` asks whether to install for the current user only (`%LOCALAPPDATA%\VoxLoom`, no administrator rights) or for everyone on the computer (`C:\Program Files\VoxLoom`, needs an administrator). It adds a Start menu shortcut and an entry in Settings → Apps. The package holds the program, its Python with LiteRT, FFmpeg, the engines and the licences of everything bundled (`licenses/`); models are downloaded by first-run setup. Installing a newer version over an older one keeps everything the user made.

Uninstalling (Settings → Apps → VoxLoom → Uninstall, or `uninstall.exe` in the program folder) removes:

- the program folder, including anything the app wrote into it later;
- the Start menu and desktop shortcuts, the Apps entry and the remembered install location;
- in `%LOCALAPPDATA%\org.voxloom.desktop`: the downloaded models, the cache and the window's browser data (`EBWebView`);
- anything VoxLoom kept in `%APPDATA%\org.voxloom.desktop`, and Windows' record that VoxLoom used the microphone.

The user's recordings, transcripts, notes and settings (`library.db`, `recordings\`) stay unless the user ticks "Also delete my recordings, transcripts and settings" on the uninstall page; then the whole `org.voxloom.desktop` folder goes. Recordings cannot be downloaded again, so keeping them is the default.

For IT:

| What | Command |
|---|---|
| Install for everyone, silently | `VoxLoom-<version>-windows-x64-setup.exe /S /AllUsers` (as administrator) |
| Install for the current user, silently | `VoxLoom-<version>-windows-x64-setup.exe /S /CurrentUser` |
| Uninstall silently, keeping recordings | `"<program folder>\uninstall.exe" /S` |
| Uninstall silently, deleting recordings too | `"<program folder>\uninstall.exe" /S /DELETEDATA` |

The draft release also carries `VoxLoom-<version>-windows-x64-programs-SHA256.txt`: every program file (`.exe`, `.dll`, `.pyd`) the installer puts down, with its SHA-256.

Limits of the clean-up:

- Data lives per Windows account. Uninstalling a per-machine install cleans the data of the account that runs the uninstaller; other accounts' `org.voxloom.desktop` folders stay.
- A models folder or tool path the user chose under Advanced settings is theirs and is never deleted.
- The program folder is deleted whole only when it is VoxLoom's own (named `VoxLoom` and holding the app). Installed into a shared folder, only the installed files go.

## Known limits today

- Recording has passed its checks only on machines without audio devices: the timeline, the FFmpeg graph through the bundled FFmpeg, device listing and the refusal of a missing device. Real microphones, loopback, headset switching and the privacy switch need a Windows 11 laptop.
- The installer is not code-signed, so Windows SmartScreen warns on first run (More info → Run anyway), and managed laptops may block it until IT allows it. The programs list on the release helps IT write an allow-list rule by hash.
- The preview zip still exists for running without installing: unzip it and open `voxloom.exe`. Its data is the same library as the installed app's.
- Speeds and accuracy are measured only on GitHub's Windows Server machines so far; a real Windows 11 laptop comes in Phase 5.

## Gotchas to keep in mind

- `python3` on Windows opens the Microsoft Store; always use the bundled interpreter. `preexec_fn` and `resource` don't exist there, and `HOME` is usually unset.
- The embeddable Python takes `sys.path` only from its `._pth` file, which leaves out the script's own folder. A worker script that imports a neighbour (`notes.py`) adds its own folder first; `tests/test_worker.py` runs the scripts with `-P` to catch a new one that doesn't.
- Windows won't delete or rename files another process has open, such as a recording being played.
- Each preview runs from the folder it was unzipped to, so never store a path inside the app's own files. Settings used to save the bundled Python's path; after a newer preview replaced the folder, setup failed with "The system cannot find the path specified". `Service::tool` now ignores a saved path that is gone or is in another copy's `engines` or `python` folder. The running copy's own files and custom paths stay, and saving Settings changes a path only when it was edited.
- WASAPI loopback delivers no packets while nothing is playing. The timeline writes silence so segment offsets match wall-clock time; never write captured audio straight to FFmpeg.
- Headsets switching devices mid-meeting must never leave capture silently recording a dead device. Capture follows the default, replaces a lost device, and shows a notice until it has one.
- The `windows` crate version must stay the one Tauri already uses (0.62 today), so the app carries one copy.
- Managed company laptops may block unsigned programs, including everything the app bundles. Signing (and a publisher allow-list rule) is what keeps IT from re-approving every release.
- Ubuntu 24.04's mingw-w64 11 headers lack a Windows 11 thread-throttling type ggml uses; the runtime build defines it.
