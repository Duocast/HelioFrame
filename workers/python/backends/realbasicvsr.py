from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np

try:
    import torch
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "RealBasicVSR backend requires PyTorch. Install `torch` in the worker environment."
    ) from exc

try:
    from PIL import Image
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "RealBasicVSR backend requires Pillow. Install `Pillow` in the worker environment."
    ) from exc


@dataclass(frozen=True)
class WindowResult:
    index: int
    file_name: str
    source_sha256: str
    output_sha256: str


class RealBasicVSRBackend:
    """Minimal practical bridge backend for running RealBasicVSR-style torch models.

    This backend expects a TorchScript model file and performs temporal-window
    inference on decoded frame tensors.
    """

    def __init__(
        self,
        model_path: Path,
        device: str = "cuda",
        window_size: int = 6,
        overlap: int = 2,
        precision: str = "fp16",
    ) -> None:
        if window_size <= 0:
            raise ValueError("window_size must be positive")
        if overlap < 0 or overlap >= window_size:
            raise ValueError("overlap must be >= 0 and < window_size")

        self.model_path = model_path
        self.window_size = window_size
        self.overlap = overlap

        requested = device
        if requested == "cuda" and not torch.cuda.is_available():
            requested = "cpu"
        self.device = torch.device(requested)

        self.use_half = precision.lower() == "fp16" and self.device.type == "cuda"
        self.model = self._load_model(model_path)

    def _load_model(self, model_path: Path) -> torch.nn.Module:
        if not model_path.exists():
            raise FileNotFoundError(
                f"RealBasicVSR checkpoint not found: {model_path}. "
                "Put a TorchScript checkpoint in models/realbasicvsr/."
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
        # Prefer shape [1, T, C, H, W]. Fallback to [T, C, H, W] if required.
        batched = frames.unsqueeze(0).to(self.device)
        if self.use_half:
            batched = batched.half()

        try:
            output = self.model(batched)
        except RuntimeError:
            output = self.model(frames.to(self.device))

        if isinstance(output, (tuple, list)):
            output = output[0]

        if output.ndim == 5:
            output = output.squeeze(0)

        if output.ndim != 4:
            raise RuntimeError(f"unexpected RealBasicVSR output shape: {tuple(output.shape)}")

        return output

    def run(
        self,
        manifest: Any,
        sha256_fn: Any,
    ) -> tuple[list[WindowResult], dict[str, Any]]:
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

        written: list[WindowResult] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            destination = manifest.output_frames_dir / frame.file_name
            output_tensor = output_tensors.get(frame.index)
            if output_tensor is None:
                output_tensor = frame_tensors[frame.index]
            self._write_frame(output_tensor, destination)

            written.append(
                WindowResult(
                    index=frame.index,
                    file_name=frame.file_name,
                    source_sha256=sha256_fn(source),
                    output_sha256=sha256_fn(destination),
                )
            )

        backend_meta = {
            "backend": "realbasicvsr-bridge",
            "model_path": str(self.model_path),
            "device": str(self.device),
            "window_size": self.window_size,
            "overlap": self.overlap,
            "precision": "fp16" if self.use_half else "fp32",
        }

        return written, backend_meta
