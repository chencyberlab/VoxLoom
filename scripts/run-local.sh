#!/usr/bin/env bash
# Launch this checkout with its separate, Git-ignored data and model setup.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
export VOXLOOM_DATA_DIR="${VOXLOOM_DATA_DIR:-$repo/.local-data}"
export VOXLOOM_CACHE_DIR="${VOXLOOM_CACHE_DIR:-$VOXLOOM_DATA_DIR/cache}"
if [[ ! -x "$repo/src-tauri/target/debug/voxloom" ]]; then
  printf 'Build the local desktop app first: bash scripts/desktop.sh build --debug --no-bundle\n' >&2
  exit 1
fi
exec "$repo/src-tauri/target/debug/voxloom"
