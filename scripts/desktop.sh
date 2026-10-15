#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
if ! command -v cargo >/dev/null && [[ -x "$repo/.toolchain/cargo/bin/cargo" ]]; then
  export RUSTUP_HOME="$repo/.toolchain/rustup"
  export CARGO_HOME="$repo/.toolchain/cargo"
  export PATH="$repo/.toolchain/cargo/bin:$PATH"
fi
cd "$repo"
if [[ "${1:-}" == "build" ]]; then
  mkdir -p "$repo/src-tauri/engines"
  runtime_dir="${VOXLOOM_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/voxloom}/runtimes"
  for engine in whisper llama; do
    if [[ "$engine" == whisper ]]; then executable=whisper-cli; else executable=llama-completion; fi
    source="$repo/.runtimes/$engine-build/bin/$executable"
    if [[ ! -f "$source" ]]; then source="$runtime_dir/$engine/bin/$executable"; fi
    if [[ ! -f "$source" ]]; then printf 'Build pinned CPU inference engines with scripts/setup-runtimes.sh before packaging.\n' >&2; exit 1; fi
    cp "$source" "$repo/src-tauri/engines/$executable"
    generic="$repo/.runtimes/generic/$executable"
    if [[ ! -f "$generic" ]]; then generic="$runtime_dir/$engine-generic/bin/$executable"; fi
    if [[ -f "$generic" ]]; then cp "$generic" "$repo/src-tauri/engines/$executable-generic"; fi
  done
fi
exec npm exec -- tauri "$@"
