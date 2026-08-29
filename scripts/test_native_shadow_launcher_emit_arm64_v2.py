#!/usr/bin/env python3
"""Fail-closed tests for emitting the sealed arm64 launcher-v2 ELF.

The two-build result seals bytes but does not track the 2 MiB ELF in Git.  The
emitter is the narrow bridge that may rebuild those bytes for a later image
producer.  It is not an image producer and it grants no boot authority.
"""

from __future__ import annotations

import json
import inspect
import os
import pathlib
import tempfile
import unittest
from typing import Optional
from unittest import mock

from scripts import native_shadow_launcher_build_arm64_v2 as build
from scripts import native_shadow_launcher_emit_arm64_v2 as emit


MODULE_PATH = pathlib.Path("scripts/native_shadow_launcher_emit_arm64_v2.py")
WORKFLOW_PATH = pathlib.Path(".github/workflows/ci.yml")
SELF_TEST_PATH = pathlib.Path("scripts/self-test.sh")
DOCS_SMOKE_PATH = pathlib.Path("scripts/docs-smoke.sh")
SUCCESSOR_PRODUCER_PATHS = (
    pathlib.Path("scripts/native_shadow_successor_produce_phase_arm64_v2.py"),
    pathlib.Path(".github/workflows/native-shadow-successor-produce-arm64.yml"),
)
SEALED_RESULT_SHA256 = (
    "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08"
)
SEALED_LAUNCHER_SHA256 = (
    "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"
)


def stand_in_seal(raw: bytes) -> dict[str, object]:
    record = json.loads(build.RESULT_PATH.read_text(encoding="utf-8"))
    record["launcher"] = dict(record["launcher"])
    record["launcher"]["sha256"] = build.sha256_bytes(raw)
    record["launcher"]["sizeBytes"] = len(raw)
    return record


class SealTests(unittest.TestCase):
    def test_sealed_result_is_canonical_and_exactly_pinned(self) -> None:
        raw = build.RESULT_PATH.read_bytes()
        self.assertEqual(build.sha256_bytes(raw), SEALED_RESULT_SHA256)
        self.assertEqual(build.v1.canonical_json(json.loads(raw)), raw)
        self.assertEqual(emit.load_seal(), json.loads(raw))

    def test_seal_is_reconstructed_not_only_parsed(self) -> None:
        record = emit.load_seal()
        expected = build.build_result(
            build.load_authority(),
            built={
                "buildCount": record["independentBuildCount"],
                "producerPathHits": record["producerPathHits"],
                "sha256": record["launcher"]["sha256"],
                "sizeBytes": record["launcher"]["sizeBytes"],
            },
            identity=record["observedToolchain"],
        )
        self.assertEqual(record, expected)

    def test_seal_has_only_zero_counts_for_every_declared_path_class(self) -> None:
        record = emit.load_seal()
        expected = set(
            build.load_authority()["determinism"]["producerPathClasses"]
        )
        self.assertEqual(set(record["producerPathHits"]), expected)
        self.assertEqual(set(record["producerPathHits"].values()), {0})

    def test_emitter_pins_the_result_not_a_second_copy_of_the_elf_digest(self) -> None:
        source = MODULE_PATH.read_text(encoding="utf-8")
        self.assertIn(SEALED_RESULT_SHA256, source)
        self.assertNotIn(SEALED_LAUNCHER_SHA256, source)


