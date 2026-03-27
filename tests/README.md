# Tests

Regression assets in this folder are focused on deterministic I/O validation.

## What is covered

- Tiny generated fixture clips (`tests/fixtures/clip_recipes.json`) for fast CI.
- Probe metadata checks (container, resolution, fps, audio, duration).
- Decode/encode round-trip checks with output existence assertions.
- Baseline metadata checks (`tests/integration/baseline_expectations.json`) before
  any visual quality analysis.

## Executable test entrypoint

Run:

```bash
cargo test -p helioframe-video --test io_regression
```

The test harness lives in `crates/helioframe-video/tests/io_regression.rs`.
