# HelioFrame

A Rust-first research scaffold for a quality-first 4K video upscaling application.

## Positioning

This scaffold is intentionally biased toward **end-product quality**:
- perceptual richness,
- temporal stability,
- structural fidelity,
- 4K-native reconstruction behavior.

Throughput is treated as a secondary operating mode. Fast backends still exist, but they are not the reference path.

## What changed in this revision

The repository now assumes a **backend ladder** with a clear quality hierarchy:

1. `classical-baseline`  
   Deterministic restoration baseline for integration safety and regression checks.

2. `fast-preview`  
   Distilled / one-step preview backend for quick iteration and approximate output review.

3. `seedvr-teacher`  
   Heavy teacher-style restoration backend intended for reference-grade offline generation.

4. `stcdit-studio`  
   Default studio backend. Quality-first, structure-guided, patch-wise 4K capable, and strict about temporal coherence.

5. `HelioFrame-master`  
   Experimental flagship backend combining teacher guidance, patch-wise 4K synthesis, detail refinement, and temporal QC gating.

## Quality-first pipeline goals

The primary pipeline is designed around:
- shot-aware segmentation,
- motion / structural guidance,
- patch-wise 4K scheduling,
- heavy restoration pass,
- dedicated detail refinement,
- temporal regression checks before final encode.

## Workspace layout

```text
HelioFrame-vsr/
├── Cargo.toml
├── README.md
├── configs/
│   └── presets/
├── crates/
│   ├── HelioFrame-cli/
│   ├── HelioFrame-core/
│   ├── HelioFrame-model/
│   ├── HelioFrame-pipeline/
│   └── HelioFrame-video/
├── docs/
├── examples/
├── models/
├── scripts/
└── tests/
```

## Quick start

Default run: quality-first studio path.

```bash
cargo run -p HelioFrame-cli -- \
  upscale input.mp4 \
  --output output_4k.mp4
```

Explicit studio run:

```bash
cargo run -p HelioFrame-cli -- \
  upscale input.mp4 \
  --output output_4k.mp4 \
  --preset studio \
  --backend stcdit-studio
```

Experimental maximum-quality run:

```bash
cargo run -p HelioFrame-cli -- \
  upscale input.mp4 \
  --output output_4k.mp4 \
  --preset experimental \
  --backend HelioFrame-master
```

## Current status

- [x] Multi-crate Rust workspace
- [x] Quality-first backend ladder
- [x] Preset configs oriented around preview / balanced / studio / experimental
- [x] Pipeline stages updated for detail refinement and temporal QC
- [x] Docs updated to reflect studio-quality default behavior
- [ ] Real FFmpeg decode / encode
- [ ] Real model inference
- [ ] Patch-wise 4K stitcher implementation
- [ ] Detail refinement runtime
- [ ] Temporal regression metrics and output rejection loop
- [ ] CUDA / TensorRT / ONNX backend bindings

## Notes

This is still a scaffold. It does not claim universal SOTA performance without training, validation, and benchmark reproduction. What it does do is place the architecture on the right side of the tradeoff you specified: **quality first**.
