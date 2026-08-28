#!/usr/bin/env python3
"""The sixth step: a successor production path, and every way it has to refuse.

The path already in the tree reads the predecessor boot source lock, imports the
predecessor builder, and calls the layout entry point with no nested tree.  It is
kept exactly as it is, because it is what reproduces the image that already
booted.  What it cannot do is produce the successor image, and the budget for
finding that out the expensive way is one run.

So this gate is written as refusals.  Almost every test here asserts that
something is rejected rather than that something is built: a predecessor lock
reaching the successor gate, a successor lock reaching the predecessor gate, a
missing account file, the superseded launcher unit, a launcher whose rebuilt
digest or size differs from the seal, totals that disagree with the sealed
measurement, a limit exceeded, a conflict or a duplicate or an escaping symlink,
a nested tree left to a default, a preflight that reaches for an image tool, and
a production that assembles through a different merge than the measurement did.

Two properties are load-bearing and worth naming separately.

The first is that the successor and the predecessor cannot be swapped in either
direction.  The release gates already refuse each other's locks, and that refusal
lands before either gate opens a tool, so the test costs nothing and proves the
half that matters: a misconfiguration cannot quietly become a wrong image.

The second is the budget boundary.  It is a marker the production writes on
purpose, atomically, immediately before its first image file: a refusal raised
before it has cost nothing, and a failure raised after it has cost the only
attempt there is.  ``BudgetBoundaryTests`` and ``ConsumedMarkerTests`` check that
the preflight cannot reach the far side of that line -- not because it promises
to be careful, but because it never calls anything that would create an image or
write the marker -- and that a run cut off mid-write lands on the unspent side,
because the marker is a rename rather than a write.

Nothing here reads the payload store, so the gate runs where the artifacts are
absent.  Fixtures are hand-built entry tables, which is enough: every refusal in
this file is about the shape of a table or the shape of the module, not about the
contents of a package.
"""

from __future__ import annotations

import ast
import contextlib
import hashlib
import inspect
import io
import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts import native_shadow_boot_produce_phase_arm64_v1 as predecessor
from scripts import native_shadow_boot_staging_measure_arm64_v1 as measure
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as builder
from scripts import native_shadow_rootfs_portable_boot_arm64_v1 as predecessor_gate
from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as successor_gate
from scripts import native_shadow_successor_produce_phase_arm64_v2 as mod



@contextlib.contextmanager
def restored_temporary_directory():
    """Give the process its temporary directory back.

    ``main`` names one out of the scratch it was handed, and the naming outlives
    the call because that is the whole point of it -- helpers deep in the shared
    builder ask for a temporary directory without naming a place, and this is
    what answers them.  A test that let the pin escape would leave every later
    test pointed at a scratch it had already deleted.
    """

    previous = tempfile.tempdir
    try:
        yield
    finally:
        tempfile.tempdir = previous


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO_ROOT / "native/containment"
AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v3.json"
)
# The authority the two prior dispatches ran under.  It is not the current one and
# it is not edited: the documents written about those attempts are judged
# against the bytes that were sealed when they ran.
SPENT_AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v2.json"
)
PRODUCER_FINGERPRINT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-producer-fingerprint-arm64-v3.json"
)
PREDECESSOR_LOCK_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
SUCCESSOR_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v2.json"
MEASUREMENT_PATH = (
    CONTAINMENT / "native-shadow-boot-staging-tree-measurement-arm64-v1.json"
)
PREFLIGHT_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-preflight-result-arm64-v1.json"
)
LAUNCHER_RESULT_PATH = CONTAINMENT / "native-shadow-launcher-build-result-arm64-v1.json"
REPLAY_EXPECTATION_PATH = (
    CONTAINMENT / "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
)
MISSING_TOOL = pathlib.Path("/nonexistent/replay-tool")
WORKFLOW_PATH = (
    REPO_ROOT / ".github/workflows/native-shadow-successor-produce-arm64.yml"
)


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_bytes().decode("utf-8"))


def workflow_job(name: str) -> str:
    """One job's block, cut out of the workflow by indentation.

    No YAML parser is used.  The runner this gate has to pass on is not promised
    one, and the question asked below is which lines belong to which job, which
    the indentation already answers exactly.
    """

    lines = WORKFLOW_PATH.read_text(encoding="utf-8").splitlines()
    try:
        start = lines.index(f"  {name}:") + 1
    except ValueError:
        raise AssertionError(f"the workflow has no {name} job") from None
    block = []
    for line in lines[start:]:
        if line.strip() and not line.startswith("    "):
            break
        block.append(line)
    return "\n".join(block)


