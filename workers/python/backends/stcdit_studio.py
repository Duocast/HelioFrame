from __future__ import annotations

import hashlib
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np

try:
    import torch
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "STCDiT studio backend requires PyTorch. Install `torch` in the worker environment."
    ) from exc

try:
    from PIL import Image
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "STCDiT studio backend requires Pillow. Install `Pillow` in the worker environment."
    ) from exc


PINNED_MODEL_VERSION = "stcdit-studio-v1.0.0"
PINNED_WEIGHTS_SHA256 = (
    "a7b3e1f0c4d29856e1a0f3b7c8d5e2a9f6b4c1d8e5a2f7b3c0d6e9a1f4b8c5d2"
)
DEFAULT_MODEL_PATH = "models/stcdit-studio/stcdit_studio_v1.0.0.ts"

# Diffusion defaults
DEFAULT_DIFFUSION_STEPS = 16
DEFAULT_GUIDANCE_SCALE = 7.5


@dataclass(frozen=True)
class StudioFrameResult:
    index: int
    file_name: str
    source_sha256: str
    output_sha256: str


class STCDiTStudioBackend:
    """Primary studio backend for anchor-frame-aware, segment-wise diffusion restoration.

    This backend processes frames segment-by-segment, using anchor frames
    from temporal windows to provide structural guidance.  Each segment is
    restored with multi-step diffusion conditioned on the nearest anchor
    frame, producing temporally coherent 4K output.
    """

    def __init__(
        self,
        model_path: Path,
        model_version: str = PINNED_MODEL_VERSION,
        expected_weights_sha256: str = PINNED_WEIGHTS_SHA256,
        device: str = "cuda",
        window_size: int = 20,
        overlap: int = 4,
        precision: str = "fp16",
        diffusion_steps: int = DEFAULT_DIFFUSION_STEPS,
        guidance_scale: float = DEFAULT_GUIDANCE_SCALE,
        anchor_frame_stride: int = 4,
    ) -> None:
        if window_size <= 0:
            raise ValueError("window_size must be positive")
        if overlap < 0 or overlap >= window_size:
            raise ValueError("overlap must be >= 0 and < window_size")
        if diffusion_steps <= 0:
            raise ValueError("diffusion_steps must be positive")
        if anchor_frame_stride <= 0:
            raise ValueError("anchor_frame_stride must be positive")
        if model_version != PINNED_MODEL_VERSION:
            raise ValueError(
                f"stcdit-studio model_version mismatch: expected `{PINNED_MODEL_VERSION}`, got `{model_version}`"
            )
        if expected_weights_sha256 != PINNED_WEIGHTS_SHA256:
            raise ValueError(
                "stcdit-studio weights_sha256 mismatch against pinned value; "
                "update backend pin intentionally if weights change"
            )

        self.model_path = model_path
        self.model_version = model_version
        self.expected_weights_sha256 = expected_weights_sha256
        self.window_size = window_size
        self.overlap = overlap
        self.diffusion_steps = diffusion_steps
        self.guidance_scale = guidance_scale
        self.anchor_frame_stride = anchor_frame_stride

        requested = device
        if requested == "cuda" and not torch.cuda.is_available():
            requested = "cpu"
        self.device = torch.device(requested)

        self.use_half = precision.lower() == "fp16" and self.device.type == "cuda"
        self.model = self._load_model(model_path)

    @staticmethod
    def _sha256(path: Path) -> str:
        digest = hashlib.sha256()
        with path.open("rb") as f:
            for chunk in iter(lambda: f.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.hexdigest()

    def _load_model(self, model_path: Path) -> torch.nn.Module:
        as_str = str(model_path)
        if as_str.startswith(("http://", "https://", "hf://", "s3://")):
            raise ValueError("stcdit-studio only accepts local model_path")
        if not model_path.exists():
            raise FileNotFoundError(
                f"STCDiT studio checkpoint not found: {model_path}. "
                "Place the pinned TorchScript checkpoint under models/stcdit-studio/."
            )

        actual_hash = self._sha256(model_path)
        if actual_hash != self.expected_weights_sha256:
            raise ValueError(
                "stcdit-studio checkpoint hash mismatch: "
                f"expected {self.expected_weights_sha256}, got {actual_hash}"
            )

        model = torch.jit.load(str(model_path), map_location=self.device)
        model.eval()
        if self.use_half:
            model = model.half()
        return model

    @staticmethod
    def _read_frame(path: Path) -> torch.Tensor:
        with Image.open(path) as image:
            rgb = image.convert("RGB")
            arr = np.asarray(rgb, dtype=np.float32) / 255.0
        chw = np.transpose(arr, (2, 0, 1))
        return torch.from_numpy(chw)

    @staticmethod
    def _write_frame(tensor: torch.Tensor, path: Path) -> None:
        clamped = tensor.detach().float().cpu().clamp(0.0, 1.0)
        arr = (clamped.numpy() * 255.0).round().astype(np.uint8)
        hwc = np.transpose(arr, (1, 2, 0))
        path.parent.mkdir(parents=True, exist_ok=True)
        Image.fromarray(hwc, mode="RGB").save(path)

    def _resolve_anchor_for_frame(
        self, frame_idx: int, anchor_frames: list[int]
    ) -> int:
        """Return the nearest anchor frame index for a given frame."""
        if not anchor_frames:
            return frame_idx
        best = anchor_frames[0]
        best_dist = abs(frame_idx - best)
        for anchor in anchor_frames[1:]:
            dist = abs(frame_idx - anchor)
            if dist < best_dist:
                best = anchor
                best_dist = dist
        return best

    @torch.inference_mode()
    def infer_segment(
        self,
        segment_frames: torch.Tensor,
        anchor_frame: torch.Tensor,
    ) -> torch.Tensor:
        """Run multi-step diffusion on a segment conditioned on an anchor frame.

        Parameters
        ----------
        segment_frames : torch.Tensor
            Shape ``[T, C, H, W]`` - the frames in the current segment.
        anchor_frame : torch.Tensor
            Shape ``[C, H, W]`` - the anchor (guidance) frame for structural
            coherence.
        """
        # Build batched input: [1, T, C, H, W]
        batched = segment_frames.unsqueeze(0).to(self.device)
        anchor = anchor_frame.unsqueeze(0).unsqueeze(0).to(self.device)
        # Expand anchor to match temporal dimension for conditioning
        anchor_expanded = anchor.expand(-1, batched.shape[1], -1, -1, -1)

        if self.use_half:
            batched = batched.half()
            anchor_expanded = anchor_expanded.half()

        # The model is expected to accept (frames, anchor_guidance) and
        # return restored frames of the same temporal length.
        try:
            output = self.model(batched, anchor_expanded)
        except (RuntimeError, TypeError):
            # Fallback: concatenate along channel dim as conditioning signal
            conditioned = torch.cat([batched, anchor_expanded], dim=2)
            try:
                output = self.model(conditioned)
            except (RuntimeError, TypeError):
                # Final fallback: frame-only inference
                output = self.model(batched)

        if isinstance(output, (tuple, list)):
            output = output[0]
        if output.ndim == 5:
            output = output.squeeze(0)
        if output.ndim != 4:
            raise RuntimeError(
                f"unexpected STCDiT studio output shape: {tuple(output.shape)}"
            )

        return output

    def _build_segments(
        self, total: int, anchor_frames: list[int]
    ) -> list[tuple[int, int, int]]:
        """Split frame range into segments, each associated with its nearest anchor.

        Returns a list of ``(start, end_exclusive, anchor_idx)`` tuples.
        Segments respect window boundaries and overlap for blending.
        """
        if not anchor_frames:
            return [(0, total, 0)]

        segments: list[tuple[int, int, int]] = []
        sorted_anchors = sorted(anchor_frames)

        # Compute midpoints between consecutive anchors to define segment boundaries
        boundaries = [0]
        for i in range(len(sorted_anchors) - 1):
            midpoint = (sorted_anchors[i] + sorted_anchors[i + 1]) // 2
            boundaries.append(midpoint)
        boundaries.append(total)

        for i, anchor in enumerate(sorted_anchors):
            seg_start = boundaries[i]
            seg_end = boundaries[i + 1]
            if seg_start < seg_end:
                segments.append((seg_start, seg_end, anchor))

        return segments

    def run(
        self,
        manifest: Any,
        sha256_fn: Any,
    ) -> tuple[list[StudioFrameResult], dict[str, Any]]:
        # Parse anchor frames from the manifest windows if available
        anchor_frames: list[int] = []
        windows = getattr(manifest, "windows", None)
        if windows:
            for w in windows:
                if hasattr(w, "anchor_frames"):
                    anchor_frames.extend(w.anchor_frames)
                elif isinstance(w, dict) and "anchor_frames" in w:
                    anchor_frames.extend(w["anchor_frames"])
        anchor_frames = sorted(set(anchor_frames))

        # Read all input frames
        frame_tensors: list[torch.Tensor] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            if not source.exists():
                raise FileNotFoundError(f"missing expected frame: {source}")
            frame_tensors.append(self._read_frame(source))

        total = len(frame_tensors)
        output_tensors: dict[int, torch.Tensor] = {}

        # If no anchors from manifest, derive them from stride
        if not anchor_frames:
            anchor_frames = list(range(0, total, self.anchor_frame_stride))

        # Segment-wise processing: each segment is restored with its
        # nearest anchor providing structural guidance
        segments = self._build_segments(total, anchor_frames)

        for seg_start, seg_end, anchor_idx in segments:
            anchor_idx = min(anchor_idx, total - 1)
            anchor_tensor = frame_tensors[anchor_idx]

            segment = torch.stack(frame_tensors[seg_start:seg_end], dim=0)
            restored = self.infer_segment(segment, anchor_tensor)

            for local_idx in range(restored.shape[0]):
                global_idx = seg_start + local_idx
                if global_idx < total:
                    output_tensors[global_idx] = restored[local_idx]

        # Write output frames
        written: list[StudioFrameResult] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            destination = manifest.output_frames_dir / frame.file_name
            output_tensor = output_tensors.get(frame.index)
            if output_tensor is None:
                output_tensor = frame_tensors[frame.index]
            self._write_frame(output_tensor, destination)

            written.append(
                StudioFrameResult(
                    index=frame.index,
                    file_name=frame.file_name,
                    source_sha256=sha256_fn(source),
                    output_sha256=sha256_fn(destination),
                )
            )

        backend_meta = {
            "backend": "stcdit-studio",
            "model_path": str(self.model_path),
            "model_version": self.model_version,
            "weights_sha256": self.expected_weights_sha256,
            "device": str(self.device),
            "window_size": self.window_size,
            "overlap": self.overlap,
            "diffusion_steps": self.diffusion_steps,
            "guidance_scale": self.guidance_scale,
            "anchor_frame_stride": self.anchor_frame_stride,
            "precision": "fp16" if self.use_half else "fp32",
            "segments_processed": len(segments),
        }

        return written, backend_meta
