"""What the guest may say, and what the host refuses to conclude from it.

The console is the only way out of a closed-local boot, so three of the five
stopped conditions depend entirely on records printed into it.  That makes the
reader a place where a mistake is expensive in a particular direction: a lenient
reader turns console noise into a pass, and a pass here spends the one boot on a
verdict nobody can stand behind.

So the tests below are mostly about refusing.  A record that appears twice with
two answers is dropped rather than resolved.  A record id the protocol does not
define is ignored rather than trusted.  A line that carries the prefix and then
does not parse is an error rather than a shrug.  And the half-condition a closed
boot cannot observe is never reported as observed, no matter what the guest
prints.
"""

import importlib
import hashlib
import json
import pathlib
import sys
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]


def protocol():
    if str(REPO / "scripts") not in sys.path:
        sys.path.insert(0, str(REPO / "scripts"))
    return importlib.import_module("native_shadow_mac3_guest_evidence_protocol_arm64_v1")


class FormatTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = protocol()

    def test_a_record_is_one_line_and_starts_with_the_prefix(self) -> None:
        """Two lines could be cut in half by another writer between them."""

        line = self.module.format_record("readiness", {"ready": True})
        self.assertNotIn("\n", line)
        self.assertTrue(line.startswith(self.module.PREFIX))

    def test_a_record_id_the_protocol_does_not_define_cannot_be_written(self) -> None:
        with self.assertRaises(self.module.MalformedRecord):
            self.module.format_record("invented-record", {"ready": True})

    def test_a_payload_carrying_a_line_break_still_renders_one_line(self) -> None:
        """A newline inside a record would split it into two half-records.

        The property being held is that the record is always one line, not that
        such a payload is rejected: JSON escapes the break, so the line stays
        whole and the value survives the round trip intact.
        """

        line = self.module.format_record("readiness", {"note": "one\ntwo"})
        self.assertNotIn("\n", line)
        identifier, payload = self.module.parse_line(line)
        self.assertEqual(identifier, "readiness")
        self.assertEqual(payload["note"], "one\ntwo")

    def test_the_same_payload_always_renders_the_same_line(self) -> None:
        first = self.module.format_record("readiness", {"b": 2, "a": 1})
        second = self.module.format_record("readiness", {"a": 1, "b": 2})
        self.assertEqual(first, second)

    def test_every_record_names_the_condition_it_feeds(self) -> None:
        for identifier in self.module.RECORDS:
            self.assertIn(identifier, self.module.FEEDS)
            self.assertTrue(self.module.FEEDS[identifier].strip())


class ParseTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = protocol()

    def test_a_record_survives_a_console_timestamp_in_front_of_it(self) -> None:
        """The console puts things in front of lines; the reader starts at the prefix."""

        line = "[   4.812345] boole-launcher[431]: " + self.module.format_record(
            "readiness", {"ready": True}
        )
        self.assertEqual(self.module.parse_line(line), ("readiness", {"ready": True}))

    def test_an_ordinary_console_line_is_not_a_record(self) -> None:
        self.assertIsNone(self.module.parse_line("[ 0.00 ] Linux version 6.8.0"))

    def test_a_line_with_the_prefix_and_no_payload_is_an_error(self) -> None:
        """Silence about a broken record is how a reader loses evidence quietly."""

        with self.assertRaises(self.module.MalformedRecord):
            self.module.parse_line(self.module.PREFIX + " readiness not-json-at-all")

    def test_a_payload_that_is_not_an_object_is_an_error(self) -> None:
        with self.assertRaises(self.module.MalformedRecord):
            self.module.parse_line(self.module.PREFIX + " readiness [1,2,3]")


class TranscriptTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = protocol()

    def transcript(self, *records) -> str:
        lines = ["[    0.000000] Linux version 6.8.0", "[    0.100000] systemd 255 running"]
        for identifier, payload in records:
            lines.append("[    4.000000] " + self.module.format_record(identifier, payload))
        lines.append("[    9.000000] Reached target graphical.target")
        return "\n".join(lines)

    def test_records_are_collected_out_of_ordinary_console_noise(self) -> None:
        read = self.module.read_transcript(
            self.transcript(("readiness", {"ready": True}))
        )
        self.assertEqual(read["records"]["readiness"], {"ready": True})

    def test_a_record_repeated_with_the_same_answer_is_not_a_conflict(self) -> None:
        read = self.module.read_transcript(
            self.transcript(("readiness", {"ready": True}), ("readiness", {"ready": True}))
        )
        self.assertEqual(read["conflicting"], [])
        self.assertIn("readiness", read["records"])

    def test_a_record_repeated_with_a_different_answer_is_dropped(self) -> None:
        """There is no way from here to tell which line describes this run.

        Preferring the last one would let a guest that printed the wrong answer
        first and the right answer second look identical to a guest that got it
        right, and preferring the first has the same problem reversed.
        """

        read = self.module.read_transcript(
            self.transcript(("readiness", {"ready": False}), ("readiness", {"ready": True}))
        )
        self.assertEqual(read["conflicting"], ["readiness"])
        self.assertNotIn("readiness", read["records"])
        met, _ = self.module.readiness_seen(read)
        self.assertFalse(met, "a conflicting record was still usable")

    def test_a_record_id_the_protocol_does_not_define_is_ignored(self) -> None:
        """The reader must not grow a new evidence source because a line asked."""

        line = "%s invented-record %s" % (self.module.PREFIX, json.dumps({"ok": True}))
        read = self.module.read_transcript(line)
        self.assertEqual(read["records"], {})
        self.assertEqual(read["unknownRecordIds"], ["invented-record"])

    def test_a_malformed_record_is_reported_rather_than_swallowed(self) -> None:
        read = self.module.read_transcript(self.module.PREFIX + " readiness {oops")
        self.assertEqual(len(read["malformed"]), 1)
        self.assertEqual(read["records"], {})

    def test_an_empty_transcript_answers_nothing(self) -> None:
        read = self.module.read_transcript("")
        self.assertEqual(read["records"], {})
        self.assertEqual(sorted(read["missing"]), sorted(self.module.RECORDS))

    def test_the_read_never_reports_submissions_as_observed(self) -> None:
        """Even if the guest prints the claim, a closed boot did not see it."""

        line = "%s readiness %s" % (
            self.module.PREFIX,
            json.dumps({"ready": True, "submissionsRanUnprivileged": True}),
        )
        read = self.module.read_transcript(line)
        self.assertFalse(read["submissionsObserved"])


class HostSideJudgementTests(unittest.TestCase):
    """The comparisons happen on the side that already knows the answer."""

    def setUp(self) -> None:
        self.module = protocol()

    def read(self, records) -> dict:
        lines = [self.module.format_record(name, body) for name, body in records.items()]
        return self.module.read_transcript("\n".join(lines))

    def test_a_launcher_digest_is_judged_against_the_sealed_value(self) -> None:
        read = self.read({"launcher-executable": {"sha256": "a" * 64}})
        met, why = self.module.launcher_digest_matches(read, "a" * 64)
        self.assertTrue(met, why)

    def test_a_guest_reporting_the_wrong_digest_fails_its_own_condition(self) -> None:
        """This is why a guest cannot talk its way to a pass."""

        read = self.read({"launcher-executable": {"sha256": "b" * 64}})
        met, why = self.module.launcher_digest_matches(read, "a" * 64)
        self.assertFalse(met)
        self.assertIn("a" * 64, why)

    def test_a_silent_guest_passes_nothing(self) -> None:
        read = self.module.read_transcript("")
        for met, _ in (
            self.module.launcher_digest_matches(read, "a" * 64),
            self.module.prerequisites_resolved(read),
            self.module.supervises_as_root(read),
            self.module.readiness_seen(read),
        ):
            self.assertFalse(met)

    def test_an_empty_prerequisite_list_is_not_a_pass(self) -> None:
        """Nothing checked and everything checked must not look the same."""

        read = self.read({"launcher-prerequisites": {"prerequisites": []}})
        met, why = self.module.prerequisites_resolved(read)
        self.assertFalse(met)
        self.assertIn("proves nothing", why)

    def test_one_unresolved_prerequisite_names_itself(self) -> None:
        read = self.read(
            {
                "launcher-prerequisites": {
                    "prerequisites": [
                        {"name": "lean-toolchain", "resolved": True},
                        {"name": "verifier-corpus", "resolved": False},
                    ]
                }
            }
        )
        met, why = self.module.prerequisites_resolved(read)
        self.assertFalse(met)
        self.assertIn("verifier-corpus", why)

    def test_present_is_not_an_alias_for_the_sealed_resolved_field(self) -> None:
        """A producer and reader must not silently speak two similar schemas."""

        read = self.read(
            {
                "launcher-prerequisites": {
                    "prerequisites": [{"name": "lean-toolchain", "present": True}]
                }
            }
        )
        met, why = self.module.prerequisites_resolved(read)
        self.assertFalse(met)
        self.assertIn("resolved", why)

    def test_the_shared_rehearsal_fixture_uses_the_field_the_reader_accepts(self) -> None:
        rehearsal = importlib.import_module(
            "native_shadow_mac3_closed_local_boot_rehearsal_arm64_v1"
        )
        read = self.module.read_transcript(rehearsal.healthy_console())
        met, why = self.module.prerequisites_resolved(read)
        self.assertTrue(met, why)

    def test_a_non_root_supervisor_fails(self) -> None:
        read = self.read({"supervisor-privilege": {"uid": 1000}})
        met, _ = self.module.supervises_as_root(read)
        self.assertFalse(met)

    def test_a_root_supervisor_passes_only_the_half_it_can_see(self) -> None:
        """The condition has two halves and only one of them was observed."""

        read = self.read({"supervisor-privilege": {"uid": 0}})
        met, why = self.module.supervises_as_root(read)
        self.assertTrue(met)
        self.assertIn("not observed", why)

    def test_ready_with_a_failed_unit_is_not_ready(self) -> None:
        read = self.read({"readiness": {"ready": True, "failedUnits": ["boole.service"]}})
        met, why = self.module.readiness_seen(read)
        self.assertFalse(met)
        self.assertIn("failed unit", why)


