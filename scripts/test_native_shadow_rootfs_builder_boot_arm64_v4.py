#!/usr/bin/env python3
"""RED contract for the launcher-v2 boot-rootfs builder projection.

The v3 builder is the last sealed assembler used by the successful historical
image.  The next projection may change exactly one authority: the launcher seal
from v1 to v2.  It must do so in a fresh derived namespace rather than changing
v3's globals, tables, closures or bytes in place.

The launcher-v2 executable is intentionally not a repository blob.  The exact
positive byte check below therefore substitutes only the digest observation in
the *fresh v4 namespace* for the duration of one test.  Size, sealed identity,
guest metadata and every production call path remain the real ones.  The two
negative checks use the real digest function.
"""

from __future__ import annotations

import hashlib
import importlib
import inspect
import json
import pathlib
import unittest
from unittest import mock

from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as builder_v1
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as builder_v3


REPO = pathlib.Path(__file__).resolve().parents[1]
MODULE_NAME = "scripts.native_shadow_rootfs_builder_boot_arm64_v4"
V3_PATH = pathlib.Path(builder_v3.__file__).resolve()
V3_SHA256 = "93bd05d06e43cc69f325036d204b7b57721e358dd5c5d5990227ef88c4de8c39"
V3_SIZE_BYTES = 12505
V1_RESULT_PATH = (
    REPO / "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
)
V2_RESULT_PATH = (
    REPO / "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
)


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


# Import the predecessor first and remember the live objects.  A successor that
# mutates it while importing is caught after the import, not hidden by taking the
# snapshot afterwards.
V3_IMPL = builder_v3._IMPL
V3_LAUNCHER_ENTRY = builder_v3.__getattr__("launcher_entry")
V3_EXPECTED_AUTHORITY_FILES = builder_v3.__getattr__("EXPECTED_AUTHORITY_FILES")
V3_EXPECTED_CLOSURE_ROOTS = builder_v3.__getattr__(
    "EXPECTED_PROVENANCE_CLOSURE_ROOTS"
)
V3_REQUIRED_CLOSURES = builder_v3.__getattr__("REQUIRED_PROVENANCE_CLOSURES")

try:
    successor = importlib.import_module(MODULE_NAME)
except ModuleNotFoundError as exc:
    if exc.name != MODULE_NAME:
        raise
    successor = None


class BuilderExistenceTests(unittest.TestCase):
    def test_the_launcher_v2_builder_projection_exists(self) -> None:
        self.assertIsNotNone(
            successor,
            "scripts/native_shadow_rootfs_builder_boot_arm64_v4.py is absent",
        )


@unittest.skipIf(successor is None, "launcher-v2 builder projection is not implemented")
class PredecessorSealTests(unittest.TestCase):
    def test_the_predecessor_is_the_exact_v3_file(self) -> None:
        self.assertEqual(successor.BOOT_V3.resolve(), V3_PATH)
        self.assertEqual(successor.BOOT_V3_SHA256, V3_SHA256)
        self.assertEqual(digest(V3_PATH), V3_SHA256)
        self.assertEqual(V3_PATH.stat().st_size, V3_SIZE_BYTES)

    def test_the_projection_records_its_own_live_bytes(self) -> None:
        path = pathlib.Path(successor.__file__).resolve()
        self.assertEqual(successor.SUCCESSOR_PROJECTION_SHA256, digest(path))


@unittest.skipIf(successor is None, "launcher-v2 builder projection is not implemented")
class FreshNamespaceTests(unittest.TestCase):
    def test_the_successor_executes_in_a_fresh_namespace(self) -> None:
        self.assertIsNot(successor._IMPL, V3_IMPL)
        self.assertEqual(
            successor._IMPL["__name__"],
            "scripts.native_shadow_rootfs_builder_boot_arm64_v4_impl",
        )

    def test_build_and_assembly_functions_resolve_through_that_namespace(self) -> None:
        for name in ("build_oci_layout", "verify_oci_layout", "_assemble_entries"):
            function = successor.__getattr__(name)
            with self.subTest(name=name):
                self.assertIs(function.__globals__, successor._IMPL)

    def test_materialization_reaches_the_same_namespace_as_build(self) -> None:
        self.assertIs(
            successor.materialize_staging_tree.__globals__["_IMPL"],
            successor._IMPL,
        )
        self.assertIs(
            successor.__getattr__("build_oci_layout").__globals__,
            successor.materialize_staging_tree.__globals__["_IMPL"],
        )

    def test_the_new_launcher_gate_lives_in_the_fresh_namespace(self) -> None:
        launcher_entry = successor.__getattr__("launcher_entry")
        self.assertIs(launcher_entry, successor._IMPL["launcher_entry"])
        self.assertIs(launcher_entry.__globals__, successor._IMPL)
        self.assertIsNot(launcher_entry, V3_LAUNCHER_ENTRY)


