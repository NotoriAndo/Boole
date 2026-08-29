"""The third boot attempt's exam, frozen before anyone is allowed to sit it.

Two attempts have been made against two earlier images.  The first failed, the
second passed, and both records are sealed.  This is a third attempt against a
third image -- the one the spent production budget produced -- and it carries
its own single allowance rather than reusing either of the two already gone.

Three things make this record different from its predecessors and each has a
class below.  The image no longer lives only in a CI artifact that expires: it
sits in a permanent archive, so the exam names archive paths, sizes and digests
and requires them recomputed immediately before the machine is configured --
read-only modes stop an accident, not an owner, and the record says so rather
than leaning on the modes.  The three gaps the second attempt recorded as open
are closed, so the launcher's own prerequisites become graded conditions
instead of known refusals.  And the record does not authorise the boot it
describes: the criteria are sealed first and reviewed, and the run is approved
separately, which is the only order in which the review can fail.

Every condition here is either carried from a record that froze it earlier --
byte for byte, judged by the same words -- or marked as new with a reason.  A
condition with no provenance is how an exam gets easier without anyone deciding
to make it easier, so this module refuses one.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"

QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
)
FIRST_QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json"
)
SECOND_QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json"
)
SECOND_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v2.json"
)
CONTRACT_PATH = CONTAINMENT / "native-shadow-mac3-guest-runtime-contract-arm64-v1.json"
CORRECTION_PATH = CONTAINMENT / "native-shadow-mac3-condition-4-correction-arm64-v1.json"
PRESERVATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-preservation-arm64-v4.json"
)
PRODUCTION_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-production-result-arm64-v4.json"
)
PRODUCTION_AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v4.json"
)
GREEN_PATH = CONTAINMENT / "native-shadow-boot-root-disk-determinism-green-arm64-v1.json"

IMAGE_NAMES = ("guest-initrd", "guest-kernel", "guest-root-disk")

# The six conditions the first attempt was judged by, and the frozen conditions
# of the guest runtime contract.  Named here rather than derived, so a source
# record edited into agreement with this one still fails.
FIRST_ATTEMPT_CONDITION_IDS = (
    "console-transcript-captured-and-hashed",
    "closed-local-configuration",
    "guest-systemd-is-pid-1",
    "kernel-reaches-its-root-filesystem",
    "loads-the-converged-image",
    "sealed-image-unchanged-after-the-run",
)
CONTRACT_CONDITION_IDS = (
    "every-write-lands-on-tmpfs",
    "launcher-executable-matches-the-sealed-digest",
    "launcher-service-starts-without-being-asked",
    "no-host-wallet-model-key-or-node-secret-in-the-guest",
    "no-network-device-and-no-shared-directory",
    "nothing-of-the-guest-survives-the-shutdown",
    "readiness-and-clean-shutdown-are-observed",
    "root-disk-stays-read-only",
    "systemd-does-not-enter-emergency-and-reaches-its-target",
)
HELD_CONDITION_ID = "launcher-runs-under-an-unprivileged-account"
CORRECTED_CONDITION_ID = "launcher-supervises-as-root-and-submissions-run-unprivileged"
NEW_CONDITION_IDS = (
    "archive-digests-recomputed-immediately-before-boot",
    "exactly-one-boot-of-this-image",
    "launcher-prerequisites-verify-inside-the-guest",
    "no-failed-unit-and-no-freeze-in-the-transcript",
    "nothing-beyond-the-closed-local-boot-is-attempted",
)


def read(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def document() -> dict:
    return read(QUALIFICATION_PATH)


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, ensure_ascii=False)


def conditions_by_id() -> dict:
    return {row["id"]: row for row in document()["passConditions"]}


class PreFreezeTests(unittest.TestCase):
    """Written before the run, and saying nothing a run would have taught it."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(QUALIFICATION_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_exactly_one_attempt_is_allowed_and_none_has_been_spent(self) -> None:
        record = document()
        self.assertEqual(record["runsAllowed"], 1)
        self.assertEqual(record["runsPerformed"], 0)

    def test_the_status_does_not_read_as_a_result(self) -> None:
        self.assertIn("NOT-RUN", document()["status"])

    def test_the_conditions_are_frozen_before_the_attempt(self) -> None:
        self.assertEqual(document()["frozenBefore"], "any qualification run")

    def test_the_receipt_it_names_does_not_exist_yet(self) -> None:
        # The count is read off the filesystem rather than taken on trust from
        # this record: while the receipt is absent, nothing has been spent.
        receipt = REPO / document()["resultPath"]
        self.assertFalse(receipt.exists(), receipt)

    def test_it_writes_to_a_receipt_neither_earlier_attempt_used(self) -> None:
        taken = {
            "native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json",
            SECOND_RESULT_PATH.relative_to(REPO).as_posix(),
        }
        self.assertNotIn(document()["resultPath"], taken)

    def test_the_record_carries_no_verdict_of_its_own(self) -> None:
        for absent in ("verdict", "console", "whatFailed", "whatWorked", "cleanup"):
            self.assertNotIn(absent, document())

    def test_it_has_an_attempt_identifier_of_its_own(self) -> None:
        """Reusing an earlier attempt's name would reopen an exam already sat.

        The first attempt predates the scheme and carries no identifier at all,
        in its qualification or in its receipt.  It is read here as the absent
        value it is rather than given a name after the fact.
        """

        record = document()
        attempt = record["attemptId"]
        self.assertTrue(attempt.strip())
        earlier = {row["attemptId"] for row in record["earlierAttempts"]}
        for path in (FIRST_QUALIFICATION_PATH, SECOND_QUALIFICATION_PATH):
            earlier.add(read(path).get("attemptId"))
        self.assertNotIn(attempt, earlier)

    def test_it_is_not_an_earlier_records_name_reused(self) -> None:
        record = document()
        for earlier in (FIRST_QUALIFICATION_PATH, SECOND_QUALIFICATION_PATH):
            other = read(earlier)
            for field in ("record", "release", "schema"):
                self.assertNotEqual(record[field], other[field], field)


