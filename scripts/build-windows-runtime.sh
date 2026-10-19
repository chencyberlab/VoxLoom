#!/usr/bin/env bash
# Cross-builds, on Linux, everything the Windows package bundles so users install
# nothing else. All sources are pinned by SHA-256; no model is downloaded.
#
#   engines/whisper-cli.exe, whisper-cli-generic.exe        whisper.cpp (pins from setup-runtimes.sh)
#   engines/llama-completion.exe, llama-completion-generic.exe   llama.cpp, patched
#   engines/ffmpeg.exe, ffprobe.exe                          minimal LGPL FFmpeg, only what VoxLoom uses
#   python/                                                  embeddable CPython + LiteRT (requirements.windows.lock.txt)
#
# Executables are static (only Windows' own DLLs) and carry a manifest that
# makes their ANSI code page UTF-8, so paths like C:\Users\Zoë\… reach them intact.
# Needs mingw-w64, cmake, make, nasm, python3-venv, curl.
# Usage: scripts/build-windows-runtime.sh [--parts engines,ffmpeg,python] [output folder]
# (default: all parts into .local-data/windows-runtime). CI builds and caches each part.
set -euo pipefail
umask 022
repo="$(cd "$(dirname "$0")/.." && pwd)"
parts=engines,ffmpeg,python
if [[ "${1:-}" == --parts ]]; then parts="$2"; shift 2; fi
wants() { [[ ",$parts," == *",$1,"* ]]; }
out="$(mkdir -p "${1:-$repo/.local-data/windows-runtime}" && cd "${1:-$repo/.local-data/windows-runtime}" && pwd)"
work="$out/build"
jobs="${VOXLOOM_BUILD_JOBS:-$(nproc)}"
mkdir -p "$work" "$out/engines" "$out/licenses"

ffmpeg_version=7.1.5
ffmpeg_sha256=de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f
python_version=3.13.16
python_sha256=97dae5274cc54867065e8d5a3226e48c35017ed332a0fdb0e27d5b5821961297

fetch() {
  local url="$1" expected="$2" file="$3"
  if [[ ! -f "$file" ]]; then curl --fail --location --retry 3 "$url" -o "$file.part" && mv "$file.part" "$file"; fi
  printf '%s  %s\n' "$expected" "$file" | sha256sum --check --status || { printf 'SHA-256 mismatch: %s\n' "$file" >&2; exit 1; }
}

# The manifest every bundled executable gets.
cat > "$work/voxloom.manifest" <<'EOF'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
</assembly>
EOF
printf '1 24 "voxloom.manifest"\n' > "$work/voxloom.rc"
(cd "$work" && x86_64-w64-mingw32-windres voxloom.rc -O coff -o voxloom-manifest.o)
static_ldflags="-static -static-libgcc -static-libstdc++ $work/voxloom-manifest.o"

