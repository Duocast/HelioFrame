# Backend ladder

## `classical-baseline`
Purpose: stable baseline, deterministic regression target, low integration risk.

## `fast-preview`
Purpose: quick look generation and operator feedback loop.  
Non-goal: final delivery master.

## `seedvr-teacher`
Purpose: heavy offline reference generation or teacher outputs for distillation and comparison.

## `stcdit-studio`
Purpose: default final-output backend.  
Requirements:
- patch-wise 4K support,
- structural guidance,
- temporal QC,
- detail refinement compatibility.

## `helioframe-master`
Purpose: research flagship path that can exceed the studio path, but only if it passes the same visual and temporal gates.

Architecture: orchestration backend that chains existing backends in sequence:
1. **Teacher guidance** (`seedvr-teacher`) — conservative reference-grade restoration produces anchor frames that stabilize later stages.
2. **Studio diffusion** (`stcdit-studio`) — anchor-frame-aware segment-wise diffusion runs on the teacher output, using 24 diffusion steps with structural guidance.
3. **Detail refinement** (`detail-refiner`) — selective HF enhancement on text, hair, fabric, foliage, and architecture patches, with per-window sparkle guard rollback.
4. **QC rerun** — temporal quality gate evaluates flicker and shimmer per window. Failed windows are re-run through studio (without refiner) on the first attempt; a second failure falls back to teacher output.

Requirements:
- All three model checkpoints must be present locally (seedvr-teacher, stcdit-studio, detail-refiner).
- Only available via the `experimental` preset.
- Must beat studio-path output on the internal benchmark before promotion.