class BootIsNotAuthorisedHereTests(unittest.TestCase):
    """Criteria first, review, then approval.  Not all three at once.

    A document that set the exam and also opened the run would make the review
    it exists for impossible to fail: by the time anyone disagreed with a
    condition, the run it governs would already be permitted.
    """

    def authorisation(self) -> dict:
        return document()["bootAuthorisation"]

    def test_this_record_does_not_authorise_the_boot(self) -> None:
        self.assertFalse(self.authorisation()["grantedByThisRecord"])

    def test_it_says_what_has_to_happen_before_a_boot(self) -> None:
        authorisation = self.authorisation()
        self.assertTrue(authorisation["requiredBefore"].strip())
        self.assertTrue(authorisation["whoApproves"].strip())
        self.assertTrue(authorisation["why"].strip())

    def test_the_allowance_it_opens_is_at_most_one_run(self) -> None:
        # runsAllowed is a ceiling on a separately approved run, not a licence.
        self.assertEqual(document()["runsAllowed"], 1)
        self.assertIn("one", self.authorisation()["requiredBefore"].lower())


class SubjectTests(unittest.TestCase):
    """The image, named where it now permanently lives."""

    def subject(self) -> dict:
        return document()["subject"]

    def preservation(self) -> dict:
        return read(PRESERVATION_PATH)

    def test_the_archive_root_is_the_one_the_preservation_record_named(self) -> None:
        self.assertEqual(
            self.subject()["archiveRoot"], self.preservation()["archive"]["root"]
        )

    def test_the_preservation_record_is_bound_at_its_live_digest(self) -> None:
        row = self.subject()["preservationRecord"]
        self.assertEqual(REPO / row["path"], PRESERVATION_PATH)
        self.assertEqual(row["sha256"], digest(PRESERVATION_PATH))

    def test_the_three_images_carry_a_path_a_size_and_a_digest(self) -> None:
        images = self.subject()["images"]
        self.assertEqual(sorted(row["name"] for row in images), list(IMAGE_NAMES))
        for row in images:
            self.assertTrue(row["archivePath"].strip(), row["name"])
            self.assertGreater(row["bytes"], 0, row["name"])
            self.assertEqual(len(row["sha256"]), 64, row["name"])

    def test_the_digests_are_the_ones_the_production_run_recorded(self) -> None:
        produced = read(PRODUCTION_RESULT_PATH)["outputs"]
        for row in self.subject()["images"]:
            self.assertEqual(row["sha256"], produced[row["name"]], row["name"])

    def test_the_sizes_and_digests_agree_with_the_preservation_record(self) -> None:
        preserved = {row["name"]: row for row in self.preservation()["images"]}
        for row in self.subject()["images"]:
            self.assertEqual(row["bytes"], preserved[row["name"]]["bytes"], row["name"])
            self.assertEqual(
                row["sha256"], preserved[row["name"]]["sha256"], row["name"]
            )

    def test_every_named_path_is_a_file_the_preservation_record_preserved(self) -> None:
        # A path outside that list is a path nothing was checked or locked at.
        preserved = {row["path"] for row in self.preservation()["preservedFiles"]}
        for row in self.subject()["images"]:
            self.assertIn(row["archivePath"], preserved, row["name"])

    def test_the_second_replica_is_named_and_is_not_a_silent_fallback(self) -> None:
        second = self.subject()["secondReplica"]
        self.assertTrue(second["exists"])
        self.assertFalse(second["usedAsFallbackOnDigestMismatch"])
        self.assertTrue(second["why"].strip())
        preserved = {row["path"] for row in self.preservation()["preservedFiles"]}
        for path in second["paths"].values():
            self.assertIn(path, preserved, path)

    def test_the_digests_are_re_checked_against_the_files_at_boot_time(self) -> None:
        self.assertTrue(self.subject()["verifiedImmediatelyBeforeBoot"])

    def test_the_before_digest_for_the_unchanged_check_is_this_record(self) -> None:
        # One carried condition compares the image afterwards against a frozen
        # digest.  Which document holds it has to be settled before the run.
        source = self.subject()["digestSourceForTheUnchangedCheck"]
        self.assertEqual(source["path"], QUALIFICATION_PATH.relative_to(REPO).as_posix())
        self.assertTrue(source["field"].strip())
        self.assertTrue(source["why"].strip())

    def test_the_initrd_is_named_and_its_use_is_stated(self) -> None:
        initrd = next(
            row for row in self.subject()["images"] if row["name"] == "guest-initrd"
        )
        self.assertIn("used", initrd)
        if not initrd["used"]:
            self.assertTrue(initrd["whyUnused"].strip())