@unittest.skipIf(successor is None, "launcher-v2 builder projection is not implemented")
class NoPredecessorMonkeypatchTests(unittest.TestCase):
    def test_import_did_not_replace_the_predecessor_namespace_or_launcher_gate(self) -> None:
        self.assertIs(builder_v3._IMPL, V3_IMPL)
        self.assertIs(builder_v3.__getattr__("launcher_entry"), V3_LAUNCHER_ENTRY)
        self.assertIs(
            builder_v3.__getattr__("EXPECTED_AUTHORITY_FILES"),
            V3_EXPECTED_AUTHORITY_FILES,
        )
        self.assertIs(
            builder_v3.__getattr__("EXPECTED_PROVENANCE_CLOSURE_ROOTS"),
            V3_EXPECTED_CLOSURE_ROOTS,
        )

    def test_import_did_not_change_the_predecessors_launcher_identity(self) -> None:
        v1 = load(V1_RESULT_PATH)["launcher"]
        globals_ = V3_LAUNCHER_ENTRY.__globals__
        self.assertEqual(globals_["LAUNCHER_SHA256"], v1["sha256"])
        self.assertEqual(
            globals_["LAUNCHER_SIZE_BYTES"], v1["sizeBytes"]
        )
        self.assertEqual(
            globals_["LAUNCHER_GUEST_PATH"], v1["guestLogicalPath"]
        )

    def test_successor_source_contains_no_assignment_into_predecessor_globals(self) -> None:
        source = pathlib.Path(successor.__file__).read_text(encoding="utf-8")
        for forbidden in (
            "builder_v3._IMPL[",
            "boot_v3._IMPL[",
            "setattr(builder_v3",
            "setattr(boot_v3",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, source)


