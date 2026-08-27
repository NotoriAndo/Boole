#!/usr/bin/env python3
"""The e2fsprogs actually selected, and the reading that selected it.

The pre-registration record fixed the accept/reject rule while
`debsFetchedSoFar` was still zero.  This record is its successor: the rule has
now been applied to signature-verified official binaries, and one of them was
chosen.  The pre-registration record is not edited to say so -- it stays as the
statement made before the candidates were read, which is the only thing that
makes it worth anything.

Two properties are worth more than the choice itself and are tested here.

The first is that the rule discriminates.  A criterion that every binary passes
has decided nothing, so the record carries a negative control -- an e2fsprogs
from the frozen suite, the same upstream version as the tool that failed -- and
the same code that passed the selected binary has to fail that one.

The second is that the selection changes only the writer.  The 191 packages the
guest is built from are not touched, the image inspector and the read-only
checker stay on their frozen build, and the one auxiliary library that has to
match the writer exactly is the one the package's own `Pre-Depends` says has to
match exactly, not one this record picked.
"""
from __future__ import annotations

import json
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RECORD_PATH = (
    REPO / "native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json"
)
PREREGISTRATION_PATH = (
    REPO
    / "native/containment/native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json"
)
SOURCE_LOCK_PATH = (
    REPO / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)


def document() -> dict:
    return json.loads(RECORD_PATH.read_text(encoding="utf-8"))


def reader():
    from scripts import native_shadow_boot_e2fsprogs_static_read_arm64_v1 as module

    return module


class SelectionRecordTests(unittest.TestCase):
    """What the record is allowed to claim."""

    def test_the_record_is_canonical_json(self) -> None:
        raw = RECORD_PATH.read_bytes()
        rebuilt = (
            json.dumps(json.loads(raw), indent=2, sort_keys=True, ensure_ascii=False) + "\n"
        )
        self.assertEqual(raw, rebuilt.encode("utf-8"))

    def test_the_record_produces_nothing_and_activates_nothing(self) -> None:
        record = document()
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["bootableClaim"])
        boundaries = record["boundaries"]
        self.assertFalse(boundaries["claimsARootDiskIsDeterministic"])
        self.assertFalse(boundaries["dispatchesProduction"])
        self.assertFalse(boundaries["modifiesASealedRecord"])
        self.assertFalse(boundaries["replacesAGuestPackage"])

    def test_the_pre_registration_record_is_carried_unchanged(self) -> None:
        # The successor names its predecessor by digest.  If someone edits the
        # pre-registration record to agree with what was later found, this fails.
        carried = document()["appendOnly"]["predecessor"]
        self.assertEqual(carried["commit"], "d8ef4d2bf5b81dfebc94159ac8cbdc4e07981ec4")
        self.assertEqual(
            carried["path"],
            "native/containment/"
            "native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json",
        )
        self.assertEqual(carried["sha256"], _digest(PREREGISTRATION_PATH))

    def test_the_criterion_is_the_one_that_was_pre_registered(self) -> None:
        pre = json.loads(PREREGISTRATION_PATH.read_text(encoding="utf-8"))
        criterion = document()["criterion"]
        self.assertTrue(criterion["staticOnly"])
        self.assertEqual(criterion["staticOnly"], pre["acceptanceCriteria"]["staticOnly"])
        self.assertFalse(criterion["establishedByRunningTheBinary"])
        self.assertEqual(
            criterion["establishedByRunningTheBinary"],
            pre["acceptanceCriteria"]["establishedByRunningTheBinary"],
        )


