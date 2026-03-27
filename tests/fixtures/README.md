# Test video fixtures

Fixtures are generated on demand from deterministic recipes in
`clip_recipes.json` and stored under `tests/fixtures/clips/`.

Why generated instead of committed binaries:

- keeps repository size small,
- makes fixture generation deterministic and auditable,
- still exercises real `ffprobe`, decode, and encode code paths.

Current recipe set: 6 clips spanning mp4/mov/mkv/avi/m4v with and without audio.
