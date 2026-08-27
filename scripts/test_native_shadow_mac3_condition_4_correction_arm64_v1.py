"""The corrected MAC.3 condition 4, pre-registered before anything is built against it.

The operator's decision replaced the subject of a held condition: the thing that
must be unprivileged is not the launcher, which is the privilege manager, but the
checker and submission processes the launcher creates. A correction is only worth
the name if it can be told apart from a relaxation, so every clause of the
corrected wording is required here to name code that already enforces it, and
each of those files is re-digested from the tree rather than quoted.
"""

import hashlib
import json
import pathlib
import re
import unittest

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
RECORD_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json"
)
CONTRACT_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json"
)
PRIVILEGE_PATH = REPOSITORY_ROOT / "crates/boole-native-shadow-launcher/src/privilege.rs"
CONTAINMENT_PATH = (
    REPOSITORY_ROOT
    / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
)
POLICY_PATH = REPOSITORY_ROOT / "native/containment/native-shadow-execution-policy-v1.json"

SEALED_CAPABILITIES = ["CAP_SETGID", "CAP_SETUID", "CAP_SETPCAP", "CAP_SYS_ADMIN"]
HELD_CONDITION_ID = "launcher-runs-under-an-unprivileged-account"


def digest_of(path):
    """Return the (sha256, size) of a file as the tree actually holds it."""
    payload = path.read_bytes()
    return hashlib.sha256(payload).hexdigest(), len(payload)


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


class RecordShapeTests(unittest.TestCase):
    """A pre-registration answers nothing, so it may not read like a result."""

    def setUp(self):
        self.record = load(RECORD_PATH)

    def test_the_record_is_the_arm64_condition_correction_schema(self):
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3-condition-4-correction.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "MAC3-CONDITION-4-CORRECTED-PRE-REGISTERED-NOT-IMPLEMENTED",
        )
        self.assertTrue(self.record["release"].endswith("PRE-REGISTERED-NOT-IMPLEMENTED"))

    def test_no_outcome_key_may_appear_anywhere_in_the_record(self):
        """Pre-registration that carries a verdict is a report wearing its name."""
        forbidden = {"verdict", "passed", "result", "servingReached"}
        seen = set()

        def walk(node):
            if isinstance(node, dict):
                for key, value in node.items():
                    seen.add(key)
                    walk(value)
            elif isinstance(node, list):
                for item in node:
                    walk(item)

        walk(self.record)
        self.assertEqual(forbidden & seen, set())

    def test_nothing_is_claimed_to_have_been_built_produced_or_booted(self):
        nothing = self.record["nothingWasBuilt"]
        for field in (
            "imageProduced",
            "productionDispatched",
            "bootPerformed",
            "builderChanged",
            "unitFileChanged",
            "launcherCodeChanged",
        ):
            self.assertIs(nothing[field], False, field)
        self.assertIs(self.record["activationAllowed"], False)
        self.assertIs(self.record["servingClaim"], False)


class SupersededConditionTests(unittest.TestCase):
    """What is being corrected has to match the sealed contract exactly."""

    def setUp(self):
        self.record = load(RECORD_PATH)
        self.section = self.record["supersededCondition"]
        self.contract = load(CONTRACT_PATH)

    def test_the_contract_is_stamped_as_the_tree_holds_it(self):
        stamp = self.section["contractStamp"]
        sha256, size = digest_of(CONTRACT_PATH)
        self.assertEqual(stamp["path"], "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json")
        self.assertEqual(stamp["sha256"], sha256)
        self.assertEqual(stamp["sizeBytes"], size)

    def test_the_original_wording_and_status_are_quoted_from_the_contract(self):
        """Correcting a condition by first misquoting it would prove nothing."""
        original = next(
            row for row in self.contract["conditions"] if row["id"] == HELD_CONDITION_ID
        )
        self.assertEqual(self.section["id"], original["id"])
        self.assertEqual(self.section["originalWording"], original["condition"])
        self.assertEqual(self.section["originalStatus"], original["status"])
        self.assertEqual(original["status"], "held")

    def test_the_held_state_is_carried_over_as_it_was_before_the_decision(self):
        held = self.contract["heldCondition"]
        before = self.section["originalHeldState"]
        for field in ("relaxed", "waived", "satisfied", "state"):
            self.assertEqual(before[field], held[field], field)
        self.assertEqual(held["state"], "awaiting an operator decision")

    def test_the_sealed_contract_itself_is_not_edited_by_this_record(self):
        """Append-only means the held condition still reads as held in its own file."""
        appended = self.record["appendOnly"]
        self.assertIs(appended["originalRecordEdited"], False)
        self.assertIs(appended["originalHeldConditionStillReadsAsHeld"], True)
        held = self.contract["heldCondition"]
        self.assertIs(held["relaxed"], False)
        self.assertIs(held["waived"], False)
        self.assertIs(held["readingApplied"], False)


