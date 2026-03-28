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
        "SeedVR teacher backend requires PyTorch. Install `torch` in the worker environment."
    ) from exc

try:
    from PIL import Image
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "SeedVR teacher backend requires Pillow. Install `Pillow` in the worker environment."
    ) from exc


PINNED_MODEL_VERSION = "seedvr-teacher-v1.0.0"
PINNED_WEIGHTS_SHA256 = (
    "c2c0d9ec5b0c8c1f8b03419e7f3e462f8ab7604f53f6ed15ec5122f7e14b8079"
)
DEFAULT_MODEL_PATH = "models/seedvr-teacher/seedvr_teacher_v1.0.0.ts"


@dataclass(frozen=True)
class TeacherFrameResult:
    index: int
    file_name: str
    source_sha256: str
    output_sha256: str


class SeedVRTeacherBackend:
    """Reference-grade SeedVR teacher backend.

    This backend is intentionally conservative: it only runs from local,
    pinned model artifacts and enforces offline execution.
    """

    def __init__(
        self,
        model_path: Path,
        model_version: str,
        expected_weights_sha256: str,
        device: str = "cuda",
        window_size: int = 12,
        overlap: int = 4,
        precision: str = "fp32",
        offline_only: bool = True,
    ) -> None:
        if window_size <= 0:
            raise ValueError("window_size must be positive")
        if overlap < 0 or overlap >= window_size:
            raise ValueError("overlap must be >= 0 and < window_size")
        if not offline_only:
            raise ValueError(
                "seedvr-teacher requires offline_only=true; network fetches are intentionally disabled"
            )
        if model_version != PINNED_MODEL_VERSION:
            raise ValueError(
                f"seedvr-teacher model_version mismatch: expected `{PINNED_MODEL_VERSION}`, got `{model_version}`"
            )
        if expected_weights_sha256 != PINNED_WEIGHTS_SHA256:
            raise ValueError(
                "seedvr-teacher weights_sha256 mismatch against pinned value; "
                "update backend pin intentionally if weights change"
            )

        self._enable_offline_mode()

        self.model_path = model_path
        self.model_version = model_version
        self.expected_weights_sha256 = expected_weights_sha256
        self.window_size = window_size
        self.overlap = overlap

        requested = device
        if requested == "cuda" and not torch.cuda.is_available():
            requested = "cpu"
        self.device = torch.device(requested)

        self.use_half = precision.lower() == "fp16" and self.device.type == "cuda"
        self.model = self._load_model(model_path)

    @staticmethod
    def _enable_offline_mode() -> None:
        # Guard rails for accidental online model pulls.
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        os.environ["HF_DATASETS_OFFLINE"] = "1"

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
            raise ValueError("seedvr-teacher only accepts local model_path in offline mode")
        if not model_path.exists():
            raise FileNotFoundError(
                f"SeedVR teacher checkpoint not found: {model_path}. "
                "Place the pinned TorchScript checkpoint under models/seedvr-teacher/."
            )

        actual_hash = self._sha256(model_path)
        if actual_hash != self.expected_weights_sha256:
            raise ValueError(
                "seedvr-teacher checkpoint hash mismatch: "
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

    @torch.inference_mode()
    def infer_window(self, frames: torch.Tensor) -> torch.Tensor:
        batched = frames.unsqueeze(0).to(self.device)
        if self.use_half:
            batched = batched.half()

        output = self.model(batched)
        if isinstance(output, (tuple, list)):
            output = output[0]

        if output.ndim == 5:
            output = output.squeeze(0)
        if output.ndim != 4:
            raise RuntimeError(f"unexpected SeedVR teacher output shape: {tuple(output.shape)}")

        return output

    def run(
        self,
        manifest: Any,
        sha256_fn: Any,
    ) -> tuple[list[TeacherFrameResult], dict[str, Any]]:
        frame_tensors: list[torch.Tensor] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            if not source.exists():
                raise FileNotFoundError(f"missing expected frame: {source}")
            frame_tensors.append(self._read_frame(source))

        total = len(frame_tensors)
        output_tensors: dict[int, torch.Tensor] = {}

        step = self.window_size - self.overlap
        for start in range(0, total, step):
            end = min(start + self.window_size, total)
            window = torch.stack(frame_tensors[start:end], dim=0)
            restored = self.infer_window(window)
            for local_idx in range(restored.shape[0]):
                global_idx = start + local_idx
                if global_idx >= total or global_idx in output_tensors:
                    continue
                output_tensors[global_idx] = restored[local_idx]

        written: list[TeacherFrameResult] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            destination = manifest.output_frames_dir / frame.file_name
            output_tensor = output_tensors.get(frame.index)
            if output_tensor is None:
                output_tensor = frame_tensors[frame.index]
            self._write_frame(output_tensor, destination)

            written.append(
                TeacherFrameResult(
                    index=frame.index,
                    file_name=frame.file_name,
                    source_sha256=sha256_fn(source),
                    output_sha256=sha256_fn(destination),
                )
            )

        backend_meta = {
            "backend": "seedvr-teacher",
            "model_path": str(self.model_path),
            "model_version": self.model_version,
            "weights_sha256": self.expected_weights_sha256,
            "device": str(self.device),
            "window_size": self.window_size,
            "overlap": self.overlap,
            "precision": "fp16" if self.use_half else "fp32",
            "offline_only": True,
        }

        return written, backend_meta
