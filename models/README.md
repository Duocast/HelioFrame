# Models directory

Place exported model artifacts here when they exist.

Suggested layout:

- `models/classical-baseline/`
- `models/fast-preview/`
- `models/seedvr-teacher/`
- `models/stcdit-studio/`
- `models/realbasicvsr/`
- `models/helioframe-master/`

Keep large binaries out of git.

## SeedVR teacher backend (HF-017)

`seedvr-teacher` is intentionally heavy and quality-first. It is configured for
strict offline execution and pinned model identity checks.

Expected artifact path:

- `models/seedvr-teacher/seedvr_teacher_v1.0.0.ts` (TorchScript checkpoint)

Pinned identity:

- `model_version`: `seedvr-teacher-v1.0.0`
- `weights_sha256`: `c2c0d9ec5b0c8c1f8b03419e7f3e462f8ab7604f53f6ed15ec5122f7e14b8079`

Worker manifest snippet:

```json
{
  "backend": "seedvr-teacher",
  "backend_options": {
    "model_path": "models/seedvr-teacher/seedvr_teacher_v1.0.0.ts",
    "model_version": "seedvr-teacher-v1.0.0",
    "weights_sha256": "c2c0d9ec5b0c8c1f8b03419e7f3e462f8ab7604f53f6ed15ec5122f7e14b8079",
    "offline_only": true,
    "device": "cuda",
    "window_size": 12,
    "overlap": 4,
    "precision": "fp32"
  }
}
```

The worker fails fast if any pin changes, if the model file hash does not
match, or if `offline_only` is disabled.

## RealBasicVSR bridge backend (HF-016)

`realbasicvsr-bridge` is the first practical Python bridge backend and is
intentionally separate from `stcdit-studio`.

Expected artifact path:

- `models/realbasicvsr/realbasicvsr_x4.ts` (TorchScript checkpoint)

Worker manifest snippet:

```json
{
  "backend": "realbasicvsr-bridge",
  "backend_options": {
    "model_path": "models/realbasicvsr/realbasicvsr_x4.ts",
    "device": "cuda",
    "window_size": 6,
    "overlap": 2,
    "precision": "fp16"
  }
}
```

The backend performs temporal-window inference and writes restored output
frames to `output_frames_dir`.

## Worker protocol notes

HF-014 introduces a process-based Python worker skeleton under
`workers/python/`. Worker input/output manifests and frame directories are
specified in `docs/worker-protocol.md` with JSON schemas stored in
`workers/python/schemas/`.
