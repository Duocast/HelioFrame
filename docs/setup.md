# Setup and environment validation

HelioFrame currently targets **Linux/NVIDIA/SDR only**.

## Dependencies

Install the following tools before running pipelines:

- `ffmpeg`
- `ffprobe`
- `python3`
- NVIDIA GPU + driver stack (`nvidia-smi` must work)

You can bootstrap FFmpeg with:

```bash
./scripts/fetch_ffmpeg.sh
```

## Doctor command

Run:

```bash
helioframe doctor
```

The doctor command validates runtime prerequisites and fails hard when any dependency is missing. It checks:

- `ffmpeg` on `PATH`
- `ffprobe` on `PATH`
- `python3` on `PATH`
- NVIDIA GPU visibility (`nvidia-smi -L`)
- writable temp directory
- writable run directory (`$HELIOFRAME_RUN_DIR`, then `$XDG_RUNTIME_DIR/helioframe`, else `/run/helioframe`)

Output includes both a human-readable pass/fail report and a JSON summary suitable for automation.
