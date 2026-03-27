# Roadmap

## Phase 1
- Real FFmpeg probing and transcoding
- File validation
- Preset loading
- End-to-end no-op pipeline smoke test

## Phase 2
- Patch scheduler
- Seam-aware overlap blending
- Shot detection
- Anchor-frame selection
- Temporal-window batching

## Phase 3
- Classical baseline integration
- Studio backend runtime abstraction
- Temporal QC metrics
- Regression fixtures for flicker and ghosting

## Phase 4
- Detail refinement stage
- Patch-wise 4K stitcher
- Reject / rerun flow for unstable windows
- Model artifact registry and weight loading

## Phase 5
- Teacher-guided experimental backend
- TensorRT / CUDA FFI runtime
- ONNX Runtime fallback
- Benchmark suite and visual QA dashboard
