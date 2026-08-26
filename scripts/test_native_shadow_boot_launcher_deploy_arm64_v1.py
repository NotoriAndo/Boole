#!/usr/bin/env python3
"""Tests for placing the rebuilt launcher into the assembled arm64 guest tree.

``/usr/libexec/boole`` is a declared closure root that no package fills: the
launcher is a build product, not a repo file, so it cannot arrive as a tracked
authority file and it cannot arrive as a deb.  The sealed producer authority
says how it may arrive instead -- ``rebuild-and-match-seal`` -- and names the
digest, the size and the path it lands on.  These tests pin that the projection
honours that seal rather than restating it, and that a launcher which does not
match the seal stops the build with the authority's own abort id.
"""

from __future__ import annotations

import json
import pathlib
import unittest

from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot


PRODUCER_AUTHORITY_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)
PROJECTION_PATH = pathlib.Path(
    "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py"
)


def sealed_launcher() -> dict:
    document = json.loads(PRODUCER_AUTHORITY_PATH.read_text(encoding="utf-8"))
    return document["launcher"]


class SealTests(unittest.TestCase):
    def test_the_seal_is_read_from_the_authority_not_restated(self) -> None:
        sealed = sealed_launcher()
        self.assertEqual(boot.LAUNCHER_SHA256, sealed["sha256"])
        self.assertEqual(boot.LAUNCHER_SIZE_BYTES, sealed["sizeBytes"])
        self.assertEqual(boot.LAUNCHER_GUEST_PATH, sealed["guestLogicalPath"])

    def test_the_digest_is_not_a_second_copy_in_the_projection_source(self) -> None:
        source = PROJECTION_PATH.read_text(encoding="utf-8")
        self.assertNotIn(sealed_launcher()["sha256"], source)

    def test_the_abort_id_is_the_authority_s_own(self) -> None:
        document = json.loads(PRODUCER_AUTHORITY_PATH.read_text(encoding="utf-8"))
        ids = {row["id"] for row in document["abortConditions"]}
        self.assertIn(boot.ABORT_LAUNCHER_DIGEST_MISMATCH, ids)

    def test_the_acquisition_rule_is_rebuild_and_match_seal(self) -> None:
        self.assertEqual(sealed_launcher()["acquisition"], "rebuild-and-match-seal")


class EntryTests(unittest.TestCase):
    def matching_binary(self) -> bytes:
        """Bytes that hash to the seal are not available here; forge the check.

        The launcher ELF is a CI build product and is not in the repository, so
        these tests drive the verification with a stand-in seal rather than a
        stand-in binary -- the point under test is the refusal, not the ELF.
        """

        return b"launcher-elf-stand-in"

    def test_a_binary_that_matches_the_seal_lands_on_the_guest_path(self) -> None:
        raw = self.matching_binary()
        entry = boot.launcher_entry(raw, sha256=boot.sha256_hex(raw), size=len(raw))
        self.assertEqual(entry["path"], boot.LAUNCHER_GUEST_PATH.lstrip("/"))
        self.assertEqual(entry["kind"], "file")
        self.assertEqual(entry["raw"], raw)

    def test_it_lands_owned_by_root_and_executable(self) -> None:
        raw = self.matching_binary()
        entry = boot.launcher_entry(raw, sha256=boot.sha256_hex(raw), size=len(raw))
        self.assertEqual(entry["uid"], 0)
        self.assertEqual(entry["gid"], 0)
        self.assertEqual(entry["mode"], 0o755)

    def test_a_binary_whose_digest_differs_is_refused(self) -> None:
        raw = self.matching_binary()
        with self.assertRaises(boot.RootfsBuildError) as caught:
            boot.launcher_entry(raw, sha256="0" * 64, size=len(raw))
        self.assertIn(boot.ABORT_LAUNCHER_DIGEST_MISMATCH, str(caught.exception))

    def test_a_binary_whose_size_differs_is_refused_on_its_own(self) -> None:
        raw = self.matching_binary()
        with self.assertRaises(boot.RootfsBuildError) as caught:
            boot.launcher_entry(raw, sha256=boot.sha256_hex(raw), size=len(raw) + 1)
        self.assertIn(boot.ABORT_LAUNCHER_DIGEST_MISMATCH, str(caught.exception))

    def test_an_empty_binary_is_refused(self) -> None:
        with self.assertRaises(boot.RootfsBuildError):
            boot.launcher_entry(b"", sha256=boot.sha256_hex(b""), size=0)

    def test_the_default_seal_is_the_sealed_one(self) -> None:
        raw = self.matching_binary()
        with self.assertRaises(boot.RootfsBuildError) as caught:
            boot.launcher_entry(raw)
        self.assertIn(boot.ABORT_LAUNCHER_DIGEST_MISMATCH, str(caught.exception))


class AssemblyTests(unittest.TestCase):
    def test_build_oci_layout_takes_the_launcher_by_keyword(self) -> None:
        import inspect

        signature = inspect.signature(boot.build_oci_layout)
        parameter = signature.parameters.get("launcher_binary")
        self.assertIsNotNone(parameter)
        self.assertEqual(parameter.kind, inspect.Parameter.KEYWORD_ONLY)

    def test_the_closure_root_it_fills_is_the_one_the_lock_declares(self) -> None:
        lock = json.loads(
            pathlib.Path(
                "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        roots = {root for row in lock["closureRoots"] for root in row["logicalRoots"]}
        parent = boot.LAUNCHER_GUEST_PATH.rsplit("/", 1)[0]
        self.assertIn(parent, roots)


if __name__ == "__main__":
    unittest.main()