class ReadOnlyIsNotSecurityTests(unittest.TestCase):
    """0444 stops a slip, not an owner, and the record says which."""

    def caveat(self) -> dict:
        return document()["readOnlyModeIsNotASecurityControl"]

    def test_the_record_states_the_limit_of_the_mode(self) -> None:
        caveat = self.caveat()
        self.assertTrue(caveat["stopsAccidentalOverwrite"])
        self.assertFalse(caveat["stopsTheOwnerChangingIt"])
        self.assertTrue(caveat["why"].strip())

    def test_what_follows_from_it_is_a_recomputation_not_a_reassurance(self) -> None:
        self.assertIn(
            "archive-digests-recomputed-immediately-before-boot",
            self.caveat()["whatFollows"],
        )

    def test_the_recomputation_covers_the_manifest_and_all_three_images(self) -> None:
        recompute = document()["recomputeImmediatelyBeforeBoot"]
        self.assertEqual(sorted(recompute["images"]), list(IMAGE_NAMES))
        self.assertTrue(recompute["preservationManifestAtTheArchive"])
        self.assertTrue(recompute["preservationRecordInTheRepository"])
        self.assertEqual(recompute["onMismatch"], "abort")


class PassConditionShapeTests(unittest.TestCase):
    """Every condition is judged, distinct, unmarked and accounted for."""

    def test_every_condition_carries_an_id_and_a_way_to_judge_it(self) -> None:
        for condition in document()["passConditions"]:
            self.assertTrue(condition["id"].strip())
            self.assertTrue(condition["condition"].strip())
            self.assertTrue(condition["judgedBy"].strip())

    def test_the_ids_are_distinct(self) -> None:
        ids = [row["id"] for row in document()["passConditions"]]
        self.assertEqual(len(ids), len(set(ids)))

    def test_no_condition_carries_a_verdict_yet(self) -> None:
        for condition in document()["passConditions"]:
            self.assertNotIn("verdict", condition)

    def test_every_condition_is_either_carried_or_declared_new(self) -> None:
        """A condition with no provenance is how an exam quietly gets easier."""

        for condition in document()["passConditions"]:
            carried = "carriedFrom" in condition
            fresh = condition.get("newInThisRecord", False)
            self.assertNotEqual(carried, fresh, condition["id"])

    def test_every_new_condition_says_why_it_is_new(self) -> None:
        for condition in document()["passConditions"]:
            if condition.get("newInThisRecord"):
                self.assertTrue(condition["why"].strip(), condition["id"])

    def test_the_new_conditions_are_the_ones_expected(self) -> None:
        fresh = sorted(
            row["id"]
            for row in document()["passConditions"]
            if row.get("newInThisRecord")
        )
        self.assertEqual(fresh, list(NEW_CONDITION_IDS))


