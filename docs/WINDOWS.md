# VoxLoom on Windows

Windows 11 x64 is a second platform of the same codebase. Linux stays fully supported and its behaviour must not change. Windows on Arm, Windows 10 and macOS are out of scope for now.

## What a Windows user gets

One installer with everything the app needs: VoxLoom, its own Python with LiteRT, FFmpeg, and the speech and summary engines. Nothing else to install: no Python, no FFmpeg, no Visual C++ runtime prompts, and WebView2 is part of Windows 11. Only the models are downloaded, through the existing first-run setup (or imported offline), because they are several GB and never ship in the package.

Library: `%LOCALAPPDATA%\VoxLoom`, cache: `%LOCALAPPDATA%\VoxLoom\cache` (Local, not Roaming).

## Status

| Phase | What | State |
|---|---|---|
| 1 | Platform seams; the app compiles, launches and passes the fast tests on Windows | Done; see CI |
| 2 | Engines built for Windows; bundled Python + LiteRT and FFmpeg (pinned, SHA-256); Job Objects so a crashed host leaves nothing running; import → transcribe → search → summary on Windows | Next |
| 3 | Live capture with WASAPI: microphones and loopback of the default output, same FLAC segments as Linux | |
| 4 | NSIS installer (per-user and per-machine), models-only first-run setup, third-party notices, list of every bundled executable with SHA-256 for IT allow-listing | |
| 5 | Measurements on a real Windows 11 laptop, transcription quality checks, docs | |

Until Phase 3, Windows builds report that recording isn't available yet; everything else in the UI loads.

## How the code is split

- `src-tauri/src/process.rs`: starting and killing child processes. Linux: process groups and `PR_SET_PDEATHSIG`. Windows: no console windows; `kill_tree` uses `taskkill /T` until Job Objects land in Phase 2.
- `src-tauri/src/platform.rs`: data folders, permissions (Windows AppData is already private to the account), free disk space, CPU features, `.exe` and venv names, PATH lookup.
- `src-tauri/src/audio/pulse.rs` / `audio/wasapi.rs`: device discovery and starting capture. Everything after the FLAC segments exist is shared.
- `worker/engine.py`: `child_options()` for engine processes, explicit UTF-8 for all text files. The host also starts every Python with `PYTHONUTF8=1` on Windows.
- `src-tauri/tauri.windows.conf.json`: Windows-only Tauri settings (the `.ico` today; the installer later). The Linux `tauri.conf.json` is unchanged.

## Known limits today

- No recording on Windows (Phase 3).
- Engines, Python and FFmpeg are not bundled yet (Phase 2), so a Windows build can't transcribe yet.
- If the host process is killed, its children are not cleaned up on Windows until Job Objects land (Phase 2).
- Process-tree memory metrics read `/proc` and report 0 on Windows (Phase 2, from the Job Object).
- The WebView2 profile lives in `%LOCALAPPDATA%\org.voxloom.desktop`, next to the library folder. The uninstaller (Phase 4) must handle both.

## Gotchas to keep in mind

- `python3` on Windows opens the Microsoft Store; always use the bundled interpreter. `preexec_fn` and `resource` don't exist there, and `HOME` is usually unset.
- Windows won't delete or rename files another process has open, such as a recording being played.
- WASAPI loopback delivers no packets while nothing is playing: Phase 3 must write silence so segment offsets match wall-clock time.
- Headsets switching devices mid-meeting must never leave capture silently recording a dead device.
- Managed company laptops may block unsigned programs, including everything the app bundles. Signing (and a publisher allow-list rule) is what keeps IT from re-approving every release.
