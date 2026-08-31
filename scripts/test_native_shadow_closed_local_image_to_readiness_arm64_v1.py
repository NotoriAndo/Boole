#!/usr/bin/env python3
"""Behavior tests for the reversible closed-local successor image path."""

from __future__ import annotations

import hashlib
import pathlib
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v1 as dev
from scripts import native_shadow_successor_produce_phase_arm64_v5 as sealed


class FakeBackend:
    def __init__(self) -> None:
        self.calls: list[str] = []

    def prepare(self, request):
        self.calls.append("prepare")
        return sealed.PreparedProduction(
            measurement={"entries": 17_677},
            build_receipt={"source": "fake"},
            state={},
        )

    def extract_kernel(self, request, prepared):
        self.calls.append("extract-kernel")
        raw = b"kernel"
        (request.outputs / "guest-kernel").write_bytes(raw)
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "kernel": {
                "name": "guest-kernel",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            },
        }

    def build_initrd(self, request, prepared):
        self.calls.append("build-initrd")
        return b"initrd"

    def build_root_disk(self, request, prepared):
        self.calls.append("build-root-disk")
        raw = b"root-disk"
        (request.outputs / "guest-root-disk").write_bytes(raw)
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "image": {
                "name": "guest-root-disk",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            },
        }

    def verify_images(self, request, prepared, kernel, initrd, root_disk):
        self.calls.append("verify-images")
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "guestBootVerified": False,
            "passed": True,
        }

    def readback(self, repository_root, outputs, chain):
        self.calls.append("readback")
        raw = (outputs / "guest-root-disk").read_bytes()
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "guestBootVerified": False,
            "image": {
                "name": "guest-root-disk",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            },
            "mayEnterQualification": True,
            "qualifiedForReplicaComparison": True,
            "status": sealed.READBACK_PASS_STATUS,
        }


def fake_chain():
    identity = sealed.FileIdentity("record.json", "0" * 64, 1)
    return types.SimpleNamespace(
        fresh_rehearsal={"measurement": {"entries": 17_677}},
        fingerprint={"status": sealed.F7_STATUS},
        identities={"P4": identity, "R3": identity, "F7": identity},
        import_identities=(),
        output_names=sealed.OUTPUT_NAMES,
    )


class ClosedLocalImageBehaviorTests(unittest.TestCase):
    def test_cli_bootstraps_its_repository_import_under_isolated_python(self):
        completed = subprocess.run(
            [
                sys.executable,
                "-I",
                "-S",
                str(
                    REPOSITORY_ROOT
                    / "scripts/native_shadow_closed_local_image_to_readiness_arm64_v1.py"
                ),
                "--help",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_development_chain_accepts_f7_without_creating_or_requiring_a7(self):
        self.assertFalse((REPOSITORY_ROOT / sealed.A7_PATH).exists())
        chain = dev.verify_development_generation_chain(REPOSITORY_ROOT)
        self.assertEqual(chain.fingerprint["status"], sealed.F7_STATUS)
        self.assertNotIn("A7", chain.identities)
        with self.assertRaisesRegex(
            sealed.SuccessorProduceV5Error, "production-authority-arm64-v7"
        ):
            sealed.verify_generation_chain(REPOSITORY_ROOT)

    def test_preflight_prepares_the_real_staging_shape_without_creating_images(self):
        backend = FakeBackend()
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            store = root / "cas"
            scratch = root / "scratch"
            outputs = root / "outputs"
            store.mkdir()
            scratch.mkdir()
            with mock.patch.object(
                dev, "verify_development_generation_chain", return_value=fake_chain()
            ):
                report = dev.preflight(
                    repository_root=REPOSITORY_ROOT,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=pathlib.Path("/usr/bin/gpgv"),
                    zstd=pathlib.Path("/usr/bin/zstd"),
                    launcher=pathlib.Path("launcher"),
                    backend=backend,
                )
            self.assertEqual(backend.calls, ["prepare"])
            self.assertFalse(outputs.exists())
            self.assertEqual(report["status"], "READY-NO-IMAGE-CREATED")
            self.assertFalse(report["authorisations"]["imageProductionAuthorised"])

    def test_build_uses_the_image_backend_but_emits_no_production_or_boot_claim(self):
        backend = FakeBackend()
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            store = root / "cas"
            scratch = root / "scratch"
            outputs = root / "outputs"
            result = root / "result.json"
            store.mkdir()
            scratch.mkdir()
            with mock.patch.object(
                dev, "verify_development_generation_chain", return_value=fake_chain()
            ):
                report = dev.build(
                    repository_root=REPOSITORY_ROOT,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    result=result,
                    gpgv=pathlib.Path("/usr/bin/gpgv"),
                    zstd=pathlib.Path("/usr/bin/zstd"),
                    launcher=pathlib.Path("launcher"),
                    run_label="unit-test",
                    backend=backend,
                )

            self.assertEqual(
                backend.calls,
                [
                    "prepare",
                    "extract-kernel",
                    "build-initrd",
                    "build-root-disk",
                    "verify-images",
                    "readback",
                ],
            )
            self.assertEqual(
                sorted(path.name for path in outputs.iterdir()),
                ["guest-initrd", "guest-kernel", "guest-root-disk"],
            )
            self.assertTrue(result.is_file())
            self.assertEqual(report["status"], "CLOSED-LOCAL-IMAGE-VERIFIED")
            self.assertEqual(report["artifactClass"], "DISPOSABLE-DEVELOPMENT")
            self.assertFalse(report["claims"]["bootVerified"])
            self.assertFalse(report["claims"]["productionRelease"])
            self.assertFalse(report["authorisations"]["activationAllowed"])
            self.assertFalse(report["authorisations"]["imageProductionAuthorised"])
            self.assertNotIn("authoritySha256", report)
            self.assertNotIn("attemptId", report)
            self.assertFalse((outputs / sealed.CONSUMED_MARKER_NAME).exists())


if __name__ == "__main__":
    unittest.main()
