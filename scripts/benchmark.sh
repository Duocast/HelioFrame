#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

command -v ffmpeg >/dev/null 2>&1 || { echo "error: ffmpeg is required" >&2; exit 1; }
command -v ffprobe >/dev/null 2>&1 || { echo "error: ffprobe is required" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "error: python3 is required" >&2; exit 1; }

CLIP_MANIFEST="tests/fixtures/benchmark_clips.json"
PRESET="studio"
BACKENDS_CSV="stcdit-studio,classical-baseline"
OUT_ROOT=".helioframe/benchmarks"
TAG=""
TARGET_WIDTH=""
TARGET_HEIGHT=""

usage() {
  cat <<USAGE
Usage: scripts/benchmark.sh [options]

Runs the same clip set through multiple backends, writes run artifacts,
produces side-by-side review clips, and emits benchmark metrics JSON.

Options:
  --clips <path>          Clip manifest JSON (default: ${CLIP_MANIFEST})
  --preset <name>         CLI preset to use (default: ${PRESET})
  --backends <csv>        Comma-separated backend names (default: ${BACKENDS_CSV})
  --out-root <path>       Benchmark output root (default: ${OUT_ROOT})
  --tag <name>            Optional run tag added to run directory name
  --target-width <px>     Output width passed to CLI upscale
  --target-height <px>    Output height passed to CLI upscale
  -h, --help              Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --clips)
      CLIP_MANIFEST="$2"; shift 2 ;;
    --preset)
      PRESET="$2"; shift 2 ;;
    --backends)
      BACKENDS_CSV="$2"; shift 2 ;;
    --out-root)
      OUT_ROOT="$2"; shift 2 ;;
    --tag)
      TAG="$2"; shift 2 ;;
    --target-width)
      TARGET_WIDTH="$2"; shift 2 ;;
    --target-height)
      TARGET_HEIGHT="$2"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

if [[ ! -f "$CLIP_MANIFEST" ]]; then
  echo "error: clip manifest not found: $CLIP_MANIFEST" >&2
  exit 1
fi