class OperatorDecisionTests(unittest.TestCase):
    """The decision is the input to this record and is recorded as given."""

    def setUp(self):
        self.section = load(RECORD_PATH)["operatorDecision"]

    def test_the_choice_is_the_correction_and_not_the_redesign(self):
        self.assertEqual(self.section["choice"], "1-a")
        self.assertEqual(self.section["rejectedChoice"], "1-b")
        self.assertTrue(self.section["whyTheOtherWasRejected"].strip())

    def test_every_clause_is_numbered_and_carries_its_own_source_wording(self):
        """Clauses stay in the order given, and none may be silently merged away."""
        clauses = self.section["clauses"]
        self.assertEqual(len(clauses), self.section["clauseCount"])
        self.assertGreaterEqual(len(clauses), 7)
        self.assertEqual(len({row["id"] for row in clauses}), len(clauses))
        self.assertEqual(
            [row["order"] for row in clauses], list(range(1, len(clauses) + 1))
        )
        for row in clauses:
            self.assertTrue(row["text"].strip(), row["id"])
            self.assertTrue(row["sourceWording"].strip(), row["id"])


class ClauseEnforcementTests(unittest.TestCase):
    """A clause with no enforcement site is a wish, not a condition."""

    def setUp(self):
        self.record = load(RECORD_PATH)
        self.rows = self.record["clauseEnforcement"]

    def test_every_clause_of_the_decision_has_an_enforcement_row(self):
        clauses = {row["id"] for row in self.record["operatorDecision"]["clauses"]}
        enforced = {row["clauseId"] for row in self.rows}
        self.assertEqual(clauses, enforced)

    def test_every_named_enforcement_file_matches_the_tree(self):
        """Citing a file by a digest it no longer has is the drift this catches."""
        for row in self.rows:
            for stamp in row["enforcedBy"]:
                path = REPOSITORY_ROOT / stamp["path"]
                self.assertTrue(path.is_file(), stamp["path"])
                sha256, size = digest_of(path)
                self.assertEqual(stamp["sha256"], sha256, stamp["path"])
                self.assertEqual(stamp["sizeBytes"], size, stamp["path"])

    def test_every_clause_names_the_check_and_names_it_as_already_existing(self):
        for row in self.rows:
            self.assertTrue(row["theCheck"].strip(), row["clauseId"])
            self.assertIs(row["existsBeforeThisRecord"], True, row["clauseId"])
            self.assertGreaterEqual(len(row["enforcedBy"]), 1, row["clauseId"])


class CapabilityMaskTests(unittest.TestCase):
    """The four capabilities are the load-bearing number, so they are recomputed."""

    def setUp(self):
        self.section = load(RECORD_PATH)["capabilityMask"]

    def test_the_four_names_are_the_sealed_four(self):
        self.assertEqual(self.section["names"], SEALED_CAPABILITIES)
        self.assertEqual(self.section["count"], 4)

    def test_the_mask_is_the_arithmetic_of_its_own_bit_positions(self):
        expected = 0
        for bit in self.section["bitPositions"]:
            expected |= 1 << bit
        self.assertEqual(self.section["maskDecimal"], expected)
        self.assertEqual(self.section["maskHex"], f"0x{expected:016x}")

    def test_the_mask_is_the_one_compiled_into_the_launcher(self):
        """If the constant moves, this record stops describing the running code."""
        source = PRIVILEGE_PATH.read_text(encoding="utf-8")
        shifts = re.findall(r"\(1_u64 << (\d+)\)", source)
        self.assertTrue(shifts, "the launcher no longer builds its mask from shifts")
        compiled = 0
        for shift in shifts:
            compiled |= 1 << int(shift)
        self.assertEqual(self.section["maskDecimal"], compiled)
        self.assertEqual(sorted(int(s) for s in shifts), sorted(self.section["bitPositions"]))

    def test_the_policy_and_the_unit_name_the_same_four(self):
        policy = load(POLICY_PATH)["privilege"]
        for slot in ("effective", "permitted", "bounding"):
            self.assertEqual(policy["launcherCapabilitySets"][slot], SEALED_CAPABILITIES)
        for slot in ("inheritable", "ambient"):
            self.assertEqual(policy["launcherCapabilitySets"][slot], [])
        self.assertEqual(policy["systemdUnit"]["CapabilityBoundingSet"], SEALED_CAPABILITIES)
        self.assertEqual(policy["systemdUnit"]["AmbientCapabilities"], [])

    def test_the_comparison_is_equality_and_not_containment(self):
        """A subset check would pass an extra capability, which the clause forbids."""
        self.assertEqual(self.section["comparison"], "exact-equality")
        source = PRIVILEGE_PATH.read_text(encoding="utf-8")
        self.assertIn("if actual != expected {", source)
        self.assertIn("CapabilityMismatch", source)


