#!/usr/bin/env python3
"""The fourth step: the builder's staging table, and the release gate before it.

The sealed successor lock names fifteen tracked files.  Two modules stand between
that lock and a build, and both were frozen around the ten the predecessor named:
the release gate that turns a sealed lock into a builder input, and the builder
whose table decides which sources are allowed to be in it.  This gate covers the
successors of both.

The load-bearing test is ``EndToEndSourceShapeTests``.  It takes the sealed
successor lock, binds replay tools into it exactly as production does, and hands
it to each builder.  The predecessor refuses on its narrower table.  The successor
passes every source-shape check and stops at the one thing this step does not
open -- the payload store.  No package is hashed and no artifact is read.

2026-08-28 addendum: the fifth step merged the nested tree, on the terms this
gate's own ``ChainPositionTests`` set.  Those tests said the merge would open with
the measurement that is taken immediately before assembly, and that is what
happened -- in a successor projection, not here.  Every assertion in this file
still holds without a word changed: this step's module keeps its bytes, keeps
``NESTED_RUNTIME_TREE_ASSEMBLED`` false, and still does not call
``build_oci_layout``; the sealed result it reads still records a declared bound
rather than a measurement, because the measurement was sealed in a new file
beside it.  Two tests are renamed to say what they now mean, and one names where
the merge went: ``scripts/test_native_shadow_boot_staging_measure_arm64_v1.py``.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import unittest

from scripts import native_shadow_rootfs_builder_arm64_v1 as arm64
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_v1
from scripts import native_shadow_rootfs_builder_boot_arm64_v2 as mod
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as successor_merge
from scripts import native_shadow_rootfs_portable_boot_arm64_v1 as portable_v1
from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as portable_v2


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SEALED_LOCK_PATH = (
    REPO_ROOT / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)
PREDECESSOR_LOCK_PATH = (
    REPO_ROOT / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
SEALED_RESULT_PATH = (
    REPO_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json"
)
REPLAY_EXPECTATION_PATH = (
    REPO_ROOT
    / "native/containment/native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
)
LAUNCHER_AUTHORITY_ARCH = (
    REPO_ROOT / "crates/boole-native-shadow-launcher/src/authority_arch.rs"
)

# A tool the release gate can read and run.  ``materialize_runtime_lock`` records
# whatever digest it finds, so the file only has to exist, be executable and
# answer ``--version``; nothing downstream compares it against a seal.
REPLAY_TOOL = pathlib.Path("/bin/echo")

ACCOUNT_ROLES = {
    "guest-group": ("native/etc/group", "/etc/group"),
    "guest-gshadow": ("native/etc/gshadow", "/etc/gshadow"),
    "guest-nsswitch": ("native/etc/nsswitch.conf", "/etc/nsswitch.conf"),
    "guest-passwd": ("native/etc/passwd", "/etc/passwd"),
    "guest-shadow": ("native/etc/shadow", "/etc/shadow"),
}
SUPERSEDED_ROLES = {
    "launcher-unit": (
        "native/systemd/boole-native-shadow-launcher.service",
        "native/systemd/boole-native-shadow-launcher-v2.service",
    ),
    "tmpfiles-config": (
        "native/tmpfiles.d/boole-native-shadow.conf",
        "native/tmpfiles.d/boole-native-shadow-v2.conf",
    ),
}


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def module_sha256(module) -> str:
    """The bytes a successor pins its predecessor by."""

    return hashlib.sha256(pathlib.Path(module.__file__).resolve().read_bytes()).hexdigest()


def arm64_constant(name: str) -> str:
    """The arm64 half of a constant the launcher declares twice."""

    source = LAUNCHER_AUTHORITY_ARCH.read_text(encoding="utf-8")
    pattern = re.compile(
        r'#\[cfg\(all\(feature = "linux-arm64-authority"[^\]]*\)\]\s*\n'
        r"pub\(crate\) const " + re.escape(name) + r"[^=]*=\s*([^;]+);",
        re.MULTILINE,
    )
    found = pattern.search(source)
    if found is None:  # pragma: no cover - the gate below turns this into a failure
        raise AssertionError(f"the launcher no longer declares {name} under arm64")
    return found.group(1).strip().strip('"').replace("_", "")


def runtime_lock_from_sealed(path: pathlib.Path, gate) -> tuple[dict, bytes]:
    """Sealed lock to builder input, by the same call production makes."""

    raw = path.read_bytes()
    runtime, _ = gate.materialize_runtime_lock(
        json.loads(raw.decode("utf-8")), raw, REPLAY_TOOL, REPLAY_TOOL
    )
    normalized, normalized_raw, _ = boot_v1.normalized_runtime_lock(runtime)
    return normalized, normalized_raw


def synthetic_entries() -> dict:
    """A handful of entries shaped like the runtime closure, small enough to read.

    Every required runtime closure is populated, because the frozen manifest
    writer refuses a tree that leaves one empty.
    """

    return {
        "opt/boole/native-checker-toolchain/bin/rustc": {
            "path": "opt/boole/native-checker-toolchain/bin/rustc",
            "kind": "file",
            "mode": 0o755,
            "uid": 0,
            "gid": 0,
            "raw": b"toolchain\n",
        },
        "usr/bin/python3.12": {
            "path": "usr/bin/python3.12",
            "kind": "file",
            "mode": 0o755,
            "uid": 0,
            "gid": 0,
            "raw": b"interpreter\n",
        },
        "usr/bin/python3": {
            "path": "usr/bin/python3",
            "kind": "symlink",
            "mode": 0o777,
            "uid": 0,
            "gid": 0,
            "target": "python3.12",
            "resolvedTarget": "usr/bin/python3.12",
        },
        "usr/lib": {
            "path": "usr/lib",
            "kind": "directory",
            "mode": 0o755,
            "uid": 0,
            "gid": 0,
        },
    }


def runtime_closure_roots() -> list:
    return read_json(
        REPO_ROOT
        / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
    )["closureRoots"]


class ProvenanceTests(unittest.TestCase):
    def test_the_predecessor_builder_bytes_are_pinned(self) -> None:
        raw = pathlib.Path(boot_v1.__file__).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.BOOT_V1_SHA256)

    def test_the_predecessor_release_gate_bytes_are_pinned(self) -> None:
        raw = pathlib.Path(portable_v1.__file__).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), portable_v2.BOOT_PORTABLE_V1_SHA256)

    def test_each_successor_records_its_own_bytes(self) -> None:
        for module, attribute in (
            (mod, "SUCCESSOR_PROJECTION_SHA256"),
            (portable_v2, "SUCCESSOR_PROJECTION_SHA256"),
        ):
            raw = pathlib.Path(module.__file__).read_bytes()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), getattr(module, attribute))

    def test_the_predecessor_builder_table_is_left_at_ten(self) -> None:
        """A widened successor must not widen the module it succeeds."""

        self.assertEqual(len(boot_v1.__getattr__("EXPECTED_AUTHORITY_FILES")), 10)
        self.assertEqual(len(boot_v1.BOOT_AUTHORITY_FILES), 4)

    def test_the_predecessor_release_gate_still_names_the_predecessor_release(self) -> None:
        self.assertEqual(portable_v1.SOURCE_LOCK_RELEASE, boot_v1.BOOT_SOURCE_LOCK_RELEASE)
        self.assertNotEqual(portable_v1.SOURCE_LOCK_RELEASE, portable_v2.SOURCE_LOCK_RELEASE)


class StagingTableTests(unittest.TestCase):
    def setUp(self) -> None:
        self.lock = read_json(SEALED_LOCK_PATH)
        self.table = mod.__getattr__("EXPECTED_AUTHORITY_FILES")

    def test_the_table_covers_every_tracked_file_in_the_sealed_successor_lock(self) -> None:
        self.assertEqual(len(self.table), 15)
        expected = {row["sourcePath"]: row["logicalPath"] for row in self.lock["trackedFiles"]}
        self.assertEqual({source: logical for source, logical in self.table.values()}, expected)

    def test_the_table_identities_are_the_locks_authority_binding_identities(self) -> None:
        expected = {row["id"]: row["sourcePath"] for row in self.lock["authorityBindings"]}
        self.assertEqual({role: source for role, (source, _) in self.table.items()}, expected)

    def test_the_five_account_files_are_staged(self) -> None:
        for role, (source, logical) in ACCOUNT_ROLES.items():
            self.assertIn(role, self.table)
            self.assertEqual(self.table[role], (source, logical))

    def test_the_two_superseded_roles_name_the_successor_sources(self) -> None:
        staged = {source for source, _ in self.table.values()}
        for role, (old, new) in SUPERSEDED_ROLES.items():
            self.assertEqual(self.table[role][0], new)
            self.assertNotIn(old, staged)

    def test_the_superseded_roles_keep_the_guest_paths_the_predecessor_gave_them(self) -> None:
        predecessor = boot_v1.__getattr__("EXPECTED_AUTHORITY_FILES")
        for role in SUPERSEDED_ROLES:
            self.assertEqual(self.table[role][1], predecessor[role][1])

    def test_the_eight_unchanged_roles_are_carried_forward_untouched(self) -> None:
        predecessor = boot_v1.__getattr__("EXPECTED_AUTHORITY_FILES")
        carried = set(predecessor) - set(SUPERSEDED_ROLES)
        self.assertEqual(len(carried), 8)
        for role in carried:
            self.assertEqual(self.table[role], predecessor[role])

    def test_the_closure_table_is_the_predecessors(self) -> None:
        """This step adds files, not closures."""

        self.assertEqual(
            mod.__getattr__("REQUIRED_PROVENANCE_CLOSURES"),
            boot_v1.__getattr__("REQUIRED_PROVENANCE_CLOSURES"),
        )
        self.assertEqual(
            tuple(row["name"] for row in self.lock["closureRoots"]),
            mod.__getattr__("REQUIRED_PROVENANCE_CLOSURES"),
        )


class InjectionShapeTests(unittest.TestCase):
    """The successor reproduces the predecessor's namespace, not a subset of it."""

    def test_the_names_the_predecessor_injects_are_all_accounted_for(self) -> None:
        source = pathlib.Path(boot_v1.__file__).read_text(encoding="utf-8")
        injected = sorted(set(re.findall(r'^_IMPL\["(\w+)"\] = ', source, re.MULTILINE)))
        self.assertEqual(
            injected,
            sorted(set(mod.RECOMPUTED_INJECTIONS) | set(mod.INHERITED_INJECTIONS)),
        )

    def test_the_inherited_objects_are_the_predecessors_own(self) -> None:
        for name in mod.INHERITED_INJECTIONS:
            self.assertIs(mod.__getattr__(name), getattr(boot_v1, name))

    def test_the_two_namespaces_are_not_the_same_namespace(self) -> None:
        self.assertIsNot(mod._IMPL, boot_v1._IMPL)
        self.assertIsNot(
            mod.__getattr__("EXPECTED_AUTHORITY_FILES"),
            boot_v1.__getattr__("EXPECTED_AUTHORITY_FILES"),
        )

    def test_each_projection_layer_keeps_its_own_refusal_class(self) -> None:
        """Inherited from how the layers below already work, written down here so
        a caller catching one layer's class knows it is not catching the others."""

        self.assertIsNot(mod.RootfsBuildError, boot_v1.__getattr__("RootfsBuildError"))
        self.assertIsNot(
            boot_v1.__getattr__("RootfsBuildError"), arm64.__getattr__("RootfsBuildError")
        )
        for error in (mod.RootfsBuildError, boot_v1.__getattr__("RootfsBuildError")):
            self.assertTrue(issubclass(error, ValueError))

    def test_the_guard_class_is_shared_so_the_predecessors_callers_still_catch_it(self) -> None:
        self.assertIs(mod.BootProjectionError, boot_v1.BootProjectionError)

    def test_the_x86_only_loader_alias_stays_removed_by_inheritance(self) -> None:
        entries = portable_v2.__getattr__("PORTABLE_V2_DERIVED_ENTRIES")
        self.assertIs(entries, portable_v1.__getattr__("PORTABLE_V2_DERIVED_ENTRIES"))
        paths = {row["logicalPath"] for row in entries}
        self.assertNotIn("/lib64", paths)
        self.assertIn("/lib", paths)


