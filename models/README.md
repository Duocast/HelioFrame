# Models directory

Place exported model artifacts here when they exist.

Suggested layout:

- `models/classical-baseline/`
- `models/fast-preview/`
- `models/seedvr-teacher/`
- `models/stcdit-studio/`
- `models/helioframe-master/`

Keep large binaries out of git.

## Worker protocol notes

HF-014 introduces a process-based Python worker skeleton under
`workers/python/`. Worker input/output manifests and frame directories are
specified in `docs/worker-protocol.md` with JSON schemas stored in
`workers/python/schemas/`.