class ControlTests(unittest.TestCase):
    """A rule that cannot fail anything has not been applied to anything."""

    def test_both_controls_are_present_and_are_different_binaries(self) -> None:
        controls = document()["controls"]
        negative = controls["negative"]
        positive = controls["positive"]
        self.assertNotEqual(negative["writer"]["sha256"], positive["writer"]["sha256"])
        self.assertNotEqual(negative["library"]["sha256"], positive["library"]["sha256"])
        self.assertEqual(negative["suite"], "noble-updates")
        self.assertEqual(positive["suite"], "plucky")

    def test_the_negative_control_is_the_frozen_upstream_version(self) -> None:
        # Same upstream 1.47.0 as the tool whose failure is sealed, so the
        # control is the defect itself rather than an unrelated old binary.
        negative = document()["controls"]["negative"]
        self.assertTrue(negative["version"].startswith("1.47.0-2.4~exp1ubuntu4"))
        frozen = document()["selection"]["currentlyFrozen"]
        self.assertEqual(frozen["version"], "1.47.0-2.4~exp1ubuntu4")

    def test_the_same_code_fails_the_negative_and_passes_the_positive(self) -> None:
        module = reader()
        controls = document()["controls"]
        negative = module.verdict(
            controls["negative"]["writerMeasurement"],
            controls["negative"]["libraryMeasurement"],
        )
        positive = module.verdict(
            controls["positive"]["writerMeasurement"],
            controls["positive"]["libraryMeasurement"],
        )
        self.assertEqual(negative["verdict"], module.VERDICT_DEFECT)
        self.assertEqual(positive["verdict"], module.VERDICT_FIXED)

    def test_the_record_states_the_verdicts_the_code_produces(self) -> None:
        module = reader()
        for side in ("negative", "positive"):
            control = document()["controls"][side]
            computed = module.verdict(
                control["writerMeasurement"], control["libraryMeasurement"]
            )
            self.assertEqual(control["verdict"], computed["verdict"], side)

    def test_a_missing_library_gate_is_decisive_even_with_no_window(self) -> None:
        # The window that carries the defect is found by a version-specific
        # heuristic, and on the frozen build the compiler inlined it away.  The
        # library gate does not depend on that: a library that never arms the
        # fixed-time flag cannot be fixed whatever the writer looks like, so a
        # missing gate has to decide the verdict on its own.
        module = reader()
        outcome = module.verdict(
            {"window": None},
            {
                "hasSourceDateEpochString": False,
                "hasFakeTimeString": True,
                "flagArmedAt": [],
                "timeCallsNotBehindTheFlag": ["0x0"],
            },
        )
        self.assertEqual(outcome["verdict"], module.VERDICT_DEFECT)

    def test_a_writer_that_copies_the_staged_ctime_fails_even_with_a_good_library(
        self,
    ) -> None:
        module = reader()
        outcome = module.verdict(
            {
                "window": "0x0..0x1",
                "instructions": 2,
                "stagedCtimeLoads": 1,
                "fsNowLoads": 0,
                "fixedTimeFlagLoads": 0,
            },
            {
                "hasSourceDateEpochString": True,
                "hasFakeTimeString": True,
                "flagArmedAt": ["0x0"],
                "timeCallsNotBehindTheFlag": [],
            },
        )
        self.assertEqual(outcome["verdict"], module.VERDICT_DEFECT)