# whisper.cpp and llama.cpp: the same pins, patch and CPU variants as Linux.
cat > "$work/mingw.cmake" <<'EOF'
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_C_COMPILER x86_64-w64-mingw32-gcc-posix)
set(CMAKE_CXX_COMPILER x86_64-w64-mingw32-g++-posix)
set(CMAKE_RC_COMPILER x86_64-w64-mingw32-windres)
set(CMAKE_FIND_ROOT_PATH /usr/x86_64-w64-mingw32)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
EOF
# ggml tells Windows 11 not to park its threads on efficiency cores. Ubuntu
# 24.04's mingw-w64 11 headers lack that structure; define it for them.
cat > "$work/mingw-compat.h" <<'HEADER'
#include <_mingw.h>
#if defined(__MINGW64_VERSION_MAJOR) && __MINGW64_VERSION_MAJOR < 12
typedef struct _THREAD_POWER_THROTTLING_STATE {
  unsigned long Version;
  unsigned long ControlMask;
  unsigned long StateMask;
} THREAD_POWER_THROTTLING_STATE;
#define THREAD_POWER_THROTTLING_CURRENT_VERSION 1
#define THREAD_POWER_THROTTLING_EXECUTION_SPEED 0x1
#endif
HEADER
build_engine() {
  local name="$1" version="$2" expected="$3" target="$4" upstream="$5"
  fetch "https://github.com/ggml-org/$upstream/archive/refs/tags/$version.tar.gz" "$expected" "$work/$name-$version.tar.gz"
  rm -rf "$work/$name-src" && mkdir -p "$work/$name-src"
  tar -xzf "$work/$name-$version.tar.gz" -C "$work/$name-src" --strip-components=1
  cp "$work/$name-src/LICENSE" "$out/licenses/$upstream-LICENSE.txt"
  if [[ "$name" == llama ]]; then python3 "$repo/scripts/patch-llama-completion.py" "$work/$name-src"; fi
  for variant in optimized generic; do
    local cpu_flags=(-DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_BMI2=ON -DGGML_AVX512=OFF -DGGML_AVX_VNNI=OFF) suffix=""
    if [[ "$variant" == generic ]]; then cpu_flags=(-DGGML_AVX=OFF -DGGML_AVX2=OFF -DGGML_FMA=OFF -DGGML_F16C=OFF -DGGML_BMI2=OFF); suffix="-generic"; fi
    # No OpenMP: its runtime would be another DLL; ggml's own thread pool is used instead.
    cmake -S "$work/$name-src" -B "$work/$name-$variant" -DCMAKE_TOOLCHAIN_FILE="$work/mingw.cmake" \
      -DGGML_NATIVE=OFF "${cpu_flags[@]}" -DGGML_OPENMP=OFF -DGGML_CCACHE=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF \
      -DWHISPER_BUILD_TESTS=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF \
      -DCMAKE_C_FLAGS="-include $work/mingw-compat.h" -DCMAKE_CXX_FLAGS="-include $work/mingw-compat.h" \
      -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_EXE_LINKER_FLAGS="$static_ldflags" > "$work/$name-$variant.log"
    cmake --build "$work/$name-$variant" --target "$target" -j "$jobs" >> "$work/$name-$variant.log"
    cp "$work/$name-$variant/bin/$target.exe" "$out/engines/$target$suffix.exe"
  done
}
read -r _ _ whisper_version whisper_sha256 _ < <(grep '^build_engine whisper ' "$repo/scripts/setup-runtimes.sh")
read -r _ _ llama_version llama_sha256 _ < <(grep '^build_engine llama ' "$repo/scripts/setup-runtimes.sh")
if wants engines; then
  build_engine whisper "$whisper_version" "$whisper_sha256" whisper-cli whisper.cpp
  build_engine llama "$llama_version" "$llama_sha256" llama-completion llama.cpp
fi

# FFmpeg: LGPL (no --enable-gpl, no external libraries), only the formats import
# accepts and the filters VoxLoom runs. Raw s16le/f32le input is for capture.
if wants ffmpeg; then
fetch "https://ffmpeg.org/releases/ffmpeg-$ffmpeg_version.tar.xz" "$ffmpeg_sha256" "$work/ffmpeg-$ffmpeg_version.tar.xz"
rm -rf "$work/ffmpeg-src" && mkdir -p "$work/ffmpeg-src"
tar -xJf "$work/ffmpeg-$ffmpeg_version.tar.xz" -C "$work/ffmpeg-src" --strip-components=1
cp "$work/ffmpeg-src/COPYING.LGPLv2.1" "$out/licenses/FFmpeg-COPYING.LGPLv2.1.txt"
cp "$work/ffmpeg-src/LICENSE.md" "$out/licenses/FFmpeg-LICENSE.md"
(
  cd "$work/ffmpeg-src"
  ./configure --enable-cross-compile --target-os=mingw32 --arch=x86_64 --cross-prefix=x86_64-w64-mingw32- \
    --prefix="$work/ffmpeg-install" --disable-everything --disable-autodetect --disable-doc --disable-debug \
    --disable-network --disable-ffplay --disable-swscale --enable-ffmpeg --enable-ffprobe --enable-swresample \
    --enable-protocol=file,pipe \
    --enable-demuxer=wav,flac,mp3,ogg,mov,matroska,aac,concat,pcm_s16le,pcm_f32le \
    --enable-muxer=flac,wav,segment,null,pcm_s16le \
    --enable-decoder='flac,pcm_*,adpcm_ms,adpcm_ima_wav,mp3,mp3float,aac,aac_latm,opus,vorbis,alac,ac3,eac3' \
    --enable-encoder=flac,pcm_s16le \
    --enable-parser=aac,aac_latm,ac3,flac,mpegaudio,opus,vorbis \
    --enable-filter=aresample,aformat,anull,atrim,astats,ametadata,apad,highpass,lowpass,sine,channelsplit \
    --enable-indev=lavfi \
    --extra-ldflags="$static_ldflags" > "$work/ffmpeg-configure.log"
  make -j "$jobs" > "$work/ffmpeg-build.log"
  cp ffmpeg.exe ffprobe.exe "$out/engines/"
)
fi