class CarriedConditionTests(unittest.TestCase):
    """Carried means byte for byte, from a record bound at its live digest."""

    def carried(self) -> list:
        return [
            row for row in document()["passConditions"] if "carriedFrom" in row
        ]

    def source_entry(self, condition: dict) -> dict:
        origin = condition["carriedFrom"]
        source = read(REPO / origin["path"])[origin["field"]]
        if isinstance(source, dict):
            return source
        return next(row for row in source if row["id"] == condition["id"])

    def test_each_source_record_is_on_disk_at_the_digest_named(self) -> None:
        for condition in self.carried():
            origin = condition["carriedFrom"]
            path = REPO / origin["path"]
            self.assertTrue(path.is_file(), origin["path"])
            self.assertEqual(digest(path), origin["sha256"], origin["path"])

    def test_each_carried_condition_reads_as_its_source_does(self) -> None:
        for condition in self.carried():
            entry = self.source_entry(condition)
            self.assertEqual(entry["id"], condition["id"])
            self.assertEqual(entry["condition"], condition["condition"], condition["id"])

    def test_each_carried_condition_is_judged_by_the_same_words(self) -> None:
        # Rewording how a condition is judged is how it stops being the same
        # condition while still looking like it.
        for condition in self.carried():
            entry = self.source_entry(condition)
            key = condition["carriedFrom"]["judgedByKey"]
            self.assertEqual(entry[key], condition["judgedBy"], condition["id"])

    def test_the_first_attempts_six_conditions_are_all_here(self) -> None:
        present = set(conditions_by_id())
        for identifier in FIRST_ATTEMPT_CONDITION_IDS:
            self.assertIn(identifier, present, identifier)

    def test_they_are_carried_from_the_first_qualification_itself(self) -> None:
        wanted = FIRST_QUALIFICATION_PATH.relative_to(REPO).as_posix()
        for identifier in FIRST_ATTEMPT_CONDITION_IDS:
            self.assertEqual(
                conditions_by_id()[identifier]["carriedFrom"]["path"], wanted, identifier
            )

    def test_every_frozen_condition_of_the_runtime_contract_is_here(self) -> None:
        present = set(conditions_by_id())
        for identifier in CONTRACT_CONDITION_IDS:
            self.assertIn(identifier, present, identifier)

    def test_the_contract_has_no_frozen_condition_this_record_dropped(self) -> None:
        frozen = {
            row["id"]
            for row in read(CONTRACT_PATH)["conditions"]
            if row["status"] == "frozen"
        }
        self.assertEqual(frozen, set(CONTRACT_CONDITION_IDS))
        self.assertTrue(frozen.issubset(set(conditions_by_id())))

    def test_the_held_condition_appears_only_in_its_corrected_form(self) -> None:
        present = set(conditions_by_id())
        self.assertNotIn(HELD_CONDITION_ID, present)
        self.assertIn(CORRECTED_CONDITION_ID, present)
        correction = read(CORRECTION_PATH)["correctedCondition"]
        self.assertEqual(correction["replacesConditionId"], HELD_CONDITION_ID)

    def test_the_corrected_condition_is_carried_from_the_correction(self) -> None:
        origin = conditions_by_id()[CORRECTED_CONDITION_ID]["carriedFrom"]
        self.assertEqual(origin["path"], CORRECTION_PATH.relative_to(REPO).as_posix())
        self.assertEqual(origin["field"], "correctedCondition")

    def test_the_exam_is_a_superset_of_both_earlier_ones(self) -> None:
        present = set(conditions_by_id())
        for path in (FIRST_QUALIFICATION_PATH, SECOND_QUALIFICATION_PATH):
            for row in read(path)["passConditions"]:
                self.assertIn(row["id"], present, row["id"])
        self.assertGreater(len(present), len(read(SECOND_QUALIFICATION_PATH)["passConditions"]))