IFS=',' read -r -a BACKENDS <<< "$BACKENDS_CSV"
if [[ ${#BACKENDS[@]} -lt 2 ]]; then
  echo "error: provide at least two backends" >&2
  exit 1
fi

RUN_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_NAME="bench-${RUN_STAMP}"
if [[ -n "$TAG" ]]; then
  RUN_NAME+="-${TAG}"
fi
RUN_DIR="${OUT_ROOT}/${RUN_NAME}"
mkdir -p "$RUN_DIR" "$RUN_DIR/logs" "$RUN_DIR/runs" "$RUN_DIR/outputs" "$RUN_DIR/review"

echo "benchmark run dir: $RUN_DIR"

python3 - "$CLIP_MANIFEST" <<'PY'
import json
import pathlib
import subprocess
import sys

manifest = pathlib.Path(sys.argv[1])
root = pathlib.Path.cwd()
fixtures_dir = root / "tests" / "fixtures" / "clips"
fixtures_dir.mkdir(parents=True, exist_ok=True)
recipes = json.loads((root / "tests" / "fixtures" / "clip_recipes.json").read_text())
by_file = {r["file"]: r for r in recipes}

clip_cfg = json.loads(manifest.read_text())
all_files = []
for category in ("synthetic", "real-world", "torture"):
    for entry in clip_cfg.get(category, []):
        if isinstance(entry, str):
            all_files.append(entry)
        else:
            all_files.append(entry.get("path", ""))

for rel_path in sorted(set(filter(None, all_files))):
    if "/" in rel_path or "\\" in rel_path:
        continue
    out = fixtures_dir / rel_path
    if out.exists():
        continue
    recipe = by_file.get(rel_path)
    if recipe is None:
        continue

    video_input = f'{recipe["video_filter"]}=size={recipe["width"]}x{recipe["height"]}:rate={recipe["fps"]}:duration={recipe["duration_seconds"]}'
    cmd = [
        "ffmpeg", "-y", "-v", "error",
        "-f", "lavfi", "-i", video_input,
        "-pix_fmt", "yuv420p", "-threads", "1",
        "-fflags", "+bitexact", "-flags:v", "+bitexact",
        "-c:v", "libx264",
    ]
    if recipe["has_audio"]:
        audio_input = f'sine=frequency=440:sample_rate=48000:duration={recipe["duration_seconds"]}'
        cmd += ["-f", "lavfi", "-i", audio_input, "-shortest", "-c:a", "aac", "-b:a", "96k"]
    else:
        cmd += ["-an"]
    cmd += [str(out)]
    subprocess.run(cmd, check=True)
    print(f"generated fixture: {out}")
PY

benchmark_now() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

python3 - "$CLIP_MANIFEST" "$ROOT_DIR" > "$RUN_DIR/clip-plan.tsv" <<'PY'
import json
import pathlib
import sys

manifest = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
raw = json.loads(manifest.read_text())
for category in ("synthetic", "real-world", "torture"):
    entries = raw.get(category, [])
    if not isinstance(entries, list):
        continue
    for item in entries:
        if isinstance(item, str):
            rel = item
            name = pathlib.Path(item).stem
        else:
            rel = item.get("path", "")
            name = item.get("name") or pathlib.Path(rel).stem
        if not rel:
            continue
        path = pathlib.Path(rel)
        if not path.is_absolute():
            path = root / rel
        print(f"{category}\t{name}\t{path}")
PY

if [[ ! -s "$RUN_DIR/clip-plan.tsv" ]]; then
  echo "error: no clips found in clip manifest: $CLIP_MANIFEST" >&2
  exit 1
fi

echo "started: $(benchmark_now)" | tee "$RUN_DIR/benchmark.log"

while IFS=$'\t' read -r category clip_name clip_path; do
  if [[ ! -f "$clip_path" ]]; then
    echo "warning: missing clip ($category/$clip_name): $clip_path" | tee -a "$RUN_DIR/benchmark.log"
    continue
  fi

  clip_slug="${category}__${clip_name}"
  clip_dir="$RUN_DIR/outputs/$clip_slug"
  mkdir -p "$clip_dir"

  for backend in "${BACKENDS[@]}"; do
    out_path="$clip_dir/${backend}.mp4"
    run_dir="$RUN_DIR/runs/$clip_slug/$backend"
    mkdir -p "$run_dir"

    cmd=(cargo run -p helioframe-cli -- upscale "$clip_path" --output "$out_path" --preset "$PRESET" --backend "$backend")
    if [[ -n "$TARGET_WIDTH" ]]; then
      cmd+=(--target-width "$TARGET_WIDTH")
    fi
    if [[ -n "$TARGET_HEIGHT" ]]; then
      cmd+=(--target-height "$TARGET_HEIGHT")
    fi

    echo "[$(benchmark_now)] ${category}/${clip_name} :: ${backend}" | tee -a "$RUN_DIR/benchmark.log"
    (
      cd "$run_dir"
      "${cmd[@]}"
    ) > "$RUN_DIR/logs/${clip_slug}__${backend}.log" 2>&1
  done

  inputs=()
  for backend in "${BACKENDS[@]}"; do
    inputs+=( -i "$clip_dir/${backend}.mp4" )
  done

  filter=""
  idx=0
  for backend in "${BACKENDS[@]}"; do
    filter+="[${idx}:v]setpts=PTS-STARTPTS,scale=-2:540,drawtext=text='${backend}':x=10:y=10:fontcolor=white:fontsize=24:box=1:boxcolor=0x000000AA[v${idx}];"
    idx=$((idx + 1))
  done
  stack_inputs=""
  for ((i=0; i<idx; i++)); do
    stack_inputs+="[v${i}]"
  done
  filter+="${stack_inputs}hstack=inputs=${idx}[vout]"

  ffmpeg -y -v error "${inputs[@]}" -filter_complex "$filter" -map "[vout]" -an "$RUN_DIR/review/${clip_slug}__side_by_side.mp4"
done < "$RUN_DIR/clip-plan.tsv"

python3 - "$RUN_DIR" "$BACKENDS_CSV" <<'PY'
import json
import pathlib
import subprocess
import sys

run_dir = pathlib.Path(sys.argv[1])
backends = [b.strip() for b in sys.argv[2].split(",") if b.strip()]
outputs_root = run_dir / "outputs"
review_root = run_dir / "review"


def ffprobe_json(path: pathlib.Path) -> dict:
    cmd = [
        "ffprobe", "-v", "error",
        "-show_streams", "-show_format",
        "-of", "json", str(path),
    ]
    out = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return json.loads(out.stdout)


def parse_video_stats(path: pathlib.Path) -> dict:
    probe = ffprobe_json(path)
    v = next((s for s in probe.get("streams", []) if s.get("codec_type") == "video"), {})
    fmt = probe.get("format", {})
    return {
        "path": str(path),
        "size_bytes": int(path.stat().st_size),
        "width": int(v.get("width", 0) or 0),
        "height": int(v.get("height", 0) or 0),
        "frame_rate": v.get("r_frame_rate", "0/1"),
        "duration_seconds": float(fmt.get("duration", 0.0) or 0.0),
    }


def load_manifest_timing(manifest_path: pathlib.Path) -> dict:
    if not manifest_path.exists():
        return {"total_elapsed_ms": None, "stage_timings": []}
    raw = json.loads(manifest_path.read_text())
    stages = raw.get("stage_timings", [])
    total = 0
    for stage in stages:
        try:
            total += int(stage.get("elapsed_ms", 0))
        except Exception:
            pass
    return {"total_elapsed_ms": total, "stage_timings": stages}


def psnr_ssim(ref: pathlib.Path, cand: pathlib.Path) -> dict:
    cmd = [
        "ffmpeg", "-v", "error", "-i", str(ref), "-i", str(cand),
        "-lavfi", "[0:v][1:v]psnr=stats_file=-;[0:v][1:v]ssim=stats_file=-",
        "-f", "null", "-",
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    text = (proc.stderr or "") + "\n" + (proc.stdout or "")
    out = {"psnr_avg": None, "ssim_all": None}
    for token in text.replace("\n", " ").split():
        if token.startswith("average:"):
            try:
                out["psnr_avg"] = float(token.split(":", 1)[1])
            except Exception:
                pass
        if token.startswith("All:"):
            try:
                out["ssim_all"] = float(token.split(":", 1)[1])
            except Exception:
                pass
    return out

results = {
    "run_dir": str(run_dir),
    "created_utc": subprocess.run(["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"], capture_output=True, text=True).stdout.strip(),
    "backends": backends,
    "clips": [],
}

for clip_dir in sorted(p for p in outputs_root.iterdir() if p.is_dir()):
    category, clip_name = clip_dir.name.split("__", 1)
    clip_entry = {
        "category": category,
        "clip": clip_name,
        "backend_outputs": {},
        "comparisons": {},
        "review_asset": str(review_root / f"{clip_dir.name}__side_by_side.mp4"),
    }
    for backend in backends:
        out_video = clip_dir / f"{backend}.mp4"
        run_manifest = run_dir / "runs" / clip_dir.name / backend / ".helioframe" / "runs"
        manifest_path = None
        if run_manifest.exists():
            manifests = sorted(run_manifest.glob("*/manifest.json"))
            if manifests:
                manifest_path = manifests[-1]
        if out_video.exists():
            clip_entry["backend_outputs"][backend] = {
                **parse_video_stats(out_video),
                **load_manifest_timing(manifest_path) if manifest_path else {"total_elapsed_ms": None, "stage_timings": []},
                "manifest": str(manifest_path) if manifest_path else None,
            }

    ref = backends[0]
    ref_video = clip_dir / f"{ref}.mp4"
    if ref_video.exists():
        for backend in backends[1:]:
            cand = clip_dir / f"{backend}.mp4"
            if cand.exists():
                clip_entry["comparisons"][f"{ref}_vs_{backend}"] = psnr_ssim(ref_video, cand)

    results["clips"].append(clip_entry)

summary = {
    "total_clips": len(results["clips"]),
    "categories": {},
}
for c in results["clips"]:
    summary["categories"].setdefault(c["category"], 0)
    summary["categories"][c["category"]] += 1
results["summary"] = summary

metrics_path = run_dir / "metrics.json"
metrics_path.write_text(json.dumps(results, indent=2) + "\n")
print(metrics_path)
PY

echo "completed: $(benchmark_now)" | tee -a "$RUN_DIR/benchmark.log"
echo "metrics: $RUN_DIR/metrics.json"
echo "review assets: $RUN_DIR/review"
