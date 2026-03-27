# Worker Protocol (process-based v1)

HF-014 defines a file/manifest protocol between Rust orchestration and Python
workers. The first version is intentionally process-based and does not use
sockets.

## Launch model

Rust starts a worker process with a single positional argument:

```bash
python3 workers/python/worker.py <input-manifest-path>
```

- `input-manifest-path` points to a JSON file matching
  `workers/python/schemas/input_manifest.schema.json`.
- The worker writes an output manifest matching
  `workers/python/schemas/output_manifest.schema.json`.

## Input manifest

Required fields:

- `schema_version`: currently `1.0.0`
- `run_id`: run identifier from HelioFrame
- `clip_id`: clip/window identifier for worker execution
- `input_frames_dir`: source frame directory
- `output_frames_dir`: destination frame directory
- `frames`: ordered list of frame descriptors (`index`, `file_name`)

Optional field:

- `output_manifest_path`: explicit destination for worker result JSON

## Output manifest

Required fields:

- `schema_version`: currently `1.0.0`
- `run_id`
- `clip_id`
- `status`: `ok` or `error`
- `mode`: `passthrough` for HF-014
- `input_manifest_schema_version`
- `output_frames_dir`
- `frame_count`
- `frames`: frame results containing checksums for source/output parity

Optional:

- `error`: populated when `status=error`

## Frame directory contract

For HF-014, worker behavior is pass-through:

1. Read each declared frame from `input_frames_dir/<file_name>`.
2. Write frame bytes unchanged to `output_frames_dir/<file_name>`.
3. Preserve filename and index ordering from the input manifest.
4. Return per-frame checksums to prove unchanged output.

This contract keeps IPC simple and deterministic while Rust remains responsible
for decode/encode and run artifact layout.
