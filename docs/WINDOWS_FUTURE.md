# Future Windows work

No Windows implementation or packaging is included. Replace the source discovery and `Capture` implementation behind `src-tauri/src/audio.rs` with a WASAPI loopback/microphone backend. Preserve timestamp/track and supervised-worker contracts. Then validate native runtime wheels, filesystem permissions, process-tree teardown, device changes, WebView media behavior, installer dependencies and offline model setup on Windows. Linux Pulse device names must not leak into the portable data model beyond track provenance.
