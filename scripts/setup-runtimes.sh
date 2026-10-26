#!/usr/bin/env bash
# Explicit, one-time dependency provisioning. No models are downloaded here.
# --engines-only builds just the pinned whisper.cpp/llama.cpp/transcribe.cpp CPU executables
# that are bundled into the package (used by scripts/build-app.sh).
set -euo pipefail
umask 077
repo="$(cd "$(dirname "$0")/.." && pwd)"
runtime_dir="${VOXLOOM_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/voxloom}/runtimes"
mkdir -p "$runtime_dir"
if [[ "${1:-}" != "--engines-only" ]]; then
  python3 -m venv "$runtime_dir/inference"
  "$runtime_dir/inference/bin/pip" install -r "$repo/worker/requirements.lock.txt"
fi
build_engine() {
  local name="$1" version="$2" expected="$3" target="$4" upstream="$5"
  local archive="$runtime_dir/$name.tar.gz"
  if [[ ! -f "$archive" ]]; then curl --fail --location --retry 3 "https://github.com/$upstream/archive/refs/tags/$version.tar.gz" -o "$archive"; fi
  printf '%s  %s\n' "$expected" "$archive" | sha256sum --check --status
  mkdir -p "$runtime_dir/$name-src"
  tar -xzf "$archive" -C "$runtime_dir/$name-src" --strip-components=1
  if [[ "$name" == llama ]]; then python3 "$repo/scripts/patch-llama-completion.py" "$runtime_dir/$name-src"; fi
  # transcribe.cpp: no system BLAS (the Windows package bundles none either) and no tests.
  local extra=()
  if [[ "$name" == transcribe ]]; then extra=(-DTRANSCRIBE_BUILD_TESTS=OFF -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF -DTRANSCRIBE_METAL=OFF -DTRANSCRIBE_INSTALL=OFF); fi
  for variant in generic optimized; do
    build_dir="$runtime_dir/$name"
    cpu_flags=(-DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_BMI2=ON -DGGML_AVX512=OFF -DGGML_AVX_VNNI=OFF)
    if [[ "$variant" == generic ]]; then build_dir="$runtime_dir/$name-generic"; cpu_flags=(-DGGML_AVX=OFF -DGGML_AVX2=OFF -DGGML_FMA=OFF -DGGML_F16C=OFF -DGGML_BMI2=OFF); fi
    cmake -S "$runtime_dir/$name-src" -B "$build_dir" -DGGML_NATIVE=OFF "${cpu_flags[@]}" -DGGML_CCACHE=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DWHISPER_BUILD_TESTS=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF "${extra[@]}" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF
    cmake --build "$build_dir" --target "$target" -j "${VOXLOOM_BUILD_JOBS:-$(nproc)}"
  done
}
build_engine whisper v1.9.5 ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720 whisper-cli ggml-org/whisper.cpp
build_engine llama v0.6.0 09b36dba235fcac180efff18514da7038e65d8e5b9e4aefa59238468abfdec12 llama-completion ggml-org/llama.cpp
# Accurate transcription: Parakeet TDT 0.6B v2 (see docs/TESTING.md).
build_engine transcribe v0.4.1 96a3993c075770bf3ca7aeaa86cc55c3673c24d7176643b60cc6a1f46dcab4a5 transcribe-cli handy-computer/transcribe.cpp
printf 'Local runtimes installed in %s\n' "$runtime_dir"