class GapsAreClosedTests(unittest.TestCase):
    """What the second attempt recorded as absent is now graded, not excused."""

    def gaps(self) -> list:
        return document()["gapsOpenAtTheSecondAttempt"]

    def test_the_three_gaps_are_listed_and_marked_closed(self) -> None:
        self.assertEqual(len(self.gaps()), 3)
        for gap in self.gaps():
            self.assertTrue(gap["what"].strip())
            self.assertTrue(gap["path"].strip())
            self.assertFalse(gap["stillAbsent"], gap["path"])
            self.assertTrue(gap["closedBy"].strip(), gap["path"])

    def test_each_closure_rests_on_a_record_bound_at_its_digest(self) -> None:
        for gap in self.gaps():
            evidence = gap["evidence"]
            path = REPO / evidence["path"]
            self.assertTrue(path.is_file(), evidence["path"])
            self.assertEqual(digest(path), evidence["sha256"], evidence["path"])
            self.assertTrue(evidence["check"].strip(), evidence["path"])

    def test_the_gaps_the_contract_named_are_the_gaps_answered_here(self) -> None:
        contract = {gap["path"] for gap in read(CONTRACT_PATH)["gaps"]}
        self.assertEqual({gap["path"] for gap in self.gaps()}, contract)

    def test_the_second_attempts_two_absences_are_no_longer_absences(self) -> None:
        closed = {gap["path"] for gap in self.gaps() if not gap["stillAbsent"]}
        for absent in read(SECOND_QUALIFICATION_PATH)["knownAbsentBeforeTheRun"]:
            self.assertIn(absent["path"], closed, absent["path"])

    def test_the_launcher_prerequisites_are_now_a_graded_condition(self) -> None:
        # While the material was absent the launcher's refusal was a known
        # result.  With it present, refusing is a failure and must be scored.
        condition = conditions_by_id()["launcher-prerequisites-verify-inside-the-guest"]
        for phrase in ("account", "runtime rootfs", "content manifest"):
            self.assertIn(phrase, condition["condition"].lower(), phrase)

    def test_what_is_still_absent_is_written_down_before_the_run(self) -> None:
        absent = document()["knownAbsentBeforeTheRun"]
        self.assertTrue(absent)
        for row in absent:
            self.assertTrue(row["what"].strip())
            self.assertTrue(row["consequence"].strip())

    def test_no_remaining_absence_is_listed_as_a_pass_condition(self) -> None:
        graded = " ".join(row["condition"] for row in document()["passConditions"])
        for row in document()["knownAbsentBeforeTheRun"]:
            self.assertNotIn(row["what"], graded)


class OperatorRequirementTests(unittest.TestCase):
    """Each thing the operator asked to be fixed maps onto something graded."""

    def requirements(self) -> list:
        return document()["operatorRequirements"]

    def test_every_requirement_is_stated_and_mapped(self) -> None:
        self.assertTrue(self.requirements())
        present = set(conditions_by_id())
        for row in self.requirements():
            self.assertTrue(row["requirement"].strip())
            self.assertTrue(row["conditionIds"], row["requirement"])
            for identifier in row["conditionIds"]:
                self.assertIn(identifier, present, identifier)

    def test_the_thirteen_items_are_all_accounted_for(self) -> None:
        self.assertEqual(len(self.requirements()), 13)

    def test_no_condition_is_left_answering_nothing(self) -> None:
        # Not a coverage count for its own sake: a condition that maps to no
        # requirement is either an inherited safeguard or a stray, and the
        # record has to say which.
        mapped = {
            identifier
            for row in self.requirements()
            for identifier in row["conditionIds"]
        }
        inherited = set(document()["conditionsKeptBeyondTheRequestedList"])
        self.assertEqual(mapped | inherited, set(conditions_by_id()))
        self.assertFalse(mapped & inherited)


