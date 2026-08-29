"""Cross-language host contract for launcher-v2 console observations."""

import importlib
import hashlib
import json
import pathlib
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = (
    REPO
    / "native"
    / "containment"
    / "native-shadow-launcher-v2-console-evidence-example.txt"
)
RECORD = (
    REPO
    / "native"
    / "containment"
    / "native-shadow-launcher-v2-console-evidence-protocol-arm64-v1.json"
)


def protocol():
    return importlib.import_module(
        "scripts.native_shadow_mac3_guest_evidence_protocol_arm64_v2"
    )


class ExactSchemaTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = protocol()
        self.read = self.module.read_transcript(FIXTURE.read_text(encoding="utf-8"))

    def test_shared_rust_producer_fixture_passes_each_guest_observation(self) -> None:
        self.assertEqual(self.read["missing"], [])
        self.assertTrue(
            self.module.launcher_executable_matches(
                self.read,
                expected_path="/usr/libexec/boole/boole-native-shadow-launcher",
                expected_sha256="0" * 64,
            )[0]
        )
        self.assertTrue(self.module.prerequisites_match(self.read)[0])
        self.assertTrue(self.module.supervisor_matches(self.read)[0])
        self.assertTrue(self.module.readiness_matches(self.read)[0])

    def test_prerequisite_names_values_and_order_are_all_exact(self) -> None:
        base = [
            {"name": name, "resolved": True}
            for name in self.module.EXACT_PREREQUISITES
        ]
        variants = [
            base[:-1],
            base + [{"name": "invented", "resolved": True}],
            list(reversed(base)),
            [dict(row) for row in base],
        ]
        variants[-1][0]["resolved"] = False
        for rows in variants:
            with self.subTest(rows=rows):
                line = self.module.format_record(
                    "launcher-prerequisites", {"prerequisites": rows}
                )
                read = self.module.read_transcript(line)
                self.assertFalse(self.module.prerequisites_match(read)[0])

    def test_boolean_prerequisite_fields_do_not_accept_integer_aliases(self) -> None:
        rows = [
            {"name": name, "resolved": True}
            for name in self.module.EXACT_PREREQUISITES
        ]
        rows[0]["resolved"] = 1
        read = self.module.read_transcript(
            self.module.format_record(
                "launcher-prerequisites", {"prerequisites": rows}
            )
        )
        self.assertFalse(self.module.prerequisites_match(read)[0])

    def test_present_only_schema_is_refused(self) -> None:
        rows = [
            {"name": name, "present": True}
            for name in self.module.EXACT_PREREQUISITES
        ]
        read = self.module.read_transcript(
            self.module.format_record(
                "launcher-prerequisites", {"prerequisites": rows}
            )
        )
        met, why = self.module.prerequisites_match(read)
        self.assertFalse(met)
        self.assertIn("resolved", why)

    def test_each_supervisor_field_is_compared_not_just_uid(self) -> None:
        record = dict(self.read["records"]["supervisor-privilege"])
        mutations = {
            "uids": [0, 0, 0, 1],
            "gids": [0, 0, 0, 1],
            "capabilitiesInheritable": "0000000000000001",
            "capabilitiesPermitted": "0000000000000000",
            "capabilitiesEffective": "0000000000000000",
            "capabilitiesBounding": "0000000000000000",
            "capabilitiesAmbient": "0000000000000001",
            "noNewPrivileges": 1,
        }
        for field, changed in mutations.items():
            with self.subTest(field=field):
                payload = dict(record)
                payload[field] = changed
                read = self.module.read_transcript(
                    self.module.format_record("supervisor-privilege", payload)
                )
                self.assertFalse(self.module.supervisor_matches(read)[0])

    def test_supervisor_integer_fields_do_not_accept_boolean_aliases(self) -> None:
        record = dict(self.read["records"]["supervisor-privilege"])
        mutations = {
            "uids": [False, 0, 0, 0],
            "gids": [0, 0, 0, False],
            "noNewPrivileges": False,
        }
        for field, changed in mutations.items():
            with self.subTest(field=field):
                payload = dict(record)
                payload[field] = changed
                read = self.module.read_transcript(
                    self.module.format_record("supervisor-privilege", payload)
                )
                self.assertFalse(self.module.supervisor_matches(read)[0])

    def test_any_malformed_prefixed_line_invalidates_every_matcher(self) -> None:
        transcript = FIXTURE.read_text(encoding="utf-8")
        changed = transcript + "\nBOOLE-GUEST-EVIDENCE-1 broken\n"
        read = self.module.read_transcript(changed)
        matchers = [
            lambda: self.module.launcher_executable_matches(
                read,
                expected_path="/usr/libexec/boole/boole-native-shadow-launcher",
                expected_sha256="0" * 64,
            ),
            lambda: self.module.prerequisites_match(read),
            lambda: self.module.supervisor_matches(read),
            lambda: self.module.readiness_matches(read),
        ]
        self.assertTrue(read["malformed"])
        self.assertTrue(all(not matcher()[0] for matcher in matchers))

    def test_unknown_record_ids_remain_counted_and_ignored_by_the_v1_framing_contract(self) -> None:
        transcript = FIXTURE.read_text(encoding="utf-8")
        changed = (
            transcript
            + "\nBOOLE-GUEST-EVIDENCE-1 invented-record {\"value\":true}\n"
        )
        read = self.module.read_transcript(changed)
        self.assertEqual(read["unknownRecordIds"], ["invented-record"])
        self.assertTrue(self.module.prerequisites_match(read)[0])

    def test_a_guest_record_never_observes_a_submission(self) -> None:
        self.assertFalse(self.read["submissionsObserved"])


class SealedSuccessorRecordTests(unittest.TestCase):
    def setUp(self) -> None:
        self.record = json.loads(RECORD.read_text(encoding="utf-8"))

    def test_record_pins_the_exact_prerequisite_sequence_and_fixture(self) -> None:
        module = protocol()
        self.assertEqual(self.record["prerequisites"], list(module.EXACT_PREREQUISITES))
        self.assertEqual(self.record["supervisorExpected"], module.EXACT_SUPERVISOR)
        self.assertEqual(
            self.record["sharedFixture"]["path"],
            FIXTURE.relative_to(REPO).as_posix(),
        )
        self.assertEqual(
            self.record["sharedFixture"]["sha256"],
            hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
        )

    def test_record_grants_no_image_boot_or_activation(self) -> None:
        self.assertFalse(self.record["imageProductionAuthorisation"])
        self.assertFalse(self.record["bootAuthorisation"]["grantedByThisRecord"])
        self.assertEqual(self.record["bootAuthorisation"]["runsPerformed"], 0)
        self.assertFalse(self.record["activationAllowed"])

    def test_record_keeps_submission_execution_unobserved(self) -> None:
        self.assertFalse(self.record["submissionsObserved"])
        self.assertFalse(self.record["conditionFourFullySettledByThisRecord"])


if __name__ == "__main__":
    unittest.main()