class EmitTests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = b"\x7fELF-launcher-v2-stand-in"
        self.seal = stand_in_seal(self.raw)

    @staticmethod
    def temporary_path(path: pathlib.Path) -> pathlib.Path:
        return path.with_name(f".{path.name}.boole-v2-new")

    def run_emit(
        self,
        path: pathlib.Path,
        *,
        raw: Optional[bytes] = None,
        seal: Optional[dict[str, object]] = None,
    ) -> dict[str, object]:
        with mock.patch.object(
            emit, "load_seal", return_value=self.seal if seal is None else seal
        ), mock.patch.object(
            emit, "rebuild", return_value=self.raw if raw is None else raw
        ):
            return emit.emit(path)

    def test_public_emit_api_has_no_builder_or_seal_bypass(self) -> None:
        self.assertEqual(list(inspect.signature(emit.emit).parameters), ["path"])

    def test_exact_bytes_are_published_executable_once(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher-v2"
            result = self.run_emit(path)
            self.assertEqual(path.read_bytes(), self.raw)
            self.assertEqual(path.stat().st_mode & 0o777, 0o755)
            self.assertEqual(result["sha256"], build.sha256_bytes(self.raw))
            self.assertEqual(result["sizeBytes"], len(self.raw))
            self.assertFalse(result["activationAllowed"])
            self.assertFalse(result["bootableClaim"])
            self.assertFalse(result["launcherDeployedIntoGuest"])

    def test_wrong_digest_is_refused_without_a_final_or_temporary_file(self) -> None:
        changed = dict(self.seal)
        changed["launcher"] = dict(self.seal["launcher"])
        changed["launcher"]["sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            with self.assertRaises(emit.LauncherV2EmitError):
                self.run_emit(path, seal=changed)
            self.assertFalse(os.path.lexists(path))
            self.assertFalse(os.path.lexists(self.temporary_path(path)))

    def test_wrong_size_is_refused_without_a_final_or_temporary_file(self) -> None:
        changed = dict(self.seal)
        changed["launcher"] = dict(self.seal["launcher"])
        changed["launcher"]["sizeBytes"] = len(self.raw) + 1
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            with self.assertRaises(emit.LauncherV2EmitError):
                self.run_emit(path, seal=changed)
            self.assertFalse(os.path.lexists(path))
            self.assertFalse(os.path.lexists(self.temporary_path(path)))

    def test_malformed_seal_is_refused_before_build(self) -> None:
        changed = dict(self.seal)
        changed["unexpected"] = False
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            with mock.patch.object(emit, "load_seal", return_value=changed), mock.patch.object(
                emit, "rebuild", side_effect=AssertionError("build must not start")
            ) as builder:
                with self.assertRaises(emit.LauncherV2EmitError):
                    emit.emit(path)
                builder.assert_not_called()

    def test_empty_build_is_refused_before_any_publication(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            with self.assertRaises(emit.LauncherV2EmitError):
                self.run_emit(path, raw=b"")
            self.assertFalse(os.path.lexists(path))

    def test_existing_final_is_never_overwritten_or_rebuilt(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            path.write_bytes(b"existing")
            with mock.patch.object(emit, "load_seal") as loader, mock.patch.object(
                emit, "rebuild"
            ) as builder, self.assertRaises(emit.LauncherV2EmitError):
                emit.emit(path)
            self.assertEqual(path.read_bytes(), b"existing")
            loader.assert_not_called()
            builder.assert_not_called()

    def test_dangling_final_symlink_is_also_an_existing_target(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            path.symlink_to(pathlib.Path(scratch) / "missing")
            with mock.patch.object(emit, "load_seal") as loader, mock.patch.object(
                emit, "rebuild"
            ) as builder, self.assertRaises(emit.LauncherV2EmitError):
                emit.emit(path)
            self.assertTrue(path.is_symlink())
            loader.assert_not_called()
            builder.assert_not_called()

    def test_existing_temporary_file_is_never_reused(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            temporary = self.temporary_path(path)
            temporary.write_bytes(b"stale")
            with mock.patch.object(emit, "load_seal") as loader, mock.patch.object(
                emit, "rebuild"
            ) as builder, self.assertRaises(emit.LauncherV2EmitError):
                emit.emit(path)
            self.assertEqual(temporary.read_bytes(), b"stale")
            self.assertFalse(path.exists())
            loader.assert_not_called()
            builder.assert_not_called()

    def test_publish_failure_removes_new_temporary_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            temporary = self.temporary_path(path)
            with mock.patch.object(emit.os, "link", side_effect=OSError("no link")):
                with self.assertRaises(OSError):
                    self.run_emit(path)
            self.assertFalse(os.path.lexists(path))
            self.assertFalse(os.path.lexists(temporary))

    def test_concurrent_final_is_preserved_and_never_replaced(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "launcher"
            temporary = self.temporary_path(path)

            def race(*args: object, **kwargs: object) -> None:
                path.write_bytes(b"concurrent-writer")
                raise FileExistsError("the final name raced into existence")

            with mock.patch.object(emit.os, "link", side_effect=race):
                with self.assertRaises(FileExistsError):
                    self.run_emit(path)
            self.assertEqual(path.read_bytes(), b"concurrent-writer")
            self.assertFalse(os.path.lexists(temporary))

    def test_output_without_a_filename_is_a_typed_refusal_before_build(self) -> None:
        with mock.patch.object(emit, "load_seal") as loader, mock.patch.object(
            emit, "rebuild"
        ) as builder, self.assertRaises(emit.LauncherV2EmitError):
            emit.emit(pathlib.Path("/"))
        loader.assert_not_called()
        builder.assert_not_called()

    def test_relative_output_is_refused_before_build(self) -> None:
        with mock.patch.object(emit, "load_seal") as loader, mock.patch.object(
            emit, "rebuild"
        ) as builder, self.assertRaises(emit.LauncherV2EmitError):
            emit.emit(pathlib.Path("relative-launcher"))
        loader.assert_not_called()
        builder.assert_not_called()

    def test_symlinked_parent_is_refused_before_build(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            real = root / "real"
            real.mkdir()
            alias = root / "alias"
            alias.symlink_to(real, target_is_directory=True)
            with mock.patch.object(emit, "load_seal") as loader, mock.patch.object(
                emit, "rebuild"
            ) as builder, self.assertRaises(emit.LauncherV2EmitError):
                emit.emit(alias / "launcher")
            loader.assert_not_called()
            builder.assert_not_called()


class RebuildTests(unittest.TestCase):
    def test_wrong_host_is_refused_before_build_once(self) -> None:
        uname = os.uname()
        if uname.sysname == "Linux" and uname.machine in {"aarch64", "arm64"}:
            self.skipTest("this host is the authorised build platform")
        with mock.patch.object(
            build,
            "build_once",
            side_effect=AssertionError("build started before host refusal"),
        ):
            with self.assertRaises(build.LauncherV2BuildError):
                emit.rebuild()

    def test_rebuild_refuses_nonzero_or_misshapen_path_scan(self) -> None:
        expected = {
            label: 0
            for label in build.load_authority()["determinism"][
                "producerPathClasses"
            ]
        }
        for changed in (
            {**expected, "source-root": 1},
            {key: value for key, value in expected.items() if key != "source-root"},
            {**expected, "invented-root": 0},
        ):
            with self.subTest(changed=changed):
                with self.assertRaises(emit.LauncherV2EmitError):
                    emit.validate_path_scan(changed)


class WiringAndBoundaryTests(unittest.TestCase):
    def test_ci_rebuilds_through_the_emitter_after_reproving_the_result(self) -> None:
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        job = workflow.split("  native-shadow-launcher-build-arm64-v2:\n", 1)[1]
        job = job.split("\n  self-test:\n", 1)[0]
        reprove = job.index("native_shadow_launcher_build_arm64_v2.py --build")
        tracked = job.index('git ls-files --error-unmatch -- "$result"')
        emitted = job.index("native_shadow_launcher_emit_arm64_v2.py emit")
        self.assertLess(reprove, tracked)
        self.assertLess(tracked, emitted)
        self.assertIn('actual_sha="$(sha256sum "$emitted" | awk', job)
        self.assertIn('test "$actual_sha" = "$expected_sha"', job)
        self.assertIn('test "$actual_size" = "$expected_size"', job)
        self.assertNotIn("continue-on-error", job)
        self.assertNotIn("|| true", job)

    def test_self_test_runs_the_emitter_contract(self) -> None:
        self.assertIn(
            "scripts/test_native_shadow_launcher_emit_arm64_v2.py",
            SELF_TEST_PATH.read_text(encoding="utf-8"),
        )

    def test_document_gate_pins_the_emitter_and_atomic_publication(self) -> None:
        smoke = DOCS_SMOKE_PATH.read_text(encoding="utf-8")
        self.assertIn("native_shadow_launcher_emit_arm64_v2.py", smoke)
        self.assertIn(SEALED_RESULT_SHA256, smoke)
        self.assertIn('src_dir_fd=directory', smoke)
        self.assertIn('dst_dir_fd=directory', smoke)
        self.assertIn('os.fsync(directory)', smoke)
        self.assertIn('test "$actual_sha" = "$expected_sha"', smoke)
        self.assertIn('test "$actual_size" = "$expected_size"', smoke)

    def test_no_image_or_boot_authority_is_opened(self) -> None:
        self.assertFalse(emit.BOOTABLE_CLAIM)
        self.assertFalse(emit.ACTIVATION_ALLOWED)
        self.assertFalse(emit.LAUNCHER_DEPLOYED_INTO_GUEST)
        result = emit.load_seal()
        self.assertTrue(all(value is False for value in result["boundaries"].values()))

    def test_no_successor_image_producer_imports_the_emitter(self) -> None:
        for path in SUCCESSOR_PRODUCER_PATHS:
            with self.subTest(path=path):
                self.assertNotIn(
                    "native_shadow_launcher_emit_arm64_v2",
                    path.read_text(encoding="utf-8"),
                )


if __name__ == "__main__":
    unittest.main()
