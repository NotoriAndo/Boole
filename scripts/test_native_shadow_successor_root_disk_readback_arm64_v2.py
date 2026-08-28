#!/usr/bin/env python3
"""The successor's root disk is read back against the lock it was built from.

The third production attempt built three files that were exactly what the
successor lock asks for, and then failed, because the stage that reads the image
back judged them against the predecessor's lock.  Nothing was wrong with the
image.  What was wrong was one line of wiring, and these are the tests that hold
the correction in place.

The shape they defend is narrow on purpose.  The predecessor keeps its own
consumer and its own lock, byte for byte.  The successor gets a consumer of its
own that can reach exactly one lock -- the digest-checked second one -- and no
caller, argument, environment variable or image content can talk it into a
different one.  Where the two locks disagree, both directions are refusals
rather than a fallback.
"""

from __future__ import annotations

import copy
import hashlib
import inspect
import json
import pathlib
import platform
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_produce_phase_arm64_v1 as predecessor_phase
from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as predecessor
from scripts import native_shadow_successor_produce_phase_arm64_v2 as phase
from scripts import native_shadow_successor_root_disk_readback_arm64_v2 as readback


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO_ROOT / "native/containment"
SUCCESSOR_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v2.json"
PREDECESSOR_LOCK_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
LAUNCHER_BUILD_RESULT_PATH = (
    CONTAINMENT / "native-shadow-launcher-build-result-arm64-v1.json"
)
WRAPPER_PATH = REPO_ROOT / "scripts/native-shadow-successor-produce-arm64.sh"
PREDECESSOR_WRAPPER_PATH = REPO_ROOT / "scripts/native-shadow-boot-produce-arm64.sh"
WORKFLOW_PATH = (
    REPO_ROOT / ".github/workflows/native-shadow-successor-produce-arm64.yml"
)
RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-production-result-arm64-v3.json"
)
HARD_STOP_PATH = (
    CONTAINMENT
    / "native-shadow-mac3-successor-image-production-hard-stop-arm64-v3.json"
)

# What the earlier attempts left.  They are the evidence this correction was
# derived from, so a change to any of them is a test failure rather than
# something somebody resolves.
SEALED_RECORDS = {
    "native-shadow-mac3-successor-production-authority-arm64-v2.json": (
        "c52e319790e3ca52ba6d635007e541f25e12d6d1497c1abb46ef00b1684b6e58"
    ),
    "native-shadow-mac3-successor-production-authority-arm64-v3.json": (
        "4b9309146ebf05adbd064b2604c5d6693585e30a9a5550eece601d41b9cd282b"
    ),
    "native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json": (
        "98f1bbed5a0cfe4c6f8365552f80d9408027e4e43c262e84e19631e441041115"
    ),
    "native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json": (
        "cb948b6c34aa88247ce90782fe873d6a1d3c743dd8c43143a4c18a67e0bfccaa"
    ),
    "native-shadow-mac3-successor-image-production-hard-stop-arm64-v3.json": (
        "026427bf0000903872fb663e36529b5d5e991d5a4e333b56d66b2c4baff2bc58"
    ),
    "native-shadow-mac3-successor-image-production-budget-ruling-arm64-v1.json": (
        "f1059b8bde4ce4acd13be0058acedec6c1293089a78675ba8612c7c41575a82e"
    ),
    "native-shadow-mac3-successor-image-production-result-arm64-v3.json": (
        "db4af374f7a62ab2ee27546b6118d8182d786b653fcd3abefd327613d6bce066"
    ),
    "native-shadow-mac3-successor-image-production-diagnostic-arm64-v3.json": (
        "de14a1d38d1e70d93af22ce6f4183e4cc3d9b8b00ab53f427c352f46675c4cab"
    ),
}

