#!/usr/bin/env bash
# One command to build the installable VoxLoom Linux package from this checkout.
#
#   bash scripts/build-app.sh                 # check everything, build, copy to release/
#   bash scripts/build-app.sh --install-deps  # also install missing Ubuntu packages (sudo)
#   bash scripts/build-app.sh --check         # also run the automated tests first
#
# Node and Rust are installed privately into .toolchain/ when missing (no sudo).
# The pinned whisper.cpp/llama.cpp/transcribe.cpp engines are compiled into .local-data/runtimes/
# once. AI models are never downloaded or bundled here: the app's first-run
# "Download & set up" handles them.
set -euo pipefail
umask 022
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"

NODE_VERSION=24.22.0
NODE_SHA256=cc2f601cee82d9a5a233302fe307abc600727035fd7d2c3a3571755989e95008
RUST_VERSION=1.99.0
PACKAGES=(build-essential cmake pkg-config curl libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev
          libayatana-appindicator3-dev patchelf ffmpeg pulseaudio-utils python3 python3-venv)

install_deps=0 run_checks=0 release_dir="$repo/release"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-deps) install_deps=1 ;;
    --check) run_checks=1 ;;
    --release-dir) release_dir="$(realpath -m "$2")"; shift ;;
    -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'Unknown option: %s (try --help)\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
fail() { printf '\n\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }
version_at_least() { [[ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" == "$2" ]]; }

step "Checking this computer"
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || fail "VoxLoom builds on x86_64 Linux only."
command -v dpkg-query >/dev/null || fail "An Ubuntu/Debian system (apt) is required."
missing=()
for package in "${PACKAGES[@]}"; do
  dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q "install ok installed" || missing+=("$package")
done
if ((${#missing[@]})); then
  if ((install_deps)); then
    step "Installing system packages (asks for your password): ${missing[*]}"
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends "${missing[@]}"
  else
    fail "Missing system packages: ${missing[*]}
Install them with:
  sudo apt-get install -y --no-install-recommends ${missing[*]}
or rerun: bash scripts/build-app.sh --install-deps"
  fi
fi
echo "System packages: OK"

# Node.js 22+ (system install, else a private pinned Node 24 in .toolchain/node).
if [[ -x "$repo/.toolchain/node/bin/node" ]]; then export PATH="$repo/.toolchain/node/bin:$PATH"; fi
if ! command -v node >/dev/null || ! version_at_least "$(node --version | tr -d v)" 22.0.0; then
  step "Installing Node.js $NODE_VERSION privately into .toolchain/node"
  archive="$(mktemp -d)/node.tar.xz"
  curl --fail --location --retry 3 -o "$archive" "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.xz"
  printf '%s  %s\n' "$NODE_SHA256" "$archive" | sha256sum --check --status || fail "Node.js download failed its checksum."
  rm -rf "$repo/.toolchain/node" && mkdir -p "$repo/.toolchain/node"
  tar -xJf "$archive" -C "$repo/.toolchain/node" --strip-components=1
  rm -f "$archive"
  export PATH="$repo/.toolchain/node/bin:$PATH"
fi
echo "Node.js: $(node --version)"

# Rust 1.85+ (system install, else a private pinned copy in .toolchain/).
if [[ -x "$repo/.toolchain/cargo/bin/cargo" ]]; then
  export RUSTUP_HOME="$repo/.toolchain/rustup" CARGO_HOME="$repo/.toolchain/cargo"
  export PATH="$repo/.toolchain/cargo/bin:$PATH"
fi
if ! command -v cargo >/dev/null || ! version_at_least "$(rustc --version | awk '{print $2}')" 1.85.0; then
  step "Installing Rust $RUST_VERSION privately into .toolchain/ (does not change your shell)"
  export RUSTUP_HOME="$repo/.toolchain/rustup" CARGO_HOME="$repo/.toolchain/cargo"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION"
  export PATH="$repo/.toolchain/cargo/bin:$PATH"
fi
echo "Rust: $(rustc --version)"

step "Installing JavaScript dependencies"
if [[ ! -f node_modules/.package-lock.json || package-lock.json -nt node_modules/.package-lock.json ]]; then
  npm ci
else
  echo "Already up to date."
fi

# Packaged engines come from VOXLOOM_DATA_DIR/runtimes (see scripts/desktop.sh).
export VOXLOOM_DATA_DIR="${VOXLOOM_DATA_DIR:-$repo/.local-data}"
runtimes="$VOXLOOM_DATA_DIR/runtimes"
step "Preparing the speech and summary engines"
if [[ -f "$runtimes/whisper/bin/whisper-cli" && -f "$runtimes/llama/bin/llama-completion" &&
      -f "$runtimes/whisper-generic/bin/whisper-cli" && -f "$runtimes/llama-generic/bin/llama-completion" &&
      -f "$runtimes/transcribe/bin/transcribe-cli" && -f "$runtimes/transcribe-generic/bin/transcribe-cli" ]]; then
  echo "Already built."
else
  echo "Compiling pinned whisper.cpp, llama.cpp and transcribe.cpp (first time only; about 10-20 minutes)."
  bash scripts/setup-runtimes.sh --engines-only
fi

if ((run_checks)); then
  step "Running automated checks"
  npm run check
  python3 -m unittest discover -s tests
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --offline
  cargo clippy --manifest-path src-tauri/Cargo.toml --offline -- -D warnings
fi

python3 scripts/version.py >/dev/null || { python3 scripts/version.py; fail "Version copies differ from VERSION"; }
version="$(tr -d '[:space:]' < VERSION)"
step "Building VoxLoom $version"
bash scripts/desktop.sh build --bundles deb
deb="src-tauri/target/release/bundle/deb/voxloom_${version}_amd64.deb"
[[ -f "$deb" ]] || fail "The package was not produced: $deb"

step "Copying the result to $release_dir"
mkdir -p "$release_dir"
cp "$deb" "$release_dir/"
portable="$release_dir/VoxLoom-$version"
rm -rf "$portable" && mkdir -p "$portable"
dpkg-deb -x "$deb" "$portable"
ln -s usr/bin/voxloom "$portable/voxloom"
cat > "$portable/START HERE.txt" <<EOF
VoxLoom $version — Linux preview

Open voxloom in this folder; keep usr beside it.
The .deb beside this folder installs VoxLoom in the applications menu.
Your existing recordings and models are kept.
EOF

printf '\n\033[32mDone.\033[0m\n'
printf '  Install:   sudo apt install "%s"\n' "$release_dir/voxloom_${version}_amd64.deb"
printf '  Or run without installing: "%s"\n' "$portable/voxloom"
printf '  SHA-256:   %s\n' "$(sha256sum "$release_dir/voxloom_${version}_amd64.deb" | cut -d' ' -f1)"
