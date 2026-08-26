#!/usr/bin/env python3
"""Tests for handing the arm64 produce phase a launcher it can place.

The double build proves the launcher is reproducible and keeps only the digest;
the produce phase needs the bytes.  The sealed producer authority already says
how they may arrive -- ``rebuild-and-match-seal`` -- so this is a rebuild that
is believed only because it reproduces a digest two other things already sealed,
and refused otherwise.

Nothing here compiles anything: the real build is an aarch64 cargo run on the
runner.  These tests drive the acceptance with an injected builder, because what
must hold is the refusal, not the compiler.
"""

from __future__ import annotations

import json
import os
import pathlib
import tempfile
import unittest

from scripts import native_shadow_launcher_build_arm64_v1 as build
from scripts import native_shadow_launcher_emit_arm64_v1 as emit
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot


PRODUCER_AUTHORITY_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)
BUILD_RESULT_PATH = pathlib.Path(
    "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
)
MODULE_PATH = pathlib.Path("scripts/native_shadow_launcher_emit_arm64_v1.py")


class SealAgreementTests(unittest.TestCase):
    """Two records sealed the same launcher; a rebuild answers to both."""

    def test_the_producer_authority_and_the_build_result_agree(self) -> None:
        producer = json.loads(PRODUCER_AUTHORITY_PATH.read_text(encoding="utf-8"))
        built = json.loads(BUILD_RESULT_PATH.read_text(encoding="utf-8"))
        self.assertEqual(producer["launcher"]["sha256"], built["launcher"]["sha256"])
        self.assertEqual(
            producer["launcher"]["sizeBytes"], built["launcher"]["sizeBytes"]
        )
        self.assertEqual(
            producer["launcher"]["guestLogicalPath"],
            built["launcher"]["guestLogicalPath"],
        )

    def test_the_agreement_is_checked_rather_than_assumed(self) -> None:
        self.assertEqual(emit.sealed_digest(), boot.LAUNCHER_SHA256)

    def test_the_digest_is_not_restated_in_this_module(self) -> None:
        self.assertNotIn(boot.LAUNCHER_SHA256, MODULE_PATH.read_text(encoding="utf-8"))

    def test_the_acquisition_rule_is_the_authority_s(self) -> None:
        producer = json.loads(PRODUCER_AUTHORITY_PATH.read_text(encoding="utf-8"))
        self.assertEqual(
            producer["launcher"]["acquisition"], boot.LAUNCHER_ACQUISITION
        )


class EmitTests(unittest.TestCase):
    def sealed(self) -> bytes:
        """Bytes the seal would accept, with the seal moved to meet them.

        The launcher ELF is an aarch64 build product and is not in this
        repository, so the acceptance is driven by naming the digest of a
        stand-in rather than by forging a 2 MB binary.
        """

        return b"\x7fELF-stand-in-for-the-rebuilt-launcher"

    def test_bytes_that_reproduce_the_seal_are_written(self) -> None:
        raw = self.sealed()
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            emit.emit(
                path,
                builder=lambda: raw,
                sha256=boot.sha256_hex(raw),
                size=len(raw),
            )
            self.assertEqual(path.read_bytes(), raw)

    def test_what_is_written_is_executable(self) -> None:
        raw = self.sealed()
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            emit.emit(
                path,
                builder=lambda: raw,
                sha256=boot.sha256_hex(raw),
                size=len(raw),
            )
            self.assertEqual(path.stat().st_mode & 0o777, 0o755)

    def test_a_rebuild_that_does_not_reproduce_the_seal_is_refused(self) -> None:
        raw = self.sealed()
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            with self.assertRaises(emit.LauncherEmitError) as caught:
                emit.emit(path, builder=lambda: raw, sha256="0" * 64, size=len(raw))
            self.assertIn(boot.ABORT_LAUNCHER_DIGEST_MISMATCH, str(caught.exception))
            self.assertFalse(path.exists())

    def test_a_rebuild_of_the_wrong_size_is_refused_on_its_own(self) -> None:
        raw = self.sealed()
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            with self.assertRaises(emit.LauncherEmitError) as caught:
                emit.emit(
                    path,
                    builder=lambda: raw,
                    sha256=boot.sha256_hex(raw),
                    size=len(raw) + 1,
                )
            self.assertIn(boot.ABORT_LAUNCHER_DIGEST_MISMATCH, str(caught.exception))

    def test_a_rebuild_that_produced_nothing_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            with self.assertRaises(emit.LauncherEmitError):
                emit.emit(path, builder=lambda: b"", sha256="0" * 64, size=0)

    def test_an_existing_launcher_is_never_overwritten(self) -> None:
        raw = self.sealed()
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            path.write_bytes(b"someone-else-was-here")
            with self.assertRaises(emit.LauncherEmitError):
                emit.emit(
                    path,
                    builder=lambda: raw,
                    sha256=boot.sha256_hex(raw),
                    size=len(raw),
                )
            self.assertEqual(path.read_bytes(), b"someone-else-was-here")

    def test_the_default_seal_is_the_sealed_one(self) -> None:
        raw = self.sealed()
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "boole-native-shadow-launcher"
            with self.assertRaises(emit.LauncherEmitError):
                emit.emit(path, builder=lambda: raw)


class HostTests(unittest.TestCase):
    """A host that cannot produce this binary is told so, not shown a mismatch."""

    def test_a_host_that_is_not_arm64_linux_is_refused_before_anything_is_built(
        self,
    ) -> None:
        uname = os.uname()
        if uname.sysname == "Linux" and uname.machine in {"aarch64", "arm64"}:
            self.skipTest("this host can build the launcher; the refusal is not visible")

        def explode(*args: object, **kwargs: object) -> None:
            raise AssertionError(
                "the build started on a host that cannot produce this binary"
            )

        original = build.prefetch
        build.prefetch = explode
        try:
            with self.assertRaises(build.LauncherBuildError) as caught:
                emit.rebuild()
        finally:
            build.prefetch = original
        # Without this, a wrong-arch host compiles a wrong-arch binary and the
        # failure arrives as `launcher-digest-mismatch`, which is a report about
        # the launcher rather than about the host.
        self.assertIn("aarch64", str(caught.exception))


class BoundaryTests(unittest.TestCase):
    def test_emitting_a_launcher_is_not_a_deployment_or_a_boot(self) -> None:
        self.assertFalse(emit.BOOTABLE_CLAIM)
        self.assertFalse(emit.ACTIVATION_ALLOWED)

    def test_the_sealed_boundaries_still_say_nothing_was_deployed(self) -> None:
        producer = json.loads(PRODUCER_AUTHORITY_PATH.read_text(encoding="utf-8"))
        self.assertFalse(producer["boundaries"]["launcherDeployedIntoGuest"])
        self.assertFalse(producer["boundaries"]["guestBootVerified"])


if __name__ == "__main__":
    unittest.main()