# The kernel header the verification stage looks at, and nothing else: these
# tests are about which lock the checks are given, not about kernels.
ARM64_KERNEL = b"\x00" * 0x38 + b"ARM\x64" + b"\x00" * 4


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def digest_of(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sealed_launcher() -> str:
    return read_json(LAUNCHER_BUILD_RESULT_PATH)["launcher"]["sha256"]


def tree_from_lock(lock: dict) -> dict:
    """A tree that is exactly what one lock asks for, and nothing else.

    Built rather than mounted.  The checks under test compare a tree against
    expectations, and a synthetic tree is the only way to ask what happens when
    the material comes from one lock and the expectations from the other --
    which is precisely the arrangement that spent the third attempt.
    """

    tree: dict[str, dict] = {}
    for row in lock["trackedFiles"]:
        tree[row["logicalPath"]] = {
            "gid": row["gid"],
            "kind": "file",
            "mode": int(row["mode"], 8),
            "sha256": row["sha256"],
            "uid": row["uid"],
        }
    for row in lock["derivedEntries"]:
        if row["kind"] != "symlink":
            continue
        tree[row["logicalPath"]] = {
            "gid": row["gid"],
            "kind": "symlink",
            "mode": int(row["mode"], 8),
            "target": row["target"],
            "uid": row["uid"],
        }
    tree[image_verify.SYSTEMD_PATH] = {
        "gid": 0,
        "kind": "file",
        "mode": 0o555,
        "sha256": "0" * 64,
        "uid": 0,
    }
    tree[image_verify.LAUNCHER_PATH] = {
        "gid": 0,
        "kind": "file",
        "mode": 0o555,
        "sha256": sealed_launcher(),
        "uid": 0,
    }
    for row in image_verify.mount_points.required_root_directories():
        tree["/" + row["path"]] = {
            "gid": 0,
            "kind": "directory",
            "mode": int(row["mode"], 8),
            "uid": 0,
        }
    return tree


def verify_with(lock: dict, tree: dict) -> dict:
    return image_verify.verify_tree(
        tree=tree,
        expectations=image_verify.expectations_from_lock(lock),
        launcherSha256=sealed_launcher(),
        kernel=ARM64_KERNEL,
    )


def failed_checks(report: dict) -> set:
    return {row["id"] for row in report["checks"] if not row["ok"]}


def step_block(workflow: str, title: str) -> str:
    """One workflow step, from its name to the start of the next one."""

    start = workflow.index(f"- name: {title}")
    return workflow[start : workflow.index("\n      - name: ", start + 1)]


class TheConsumerReachesExactlyOneLockTests(unittest.TestCase):
    """One lock, named here, digest-checked, and not selectable from outside."""

    def test_the_expectations_come_from_the_successor_lock(self) -> None:
        self.assertEqual(
            readback.sealed_expectations(),
            image_verify.expectations_from_lock(read_json(SUCCESSOR_LOCK_PATH)),
        )

    def test_they_are_not_the_predecessors(self) -> None:
        self.assertNotEqual(
            readback.sealed_expectations(),
            image_verify.expectations_from_lock(read_json(PREDECESSOR_LOCK_PATH)),
        )

    def test_the_lock_it_reads_is_the_one_the_producing_phase_read(self) -> None:
        self.assertEqual(readback.SOURCE_LOCK_PATH, phase.SOURCE_LOCK_PATH)
        self.assertEqual(readback.SOURCE_LOCK_SHA256, phase.SOURCE_LOCK_SHA256)
        self.assertEqual(digest_of(SUCCESSOR_LOCK_PATH), readback.SOURCE_LOCK_SHA256)

    def test_the_production_authority_bound_that_same_lock(self) -> None:
        """The lock is not merely agreed between two modules: it is the one the
        pre-registration named, at the digest it named."""

        bound = {
            row["path"]: row["sha256"]
            for row in phase.authority()["boundInputDigests"]["files"]
        }
        relative = str(SUCCESSOR_LOCK_PATH.relative_to(REPO_ROOT))
        self.assertEqual(bound.get(relative), readback.SOURCE_LOCK_SHA256)

    def test_no_caller_can_choose_the_lock(self) -> None:
        self.assertEqual(
            set(inspect.signature(readback.verify).parameters),
            {"outputs", "mountpoint", "result"},
        )
        text = pathlib.Path(readback.__file__).read_text(encoding="utf-8")
        for forbidden in ("--lock", "--source-lock", "--expectations", "os.environ", "getenv"):
            self.assertNotIn(forbidden, text, msg=forbidden)

    def test_it_cannot_name_the_predecessors_lock_at_all(self) -> None:
        readback.assert_no_lock_fallback()
        text = pathlib.Path(readback.__file__).read_text(encoding="utf-8")
        for named in readback.predecessor_names():
            self.assertNotIn(named, text, msg=named)
        self.assertNotIn(phase.HISTORICAL_LOCK_CONSTANT, text)

    def test_a_lock_whose_bytes_moved_is_refused(self) -> None:
        moved = copy.deepcopy(read_json(SUCCESSOR_LOCK_PATH))
        moved["trackedFiles"][0]["sha256"] = "1" * 64
        with self.assertRaises(phase.SuccessorProduceError):
            readback.sealed_expectations(path=self._written(moved))

    def test_a_lock_of_the_wrong_release_is_refused(self) -> None:
        with self.assertRaises(phase.SuccessorProduceError):
            readback.sealed_expectations(path=PREDECESSOR_LOCK_PATH)

    def test_the_lock_is_checked_before_a_device_is_attached(self) -> None:
        source = inspect.getsource(readback.verify)
        self.assertLess(
            source.index("sealed_expectations("),
            source.index("mount_argv("),
            msg="a lock that has moved has to be refused before the mount",
        )

    def _written(self, document: dict) -> pathlib.Path:
        handle = tempfile.NamedTemporaryFile(
            "w", suffix=".json", delete=False, encoding="utf-8"
        )
        handle.write(json.dumps(document))
        handle.close()
        self.addCleanup(pathlib.Path(handle.name).unlink)
        return pathlib.Path(handle.name)


class MaterialAndLockMustBeTheSameGenerationTests(unittest.TestCase):
    """Both crossings are refusals.  Neither falls back to the other."""

    def setUp(self) -> None:
        self.successor = read_json(SUCCESSOR_LOCK_PATH)
        self.predecessor = read_json(PREDECESSOR_LOCK_PATH)

    def test_successor_material_against_its_own_lock_passes(self) -> None:
        report = verify_with(self.successor, tree_from_lock(self.successor))
        self.assertTrue(report["passed"], msg=report["checks"])

    def test_successor_material_against_the_predecessors_lock_is_refused(self) -> None:
        report = verify_with(self.predecessor, tree_from_lock(self.successor))
        self.assertFalse(report["passed"])
        self.assertEqual(
            failed_checks(report), {"modes-owners-and-paths-match-the-lock"}
        )

    def test_that_refusal_is_the_one_that_spent_the_third_attempt(self) -> None:
        """The same paths, for the same reason, from a tree assembled here.

        If this ever stops reproducing, the sealed failure record no longer
        describes anything that can happen, and the correction below is being
        held in place by nothing.
        """

        report = verify_with(self.predecessor, tree_from_lock(self.successor))
        row = next(
            check
            for check in report["checks"]
            if check["id"] == "modes-owners-and-paths-match-the-lock"
        )
        reported = {
            difference["guestPath"]
            for difference in read_json(RESULT_PATH)["failedCheck"]["differences"]
        }
        for path in reported:
            self.assertIn(f"{path}: sha256", row["detail"])
        self.assertEqual(len(reported), row["detail"].count("sha256"))

    def test_predecessor_material_against_the_successors_lock_is_refused(self) -> None:
        report = verify_with(self.successor, tree_from_lock(self.predecessor))
        self.assertFalse(report["passed"])
        self.assertEqual(
            failed_checks(report), {"modes-owners-and-paths-match-the-lock"}
        )

    def test_predecessor_material_against_its_own_lock_still_passes(self) -> None:
        report = verify_with(self.predecessor, tree_from_lock(self.predecessor))
        self.assertTrue(report["passed"], msg=report["checks"])

    def test_the_two_disagree_only_where_this_wave_changed_things(self) -> None:
        successor = {
            row["logicalPath"]: row["sha256"] for row in self.successor["trackedFiles"]
        }
        older = {
            row["logicalPath"]: row["sha256"]
            for row in self.predecessor["trackedFiles"]
        }
        self.assertEqual(
            {path for path in older if successor.get(path) != older[path]},
            {
                "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
                "/usr/lib/tmpfiles.d/boole-native-shadow.conf",
            },
        )
        self.assertEqual(
            set(successor) - set(older),
            {
                "/etc/group",
                "/etc/gshadow",
                "/etc/nsswitch.conf",
                "/etc/passwd",
                "/etc/shadow",
            },
        )


class OneChangedFieldIsEnoughToRefuseTests(unittest.TestCase):
    """Content, permission bits, owner, group, kind, target, presence."""

    def setUp(self) -> None:
        self.lock = read_json(SUCCESSOR_LOCK_PATH)
        self.tracked = "/etc/shadow"
        self.symlink = "/etc/localtime"

    def _refused(self, tree: dict) -> None:
        report = verify_with(self.lock, tree)
        self.assertFalse(report["passed"])
        self.assertIn("modes-owners-and-paths-match-the-lock", failed_checks(report))

    def test_a_changed_content_digest_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        tree[self.tracked]["sha256"] = "2" * 64
        self._refused(tree)

    def test_changed_permission_bits_are_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        tree[self.tracked]["mode"] = 0o444
        self._refused(tree)

    def test_a_changed_owner_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        tree[self.tracked]["uid"] = 1000
        self._refused(tree)

    def test_a_changed_group_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        tree[self.tracked]["gid"] = 1000
        self._refused(tree)

    def test_a_tracked_file_that_became_a_symlink_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        tree[self.tracked] = {
            "gid": 0,
            "kind": "symlink",
            "mode": 0o777,
            "target": "/dev/null",
            "uid": 0,
        }
        self._refused(tree)

    def test_a_changed_symlink_target_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        tree[self.symlink]["target"] = "../usr/share/zoneinfo/Etc/GMT"
        self._refused(tree)

    def test_a_missing_tracked_path_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        del tree[self.tracked]
        self._refused(tree)

    def test_a_path_that_must_not_be_there_is_refused(self) -> None:
        """A produced image legitimately holds thousands of entries the lock says
        nothing about, so "an extra path" cannot be a difference against the
        lock.  The paths that must not exist are refused by the check written
        for them."""

        tree = tree_from_lock(self.lock)
        tree["/usr/lib/boole/replay-node"] = {
            "gid": 0,
            "kind": "file",
            "mode": 0o555,
            "sha256": "3" * 64,
            "uid": 0,
        }
        report = verify_with(self.lock, tree)
        self.assertFalse(report["passed"])
        self.assertEqual(failed_checks(report), {"replay-node-absent"})

    def test_a_missing_mount_point_is_refused(self) -> None:
        tree = tree_from_lock(self.lock)
        del tree["/run"]
        report = verify_with(self.lock, tree)
        self.assertFalse(report["passed"])
        self.assertEqual(failed_checks(report), {"runtime-mount-points-present"})

    def test_the_consumer_requires_every_check_the_verifier_names(self) -> None:
        self.assertEqual(
            sorted(readback.REQUIRED_CHECKS), sorted(image_verify.REQUIRED_CHECKS)
        )
        report = verify_with(self.lock, tree_from_lock(self.lock))
        self.assertEqual(
            sorted(row["id"] for row in report["checks"]),
            sorted(readback.REQUIRED_CHECKS),
        )


class TheWiringIsWhatActuallyRunsTests(unittest.TestCase):
    """A consumer nothing calls would have corrected nothing."""

    def setUp(self) -> None:
        self.wrapper = WRAPPER_PATH.read_text(encoding="utf-8")
        self.workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.consumer = pathlib.Path(readback.__file__).name
        self.older_consumer = pathlib.Path(predecessor.__file__).name

    def test_the_successor_wrapper_calls_the_successor_consumer(self) -> None:
        self.assertIn(f"scripts/{self.consumer}", self.wrapper)

    def test_the_successor_wrapper_no_longer_calls_the_predecessors(self) -> None:
        self.assertNotIn(
            self.older_consumer,
            self.wrapper,
            msg="the successor path may not read its image back through the "
            "consumer that reads the predecessor's lock",
        )

    def test_the_predecessor_wrapper_is_left_alone(self) -> None:
        older = PREDECESSOR_WRAPPER_PATH.read_text(encoding="utf-8")
        self.assertIn(self.older_consumer, older)
        self.assertNotIn(self.consumer, older)

    def test_the_predecessor_consumer_still_reads_the_predecessors_lock(self) -> None:
        self.assertEqual(
            predecessor_phase.BOOT_SOURCE_LOCK_PATH.name, PREDECESSOR_LOCK_PATH.name
        )
        self.assertIn(
            "phase.BOOT_SOURCE_LOCK_PATH",
            pathlib.Path(predecessor.__file__).read_text(encoding="utf-8"),
        )

    def test_the_readback_runs_after_the_phase_and_before_the_manifest(self) -> None:
        order = [
            self.wrapper.index(
                'native_shadow_successor_produce_phase_arm64_v2.py" produce'
            ),
            self.wrapper.index("the produce phase wrote no result document"),
            self.wrapper.index(f"scripts/{self.consumer}"),
            self.wrapper.index('native_shadow_boot_image_produce_arm64_v1.py" manifest'),
        ]
        self.assertEqual(order, sorted(order))

    def test_the_wrapper_writes_the_successors_own_result_name(self) -> None:
        self.assertIn(f"$outputs/{readback.RESULT_NAME}", self.wrapper)
        self.assertNotEqual(readback.RESULT_NAME, "ROOT-DISK-READBACK.json")

    def test_the_workflow_produces_through_that_wrapper(self) -> None:
        block = step_block(
            self.workflow, "Produce the three successor boot files offline"
        )
        self.assertIn("scripts/native-shadow-successor-produce-arm64.sh", block)

    def test_only_a_replica_that_passed_uploads_a_production_artifact(self) -> None:
        block = step_block(self.workflow, "Keep the produced files")
        self.assertIn("if: success()", block)
        self.assertIn("name: successor-outputs-", block)

    def test_a_replica_that_failed_uploads_under_a_name_that_disowns_it(self) -> None:
        block = step_block(
            self.workflow, "Keep what a failed replica left, under a name that disowns it"
        )
        self.assertIn("if: failure()", block)
        self.assertIn("name: successor-unqualified-diagnostic-", block)

    def test_the_evidence_the_comparison_reads_is_written_after_the_readback(
        self,
    ) -> None:
        """The manifest step carries no condition of its own, so it runs only
        when every step before it passed -- and the read-back is one of those
        steps, inside the wrapper the step before it runs."""

        block = step_block(self.workflow, "Write the SHA-256 manifest")
        self.assertNotIn("if:", block)
        self.assertLess(
            self.workflow.index("- name: Produce the three successor boot files"),
            self.workflow.index("- name: Write the SHA-256 manifest"),
        )

    def test_the_comparison_never_reads_a_disowned_replica(self) -> None:
        compare = self.workflow[self.workflow.index("\n  compare:") :]
        self.assertIn("name: successor-manifest-1", compare)
        self.assertIn("name: successor-manifest-2", compare)
        self.assertNotIn("successor-unqualified-diagnostic-", compare)


class TheWholeTailRehearsedOnFakeFilesTests(unittest.TestCase):
    """The stages after the marker, walked end to end for nothing.

    None of them had ever executed on a successor production when they first
    ran, and two of them failed there.  This walks the same order on files that
    cost nothing: marker, produced files, read-back, the branch that decides
    whether what came out is a production or a disowned copy, and what is left
    on disk afterwards.
    """

    def setUp(self) -> None:
        self.scratch = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.scratch, True)
        self.outputs = self.scratch / "outputs"
        self.outputs.mkdir()
        self.lock = read_json(SUCCESSOR_LOCK_PATH)

    def _produced(self) -> None:
        for role, path in readback.output_paths(self.outputs).items():
            path.write_bytes(f"fake {role}".encode("utf-8"))

    def _document(self, tree: dict) -> dict:
        return readback.result_document(
            report=verify_with(self.lock, tree),
            image=readback.output_paths(self.outputs)["root-disk"],
            entries=len(tree),
        )

    def test_the_marker_comes_first_and_the_produced_files_after_it(self) -> None:
        with phase.consumed_attempt(self.outputs):
            self._produced()
        self.assertTrue((self.outputs / phase.CONSUMED_MARKER_NAME).is_file())
        for path in readback.output_paths(self.outputs).values():
            self.assertTrue(path.is_file(), msg=path.name)

    def test_a_passing_readback_writes_a_result_and_disowns_nothing(self) -> None:
        with phase.consumed_attempt(self.outputs):
            self._produced()
        document = self._document(tree_from_lock(self.lock))
        self.assertTrue(document["verification"]["passed"])
        self.assertEqual(document["release"], readback.RELEASE)
        self.assertEqual(document["schema"], readback.SCHEMA)
        self.assertEqual(document["status"], readback.STATUS)
        for flag in ("activationAllowed", "bootableClaim", "guestBootVerified"):
            self.assertFalse(document[flag], msg=flag)
        readback.settle(
            outputs=self.outputs,
            document=document,
            result=self.outputs / readback.RESULT_NAME,
        )
        self.assertTrue((self.outputs / readback.RESULT_NAME).is_file())
        self.assertFalse(phase.unqualified_marker(self.outputs).exists())

    def test_a_failing_readback_disowns_what_it_leaves_and_still_raises(self) -> None:
        with phase.consumed_attempt(self.outputs):
            self._produced()
        tree = tree_from_lock(self.lock)
        tree["/etc/passwd"]["sha256"] = "4" * 64
        document = self._document(tree)
        self.assertFalse(document["verification"]["passed"])

        with self.assertRaises(image_verify.ImageVerifyError):
            readback.settle(
                outputs=self.outputs,
                document=document,
                result=self.outputs / readback.RESULT_NAME,
            )

        written = self.outputs / readback.RESULT_NAME
        self.assertTrue(written.is_file(), msg="the refusal is written down")
        self.assertFalse(read_json(written)["verification"]["passed"])

        disowned = phase.unqualified_marker(self.outputs)
        self.assertTrue(
            disowned.is_file(),
            msg="a run whose image failed the read-back keeps its files only "
            "under the document that says they are not a production",
        )
        kept = read_json(disowned)
        self.assertFalse(kept["qualifiedImage"])
        self.assertFalse(kept["mayBeAdopted"])
        self.assertFalse(kept["mayBeBooted"])
        self.assertIn(
            readback.output_paths(self.outputs)["root-disk"].name, kept["filesKept"]
        )
        self.assertTrue((self.outputs / phase.CONSUMED_MARKER_NAME).is_file())

    def test_the_disowning_document_is_not_the_production_result(self) -> None:
        self.assertNotEqual(phase.UNQUALIFIED_MARKER_NAME, readback.RESULT_NAME)
        self.assertNotEqual(phase.UNQUALIFIED_MARKER_NAME, phase.CONSUMED_MARKER_NAME)


