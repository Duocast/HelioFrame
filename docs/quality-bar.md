# Quality bar

HelioFrame should optimize for these failure modes in descending priority:

1. Temporal instability  
   Visible flicker, drift, or inconsistent reconstruction across adjacent frames.

2. Structural breakage  
   Bent lines, malformed faces, unstable edges, or incorrect object geometry.

3. Texture hallucination artifacts  
   Over-sharpened patterns, ringing, fake grain, or brittle micro-detail.

4. Seam artifacts  
   Tile boundary mismatches or overlap blending failures.

5. Throughput  
   Speed matters only after the output passes the visual bar above.

## Release rule

A faster backend may never replace the studio backend as the default unless it matches the studio backend on temporal stability and structural fidelity in regression tests.

## Benchmark acceptance harness

Use `scripts/benchmark.sh` as the baseline quality gate command.

Quality-review outputs are stored in per-run directories:

- `.helioframe/benchmarks/<run>/runs/` — backend run directories containing CLI manifests and logs.
- `.helioframe/benchmarks/<run>/outputs/` — per-clip backend render outputs.
- `.helioframe/benchmarks/<run>/review/` — side-by-side videos for visual QA.
- `.helioframe/benchmarks/<run>/metrics.json` — machine-readable benchmark metrics.

The clip manifest includes category buckets (`synthetic`, `real-world`, `torture`) and must be identical across compared backends for each benchmark run.
