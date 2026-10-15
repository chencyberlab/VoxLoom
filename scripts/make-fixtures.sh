#!/usr/bin/env bash
set -euo pipefail
umask 077
fixture_dir="${1:-/tmp}"
mkdir -p "$fixture_dir"
espeak-ng -s 145 -w "$fixture_dir/voxloom-budget.wav" 'The project budget is approved. Sarah will send the final invoice on Friday.'
espeak-ng -s 145 -w "$fixture_dir/voxloom-garden.wav" 'The garden has red roses. Water the flowers every morning.'
ffmpeg -nostdin -v error -y -stream_loop 5 -i "$fixture_dir/voxloom-budget.wav" -t 42 -ar 16000 -ac 1 "$fixture_dir/voxloom-long.wav"
