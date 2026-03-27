#!/usr/bin/env bash
set -euo pipefail

cargo run -p helioframe-cli -- upscale sample.mp4 --output sample_4k.mp4 --preset studio --backend stcdit-studio
