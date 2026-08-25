#!/usr/bin/env python3
"""Contract tests for the ARM64 boot-artifact builder preflight."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import socket
import stat
import struct
import tempfile
import unittest
from unittest import mock

from scripts import native_shadow_boot_artifact_builder_arm64_v1 as boot


ROOT = pathlib.Path(__file__).resolve().parents[1]
TRACKED_PLAN = (
    ROOT
    / "native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json"
)
TRACKED_LOCK = (
    ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
)


def _canonical(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        indent=2,
        allow_nan=False,
    ).encode("utf-8") + b"\n"


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _arm64_image() -> bytes:
    raw = bytearray(4096)
    struct.pack_into("<Q", raw, 0x10, len(raw))
    raw[0x38:0x3C] = b"ARM\x64"
    return bytes(raw)


def _elf64_aarch64(*, dynamic: bool = False) -> bytes:
    program_count = 2 if dynamic else 1
    raw = bytearray(64 + (56 * program_count))
    raw[:16] = b"\x7fELF\x02\x01\x01" + (b"\x00" * 9)
    struct.pack_into("<H", raw, 16, 2)
    struct.pack_into("<H", raw, 18, 183)
    struct.pack_into("<I", raw, 20, 1)
    struct.pack_into("<Q", raw, 24, 0x400000)
    struct.pack_into("<Q", raw, 32, 64)
    struct.pack_into("<H", raw, 52, 64)
    struct.pack_into("<H", raw, 54, 56)
    struct.pack_into("<H", raw, 56, program_count)
    struct.pack_into("<I", raw, 64, 1)
    struct.pack_into("<I", raw, 68, 5)
    struct.pack_into("<Q", raw, 72, 0)
    struct.pack_into("<Q", raw, 80, 0x400000)
    struct.pack_into("<Q", raw, 96, len(raw))
    struct.pack_into("<Q", raw, 104, len(raw))
    struct.pack_into("<Q", raw, 112, 4096)
    if dynamic:
        struct.pack_into("<I", raw, 120, 3)
    return bytes(raw)


def _elf64_aarch64_with_needed_but_no_interp() -> bytes:
    program_count = 2
    dynamic_offset = 64 + (56 * program_count)
    raw = bytearray(dynamic_offset + 32)
    raw[:16] = b"\x7fELF\x02\x01\x01" + (b"\x00" * 9)
    struct.pack_into("<H", raw, 16, 2)
    struct.pack_into("<H", raw, 18, 183)
    struct.pack_into("<I", raw, 20, 1)
    struct.pack_into("<Q", raw, 24, 0x400000)
    struct.pack_into("<Q", raw, 32, 64)
    struct.pack_into("<H", raw, 52, 64)
    struct.pack_into("<H", raw, 54, 56)
    struct.pack_into("<H", raw, 56, program_count)

    struct.pack_into("<I", raw, 64, 1)
    struct.pack_into("<I", raw, 68, 5)
    struct.pack_into("<Q", raw, 72, 0)
    struct.pack_into("<Q", raw, 80, 0x400000)
    struct.pack_into("<Q", raw, 96, len(raw))
    struct.pack_into("<Q", raw, 104, len(raw))
    struct.pack_into("<Q", raw, 112, 4096)

    second = 64 + 56
    struct.pack_into("<I", raw, second, 2)
    struct.pack_into("<I", raw, second + 4, 6)
    struct.pack_into("<Q", raw, second + 8, dynamic_offset)
    struct.pack_into("<Q", raw, second + 16, 0x401000)
    struct.pack_into("<Q", raw, second + 32, 32)
    struct.pack_into("<Q", raw, second + 40, 32)
    struct.pack_into("<Q", raw, second + 48, 8)

    struct.pack_into("<qQ", raw, dynamic_offset, 1, 1)
    struct.pack_into("<qQ", raw, dynamic_offset + 16, 0, 0)
    return bytes(raw)


class NativeShadowBootArtifactBuilderPreflightTests(unittest.TestCase):
    def _write_regular(
        self, path: pathlib.Path, raw: bytes, *, executable: bool = False
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        path.chmod(0o755 if executable else 0o644)

    def _fixture(self, root: pathlib.Path) -> dict[str, pathlib.Path | bytes | dict[str, object]]:
        artifact_a = b"rootfs-a"
        artifact_b = b"rootfs-b-longer"
        artifacts = [
            {
                "id": "artifact-a",
                "kind": "deb",
                "sha256": _sha(artifact_a),
                "sizeBytes": len(artifact_a),
            },
            {
                "id": "artifact-b",
                "kind": "rust-dist",
                "sha256": _sha(artifact_b),
                "sizeBytes": len(artifact_b),
            },
        ]
        lock = {
            "activationAllowed": False,
            "artifacts": artifacts,
            "platform": {
                "debArchitecture": "arm64",
                "ociArchitecture": "arm64",
                "os": "linux",
                "rustTarget": "aarch64-unknown-linux-gnu",
            },
            "schema": "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1",
        }
        lock_raw = _canonical(lock)
        lock_path = root / "source-lock.json"
        lock_path.write_bytes(lock_raw)

        kernel = _arm64_image()
        pid1 = _elf64_aarch64()
        ext4_tool = b"synthetic-ext4-tool"
        kernel_path = root / "kernel.Image"
        pid1_path = root / "pid1"
        ext4_path = root / "mke2fs"
        self._write_regular(kernel_path, kernel)
        self._write_regular(pid1_path, pid1, executable=True)
        self._write_regular(ext4_path, ext4_tool, executable=True)

        plan = {
            "activationAllowed": False,
            "bootFormatVersion": 1,
            "guestDownloadMaxBytes": 2_147_483_648,
            "inputs": {
                "ext4Tool": {
                    "format": "pinned-host-executable",
                    "sha256": _sha(ext4_tool),
                    "sizeBytes": len(ext4_tool),
                },
                "kernel": {
                    "format": "linux-arm64-image",
                    "sha256": _sha(kernel),
                    "sizeBytes": len(kernel),
                },
                "pid1": {
                    "format": "elf64-aarch64-static",
                    "sha256": _sha(pid1),
                    "sizeBytes": len(pid1),
                },
            },
            "release": "SYNTHETIC-BOOT-ARTIFACT-PREFLIGHT-NOT-ACTIVATABLE",
            "rootfsSourceLock": {
                "artifactCount": len(artifacts),
                "sha256": _sha(lock_raw),
                "totalBytes": sum(int(row["sizeBytes"]) for row in artifacts),
            },
            "schema": "boole.native-shadow.boot-artifact-build-plan.arm64.v1",
        }
        plan_path = root / "plan.json"
        plan_path.write_bytes(_canonical(plan))
        cas = root / "cas"
        (cas / "sha256").mkdir(parents=True)
        return {
            "artifact_a": artifact_a,
            "artifact_b": artifact_b,
            "artifacts": artifacts,
            "cas": cas,
            "ext4": ext4_path,
            "kernel": kernel_path,
            "lock": lock_path,
            "pid1": pid1_path,
            "plan": plan_path,
        }

    def _audit(self, fixture: dict[str, object]) -> dict[str, object]:
        return boot.audit_inputs(
            pathlib.Path(fixture["plan"]),
            pathlib.Path(fixture["lock"]),
            [pathlib.Path(fixture["cas"])],
            kernel_path=pathlib.Path(fixture["kernel"]),
            pid1_path=pathlib.Path(fixture["pid1"]),
            ext4_tool_path=pathlib.Path(fixture["ext4"]),
        )

    def test_tracked_scaffold_pins_the_exact_incomplete_arm64_closure(self) -> None:
        plan = boot.load_plan(TRACKED_PLAN)
        lock_raw = TRACKED_LOCK.read_bytes()
        lock = json.loads(lock_raw)
        self.assertEqual(plan["rootfsSourceLock"]["sha256"], _sha(lock_raw))
        self.assertEqual(plan["rootfsSourceLock"]["artifactCount"], 62)
        self.assertEqual(plan["rootfsSourceLock"]["totalBytes"], 181_623_999)
        self.assertEqual(len(lock["artifacts"]), 62)
        self.assertEqual(sum(row["sizeBytes"] for row in lock["artifacts"]), 181_623_999)
        self.assertEqual(
            [plan["inputs"][name]["sha256"] for name in ("kernel", "pid1", "ext4Tool")],
            [None, None, None],
        )

    def test_missing_rootfs_inputs_are_sorted_and_never_claim_bootability(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            artifact = fixture["artifacts"][1]
            self._write_regular(
                pathlib.Path(fixture["cas"]) / "sha256" / artifact["sha256"],
                fixture["artifact_b"],
            )
            result = self._audit(fixture)
        self.assertEqual(result["status"], "BLOCKED_MISSING_INPUTS")
        self.assertEqual(result["missingArtifactIds"], ["artifact-a"])
        self.assertEqual(
            result["inputSummary"],
            {
                "expectedArtifacts": 2,
                "expectedBytes": len(fixture["artifact_a"]) + len(fixture["artifact_b"]),
                "invalidArtifacts": 0,
                "missingArtifacts": 1,
                "missingBytes": len(fixture["artifact_a"]),
                "presentArtifacts": 1,
                "presentBytes": len(fixture["artifact_b"]),
            },
        )
        self.assertEqual(result["artifactsWritten"], 0)
        self.assertIs(result["bootableClaim"], False)

    def test_complete_synthetic_inputs_reach_preflight_only(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            for artifact, key in zip(
                fixture["artifacts"], ("artifact_a", "artifact_b")
            ):
                self._write_regular(
                    pathlib.Path(fixture["cas"]) / "sha256" / artifact["sha256"],
                    fixture[key],
                )
            result = self._audit(fixture)
        self.assertEqual(result["status"], "PREFLIGHT_READY")
        self.assertEqual(result["missingArtifactIds"], [])
        self.assertEqual(result["missingPinnedInputs"], [])
        self.assertEqual(result["artifactsWritten"], 0)
        self.assertIs(result["bootableClaim"], False)

    def test_digest_size_and_symlink_artifacts_are_hard_errors(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            artifact = fixture["artifacts"][0]
            destination = pathlib.Path(fixture["cas"]) / "sha256" / artifact["sha256"]
            self._write_regular(destination, b"wrong")
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "digest/size"):
                self._audit(fixture)
            destination.unlink()
            target = pathlib.Path(raw_root) / "target"
            self._write_regular(target, fixture["artifact_a"])
            destination.symlink_to(target)
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "symlink|unsafe"):
                self._audit(fixture)

    def test_kernel_must_be_an_uncompressed_arm64_image(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            bad = b"not-an-arm64-image"
            self._write_regular(pathlib.Path(fixture["kernel"]), bad)
            plan = json.loads(pathlib.Path(fixture["plan"]).read_bytes())
            plan["inputs"]["kernel"].update(sha256=_sha(bad), sizeBytes=len(bad))
            pathlib.Path(fixture["plan"]).write_bytes(_canonical(plan))
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "ARM64 Image"):
                self._audit(fixture)

    def test_kernel_magic_without_a_nonzero_image_size_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            fake = bytearray(4096)
            fake[0x38:0x3C] = b"ARM\x64"
            raw = bytes(fake)
            self._write_regular(pathlib.Path(fixture["kernel"]), raw)
            plan = json.loads(pathlib.Path(fixture["plan"]).read_bytes())
            plan["inputs"]["kernel"].update(sha256=_sha(raw), sizeBytes=len(raw))
            pathlib.Path(fixture["plan"]).write_bytes(_canonical(plan))
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "image size"):
                self._audit(fixture)

    def test_kernel_requires_the_complete_64_byte_arm64_header(self) -> None:
        truncated = bytearray(0x3C)
        struct.pack_into("<Q", truncated, 0x10, 1)
        truncated[0x38:0x3C] = b"ARM\x64"
        with self.assertRaisesRegex(boot.BootArtifactPreflightError, "ARM64 Image"):
            boot._validate_arm64_image(bytes(truncated))

    def test_kernel_effective_image_size_may_exceed_the_file_length(self) -> None:
        raw = bytearray(_arm64_image())
        struct.pack_into("<Q", raw, 0x10, len(raw) + 4096)
        boot._validate_arm64_image(bytes(raw))

    def test_pid1_must_be_static_elf64_aarch64(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            dynamic = _elf64_aarch64(dynamic=True)
            self._write_regular(pathlib.Path(fixture["pid1"]), dynamic, executable=True)
            plan = json.loads(pathlib.Path(fixture["plan"]).read_bytes())
            plan["inputs"]["pid1"].update(
                sha256=_sha(dynamic), sizeBytes=len(dynamic)
            )
            pathlib.Path(fixture["plan"]).write_bytes(_canonical(plan))
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "PT_INTERP|static"):
                self._audit(fixture)

    def test_pid1_requires_an_entrypoint_in_an_executable_load_segment(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            invalid = bytearray(_elf64_aarch64())
            struct.pack_into("<Q", invalid, 24, 0)
            raw = bytes(invalid)
            self._write_regular(pathlib.Path(fixture["pid1"]), raw, executable=True)
            plan = json.loads(pathlib.Path(fixture["plan"]).read_bytes())
            plan["inputs"]["pid1"].update(sha256=_sha(raw), sizeBytes=len(raw))
            pathlib.Path(fixture["plan"]).write_bytes(_canonical(plan))
            with self.assertRaisesRegex(
                boot.BootArtifactPreflightError, "entrypoint|executable PT_LOAD"
            ):
                self._audit(fixture)

    def test_pid1_rejects_dt_needed_even_without_pt_interp(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            dynamic = _elf64_aarch64_with_needed_but_no_interp()
            self._write_regular(pathlib.Path(fixture["pid1"]), dynamic, executable=True)
            plan = json.loads(pathlib.Path(fixture["plan"]).read_bytes())
            plan["inputs"]["pid1"].update(
                sha256=_sha(dynamic), sizeBytes=len(dynamic)
            )
            pathlib.Path(fixture["plan"]).write_bytes(_canonical(plan))
            with self.assertRaisesRegex(
                boot.BootArtifactPreflightError, "DT_NEEDED|static"
            ):
                self._audit(fixture)

    def test_audit_has_no_network_path(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            with mock.patch.object(
                socket,
                "socket",
                side_effect=AssertionError("network access attempted"),
            ):
                result = self._audit(fixture)
        self.assertEqual(result["status"], "BLOCKED_MISSING_INPUTS")

    def test_cas_parent_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = pathlib.Path(raw_root)
            fixture = self._fixture(root)
            real_parent = root / "real-parent"
            real_parent.mkdir()
            pathlib.Path(fixture["cas"]).rename(real_parent / "cas")
            alias = root / "alias-parent"
            alias.symlink_to(real_parent, target_is_directory=True)
            fixture["cas"] = alias / "cas"
            with self.assertRaisesRegex(
                boot.BootArtifactPreflightError, "symlink|unsafe"
            ):
                self._audit(fixture)

    def test_opened_cas_descriptors_close_if_a_later_root_is_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            with mock.patch.object(
                boot,
                "_cas_sha_directory",
                side_effect=[101, boot.BootArtifactPreflightError("later root invalid")],
            ), mock.patch.object(boot.os, "close") as close:
                with self.assertRaisesRegex(
                    boot.BootArtifactPreflightError, "later root invalid"
                ):
                    boot.audit_inputs(
                        pathlib.Path(fixture["plan"]),
                        pathlib.Path(fixture["lock"]),
                        [pathlib.Path("first"), pathlib.Path("second")],
                    )
        close.assert_any_call(101)

    def test_unpinned_boot_inputs_are_reported_without_opening_paths(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            plan = json.loads(pathlib.Path(fixture["plan"]).read_bytes())
            for name in ("kernel", "pid1", "ext4Tool"):
                plan["inputs"][name]["sha256"] = None
                plan["inputs"][name]["sizeBytes"] = None
            pathlib.Path(fixture["plan"]).write_bytes(_canonical(plan))
            for name in ("kernel", "pid1", "ext4"):
                pathlib.Path(fixture[name]).unlink()
            result = self._audit(fixture)
        self.assertEqual(
            result["missingPinnedInputs"], ["ext4Tool", "kernel", "pid1"]
        )
        self.assertEqual(result["status"], "BLOCKED_MISSING_INPUTS")
        self.assertIs(result["bootableClaim"], False)


if __name__ == "__main__":
    unittest.main()