# Python: the embeddable package with LiteRT installed into it.
if wants python; then
fetch "https://www.python.org/ftp/python/$python_version/python-$python_version-embed-amd64.zip" "$python_sha256" "$work/python-$python_version-embed-amd64.zip"
rm -rf "$out/python" && mkdir -p "$out/python"
python3 -m zipfile -e "$work/python-$python_version-embed-amd64.zip" "$out/python"
pth="$(ls "$out"/python/python3*._pth)"
# Embeddable Python ignores site-packages unless its ._pth file lists it.
printf 'python313.zip\n.\nLib\\site-packages\nimport site\n' > "$pth"
rm -rf "$work/pip-venv" && python3 -m venv "$work/pip-venv"
"$work/pip-venv/bin/pip" install --quiet --disable-pip-version-check --no-input --no-deps --require-hashes \
  --only-binary=:all: --platform win_amd64 --python-version 3.13 --implementation cp --abi cp313 \
  --target "$out/python/Lib/site-packages" -r "$repo/worker/requirements.windows.lock.txt"
# pip writes Linux launcher scripts for the CLIs; the app never uses them.
rm -rf "$out/python/Lib/site-packages/bin"
fi

# Everything must run on a clean Windows: only DLLs that ship with Windows.
for file in "$out"/engines/*.exe; do
  [[ -f "$file" ]] || continue
  extra="$(x86_64-w64-mingw32-objdump -p "$file" | sed -n 's/^\s*DLL Name: //p' | grep -viE '^(kernel32|msvcrt|shell32|user32|advapi32|bcrypt|ws2_32|ole32|oleaut32|ntdll|api-ms-win-.*)\.dll$' || true)"
  if [[ -n "$extra" ]]; then printf '%s needs DLLs Windows does not ship: %s\n' "$file" "$extra" >&2; exit 1; fi
done

printf 'whisper.cpp %s\nllama.cpp %s\nFFmpeg %s (LGPL)\nCPython %s embeddable\n' \
  "$whisper_version" "$llama_version" "$ffmpeg_version" "$python_version" > "$out/VERSIONS.txt"
# What a package that bundles the runtime must say about it.
cat > "$out/licenses/README.txt" <<EOF
Third-party software bundled with VoxLoom for Windows

whisper.cpp $whisper_version (MIT): https://github.com/ggml-org/whisper.cpp/tree/$whisper_version
  Licence: whisper.cpp-LICENSE.txt
llama.cpp $llama_version (MIT): https://github.com/ggml-org/llama.cpp/tree/$llama_version
  Patched by scripts/patch-llama-completion.py in VoxLoom's source. Licence: llama.cpp-LICENSE.txt
FFmpeg $ffmpeg_version (LGPL 2.1 or later): https://ffmpeg.org/releases/ffmpeg-$ffmpeg_version.tar.xz
  Built unmodified with the options in scripts/build-windows-runtime.sh in VoxLoom's
  source (https://github.com/chencyberlab/VoxLoom). Licence: FFmpeg-COPYING.LGPLv2.1.txt, FFmpeg-LICENSE.md
CPython $python_version embeddable package (PSF License): https://www.python.org/downloads/release/python-${python_version//./}/
  Licence: python\LICENSE.txt
LiteRT-LM and the Python packages it needs: their licences are in python\Lib\site-packages\*.dist-info
EOF
printf 'Windows runtime in %s\n' "$out"
