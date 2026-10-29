# Footprint report

Measurements are from the actual Ubuntu 24.04 x86_64 development machine, not model-card mobile benchmarks. See TESTING.md for methods and limits.

| Item | Observed size / result |
| --- | --- |
| Source excluding Git, generated outputs, local data, caches, runtimes and models | Under 1 MiB at initial implementation checkpoint; lockfiles included |
| Frontend production bundle | About 300 KiB uncompressed; main JS about 84 KiB gzip |
| Node dependency directory | About 174 MiB |
| Project-local Rust toolchain and Cargo registry | About 1.5 GiB |
| Rust debug build artifacts | About 1.5 GiB (may grow with lint/tests) |
| Rust release build artifacts/cache | About 1.2 GiB; separate from final package |
| Python CPU inference environment | About 159 MiB installed; no Torch/Transformers |
| Standard model weights | 3,474,067,731 bytes including Gemma 4 E2B (1,353,012,147 bytes with Gemma 3 1B before 0.1.8), excluding generated caches |
| Core model weights | 632,586,547 bytes |
| Optional E2B model | 2,841,481,184 bytes. One real summary in the installed app (2.5-minute transcript, two threads): 58.4 s, llama peak RSS 3,970,744 KiB, process-tree peak 4,494,220 KiB. Of that RSS only about 1.15 GiB is private; the rest is the memory-mapped model file, which the system can drop under pressure (0.2.6, 37-minute podcast: 15 passes, 935 s) |
| Optional E4B model | 4,590,807,392 bytes; not installed or run. Estimated from E2B: about 6 GB peak memory and roughly twice the time |
| LiteRT compiled caches for the search model | About 370 MiB in the cache folder; rebuilt on next use; removed with the search model |
| Final Linux `.deb`, including CPU engines, excluding model weights | 14,977,428 bytes (14.28 MiB), version 0.1.3 |

The original host-only `.deb` was about 3.6 MiB. Final packaging includes CPU ASR/summary executables and generic fallbacks for one-click nontechnical setup. Models remain excluded. Static C++ runtime binaries use standard Linux C/C++/OpenMP libraries; the package declares those dependencies. LiteRT support is an explicitly approved onboarding download into a private venv. Package caches and model caches must be counted separately from weights and are never committed. The development models and setup data were removed after validation so manual testing starts with onboarding.

The native idle process tree sampled about 431–439 MiB after replacing the two-webview prototype (about 613 MiB) with one resizable webview. The extracted release sample was 441,820 KiB (431.5 MiB). No AI models are resident at idle. The actual backend fixture process tree sampled around 311 MiB for ASR, 518–542 MiB for indexing and 1,194 MiB for 1B summaries across runs. These backend figures exclude WebKit; full native per-stage RSS on a busy 16 GB desktop still needs collection. The application records host-plus-descendant RSS in its local diagnostic metrics after real jobs. Neither file size nor a sum of separately observed peaks is reported as an actual simultaneous RAM measurement.

Recording has priority over indexing/summaries. The queue is bounded, models do not co-reside across engines, threads default to two, audio embedding windows are bounded to 30 seconds, pause-aligned ASR chunks to 28 seconds plus 10 seconds of noise-floor lookback (about 1.2 MB of 16 kHz mono PCM), LLM context to 4096 tokens and each generation to 768 tokens. Long summaries can invoke multiple model loads for bounded passes. Native Quit and active-model shutdown tests verified worker release. Short E2B correctness fixtures now pass, but no business-quality or full-native-memory performance promise is made for E2B, generic CPU fallback on old hardware, multi-hour recordings or full memory-pressure workloads until measured.

Version 0.1.2 extracted-package native check: sampled idle host/helper process-tree RSS 434,888 KiB (424.7 MiB), no models loaded. Compact/expand and Quit passed; all four observed processes exited. This remains an isolated Xvfb idle measurement, not a full GUI-plus-E2B load benchmark.
