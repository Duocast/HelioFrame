"""HelioFrame master orchestration backend.

This backend composes the full quality pipeline as a single worker pass:

1. **Teacher guidance** -- run the SeedVR teacher to produce conservative
   reference-grade frames that anchor later stages.
2. **Studio path** -- run STCDiT studio restoration conditioned on the teacher
   output as structural guidance, producing diffusion-restored frames.
3. **Detail refinement** -- run the selective high-frequency detail refiner on
   the studio output, enhancing text, hair, fabric, foliage, and architecture
   while guarding against temporal sparkle.
4. **QC rerun** -- evaluate temporal quality metrics on each window.  Windows
   that fail the QC gate are re-run through the studio path (skipping the
   refiner) to recover a safer baseline.  A second QC failure on a rerun
   window falls back to the teacher output for that window.

The orchestrator reuses the existing backend classes (``SeedVRTeacherBackend``,
``STCDiTStudioBackend``, ``DetailRefinerBackend``) rather than duplicating
inference logic.
"""

from __future__ import annotations

import copy
import hashlib
import shutil
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any

import numpy as np

try:
    import torch
except ImportError as exc:  # pragma: no cover
    raise RuntimeError(
        "HelioFrame master backend requires PyTorch. "
        "Install `torch` in the worker environment."
    ) from exc

try:
    from PIL import Image
except ImportError as exc:  # pragma: no cover
    raise RuntimeError(
        "HelioFrame master backend requires Pillow. "
        "Install `Pillow` in the worker environment."
    ) from exc

from .seedvr_teacher import SeedVRTeacherBackend
from .stcdit_studio import STCDiTStudioBackend
from .detail_refiner import DetailRefinerBackend


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

MAX_QC_RERUNS = 2

# Temporal QC thresholds -- tighter than studio defaults because master is the
# research flagship and must beat studio quality.
DEFAULT_MAX_FLICKER_SCORE = 0.12
DEFAULT_MAX_PATCH_SHIMMER = 0.08


# ---------------------------------------------------------------------------
# Result dataclass
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class MasterFrameResult:
    index: int
    file_name: str
    source_sha256: str
    output_sha256: str


# ---------------------------------------------------------------------------
# Lightweight manifest proxy
# ---------------------------------------------------------------------------

class _ManifestSlice:
    """A lightweight proxy that presents a subset of frames as a full manifest.

    The orchestrator uses this to feed individual stages (teacher, studio,
    refiner) with subsets of frames while pointing ``input_frames_dir`` and
    ``output_frames_dir`` at stage-specific scratch directories.
    """

    def __init__(
        self,
        original: Any,
        frames: list[Any],
        input_frames_dir: Path,
        output_frames_dir: Path,
    ) -> None:
        self.schema_version = original.schema_version
        self.run_id = original.run_id
        self.clip_id = original.clip_id
        self.backend = original.backend
        self.backend_options = original.backend_options
        self.input_frames_dir = input_frames_dir
        self.output_frames_dir = output_frames_dir
        self.output_manifest_path = original.output_manifest_path
        self.frames = frames
        self.windows = getattr(original, "windows", None)


# ---------------------------------------------------------------------------
# Temporal QC helpers
# ---------------------------------------------------------------------------

def _read_frame(path: Path) -> torch.Tensor:
    with Image.open(path) as image:
        rgb = image.convert("RGB")
        arr = np.asarray(rgb, dtype=np.float32) / 255.0
    chw = np.transpose(arr, (2, 0, 1))
    return torch.from_numpy(chw)


def _laplacian_energy(frame: torch.Tensor) -> float:
    """Scalar high-frequency energy for a single frame."""
    gray = frame.mean(dim=0, keepdim=True).unsqueeze(0)
    kernel = torch.tensor(
        [[0.0, 1.0, 0.0], [1.0, -4.0, 1.0], [0.0, 1.0, 0.0]],
        dtype=gray.dtype,
        device=gray.device,
    ).reshape(1, 1, 3, 3)
    import torch.nn.functional as F
    response = F.conv2d(gray, kernel, padding=1)
    return float(response.abs().mean())


def _temporal_qc_score(
    frames_dir: Path,
    frame_names: list[str],
) -> tuple[float, float]:
    """Compute (flicker_score, shimmer_score) for a window of output frames.

    flicker_score: variance of per-frame HF energy across the window.
    shimmer_score: mean absolute frame-to-frame HF energy difference.
    """
    energies: list[float] = []
    for name in frame_names:
        path = frames_dir / name
        if not path.exists():
            continue
        tensor = _read_frame(path)
        energies.append(_laplacian_energy(tensor))

    if len(energies) < 2:
        return 0.0, 0.0

    arr = np.array(energies, dtype=np.float64)
    flicker = float(np.var(arr))
    diffs = np.abs(np.diff(arr))
    shimmer = float(np.mean(diffs))
    return flicker, shimmer


