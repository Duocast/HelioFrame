from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class WorkerPassthroughTest(unittest.TestCase):
    def test_worker_echoes_frames_unchanged(self) -> None:
        with tempfile.TemporaryDirectory(prefix="hf-worker-test-") as tmp:
            root = Path(tmp)
            input_dir = root / "input-frames"
            output_dir = root / "output-frames"
            input_dir.mkdir(parents=True)

            frame_bytes = {
                "frame_000001.png": b"frame-one",
                "frame_000002.png": b"frame-two",
            }

            frames = []
            for index, (name, payload) in enumerate(frame_bytes.items()):
                (input_dir / name).write_bytes(payload)
                frames.append({"index": index, "file_name": name})

            output_manifest_path = root / "worker-output.json"
            input_manifest_path = root / "worker-input.json"

            input_manifest = {
                "schema_version": "1.0.0",
                "run_id": "run-test-001",
                "clip_id": "clip-001",
                "input_frames_dir": str(input_dir),
                "output_frames_dir": str(output_dir),
                "output_manifest_path": str(output_manifest_path),
                "frames": frames,
            }
            input_manifest_path.write_text(json.dumps(input_manifest, indent=2))

            subprocess.run(
                [sys.executable, "workers/python/worker.py", str(input_manifest_path)],
                check=True,
                cwd=Path(__file__).resolve().parents[3],
            )

            self.assertTrue(output_manifest_path.exists())
            output_manifest = json.loads(output_manifest_path.read_text())
            self.assertEqual(output_manifest["status"], "ok")
            self.assertEqual(output_manifest["frame_count"], len(frames))

            for frame in output_manifest["frames"]:
                source = input_dir / frame["file_name"]
                copied = output_dir / frame["file_name"]
                self.assertTrue(copied.exists())
                self.assertEqual(copied.read_bytes(), source.read_bytes())
                self.assertEqual(frame["source_sha256"], frame["output_sha256"])


if __name__ == "__main__":
    unittest.main()


class WorkerManifestParsingTest(unittest.TestCase):
    def test_backend_name_alias_is_supported(self) -> None:
        from workers.python.worker import load_input_manifest

        with tempfile.TemporaryDirectory(prefix="hf-worker-manifest-") as tmp:
            root = Path(tmp)
            input_dir = root / "in"
            output_dir = root / "out"
            input_dir.mkdir(parents=True)
            (input_dir / "frame_000000.png").write_bytes(b"data")
            manifest_path = root / "manifest.json"
            manifest_path.write_text(
                json.dumps(
                    {
                        "schema_version": "1.0.0",
                        "run_id": "run-test-002",
                        "clip_id": "clip-002",
                        "backend_name": "seedvr-teacher",
                        "backend_options": {"offline_only": True},
                        "input_frames_dir": str(input_dir),
                        "output_frames_dir": str(output_dir),
                        "frames": [{"index": 0, "file_name": "frame_000000.png"}],
                    }
                )
            )

            parsed = load_input_manifest(manifest_path)
            self.assertEqual(parsed.backend, "seedvr-teacher")
            self.assertEqual(parsed.backend_options["offline_only"], True)
