#!/usr/bin/env python3
"""The contract a replacement e2fsprogs has to satisfy, written before it is fetched.

The frozen mke2fs overwrites a staged file's `i_ctime` from the staging file's
`st_ctime`, which no caller can set, so the root disk cannot be made
byte-identical with that binary no matter what `E2FSPROGS_FAKE_TIME` is set to.
The approved way forward is an official e2fsprogs whose `create_inode` honours
`fs->now` instead.

This record exists so the candidate, its origin, the trust chain and the
accept/reject rule are fixed in the remote before anything is downloaded.  Fixed
first, then read: a criterion written after seeing the binary is not a
criterion.  Every test here reads only the record and files already in the tree.
"""
from __future__ import annotations

import hashlib
import json
import re
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RECORD_PATH = (
    REPO
    / "native/containment/native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json"
)

SHA256_LITERAL = re.compile(r"\b[0-9a-f]{64}\b")


def document() -> dict:
    return json.loads(RECORD_PATH.read_text(encoding="utf-8"))


class PreRegistrationTests(unittest.TestCase):
    """Pre-registration means this record precedes the fetch, not explains it."""

    def test_this_record_downloads_nothing_and_produces_nothing(self) -> None:
        pre = document()["preRegistration"]
        self.assertEqual(pre["downloadsPerformedByThisRecord"], 0)
        self.assertEqual(pre["producedArtifactsByThisRecord"], 0)
        self.assertFalse(pre["productionCodeChangedByThisRecord"])

    def test_it_must_reach_the_remote_before_any_candidate_is_fetched(self) -> None:
        pre = document()["preRegistration"]
        self.assertTrue(pre["pushedToRemoteBeforeAnyDownload"])
        self.assertTrue(pre["amendForbidden"])

    def test_nothing_here_claims_a_boot_or_an_activation(self) -> None:
        record = document()
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["bootableClaim"])
        for value in record["boundaries"].values():
            self.assertFalse(value)


class BindingTests(unittest.TestCase):
    """The records this one builds on are pinned, not merely named."""

    def pins(self) -> list[dict]:
        return document()["bindings"]["recordsThatStayByteUnchanged"]

    def test_every_bound_record_matches_its_digest_on_disk(self) -> None:
        for pin in self.pins():
            raw = (REPO / pin["path"]).read_bytes()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), pin["sha256"], pin["path"])
            self.assertEqual(len(raw), pin["sizeBytes"], pin["path"])

    def test_the_sealed_hard_stop_and_the_source_lock_are_both_bound(self) -> None:
        bound = {pin["path"] for pin in self.pins()}
        for required in (
            "native/containment/native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json",
            "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json",
        ):
            self.assertIn(required, bound)

    def test_the_existing_lock_is_added_to_rather_than_edited(self) -> None:
        method = document()["sourceLockAdditionMethod"]
        self.assertTrue(method["successorOnly"])
        self.assertFalse(method["existingLockEdited"])
        self.assertEqual(method["existingPackagesRedownloaded"], 0)
        self.assertEqual(method["existingPackageCountThatStays"], 191)


class DefectTests(unittest.TestCase):
    """What the candidate must fix is stated as the measured field, not as a mood.

    The counts come from the sealed hard-stop record's byte diff and are checked
    against it here, so this record cannot drift from the evidence it rests on.
    """

    def sealed(self) -> dict:
        pin = next(
            p
            for p in document()["bindings"]["recordsThatStayByteUnchanged"]
            if p["path"].endswith("determinism-hard-stop-arm64-v1.json")
        )
        return json.loads((REPO / pin["path"]).read_text(encoding="utf-8"))

    def test_the_field_the_candidate_must_fix_is_the_one_still_moving(self) -> None:
        defect = document()["defect"]
        self.assertEqual(defect["fieldsThatMustBecomeDeterministic"], ["i_ctime"])

    def test_the_fields_the_fixed_time_already_handles_are_listed_apart(self) -> None:
        # Separating these two sets is the whole reason the successor value is
        # necessary but not sufficient.  Collapsing them would hide the defect.
        defect = document()["defect"]
        handled = set(defect["fieldsTheFixedTimeAlreadyHandles"])
        must_fix = set(defect["fieldsThatMustBecomeDeterministic"])
        self.assertEqual(handled & must_fix, set())
        self.assertIn("i_crtime", handled)
        self.assertIn("s_mkfs_time", handled)

    def test_every_field_named_here_actually_differed_in_the_sealed_run(self) -> None:
        differed = set(self.sealed()["investigation"]["byteDiff"]["byField"])
        defect = document()["defect"]
        for field in defect["fieldsThatMustBecomeDeterministic"]:
            self.assertIn(field, differed, field)
        for field in defect["fieldsTheFixedTimeAlreadyHandles"]:
            self.assertIn(field, differed, field)

    def test_the_recorded_counts_are_the_sealed_counts(self) -> None:
        counts = self.sealed()["investigation"]["byteDiff"]["byField"]
        for field, count in document()["defect"]["recordCountsFromTheSealedDiff"].items():
            self.assertEqual(count, counts[field], field)

    def test_the_reason_the_current_binary_cannot_be_rescued_is_written_down(self) -> None:
        defect = document()["defect"]
        self.assertTrue(defect["whyTheFrozenBinaryCannotBeMadeDeterministic"].strip())
        self.assertFalse(defect["settableFromUserspace"])