class IsolationAndSigningTests(unittest.TestCase):
    """The parts that were right before are carried, not rewritten."""

    def test_the_kernel_command_line_is_the_same_frozen_string(self) -> None:
        self.assertEqual(
            document()["boot"]["kernelCommandLine"],
            read(FIRST_QUALIFICATION_PATH)["boot"]["kernelCommandLine"],
        )

    def test_the_isolation_is_the_same_and_still_closed(self) -> None:
        isolation = document()["isolation"]
        self.assertEqual(isolation["networkDevices"], 0)
        self.assertEqual(isolation["sharedDirectories"], 0)
        self.assertEqual(isolation["writableDisksAttached"], 0)
        self.assertFalse(isolation["hostFilesystemExposedToGuest"])
        self.assertTrue(isolation["rootDiskAttachedReadOnly"])

    def test_no_release_identity_is_used(self) -> None:
        signing = document()["signing"]
        self.assertTrue(signing["adHocOnly"])
        self.assertEqual(signing["entitlement"], "com.apple.security.virtualization")
        for forbidden in (
            "teamId",
            "developerIdCertificate",
            "provisioningProfile",
            "notarization",
        ):
            self.assertFalse(signing[forbidden], forbidden)

    def test_the_invariants_carry_across_unchanged(self) -> None:
        self.assertEqual(document()["invariants"], read(GREEN_PATH)["invariants"])


class AbortTests(unittest.TestCase):
    """What stops the run rather than being worked around."""

    def aborts(self) -> list:
        return document()["abortConditions"]

    def test_the_first_attempts_abort_conditions_all_survive_verbatim(self) -> None:
        carried = set(self.aborts())
        for condition in read(FIRST_QUALIFICATION_PATH)["abortConditions"]:
            self.assertIn(condition, carried, condition)

    def test_the_second_attempts_are_carried_or_declared_superseded(self) -> None:
        """Nothing is dropped quietly; a line that goes says what replaced it.

        One of them speaks of five directories added to an image two versions
        ago, which this attempt's image is not.  Carrying that wording forward
        would put a condition in the list that cannot trigger, so it is
        superseded by name rather than left there looking like a safeguard.
        """

        live = set(self.aborts())
        disposition = {
            row["condition"]: row
            for row in document()["abortConditionsFromTheSecondAttempt"]
        }
        for condition in read(SECOND_QUALIFICATION_PATH)["abortConditions"]:
            self.assertIn(condition, disposition, condition)
            row = disposition[condition]
            if row["disposition"] == "carried":
                self.assertIn(condition, live, condition)
            else:
                self.assertEqual(row["disposition"], "superseded", condition)
                self.assertTrue(row["why"].strip(), condition)
                self.assertIn(row["supersededBy"], live, condition)

    def test_a_digest_that_moved_in_the_archive_stops_the_run(self) -> None:
        joined = " ".join(self.aborts()).lower()
        for phrase in ("archive", "digest"):
            self.assertIn(phrase, joined, phrase)

    def test_relaxing_the_archive_permissions_stops_the_run(self) -> None:
        joined = " ".join(self.aborts()).lower()
        self.assertIn("writable", joined)

    def test_reaching_past_the_boot_stops_the_run(self) -> None:
        joined = " ".join(self.aborts()).lower()
        for phrase in ("mac.4", "node", "mining"):
            self.assertIn(phrase, joined, phrase)

    def test_a_second_boot_stops_the_run(self) -> None:
        joined = " ".join(self.aborts()).lower()
        self.assertIn("second", joined)


class BoundaryTests(unittest.TestCase):
    """A boot is not serving, and this record refuses to be read as either."""

    def test_it_claims_nothing_before_the_run(self) -> None:
        record = document()
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["bootableClaim"])
        self.assertFalse(record["servingClaim"])
        for key in (
            "cleanMacEvidence",
            "guestBootVerified",
            "launcherServing",
            "mac4Started",
            "miningEnabled",
            "nodeConnected",
            "productRelease",
            "publicMiningOrBenchmark",
        ):
            self.assertFalse(record["boundaries"][key], key)

    def test_the_earlier_boundaries_are_not_narrowed(self) -> None:
        carried = set(document()["notEstablishedByAPass"])
        for row in read(FIRST_QUALIFICATION_PATH)["notEstablishedByAPass"]:
            self.assertIn(row, carried, row)

    def test_what_is_out_of_scope_is_named_rather_than_implied(self) -> None:
        joined = " ".join(document()["outOfScope"]).lower()
        for phrase in ("mac.4", "node", "mining", "reward", "wallet"):
            self.assertIn(phrase, joined, phrase)

    def test_a_pass_would_still_not_reopen_the_release_gates(self) -> None:
        joined = " ".join(document()["notEstablishedByAPass"]).lower()
        for gate in ("curl.3", "clean-mac", "release", "public mining", "activation"):
            self.assertIn(gate, joined, gate)

    def test_the_serving_claim_needs_more_than_the_launcher_starting(self) -> None:
        # The launcher reaching its prerequisites is graded here.  Serving an
        # execution is not, and a pass must not be read as one.
        joined = " ".join(document()["notEstablishedByAPass"]).lower()
        self.assertIn("serving", joined)

    def test_what_gets_recorded_regardless_of_verdict_is_not_reduced(self) -> None:
        carried = set(document()["recordRegardlessOfVerdict"])
        for row in read(FIRST_QUALIFICATION_PATH)["recordRegardlessOfVerdict"]:
            self.assertIn(row, carried, row)


