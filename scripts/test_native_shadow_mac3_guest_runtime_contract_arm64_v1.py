"""The MAC.3 guest runtime contract is frozen before anything is run against it.

The guest boots. That much is sealed, and it is the whole of what is sealed:
the launcher was started and refused somewhere invisible, and no record claims
otherwise. This module holds the shape of the record that says what "the
launcher serves" would have to mean before any run is allowed to answer.

Two habits are enforced here rather than trusted.

The first is that the conditions are written down before the results exist. A
condition invented after a run is a description of that run, not a test of it,
so the record carries a status that reads as unrun and no verdict of its own.

The second is that a condition nobody can meet is not quietly softened into one
they can. One of the ten -- the launcher running under an unprivileged account
-- is contradicted by the containment already sealed in this repository: the
launcher is a privilege manager that refuses to start unless it is root holding
exactly four capabilities, and it is what drops to the unprivileged accounts on
the way to the work. Rather than reword that condition into something the
existing design happens to satisfy, the record holds it, states the conflict,
and waits. The tests below fail if it is ever marked satisfied, waived, or
reworded while it is still held.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
CONTRACT_PATH = (
    CONTAINMENT / "native-shadow-mac3-guest-runtime-contract-arm64-v1.json"
)
BOOT_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v2.json"
)

# The ten conditions, in the order they were given, keyed by the identifier the
# record has to use for each. Written out here rather than read from the record
# so that a record renaming or reordering its own conditions still fails.
CONDITION_IDS = (
    "systemd-does-not-enter-emergency-and-reaches-its-target",
    "launcher-service-starts-without-being-asked",
    "launcher-executable-matches-the-sealed-digest",
    "launcher-runs-under-an-unprivileged-account",
    "every-write-lands-on-tmpfs",
    "root-disk-stays-read-only",
    "no-network-device-and-no-shared-directory",
    "no-host-wallet-model-key-or-node-secret-in-the-guest",
    "readiness-and-clean-shutdown-are-observed",
    "nothing-of-the-guest-survives-the-shutdown",
)
HELD_CONDITION = "launcher-runs-under-an-unprivileged-account"

# The launcher as the producer authority sealed it. The contract may name it but
# may not restate it differently.
LAUNCHER_SHA256 = "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434"
LAUNCHER_SIZE_BYTES = 2006632
LAUNCHER_GUEST_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"

# The two gaps the sealed boot receipt already named as still absent.
KNOWN_ABSENT = (
    "/etc/passwd",
    "/var/lib/boole/native-shadow/runtime-rootfs",
)

# Paths the launcher writes to, and the one filesystem they are allowed to be on.
RUNTIME_SOCKET = "/run/boole/native-shadow/launcher.sock"
RUNTIME_LOCK = "/run/boole/native-shadow/launcher.lock"


def document() -> dict:
    return json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))


def boot_result() -> dict:
    return json.loads(BOOT_RESULT_PATH.read_text(encoding="utf-8"))


def conditions() -> dict:
    return {row["id"]: row for row in document()["conditions"]}


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, ensure_ascii=False)


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class FrozenBeforeAnythingRanTests(unittest.TestCase):
    """The record opens a run; it does not report one."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(CONTRACT_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_the_status_does_not_read_as_a_result(self) -> None:
        self.assertIn("NOT-RUN", document()["status"])

    def test_the_conditions_are_frozen_before_the_attempt(self) -> None:
        self.assertEqual(document()["frozenBefore"], "any guest runtime run")

    def test_the_record_carries_no_verdict_of_its_own(self) -> None:
        for absent in ("verdict", "console", "whatFailed", "whatWorked", "runsPerformed"):
            self.assertNotIn(absent, document())

    def test_serving_is_not_claimed_by_the_document_that_only_asks_for_it(self) -> None:
        self.assertIs(document()["servingClaim"], False)
        self.assertIs(document()["activationAllowed"], False)