class CandidateTests(unittest.TestCase):
    """Official builds only, and no guessing that a later version fixed it."""

    def test_only_official_distribution_is_allowed(self) -> None:
        family = document()["candidateFamily"]
        self.assertTrue(family["officialDistributionOnly"])
        for forbidden in ("forks", "localPatches", "selfBuiltFromTarball"):
            self.assertTrue(family["forbidden"][forbidden], forbidden)

    def test_a_later_version_is_a_hypothesis_rather_than_an_answer(self) -> None:
        family = document()["candidateFamily"]
        self.assertFalse(family["assumeNewerVersionIsFixed"])
        self.assertTrue(family["decidedByStaticReadOfTheCandidateBinary"])

    def test_the_concrete_version_is_pinned_before_any_deb_is_fetched(self) -> None:
        # This assertion read `resolvedCandidates == []` in the pre-registration
        # commit, which is what "pre-registered" meant while nothing had been
        # resolved.  The list is filled from signature-verified indexes and the
        # ordering rule it protects is unchanged and asserted below: every
        # candidate is fully pinned, and no deb had been fetched when it was
        # written.  Git holds the earlier state; loosening to a subset check
        # would give up the guarantee, so each entry is checked in full.
        discovery = document()["candidateDiscovery"]
        self.assertTrue(discovery["mustBeAppendedAndPushedBeforeFetchingTheDeb"])
        self.assertEqual(discovery["debsFetchedSoFar"], 0)
        for field in ("version", "sizeBytes", "sha256"):
            self.assertIn(field, discovery["fieldsResolvedFromTheSignedIndex"])

    def test_every_resolved_candidate_is_fully_pinned_and_signature_backed(self) -> None:
        candidates = document()["candidateDiscovery"]["resolvedCandidates"]
        self.assertTrue(candidates, "a resolved list with nothing in it pins nothing")
        for candidate in candidates:
            self.assertTrue(candidate["inReleaseSignatureVerified"], candidate["suite"])
            self.assertTrue(candidate["packagesIndexDigestVerified"], candidate["suite"])
            self.assertRegex(candidate["packagesIndexSha256"], r"^[0-9a-f]{64}$")
            names = {package["name"] for package in candidate["packages"]}
            self.assertEqual(names, {"e2fsprogs", "libext2fs2t64"}, candidate["suite"])
            for package in candidate["packages"]:
                self.assertRegex(package["sha256"], r"^[0-9a-f]{64}$", package["name"])
                self.assertGreater(package["sizeBytes"], 0, package["name"])
                self.assertTrue(package["version"].strip(), package["name"])

    def test_the_candidates_were_verified_with_the_keyring_the_lock_pins(self) -> None:
        lock = json.loads(
            (REPO / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json")
            .read_text(encoding="utf-8")
        )
        pinned = next(a["sha256"] for a in lock["artifacts"] if a["id"] == "ubuntu-keyring")
        for candidate in document()["candidateDiscovery"]["resolvedCandidates"]:
            self.assertEqual(candidate["verifiedWithKeyringSha256"], pinned, candidate["suite"])

    def test_the_trust_chain_runs_from_the_keyring_to_the_package_digest(self) -> None:
        steps = document()["trustChain"]["steps"]
        self.assertEqual(
            [step["verifies"] for step in steps],
            ["ubuntu-keyring", "ubuntu-inrelease", "ubuntu-packages-index", "deb"],
        )
        for step in steps:
            self.assertTrue(step["how"].strip(), step["verifies"])

    def test_the_trust_chain_roots_in_artifacts_the_lock_already_pins(self) -> None:
        lock = json.loads(
            (REPO / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json")
            .read_text(encoding="utf-8")
        )
        pinned = {artifact["id"]: artifact["sha256"] for artifact in lock["artifacts"]}
        for step in document()["trustChain"]["steps"]:
            recorded = step.get("artifactIdInTheExistingLock")
            if recorded:
                self.assertIn(recorded, pinned, recorded)
                self.assertEqual(step["sha256"], pinned[recorded], recorded)


class AcceptanceTests(unittest.TestCase):
    """Both paths and both binaries, decided by reading rather than by running."""

    def criteria(self) -> dict:
        return document()["acceptanceCriteria"]

    def test_both_write_paths_have_to_be_read(self) -> None:
        paths = {path["id"]: path for path in self.criteria()["paths"]}
        self.assertIn("staged-file-inode", paths)
        self.assertIn("self-created-inode-and-superblock", paths)
        for path in paths.values():
            self.assertTrue(path["mustBeRead"])
            self.assertTrue(path["passCondition"].strip())

    def test_both_binaries_have_to_be_read(self) -> None:
        binaries = {binary["name"] for binary in self.criteria()["binaries"]}
        self.assertEqual(binaries, {"mke2fs", "libext2fs"})
        for binary in self.criteria()["binaries"]:
            self.assertTrue(binary["mustHonourTheFixedTime"])

    def test_the_finding_is_static_and_nothing_is_executed_to_establish_it(self) -> None:
        criteria = self.criteria()
        self.assertTrue(criteria["staticOnly"])
        self.assertFalse(criteria["establishedByRunningTheBinary"])

    def test_the_read_carries_the_same_evidence_standard_as_the_last_one(self) -> None:
        # The previous walk was accepted because it counted an absence over a
        # stated window and showed a positive control.  A weaker read of the
        # candidate would not be comparable evidence.
        evidence = self.criteria()["evidenceStandard"]
        self.assertTrue(evidence["offsetsRecomputedFromStructOffsets"])
        self.assertTrue(evidence["absenceCountedOverAStatedWindow"])
        self.assertTrue(evidence["positiveControlInTheSameBinary"])

    def test_the_pass_condition_names_the_write_that_must_be_gone(self) -> None:
        staged = next(
            path for path in self.criteria()["paths"] if path["id"] == "staged-file-inode"
        )
        self.assertIn("st_ctime", staged["passCondition"])
        self.assertIn("i_ctime", staged["passCondition"])


class HardStopTests(unittest.TestCase):
    """The two outcomes that end the attempt rather than adapt it."""

    def test_no_official_candidate_is_a_stop_rather_than_a_workaround(self) -> None:
        conditions = {c["id"]: c for c in document()["hardStopConditions"]}
        self.assertIn("no-official-candidate-fixes-it", conditions)
        self.assertIn("candidate-still-copies-staged-ctime", conditions)
        for condition in conditions.values():
            self.assertTrue(condition["stop"])
            self.assertFalse(condition["proceedAnyway"])

    def test_patching_or_switching_builders_is_not_this_records_decision(self) -> None:
        deferred = document()["deferredToTheOperator"]
        self.assertIn("local-patch", deferred["options"])
        self.assertIn("different-image-builder", deferred["options"])
        self.assertFalse(deferred["decidedHere"])

    def test_production_stays_blocked_until_a_candidate_passes(self) -> None:
        readiness = document()["productionReadiness"]
        self.assertTrue(readiness["blocked"])
        self.assertTrue(readiness["dispatchForbiddenWhileBlocked"])
        self.assertTrue(readiness["unblocksOnlyOnAPassingStaticRead"])


class InvariantTests(unittest.TestCase):
    def test_the_numbers_this_slice_must_not_move_are_written_down_unmoved(self) -> None:
        invariants = document()["invariants"]
        self.assertEqual(invariants["LLM-MINEABLE-ELIGIBLE-V5"], 14160)
        self.assertEqual(invariants["mineable_now"], 0)
        self.assertEqual(invariants["REWARD_READY"], 0)
        self.assertEqual(invariants["RP0-MD"], "HOLD")
        self.assertEqual(invariants["BF.7"], "HOLD")
        self.assertFalse(invariants["baseActivation"])

    def test_no_digest_appears_that_this_record_cannot_account_for(self) -> None:
        # Equality, not a subset: a digest here that belongs to no pin, no trust
        # chain step and no signature-verified candidate stanza is exactly the
        # loose digest this guard exists to catch.  The candidate digests are
        # accounted for by the signed index they were read from rather than by a
        # local recompute, which is why they are enumerated from their structure
        # rather than waved through by a regex.
        record = document()
        allowed = {pin["sha256"] for pin in record["bindings"]["recordsThatStayByteUnchanged"]}
        allowed |= {
            step["sha256"] for step in record["trustChain"]["steps"] if step.get("sha256")
        }
        for candidate in record["candidateDiscovery"]["resolvedCandidates"]:
            allowed.add(candidate["packagesIndexSha256"])
            allowed.add(candidate["verifiedWithKeyringSha256"])
            allowed |= {package["sha256"] for package in candidate["packages"]}
        found = set(SHA256_LITERAL.findall(RECORD_PATH.read_text(encoding="utf-8")))
        self.assertEqual(found, allowed)


if __name__ == "__main__":
    unittest.main()