class TheEarlierRecordsAreUntouchedTests(unittest.TestCase):
    """Correcting the cause does not get to rewrite what it was derived from."""

    def test_every_sealed_record_still_hashes_to_what_it_hashed_to(self) -> None:
        for name, sealed in SEALED_RECORDS.items():
            path = CONTAINMENT / name
            self.assertTrue(path.is_file(), msg=name)
            self.assertEqual(digest_of(path), sealed, msg=name)

    def test_the_spent_authorities_still_read_zero_runs(self) -> None:
        for name in SEALED_RECORDS:
            if "production-authority" not in name:
                continue
            self.assertEqual(
                read_json(CONTAINMENT / name)["runsPerformed"], 0, msg=name
            )

    def test_the_diagnosis_still_names_the_line_being_corrected(self) -> None:
        diagnostic = read_json(
            CONTAINMENT
            / "native-shadow-mac3-successor-image-production-diagnostic-arm64-v3.json"
        )
        self.assertIn(
            pathlib.Path(predecessor.__file__).name,
            diagnostic["determination"]["whereItIsWritten"],
        )
        self.assertEqual(diagnostic["determination"]["status"], "ROOT-CAUSE-RESOLVED")
        self.assertFalse(diagnostic["determination"]["builderDefect"])
        self.assertTrue(diagnostic["determination"]["checkerBaselineWrong"])


