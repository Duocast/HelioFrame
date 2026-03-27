# Run Manifest Integration Expectations

Each CLI `upscale` execution should create:

- `.helioframe/runs/<run-id>/manifest.json`
- `.helioframe/runs/<run-id>/artifacts/input/`
- `.helioframe/runs/<run-id>/artifacts/intermediate/`
- `.helioframe/runs/<run-id>/artifacts/output/`

The `manifest.json` is updated incrementally as each pipeline stage completes.