class TenConditionsTests(unittest.TestCase):
    """All ten are present, in order, and each says how it will be checked."""

    def test_every_condition_is_present_and_in_the_order_given(self) -> None:
        self.assertEqual(
            tuple(row["id"] for row in document()["conditions"]), CONDITION_IDS
        )

    def test_no_condition_was_added(self) -> None:
        self.assertEqual(len(document()["conditions"]), len(CONDITION_IDS))

    def test_every_condition_says_how_it_will_be_checked(self) -> None:
        for identifier, row in conditions().items():
            with self.subTest(condition=identifier):
                self.assertIsInstance(row["howItWillBeChecked"], str)
                self.assertGreater(len(row["howItWillBeChecked"]), 40)

    def test_every_condition_is_either_frozen_or_held(self) -> None:
        for identifier, row in conditions().items():
            with self.subTest(condition=identifier):
                self.assertIn(row["status"], ("frozen", "held"))

    def test_no_condition_carries_a_result(self) -> None:
        for identifier, row in conditions().items():
            with self.subTest(condition=identifier):
                for absent in ("met", "verdict", "observed", "outcome"):
                    self.assertNotIn(absent, row)


class TheHeldConditionTests(unittest.TestCase):
    """The one condition the sealed design contradicts is held, not softened."""

    def test_exactly_one_condition_is_held(self) -> None:
        held = [row["id"] for row in document()["conditions"] if row["status"] == "held"]
        self.assertEqual(held, [HELD_CONDITION])

    def test_the_held_condition_is_not_reworded(self) -> None:
        # The words are the operator's. A held condition that had been rewritten
        # into one the current design satisfies would be a silent relaxation, so
        # the record has to keep the original and put its own reading elsewhere.
        row = conditions()[HELD_CONDITION]
        self.assertEqual(row["condition"], "the launcher runs under an unprivileged account")
        self.assertNotIn("root", row["condition"])

    def test_the_conflict_is_stated_rather_than_resolved(self) -> None:
        conflict = document()["heldCondition"]
        self.assertEqual(conflict["id"], HELD_CONDITION)
        self.assertIs(conflict["relaxed"], False)
        self.assertIs(conflict["waived"], False)
        self.assertIs(conflict["satisfied"], False)
        self.assertIn("awaiting", conflict["state"])

    def test_the_conflict_names_what_contradicts_it(self) -> None:
        conflict = document()["heldCondition"]
        cited = {row["path"] for row in conflict["contradictedBy"]}
        self.assertIn("crates/boole-native-shadow-launcher/src/privilege.rs", cited)
        self.assertIn("scripts/native-shadow-launcher-privilege-gate.sh", cited)

    def test_the_cited_files_are_bound_by_digest(self) -> None:
        # Citing a file by name only says where to look. Binding it by digest is
        # what makes the citation fail when the thing it describes changes.
        for row in document()["heldCondition"]["contradictedBy"]:
            with self.subTest(path=row["path"]):
                path = REPO / row["path"]
                self.assertTrue(path.is_file(), path)
                self.assertEqual(sha256(path), row["sha256"])

    def test_the_reading_the_operator_was_offered_is_recorded_separately(self) -> None:
        # Recommending a rewording is allowed. Applying it before the operator
        # answers is not, so it lives beside the condition and never inside it.
        offered = document()["heldCondition"]["readingOffered"]
        self.assertIn("four", offered)
        self.assertIs(document()["heldCondition"]["readingApplied"], False)


class SurveyTests(unittest.TestCase):
    """The read-only survey the contract was built from is written down."""

    def test_every_surveyed_area_the_operator_named_is_covered(self) -> None:
        surveyed = {row["area"] for row in document()["survey"]}
        self.assertEqual(
            surveyed,
            {
                "account-database",
                "launcher-binary-and-digest",
                "service-and-enablement-wiring",
                "runtime-directory",
                "socket-path",
                "tmpfiles-and-sysusers-configuration",
                "writes-needed-on-a-read-only-root",
            },
        )

    def test_every_surveyed_area_says_how_it_was_established(self) -> None:
        for row in document()["survey"]:
            with self.subTest(area=row["area"]):
                self.assertIsInstance(row["how"], str)
                self.assertGreater(len(row["how"]), 30)
                self.assertIn(row["readOnly"], (True,))

    def test_the_survey_is_bound_to_the_files_it_read(self) -> None:
        for row in document()["survey"]:
            for cited in row["read"]:
                with self.subTest(path=cited["path"]):
                    path = REPO / cited["path"]
                    self.assertTrue(path.is_file(), path)
                    self.assertEqual(sha256(path), cited["sha256"])