class OffsiteCopyTests(unittest.TestCase):
    """The missing backup: not a blocker here, and not forgotten either."""

    def offsite(self) -> dict:
        return document()["offsiteCopy"]

    def test_it_records_that_no_offsite_copy_exists(self) -> None:
        offsite = self.offsite()
        self.assertFalse(offsite["existsToday"])
        self.assertEqual(
            offsite["existsToday"],
            read(PRESERVATION_PATH)["boundaries"]["offsiteCopyExists"],
        )

    def test_it_does_not_block_this_closed_local_boot(self) -> None:
        self.assertFalse(self.offsite()["blocksThisClosedLocalBoot"])
        self.assertTrue(self.offsite()["why"].strip())

    def test_it_is_still_required_before_anything_is_distributed(self) -> None:
        self.assertTrue(self.offsite()["requiredBeforeProductDistribution"])


class PredecessorTests(unittest.TestCase):
    """Every record leaned on is bound, and none of them is edited."""

    def test_each_predecessor_is_on_disk_at_the_digest_named(self) -> None:
        predecessors = document()["predecessorRecords"]
        self.assertTrue(predecessors)
        for row in predecessors:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])
            self.assertTrue(row["role"].strip(), row["path"])

    def test_the_records_this_exam_is_built_from_are_among_them(self) -> None:
        paths = {row["path"] for row in document()["predecessorRecords"]}
        for path in (
            FIRST_QUALIFICATION_PATH,
            SECOND_QUALIFICATION_PATH,
            SECOND_RESULT_PATH,
            CONTRACT_PATH,
            CORRECTION_PATH,
            PRESERVATION_PATH,
            PRODUCTION_RESULT_PATH,
            PRODUCTION_AUTHORITY_PATH,
        ):
            self.assertIn(path.relative_to(REPO).as_posix(), paths, path.name)

    def test_the_sealed_records_are_left_byte_unchanged(self) -> None:
        append_only = document()["appendOnly"]
        self.assertTrue(append_only["policy"].strip())
        self.assertTrue(append_only["why"].strip())
        for row in append_only["recordsLeftByteUnchanged"]:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])
            self.assertEqual(path.stat().st_size, row["sizeBytes"], row["path"])

    def test_the_two_spent_attempts_are_restated_as_spent(self) -> None:
        for row in document()["earlierAttempts"]:
            self.assertEqual(row["runsAllowed"], 1)
            self.assertEqual(row["runsPerformed"], 1)
            self.assertFalse(row["rerunPermitted"])
            self.assertFalse(row["reusedByThisAttempt"])

    def test_what_it_says_of_them_matches_their_own_receipts(self) -> None:
        for row in document()["earlierAttempts"]:
            sealed = read(REPO / row["resultPath"])
            self.assertEqual(row["verdict"], sealed["verdict"], row["attemptId"])
            self.assertEqual(row["runsPerformed"], sealed["runsPerformed"])


class GateTests(unittest.TestCase):
    """The record and this module are held by gates that run on every push."""

    def test_the_record_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(QUALIFICATION_PATH.relative_to(REPO).as_posix(), smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)

    def test_the_record_is_canonical(self) -> None:
        self.assertEqual(
            QUALIFICATION_PATH.read_bytes(),
            (json.dumps(document(), indent=2, sort_keys=True) + "\n").encode("utf-8"),
        )


if __name__ == "__main__":
    unittest.main()
