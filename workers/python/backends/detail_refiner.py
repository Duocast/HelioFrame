"""Selective high-frequency detail refinement backend.

This second-stage backend runs *after* the primary restoration pass and
selectively enhances patches that contain high-frequency content (text, hair,
fabric, foliage, architecture).  Smooth regions are left untouched to avoid
introducing temporal sparkle.

The refiner operates patch-by-patch within each temporal window:

1.  Compute a Laplacian-based high-frequency energy map for every frame.
2.  Classify each patch against the requested detail categories using a
    lightweight frequency-domain heuristic (no external classifier needed).
3.  Run a short diffusion refinement (fewer steps than the main restore) on
    qualifying patches only.
4.  Blend refined patches back, gated by a temporal sparkle guard that
    compares frame-to-frame variance of the high-frequency band before and
    after refinement.  If the guard trips, the pre-refinement frames for
    that window are kept instead.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np

try:
    import torch
    import torch.nn.functional as F
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "Detail refiner backend requires PyTorch. Install `torch` in the worker environment."
    ) from exc

try:
    from PIL import Image
except ImportError as exc:  # pragma: no cover - runtime env dependency
    raise RuntimeError(
        "Detail refiner backend requires Pillow. Install `Pillow` in the worker environment."
    ) from exc


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

PINNED_MODEL_VERSION = "detail-refiner-v1.0.0"
PINNED_WEIGHTS_SHA256 = (
    "b8d4f2a1e6c73950d2b1e4a8f7c3d6b9e5a2f8c1d7b4e0a3f6c9d2b5e8a1f4c7"
)
DEFAULT_MODEL_PATH = "models/detail-refiner/detail_refiner_v1.0.0.ts"

# Default refinement parameters — deliberately conservative.
DEFAULT_REFINEMENT_STEPS = 6
DEFAULT_REFINEMENT_STRENGTH = 0.4
DEFAULT_HF_ENERGY_THRESHOLD = 0.25
DEFAULT_MAX_HF_FLICKER = 0.12
DEFAULT_MAX_PATCH_SHIMMER = 0.08

# Supported detail categories for selective refinement.
ALL_CATEGORIES = frozenset({"text", "hair", "fabric", "foliage", "architecture"})


# ---------------------------------------------------------------------------
# Result dataclass
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class RefinerFrameResult:
    index: int
    file_name: str
    source_sha256: str
    output_sha256: str


# ---------------------------------------------------------------------------
# Backend
# ---------------------------------------------------------------------------

class DetailRefinerBackend:
    """Second-stage selective high-frequency detail refinement.

    Applies refinement only to high-frequency patches that match the
    requested content categories, and rolls back any window where the
    temporal sparkle guard detects introduced flicker.
    """

    def __init__(
        self,
        model_path: Path,
        model_version: str = PINNED_MODEL_VERSION,
        expected_weights_sha256: str = PINNED_WEIGHTS_SHA256,
        device: str = "cuda",
        refinement_steps: int = DEFAULT_REFINEMENT_STEPS,
        refinement_strength: float = DEFAULT_REFINEMENT_STRENGTH,
        hf_energy_threshold: float = DEFAULT_HF_ENERGY_THRESHOLD,
        max_hf_flicker: float = DEFAULT_MAX_HF_FLICKER,
        max_patch_shimmer: float = DEFAULT_MAX_PATCH_SHIMMER,
        categories: list[str] | None = None,
        patch_size: int = 128,
        precision: str = "fp16",
    ) -> None:
        if refinement_steps <= 0:
            raise ValueError("refinement_steps must be positive")
        if not (0.0 < refinement_strength <= 1.0):
            raise ValueError("refinement_strength must be in (0.0, 1.0]")
        if not (0.0 <= hf_energy_threshold <= 1.0):
            raise ValueError("hf_energy_threshold must be in [0.0, 1.0]")
        if patch_size <= 0:
            raise ValueError("patch_size must be positive")
        if model_version != PINNED_MODEL_VERSION:
            raise ValueError(
                f"detail-refiner model_version mismatch: expected "
                f"`{PINNED_MODEL_VERSION}`, got `{model_version}`"
            )
        if expected_weights_sha256 != PINNED_WEIGHTS_SHA256:
            raise ValueError(
                "detail-refiner weights_sha256 mismatch against pinned value; "
                "update backend pin intentionally if weights change"
            )

        resolved_categories = set(categories) if categories else set(ALL_CATEGORIES)
        unknown = resolved_categories - ALL_CATEGORIES
        if unknown:
            raise ValueError(f"unknown detail categories: {sorted(unknown)}")

        self.model_path = model_path
        self.model_version = model_version
        self.expected_weights_sha256 = expected_weights_sha256
        self.refinement_steps = refinement_steps
        self.refinement_strength = refinement_strength
        self.hf_energy_threshold = hf_energy_threshold
        self.max_hf_flicker = max_hf_flicker
        self.max_patch_shimmer = max_patch_shimmer
        self.categories = sorted(resolved_categories)
        self.patch_size = patch_size

        requested = device
        if requested == "cuda" and not torch.cuda.is_available():
            requested = "cpu"
        self.device = torch.device(requested)

        self.use_half = precision.lower() == "fp16" and self.device.type == "cuda"
        self.model = self._load_model(model_path)

    # ------------------------------------------------------------------
    # I/O helpers
    # ------------------------------------------------------------------

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
            raise ValueError("detail-refiner only accepts local model_path")
        if not model_path.exists():
            raise FileNotFoundError(
                f"Detail refiner checkpoint not found: {model_path}. "
                "Place the pinned TorchScript checkpoint under models/detail-refiner/."
            )

        actual_hash = self._sha256(model_path)
        if actual_hash != self.expected_weights_sha256:
            raise ValueError(
                "detail-refiner checkpoint hash mismatch: "
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

    # ------------------------------------------------------------------
    # High-frequency analysis
    # ------------------------------------------------------------------

    @staticmethod
    def _laplacian_energy(frame: torch.Tensor) -> torch.Tensor:
        """Compute per-pixel high-frequency energy via a Laplacian kernel.

        Parameters
        ----------
        frame : torch.Tensor
            Shape ``[C, H, W]`` in [0, 1].

        Returns
        -------
        torch.Tensor
            Shape ``[1, H, W]`` — absolute Laplacian response averaged over
            channels.
        """
        gray = frame.mean(dim=0, keepdim=True).unsqueeze(0)  # [1, 1, H, W]
        kernel = torch.tensor(
            [[0.0, 1.0, 0.0], [1.0, -4.0, 1.0], [0.0, 1.0, 0.0]],
            dtype=gray.dtype,
            device=gray.device,
        ).reshape(1, 1, 3, 3)
        response = F.conv2d(gray, kernel, padding=1)
        return response.abs().squeeze(0)  # [1, H, W]

    def _patch_hf_energies(
        self, energy_map: torch.Tensor, height: int, width: int
    ) -> list[tuple[int, int, float]]:
        """Return (y, x, mean_energy) for each non-overlapping patch.

        Only patches whose mean energy exceeds ``self.hf_energy_threshold``
        are included.
        """
        ps = self.patch_size
        candidates: list[tuple[int, int, float]] = []
        for y in range(0, height, ps):
            for x in range(0, width, ps):
                patch = energy_map[
                    :, y : min(y + ps, height), x : min(x + ps, width)
                ]
                mean_e = float(patch.mean())
                if mean_e >= self.hf_energy_threshold:
                    candidates.append((y, x, mean_e))
        return candidates

    @staticmethod
    def _classify_patch_category(patch: torch.Tensor) -> set[str]:
        """Lightweight frequency-domain heuristic to guess content category.

        Uses simple statistical proxies (edge density, directional energy,
        periodicity) rather than a learned classifier.  This keeps the
        refiner self-contained.

        Returns the set of matching category names.
        """
        gray = patch.mean(dim=0)  # [H, W]
        h, w = gray.shape

        # Sobel-like gradients for directionality.
        gy = gray[1:, :] - gray[:-1, :]
        gx = gray[:, 1:] - gray[:, :-1]
        grad_mag = (gy[:, :min(w - 1, gy.shape[1])] ** 2 + gx[:min(h - 1, gx.shape[0]), :] ** 2).sqrt()

        edge_density = float((grad_mag > 0.08).float().mean())
        mean_grad = float(grad_mag.mean())

        # Vertical vs horizontal energy ratio — text and architecture tend
        # to be more directional than hair or foliage.
        vert_energy = float(gy.abs().mean())
        horiz_energy = float(gx.abs().mean())
        dir_ratio = max(vert_energy, horiz_energy) / max(min(vert_energy, horiz_energy), 1e-6)

        # High-frequency variance — hair and foliage have more isotropic
        # high-frequency energy than text.
        hf_var = float(grad_mag.var())

        categories: set[str] = set()

        # Text: high edge density, strong directionality, moderate variance.
        if edge_density > 0.15 and dir_ratio > 2.0 and mean_grad > 0.04:
            categories.add("text")

        # Hair: high edge density, low directionality (isotropic fine detail).
        if edge_density > 0.12 and dir_ratio < 2.5 and hf_var > 0.002:
            categories.add("hair")

        # Fabric: moderate edge density with periodic-ish structure.
        if 0.06 < edge_density < 0.25 and mean_grad > 0.02:
            categories.add("fabric")

        # Foliage: high variance, moderate-to-high edge density, isotropic.
        if hf_var > 0.003 and edge_density > 0.10 and dir_ratio < 3.0:
            categories.add("foliage")

        # Architecture: strong directionality with high edge density.
        if edge_density > 0.10 and dir_ratio > 2.5:
            categories.add("architecture")

        return categories

    # ------------------------------------------------------------------
    # Temporal sparkle guard
    # ------------------------------------------------------------------

    def _check_sparkle(
        self,
        original_frames: list[torch.Tensor],
        refined_frames: list[torch.Tensor],
    ) -> tuple[bool, float, float]:
        """Check whether refinement introduced temporal sparkle.

        Returns ``(passed, hf_flicker, patch_shimmer)`` where ``passed`` is
        True when the refined output is safe to use.
        """
        if len(original_frames) < 2 or len(refined_frames) < 2:
            return True, 0.0, 0.0

        def _hf_series(frames: list[torch.Tensor]) -> list[torch.Tensor]:
            return [self._laplacian_energy(f) for f in frames]

        orig_hf = _hf_series(original_frames)
        refined_hf = _hf_series(refined_frames)

        # Frame-to-frame HF energy variance.
        def _temporal_var(hf_maps: list[torch.Tensor]) -> float:
            energies = [float(m.mean()) for m in hf_maps]
            arr = np.array(energies, dtype=np.float64)
            return float(np.var(arr))

        orig_var = _temporal_var(orig_hf)
        refined_var = _temporal_var(refined_hf)
        hf_flicker = max(0.0, refined_var - orig_var)

        # Per-patch temporal gradient magnitude.
        shimmer_values: list[float] = []
        for i in range(1, len(refined_hf)):
            diff = (refined_hf[i] - refined_hf[i - 1]).abs()
            shimmer_values.append(float(diff.mean()))
        patch_shimmer = float(np.mean(shimmer_values)) if shimmer_values else 0.0

        passed = (
            hf_flicker <= self.max_hf_flicker
            and patch_shimmer <= self.max_patch_shimmer
        )
        return passed, hf_flicker, patch_shimmer

    # ------------------------------------------------------------------
    # Inference
    # ------------------------------------------------------------------

    @torch.inference_mode()
    def _refine_patch(self, patch: torch.Tensor) -> torch.Tensor:
        """Run the refinement model on a single spatial patch.

        Parameters
        ----------
        patch : torch.Tensor
            Shape ``[T, C, H, W]`` — the temporal stack of a single patch
            across all frames in the window.

        Returns
        -------
        torch.Tensor
            Same shape as input — the refined patch.
        """
        batched = patch.unsqueeze(0).to(self.device)  # [1, T, C, H, W]
        if self.use_half:
            batched = batched.half()

        # Build conditioning: strength scalar as a 1-element tensor.
        strength = torch.tensor(
            [self.refinement_strength], dtype=batched.dtype, device=self.device
        )

        try:
            output = self.model(batched, strength)
        except (RuntimeError, TypeError):
            # Fallback: model may not accept strength conditioning.
            try:
                output = self.model(batched)
            except (RuntimeError, TypeError):
                # Final fallback: per-frame inference.
                results = []
                for t in range(batched.shape[1]):
                    frame_input = batched[:, t : t + 1]
                    result = self.model(frame_input)
                    if isinstance(result, (tuple, list)):
                        result = result[0]
                    results.append(result)
                output = torch.cat(results, dim=1)

        if isinstance(output, (tuple, list)):
            output = output[0]
        if output.ndim == 5:
            output = output.squeeze(0)  # [T, C, H, W]
        if output.ndim != 4:
            raise RuntimeError(
                f"unexpected detail refiner output shape: {tuple(output.shape)}"
            )

        return output

    # ------------------------------------------------------------------
    # Main entry point
    # ------------------------------------------------------------------

    def run(
        self,
        manifest: Any,
        sha256_fn: Any,
    ) -> tuple[list[RefinerFrameResult], dict[str, Any]]:
        # Read all input frames.
        frame_tensors: list[torch.Tensor] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            if not source.exists():
                raise FileNotFoundError(f"missing expected frame: {source}")
            frame_tensors.append(self._read_frame(source))

        total = len(frame_tensors)
        if total == 0:
            raise ValueError("detail refiner received an empty frame list")

        _, height, width = frame_tensors[0].shape
        requested_cats = set(self.categories)
        ps = self.patch_size

        # ------- per-frame: identify qualifying patches -------
        # A patch qualifies if it has enough HF energy AND matches at
        # least one requested category.
        qualifying_patches: set[tuple[int, int]] = set()
        for tensor in frame_tensors:
            energy_map = self._laplacian_energy(tensor)
            candidates = self._patch_hf_energies(energy_map, height, width)
            for py, px, _e in candidates:
                patch = tensor[:, py : min(py + ps, height), px : min(px + ps, width)]
                cats = self._classify_patch_category(patch)
                if cats & requested_cats:
                    qualifying_patches.add((py, px))

        # ------- refine qualifying patches across all frames -------
        # Build temporal patch stacks and refine them in one shot.
        refined_tensors = [t.clone() for t in frame_tensors]
        patches_refined = 0

        for py, px in sorted(qualifying_patches):
            y_end = min(py + ps, height)
            x_end = min(px + ps, width)

            # Stack [T, C, pH, pW]
            patch_stack = torch.stack(
                [t[:, py:y_end, px:x_end] for t in frame_tensors], dim=0
            )

            refined_patch = self._refine_patch(patch_stack)

            # Blend back with strength-weighted alpha to soften transitions.
            alpha = self.refinement_strength
            for t_idx in range(total):
                original_region = refined_tensors[t_idx][:, py:y_end, px:x_end]
                blended = (
                    alpha * refined_patch[t_idx].float().cpu()
                    + (1.0 - alpha) * original_region
                )
                refined_tensors[t_idx][:, py:y_end, px:x_end] = blended

            patches_refined += 1

        # ------- sparkle guard -------
        sparkle_passed, hf_flicker, patch_shimmer = self._check_sparkle(
            frame_tensors, refined_tensors
        )

        if not sparkle_passed:
            # Roll back to pre-refinement frames.
            refined_tensors = frame_tensors

        # ------- write output -------
        written: list[RefinerFrameResult] = []
        for frame in manifest.frames:
            source = manifest.input_frames_dir / frame.file_name
            destination = manifest.output_frames_dir / frame.file_name
            out_tensor = refined_tensors[frame.index]
            self._write_frame(out_tensor, destination)

            written.append(
                RefinerFrameResult(
                    index=frame.index,
                    file_name=frame.file_name,
                    source_sha256=sha256_fn(source),
                    output_sha256=sha256_fn(destination),
                )
            )

        backend_meta: dict[str, Any] = {
            "backend": "detail-refiner",
            "model_path": str(self.model_path),
            "model_version": self.model_version,
            "weights_sha256": self.expected_weights_sha256,
            "device": str(self.device),
            "refinement_steps": self.refinement_steps,
            "refinement_strength": self.refinement_strength,
            "hf_energy_threshold": self.hf_energy_threshold,
            "categories": self.categories,
            "patch_size": self.patch_size,
            "precision": "fp16" if self.use_half else "fp32",
            "qualifying_patches": len(qualifying_patches),
            "patches_refined": patches_refined,
            "sparkle_guard_passed": sparkle_passed,
            "hf_flicker": round(hf_flicker, 6),
            "patch_shimmer": round(patch_shimmer, 6),
        }

        return written, backend_meta