class TheBudgetAddsUpTests(unittest.TestCase):
    """The totals are the detail rows added up, and nothing else."""

    def setUp(self) -> None:
        self.accounting = read_json(HARD_STOP_PATH)["accounting"]

    def test_the_dispatches_are_three_and_one_of_them_was_free(self) -> None:
        self.assertEqual(
            self.accounting["priorProductionDispatches"]
            + self.accounting["workflowRunsDispatched"],
            3,
        )
        self.assertEqual(self.accounting["priorProductionDispatchesUnspent"], 1)

    def test_the_spent_total_is_the_sum_of_the_spent_rows(self) -> None:
        self.assertEqual(
            self.accounting["priorProductionAttemptsSpent"]
            + self.accounting["thisAttemptSpent"],
            self.accounting["totalProductionAttemptsSpent"],
        )
        self.assertEqual(self.accounting["totalProductionAttemptsSpent"], 2)

    def test_nothing_remains_and_nothing_was_booted(self) -> None:
        self.assertEqual(self.accounting["productionAttemptsRemaining"], 0)
        self.assertEqual(self.accounting["attemptsRemainingUnderThisAuthority"], 0)
        self.assertEqual(self.accounting["bootAttemptsUsed"], 0)
        self.assertEqual(self.accounting["bootAttemptsStarted"], 0)
        self.assertEqual(self.accounting["officialImages"], 0)

    def test_the_two_diagnostic_sets_are_counted_and_disowned(self) -> None:
        images = self.accounting["diagnosticImages"]
        self.assertEqual(images["replicas"], 2)
        self.assertEqual(images["setsPerReplica"], 1)
        self.assertFalse(images["adoptable"])