@unittest.skipIf(successor is None, "launcher-v2 builder projection is not implemented")
class LauncherIdentityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.v1 = load(V1_RESULT_PATH)["launcher"]
        self.v2 = load(V2_RESULT_PATH)["launcher"]

    def test_the_default_seal_is_the_exact_launcher_v2_identity(self) -> None:
        self.assertEqual(successor.__getattr__("LAUNCHER_SHA256"), self.v2["sha256"])
        self.assertEqual(
            successor.__getattr__("LAUNCHER_SIZE_BYTES"), self.v2["sizeBytes"]
        )
        self.assertEqual(
            successor.__getattr__("LAUNCHER_GUEST_PATH"),
            self.v2["guestLogicalPath"],
        )

    def test_exact_v2_size_and_digest_are_accepted_by_the_default_gate(self) -> None:
        # The executable is a CI-built artifact rather than a tracked blob.  Only
        # the digest observer is replaced, inside v4's fresh namespace and only
        # for this call; the gate's sealed size/path/metadata remain live.
        binary = bytes(self.v2["sizeBytes"])
        original = successor._IMPL["sha256_hex"]
        with mock.patch.dict(
            successor._IMPL,
            {"sha256_hex": lambda observed: self.v2["sha256"]},
        ):
            entry = successor.__getattr__("launcher_entry")(binary)
        self.assertIs(successor._IMPL["sha256_hex"], original)
        self.assertIs(entry["raw"], binary)

    def test_the_v1_identity_is_rejected_even_when_its_digest_is_observed(self) -> None:
        binary = bytes(self.v1["sizeBytes"])
        with mock.patch.dict(
            successor._IMPL,
            {"sha256_hex": lambda observed: self.v1["sha256"]},
        ):
            with self.assertRaisesRegex(
                successor.RootfsBuildError, "launcher-digest-mismatch"
            ):
                successor.__getattr__("launcher_entry")(binary)

    def test_v2_size_with_the_wrong_bytes_is_rejected_by_the_real_digest(self) -> None:
        with self.assertRaisesRegex(
            successor.RootfsBuildError, "launcher-digest-mismatch"
        ):
            successor.__getattr__("launcher_entry")(bytes(self.v2["sizeBytes"]))

    def test_the_launcher_metadata_is_unchanged(self) -> None:
        binary = bytes(self.v2["sizeBytes"])
        with mock.patch.dict(
            successor._IMPL,
            {"sha256_hex": lambda observed: self.v2["sha256"]},
        ):
            entry = successor.__getattr__("launcher_entry")(binary)
        probe = b"launcher-v1-metadata-probe"
        historical = builder_v3.launcher_entry(
            probe,
            sha256=hashlib.sha256(probe).hexdigest(),
            size=len(probe),
        )
        self.assertEqual(
            {key: value for key, value in entry.items() if key != "raw"},
            {key: value for key, value in historical.items() if key != "raw"},
        )
        self.assertEqual(entry["path"], self.v2["guestLogicalPath"].lstrip("/"))
        self.assertEqual(entry["kind"], "file")
        self.assertEqual(entry["mode"], 0o755)
        self.assertEqual(entry["uid"], 0)
        self.assertEqual(entry["gid"], 0)

    def test_callers_cannot_replace_the_v2_digest_or_size(self) -> None:
        signature = inspect.signature(successor.__getattr__("launcher_entry"))
        self.assertEqual(tuple(signature.parameters), ("binary",))


@unittest.skipIf(successor is None, "launcher-v2 builder projection is not implemented")
class TablesAndClosuresTests(unittest.TestCase):
    def test_the_staging_table_is_equal_but_not_aliased(self) -> None:
        current = successor.__getattr__("EXPECTED_AUTHORITY_FILES")
        historical = builder_v3.__getattr__("EXPECTED_AUTHORITY_FILES")
        self.assertEqual(current, historical)
        self.assertIsNot(current, historical)
        self.assertEqual(successor.BOOT_AUTHORITY_FILES, builder_v3.BOOT_AUTHORITY_FILES)

    def test_the_closure_table_is_equal_but_not_aliased(self) -> None:
        current = successor.__getattr__("EXPECTED_PROVENANCE_CLOSURE_ROOTS")
        historical = builder_v3.__getattr__("EXPECTED_PROVENANCE_CLOSURE_ROOTS")
        self.assertEqual(current, historical)
        self.assertIsNot(current, historical)
        self.assertEqual(
            successor.__getattr__("REQUIRED_PROVENANCE_CLOSURES"),
            builder_v3.__getattr__("REQUIRED_PROVENANCE_CLOSURES"),
        )

    def test_the_nested_runtime_contract_and_source_lock_release_are_unchanged(self) -> None:
        self.assertEqual(successor.NESTED_RUNTIME_TREE, builder_v3.NESTED_RUNTIME_TREE)
        self.assertEqual(
            successor.BOOT_SOURCE_LOCK_RELEASE,
            builder_v3.BOOT_SOURCE_LOCK_RELEASE,
        )


@unittest.skipIf(successor is None, "launcher-v2 builder projection is not implemented")
class HistoricalBytesTests(unittest.TestCase):
    def test_exercising_the_successor_did_not_change_v3_bytes(self) -> None:
        self.assertEqual(digest(V3_PATH), V3_SHA256)
        self.assertEqual(V3_PATH.stat().st_size, V3_SIZE_BYTES)
        self.assertIs(builder_v3._IMPL, V3_IMPL)
        self.assertIs(builder_v3.__getattr__("launcher_entry"), V3_LAUNCHER_ENTRY)
        self.assertEqual(
            builder_v3.__getattr__("REQUIRED_PROVENANCE_CLOSURES"),
            V3_REQUIRED_CLOSURES,
        )


if __name__ == "__main__":
    unittest.main()
