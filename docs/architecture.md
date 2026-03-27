# Architecture

## Design goals

- Accept common video file types and normalize them into a single processing graph.
- Upscale arbitrary source resolutions to 3840x2160 by default.
- Bias the system toward final-image quality, not just deployment speed.
- Keep Rust responsible for orchestration, scheduling, observability, and failure handling.
- Keep the model runtime behind a trait so the project is not locked to a single framework.
- Leave room for custom CUDA / TensorRT / ONNX integrations without changing the app contract.

## Primary pipeline

1. Probe input
2. Decode frames/audio
3. Normalize colorspace and tensor layout
4. Detect shot boundaries
5. Select anchor frames
6. Build temporal windows
7. Schedule spatial patches / tiles
8. Run main restoration backend
9. Run detail refinement pass
10. Run temporal quality control
11. Stitch and reconstruct full frames
12. Encode video and mux audio

## Backend ladder

### `classical-baseline`
Safe deterministic baseline for integration, smoke tests, and regression comparison.

### `fast-preview`
Distilled preview backend for quick turnarounds and rough look development.

### `seedvr-teacher`
Heavy offline teacher-style backend for reference-quality restoration targets.

### `stcdit-studio`
Default studio backend for quality-first rendering:
- multi-step restoration,
- structural guidance,
- patch-wise 4K scheduling,
- temporal QC gate.

### `helioframe-master`
Experimental flagship backend intended to combine:
- teacher-guided restoration,
- structure-aware diffusion,
- dedicated detail refinement,
- rejection-driven temporal QC,
- patch-wise 4K reconstruction.

## Why this split

The reference path should produce the best output the system can credibly generate. Fast paths are still useful, but they should be explicitly treated as preview or deployment compromises rather than the main design target.