def digest_of(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def module_source(module) -> str:
    return pathlib.Path(module.__file__).read_text(encoding="utf-8")


def predecessor_aliases_in(module) -> set:
    """Whatever names the given module imported the historical phase under."""

    wanted = pathlib.Path(predecessor.__file__).stem
    aliases = set()
    for node in ast.walk(ast.parse(module_source(module))):
        if isinstance(node, ast.ImportFrom):
            for name in node.names:
                if name.name == wanted:
                    aliases.add(name.asname or name.name)
    return aliases


def file_entry(path: str, raw: bytes, mode: int) -> dict:
    """A staged entry with exactly the keys the builder really puts in one.

    Deliberately no `sha256` and no `sizeBytes`.  The builder carries the bytes
    and hashes them when it writes the layer, so a fixture that supplies a digest
    alongside them describes a table that never exists, and a check written
    against that table reads `None` in production.
    """

    return {
        "path": path,
        "kind": "file",
        "mode": mode,
        "uid": 0,
        "gid": 0,
        "raw": raw,
    }


def account_entries() -> dict:
    """The five account files, at the guest paths and modes the lock stages them."""

    entries = {}
    for row in mod.ACCOUNT_DATABASE:
        key = row["guestPath"].lstrip("/")
        raw = (REPO_ROOT / row["sourcePath"]).read_bytes()
        entries[key] = file_entry(key, raw, row["mode"])
    return entries


def unit_entries(source: str) -> dict:
    key = mod.LAUNCHER_UNIT_GUEST_PATH.lstrip("/")
    return {key: file_entry(key, (REPO_ROOT / source).read_bytes(), 0o444)}


# The link that makes the unit start at boot.  Spelled out here rather than read
# from the module, so the test says what the guest path has to be and the module
# is checked against it instead of against itself.
ENABLEMENT_GUEST_PATH = (
    "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service"
)


def enablement_entry(**overrides) -> dict:
    """The enablement symlink, shaped the way the builder stages a derived one."""

    key = ENABLEMENT_GUEST_PATH.lstrip("/")
    entry = {
        "path": key,
        "kind": "symlink",
        "mode": 0o777,
        "uid": 0,
        "gid": 0,
        "target": mod.LAUNCHER_UNIT_GUEST_PATH,
        "resolvedTarget": mod.LAUNCHER_UNIT_GUEST_PATH.lstrip("/"),
    }
    entry.update(overrides)
    return {key: entry}


def rewritten_unit(entries: dict, old: str, new: str) -> dict:
    key = mod.LAUNCHER_UNIT_GUEST_PATH.lstrip("/")
    text = entries[key]["raw"].decode("utf-8")
    if old not in text:
        raise AssertionError(f"the fixture cannot rewrite what is not there: {old}")
    replaced = dict(entries)
    replaced[key] = file_entry(key, text.replace(old, new).encode("utf-8"), 0o444)
    return replaced


# The sealed manifest is 1.2MB assembled out of the artifact store, which CI
# does not carry.  So the fixture stands in bytes of its own and the tests that
# need acceptance say what the seal would have to be for those bytes -- the
# accessor is what is under test, and the real digest is checked against the
# replay expectation and the measurement separately, below.
MANIFEST_STAND_IN = b'{"entries": [], "stand-in": true}\n'


def manifest_entry(raw: bytes = MANIFEST_STAND_IN) -> dict:
    key = mod.CONTENT_MANIFEST_GUEST_PATH.lstrip("/")
    return {key: file_entry(key, raw, mod.CONTENT_MANIFEST_MODE)}


@contextlib.contextmanager
def manifest_sealed_as(raw: bytes):
    """Point the module's manifest seal at the given bytes for one block."""

    with mock.patch.object(mod, "CONTENT_MANIFEST_SHA256", hashlib.sha256(raw).hexdigest()):
        with mock.patch.object(mod, "CONTENT_MANIFEST_SIZE_BYTES", len(raw)):
            yield


def complete_entries() -> dict:
    entries = account_entries()
    entries.update(unit_entries(mod.LAUNCHER_UNIT_SOURCE))
    entries.update(enablement_entry())
    entries.update(manifest_entry())
    return entries


@contextlib.contextmanager
def written_tree(entries: dict):
    """The table written out by the same writer the preflight writes with.

    The gaps are read back off this rather than off the table, because the table
    is what the writer was asked for and this is what it did.

    The manifest seal is pointed at the stand-in for the duration, because the
    fixture cannot hold the real 1.2MB manifest and the seal and the bytes have
    to travel together -- otherwise a refusal about the manifest would fire in
    tests that are about something else entirely, and pass for the wrong reason.
    """

    with manifest_sealed_as(MANIFEST_STAND_IN):
        with tempfile.TemporaryDirectory() as scratch:
            table = dict(entries)
            measure.builder.__getattr__("_ensure_parents")(table)
            destination = pathlib.Path(scratch) / "staging"
            measure.write_staging_tree(table, destination, 0)
            yield destination


def walked(**overrides) -> dict:
    totals = dict(mod.EXPECTED_WITHOUT_LAUNCHER)
    totals.update(overrides)
    return totals


def complete(**overrides) -> dict:
    totals = dict(mod.EXPECTED_WITH_LAUNCHER)
    totals.update(overrides)
    return totals


class AuthorityTests(unittest.TestCase):
    """The pre-registration is consumed, not paraphrased."""

    def test_the_authority_is_the_one_that_was_sealed(self) -> None:
        self.assertEqual(mod.AUTHORITY_SHA256, digest_of(AUTHORITY_PATH))

    def test_an_authority_whose_bytes_moved_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.authority(path=SUCCESSOR_LOCK_PATH)

    def test_the_authority_still_claims_nothing_it_did_not_do(self) -> None:
        document = mod.authority()
        self.assertEqual(document["runsAllowed"], 1)
        self.assertEqual(document["runsPerformed"], 0)
        self.assertFalse(document["bootableClaim"])
        self.assertFalse(document["servingClaim"])
        self.assertFalse(document["imageProducedClaim"])
        self.assertFalse(document["activationAllowed"])

    def test_every_bound_input_digest_still_matches_its_file(self) -> None:
        rows = mod.assert_bound_inputs(mod.authority(), REPO_ROOT)
        self.assertEqual(rows, len(mod.authority()["boundInputDigests"]["files"]))
        self.assertGreaterEqual(rows, 11)

    def test_a_bound_input_that_moved_is_refused(self) -> None:
        document = mod.authority()
        document["boundInputDigests"]["files"][0]["sha256"] = "0" * 64
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_bound_inputs(document, REPO_ROOT)

    def test_the_predecessor_record_is_bound_byte_unchanged(self) -> None:
        row = mod.authority()["supersedes"]["predecessor"]
        self.assertTrue(row["leftByteUnchanged"])
        self.assertEqual(row["sha256"], digest_of(REPO_ROOT / row["path"]))


class LockSeparationTests(unittest.TestCase):
    """Neither lock may reach the other path, in either direction."""

    def test_the_predecessor_gate_refuses_the_successor_lock(self) -> None:
        raw = SUCCESSOR_LOCK_PATH.read_bytes()
        with self.assertRaises(predecessor_gate.PortableAuthorityError):
            predecessor_gate.materialize_runtime_lock(
                json.loads(raw.decode("utf-8")), raw, MISSING_TOOL, MISSING_TOOL
            )

    def test_the_successor_gate_refuses_the_predecessor_lock(self) -> None:
        raw = PREDECESSOR_LOCK_PATH.read_bytes()
        with self.assertRaises(successor_gate.PortableAuthorityError):
            successor_gate.materialize_runtime_lock(
                json.loads(raw.decode("utf-8")), raw, MISSING_TOOL, MISSING_TOOL
            )

    def test_the_refusal_lands_before_either_gate_opens_a_tool(self) -> None:
        # Both tool paths do not exist.  A gate that got as far as opening one
        # would say so; saying the identity differs is what proves the release
        # was read first, and that is why a swapped lock costs nothing to catch.
        raw = SUCCESSOR_LOCK_PATH.read_bytes()
        with self.assertRaises(predecessor_gate.PortableAuthorityError) as caught:
            predecessor_gate.materialize_runtime_lock(
                json.loads(raw.decode("utf-8")), raw, MISSING_TOOL, MISSING_TOOL
            )
        self.assertIn("identity differs", str(caught.exception))

    def test_the_successor_module_names_only_the_successor_lock(self) -> None:
        source = module_source(mod)
        self.assertIn("native-shadow-boot-rootfs-source-lock-arm64-v2.json", source)
        self.assertNotIn("native-shadow-boot-rootfs-source-lock-arm64-v1.json", source)

    def test_a_lock_carrying_the_predecessor_release_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_successor_release(read_json(PREDECESSOR_LOCK_PATH))

    def test_the_sealed_successor_lock_passes_its_own_gate(self) -> None:
        mod.assert_successor_release(read_json(SUCCESSOR_LOCK_PATH))

    def test_the_two_releases_are_not_the_same_string(self) -> None:
        successor_release = read_json(SUCCESSOR_LOCK_PATH)["release"]
        self.assertNotEqual(
            successor_release, read_json(PREDECESSOR_LOCK_PATH)["release"]
        )
        self.assertEqual(mod.SOURCE_LOCK_RELEASE, successor_release)

    def test_a_source_lock_whose_bytes_moved_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.sealed_source_lock(path=PREDECESSOR_LOCK_PATH)

    def test_the_module_declares_no_fallback_between_the_two_locks(self) -> None:
        mod.assert_no_lock_fallback()


class ModuleIdentityTests(unittest.TestCase):
    """The release gate and the builder are pinned, not merely imported."""

    def test_the_release_gate_is_the_successor_projection(self) -> None:
        gate = mod.release_gate()
        self.assertIs(gate, successor_gate)
        self.assertEqual(mod.RELEASE_GATE_SHA256, digest_of(pathlib.Path(gate.__file__)))

    def test_the_builder_is_the_latest_staging_projection(self) -> None:
        latest = mod.builder()
        self.assertIs(latest, builder)
        self.assertEqual(mod.BUILDER_SHA256, builder.SUCCESSOR_PROJECTION_SHA256)
        self.assertEqual(mod.BUILDER_SHA256, digest_of(pathlib.Path(latest.__file__)))

    def test_a_module_whose_bytes_moved_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_module_digest(predecessor, mod.BUILDER_SHA256)

    def test_the_predecessor_release_gate_is_not_reachable_from_here(self) -> None:
        source = module_source(mod)
        self.assertIn("native_shadow_rootfs_builder_boot_arm64_v3", source)
        self.assertNotIn("native_shadow_rootfs_portable_boot_arm64_v1", source)

    def test_the_base_projection_is_used_only_for_the_seal_and_the_normalizer(
        self,
    ) -> None:
        # The first projection in the chain is not a competing builder -- the
        # latest one is its source with replacements applied, and the sealed
        # measurement reaches it for exactly two things.  Production reaches it
        # for the same two, and an added third would be caught here rather than
        # discovered in an image.
        mod.assert_base_projection_scope()
        self.assertEqual(
            sorted(mod.BASE_PROJECTION_ALLOWED),
            [
                "BootProjectionError",
                "LAUNCHER_GUEST_PATH",
                "LAUNCHER_SHA256",
                "LAUNCHER_SIZE_BYTES",
                "normalized_runtime_lock",
            ],
        )


class SharedMergeTests(unittest.TestCase):
    """One assembler object, reached from both sides."""

    def test_the_production_and_the_measurement_reach_one_namespace(self) -> None:
        mod.assert_shared_assembler()

    def test_the_measurement_reaches_the_same_builder_this_path_does(self) -> None:
        self.assertIs(measure.builder, mod.builder())

    def test_two_equal_copies_are_not_good_enough(self) -> None:
        # A namespace that merely equals the builder's is rejected: two copies
        # can drift and one object cannot, which is the whole point of the check.
        twin = dict(builder._IMPL)
        self.assertEqual(set(twin), set(builder._IMPL))
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_shared_assembler(namespace=twin)

    def test_the_layout_entry_point_resolves_through_that_namespace(self) -> None:
        namespace = mod.shared_namespace()
        self.assertIs(namespace["build_oci_layout"].__globals__, namespace)
        self.assertIs(namespace["_assemble_entries"].__globals__, namespace)
        self.assertIs(builder.materialize_staging_tree.__globals__["_IMPL"], namespace)


class RequiredArgumentTests(unittest.TestCase):
    """The nested tree and the manifest are arguments, never defaults."""

    def test_the_production_entry_point_requires_a_nested_tree(self) -> None:
        parameter = inspect.signature(mod.produce).parameters["nested_tree"]
        self.assertIs(parameter.default, inspect.Parameter.empty)

    def test_the_production_entry_point_requires_the_manifest_expectation(self) -> None:
        parameter = inspect.signature(mod.produce).parameters["content_manifest_sha256"]
        self.assertIs(parameter.default, inspect.Parameter.empty)

    def test_no_entry_point_defaults_the_nested_tree_or_the_manifest(self) -> None:
        for entry_point in (mod.produce, mod.preflight):
            for name, parameter in inspect.signature(entry_point).parameters.items():
                if "nested" in name or "manifest" in name:
                    self.assertIs(
                        parameter.default,
                        inspect.Parameter.empty,
                        f"{entry_point.__name__}.{name} carries a default",
                    )

    def test_calling_production_without_a_nested_tree_is_a_type_error(self) -> None:
        with self.assertRaises(TypeError):
            mod.produce(repository_root=REPO_ROOT)  # type: ignore[call-arg]

    def test_calling_the_preflight_without_a_nested_tree_is_a_type_error(self) -> None:
        with self.assertRaises(TypeError):
            mod.preflight(repository_root=REPO_ROOT)  # type: ignore[call-arg]


class AccountDatabaseTests(unittest.TestCase):
    """Five files, and no fewer."""

    def test_the_five_account_paths_are_the_ones_the_lock_stages(self) -> None:
        self.assertEqual(
            sorted(row["guestPath"] for row in mod.ACCOUNT_DATABASE),
            [
                "/etc/group",
                "/etc/gshadow",
                "/etc/nsswitch.conf",
                "/etc/passwd",
                "/etc/shadow",
            ],
        )

    def test_each_account_row_matches_the_sealed_lock(self) -> None:
        staged = {
            row["logicalPath"]: row
            for row in read_json(SUCCESSOR_LOCK_PATH)["trackedFiles"]
        }
        for row in mod.ACCOUNT_DATABASE:
            sealed = staged[row["guestPath"]]
            self.assertEqual(row["sourcePath"], sealed["sourcePath"])
            self.assertEqual(row["sha256"], sealed["sha256"])
            self.assertEqual(f"{row['mode']:04o}", sealed["mode"])

    def test_a_complete_account_database_passes(self) -> None:
        mod.assert_account_database(complete_entries())

    def test_each_missing_account_file_is_refused_on_its_own(self) -> None:
        for row in mod.ACCOUNT_DATABASE:
            entries = complete_entries()
            del entries[row["guestPath"].lstrip("/")]
            with self.assertRaises(mod.SuccessorProduceError, msg=row["guestPath"]):
                mod.assert_account_database(entries)

    def test_an_account_file_owned_by_someone_else_is_refused(self) -> None:
        entries = complete_entries()
        entries["etc/passwd"] = dict(entries["etc/passwd"], uid=1000)
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_account_database(entries)

    def test_a_readable_shadow_file_is_refused(self) -> None:
        entries = complete_entries()
        entries["etc/shadow"] = dict(entries["etc/shadow"], mode=0o444)
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_account_database(entries)

    def test_an_account_file_whose_bytes_moved_is_refused(self) -> None:
        # A group added to the guest is exactly the kind of edit that has to be
        # caught here rather than discovered by a launcher that cannot resolve.
        entries = complete_entries()
        moved = entries["etc/group"]["raw"] + b"extra:x:9999:\n"
        entries["etc/group"] = dict(entries["etc/group"], raw=moved)
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_account_database(entries)

    def test_an_account_file_cannot_pass_by_claiming_a_digest(self) -> None:
        # Content replaced, sealed digest asserted beside it.  Hashing the bytes
        # is what makes the claim irrelevant.
        entries = complete_entries()
        sealed = {row["guestPath"]: row["sha256"] for row in mod.ACCOUNT_DATABASE}
        entries["etc/passwd"] = dict(
            entries["etc/passwd"],
            raw=b"root:x:0:0:root:/root:/bin/sh\nintruder:x:0:0::/:/bin/sh\n",
            sha256=sealed["/etc/passwd"],
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_account_database(entries)

    def test_an_account_path_that_is_not_a_file_is_refused(self) -> None:
        entries = complete_entries()
        entries["etc/shadow"] = dict(entries["etc/shadow"], kind="symlink")
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_account_database(entries)


class LauncherUnitTests(unittest.TestCase):
    """The successor unit, and the console the host already reads."""

    def test_the_unit_source_is_the_successor_one(self) -> None:
        self.assertEqual(
            mod.LAUNCHER_UNIT_SOURCE,
            "native/systemd/boole-native-shadow-launcher-v2.service",
        )
        self.assertEqual(
            mod.SUPERSEDED_LAUNCHER_UNIT_SOURCE,
            "native/systemd/boole-native-shadow-launcher.service",
        )
        self.assertNotEqual(
            digest_of(REPO_ROOT / mod.LAUNCHER_UNIT_SOURCE),
            digest_of(REPO_ROOT / mod.SUPERSEDED_LAUNCHER_UNIT_SOURCE),
        )

    def test_the_successor_unit_passes(self) -> None:
        mod.assert_launcher_unit(complete_entries())

    def test_the_superseded_unit_is_refused_at_the_same_guest_path(self) -> None:
        entries = complete_entries()
        entries.update(unit_entries(mod.SUPERSEDED_LAUNCHER_UNIT_SOURCE))
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_a_missing_unit_is_refused(self) -> None:
        entries = complete_entries()
        del entries[mod.LAUNCHER_UNIT_GUEST_PATH.lstrip("/")]
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_a_unit_that_writes_only_to_the_journal_is_refused(self) -> None:
        entries = rewritten_unit(
            complete_entries(),
            "StandardOutput=journal+console",
            "StandardOutput=journal",
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_a_unit_whose_error_stream_misses_the_console_is_refused(self) -> None:
        entries = rewritten_unit(
            complete_entries(), "StandardError=journal+console", "StandardError=journal"
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_a_unit_that_is_not_installed_is_refused(self) -> None:
        entries = rewritten_unit(
            complete_entries(), "WantedBy=multi-user.target", "WantedBy=graphical.target"
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_the_enablement_link_is_the_one_the_lock_stages(self) -> None:
        # `WantedBy=` inside the file is a request, not an installation.  systemd
        # acts on the link in the wants directory, and that link is a separate
        # staged entry that the image writer would happily leave out.
        staged = {
            row["logicalPath"]: row
            for row in read_json(SUCCESSOR_LOCK_PATH)["derivedEntries"]
        }
        sealed = staged[ENABLEMENT_GUEST_PATH]
        self.assertEqual(mod.LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH, ENABLEMENT_GUEST_PATH)
        self.assertEqual(sealed["kind"], "symlink")
        self.assertEqual(sealed["target"], mod.LAUNCHER_UNIT_GUEST_PATH)
        self.assertEqual(int(sealed["mode"], 8), mod.LAUNCHER_UNIT_ENABLEMENT_MODE)
        self.assertEqual(sealed["uid"], 0)
        self.assertEqual(sealed["gid"], 0)

    def test_a_unit_staged_without_its_enablement_link_is_refused(self) -> None:
        entries = complete_entries()
        del entries[ENABLEMENT_GUEST_PATH.lstrip("/")]
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_an_enablement_link_pointing_at_something_else_is_refused(self) -> None:
        entries = complete_entries()
        entries.update(enablement_entry(target="/usr/lib/systemd/system/getty.service"))
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_an_enablement_link_staged_as_a_regular_file_is_refused(self) -> None:
        # A copy of the unit in the wants directory is not an enablement: systemd
        # would start whatever that copy says, which need not be this unit.
        entries = complete_entries()
        entries.update(enablement_entry(kind="file", raw=b"[Unit]\n"))
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_an_enablement_link_owned_by_someone_else_is_refused(self) -> None:
        entries = complete_entries()
        entries.update(enablement_entry(uid=1000))
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_a_unit_that_starts_something_else_is_refused(self) -> None:
        entries = rewritten_unit(
            complete_entries(),
            "ExecStart=/usr/libexec/boole/boole-native-shadow-launcher",
            "ExecStart=/bin/true",
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_the_unit_keeps_exactly_four_bounding_capabilities(self) -> None:
        # The correction the operator issued: the launcher is a root supervisor
        # holding four capabilities, and it is the answer and the checker that
        # get dropped to the unprivileged account.  Widening the set would be a
        # different change and would need a record of its own.
        self.assertEqual(
            mod.LAUNCHER_BOUNDING_CAPABILITIES,
            ("CAP_SETGID", "CAP_SETUID", "CAP_SETPCAP", "CAP_SYS_ADMIN"),
        )
        mod.assert_launcher_unit(complete_entries())

    def test_a_fifth_capability_is_refused(self) -> None:
        entries = rewritten_unit(
            complete_entries(),
            "CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN",
            "CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN CAP_NET_ADMIN",
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)

    def test_an_inherited_ambient_capability_is_refused(self) -> None:
        entries = rewritten_unit(
            complete_entries(),
            "AmbientCapabilities=",
            "AmbientCapabilities=CAP_SYS_ADMIN",
        )
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_unit(entries)


class ContentManifestTests(unittest.TestCase):
    """The manifest has to be the one the replay expectation sealed."""

    def test_the_manifest_at_the_sealed_digest_passes(self) -> None:
        with manifest_sealed_as(MANIFEST_STAND_IN):
            mod.assert_content_manifest(complete_entries())

    def test_a_missing_manifest_is_refused(self) -> None:
        entries = complete_entries()
        del entries[mod.CONTENT_MANIFEST_GUEST_PATH.lstrip("/")]
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_content_manifest(entries)

    def test_a_manifest_at_a_different_digest_is_refused(self) -> None:
        # One byte, which is all it takes and all this needs to prove.
        entries = complete_entries()
        entries.update(manifest_entry(MANIFEST_STAND_IN.replace(b"[]", b"[ ]")))
        with manifest_sealed_as(MANIFEST_STAND_IN):
            with self.assertRaises(mod.SuccessorProduceError):
                mod.assert_content_manifest(entries)

    def test_a_manifest_of_a_different_size_is_refused(self) -> None:
        # Same length is the easy case to miss; this is the length differing.
        entries = complete_entries()
        entries.update(manifest_entry(MANIFEST_STAND_IN + b"\n"))
        with manifest_sealed_as(MANIFEST_STAND_IN):
            with self.assertRaises(mod.SuccessorProduceError):
                mod.assert_content_manifest(entries)

    def test_a_manifest_staged_without_its_bytes_is_refused(self) -> None:
        # The shape that hid a defect once: an entry carrying a digest it claims
        # rather than the content it is.  There is nothing here to hash, so the
        # only honest answer is a refusal.
        entries = complete_entries()
        key = mod.CONTENT_MANIFEST_GUEST_PATH.lstrip("/")
        entries[key] = {
            "path": key,
            "kind": "file",
            "mode": mod.CONTENT_MANIFEST_MODE,
            "uid": 0,
            "gid": 0,
            "sha256": mod.CONTENT_MANIFEST_SHA256,
            "sizeBytes": mod.CONTENT_MANIFEST_SIZE_BYTES,
        }
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_content_manifest(entries)

    def test_the_checks_read_the_staged_bytes_and_not_a_claimed_digest(self) -> None:
        # The builder stages content, not digests: a real file entry has `raw`
        # and no `sha256` at all, and a real symlink entry carries a target and
        # no bytes.  Every one of these checks has to read what is there.
        for entry in complete_entries().values():
            if entry["kind"] == "symlink":
                self.assertIn("target", entry)
                self.assertNotIn("raw", entry)
            else:
                self.assertIn("raw", entry)
            self.assertNotIn("sha256", entry)
            self.assertNotIn("sizeBytes", entry)

    def test_the_sealed_digest_is_the_one_the_replay_expectation_carries(self) -> None:
        expected = read_json(REPLAY_EXPECTATION_PATH)["expectedOutput"]
        self.assertEqual(
            expected["rootfsContentManifestSha256"], mod.CONTENT_MANIFEST_SHA256
        )
        self.assertEqual(
            expected["rootfsContentManifestSizeBytes"], mod.CONTENT_MANIFEST_SIZE_BYTES
        )

    def test_the_same_digest_is_the_one_the_measurement_carries(self) -> None:
        nested = read_json(MEASUREMENT_PATH)["nestedContentManifest"]
        self.assertEqual(nested["sha256"], mod.CONTENT_MANIFEST_SHA256)
        self.assertEqual(nested["sizeBytes"], mod.CONTENT_MANIFEST_SIZE_BYTES)
        self.assertEqual(nested["guestPath"], mod.CONTENT_MANIFEST_GUEST_PATH)


class GapReadbackTests(unittest.TestCase):
    """The three gaps read out of the written tree, not out of the table.

    The authority asks the preflight to read the closed gaps back out of the
    assembled tree "rather than out of the declarations naming them".  The entry
    table is a declaration too -- it is what the writer was asked for.  This is
    what the writer did, and it is that tree the image would be made from.
    """

    def test_a_complete_tree_yields_evidence_for_all_three(self) -> None:
        with written_tree(complete_entries()) as destination:
            evidence = mod.gap_evidence(complete_entries(), destination)
        self.assertEqual(
            [row["guestPath"] for row in evidence["accountDatabase"]],
            [row["guestPath"] for row in mod.ACCOUNT_DATABASE],
        )
        self.assertEqual(
            evidence["launcherUnit"]["sha256"],
            mod.LAUNCHER_UNIT_SHA256,
        )
        self.assertEqual(
            evidence["launcherUnit"]["enablement"]["target"],
            mod.LAUNCHER_UNIT_GUEST_PATH,
        )
        self.assertEqual(
            evidence["runtimeContentManifest"]["guestPath"],
            mod.CONTENT_MANIFEST_GUEST_PATH,
        )

    def test_the_recorded_account_rows_carry_what_the_seal_is_checked_against(
        self,
    ) -> None:
        with written_tree(complete_entries()) as destination:
            evidence = mod.gap_evidence(complete_entries(), destination)
        for row, sealed in zip(evidence["accountDatabase"], mod.ACCOUNT_DATABASE):
            self.assertEqual(row["sha256"], sealed["sha256"])
            self.assertEqual(row["mode"], f"{sealed['mode']:04o}")
            self.assertEqual(row["uid"], 0)
            self.assertEqual(row["gid"], 0)
            self.assertGreater(row["sizeBytes"], 0)

    def test_an_account_file_the_writer_did_not_write_is_refused(self) -> None:
        with written_tree(complete_entries()) as destination:
            (destination / "etc/shadow").unlink()
            with self.assertRaises(mod.SuccessorProduceError):
                mod.gap_evidence(complete_entries(), destination)

    def test_an_account_file_the_writer_changed_is_refused(self) -> None:
        with written_tree(complete_entries()) as destination:
            path = destination / "etc/passwd"
            path.chmod(0o644)
            path.write_bytes(b"root:x:0:0:root:/root:/bin/sh\n")
            with self.assertRaises(mod.SuccessorProduceError):
                mod.gap_evidence(complete_entries(), destination)

    def test_a_unit_the_writer_did_not_write_is_refused(self) -> None:
        with written_tree(complete_entries()) as destination:
            (destination / mod.LAUNCHER_UNIT_GUEST_PATH.lstrip("/")).unlink()
            with self.assertRaises(mod.SuccessorProduceError):
                mod.gap_evidence(complete_entries(), destination)

    def test_an_enablement_link_missing_from_the_written_tree_is_refused(self) -> None:
        with written_tree(complete_entries()) as destination:
            (destination / ENABLEMENT_GUEST_PATH.lstrip("/")).unlink()
            with self.assertRaises(mod.SuccessorProduceError):
                mod.gap_evidence(complete_entries(), destination)

    def test_an_enablement_link_the_writer_pointed_elsewhere_is_refused(self) -> None:
        with written_tree(complete_entries()) as destination:
            link = destination / ENABLEMENT_GUEST_PATH.lstrip("/")
            link.unlink()
            link.symlink_to("/usr/lib/systemd/system/getty.service")
            with self.assertRaises(mod.SuccessorProduceError):
                mod.gap_evidence(complete_entries(), destination)

    def test_a_manifest_the_writer_changed_is_refused(self) -> None:
        with written_tree(complete_entries()) as destination:
            path = destination / mod.CONTENT_MANIFEST_GUEST_PATH.lstrip("/")
            path.chmod(0o644)
            path.write_bytes(b'{"entries": [], "stand-in": false}\n')
            with self.assertRaises(mod.SuccessorProduceError):
                mod.gap_evidence(complete_entries(), destination)

    def test_the_evidence_says_nothing_about_ownership_on_disk(self) -> None:
        # A preflight that is not root cannot reproduce ownership, and the
        # writer says so.  The owner each file carries into the image comes from
        # the entry the image writer copies it from, so that is where the
        # recorded uid and gid come from -- and reading them off this tree
        # instead would record whoever happened to run the preflight.
        source = inspect.getsource(mod.gap_evidence)
        self.assertNotIn("st_uid", source)
        self.assertNotIn("st_gid", source)


class LauncherBinaryTests(unittest.TestCase):
    """The rebuilt launcher answers to the seal, by digest and by size."""

    def test_the_sealed_values_are_the_build_results_own(self) -> None:
        sealed = read_json(LAUNCHER_RESULT_PATH)["launcher"]
        self.assertEqual(sealed["sha256"], mod.LAUNCHER_SHA256)
        self.assertEqual(sealed["sizeBytes"], mod.LAUNCHER_SIZE_BYTES)
        self.assertEqual(sealed["guestLogicalPath"], mod.LAUNCHER_GUEST_PATH)
        self.assertEqual(mod.LAUNCHER_SIZE_BYTES, 2006632)

    def test_a_rebuilt_launcher_at_a_different_digest_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_launcher_binary(b"\x00" * mod.LAUNCHER_SIZE_BYTES)

    def test_a_rebuilt_launcher_of_a_different_size_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError) as caught:
            mod.assert_launcher_binary(b"\x00" * (mod.LAUNCHER_SIZE_BYTES - 1))
        self.assertIn(str(mod.LAUNCHER_SIZE_BYTES), str(caught.exception))

    def test_the_launcher_source_and_seal_are_not_touched_by_this_path(self) -> None:
        source = module_source(mod)
        self.assertNotIn("cargo", source)
        self.assertNotIn("boole-native-shadow-launcher/src", source)


class MeasurementAgreementTests(unittest.TestCase):
    """The numbers were frozen before the run; the run has to reach them."""

    def test_the_sealed_measurement_is_the_one_that_was_sealed(self) -> None:
        self.assertEqual(mod.MEASUREMENT_SHA256, digest_of(MEASUREMENT_PATH))

    def test_a_measurement_whose_bytes_moved_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.sealed_measurement(path=SUCCESSOR_LOCK_PATH)

    def test_the_frozen_totals_are_the_sealed_records_own(self) -> None:
        record = mod.sealed_measurement()
        self.assertEqual(record["independentTraversal"], record["builderInternal"])
        self.assertEqual(mod.EXPECTED_WITHOUT_LAUNCHER, record["independentTraversal"])
        self.assertEqual(mod.EXPECTED_WITH_LAUNCHER, record["withSealedLauncher"])
        self.assertEqual(mod.EXPECTED_WITHOUT_LAUNCHER["entries"], 17674)
        self.assertEqual(mod.EXPECTED_WITH_LAUNCHER["entries"], 17676)
        self.assertEqual(mod.EXPECTED_WITH_LAUNCHER["payloadBytes"], 1773456499)

    def test_the_frozen_totals_pass(self) -> None:
        mod.assert_totals(walked(), complete())

    def test_a_different_entry_count_without_the_launcher_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_totals(walked(entries=17673), complete())

    def test_a_different_entry_count_with_the_launcher_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_totals(walked(), complete(entries=17675))

    def test_a_different_final_payload_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_totals(walked(), complete(payloadBytes=1773456498))

    def test_a_different_path_manifest_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_totals(walked(pathManifestSha256="0" * 64), complete())

    def test_a_different_kind_breakdown_is_refused(self) -> None:
        broken = dict(mod.EXPECTED_WITHOUT_LAUNCHER["byKind"], file=15100)
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_totals(walked(byKind=broken), complete())

    def test_the_two_counts_differ_by_the_launcher_and_its_parent(self) -> None:
        self.assertEqual(
            mod.EXPECTED_WITH_LAUNCHER["entries"]
            - mod.EXPECTED_WITHOUT_LAUNCHER["entries"],
            2,
        )
        self.assertEqual(
            mod.EXPECTED_WITH_LAUNCHER["payloadBytes"]
            - mod.EXPECTED_WITHOUT_LAUNCHER["payloadBytes"],
            mod.LAUNCHER_SIZE_BYTES,
        )

    def test_the_two_added_entries_are_recorded_one_row_each(self) -> None:
        rows = mod.PRODUCTION_BOUND_ADDITIONS
        self.assertEqual(len(rows), 2)
        self.assertEqual(
            [row["guestPath"] for row in rows],
            ["/usr/libexec/boole", mod.LAUNCHER_GUEST_PATH],
        )
        self.assertEqual([row["kind"] for row in rows], ["directory", "file"])
        self.assertEqual([row["sizeBytes"] for row in rows], [0, mod.LAUNCHER_SIZE_BYTES])


class ConflictTests(unittest.TestCase):
    """A collision, a duplicate or an escape is one refusal, not a note."""

    def test_a_clean_walk_passes(self) -> None:
        mod.assert_no_conflicts(walked())

    def test_a_path_collision_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_no_conflicts(walked(pathCollisions=1))

    def test_a_duplicate_path_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_no_conflicts(walked(duplicatePaths=1))

    def test_a_symlink_that_leaves_the_tree_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_no_conflicts(walked(symlinkEscapes=1))


class LimitTests(unittest.TestCase):
    """Refuse on exceeding; never shrink to fit."""

    def test_the_limits_are_the_sealed_recipes_own_numbers(self) -> None:
        recipe = read_json(SUCCESSOR_LOCK_PATH)["buildRecipe"]
        self.assertEqual(mod.LIMITS["maxEntries"], recipe["maxEntries"])
        self.assertEqual(mod.LIMITS["maxFileBytes"], recipe["maxFileBytes"])
        self.assertEqual(mod.LIMITS["maxTotalBytes"], recipe["maxTotalBytes"])
        self.assertEqual(mod.LIMITS, read_json(MEASUREMENT_PATH)["limits"])

    def test_the_frozen_totals_are_inside_the_limits(self) -> None:
        mod.assert_within_limits(mod.LIMITS, complete())

    def test_too_many_entries_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_within_limits(mod.LIMITS, complete(entries=200001))

    def test_too_many_payload_bytes_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_within_limits(mod.LIMITS, complete(payloadBytes=2147483649))

    def test_one_file_over_the_single_file_limit_is_refused(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_within_limits(mod.LIMITS, complete(largestFileBytes=536870913))

    def test_nothing_is_shortened_sampled_or_dropped_to_fit(self) -> None:
        source = module_source(mod)
        for word in ("truncate", "islice", "random.sample", "[:limit]"):
            self.assertNotIn(word, source)


class PreflightProducesNothingTests(unittest.TestCase):
    """The no-output run cannot reach an image tool, by construction."""

    def test_the_module_never_names_an_image_tool(self) -> None:
        # The list is the sealed measurement's, not a second copy of it, so this
        # file has no occasion to write any of those names down at all.
        source = module_source(mod)
        for tool in mod.FORBIDDEN_IN_PREFLIGHT:
            self.assertEqual(source.count(f'"{tool}"'), 0, f"{tool} is named here")

    def test_each_forbidden_tool_is_refused_before_it_is_run(self) -> None:
        for tool in mod.FORBIDDEN_IN_PREFLIGHT:
            with self.assertRaises(mod.SuccessorProduceError, msg=tool):
                mod.assert_preflight_tool(pathlib.Path("/usr/sbin") / tool)

    def test_the_forbidden_list_is_the_sealed_measurements_own(self) -> None:
        self.assertIs(mod.FORBIDDEN_IN_PREFLIGHT, measure.FORBIDDEN_EXECUTABLES)
        for named_by_the_directive in ("mke2fs", "mkfs.ext4", "mkinitramfs", "debugfs"):
            self.assertIn(named_by_the_directive, mod.FORBIDDEN_IN_PREFLIGHT)

    def test_the_allow_list_is_the_sealed_measurements_own(self) -> None:
        self.assertIs(mod.ALLOWED_REPLAY_TOOLS, measure.ALLOWED_REPLAY_TOOLS)

    def test_only_the_two_replay_tools_are_allowed(self) -> None:
        self.assertEqual(sorted(mod.ALLOWED_REPLAY_TOOLS), ["gpgv", "zstd"])

    def test_the_subprocess_policy_is_default_deny(self) -> None:
        # Anything off the allow list is refused, including a tool nobody thought
        # to forbid.  A policy that only refused a named list would pass an
        # unknown binary, which is the failure this asks about.
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_preflight_tool(pathlib.Path("/usr/bin/curl"))

    def test_an_allowed_replay_tool_passes(self) -> None:
        mod.assert_preflight_tool(pathlib.Path("/usr/bin/gpgv"))

    def test_the_module_runs_no_subprocess_outside_the_gateway(self) -> None:
        mod.assert_single_subprocess_gateway()

    def test_the_preflight_never_dispatches_a_workflow(self) -> None:
        source = module_source(mod)
        for word in ("workflow_dispatch", "gh workflow", "actions/runs"):
            self.assertNotIn(word, source)

    def test_the_preflight_signature_takes_no_output_directory(self) -> None:
        parameters = inspect.signature(mod.preflight).parameters
        for forbidden in ("outputs", "output_dir", "out_dir"):
            self.assertNotIn(forbidden, parameters)

    def test_the_production_signature_is_the_one_that_takes_outputs(self) -> None:
        self.assertIn("outputs", inspect.signature(mod.produce).parameters)


class WorkflowAcquisitionTests(unittest.TestCase):
    """The preflight has to reach the store the production would have reached.

    The header over the workflow says the preflight does everything the
    production does except the part that costs the attempt.  Acquisition is not
    that part.  A preflight missing one acquirer reads a store the production
    never would have, and then answers a different question than the one it is
    being run to answer -- in either direction.  It can fail on a store the
    production would have filled, which is the cheap way to find out, and it can
    pass on one the production would have refused, which is not.

    The ext4 writer set is deliberately *not* required here.  That one is the
    tool that writes the image rather than an input the staging tree reads, and
    asking the no-output mode to fetch an image writer would undo the mode.
    """

    STAGING_ACQUIRERS = (
        "scripts/native_shadow_boot_rustdist_acquire_arm64_v1.py",
        "scripts/native_shadow_boot_ci_payload_acquire_arm64_v1.py",
    )
    IMAGE_WRITER_ACQUIRER = "scripts/native_shadow_boot_writer_set_acquire_arm64_v1.py"
    # The assembling step, which the wrapper now runs inside the production's
    # own transient unit rather than beside it. The assertion below is the one
    # it always was -- acquisition first -- and only the name of the step it
    # points at has moved.
    ASSEMBLY = "--preflight-only"

    def test_the_production_acquires_both_staging_inputs(self) -> None:
        block = workflow_job("produce")
        for acquirer in self.STAGING_ACQUIRERS:
            self.assertIn(acquirer, block)

    def test_the_preflight_acquires_every_staging_input_the_production_does(
        self,
    ) -> None:
        block = workflow_job("preflight")
        for acquirer in self.STAGING_ACQUIRERS:
            self.assertIn(acquirer, block, f"the preflight never runs {acquirer}")

    def test_each_job_acquires_the_toolchain_before_the_packages(self) -> None:
        # The package acquirer refuses a store without the distribution and says
        # which tool fills it, so the wrong order is a stop rather than a wrong
        # store -- but it is a stop in the job that was meant to be the cheap one.
        rustdist, payloads = self.STAGING_ACQUIRERS
        for job in ("preflight", "produce"):
            block = workflow_job(job)
            self.assertLess(block.index(rustdist), block.index(payloads), job)

    def test_the_preflight_acquires_before_it_assembles(self) -> None:
        block = workflow_job("preflight")
        for acquirer in self.STAGING_ACQUIRERS:
            self.assertLess(block.index(acquirer), block.index(self.ASSEMBLY), acquirer)

    def test_the_preflight_does_not_fetch_the_image_writer(self) -> None:
        self.assertNotIn(self.IMAGE_WRITER_ACQUIRER, workflow_job("preflight"))

    def test_the_production_does_fetch_the_image_writer(self) -> None:
        self.assertIn(self.IMAGE_WRITER_ACQUIRER, workflow_job("produce"))


class PreflightRecordTests(unittest.TestCase):
    """What the sealed result has to carry, checked against the authority's list.

    The preflight itself cannot run here -- it needs the payload store and an
    arm64 host -- so what is checked is the document it returns: which keys it
    has, and that every one of them comes from a value this file computed rather
    than from a sentence claiming it was computed.
    """

    def returned_keys(self, function) -> set:
        tree = ast.parse(inspect.getsource(function))
        for node in ast.walk(tree):
            if isinstance(node, ast.Return) and isinstance(node.value, ast.Dict):
                return {key.value for key in node.value.keys}
        raise AssertionError(f"{function.__name__} returns no dict literal")

    def test_the_document_answers_every_pass_requirement(self) -> None:
        keys = self.returned_keys(mod.preflight)
        for required in (
            "builderInternal",
            "gapEvidence",
            "independentTraversal",
            "launcher",
            "limits",
            "nestedContentManifest",
            "provenance",
            "withSealedLauncher",
        ):
            self.assertIn(required, keys)

    def test_the_gaps_are_read_off_the_written_tree_by_the_preflight(self) -> None:
        self.assertIn("gap_evidence(", inspect.getsource(mod.preflight))

    def test_the_provenance_names_every_sealed_input_it_read(self) -> None:
        record = mod.provenance(
            repository_root=REPO_ROOT,
            artifact_store=pathlib.Path("/store"),
            gpgv=pathlib.Path("/usr/bin/gpgv"),
            zstd=pathlib.Path("/usr/bin/zstd"),
        )
        self.assertEqual(record["authoritySha256"], mod.AUTHORITY_SHA256)
        self.assertEqual(record["sourceLockSha256"], mod.SOURCE_LOCK_SHA256)
        self.assertEqual(record["measurementSha256"], mod.MEASUREMENT_SHA256)
        self.assertEqual(record["tools"]["gpgv"], "/usr/bin/gpgv")
        self.assertEqual(record["tools"]["zstd"], "/usr/bin/zstd")
        self.assertEqual(record["artifactStore"], "/store")

    def test_the_provenance_module_digests_are_the_ones_it_verified(self) -> None:
        modules = mod.provenance(
            repository_root=REPO_ROOT,
            artifact_store=pathlib.Path("/store"),
            gpgv=pathlib.Path("/usr/bin/gpgv"),
            zstd=pathlib.Path("/usr/bin/zstd"),
        )["modules"]
        self.assertEqual(modules[builder.__name__], mod.BUILDER_SHA256)
        self.assertEqual(modules[successor_gate.__name__], mod.RELEASE_GATE_SHA256)
        self.assertEqual(modules[mod.__name__], digest_of(pathlib.Path(mod.__file__)))

    def test_the_provenance_records_the_host_the_run_happened_on(self) -> None:
        platform = mod.provenance(
            repository_root=REPO_ROOT,
            artifact_store=pathlib.Path("/store"),
            gpgv=pathlib.Path("/usr/bin/gpgv"),
            zstd=pathlib.Path("/usr/bin/zstd"),
        )["platform"]
        self.assertEqual(platform["system"], os.uname().sysname)
        self.assertEqual(platform["machine"], os.uname().machine)

    def test_the_provenance_claims_nothing_it_did_not_read(self) -> None:
        # Every digest in the record is either recomputed here or one of this
        # file's own sealed constants.  A provenance block that carried a value
        # nobody read would be a sentence, not evidence.
        source = inspect.getsource(mod.provenance)
        self.assertNotIn('"true"', source)
        self.assertNotIn("bootable", source.lower())
        self.assertNotIn("serving", source.lower())


class BudgetBoundaryTests(unittest.TestCase):
    """Unspent before the marker is written; spent after."""

    def test_the_boundary_is_the_authoritys_own_words(self) -> None:
        # The third authority draws the line at the marker rather than at the
        # output directory, which is where the code had already drawn it. Two
        # dispatches showed why: the first created the directory and no file in
        # it, and an operator had to decide what that meant.
        rule = mod.authority()["budgetBoundary"]["rule"]
        self.assertIn(f"before {mod.CONSUMED_MARKER_NAME} exists", rule)
        self.assertIn("the attempt is consumed whatever happens next", rule)

    def test_the_older_wording_is_preserved_rather_than_corrected(self) -> None:
        # The authority the prior dispatches ran under still says what it said.
        # Moving the line forward is a statement about the next attempt, not a
        # revision of how the last two were judged.
        older = read_json(SPENT_AUTHORITY_PATH)["budgetBoundary"]["rule"]
        self.assertIn("before the production output directory exists", older)

    def test_a_refusal_before_the_marker_is_unspent(self) -> None:
        self.assertFalse(mod.attempt_consumed(marker_written=False))

    def test_a_failure_after_the_marker_is_spent(self) -> None:
        self.assertTrue(mod.attempt_consumed(marker_written=True))

    def test_the_preflight_can_never_create_an_output_directory(self) -> None:
        mod.assert_preflight_creates_no_outputs()

    def test_a_consumed_attempt_is_not_retried(self) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            mod.assert_attempt_available(runs_performed=1)

    def test_an_unspent_attempt_is_available(self) -> None:
        mod.assert_attempt_available(runs_performed=0)


class CommandLineTests(unittest.TestCase):
    """The two modes are separate all the way out to what a workflow types."""

    def test_the_preflight_mode_never_reaches_production(self) -> None:
        # The mode is an argument, so the guarantee cannot come from the call
        # graph the way it does inside the module.  Production is replaced by
        # something that fails if it is entered at all, and the preflight run is
        # then allowed to fail for its own reasons -- just never that one.
        def refuse(**_kwargs):
            raise AssertionError("the preflight mode entered production")

        with tempfile.TemporaryDirectory() as scratch:
            scratch = pathlib.Path(scratch)
            launcher = scratch / "launcher"
            launcher.write_bytes(b"not the sealed launcher")
            with mock.patch.object(mod, "produce", refuse), restored_temporary_directory():
                with self.assertRaises(mod.SuccessorProduceError):
                    mod.main(
                        [
                            "preflight",
                            "--cas",
                            str(scratch / "cas"),
                            "--launcher",
                            str(launcher),
                            "--scratch",
                            str(scratch / "s"),
                            "--result",
                            str(scratch / "result.json"),
                        ]
                    )

    def test_the_preflight_mode_takes_no_output_directory(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                mod._parser().parse_args(
                    ["preflight", "--cas", "c", "--launcher", "l", "--scratch", "s",
                     "--result", "r", "--outputs", "o"]
                )

    def test_production_mode_requires_an_output_directory(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                mod._parser().parse_args(
                    ["produce", "--cas", "c", "--launcher", "l", "--scratch", "s",
                     "--result", "r"]
                )

    def test_a_sealed_result_is_never_overwritten(self) -> None:
        # Append-only is the property; refusing to replace is how it is kept.
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "result.json"
            first = mod._write_once(path, {"release": mod.RELEASE})
            self.assertEqual(first, digest_of(path))
            with self.assertRaises(mod.SuccessorProduceError):
                mod._write_once(path, {"release": mod.RELEASE})

    def test_the_module_imports_when_it_is_run_the_way_a_workflow_runs_it(
        self,
    ) -> None:
        # Every test above imports this module as `scripts.<name>`, which puts
        # the repository root on the path before the module is even read.  A
        # workflow types `python3 scripts/<name>.py`, which puts `scripts/` there
        # instead -- and then the module's own `from scripts import ...` block
        # has nothing to import from.  No amount of importing it here would ever
        # notice, so it is run the other way, in a subprocess, with PYTHONPATH
        # taken away so an inherited one cannot answer for it.
        environment = dict(os.environ)
        environment.pop("PYTHONPATH", None)
        finished = subprocess.run(
            [sys.executable, str(pathlib.Path(mod.__file__).resolve())],
            capture_output=True,
            cwd=REPO_ROOT,
            env=environment,
            text=True,
        )
        self.assertNotIn("ModuleNotFoundError", finished.stderr)
        # Reaching argparse is the proof the import block completed; a usage
        # error is the expected end of a run with no subcommand.
        self.assertEqual(finished.returncode, 2, finished.stderr)

    def test_the_predecessor_puts_the_root_on_the_path_and_so_does_this(
        self,
    ) -> None:
        for module in (predecessor, mod):
            source = module_source(module)
            insert = source.index("sys.path.insert(0")
            self.assertLess(
                insert,
                source.index("from scripts import"),
                f"{module.__name__} imports the package before it can be found",
            )

    def test_the_result_paths_are_the_ones_the_authority_named(self) -> None:
        document = mod.authority()
        self.assertEqual(
            document["preflightResultPath"],
            "native/containment/"
            "native-shadow-mac3-successor-preflight-result-arm64-v1.json",
        )
        self.assertEqual(
            document["resultPath"],
            "native/containment/"
            "native-shadow-mac3-successor-image-production-result-arm64-v3.json",
        )
        self.assertNotEqual(
            document["resultPath"],
            read_json(SPENT_AUTHORITY_PATH)["resultPath"],
            msg="the new attempt writes where a spent attempt would have",
        )
        self.assertNotEqual(document["resultPath"], document["preflightResultPath"])


class PredecessorIsUntouchedTests(unittest.TestCase):
    """The historical path stays exactly as it is."""

    def test_the_predecessor_still_reads_the_predecessor_lock(self) -> None:
        self.assertTrue(
            str(predecessor.BOOT_SOURCE_LOCK_PATH).endswith(
                "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
            )
        )

    def test_the_predecessor_still_passes_no_nested_tree(self) -> None:
        self.assertNotIn("nested_tree", module_source(predecessor))

    def test_the_predecessor_release_is_still_its_own(self) -> None:
        self.assertEqual(predecessor.RELEASE, "NATIVE-SHADOW-BOOT-PRODUCE-PHASE-ARM64-V1")

    def test_the_successor_never_reaches_the_predecessor_lock_reader(self) -> None:
        # The image steps are shared tools and reusing them is what keeps the two
        # paths from drifting into two different disks.  What must never be
        # reached is the part of the historical phase that decides which lock is
        # being built: its own reader, and its own produce.
        #
        # Read from this module's syntax rather than from its text.  A substring
        # search cannot answer this: the release gate's own materialize_runtime_lock
        # and the base projection's normalized_runtime_lock end in the same
        # letters as the reader being refused, and both are legitimate.  What
        # matters is which object an attribute is taken from, which is a question
        # about the tree and not about the characters.
        mod.assert_historical_phase_scope()
        aliases = predecessor_aliases_in(mod)
        self.assertTrue(aliases, "the successor imports the historical phase")
        reached = set()
        for node in ast.walk(ast.parse(module_source(mod))):
            if (
                isinstance(node, ast.Attribute)
                and isinstance(node.value, ast.Name)
                and node.value.id in aliases
            ):
                reached.add(node.attr)
        self.assertEqual(sorted(reached & set(mod.HISTORICAL_PHASE_REFUSED)), [])

    def test_the_refused_set_is_derived_from_which_lock_a_function_reads(self) -> None:
        # Not a list somebody kept up to date: it is every function over there
        # that names the historical lock, computed from that module's own text.
        # A helper added there later that reads it becomes unreachable from here
        # without anyone remembering to add it.
        self.assertEqual(
            sorted(mod.HISTORICAL_PHASE_REFUSED),
            ["_runtime_lock", "assert_staged_ctime_cause_removed", "produce"],
        )

    def test_the_successor_claims_nothing_it_did_not_do(self) -> None:
        self.assertFalse(mod.BOOTABLE_CLAIM)
        self.assertFalse(mod.SERVING_CLAIM)
        self.assertFalse(mod.IMAGE_PRODUCED_CLAIM)
        self.assertFalse(mod.ACTIVATION_ALLOWED)


class SealedPreflightResultTests(unittest.TestCase):
    """What the arm64 run answered, kept where it can be checked rather than read.

    The preflight is repeatable, so a result is only worth sealing if the bytes
    sealed are the bytes the runner wrote.  These pin the digest the run reported
    and then re-derive every claim in the record from the sealed files beside it,
    so a hand-edited or re-typed result fails here rather than being believed by
    the production that reads it.

    The record is a measurement.  It establishes that the successor path
    assembles the sealed tree with the three gaps closed in it; it does not
    produce an image, and nothing in it may say otherwise.
    """

    RUN_ID = "33164208857"
    RESULT_SHA256 = "be4a84e1c058fa25804cfade07727e35613369f58b0307182b93f24a4ecfb071"
    AGREEMENT_KEYS = (
        "entries",
        "byKind",
        "payloadBytes",
        "largestFileBytes",
        "largestFilePath",
        "pathManifestSha256",
        "caseFoldedSiblings",
        "duplicatePaths",
        "symlinkEscapes",
    )

    def setUp(self) -> None:
        self.result = read_json(PREFLIGHT_RESULT_PATH)

    def test_the_sealed_bytes_are_the_bytes_the_run_wrote(self) -> None:
        self.assertEqual(digest_of(PREFLIGHT_RESULT_PATH), self.RESULT_SHA256)

    def test_it_sits_where_the_authority_said_it_would(self) -> None:
        named = mod.authority()["preflightResultPath"]
        self.assertEqual(
            PREFLIGHT_RESULT_PATH, REPO_ROOT / named, msg=f"authority names {named}"
        )

    def test_both_sides_agree_with_each_other_and_with_the_seal(self) -> None:
        """The comparison that refused three runs ago, kept as a sealed answer.

        Nine quantities, three sides: what the assembled table says, what an
        independent walk of the written tree says, and what the measurement
        sealed before any of this existed.  ``largestFilePath`` is in here
        because it is the one that disagreed, and it agrees now for a reason --
        the tie is broken on the path's own bytes on both sides.
        """

        sealed = read_json(MEASUREMENT_PATH)
        for key in self.AGREEMENT_KEYS:
            table = self.result["builderInternal"][key]
            walk = self.result["independentTraversal"][key]
            self.assertEqual(table, walk, msg=f"the two sides disagree on {key}")
            self.assertEqual(
                table, sealed["builderInternal"][key], msg=f"{key} is not the sealed one"
            )

    def test_the_launcher_projection_is_the_sealed_one(self) -> None:
        sealed = read_json(MEASUREMENT_PATH)
        self.assertEqual(self.result["withSealedLauncher"], sealed["withSealedLauncher"])
        launcher = self.result["launcher"]
        self.assertEqual(launcher["rebuiltSha256"], sealed["launcher"]["sealedSha256"])
        self.assertEqual(launcher["sealedSha256"], launcher["rebuiltSha256"])
        self.assertEqual(launcher["sealedSizeBytes"], sealed["launcher"]["sealedSizeBytes"])
        self.assertEqual(launcher["sealedSizeBytes"], 2006632)
        self.assertFalse(launcher["includedInTheMeasuredTree"])

    def test_the_two_added_entries_are_recorded_one_row_each(self) -> None:
        rows = self.result["productionBoundAdditions"]
        self.assertEqual(len(rows), 2)
        self.assertEqual(
            [(row["guestPath"], row["kind"], row["sizeBytes"]) for row in rows],
            [
                ("/usr/libexec/boole", "directory", 0),
                ("/usr/libexec/boole/boole-native-shadow-launcher", "file", 2006632),
            ],
        )
        self.assertEqual(
            self.result["withSealedLauncher"]["entries"]
            - self.result["builderInternal"]["entries"],
            len(rows),
        )

    def test_the_three_gaps_are_closed_in_the_tree_that_was_assembled(self) -> None:
        gaps = self.result["gapEvidence"]
        self.assertEqual(
            [row["guestPath"] for row in gaps["accountDatabase"]],
            [
                "/etc/group",
                "/etc/gshadow",
                "/etc/nsswitch.conf",
                "/etc/passwd",
                "/etc/shadow",
            ],
        )
        self.assertEqual(
            [row["uid"] for row in gaps["accountDatabase"]], [0, 0, 0, 0, 0]
        )
        unit = gaps["launcherUnit"]
        self.assertEqual(
            unit["capabilityBoundingSet"],
            ["CAP_SETGID", "CAP_SETUID", "CAP_SETPCAP", "CAP_SYS_ADMIN"],
        )
        self.assertEqual(unit["directives"]["StandardOutput"], "journal+console")
        self.assertEqual(unit["directives"]["StandardError"], "journal+console")
        self.assertEqual(unit["directives"]["AmbientCapabilities"], "")
        self.assertEqual(
            gaps["runtimeContentManifest"]["sha256"],
            read_json(MEASUREMENT_PATH)["nestedContentManifest"]["sha256"],
        )

    def test_it_reads_the_successor_lock_and_the_successor_authority(self) -> None:
        self.assertEqual(self.result["sourceLockSha256"], digest_of(SUCCESSOR_LOCK_PATH))
        self.assertNotEqual(
            self.result["sourceLockSha256"], digest_of(PREDECESSOR_LOCK_PATH)
        )
        self.assertEqual(
            self.result["authoritySha256"], digest_of(SPENT_AUTHORITY_PATH)
        )
        self.assertEqual(
            self.result["provenance"]["measurementSha256"], digest_of(MEASUREMENT_PATH)
        )
        self.assertEqual(
            self.result["provenance"]["launcherBuildResultSha256"],
            digest_of(LAUNCHER_RESULT_PATH),
        )

    def test_the_run_that_wrote_it_created_nothing_and_claims_nothing(self) -> None:
        self.assertFalse(self.result["outputsCreated"])
        for claim in (
            "imageProducedClaim",
            "bootableClaim",
            "servingClaim",
            "activationAllowed",
        ):
            self.assertFalse(self.result[claim], msg=claim)

    def test_the_totals_are_inside_the_sealed_limits(self) -> None:
        limits = self.result["limits"]
        with_launcher = self.result["withSealedLauncher"]
        self.assertLess(with_launcher["entries"], limits["maxEntries"])
        self.assertLess(with_launcher["largestFileBytes"], limits["maxFileBytes"])
        self.assertLess(with_launcher["payloadBytes"], limits["maxTotalBytes"])


if __name__ == "__main__":
    unittest.main()


WRAPPER_PATH = REPO_ROOT / "scripts/native-shadow-successor-produce-arm64.sh"


class TemporaryDirectoryInsideTheIsolationTests(unittest.TestCase):
    """The production runs where the whole filesystem is read-only but two
    directories, and the transient unit does not inherit the caller's
    environment.  Python's default temporary directory is therefore unusable
    there, and two helpers deep in the shared builder -- the InRelease
    signature check and the zstd decompressor -- ask for one without naming a
    place.  Both are on the produce path and neither is this wave's to edit:
    the predecessor image is built from the same base module and has to stay
    reproducible.

    So the phase names the place once, for every caller and every indirect
    use, out of the scratch it was already handed -- the directory the
    isolation was already told it may write.  Pinning it is not a widening of
    the isolation; it points at what the unit could always write.
    """

    def setUp(self) -> None:
        self.previous = tempfile.tempdir
        self.addCleanup(self._restore)
        tempfile.tempdir = None
        self.root = pathlib.Path(
            tempfile.mkdtemp(prefix="boole-successor-tempdir.")
        )
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)

    def _restore(self) -> None:
        tempfile.tempdir = self.previous

    def test_the_phase_pins_a_temporary_directory_under_the_scratch(self) -> None:
        scratch = self.root / "scratch"
        pinned = mod.pin_temporary_directory(scratch)
        self.assertEqual(pinned, scratch / "tmp")
        self.assertTrue(pinned.is_dir())

    def test_a_caller_that_names_no_place_lands_under_the_scratch(self) -> None:
        # The failure this refuses: a helper that asks for a temporary
        # directory without naming one, and is answered with a read-only /tmp.
        scratch = self.root / "scratch"
        pinned = mod.pin_temporary_directory(scratch)
        self.assertEqual(pathlib.Path(tempfile.gettempdir()), pinned)
        with tempfile.TemporaryDirectory(prefix="boole-probe.") as probe:
            self.assertEqual(pathlib.Path(probe).parent, pinned)

    def test_the_pin_happens_before_the_phase_reads_any_input(self) -> None:
        # Ordering, not merely presence.  A pin taken after the first read is
        # a pin the failing run would still have missed.
        scratch = self.root / "early"
        with self.assertRaises(mod.SuccessorProduceError):
            mod.main(
                [
                    "preflight",
                    "--cas",
                    str(self.root / "absent-store"),
                    "--launcher",
                    str(self.root / "absent-launcher"),
                    "--scratch",
                    str(scratch),
                    "--result",
                    str(self.root / "unwritten.json"),
                ]
            )
        self.assertEqual(pathlib.Path(tempfile.gettempdir()), scratch / "tmp")


class ProvenIsolationBeforeTheBudgetLineTests(unittest.TestCase):
    """The run that spent an attempt passed a preflight and then failed in
    production on the same tree, because the two ran in different places: the
    preflight beside the unit, the production inside it.  A preflight that
    cannot be reached by the production's environment cannot speak for it.

    The wrapper therefore runs the preflight through the same sealed unit
    first, and creates the output directory only after that has passed --
    which also puts the budget line where the phase's own comment puts it.
    """

    def setUp(self) -> None:
        self.source = WRAPPER_PATH.read_text(encoding="utf-8")

    def _index(self, needle: str) -> int:
        found = self.source.find(needle)
        self.assertNotEqual(found, -1, "the wrapper does not contain %r" % needle)
        return found

    def test_the_wrapper_runs_a_preflight_inside_the_unit(self) -> None:
        # Through the sealed unit, not beside it: the mode is handed to the
        # same isolation-argv the production is handed to.
        self._index('--read-write-path "$preflight_scratch"')
        self.assertLess(
            self._index("preflight_isolation=("),
            self._index('"${preflight_isolation[@]}"'),
        )

    def test_the_isolated_preflight_runs_before_the_outputs_directory(self) -> None:
        self.assertLess(
            self._index("$preflight_scratch"),
            self._index('mkdir -p "$outputs"'),
        )

    def test_the_isolated_preflight_cannot_write_the_outputs(self) -> None:
        # Its unit is built with one read-write path, not two.  A preflight
        # that could write there would be a preflight able to spend the
        # attempt it exists to protect.
        preflight_argv = self.source[
            self._index("preflight_isolation=(") : self._index('mkdir -p "$outputs"')
        ]
        self.assertIn('--read-write-path "$preflight_scratch"', preflight_argv)
        self.assertNotIn("$outputs", preflight_argv)

    def test_the_isolated_preflight_gets_a_scratch_of_its_own(self) -> None:
        # Sharing the produce scratch would leave a staging tree behind for
        # the production to find, and the production builds its own.
        self.assertIn('preflight_scratch="$scratch/preflight"', self.source)


HARD_STOP_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json"
)


class SpentAttemptHardStopTests(unittest.TestCase):
    """The first production attempt, written down while it is still the last.

    A failure record is worth only what it is bound to, so the digests it
    carries are re-derived from the files on disk rather than read back out of
    itself, and the path it does not occupy is checked as carefully as the one
    it does: the authority names a result path for a production, and a failure
    that sat in it would read like a production that had happened.
    """

    def setUp(self) -> None:
        self.record = json.loads(HARD_STOP_PATH.read_text(encoding="utf-8"))
        self.authority = json.loads(SPENT_AUTHORITY_PATH.read_text(encoding="utf-8"))

    def _sha256(self, path: pathlib.Path) -> str:
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def test_it_does_not_occupy_the_authority_s_result_path(self) -> None:
        result = REPO_ROOT / self.authority["resultPath"]
        self.assertNotEqual(HARD_STOP_PATH, result)
        self.assertFalse(result.exists(), "a production result exists on disk")

    def test_the_attempt_it_names_is_the_authority_s_attempt(self) -> None:
        attempt = self.record["attempt"]
        self.assertEqual(attempt["attemptId"], self.authority["attemptId"])
        self.assertEqual(attempt["dispatches"], self.authority["production"]["dispatches"])
        self.assertEqual(attempt["authoritySha256"], self._sha256(SPENT_AUTHORITY_PATH))
        self.assertEqual(
            attempt["preflightResultSha256"], self._sha256(PREFLIGHT_RESULT_PATH)
        )

    def test_no_output_file_was_written_and_none_is_claimed(self) -> None:
        budget = self.record["budget"]
        self.assertEqual(budget["outputFilesCreated"], 0)
        self.assertEqual(budget["artifactsUploaded"], 0)
        self.assertTrue(budget["outputDirectoryCreated"])
        self.assertEqual(budget["spentVerdict"], "OPERATOR-DECISION-PENDING")
        self.assertEqual(
            sorted(budget["whatWasNotWritten"]),
            sorted(predecessor.output_names()),
        )

    def test_the_rule_it_quotes_is_the_authority_s_own_sentence(self) -> None:
        # A record that paraphrases the rule it is asking about is a record that
        # can shade it. The quote is compared against the sealed authority on
        # disk, so the question the operator answers is the authority's own
        # wording and not this file's account of it.
        disagreement = self.record["budget"]["twoRulesDisagree"]
        self.assertEqual(
            disagreement["sealedAuthorityRule"],
            self.authority["budgetBoundary"]["rule"],
        )
        # And the reason it needs answering: the sentence names the free case
        # and the consumed case, and this run landed between the two.
        self.assertIn("output directory exists", disagreement["sealedAuthorityRule"])
        self.assertIn("output file has been created", disagreement["sealedAuthorityRule"])

    def test_it_claims_nothing_the_run_did_not_establish(self) -> None:
        for claim, value in self.record["boundaries"].items():
            self.assertFalse(value, claim)

    def test_the_correction_widened_no_isolation(self) -> None:
        # The one thing a fix under time pressure is most likely to reach for,
        # and the one thing the standing instruction refuses outright.
        self.assertFalse(self.record["correction"]["isolationRelaxed"])
        self.assertIn(
            "no second production has been dispatched",
            self.record["hardStop"]["noReRun"],
        )


RULING_PATH = (
    CONTAINMENT
    / "native-shadow-mac3-successor-image-production-budget-ruling-arm64-v1.json"
)


class ConsumedMarkerTests(unittest.TestCase):
    """The budget line, moved off a directory and onto a deliberate act.

    The first production failed in the one case the sealed rule did not name:
    an output directory existed, because a systemd ``ReadWritePaths`` entry has
    to exist before the unit starts, and no output file ever did.  A boundary
    that a run can cross without meaning to is a boundary that will be argued
    about afterwards, so the answer is now a file this phase writes on purpose,
    immediately before the first image file, and nothing else.
    """

    def setUp(self) -> None:
        self.outputs = pathlib.Path(
            tempfile.mkdtemp(prefix="boole-consumed-marker.")
        )
        self.addCleanup(shutil.rmtree, self.outputs, True)

    def test_the_boundary_is_the_marker_and_not_the_directory(self) -> None:
        self.assertFalse(mod.attempt_consumed(marker_written=False))
        self.assertTrue(mod.attempt_consumed(marker_written=True))

    def test_it_lands_at_one_name_under_the_outputs(self) -> None:
        self.assertEqual(
            mod.consumed_marker(self.outputs),
            self.outputs / "ATTEMPT-CONSUMED.json",
        )

    def test_writing_it_leaves_the_marker_and_no_half_written_neighbour(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            marker = mod.write_consumed_marker(self.outputs)
        self.assertTrue(marker.is_file())
        self.assertEqual(sorted(p.name for p in self.outputs.iterdir()), [
            "ATTEMPT-CONSUMED.json"
        ])

    def test_a_crash_before_the_rename_leaves_no_marker(self) -> None:
        # The reason the write is a rename and not a write: a run that dies
        # halfway must be unspent, and it can only be unspent if the name that
        # answers the question never held half a document.
        with mock.patch.object(mod.os, "replace", side_effect=OSError("cut")):
            with contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(OSError):
                    mod.write_consumed_marker(self.outputs)
        self.assertFalse(mod.consumed_marker(self.outputs).exists())
        self.assertFalse(
            mod.attempt_consumed(
                marker_written=mod.consumed_marker(self.outputs).is_file()
            )
        )
        self.assertEqual(list(self.outputs.iterdir()), [])

    def test_a_stumble_after_the_rename_does_not_abort_a_committed_run(self) -> None:
        # The mirror of the test above. Before the rename, failing is right and
        # the attempt is unspent. After it the attempt is spent whatever
        # happens, so failing in the part that merely records the spending
        # would throw the one attempt away for nothing.
        with mock.patch.object(mod.os, "fsync", side_effect=OSError("no fsync")):
            with contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(OSError):
                    mod.write_consumed_marker(self.outputs)
        # The file fsync is before the rename, so that one does refuse.
        self.assertFalse(mod.consumed_marker(self.outputs).exists())

        real_fsync = mod.os.fsync
        calls = {"n": 0}

        def only_the_directory_fails(fileno):
            calls["n"] += 1
            if calls["n"] == 1:
                return real_fsync(fileno)
            raise OSError("no directory fsync")

        complaint = io.StringIO()
        with mock.patch.object(mod.os, "fsync", side_effect=only_the_directory_fails):
            with contextlib.redirect_stdout(io.StringIO()):
                with contextlib.redirect_stderr(complaint):
                    marker = mod.write_consumed_marker(self.outputs)
        self.assertTrue(marker.is_file())
        # Continuing quietly would be worse than failing. It says what it lost.
        self.assertIn("directory fsync failed", complaint.getvalue())

    def test_the_mark_never_asks_the_system_for_a_temporary_directory(self) -> None:
        # The defect that spent the first attempt was a temporary directory the
        # sealed unit had taken away, wanted by a helper that had never named
        # one.  The mark is the worst remaining place for that mistake: it runs
        # inside the same unit, and a run refused here is refused after the
        # whole tree has been assembled and before anything has been produced.
        # So it names its own place -- the outputs directory it is about to
        # write into -- and the system is never asked where to put things.
        def there_is_no_temporary_directory(*args, **kwargs):
            raise FileNotFoundError(
                "No usable temporary directory found in "
                "['/tmp', '/var/tmp', '/usr/tmp', '/']"
            )

        with mock.patch.object(
            mod.tempfile, "gettempdir", side_effect=there_is_no_temporary_directory
        ), mock.patch.object(
            mod.tempfile, "gettempdirb", side_effect=there_is_no_temporary_directory
        ):
            with contextlib.redirect_stdout(io.StringIO()):
                marker = mod.write_consumed_marker(self.outputs)
        self.assertTrue(marker.is_file())
        self.assertEqual(
            sorted(p.name for p in self.outputs.iterdir()), ["ATTEMPT-CONSUMED.json"]
        )

    def test_a_second_write_is_refused_rather_than_overwriting(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            mod.write_consumed_marker(self.outputs)
            with self.assertRaises(mod.SuccessorProduceError):
                mod.write_consumed_marker(self.outputs)

    def test_it_says_the_attempt_is_spent_whatever_happens_next(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            marker = mod.write_consumed_marker(self.outputs)
        document = json.loads(marker.read_text(encoding="utf-8"))
        self.assertTrue(document["consumed"])
        self.assertEqual(document["attemptId"], mod.authority()["attemptId"])
        self.assertEqual(document["authoritySha256"], mod.AUTHORITY_SHA256)
        self.assertIn("whatever happens next", document["rule"])
        self.assertIn("not retried", document["rule"])
        self.assertEqual(
            sorted(document["outputNames"]), sorted(predecessor.output_names())
        )

    def test_two_replicas_write_the_same_bytes(self) -> None:
        # The two replicas have to agree byte for byte on what they produced.
        # A marker carrying a clock would not be compared -- the manifest names
        # the three outputs -- but a marker that differs between two runs of the
        # same attempt is a thing somebody would later have to explain.
        second = pathlib.Path(tempfile.mkdtemp(prefix="boole-consumed-marker."))
        self.addCleanup(shutil.rmtree, second, True)
        with contextlib.redirect_stdout(io.StringIO()):
            left = mod.write_consumed_marker(self.outputs)
            right = mod.write_consumed_marker(second)
        self.assertEqual(left.read_bytes(), right.read_bytes())

    def test_the_console_carries_it_as_well_as_the_disk(self) -> None:
        # The disk it is written to belongs to a runner that is about to be
        # destroyed.  The console is what the host already collects.
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            marker = mod.write_consumed_marker(self.outputs)
        echoed = printed.getvalue()
        self.assertIn("ATTEMPT-CONSUMED", echoed)
        self.assertIn(marker.read_text(encoding="utf-8").strip(), echoed)

    def test_the_production_marks_before_it_writes_any_image_file(self) -> None:
        # The mark is now the act of entering the section rather than a
        # statement inside it, so it cannot be moved down past an image step by
        # anyone editing that section.
        source = inspect.getsource(mod.produce)
        mark = source.index("consumed_attempt(")
        for alias in ("kernel_extract.", "initrd.", "root_disk_execute.", "producer."):
            self.assertLess(mark, source.index(alias), alias)
        self.assertIn(
            "write_consumed_marker", inspect.getsource(mod.consumed_attempt)
        )

    def test_nothing_touches_the_outputs_between_the_directory_and_the_mark(
        self,
    ) -> None:
        # The layout build and the tree extraction sit here, and both write into
        # the scratch.  If either ever wrote into the outputs, the marker would
        # no longer be the first thing in that directory and the boundary would
        # have quietly moved back to where it was.
        source = inspect.getsource(mod.produce)
        window = source[
            source.index("outputs.mkdir(") : source.index("consumed_attempt(")
        ]
        self.assertNotIn("outputs)", window.replace("phase.output_paths(outputs)", ""))
        self.assertIn("_extract_tree", window)

    def test_the_preflight_cannot_reach_the_marker(self) -> None:
        self.assertNotIn(
            "write_consumed_marker", mod._local_call_graph("preflight")
        )
        mod.assert_preflight_creates_no_outputs()

    def test_the_wrapper_refuses_an_outputs_that_already_says_consumed(self) -> None:
        # The cheapest place to refuse a retry: before the tmpfs is mounted and
        # long before anything is assembled. The name is spelled in two
        # languages, so it is compared here rather than trusted twice.
        source = WRAPPER_PATH.read_text(encoding="utf-8")
        self.assertIn(f"$outputs/{mod.CONSUMED_MARKER_NAME}", source)
        self.assertIn("already says the attempt was consumed: no retry", source)

    def test_a_failed_replica_keeps_the_marker(self) -> None:
        # This test used to require that a failed replica keep the marker *and
        # nothing else*, on the reasoning that a half-written image must not be
        # uploadable as evidence of a production.  The first real production
        # showed what that costs: three finished files, content-checked, thrown
        # away with the runner because a later statement raised.  The operator's
        # ruling of 2026-08-28 replaced the rule -- what a failed replica
        # produced is kept, under a name and a document that disown it, which is
        # ``KeptEvidenceSurvivesAFailedReplicaTests`` above.  The marker keeps
        # its own upload either way, so the accounting question never depends on
        # a larger one succeeding.
        # Anchored on the artifact name rather than on the marker's own, which
        # the file also spells in a comment: the step is what is being checked,
        # and the artifact name occurs in exactly one step.
        source = WORKFLOW_PATH.read_text(encoding="utf-8")
        keep = source.index("name: successor-attempt-consumed-")
        step = source.rindex("- name:", 0, keep)
        block = source[step : source.index("retention-days", keep)]
        self.assertIn("if: always()", block)
        self.assertIn("upload-artifact", block)
        self.assertIn(mod.CONSUMED_MARKER_NAME, block)


class OperatorBudgetRulingTests(unittest.TestCase):
    """The verdict the runner was not allowed to reach on its own.

    It is a separate document for the same reason the failure was: the sealed
    authority is what the attempt was judged against, and a record that edited
    it would describe the run instead of committing to it.  So the ruling pins
    both earlier files by digest, re-derived here from the files themselves.
    """

    def setUp(self) -> None:
        self.ruling = json.loads(RULING_PATH.read_text(encoding="utf-8"))
        self.authority = json.loads(SPENT_AUTHORITY_PATH.read_text(encoding="utf-8"))
        self.hard_stop = json.loads(HARD_STOP_PATH.read_text(encoding="utf-8"))

    def _sha256(self, path: pathlib.Path) -> str:
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def test_it_pins_the_two_records_it_rules_on(self) -> None:
        pinned = self.ruling["pinned"]
        self.assertEqual(pinned["authoritySha256"], self._sha256(SPENT_AUTHORITY_PATH))
        self.assertEqual(pinned["hardStopRecordSha256"], self._sha256(HARD_STOP_PATH))
        self.assertEqual(pinned["runId"], self.hard_stop["attempt"]["runId"])

    def test_the_accounting_is_the_one_the_operator_asked_for(self) -> None:
        accounting = self.ruling["accounting"]
        self.assertEqual(accounting["workflowRunsDispatched"], 1)
        self.assertTrue(accounting["emptyOutputDirectoryCreated"])
        self.assertEqual(accounting["imageOutputFilesCreated"], 0)
        self.assertEqual(accounting["productionBudgetConsumed"], 0)
        self.assertEqual(accounting["attemptsRemaining"], 1)

    def test_the_numbers_are_the_failed_run_s_own_numbers(self) -> None:
        budget = self.hard_stop["budget"]
        accounting = self.ruling["accounting"]
        self.assertEqual(accounting["imageOutputFilesCreated"], budget["outputFilesCreated"])
        self.assertEqual(accounting["uploadedArtifacts"], budget["artifactsUploaded"])
        self.assertEqual(
            accounting["emptyOutputDirectoryCreated"], budget["outputDirectoryCreated"]
        )

    def test_it_edits_neither_of_the_records_it_supersedes(self) -> None:
        # The authority still allows one run and still reports none performed,
        # and the failure record still says the verdict was pending when it was.
        self.assertEqual(self.authority["runsAllowed"], 1)
        self.assertEqual(self.authority["runsPerformed"], 0)
        self.assertEqual(
            self.hard_stop["budget"]["spentVerdict"], "OPERATOR-DECISION-PENDING"
        )
        self.assertEqual(
            self.hard_stop["budget"]["twoRulesDisagree"]["sealedAuthorityRule"],
            self.authority["budgetBoundary"]["rule"],
        )

    def test_it_does_not_sit_where_a_production_result_belongs(self) -> None:
        result = REPO_ROOT / self.authority["resultPath"]
        self.assertNotEqual(RULING_PATH, result)
        self.assertFalse(result.exists(), "a production result exists on disk")

    def test_it_names_what_has_to_happen_before_the_next_dispatch(self) -> None:
        required = self.ruling["requiredBeforeReDispatch"]
        self.assertEqual(len(required), 6)
        joined = " ".join(required).lower()
        for phrase in (
            "merge",
            "append-only",
            "atomically",
            "preflight",
            "exactly once",
        ):
            self.assertIn(phrase, joined)

    def test_the_replacement_boundary_is_the_marker(self) -> None:
        replacement = self.ruling["boundaryReplacedWith"]
        self.assertEqual(replacement["markerName"], mod.CONSUMED_MARKER_NAME)
        self.assertIn("before the first image", replacement["writtenWhen"].lower())
        self.assertTrue(replacement["atomic"])
        self.assertIn("whatever happens next", replacement["afterTheMarker"])


class ResultDocumentAssemblyTests(unittest.TestCase):
    """The statement that spent an attempt, given a test that executes it.

    The first attempt built all three files, passed the content check, and then
    raised while assembling the document that reports what it built.  The cause
    was a type: ``manifest_from_directory`` returns a mapping of output name to
    digest, iterating a mapping yields its keys, and the assembly treated each
    key as a row.  Nothing read the field it was building, and no test had ever
    executed the assembly, so both halves of the mistake were invisible.

    The assembly is now a function that takes plain values and returns a
    document, which is a thing a free test can run.  These tests run it.
    """

    OUTPUT_NAMES = predecessor.output_names()

    def setUp(self) -> None:
        self.outputs = pathlib.Path(tempfile.mkdtemp(prefix="boole-result-assembly."))
        self.addCleanup(shutil.rmtree, self.outputs, True)

    def _fake_outputs(self) -> dict:
        """Three tiny files under the real output names, and their digests."""

        digests = {}
        for index, name in enumerate(self.OUTPUT_NAMES):
            raw = f"stand-in for {name}\n".encode("utf-8") * (index + 1)
            (self.outputs / name).write_bytes(raw)
            digests[name] = hashlib.sha256(raw).hexdigest()
        return digests

    def _assemble(self, manifest_entries) -> dict:
        return mod.production_result(
            manifest_entries=manifest_entries,
            output_names=self.OUTPUT_NAMES,
            build_receipt={"stand-in": True},
            builder_internal={"entries": 0},
            kernel_sha256="0" * 64,
            kernel_disposition="stand-in",
            root_disk={"stand-in": True},
            root_disk_evidence={"stand-in": True},
            verify_report={"passed": True, "checks": []},
        )

    def test_what_the_producer_returns_is_what_the_assembly_accepts(self) -> None:
        # The exact seam that failed: the helper's real return value, handed to
        # the assembly, with nothing in between to launder the type.
        digests = self._fake_outputs()
        entries = mod.producer.manifest_from_directory(self.outputs, self.OUTPUT_NAMES)
        document = self._assemble(entries)
        self.assertEqual(
            document["outputManifest"],
            [{"name": name, "sha256": digests[name]} for name in self.OUTPUT_NAMES],
        )

    def test_each_output_is_one_row_in_the_order_the_phase_names_them(self) -> None:
        self._fake_outputs()
        entries = mod.producer.manifest_from_directory(self.outputs, self.OUTPUT_NAMES)
        rows = self._assemble(entries)["outputManifest"]
        self.assertEqual([row["name"] for row in rows], list(self.OUTPUT_NAMES))
        for row in rows:
            self.assertEqual(sorted(row), ["name", "sha256"])
            self.assertRegex(row["sha256"], r"\A[0-9a-f]{64}\Z")

    def test_a_name_the_manifest_does_not_carry_is_refused(self) -> None:
        # A document that quietly omitted an output would read like a production
        # of two files, which is not a thing this phase is allowed to report.
        with self.assertRaises(mod.SuccessorProduceError):
            self._assemble({self.OUTPUT_NAMES[0]: "0" * 64})

    def test_a_manifest_carrying_something_extra_is_refused(self) -> None:
        entries = {name: "0" * 64 for name in self.OUTPUT_NAMES}
        entries["guest-something-else"] = "0" * 64
        with self.assertRaises(mod.SuccessorProduceError):
            self._assemble(entries)

    def test_a_digest_that_is_not_a_digest_is_refused(self) -> None:
        entries = {name: "0" * 64 for name in self.OUTPUT_NAMES}
        entries[self.OUTPUT_NAMES[1]] = "not a digest"
        with self.assertRaises(mod.SuccessorProduceError):
            self._assemble(entries)

    def test_the_document_claims_nothing_the_run_did_not_establish(self) -> None:
        self._fake_outputs()
        entries = mod.producer.manifest_from_directory(self.outputs, self.OUTPUT_NAMES)
        document = self._assemble(entries)
        self.assertFalse(document["bootableClaim"])
        self.assertFalse(document["servingClaim"])
        self.assertFalse(document["activationAllowed"])
        self.assertFalse(document["boundaries"]["guestBootVerified"])
        self.assertFalse(document["boundaries"]["runtimeCompatibilityVerified"])
        self.assertTrue(document["outputsCreated"])
        self.assertEqual(document["authoritySha256"], mod.AUTHORITY_SHA256)
        self.assertEqual(document["release"], mod.RELEASE)

    def test_the_production_assembles_its_result_through_this_function(self) -> None:
        # Otherwise there would be two assemblies, and the tested one would not
        # be the one that runs on the attempt that costs something.
        source = inspect.getsource(mod.produce)
        self.assertIn("production_result(", source)
        self.assertNotIn('"outputManifest"', source)

    def test_the_document_is_canonical_json(self) -> None:
        # It is written with the canonical writer and compared byte for byte
        # between two replicas, so it has to survive the trip.
        self._fake_outputs()
        entries = mod.producer.manifest_from_directory(self.outputs, self.OUTPUT_NAMES)
        document = self._assemble(entries)
        raw = mod.staging.canonical_json(document)
        self.assertEqual(json.loads(raw.decode("utf-8")), document)


class MarkerIsReadableToWhoeverCollectsItTests(unittest.TestCase):
    """The evidence that could not be uploaded off the runner that made it.

    The marker is written by root inside the transient unit, and the step that
    keeps it runs as the ordinary runner account.  A temporary file is created
    at mode 0600 and a rename preserves that, so both replicas refused the
    upload with ``EACCES`` and the only surviving copy was the console echo.
    The echo is worth keeping, but it is a second copy, not a substitute for
    the file the accounting question is about.
    """

    def setUp(self) -> None:
        self.outputs = pathlib.Path(tempfile.mkdtemp(prefix="boole-marker-mode."))
        self.addCleanup(shutil.rmtree, self.outputs, True)

    def test_the_marker_can_be_read_by_an_account_that_did_not_write_it(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            marker = mod.write_consumed_marker(self.outputs)
        mode = stat.S_IMODE(marker.stat().st_mode)
        self.assertTrue(mode & stat.S_IROTH, oct(mode))
        self.assertTrue(mode & stat.S_IRGRP, oct(mode))

    def test_it_is_never_on_disk_under_a_mode_that_cannot_be_read(self) -> None:
        # Made readable before the rename rather than after it, so the name that
        # answers the budget question is readable from the instant it exists.
        observed = {}
        real_replace = mod.os.replace

        def watch(source, destination):
            observed["mode"] = stat.S_IMODE(os.stat(source).st_mode)
            return real_replace(source, destination)

        with mock.patch.object(mod.os, "replace", side_effect=watch):
            with contextlib.redirect_stdout(io.StringIO()):
                mod.write_consumed_marker(self.outputs)
        self.assertTrue(observed["mode"] & stat.S_IROTH, oct(observed["mode"]))

    def test_it_is_not_writable_by_anyone_who_did_not_write_it(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            marker = mod.write_consumed_marker(self.outputs)
        mode = stat.S_IMODE(marker.stat().st_mode)
        self.assertFalse(mode & stat.S_IWOTH, oct(mode))
        self.assertFalse(mode & stat.S_IWGRP, oct(mode))


class FailureAfterTheMarkerKeepsWhatItProducedTests(unittest.TestCase):
    """Files that existed, passed their content check, and were thrown away.

    The steps that keep the outputs ran only on success, so the first attempt
    built three files, verified them, raised one statement later, and was
    destroyed with the runner.  Nothing survives to point at and no digest of a
    produced file was ever recorded.

    Keeping them is not adopting them.  A run that failed after the marker
    produced something that is not a qualified image, and the files are kept
    with a document that says exactly that, under a name a later reader cannot
    mistake for a production.
    """

    OUTPUT_NAMES = predecessor.output_names()

    def setUp(self) -> None:
        self.outputs = pathlib.Path(tempfile.mkdtemp(prefix="boole-kept-on-failure."))
        self.addCleanup(shutil.rmtree, self.outputs, True)

    def _write_fake_outputs(self) -> None:
        for name in self.OUTPUT_NAMES:
            (self.outputs / name).write_bytes(f"stand-in for {name}\n".encode("utf-8"))

    def test_a_failure_after_the_marker_leaves_the_files_where_they_were(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(mod.SuccessorProduceError):
                with mod.consumed_attempt(self.outputs):
                    self._write_fake_outputs()
                    raise mod.SuccessorProduceError("something after the marker")
        for name in self.OUTPUT_NAMES:
            self.assertTrue((self.outputs / name).is_file(), name)

    def test_what_is_kept_says_it_is_not_a_qualified_image(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(mod.SuccessorProduceError):
                with mod.consumed_attempt(self.outputs):
                    self._write_fake_outputs()
                    raise mod.SuccessorProduceError("something after the marker")
        diagnostic = mod.unqualified_marker(self.outputs)
        self.assertTrue(diagnostic.is_file())
        document = json.loads(diagnostic.read_text(encoding="utf-8"))
        self.assertEqual(document["status"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertFalse(document["qualifiedImage"])
        self.assertFalse(document["mayBeBooted"])
        self.assertFalse(document["mayBeAdopted"])
        self.assertIn("something after the marker", document["failure"])
        self.assertEqual(document["attemptId"], mod.authority()["attemptId"])
        self.assertTrue(document["attemptConsumed"])

    def test_the_kept_files_can_be_read_by_the_account_that_collects_them(
        self,
    ) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(mod.SuccessorProduceError):
                with mod.consumed_attempt(self.outputs):
                    self._write_fake_outputs()
                    for name in self.OUTPUT_NAMES:
                        (self.outputs / name).chmod(0o600)
                    raise mod.SuccessorProduceError("something after the marker")
        for path in sorted(self.outputs.iterdir()):
            mode = stat.S_IMODE(path.stat().st_mode)
            self.assertTrue(mode & stat.S_IROTH, f"{path.name} is {oct(mode)}")

    def test_a_run_that_finished_leaves_no_diagnostic_behind(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            with mod.consumed_attempt(self.outputs):
                self._write_fake_outputs()
        self.assertFalse(mod.unqualified_marker(self.outputs).exists())
        self.assertTrue(mod.consumed_marker(self.outputs).is_file())

    def test_a_finished_run_is_still_readable_to_whoever_collects_it(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            with mod.consumed_attempt(self.outputs):
                self._write_fake_outputs()
                for name in self.OUTPUT_NAMES:
                    (self.outputs / name).chmod(0o600)
        for path in sorted(self.outputs.iterdir()):
            self.assertTrue(stat.S_IMODE(path.stat().st_mode) & stat.S_IROTH, path.name)

    def test_the_diagnostic_never_hides_the_failure_that_caused_it(self) -> None:
        # Preserving evidence must not become a way to lose the reason. If the
        # diagnostic itself cannot be written, the original failure is still the
        # one that comes out.
        with mock.patch.object(
            mod, "write_unqualified_diagnostic", side_effect=OSError("full disk")
        ):
            with contextlib.redirect_stdout(io.StringIO()):
                with contextlib.redirect_stderr(io.StringIO()) as complaint:
                    with self.assertRaises(mod.SuccessorProduceError) as raised:
                        with mod.consumed_attempt(self.outputs):
                            raise mod.SuccessorProduceError("the original failure")
        self.assertIn("the original failure", str(raised.exception))
        self.assertIn("full disk", complaint.getvalue())

    def test_the_marker_is_written_before_the_section_it_guards(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()) as printed:
            with self.assertRaises(mod.SuccessorProduceError):
                with mod.consumed_attempt(self.outputs):
                    self.assertTrue(mod.consumed_marker(self.outputs).is_file())
                    raise mod.SuccessorProduceError("after the marker")
        self.assertIn(mod.CONSUMED_MARKER_NAME, printed.getvalue())

    def test_the_production_runs_its_one_shot_section_inside_that_guard(self) -> None:
        source = inspect.getsource(mod.produce)
        self.assertIn("with consumed_attempt(outputs):", source)
        guard = source.index("with consumed_attempt(outputs):")
        for alias in ("kernel_extract.", "initrd.", "root_disk_execute.", "producer."):
            self.assertLess(guard, source.index(alias), alias)

    def test_the_preflight_cannot_reach_the_guard_either(self) -> None:
        reachable = mod._local_call_graph("preflight")
        self.assertNotIn("consumed_attempt", reachable)
        self.assertNotIn("write_unqualified_diagnostic", reachable)
        mod.assert_preflight_creates_no_outputs()


class KeptEvidenceSurvivesAFailedReplicaTests(unittest.TestCase):
    """The other half of the same defect, in the job that runs the phase.

    The module can keep a file on the disk of a runner that is about to be
    destroyed.  Whether anything survives is decided by the steps that upload,
    and those ran only when every step before them had passed.
    """

    def setUp(self) -> None:
        self.source = WORKFLOW_PATH.read_text(encoding="utf-8")

    def _step(self, needle: str) -> str:
        at = self.source.index(needle)
        start = self.source.rindex("      - name:", 0, at)
        try:
            end = self.source.index("\n      - name:", at)
        except ValueError:
            end = len(self.source)
        return self.source[start:end]

    def test_the_marker_is_kept_whatever_happened(self) -> None:
        block = self._step("successor-attempt-consumed-")
        self.assertIn("if: always()", block)
        self.assertIn("if-no-files-found: ignore", block)

    def test_a_failed_replica_keeps_what_it_produced(self) -> None:
        block = self._step("successor-unqualified-diagnostic-")
        self.assertIn("if: failure()", block)
        self.assertIn("boot-outputs", block)
        self.assertIn("if-no-files-found: ignore", block)

    def test_what_a_failed_replica_keeps_is_not_named_like_a_production(
        self,
    ) -> None:
        # Two names, and the difference between them is the whole point: the
        # compare job consumes one of them and a human reading the other cannot
        # believe it is looking at a produced image.
        kept = self._step("successor-unqualified-diagnostic-")
        self.assertNotIn("name: successor-outputs-", kept)
        produced = self._step("name: successor-outputs-")
        self.assertIn("if: success()", produced)

    def test_the_evidence_is_kept_whether_or_not_the_replica_finished(self) -> None:
        block = self._step("successor-manifest-")
        self.assertIn("if: always()", block)
        self.assertIn("if-no-files-found: ignore", block)


class OneShotSectionRehearsedOnFakeFilesTests(unittest.TestCase):
    """The whole spent section, end to end, for nothing.

    Everything between the marker and the result document had never been
    executed by a test.  It could not be: the real section extracts a kernel,
    builds an initrd and writes an ext4 image, which needs root, aarch64, a
    payload store and the one attempt there is.  So the three parts that are not
    about images -- marking, assembling the result, and what is left behind --
    ran for the first time on the attempt that had to work.

    Here they run on three files of a few bytes each.  The section is entered
    the way the production enters it, the manifest is taken with the real
    helper, the document is assembled by the real assembly and written by the
    real writer, and the failing variant goes through the same guard.  Nothing
    in this class needs root, an image tool or a payload, and it costs nothing,
    which is the point: this is the rehearsal the first attempt never had.
    """

    OUTPUT_NAMES = predecessor.output_names()

    def setUp(self) -> None:
        self.outputs = pathlib.Path(tempfile.mkdtemp(prefix="boole-rehearsal."))
        self.addCleanup(shutil.rmtree, self.outputs, True)

    def _produce_fake_files(self) -> dict:
        digests = {}
        for index, name in enumerate(self.OUTPUT_NAMES):
            raw = b"stand-in\n" * (index + 1)
            path = self.outputs / name
            path.write_bytes(raw)
            # As root inside the unit would leave them if the umask were
            # unkind: the collector still has to be able to read them.
            path.chmod(0o600)
            digests[name] = hashlib.sha256(raw).hexdigest()
        return digests

    def _run_the_section(self, *, fail_with=None) -> dict:
        with contextlib.redirect_stdout(io.StringIO()):
            with mod.consumed_attempt(self.outputs):
                digests = self._produce_fake_files()
                if fail_with is not None:
                    raise fail_with
                entries = mod.producer.manifest_from_directory(
                    self.outputs, self.OUTPUT_NAMES
                )
                document = mod.production_result(
                    manifest_entries=entries,
                    output_names=self.OUTPUT_NAMES,
                    build_receipt={"stand-in": True},
                    builder_internal={"entries": 0},
                    kernel_sha256=digests[self.OUTPUT_NAMES[0]],
                    kernel_disposition="stand-in",
                    root_disk={"stand-in": True},
                    root_disk_evidence={"stand-in": True},
                    verify_report={"passed": True, "checks": []},
                )
            mod._write_once(self.outputs / "PRODUCE-RESULT.json", document)
        return digests

    def test_the_section_runs_through_to_a_written_result(self) -> None:
        digests = self._run_the_section()
        written = json.loads(
            (self.outputs / "PRODUCE-RESULT.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            written["outputManifest"],
            [{"name": name, "sha256": digests[name]} for name in self.OUTPUT_NAMES],
        )
        self.assertTrue(written["outputsCreated"])
        self.assertFalse(written["bootableClaim"])

    def test_everything_it_leaves_behind_can_be_collected(self) -> None:
        self._run_the_section()
        left = sorted(path.name for path in self.outputs.iterdir())
        self.assertEqual(
            left,
            sorted(
                [mod.CONSUMED_MARKER_NAME, "PRODUCE-RESULT.json", *self.OUTPUT_NAMES]
            ),
        )
        for path in sorted(self.outputs.iterdir()):
            mode = stat.S_IMODE(path.stat().st_mode)
            self.assertTrue(mode & stat.S_IROTH, f"{path.name} is {oct(mode)}")

    def test_two_rehearsals_of_the_same_inputs_write_the_same_bytes(self) -> None:
        second = pathlib.Path(tempfile.mkdtemp(prefix="boole-rehearsal."))
        self.addCleanup(shutil.rmtree, second, True)
        self._run_the_section()
        first_bytes = (self.outputs / "PRODUCE-RESULT.json").read_bytes()
        self.outputs = second
        self._run_the_section()
        self.assertEqual(first_bytes, (second / "PRODUCE-RESULT.json").read_bytes())

    def test_the_failing_variant_keeps_the_files_and_says_they_are_unqualified(
        self,
    ) -> None:
        with self.assertRaises(mod.SuccessorProduceError):
            self._run_the_section(
                fail_with=mod.SuccessorProduceError("the content check failed")
            )
        for name in self.OUTPUT_NAMES:
            self.assertTrue((self.outputs / name).is_file(), name)
        self.assertFalse((self.outputs / "PRODUCE-RESULT.json").exists())
        document = json.loads(
            mod.unqualified_marker(self.outputs).read_text(encoding="utf-8")
        )
        self.assertEqual(document["status"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertIn("the content check failed", document["failure"])
        self.assertEqual(sorted(document["filesKept"]), sorted(self.OUTPUT_NAMES))
        for path in sorted(self.outputs.iterdir()):
            mode = stat.S_IMODE(path.stat().st_mode)
            self.assertTrue(mode & stat.S_IROTH, f"{path.name} is {oct(mode)}")

    def test_the_failure_the_first_attempt_actually_hit_is_caught_here(self) -> None:
        # The literal defect, replayed through the same seam: a mapping handed
        # to an assembly that expected rows.  It is a refusal now rather than a
        # ValueError from the standard library, and either way this test would
        # have failed before the attempt was dispatched.
        self._produce_fake_files()
        entries = mod.producer.manifest_from_directory(self.outputs, self.OUTPUT_NAMES)
        self.assertIsInstance(entries, dict)
        # The statement as it was written. Iterating a mapping yields its keys,
        # so this is `dict("guest-kernel")`, and it never reached the assembly.
        with self.assertRaises(ValueError):
            [dict(row) for row in entries]
        # What it was trying to build, handed to the assembly that now checks.
        with self.assertRaises(mod.SuccessorProduceError):
            mod.production_result(
                manifest_entries=list(entries),
                output_names=self.OUTPUT_NAMES,
                build_receipt={},
                builder_internal={},
                kernel_sha256="0" * 64,
                kernel_disposition="stand-in",
                root_disk={},
                root_disk_evidence={},
                verify_report={"passed": True, "checks": []},
            )


SECOND_HARD_STOP_PATH = (
    CONTAINMENT
    / "native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json"
)


class ConsumedAttemptHardStopTests(unittest.TestCase):
    """The record of the attempt that was spent, and what it may not touch.

    The first dispatch produced no output file and was ruled unspent.  The
    second wrote the marker, built all three files, passed the content check and
    then died assembling the document that reports what it built.  Two failures,
    two records, and the earlier one is not edited to accommodate the later --
    the digests it and its ruling hash to are re-derived here from the files
    themselves, so a silent revision of either fails this gate.
    """

    def setUp(self) -> None:
        self.record = json.loads(SECOND_HARD_STOP_PATH.read_text(encoding="utf-8"))

    def test_the_earlier_records_are_left_byte_unchanged(self) -> None:
        rows = self.record["appendOnly"]["recordsLeftByteUnchanged"]
        self.assertEqual(len(rows), 4)
        for row in rows:
            path = REPO_ROOT / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            found = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(found, row["sha256"], row["path"])

    def test_the_authority_it_ran_under_is_the_spent_one(self) -> None:
        # This record was the current one when it was written, and the module
        # pointed at the authority it names. The module has since moved to the
        # third authority, and this record has not: it is about an attempt that
        # ran under the second, and saying so is the whole point of it.
        self.assertEqual(
            self.record["authoritySha256"], digest_of(SPENT_AUTHORITY_PATH)
        )
        self.assertEqual(
            self.record["attemptId"], read_json(SPENT_AUTHORITY_PATH)["attemptId"]
        )
        self.assertNotEqual(self.record["authoritySha256"], mod.AUTHORITY_SHA256)
        self.assertNotEqual(self.record["attemptId"], mod.authority()["attemptId"])

    def test_the_accounting_says_the_attempt_is_spent_and_nothing_survived(
        self,
    ) -> None:
        accounting = self.record["accounting"]
        self.assertEqual(accounting["workflowRunsDispatched"], 1)
        self.assertTrue(accounting["consumedMarkerWritten"])
        self.assertEqual(accounting["productionBudgetConsumed"], 1)
        self.assertEqual(accounting["attemptsRemainingUnderThisAuthority"], 0)
        self.assertEqual(accounting["preservedArtifacts"], 0)
        self.assertEqual(accounting["bootAttemptsUsed"], 0)

    def test_it_claims_no_image_and_no_boot(self) -> None:
        boundaries = self.record["boundaries"]
        for flag in (
            "activationAllowed",
            "bootableClaim",
            "guestBootVerified",
            "imageProducedClaim",
            "publicMiningOrBenchmark",
            "runtimeCompatibilityVerified",
            "servingClaim",
        ):
            self.assertFalse(boundaries[flag], flag)

    def test_it_names_the_statement_that_raised(self) -> None:
        cause = self.record["cause"]
        self.assertEqual(
            cause["path"], "scripts/native_shadow_successor_produce_phase_arm64_v2.py"
        )
        self.assertIn("outputManifest", cause["statement"])
        self.assertIn("ValueError", cause["error"])

    def test_the_defect_it_names_is_not_in_the_module_any_more(self) -> None:
        source = mod._module_source(mod._this_module())
        self.assertNotIn(self.record["cause"]["statement"], source)
        self.assertIn("def production_result(", source)

    def test_the_two_related_defects_are_repaired_as_well(self) -> None:
        ids = {row["id"] for row in self.record["relatedDefectsFoundInTheSameRun"]}
        self.assertEqual(
            ids,
            {
                "consumed-marker-cannot-be-uploaded",
                "produced-files-are-discarded-when-a-later-step-fails",
            },
        )
        self.assertIn("COLLECTABLE_FILE_MODE", inspect.getsource(mod.write_consumed_marker))
        self.assertIn(
            "successor-unqualified-diagnostic-",
            WORKFLOW_PATH.read_text(encoding="utf-8"),
        )

    def test_the_operator_ruling_is_recorded_with_its_accounting(self) -> None:
        ruling = self.record["operatorRuling"]
        accounting = ruling["accountingTheOperatorFixed"]
        self.assertEqual(accounting["existingProductionAttempt"], "spent")
        self.assertEqual(accounting["newProductionOpportunitiesGranted"], 1)
        self.assertEqual(accounting["bootOpportunitiesUsed"], 0)
        self.assertIn("not adoptable", accounting["existingImage"])
        self.assertEqual(len(ruling["conditions"]), 9)

    def test_it_does_not_sit_where_a_production_result_belongs(self) -> None:
        result = REPO_ROOT / mod.authority()["resultPath"]
        self.assertNotEqual(SECOND_HARD_STOP_PATH, result)
        self.assertFalse(result.exists(), "a production result exists on disk")

    def test_it_is_canonical_and_says_which_record_it_follows(self) -> None:
        raw = SECOND_HARD_STOP_PATH.read_bytes()
        self.assertEqual(
            raw,
            (json.dumps(self.record, indent=2, sort_keys=True) + "\n").encode("utf-8"),
        )
        supersedes = self.record["supersedes"]
        self.assertTrue(supersedes["leftByteUnchanged"])
        self.assertEqual(
            REPO_ROOT / supersedes["path"],
            CONTAINMENT
            / "native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json",
        )


class ThirdAuthorityTests(unittest.TestCase):
    """The one further attempt the operator granted, written down before it runs.

    Two dispatches ran under the previous authority: the first created an
    empty output directory and was ruled unspent, the second wrote its marker,
    built all three files and then lost them.  One of the two was therefore
    spent.  This authority carries one new attempt and nothing else.  What it may not do is quietly become a better
    version of the one it replaces, so the checks below are mostly about what
    stayed the same.
    """

    def setUp(self) -> None:
        self.document = read_json(AUTHORITY_PATH)
        self.spent = read_json(SPENT_AUTHORITY_PATH)

    def test_the_module_runs_under_this_one(self) -> None:
        self.assertEqual(mod.AUTHORITY_PATH, AUTHORITY_PATH)
        self.assertEqual(mod.AUTHORITY_SHA256, digest_of(AUTHORITY_PATH))

    def test_it_carries_exactly_one_attempt_and_has_not_used_it(self) -> None:
        self.assertEqual(self.document["runsAllowed"], 1)
        self.assertEqual(self.document["runsPerformed"], 0)
        self.assertEqual(
            self.document["attemptId"],
            "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V3-ATTEMPT-1",
        )
        self.assertNotEqual(self.document["attemptId"], self.spent["attemptId"])

    def test_the_summary_counts_the_rows_it_summarises(self) -> None:
        """The totals at the top are counted here from the rows at the bottom.

        The first draft of this document said two attempts were spent while
        listing one unspent and one spent underneath it.  A dispatch and a
        spent attempt are not the same event, and quoting a total next to the
        detail it is meant to total is how the two came apart.  Counting it
        here means they cannot come apart again silently.
        """

        accounting = self.document["attemptAccounting"]
        attempts = self.document["priorAttempts"]
        self.assertEqual(accounting["priorProductionDispatches"], len(attempts))
        self.assertEqual(
            accounting["priorProductionAttemptsSpent"],
            sum(1 for row in attempts if row["spent"]),
        )
        self.assertEqual(
            accounting["priorProductionDispatchesUnspent"],
            sum(1 for row in attempts if not row["spent"]),
        )

    def test_the_operators_corrected_numbers_are_the_ones_written_down(self) -> None:
        accounting = self.document["attemptAccounting"]
        self.assertEqual(accounting["priorProductionDispatches"], 2)
        self.assertEqual(accounting["priorProductionDispatchesUnspent"], 1)
        self.assertEqual(accounting["priorProductionAttemptsSpent"], 1)
        self.assertEqual(accounting["productionAttemptsGrantedHere"], 1)
        self.assertEqual(accounting["bootAttemptsUsed"], 0)
        self.assertEqual(accounting["priorImage"], "created, lost, not adoptable")
        self.assertIn("this attempt is spent too", accounting["spendingRule"])

    def test_no_wording_left_calls_both_dispatches_spent(self) -> None:
        """Prose is checked too, because prose is what a reader believes.

        The numbers can be right while the sentence beside them is wrong, and
        the sentence is the part a person acts on.
        """

        text = AUTHORITY_PATH.read_text(encoding="utf-8")
        self.assertNotIn("two spent attempts", text)
        self.assertNotIn("first two attempts are spent", text)
        self.assertIn("two prior dispatches, one unspent and one spent", text)

    def test_the_correction_is_recorded_rather_than_made_quietly(self) -> None:
        """What was changed, by whose ruling, and that nothing had run yet."""

        correction = self.document["accountingCorrection"]
        self.assertEqual(correction["was"]["priorProductionAttemptsSpent"], 2)
        self.assertEqual(correction["nowIs"]["priorProductionAttemptsSpent"], 1)
        self.assertEqual(correction["nowIs"]["priorProductionDispatches"], 2)
        self.assertEqual(correction["nowIs"]["priorProductionDispatchesUnspent"], 1)
        self.assertFalse(correction["anythingRanUnderTheUncorrectedBytes"])
        self.assertEqual(correction["ruledAt"], "2026-08-28T15:01:05Z")
        self.assertEqual(len(correction["supersededSha256"]), 64)
        self.assertNotEqual(correction["supersededSha256"], digest_of(AUTHORITY_PATH))

    def test_the_four_earlier_records_are_bound_byte_unchanged(self) -> None:
        """Every document this one follows, re-derived from the file on disk.

        A record that says an earlier record is unchanged, and takes its word
        for it, has said nothing.  Each digest below is computed here.
        """

        superseded = self.document["supersedes"]["productionAuthority"]
        self.assertTrue(superseded["leftByteUnchanged"])
        self.assertEqual(superseded["sha256"], digest_of(SPENT_AUTHORITY_PATH))

        attempts = self.document["priorAttempts"]
        self.assertEqual(len(attempts), 2)
        self.assertEqual([row["spent"] for row in attempts], [False, True])
        for row in attempts:
            self.assertTrue(row["leftByteUnchanged"])
            self.assertEqual(
                row["recordSha256"], digest_of(REPO_ROOT / row["recordPath"])
            )
            if "rulingPath" in row:
                self.assertEqual(
                    row["rulingSha256"], digest_of(REPO_ROOT / row["rulingPath"])
                )

    def test_the_inherited_hard_stops_are_carried_word_for_word(self) -> None:
        conditions = self.document["hardStopConditions"]
        self.assertEqual(conditions["inherited"], self.spent["hardStopConditions"])

    def test_the_additions_are_declared_as_additions(self) -> None:
        conditions = self.document["hardStopConditions"]
        added = conditions["declaredAdditions"]
        self.assertTrue(added)
        self.assertFalse(set(added) & set(conditions["inherited"]))
        self.assertIn(
            f"{mod.CONSUMED_MARKER_NAME} is written and the run then fails, "
            "for any reason",
            added,
        )
        self.assertIn(
            f"a produced file is kept without {mod.UNQUALIFIED_MARKER_NAME} "
            "beside it",
            added,
        )

    def test_the_repair_it_required_is_the_one_that_landed(self) -> None:
        repair = self.document["repairRequiredFirst"]
        self.assertEqual(
            {row["id"] for row in repair["defects"]},
            {
                "result-document-assembly",
                "marker-not-readable-by-the-account-that-collects-it",
                "produced-files-discarded-when-a-later-step-fails",
            },
        )
        self.assertRegex(repair["mergeCommit"], r"\A[0-9a-f]{40}\Z")

    def test_it_names_the_fingerprint_without_pinning_it(self) -> None:
        """The pin goes one way, because the module already pins this file.

        The module carries this authority's digest.  If this authority carried
        the module's -- directly, or through a record whose digest it named --
        neither file could be written first.
        """

        fingerprint = self.document["producerFingerprint"]
        self.assertEqual(
            REPO_ROOT / fingerprint["path"], PRODUCER_FINGERPRINT_PATH
        )
        self.assertNotIn("sha256", fingerprint)
        raw = AUTHORITY_PATH.read_text(encoding="utf-8")
        self.assertNotIn(digest_of(PRODUCER_FINGERPRINT_PATH), raw)
        self.assertNotIn(digest_of(pathlib.Path(mod.__file__)), raw)

    def test_the_sealed_preflight_is_kept_with_the_digest_it_recorded(self) -> None:
        standing = self.document["preflightResultStillStands"]
        self.assertTrue(standing["leftByteUnchanged"])
        self.assertEqual(standing["sha256"], digest_of(PREFLIGHT_RESULT_PATH))
        self.assertIn("v2.json", standing["takenUnder"])
        self.assertIn("full preflight", standing["yetANewPreflightRunsAnyway"])

    def test_it_claims_nothing_it_has_not_done(self) -> None:
        for key in (
            "bootableClaim",
            "servingClaim",
            "imageProducedClaim",
            "activationAllowed",
        ):
            self.assertFalse(self.document[key], msg=key)
        self.assertFalse((REPO_ROOT / self.document["resultPath"]).exists())

    def test_the_preserved_records_are_bound_where_the_run_rechecks_them(self) -> None:
        """Preservation is checked on the runner, not only here.

        assert_bound_inputs walks this table before the production assembles
        anything, so a record that moved stops the attempt rather than being
        noticed afterwards.
        """

        bound = {row["path"] for row in self.document["boundInputDigests"]["files"]}
        for path in (
            SPENT_AUTHORITY_PATH,
            HARD_STOP_PATH,
            SECOND_HARD_STOP_PATH,
            RULING_PATH,
        ):
            self.assertIn(str(path.relative_to(REPO_ROOT)), bound)
        self.assertEqual(
            mod.assert_bound_inputs(self.document, REPO_ROOT), len(bound)
        )
        self.assertIn(
            "including the four records of the two prior dispatches",
            " ".join(self.document["preflight"]["passRequires"]),
            msg="the free mode does not check the preservation the paid one does",
        )

    def test_it_is_canonical(self) -> None:
        self.assertEqual(
            AUTHORITY_PATH.read_bytes(),
            (json.dumps(self.document, indent=2, sort_keys=True) + "\n").encode(
                "utf-8"
            ),
        )


class ProducerFingerprintTests(unittest.TestCase):
    """The bytes that will run, sealed before they run.

    Kept out of the authority because the module pins the authority's digest:
    this record points at the authority, and nothing points back at this one.
    """

    def setUp(self) -> None:
        self.record = read_json(PRODUCER_FINGERPRINT_PATH)

    def test_every_digest_is_the_file_it_names(self) -> None:
        rows = self.record["files"]
        self.assertGreaterEqual(len(rows), 5)
        for row in rows:
            self.assertEqual(
                row["sha256"],
                digest_of(REPO_ROOT / row["path"]),
                msg=f"{row['path']} moved after it was sealed",
            )

    def test_it_covers_the_module_the_wrapper_and_the_workflow(self) -> None:
        paths = {row["path"] for row in self.record["files"]}
        self.assertIn(
            str(pathlib.Path(mod.__file__).resolve().relative_to(REPO_ROOT)), paths
        )
        self.assertIn("scripts/native-shadow-successor-produce-arm64.sh", paths)
        self.assertIn(str(WORKFLOW_PATH.relative_to(REPO_ROOT)), paths)
        self.assertIn(
            str(pathlib.Path(__file__).resolve().relative_to(REPO_ROOT)),
            paths,
            msg="the gate that has to pass first is part of the producer",
        )

    def test_it_is_sealed_under_the_authority_it_names(self) -> None:
        self.assertEqual(
            REPO_ROOT / self.record["authorityPath"], AUTHORITY_PATH
        )
        self.assertEqual(self.record["authoritySha256"], digest_of(AUTHORITY_PATH))
        self.assertEqual(
            self.record["attemptId"], read_json(AUTHORITY_PATH)["attemptId"]
        )

    def test_it_names_the_three_defects_it_was_sealed_after(self) -> None:
        self.assertEqual(
            set(self.record["repairedDefects"]),
            {
                "result-document-assembly",
                "marker-not-readable-by-the-account-that-collects-it",
                "produced-files-discarded-when-a-later-step-fails",
            },
        )

    def test_it_claims_nothing(self) -> None:
        self.assertEqual(self.record["status"], "PRODUCER-SEALED-NOT-RUN")
        for key, value in self.record["boundaries"].items():
            self.assertFalse(value, msg=key)

    def test_it_is_canonical(self) -> None:
        self.assertEqual(
            PRODUCER_FINGERPRINT_PATH.read_bytes(),
            (json.dumps(self.record, indent=2, sort_keys=True) + "\n").encode("utf-8"),
        )
