# Test video fixtures

Fixtures are generated on demand from deterministic recipes in
`clip_recipes.json` and stored under `tests/fixtures/clips/`.

Why generated instead of committed binaries:

- keeps repository size small,
- makes fixture generation deterministic and auditable,
- still exercises real `ffprobe`, decode, and encode code paths.

Current recipe set: 6 clips spanning mp4/mov/mkv/avi/m4v with and without audio.

## Benchmark clip set manifest

`benchmark_clips.json` defines benchmark harness categories:

- `synthetic`
- `real-world`
- `torture`

Each category is an array of clip entries:

```json
{
  "name": "optional-friendly-name",
  "path": "tests/fixtures/clips/example.mp4"
}
```

The benchmark script (`scripts/benchmark.sh`) auto-generates any missing
synthetic fixture listed in `clip_recipes.json`, then runs all configured
backends on the same manifest-defined clip set.