class ReleaseGateTests(unittest.TestCase):
    def test_the_accepted_release_is_the_sealed_successors(self) -> None:
        self.assertEqual(portable_v2.SOURCE_LOCK_RELEASE, read_json(SEALED_LOCK_PATH)["release"])
        self.assertEqual(portable_v2.SOURCE_LOCK_RELEASE, mod.BOOT_SOURCE_LOCK_RELEASE)

    def test_the_schema_is_unchanged_because_only_the_release_differs(self) -> None:
        self.assertEqual(portable_v2.SOURCE_LOCK_SCHEMA, portable_v1.SOURCE_LOCK_SCHEMA)

    def test_the_predecessor_release_is_no_longer_accepted_here(self) -> None:
        """Widening which lock is accepted must not mean accepting both."""

        raw = PREDECESSOR_LOCK_PATH.read_bytes()
        with self.assertRaises(portable_v2.PortableAuthorityError):
            portable_v2.materialize_runtime_lock(
                json.loads(raw.decode("utf-8")), raw, REPLAY_TOOL, REPLAY_TOOL
            )

    def test_the_successor_release_is_not_accepted_by_the_predecessor_gate(self) -> None:
        raw = SEALED_LOCK_PATH.read_bytes()
        with self.assertRaises(portable_v1.PortableAuthorityError):
            portable_v1.materialize_runtime_lock(
                json.loads(raw.decode("utf-8")), raw, REPLAY_TOOL, REPLAY_TOOL
            )

    def test_an_activatable_successor_lock_is_still_refused(self) -> None:
        lock = read_json(SEALED_LOCK_PATH)
        lock["activationAllowed"] = True
        with self.assertRaises(portable_v2.PortableAuthorityError):
            portable_v2.materialize_runtime_lock(
                lock, mod.canonical_json(lock), REPLAY_TOOL, REPLAY_TOOL
            )

    def test_a_non_canonical_successor_lock_is_still_refused(self) -> None:
        with self.assertRaises(portable_v2.PortableAuthorityError):
            portable_v2.materialize_runtime_lock(
                read_json(SEALED_LOCK_PATH), b"{}\n", REPLAY_TOOL, REPLAY_TOOL
            )