class SealedRecordTests(unittest.TestCase):
    """The written record and the code must not drift apart.

    A format described in one place and implemented in another is a format that
    will eventually disagree with itself, and the disagreement will be found by
    the one boot rather than by a test.
    """

    def setUp(self) -> None:
        self.module = protocol()
        path = (
            REPO
            / "native"
            / "containment"
            / "native-shadow-mac3-guest-console-evidence-protocol-arm64-v1.json"
        )
        self.record = json.loads(path.read_text(encoding="utf-8"))
        correction_path = (
            REPO
            / "native"
            / "containment"
            / "native-shadow-mac3-guest-console-evidence-protocol-arm64-v1-correction.json"
        )
        self.correction = json.loads(correction_path.read_text(encoding="utf-8"))

    def test_the_prefix_in_the_record_is_the_prefix_in_the_code(self) -> None:
        self.assertEqual(self.record["format"]["prefix"], self.module.PREFIX)

    def test_the_record_lists_exactly_the_records_the_code_defines(self) -> None:
        listed = [row["id"] for row in self.record["format"]["records"]]
        self.assertEqual(sorted(listed), sorted(self.module.RECORDS))

    def test_each_listed_record_feeds_the_condition_the_code_says(self) -> None:
        for row in self.record["format"]["records"]:
            self.assertEqual(row["feeds"], self.module.FEEDS[row["id"]], row["id"])

    def test_the_record_grants_no_boot_and_changes_no_condition(self) -> None:
        self.assertFalse(self.record["bootAuthorisation"]["grantedByThisRecord"])
        self.assertEqual(self.record["bootAuthorisation"]["runsPerformed"], 0)
        self.assertFalse(self.record["changesAnyPassCondition"])
        self.assertFalse(self.record["imageBytes"]["currentImageMayBeBooted"])
        self.assertFalse(self.record["imageBytes"]["currentImageMayBeModified"])

    def test_the_record_does_not_claim_the_unobservable_half(self) -> None:
        self.assertFalse(self.record["submissionsAreNotClaimed"]["emitted"])
        self.assertFalse(self.record["submissionsAreNotClaimed"]["conditionRelaxedHere"])

    def test_the_guest_side_producer_is_recorded_as_unwritten(self) -> None:
        """It is designed, not built. Saying otherwise would be the whole lie."""

        self.assertEqual(self.record["implementedBy"]["guestSideProducer"], "not written yet")

    def test_the_original_record_is_preserved_under_the_correction(self) -> None:
        path = (
            REPO
            / "native"
            / "containment"
            / "native-shadow-mac3-guest-console-evidence-protocol-arm64-v1.json"
        )
        self.assertEqual(
            hashlib.sha256(path.read_bytes()).hexdigest(),
            self.correction["corrects"]["sha256"],
        )

    def test_the_correction_changes_no_condition_and_grants_no_run(self) -> None:
        self.assertFalse(self.correction["changesAnyPassCondition"])
        self.assertFalse(self.correction["bootAuthorisation"]["grantedByThisRecord"])
        self.assertEqual(self.correction["bootAuthorisation"]["runsPerformed"], 0)
        self.assertFalse(self.correction["imageProductionAuthorisation"])
        self.assertFalse(self.correction["activationAllowed"])

    def test_the_correction_names_resolved_as_the_only_boolean_observation(self) -> None:
        row = self.correction["canonicalPrerequisiteRow"]
        self.assertEqual(row["exactKeys"], ["name", "resolved"])
        self.assertIn("boolean", row["resolved"])
        self.assertIn("present is not an alias", " ".join(self.correction["failClosedRules"]))


class BoundaryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = protocol()

    def test_no_record_claims_that_submissions_ran_unprivileged(self) -> None:
        self.assertNotIn("submission-account", self.module.RECORDS)
        self.assertIn("manufactured", self.module.WHY_SUBMISSIONS_ARE_NOT_CLAIMED)

    def test_the_module_says_why_a_console_line_cannot_assert_a_pass(self) -> None:
        self.assertIn("sealed", self.module.WHY_THE_GUEST_CANNOT_ASSERT_A_PASS)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        name = pathlib.Path(__file__).name
        text = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertTrue(name in text, "%s is not registered in self-test.sh" % name)


if __name__ == "__main__":
    unittest.main()