class TheConsumerClaimsNothingTests(unittest.TestCase):
    def test_its_boundaries_are_all_false(self) -> None:
        for name in ("BOOTABLE_CLAIM", "ACTIVATION_ALLOWED", "GUEST_BOOT_VERIFIED"):
            self.assertFalse(getattr(readback, name), msg=name)

    def test_it_says_what_it_is_in_its_own_words(self) -> None:
        self.assertNotEqual(readback.SCHEMA, predecessor.SCHEMA)
        self.assertNotEqual(readback.RELEASE, predecessor.RELEASE)
        self.assertNotEqual(readback.STATUS, predecessor.STATUS)

    def test_it_reads_the_image_the_same_way_the_predecessor_does(self) -> None:
        """Read-only, no device, no program, and one shared implementation."""

        self.assertEqual(readback.MOUNT_OPTIONS, predecessor.MOUNT_OPTIONS)
        for option in ("ro", "nodev", "noexec", "nosuid"):
            self.assertIn(option, readback.MOUNT_OPTIONS)
        self.assertIs(readback.tree_from_directory, predecessor.tree_from_directory)

    def test_reading_an_image_off_a_machine_without_the_driver_is_refused(self) -> None:
        if platform.system() == "Linux":
            self.skipTest("this refusal is what a machine without the driver gets")
        with self.assertRaises(readback.SuccessorReadbackError):
            readback.verify(outputs=pathlib.Path("/nonexistent"))


if __name__ == "__main__":
    unittest.main()