class EndToEndSourceShapeTests(unittest.TestCase):
    """What the widened table actually buys, measured rather than described."""

    def setUp(self) -> None:
        self.lock, self.raw = runtime_lock_from_sealed(SEALED_LOCK_PATH, portable_v2)

    def test_the_predecessor_builder_refuses_the_successor_lock(self) -> None:
        with self.assertRaises(boot_v1.__getattr__("RootfsBuildError")) as raised:
            boot_v1.validate_source_lock(
                self.lock, self.raw, REPO_ROOT, None, require_complete=False
            )
        self.assertIn("authority binding identity/source set differs", str(raised.exception))

    def test_the_successor_builder_passes_every_source_shape_check(self) -> None:
        """It stops at the payload store, which is the step this one does not take."""

        with self.assertRaises(mod.RootfsBuildError) as raised:
            mod.validate_source_lock(
                self.lock, self.raw, REPO_ROOT, None, require_complete=False
            )
        self.assertIn("complete source lock needs an artifact store", str(raised.exception))

    def test_the_successor_builder_still_reads_the_predecessor_lock(self) -> None:
        """Ten of the fifteen rows are the predecessor's, so its lock still fails
        on the five that are absent rather than on anything else."""

        lock, raw = runtime_lock_from_sealed(PREDECESSOR_LOCK_PATH, portable_v1)
        with self.assertRaises(mod.RootfsBuildError) as raised:
            mod.validate_source_lock(lock, raw, REPO_ROOT, None, require_complete=False)
        self.assertIn("authority binding identity/source set differs", str(raised.exception))

    def test_an_unsorted_closure_is_refused_with_the_predecessors_words(self) -> None:
        lock = json.loads(self.raw.decode("utf-8"))
        lock["closureRoots"][0]["logicalRoots"] = list(
            reversed(lock["closureRoots"][0]["logicalRoots"])
        )
        raw = mod.canonical_json(lock)
        messages = []
        for module in (boot_v1, mod):
            with self.assertRaises(module.BootProjectionError) as raised:
                module.validate_source_lock(lock, raw, REPO_ROOT, None, require_complete=False)
            messages.append(str(raised.exception))
        self.assertEqual(messages[0], messages[1])
        self.assertIn("run normalized_runtime_lock first", messages[0])


class NestedTreeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.entries = synthetic_entries()
        self.closures = runtime_closure_roots()

    def test_the_declared_manifest_digest_is_the_one_the_launcher_compiles_against(self) -> None:
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["contentManifestSha256"],
            arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256"),
        )
        self.assertEqual(
            str(mod.NESTED_RUNTIME_TREE["contentManifestSizeBytes"]),
            arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE"),
        )
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["contentManifestSchema"],
            arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA"),
        )

    def test_the_declared_manifest_digest_is_the_sealed_replay_expectation(self) -> None:
        expected = read_json(REPLAY_EXPECTATION_PATH)["expectedOutput"]
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["contentManifestSha256"],
            expected["rootfsContentManifestSha256"],
        )
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["contentManifestSizeBytes"],
            expected["rootfsContentManifestSizeBytes"],
        )

    def test_the_declaration_agrees_with_the_sealed_successor_result(self) -> None:
        nested = read_json(SEALED_RESULT_PATH)["nestedTree"]
        self.assertEqual(mod.NESTED_RUNTIME_TREE["id"], nested["id"])
        self.assertEqual(mod.NESTED_RUNTIME_TREE["guestPrefix"], nested["guestPrefix"])
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["contentManifestSha256"], nested["contentManifestSha256"]
        )
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["drivingSourceLockSha256"], nested["drivingSourceLockSha256"]
        )

    def test_the_driving_lock_is_the_runtime_lock_at_its_sealed_digest(self) -> None:
        path = REPO_ROOT / mod.NESTED_RUNTIME_TREE["drivingSourceLockPath"]
        self.assertEqual(
            hashlib.sha256(path.read_bytes()).hexdigest(),
            mod.NESTED_RUNTIME_TREE["drivingSourceLockSha256"],
        )

    def test_the_manifest_is_derived_by_the_runtime_builder_not_this_one(self) -> None:
        """Assembled from the boot closures the document would name five closures
        and would not be the document the launcher verifies."""

        manifest, _ = mod.nested_content_manifest(
            self.entries, self.closures, sha256=None, size=None
        )
        names = {name for entry in manifest["entries"] for name in entry["closures"]}
        self.assertTrue(names)
        self.assertEqual(names - set(arm64.__getattr__("REQUIRED_PROVENANCE_CLOSURES")), set())
        self.assertEqual(manifest["schema"], mod.NESTED_RUNTIME_TREE["contentManifestSchema"])

    def test_a_manifest_that_differs_from_the_seal_stops_the_build(self) -> None:
        with self.assertRaises(mod.BootSuccessorProjectionError) as raised:
            mod.nested_content_manifest(self.entries, self.closures)
        self.assertIn("nested content manifest", str(raised.exception))

    def test_the_entries_are_placed_under_the_guest_prefix(self) -> None:
        manifest, raw = mod.nested_content_manifest(
            self.entries, self.closures, sha256=None, size=None
        )
        staged = mod.nested_tree_entries(
            self.entries, self.closures, sha256=None, size=None
        )
        prefix = mod.NESTED_RUNTIME_TREE["guestPrefix"].lstrip("/")
        for path in self.entries:
            self.assertIn(f"{prefix}/{path}", staged)
        self.assertNotIn("usr/bin/python3.12", staged)

    def test_a_symlink_keeps_its_text_and_moves_its_resolution(self) -> None:
        staged = mod.nested_tree_entries(
            self.entries, self.closures, sha256=None, size=None
        )
        prefix = mod.NESTED_RUNTIME_TREE["guestPrefix"].lstrip("/")
        link = staged[f"{prefix}/usr/bin/python3"]
        self.assertEqual(link["target"], "python3.12")
        self.assertEqual(link["resolvedTarget"], f"{prefix}/usr/bin/python3.12")

    def test_the_manifest_is_placed_beside_the_tree_read_only(self) -> None:
        manifest, raw = mod.nested_content_manifest(
            self.entries, self.closures, sha256=None, size=None
        )
        staged = mod.nested_tree_entries(
            self.entries, self.closures, sha256=None, size=None
        )
        path = mod.NESTED_RUNTIME_TREE["contentManifestGuestPath"].lstrip("/")
        entry = staged[path]
        self.assertEqual(entry["kind"], "file")
        self.assertEqual(entry["mode"], 0o444)
        self.assertEqual(entry["uid"], 0)
        self.assertEqual(entry["gid"], 0)
        self.assertEqual(entry["raw"], raw)
        self.assertFalse(path.startswith(mod.NESTED_RUNTIME_TREE["guestPrefix"].lstrip("/")))

    def test_an_entry_that_is_already_rooted_is_refused(self) -> None:
        entries = dict(self.entries)
        entries["/usr/bin/absolute"] = {
            "path": "/usr/bin/absolute",
            "kind": "file",
            "mode": 0o644,
            "uid": 0,
            "gid": 0,
            "raw": b"",
        }
        with self.assertRaises(mod.BootSuccessorProjectionError):
            mod.nested_tree_entries(entries, self.closures, sha256=None, size=None)

    def test_an_empty_tree_is_refused(self) -> None:
        with self.assertRaises(mod.BootSuccessorProjectionError):
            mod.nested_tree_entries({}, self.closures, sha256=None, size=None)


