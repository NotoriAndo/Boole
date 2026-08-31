#!/usr/bin/env python3
"""Seal F7 after fresh R3 while granting no production or boot authority."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import stat
import unittest
from unittest import mock

from scripts import native_shadow_successor_produce_phase_arm64_v5 as producer_v5


REPO = pathlib.Path(__file__).resolve().parents[1]
F7_PATH = REPO / (
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-"
    "arm64-v7.json"
)
P4_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-main-branch-"
    "dispatch-fence-correction-arm64-v1.json"
)
R3_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v3.json"
)
R3_GATE_PATH = REPO / (
    "scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_"
    "result_arm64_v3.py"
)
PROVENANCE_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-artifact-provenance-arm64-v3.json"
)
PROVENANCE_GATE_PATH = REPO / (
    "scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_"
    "artifact_provenance_arm64_v3.py"
)
A7_PATH = REPO / (
    "native/containment/native-shadow-mac3-successor-production-authority-"
    "arm64-v7.json"
)
RESULT_V7_PATH = REPO / (
    "native/containment/native-shadow-mac3-successor-image-production-result-"
    "arm64-v7.json"
)
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOCS = (
    REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md",
    REPO / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
    REPO / "docs/native-submission-shadow-verification-v1.md",
)

P4_SHA256 = "63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b"
P4_SIZE_BYTES = 13_335
R3_SHA256 = "44cd7d6feea2efc62d9ab6cb809e5d66c1452c9e4d2f034fd800e6573938fe87"
R3_SIZE_BYTES = 6_012
R3_GATE_SHA256 = "1afe90a8256ec5205ef6d692f0f85989fce2efff1cb435bbb92af19f42b8c5e4"
R3_GATE_SIZE_BYTES = 8_092
F7_SHA256 = "3839d92c189a4a56d1d6a79a7fbfb2deaaadcf3dfaec3e636385c96aa106348c"
F7_SIZE_BYTES = 2_798
SECTION_TOKEN = "LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V7-SEALED"

EXPECTED_KEYS = {
    "authorisations",
    "boundaries",
    "files",
    "mainBranchDispatchFenceCorrection",
    "predecessors",
    "rehearsalGate",
    "schema",
    "status",
    "subject",
    "whatThisRecordDoesNotEstablish",
}
ZERO_AUTHORISATIONS = {
    "bootAuthorised": False,
    "consensusActivated": False,
    "imageProductionAuthorised": False,
    "imageProductionRunsAllowed": 0,
    "mac4Started": False,
    "miningActivated": False,
    "p2pActivated": False,
    "rewardActivated": False,
    "testnetStarted": False,
}
ZERO_BOUNDARIES = {
    "activationAllowed": False,
    "bootableClaim": False,
    "servingClaim": False,
}
GENERATION_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v5.py",
    "scripts/test_native_shadow_successor_produce_phase_arm64_v5.py",
    "scripts/native-shadow-successor-produce-arm64-v5.sh",
    ".github/workflows/native-shadow-successor-produce-arm64-v5.yml",
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v5.py",
)


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def read_regular(path: pathlib.Path) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise AssertionError(f"{path.relative_to(REPO)} is not regular")
    return path.read_bytes()


def live_identity(relative: str) -> dict[str, object]:
    raw = read_regular(REPO / relative)
    return {"path": relative, "sha256": sha256_bytes(raw), "sizeBytes": len(raw)}


def assert_strict_equal(actual: object, expected: object, path: str = "$") -> None:
    if type(actual) is not type(expected):
        raise AssertionError(f"{path} type differs")
    if isinstance(expected, dict):
        if set(actual) != set(expected):
            raise AssertionError(f"{path} keys differ")
        for key in expected:
            assert_strict_equal(actual[key], expected[key], f"{path}.{key}")
        return
    if isinstance(expected, list):
        if len(actual) != len(expected):
            raise AssertionError(f"{path} length differs")
        for index, (actual_item, expected_item) in enumerate(zip(actual, expected)):
            assert_strict_equal(actual_item, expected_item, f"{path}[{index}]")
        return
    if actual != expected:
        raise AssertionError(f"{path} value differs: {actual!r} != {expected!r}")


class SuccessorProducerFingerprintV7Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(F7_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))

    def test_f7_is_exact_canonical_and_narrow(self) -> None:
        self.assertEqual(len(self.raw), F7_SIZE_BYTES)
        self.assertEqual(sha256_bytes(self.raw), F7_SHA256)
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(set(self.record), EXPECTED_KEYS)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v7",
        )
        self.assertEqual(
            self.record["status"],
            "SEALED-AFTER-FRESH-R3-PRODUCTION-GENERATION-NOT-AUTHORISED",
        )
        self.assertEqual(
            self.record["subject"],
            "Pin the exact main-only v5 production generation after fresh R3.",
        )
        self.assertEqual(
            self.record["whatThisRecordDoesNotEstablish"],
            ["image production authority", "guest boot authority"],
        )

    def test_authority_and_runtime_boundaries_are_strictly_zero(self) -> None:
        assert_strict_equal(self.record["authorisations"], ZERO_AUTHORISATIONS)
        assert_strict_equal(self.record["boundaries"], ZERO_BOUNDARIES)

    def test_predecessors_are_exact_p4_then_raw_r3_live_bytes(self) -> None:
        p4 = live_identity(P4_PATH.relative_to(REPO).as_posix())
        r3 = live_identity(R3_PATH.relative_to(REPO).as_posix())
        self.assertEqual(
            (p4["sha256"], p4["sizeBytes"]), (P4_SHA256, P4_SIZE_BYTES)
        )
        self.assertEqual(
            (r3["sha256"], r3["sizeBytes"]), (R3_SHA256, R3_SIZE_BYTES)
        )
        self.assertEqual(self.record["predecessors"], [p4, r3])
        self.assertEqual(self.record["mainBranchDispatchFenceCorrection"], p4)

    def test_files_are_exactly_the_fresh_r3_v5_generation(self) -> None:
        r3 = json.loads(read_regular(R3_PATH).decode("utf-8"))
        expected = [live_identity(path) for path in GENERATION_PATHS]
        self.assertEqual(r3["generationFiles"], expected)
        self.assertEqual(self.record["files"], expected)
        self.assertEqual(len({row["path"] for row in expected}), 5)

    def test_rehearsal_gate_is_a_direct_live_binding(self) -> None:
        gate = live_identity(R3_GATE_PATH.relative_to(REPO).as_posix())
        self.assertEqual(
            (gate["sha256"], gate["sizeBytes"]),
            (R3_GATE_SHA256, R3_GATE_SIZE_BYTES),
        )
        self.assertEqual(self.record["rehearsalGate"], gate)

    def test_transport_and_future_authority_cannot_flow_into_f7(self) -> None:
        encoded = self.raw.decode("utf-8")
        for forbidden_path in (
            PROVENANCE_PATH,
            PROVENANCE_GATE_PATH,
            A7_PATH,
            RESULT_V7_PATH,
        ):
            self.assertNotIn(forbidden_path.relative_to(REPO).as_posix(), encoded)
        for forbidden_key in (
            '"artifactId"',
            '"productionAuthoritySha256"',
            '"productionResultSha256"',
            '"selfSha256"',
        ):
            self.assertNotIn(forbidden_key, encoded)

    def test_historical_f7_gate_does_not_require_future_files_to_remain_absent(
        self,
    ) -> None:
        source = pathlib.Path(__file__).read_text(encoding="utf-8")
        self.assertNotIn("self.assertFalse(A7_PATH." + "exists())", source)
        self.assertNotIn("self.assertFalse(RESULT_V7_PATH." + "exists())", source)

    def test_live_v5_consumer_accepts_f7_before_requesting_a7(self) -> None:
        expected = (
            "required binding is absent: "
            + A7_PATH.relative_to(REPO).as_posix()
        )
        real_load = producer_v5._load_canonical
        real_require_absent = producer_v5._require_absent

        def permit_future_result_only(
            root: pathlib.Path, relative: str, context: str
        ) -> None:
            if relative == producer_v5.RESULT_V7_PATH:
                return
            real_require_absent(root, relative, context)

        def absent_a7(
            root: pathlib.Path, relative: str
        ) -> tuple[object, dict[str, object]]:
            if relative == producer_v5.A7_PATH:
                raise producer_v5.SuccessorProduceV5Error(expected)
            return real_load(root, relative)

        with (
            mock.patch.object(
                producer_v5,
                "_require_absent",
                side_effect=permit_future_result_only,
            ),
            mock.patch.object(producer_v5, "_load_canonical", side_effect=absent_a7),
            self.assertRaisesRegex(
                producer_v5.SuccessorProduceV5Error, re.escape(expected)
            ),
        ):
            producer_v5.verify_generation_chain(REPO)

    def test_gate_and_authority_docs_register_the_seal(self) -> None:
        gate_path = pathlib.Path(__file__).resolve().relative_to(REPO).as_posix()
        self_test = SELF_TEST_PATH.read_text(encoding="utf-8")
        self.assertEqual(self_test.count(gate_path), 1)
        self.assertIn(
            "run_logged native-shadow-successor-producer-f7 "
            f"python3 -m unittest {gate_path}",
            self_test,
        )
        for doc in DOCS:
            text = doc.read_text(encoding="utf-8")
            self.assertEqual(text.count(SECTION_TOKEN), 1, doc.name)
            self.assertIn(F7_SHA256, text, doc.name)
            self.assertIn("2,798-byte", text, doc.name)
            self.assertIn(
                "R3 GREEN / F7 SEALED / A7 NOT CREATED / PRODUCTION AND BOOT NOT RUN",
                text,
            )


if __name__ == "__main__":
    unittest.main()