# ---------------------------------------------------------------------------
# Orchestration backend
# ---------------------------------------------------------------------------

class HelioFrameMasterBackend:
    """Orchestration backend composing teacher + studio + refiner + QC rerun."""

    def __init__(
        self,
        *,
        # Teacher options
        teacher_model_path: Path,
        teacher_model_version: str,
        teacher_weights_sha256: str,
        teacher_device: str = "cuda",
        teacher_window_size: int = 12,
        teacher_overlap: int = 4,
        teacher_precision: str = "fp32",
        # Studio options
        studio_model_path: Path,
        studio_model_version: str,
        studio_weights_sha256: str,
        studio_device: str = "cuda",
        studio_window_size: int = 20,
        studio_overlap: int = 4,
        studio_precision: str = "fp16",
        studio_diffusion_steps: int = 24,
        studio_guidance_scale: float = 7.5,
        studio_anchor_frame_stride: int = 3,
        # Refiner options
        refiner_model_path: Path,
        refiner_model_version: str,
        refiner_weights_sha256: str,
        refiner_device: str = "cuda",
        refiner_refinement_steps: int = 6,
        refiner_refinement_strength: float = 0.4,
        refiner_hf_energy_threshold: float = 0.25,
        refiner_min_window_hf_ratio: float = 0.10,
        refiner_max_hf_flicker: float = 0.12,
        refiner_max_patch_shimmer: float = 0.08,
        refiner_categories: list[str] | None = None,
        refiner_patch_size: int = 128,
        refiner_precision: str = "fp16",
        # QC options
        max_qc_reruns: int = MAX_QC_RERUNS,
        qc_max_flicker: float = DEFAULT_MAX_FLICKER_SCORE,
        qc_max_shimmer: float = DEFAULT_MAX_PATCH_SHIMMER,
    ) -> None:
        self.max_qc_reruns = max_qc_reruns
        self.qc_max_flicker = qc_max_flicker
        self.qc_max_shimmer = qc_max_shimmer

        self.teacher = SeedVRTeacherBackend(
            model_path=teacher_model_path,
            model_version=teacher_model_version,
            expected_weights_sha256=teacher_weights_sha256,
            device=teacher_device,
            window_size=teacher_window_size,
            overlap=teacher_overlap,
            precision=teacher_precision,
            offline_only=True,
        )

        self.studio = STCDiTStudioBackend(
            model_path=studio_model_path,
            model_version=studio_model_version,
            expected_weights_sha256=studio_weights_sha256,
            device=studio_device,
            window_size=studio_window_size,
            overlap=studio_overlap,
            precision=studio_precision,
            diffusion_steps=studio_diffusion_steps,
            guidance_scale=studio_guidance_scale,
            anchor_frame_stride=studio_anchor_frame_stride,
        )

        self.refiner = DetailRefinerBackend(
            model_path=refiner_model_path,
            model_version=refiner_model_version,
            expected_weights_sha256=refiner_weights_sha256,
            device=refiner_device,
            refinement_steps=refiner_refinement_steps,
            refinement_strength=refiner_refinement_strength,
            hf_energy_threshold=refiner_hf_energy_threshold,
            min_window_hf_ratio=refiner_min_window_hf_ratio,
            max_hf_flicker=refiner_max_hf_flicker,
            max_patch_shimmer=refiner_max_patch_shimmer,
            categories=refiner_categories,
            patch_size=refiner_patch_size,
            precision=refiner_precision,
        )

    # ------------------------------------------------------------------
    # I/O helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _copy_frames(
        src_dir: Path, dst_dir: Path, file_names: list[str],
    ) -> None:
        dst_dir.mkdir(parents=True, exist_ok=True)
        for name in file_names:
            src = src_dir / name
            dst = dst_dir / name
            if src.exists():
                shutil.copy2(src, dst)

    # ------------------------------------------------------------------
    # Stage runners
    # ------------------------------------------------------------------

    def _run_teacher(
        self,
        manifest: Any,
        scratch: Path,
        sha256_fn: Any,
    ) -> Path:
        """Stage 1: teacher guidance pass. Returns output directory."""
        teacher_out = scratch / "teacher-output"
        teacher_out.mkdir(parents=True, exist_ok=True)

        proxy = _ManifestSlice(
            original=manifest,
            frames=manifest.frames,
            input_frames_dir=manifest.input_frames_dir,
            output_frames_dir=teacher_out,
        )
        self.teacher.run(proxy, sha256_fn=sha256_fn)
        return teacher_out

    def _run_studio(
        self,
        manifest: Any,
        input_dir: Path,
        scratch: Path,
        tag: str,
        sha256_fn: Any,
    ) -> Path:
        """Stage 2: studio diffusion pass. Returns output directory."""
        studio_out = scratch / f"studio-output-{tag}"
        studio_out.mkdir(parents=True, exist_ok=True)

        proxy = _ManifestSlice(
            original=manifest,
            frames=manifest.frames,
            input_frames_dir=input_dir,
            output_frames_dir=studio_out,
        )
        self.studio.run(proxy, sha256_fn=sha256_fn)
        return studio_out

    def _run_refiner(
        self,
        manifest: Any,
        input_dir: Path,
        scratch: Path,
        sha256_fn: Any,
    ) -> Path:
        """Stage 3: detail refinement pass. Returns output directory."""
        refiner_out = scratch / "refiner-output"
        refiner_out.mkdir(parents=True, exist_ok=True)

        proxy = _ManifestSlice(
            original=manifest,
            frames=manifest.frames,
            input_frames_dir=input_dir,
            output_frames_dir=refiner_out,
        )
        self.refiner.run(proxy, sha256_fn=sha256_fn)
        return refiner_out

    def _qc_windows(
        self,
        frames_dir: Path,
        manifest: Any,
    ) -> list[tuple[int, int, bool, float, float]]:
        """Evaluate temporal QC on each window.

        Returns list of (start, end_exclusive, passed, flicker, shimmer).
        """
        total = len(manifest.frames)
        raw_windows = getattr(manifest, "windows", None)
        if raw_windows and isinstance(raw_windows, list):
            windows = [
                (int(w["start_frame"]), int(w["end_frame_exclusive"]))
                if isinstance(w, dict)
                else (int(w.start_frame), int(w.end_frame_exclusive))
                for w in raw_windows
            ]
        else:
            # Treat entire clip as a single window
            windows = [(0, total)]

        results: list[tuple[int, int, bool, float, float]] = []
        for start, end in windows:
            end = min(end, total)
            names = [
                manifest.frames[i].file_name
                for i in range(start, end)
            ]
            flicker, shimmer = _temporal_qc_score(frames_dir, names)
            passed = (
                flicker <= self.qc_max_flicker
                and shimmer <= self.qc_max_shimmer
            )
            results.append((start, end, passed, flicker, shimmer))
        return results

    # ------------------------------------------------------------------
    # Main entry point
    # ------------------------------------------------------------------

    def run(
        self,
        manifest: Any,
        sha256_fn: Any,
    ) -> tuple[list[MasterFrameResult], dict[str, Any]]:
        scratch = manifest.output_frames_dir.parent / "helioframe-master-scratch"
        scratch.mkdir(parents=True, exist_ok=True)

        frame_names = [f.file_name for f in manifest.frames]

        # ----- Stage 1: teacher guidance -----
        teacher_dir = self._run_teacher(manifest, scratch, sha256_fn)

        # ----- Stage 2: studio pass (using teacher output as input) -----
        studio_dir = self._run_studio(
            manifest, teacher_dir, scratch, "initial", sha256_fn,
        )

        # ----- Stage 3: detail refinement -----
        refiner_dir = self._run_refiner(
            manifest, studio_dir, scratch, sha256_fn,
        )

        # ----- Stage 4: QC evaluation + rerun loop -----
        current_dir = refiner_dir
        qc_results = self._qc_windows(current_dir, manifest)
        qc_rounds: list[list[dict[str, Any]]] = []
        qc_rounds.append([
            {
                "start_frame": s,
                "end_frame_exclusive": e,
                "passed": p,
                "flicker": round(f, 6),
                "shimmer": round(sh, 6),
            }
            for s, e, p, f, sh in qc_results
        ])

        failed_windows = [
            (s, e) for s, e, passed, _f, _sh in qc_results if not passed
        ]
        rerun_count = 0

        # Rerun failed windows through studio (skip refiner to reduce sparkle
        # risk).  On second failure, fall back to teacher output.
        while failed_windows and rerun_count < self.max_qc_reruns:
            rerun_count += 1

            if rerun_count == 1:
                # Re-run through studio without detail refiner
                rerun_input = teacher_dir
                rerun_tag = f"rerun-{rerun_count}"
            else:
                # Final fallback: use teacher output directly
                rerun_input = teacher_dir
                rerun_tag = f"fallback-{rerun_count}"

            for win_start, win_end in failed_windows:
                if rerun_count >= 2:
                    # Hard fallback: copy teacher frames for this window
                    for i in range(win_start, min(win_end, len(manifest.frames))):
                        name = manifest.frames[i].file_name
                        src = teacher_dir / name
                        dst = current_dir / name
                        if src.exists():
                            shutil.copy2(src, dst)
                else:
                    # Re-run studio on just this window's frames
                    rerun_out = scratch / f"studio-output-{rerun_tag}-w{win_start}"
                    rerun_out.mkdir(parents=True, exist_ok=True)

                    win_frames = manifest.frames[win_start:win_end]
                    proxy = _ManifestSlice(
                        original=manifest,
                        frames=win_frames,
                        input_frames_dir=rerun_input,
                        output_frames_dir=rerun_out,
                    )
                    self.studio.run(proxy, sha256_fn=sha256_fn)

                    # Copy rerun results back into current output dir
                    for frame in win_frames:
                        src = rerun_out / frame.file_name
                        dst = current_dir / frame.file_name
                        if src.exists():
                            shutil.copy2(src, dst)

            # Re-evaluate QC on previously failed windows
            round_results: list[dict[str, Any]] = []
            new_failures: list[tuple[int, int]] = []
            for win_start, win_end in failed_windows:
                names = [
                    manifest.frames[i].file_name
                    for i in range(win_start, min(win_end, len(manifest.frames)))
                ]
                flicker, shimmer = _temporal_qc_score(current_dir, names)
                passed = (
                    flicker <= self.qc_max_flicker
                    and shimmer <= self.qc_max_shimmer
                )
                round_results.append({
                    "start_frame": win_start,
                    "end_frame_exclusive": win_end,
                    "passed": passed,
                    "flicker": round(flicker, 6),
                    "shimmer": round(shimmer, 6),
                    "action": "rerun_studio" if rerun_count == 1 else "fallback_teacher",
                })
                if not passed:
                    new_failures.append((win_start, win_end))

            qc_rounds.append(round_results)
            failed_windows = new_failures

        # ----- Copy final frames to output directory -----
        manifest.output_frames_dir.mkdir(parents=True, exist_ok=True)
        self._copy_frames(current_dir, manifest.output_frames_dir, frame_names)

        # ----- Build output frame results -----
        written: list[MasterFrameResult] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            destination = manifest.output_frames_dir / frame.file_name
            written.append(
                MasterFrameResult(
                    index=frame.index,
                    file_name=frame.file_name,
                    source_sha256=sha256_fn(source),
                    output_sha256=sha256_fn(destination),
                )
            )

        # ----- Compute summary stats -----
        initial_qc = qc_rounds[0] if qc_rounds else []
        initial_passed = sum(1 for r in initial_qc if r["passed"])
        initial_failed = len(initial_qc) - initial_passed

        backend_meta: dict[str, Any] = {
            "backend": "helioframe-master",
            "stages": [
                "teacher-guidance",
                "studio-diffusion",
                "detail-refinement",
                "temporal-qc-rerun",
            ],
            "teacher": {
                "model_path": str(self.teacher.model_path),
                "model_version": self.teacher.model_version,
                "device": str(self.teacher.device),
                "window_size": self.teacher.window_size,
                "overlap": self.teacher.overlap,
                "precision": "fp16" if self.teacher.use_half else "fp32",
            },
            "studio": {
                "model_path": str(self.studio.model_path),
                "model_version": self.studio.model_version,
                "device": str(self.studio.device),
                "window_size": self.studio.window_size,
                "overlap": self.studio.overlap,
                "diffusion_steps": self.studio.diffusion_steps,
                "guidance_scale": self.studio.guidance_scale,
                "anchor_frame_stride": self.studio.anchor_frame_stride,
                "precision": "fp16" if self.studio.use_half else "fp32",
            },
            "refiner": {
                "model_path": str(self.refiner.model_path),
                "model_version": self.refiner.model_version,
                "device": str(self.refiner.device),
                "refinement_steps": self.refiner.refinement_steps,
                "refinement_strength": self.refiner.refinement_strength,
                "categories": self.refiner.categories,
                "patch_size": self.refiner.patch_size,
                "precision": "fp16" if self.refiner.use_half else "fp32",
            },
            "qc": {
                "max_reruns": self.max_qc_reruns,
                "max_flicker": self.qc_max_flicker,
                "max_shimmer": self.qc_max_shimmer,
                "reruns_performed": rerun_count,
                "initial_windows_passed": initial_passed,
                "initial_windows_failed": initial_failed,
                "qc_rounds": qc_rounds,
            },
        }

        return written, backend_meta