class GapsTests(unittest.TestCase):
    """What is missing is named, and agrees with the sealed boot receipt."""

    def test_the_gaps_are_the_ones_the_boot_receipt_already_named(self) -> None:
        named = {row["path"] for row in document()["gaps"]}
        for path in KNOWN_ABSENT:
            self.assertIn(path, named)

    def test_each_gap_agrees_with_the_boot_receipt_rather_than_restating_it(self) -> None:
        sealed = {row["path"]: row for row in boot_result()["knownAbsentAndStillAbsent"]}
        for row in document()["gaps"]:
            if row["path"] in sealed:
                with self.subTest(path=row["path"]):
                    self.assertEqual(
                        row["consequence"], sealed[row["path"]]["consequence"]
                    )

    def test_the_boot_receipt_is_bound_by_digest(self) -> None:
        bound = {row["path"]: row for row in document()["appendOnly"]["recordsLeftByteUnchanged"]}
        row = bound["native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json"]
        self.assertEqual(sha256(BOOT_RESULT_PATH), row["sha256"])

    def test_the_launcher_is_named_as_the_producer_authority_sealed_it(self) -> None:
        launcher = document()["launcher"]
        self.assertEqual(launcher["sha256"], LAUNCHER_SHA256)
        self.assertEqual(launcher["sizeBytes"], LAUNCHER_SIZE_BYTES)
        self.assertEqual(launcher["guestLogicalPath"], LAUNCHER_GUEST_PATH)


class WritablePathTests(unittest.TestCase):
    """Where the guest writes, and why that is allowed to be nowhere else."""

    def test_the_runtime_socket_and_lock_are_named(self) -> None:
        named = {row["path"] for row in document()["writablePaths"]}
        self.assertIn(RUNTIME_SOCKET, named)
        self.assertIn(RUNTIME_LOCK, named)

    def test_every_writable_path_is_on_a_memory_filesystem(self) -> None:
        for row in document()["writablePaths"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["filesystem"], "tmpfs")

    def test_the_paths_that_are_not_yet_on_one_are_named_as_such(self) -> None:
        # /var/lib/boole is on the read-only root today. Saying so is what keeps
        # the fifth condition checkable instead of aspirational.
        unresolved = {row["path"] for row in document()["writesNotYetSeparated"]}
        self.assertIn("/var/lib/boole/native-shadow/node-state", unresolved)


class NotOverclaimedTests(unittest.TestCase):
    """The contract opens work; it does not finish any."""

    def test_it_does_not_unlock_the_things_downstream_of_it(self) -> None:
        for absent in ("publicMiningOrBenchmark", "rewardOrActivation", "mac4NodeBinding"):
            self.assertIs(document()["doesNotUnlock"][absent], False)

    def test_the_boot_that_did_happen_is_not_restated_as_serving(self) -> None:
        self.assertIs(document()["boundaries"]["closedLocalGuestBootVerified"], True)
        self.assertIs(document()["boundaries"]["launcherServing"], False)
        self.assertIs(document()["boundaries"]["cleanMacEvidence"], False)

    def test_a_new_image_is_named_as_required_and_not_as_produced(self) -> None:
        production = document()["newImageProduction"]
        self.assertIs(production["required"], True)
        self.assertIs(production["performed"], False)
        self.assertEqual(production["runsAllowed"], 1)


class PredecessorUntouchedTests(unittest.TestCase):
    """Nothing sealed is edited to make room for this."""

    def test_every_bound_record_is_still_byte_identical(self) -> None:
        rows = document()["appendOnly"]["recordsLeftByteUnchanged"]
        self.assertGreaterEqual(len(rows), 3)
        for row in rows:
            with self.subTest(path=row["path"]):
                path = REPO / row["path"]
                self.assertTrue(path.is_file(), path)
                self.assertEqual(sha256(path), row["sha256"])
                self.assertEqual(path.stat().st_size, row["sizeBytes"])


if __name__ == "__main__":  # pragma: no cover
    unittest.main()
