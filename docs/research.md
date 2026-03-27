# Research notes

This document captures the architectural bet behind the revised scaffold.

## Core assumption

The best product for this repository is not "fastest possible 4K upscaling."
It is "highest credible 4K output quality with explicit safeguards against temporal instability."

## Resulting design principles

### 1) Detail recovery is not enough without temporal discipline
A sharp single frame can still produce a bad video if it flickers, ghosts, or drifts between frames.

### 2) 4K output requires tile- and patch-aware scheduling
High quality at 4K should be treated as a first-class systems problem.

### 3) A second-stage detail refiner belongs in the architecture
The scaffold now makes room for a dedicated refinement pass instead of assuming one monolithic restore stage solves everything.

### 4) Temporal QC should be able to reject output
It is better to rerun unstable windows than to silently ship visible flicker.

### 5) Fast models are support modes, not the north star
Preview backends help iteration, but the reference backend should remain a multi-step, guidance-heavy studio path.

## Suggested implementation order

1. FFmpeg-backed decode / encode and metadata correctness.
2. Patch scheduler and overlap-safe stitcher.
3. Classical baseline for deterministic end-to-end validation.
4. Studio backend runtime contract.
5. Detail refinement stage.
6. Temporal metrics and reject / rerun loop.
7. Teacher-guided experimental backend.
