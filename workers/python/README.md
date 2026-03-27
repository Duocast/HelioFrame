# HelioFrame Python Worker (HF-014 skeleton)

This directory contains the first process-based Python worker prototype.

## Invocation contract

Rust will launch the worker as a process and pass one argument:

```bash
python3 workers/python/worker.py <input-manifest-path>
```

The worker reads the input manifest JSON, copies each frame from
`input_frames_dir` to `output_frames_dir` unchanged, and emits an output
manifest JSON.

## Schemas

- Input schema: `workers/python/schemas/input_manifest.schema.json`
- Output schema: `workers/python/schemas/output_manifest.schema.json`

## Local smoke test

```bash
python3 -m unittest discover -s workers/python/tests -p "test_worker.py"
```