class DropOrderTests(unittest.TestCase):
    """Submissions running unprivileged is an ordering claim, so the order is read."""

    def setUp(self):
        self.section = load(RECORD_PATH)["dropOrder"]
        self.source = CONTAINMENT_PATH.read_text(encoding="utf-8")

    def test_the_three_stages_appear_in_the_recorded_order(self):
        lines = {}
        for name, needle in (
            ("drop", "drop_all_privileges(setup.checker_uid, setup.checker_gid)"),
            ("verify", "verify_dropped_privileges(setup.checker_uid, setup.checker_gid)"),
            ("exec", "libc::execve(CHECKER_PATH.as_ptr()"),
        ):
            index = self.source.find(needle)
            self.assertNotEqual(index, -1, name)
            lines[name] = self.source.count("\n", 0, index) + 1
        self.assertLess(lines["drop"], lines["verify"])
        self.assertLess(lines["verify"], lines["exec"])
        self.assertEqual(self.section["dropAtLine"], lines["drop"])
        self.assertEqual(self.section["verifyAtLine"], lines["verify"])
        self.assertEqual(self.section["execAtLine"], lines["exec"])

    def test_the_bounding_set_is_emptied_before_the_identity_changes(self):
        """Dropping uid first would leave capabilities reacquirable across exec."""
        drop_index = self.source.find("fn drop_all_privileges")
        body = self.source[drop_index : drop_index + 2000]
        capbset = body.find("PR_CAPBSET_DROP")
        setresuid = body.find("setresuid")
        self.assertNotEqual(capbset, -1)
        self.assertNotEqual(setresuid, -1)
        self.assertLess(capbset, setresuid)
        self.assertIs(self.section["boundingSetEmptiedBeforeIdentityChange"], True)

    def test_the_post_drop_verification_requires_every_set_to_be_empty(self):
        verify_index = self.source.find("fn verify_dropped_privileges")
        body = self.source[verify_index : verify_index + 2200]
        for field in ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"):
            self.assertIn(field, body, field)
        self.assertIn("NoNewPrivs", body)
        self.assertEqual(
            self.section["capabilitySetsRequiredEmptyAfterDrop"],
            ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"],
        )
        self.assertIs(self.section["noNewPrivilegesRequiredOnTheChecker"], True)

    def test_a_failed_stage_stops_before_exec_rather_than_continuing(self):
        self.assertIs(self.section["failClosedBeforeExec"], True)
        self.assertIn("setup_stage(", self.source)
        self.assertTrue(self.section["howFailureStops"].strip())


class NotARelaxationTests(unittest.TestCase):
    """The whole weight of the decision rests on this distinction."""

    def setUp(self):
        self.record = load(RECORD_PATH)
        self.section = self.record["whyThisIsNotARelaxation"]

    def test_the_record_states_plainly_that_it_is_a_correction(self):
        self.assertIs(self.section["isARelaxation"], False)
        self.assertIs(self.section["allowsSubmissionsToRunAsRoot"], False)
        self.assertEqual(self.section["whatChanged"], "the subject of the sentence")

    def test_no_clause_weakens_a_check_that_the_tree_already_performs(self):
        """Every clause must add or restate a refusal, never remove one."""
        for row in self.record["clauseEnforcement"]:
            self.assertIn(
                row["direction"], {"restates-an-existing-refusal", "adds-a-refusal"},
                row["clauseId"],
            )

    def test_the_correction_matches_the_reading_the_contract_already_offered(self):
        """The sealed contract proposed this reading and declined to apply it."""
        offered = load(CONTRACT_PATH)["heldCondition"]["readingOffered"]
        self.assertEqual(self.section["readingOfferedByTheSealedContract"], offered)
        self.assertIs(self.section["operatorAppliedThatReading"], True)


class WhatThisDoesNotEstablishTests(unittest.TestCase):
    """The gap between deciding a condition and passing it stays visible."""

    def setUp(self):
        self.record = load(RECORD_PATH)

    def test_the_record_says_what_it_leaves_open(self):
        rows = self.record["whatThisDoesNotEstablish"]
        self.assertGreaterEqual(len(rows), 4)
        for row in rows:
            self.assertTrue(row.strip())

    def test_the_three_serving_gaps_are_still_open(self):
        self.assertIs(self.record["servingGapsClosed"], False)
        self.assertEqual(self.record["servingGapsRemaining"], 3)


class RegistrationTests(unittest.TestCase):
    """A test nobody runs is not a check."""

    def test_the_suite_is_registered_in_self_test(self):
        text = (REPOSITORY_ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, text)

    def test_the_record_is_pinned_in_docs_smoke(self):
        text = (REPOSITORY_ROOT / "scripts/docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(
            "native-shadow-mac3-condition-4-correction-arm64-v1.json", text
        )


if __name__ == "__main__":
    unittest.main()
