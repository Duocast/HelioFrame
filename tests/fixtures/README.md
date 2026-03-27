# Test video fixtures

This directory stores (or is used to generate) sample media for probe tests.

- `probe-sample-with-audio.mp4` is generated on demand by
  `crates/helioframe-video/src/probe.rs` tests using `ffmpeg`.
- The file is intentionally generated in test setup so we avoid committing large
  binary artifacts while still validating real `ffprobe` JSON metadata parsing.
