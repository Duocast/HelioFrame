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