class ChainPositionTests(unittest.TestCase):
    """What the fourth step did, and what it explicitly did not do."""

    def test_nothing_here_claims_a_bootable_image(self) -> None:
        self.assertFalse(mod.BOOTABLE_CLAIM)
        self.assertFalse(mod.ACTIVATION_ALLOWED)
        self.assertFalse(mod.NESTED_RUNTIME_TREE_ASSEMBLED)

    def test_this_projection_still_does_not_merge_the_nested_tree(self) -> None:
        """Superseded 2026-08-28 by the fifth step, as this test's own terms required.

        It said merging opens with the measurement taken immediately before
        assembly.  That measurement was taken, and the merge opened where the
        measurement could consume it -- in the successor projection, not here.
        What the assertions guard is the half that did not move: this module still
        stages the tree and hands it on without building anything from it, so the
        lock it was written for still builds here exactly as it did.
        """

        source = pathlib.Path(mod.__file__).read_text(encoding="utf-8")
        self.assertNotIn("build_oci_layout(", source.split('"""', 2)[-1])
        self.assertIn("nested_tree_entries", source)
        self.assertNotIn('"nested runtime tree"', mod._derived_source())

    def test_the_merge_lives_in_the_successor_projection(self) -> None:
        """One line, in the builder, before the parent directories are derived."""

        source = successor_merge.__file__ and pathlib.Path(
            successor_merge.__file__
        ).read_text(encoding="utf-8")
        self.assertIn('_merge(entries, nested_tree, "nested runtime tree")', source)
        self.assertEqual(successor_merge.BOOT_V2_SHA256, module_sha256(mod))
        derived = successor_merge._derived_source()
        self.assertLess(
            derived.index('_merge(entries, nested_tree, "nested runtime tree")'),
            derived.index("    _ensure_parents(entries)\n"),
        )

    def test_the_totals_are_still_bounds_rather_than_measurements(self) -> None:
        """The declared bound stays on this step's record; the measurement is new.

        The fifth step did not rewrite this file -- it sealed
        ``native-shadow-boot-staging-tree-measurement-arm64-v1.json`` beside it,
        which is why every number here is still the number this step sealed.
        """

        nested = read_json(SEALED_RESULT_PATH)["nestedTree"]
        self.assertFalse(nested["assembled"])
        self.assertEqual(nested["state"], "declared-not-assembled")
        self.assertEqual(
            mod.NESTED_RUNTIME_TREE["layerSizeBytes"], nested["layerSizeBytes"]
        )
        self.assertFalse(mod.NESTED_RUNTIME_TREE["layerSizeBytesIsAMeasuredTotal"])


if __name__ == "__main__":
    unittest.main()