class WriterToolSetTests(unittest.TestCase):
    """Which packages the selection adds, and which it leaves alone."""

    def test_the_writer_set_is_pinned_before_it_is_fetched(self) -> None:
        writer = document()["writerToolSet"]
        self.assertFalse(writer["fetchedIntoPermanentStorage"])
        self.assertTrue(writer["index"]["inReleaseSignatureVerified"])
        self.assertTrue(writer["index"]["packagesIndexDigestVerified"])
        self.assertEqual(
            writer["index"]["verifiedWithKeyringSha256"],
            "80a36b0a6de2f69f49d2df75ef473ccde121e9e190b9ea01d20a4f63778d5c31",
        )
        self.assertEqual(len(writer["packages"]), 2)
        for row in writer["packages"]:
            self.assertRegex(row["sha256"], r"^[0-9a-f]{64}$")
            self.assertGreater(row["sizeBytes"], 0)
            self.assertTrue(row["poolPath"].startswith("pool/main/e/e2fsprogs/"))
            self.assertEqual(row["version"], "1.47.2-1ubuntu1")

    def test_the_exact_version_set_comes_from_the_packages_own_pre_depends(self) -> None:
        writer = document()["writerToolSet"]
        exact = writer["mustMatchTheWriterExactly"]
        self.assertEqual(exact["packages"], ["libext2fs2t64"])
        self.assertEqual(
            exact["evidence"],
            "Pre-Depends: libblkid1 (>= 2.36), libc6 (>= 2.38), "
            "libcom-err2 (>= 1.43.9), libext2fs2t64 (= 1.47.2-1ubuntu1), "
            "libss2 (>= 1.38), libuuid1 (>= 2.16)",
        )
        self.assertTrue(exact["readFromTheCandidateControlFile"])

    def test_every_remaining_dependency_is_satisfied_by_a_frozen_guest_package(
        self,
    ) -> None:
        # The HARD STOP is "the writer needs packages nobody pinned".  It is
        # cleared by showing each remaining Pre-Depends is a floor the frozen
        # guest already clears, with the frozen version read from the lock.
        lock_versions = _source_lock_versions()
        for row in document()["writerToolSet"]["satisfiedByTheFrozenGuest"]:
            self.assertIn(row["package"], lock_versions)
            self.assertEqual(row["frozenVersion"], lock_versions[row["package"]])
            self.assertTrue(row["constraint"].startswith(">="))

    def test_the_writer_needs_no_shared_library_from_outside_those_two_sets(self) -> None:
        writer = document()["writerToolSet"]
        supplied = {row["soname"] for row in writer["suppliedByTheWriterSet"]}
        frozen = {row["soname"] for row in writer["satisfiedByTheFrozenGuest"]}
        self.assertEqual(set(writer["writerNeeded"]), supplied | frozen)

    def test_the_guest_source_lock_is_named_and_unchanged(self) -> None:
        guest = document()["guestPackages"]
        self.assertEqual(guest["count"], 191)
        self.assertFalse(guest["replaced"])
        self.assertFalse(guest["deleted"])
        self.assertEqual(guest["sourceLockSha256"], _digest(SOURCE_LOCK_PATH))

    def test_the_inspector_and_the_checker_stay_on_the_frozen_build(self) -> None:
        independent = document()["independentCheckers"]
        for name in ("debugfs", "e2fsck"):
            row = independent[name]
            self.assertEqual(row["version"], "1.47.0-2.4~exp1ubuntu4")
            self.assertFalse(row["replacedByTheSelection"])
        self.assertTrue(independent["why"])

    def test_the_writer_time_variable_is_the_one_the_selected_build_honours(self) -> None:
        # The pre-registered plan sets E2FSPROGS_FAKE_TIME.  In the selected
        # build that variable sets the time without arming the flag the writer
        # branches on, so carrying it over unchanged would reproduce the failure
        # with a newer binary.  The record has to say so rather than imply it.
        timing = document()["writerTime"]
        self.assertEqual(timing["variableTheSelectedBuildHonours"], "SOURCE_DATE_EPOCH")
        self.assertEqual(timing["variableThePlanCurrentlySets"], "E2FSPROGS_FAKE_TIME")
        self.assertTrue(timing["planMustChange"])
        self.assertFalse(timing["fakeTimeAloneArmsTheFlag"])


class HardStopTests(unittest.TestCase):
    """The conditions that stop this line of work, carried into the record."""

    def test_the_operator_hard_stops_are_carried_verbatim_and_answered(self) -> None:
        conditions = document()["hardStopConditions"]
        self.assertEqual(len(conditions), 6)
        for row in conditions:
            self.assertIn(row["state"], {"CLEARED", "NOT-YET-REACHED"})
            self.assertTrue(row["condition"])
            self.assertTrue(row["basis"])

    def test_the_conditions_a_static_read_cannot_clear_are_left_open(self) -> None:
        by_id = {row["id"]: row for row in document()["hardStopConditions"]}
        for open_until_production in (
            "e2fsck-fails",
            "root-disks-mismatch-again",
            "ext4-contract-changes",
        ):
            self.assertEqual(by_id[open_until_production]["state"], "NOT-YET-REACHED")
        for cleared_by_reading in ("writer-needs-extra-packages", "guest-must-change"):
            self.assertEqual(by_id[cleared_by_reading]["state"], "CLEARED")


def _digest(path: Path) -> str:
    import hashlib

    return hashlib.sha256(path.read_bytes()).hexdigest()


def _source_lock_versions() -> dict[str, str]:
    lock = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
    found: dict[str, str] = {}

    def walk(node: object) -> None:
        if isinstance(node, dict):
            if isinstance(node.get("name"), str) and isinstance(node.get("version"), str):
                found[node["name"]] = node["version"]
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(lock)
    return found


if __name__ == "__main__":
    unittest.main()
