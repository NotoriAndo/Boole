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

The second is the budget boundary.  A refusal raised before the output directory
exists has cost nothing; one raised after an output file exists has cost the only
attempt there is.  ``BudgetBoundaryTests`` checks that the preflight cannot reach
the far side of that line -- not because it promises to be careful, but because it
never calls anything that would create the directory.

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


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO_ROOT / "native/containment"
AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v2.json"
)
PREDECESSOR_LOCK_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
SUCCESSOR_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v2.json"
MEASUREMENT_PATH = (
    CONTAINMENT / "native-shadow-boot-staging-tree-measurement-arm64-v1.json"
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
    ASSEMBLY = "native_shadow_successor_produce_phase_arm64_v2.py preflight"

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
    """Unspent before an output file exists; spent after."""

    def test_the_boundary_is_the_authoritys_own_words(self) -> None:
        rule = mod.authority()["budgetBoundary"]["rule"]
        self.assertIn("before the production output directory exists", rule)
        self.assertIn("the attempt is consumed whatever happens next", rule)

    def test_a_refusal_before_any_output_exists_is_unspent(self) -> None:
        self.assertFalse(mod.attempt_consumed(outputs_created=False))

    def test_a_failure_after_an_output_exists_is_spent(self) -> None:
        self.assertTrue(mod.attempt_consumed(outputs_created=True))

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
            with mock.patch.object(mod, "produce", refuse):
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
            "native-shadow-mac3-successor-image-production-result-arm64-v2.json",
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


if __name__ == "__main__":
    unittest.main()
