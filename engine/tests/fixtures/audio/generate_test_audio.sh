#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

ffmpeg -y -f lavfi -i "sine=frequency=440:duration=15" -ar 44100 -ac 2 a_sine_440.flac
ffmpeg -y -f lavfi -i "sine=frequency=523:duration=15" -ar 44100 -ac 2 b_sine_523.flac

echo "Generated a_sine_440.flac (A4) and b_sine_523.flac (C5)"
