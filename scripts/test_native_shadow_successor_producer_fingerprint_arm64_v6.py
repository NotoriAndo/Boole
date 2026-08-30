#!/usr/bin/env python3
"""Seal F6 after fresh R2 while granting no production or boot authority."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
F6_PATH = REPO / (
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-"
    "arm64-v6.json"
)
R2_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v2.json"
)
P3_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-production-"
    "dispatch-fence-correction-arm64-v1.json"
)
R2_GATE_PATH = REPO / (
    "scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_"
    "result_arm64_v2.py"
)
PROVENANCE_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-artifact-provenance-arm64-v2.json"
)
PROVENANCE_GATE_PATH = REPO / (
    "scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_"
    "artifact_provenance_arm64_v2.py"
)
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOCS = (
    REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md",
    REPO / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
    REPO / "docs/native-submission-shadow-verification-v1.md",
)

F6_SHA256 = "0e98b02f2dc8c4752c282dba57e1aa39d1cdc62a83c57d8803d6051ea792c183"
F6_SIZE_BYTES = 3_250
R2_GATE_SHA256 = "f8787ed7f944aec6a66ba96228ff3c20a666ab986e90ce2accf060889029fbfc"
R2_GATE_SIZE_BYTES = 9_967
SECTION_TOKEN = "LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V6-SEALED"

EXPECTED_KEYS = {
    "authorisations",
    "boundaries",
    "files",
    "predecessors",
    "productionDispatchFenceCorrection",
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
PREDECESSORS = (
    (
        "native/containment/native-shadow-mac3-launcher-v2-successor-"
        "production-generation-preregistration-arm64-v1.json",
        "4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95",
        8_156,
    ),
    (
        "native/containment/native-shadow-mac3-launcher-v2-successor-"
        "producer-rehearsal-result-arm64-v1.json",
        "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c",
        10_168,
    ),
    (
        "native/containment/native-shadow-mac3-successor-producer-"
        "fingerprint-arm64-v5.json",
        "6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa",
        5_458,
    ),
    (
        "native/containment/native-shadow-mac3-launcher-v2-successor-"
        "producer-rehearsal-result-arm64-v2.json",
        "7efe89c3bc558455313b76de2a625e708a580d0256760692914e9474eb0171f0",
        6_928,
    ),
)
GENERATION_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v4.py",
    "scripts/test_native_shadow_successor_produce_phase_arm64_v4.py",
    "scripts/native-shadow-successor-produce-arm64-v4.sh",
    ".github/workflows/native-shadow-successor-produce-arm64-v4.yml",
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py",
)


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
    ).encode("utf-8")


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
        raise AssertionError(
            f"{path} type differs: {type(actual).__name__} != "
            f"{type(expected).__name__}"
        )
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


class SuccessorProducerFingerprintV6Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(F6_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))
        self.r2_raw = read_regular(R2_PATH)
        self.r2 = json.loads(self.r2_raw.decode("utf-8"))

    def test_f6_is_exact_canonical_and_narrow(self) -> None:
        self.assertEqual(len(self.raw), F6_SIZE_BYTES)
        self.assertEqual(sha256_bytes(self.raw), F6_SHA256)
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(set(self.record), EXPECTED_KEYS)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v6",
        )
        self.assertEqual(
            self.record["status"],
            "SEALED-AFTER-FRESH-R2-PRODUCTION-GENERATION-NOT-AUTHORISED",
        )
        self.assertEqual(
            self.record["subject"],
            "Pin the exact production-only v4 generation after fresh R2.",
        )
        self.assertEqual(
            self.record["whatThisRecordDoesNotEstablish"],
            ["image production authority", "guest boot authority"],
        )

    def test_authority_and_runtime_boundaries_are_strictly_zero(self) -> None:
        assert_strict_equal(self.record["authorisations"], ZERO_AUTHORISATIONS)
        assert_strict_equal(self.record["boundaries"], ZERO_BOUNDARIES)

    def test_predecessors_are_exact_live_bytes_in_frozen_order(self) -> None:
        expected = []
        for relative, digest, size in PREDECESSORS:
            identity = live_identity(relative)
            self.assertEqual(identity["sha256"], digest, relative)
            self.assertEqual(identity["sizeBytes"], size, relative)
            expected.append(identity)
        self.assertEqual(self.record["predecessors"], expected)

    def test_files_are_exactly_the_fresh_r2_generation_and_live_bytes(self) -> None:
        expected = [live_identity(path) for path in GENERATION_PATHS]
        self.assertEqual(self.r2["generationFiles"], expected)
        self.assertEqual(self.record["files"], expected)
        self.assertEqual(len(expected), 5)
        self.assertEqual(len({row["path"] for row in expected}), 5)

    def test_dispatch_correction_and_rehearsal_gate_are_direct_live_bindings(self) -> None:
        correction = live_identity(P3_PATH.relative_to(REPO).as_posix())
        self.assertEqual(
            correction,
            {
                "path": P3_PATH.relative_to(REPO).as_posix(),
                "sha256": (
                    "16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b"
                ),
                "sizeBytes": 7_295,
            },
        )
        self.assertEqual(self.record["productionDispatchFenceCorrection"], correction)
        gate = live_identity(R2_GATE_PATH.relative_to(REPO).as_posix())
        self.assertEqual(gate["sha256"], R2_GATE_SHA256)
        self.assertEqual(gate["sizeBytes"], R2_GATE_SIZE_BYTES)
        self.assertEqual(self.record["rehearsalGate"], gate)

    def test_transport_provenance_and_future_authority_cannot_flow_into_f6(self) -> None:
        encoded = self.raw.decode("utf-8")
        for forbidden_path in (
            PROVENANCE_PATH,
            PROVENANCE_GATE_PATH,
            F6_PATH,
            REPO
            / "native/containment/native-shadow-mac3-successor-production-"
            "authority-arm64-v6.json",
            REPO
            / "native/containment/native-shadow-mac3-successor-image-"
            "production-result-arm64-v6.json",
        ):
            self.assertNotIn(forbidden_path.relative_to(REPO).as_posix(), encoded)
        for forbidden_key in (
            '"gateSha256"',
            '"selfSha256"',
            '"productionAuthoritySha256"',
            '"productionResultSha256"',
        ):
            self.assertNotIn(forbidden_key, encoded)

    def test_gate_and_three_authority_docs_register_the_seal(self) -> None:
        gate_path = pathlib.Path(__file__).resolve().relative_to(REPO).as_posix()
        self_test = SELF_TEST_PATH.read_text(encoding="utf-8")
        self.assertEqual(self_test.count(gate_path), 1)
        for doc in DOCS:
            text = doc.read_text(encoding="utf-8")
            self.assertEqual(text.count(SECTION_TOKEN), 1, doc.name)
            self.assertIn(
                "R2 GREEN / F6 SEALED / A6 NOT CREATED / PRODUCTION AND BOOT NOT RUN",
                text,
            )


if __name__ == "__main__":
    unittest.main()
