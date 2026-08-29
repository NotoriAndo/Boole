#!/usr/bin/env python3
"""Pin the authority-zero v3 producer generation after its free rehearsal."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
FINGERPRINT_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-successor-producer-fingerprint-arm64-v5.json"
)
PREREGISTRATION_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
CORRECTION_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json"
)
REHEARSAL_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v1.json"
)

MARKER = "LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V5-SEALED"
FINGERPRINT_SHA256 = (
    "6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa"
)
FINGERPRINT_SIZE_BYTES = 5_458
GATE_PATH = (
    "scripts/test_native_shadow_successor_producer_fingerprint_arm64_v5.py"
)

EXPECTED_TOP_LEVEL_KEYS = {
    "authorisations",
    "bindingDirection",
    "boundaries",
    "files",
    "invariants",
    "predecessors",
    "rehearsalLineage",
    "runAccounting",
    "schema",
    "status",
    "subject",
    "uncreatedFuturePaths",
    "whatThisRecordDoesNotEstablish",
}

EXPECTED_AUTHORISATIONS = {
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

EXPECTED_INVARIANTS = {
    "BF.7": "HOLD",
    "LLM-MINEABLE-ELIGIBLE-V5": 14160,
    "REWARD_READY": 0,
    "RP0-MD": "HOLD",
    "activationAllowed": False,
    "baseActivation": False,
    "mineable_now": 0,
}

EXPECTED_FILES = [
    {
        "path": "scripts/native_shadow_successor_produce_phase_arm64_v3.py",
        "role": "authority-zero producer and rehearsal orchestrator",
        "sha256": "4633fe54c0d44b435b70742c027a83d1354293057787650bb7e91f4a901c46bd",
        "sizeBytes": 33103,
    },
    {
        "path": "scripts/test_native_shadow_successor_produce_phase_arm64_v3.py",
        "role": "producer contract gate",
        "sha256": "96420ee6eaf5a07159db37052f2fab1625b9ec1390ca314998369e598519fedf",
        "sizeBytes": 23845,
    },
    {
        "path": "scripts/native-shadow-successor-produce-arm64-v3.sh",
        "role": "authority-zero wrapper",
        "sha256": "b2c783f110bfb94f2e5d4115c4ac0699bb10c7593d8748ce2d3aff92a9b862a9",
        "sizeBytes": 9386,
    },
    {
        "path": ".github/workflows/native-shadow-successor-produce-arm64-v3.yml",
        "role": "dispatch workflow",
        "sha256": "faf89459d66a43e23efe4e70bb80d6b5747c57b2f8f3284d2550606ee7b1e9f6",
        "sizeBytes": 5457,
    },
    {
        "path": "scripts/test_native_shadow_successor_produce_workflow_arm64_v3.py",
        "role": "workflow contract gate",
        "sha256": "d6f4bdff30b30ac6b40910e6d29276cbf78ca7e0524313e0c7a320eb32718d51",
        "sizeBytes": 17499,
    },
    {
        "path": "scripts/native_shadow_successor_root_disk_readback_arm64_v3.py",
        "role": "readback-v3 consumer not executed by this rehearsal",
        "sha256": "1463dadb30bb55403b7604161839ac1164a42f56824255767a6c303a69437c4c",
        "sizeBytes": 44652,
    },
    {
        "path": "scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py",
        "role": "readback-v3 contract gate",
        "sha256": "689fa2d4dd880a0c34aa06bf9685cd60c6fb60b004de9e3dc3c2cdaa86040016",
        "sizeBytes": 44782,
    },
]

EXPECTED_PREDECESSORS = [
    {
        "path": (
            "native/containment/native-shadow-mac3-launcher-v2-successor-"
            "producer-preregistration-arm64-v1.json"
        ),
        "role": "pre-rehearsal preregistration",
        "sha256": "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
        "sizeBytes": 20145,
    },
    {
        "path": (
            "native/containment/native-shadow-mac3-launcher-v2-successor-"
            "producer-import-closure-correction-arm64-v1.json"
        ),
        "role": "append-only import-closure correction",
        "sha256": "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498",
        "sizeBytes": 10971,
    },
    {
        "path": (
            "native/containment/native-shadow-mac3-launcher-v2-successor-"
            "producer-rehearsal-result-arm64-v1.json"
        ),
        "role": "raw canonical free-rehearsal result",
        "sha256": "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c",
        "sizeBytes": 10168,
    },
]

EXPECTED_LINEAGE = {
    "artifactId": 9723056242,
    "artifactMemberCount": 1,
    "artifactMemberName": "REHEARSAL-RESULT.json",
    "artifactName": "launcher-v2-successor-v3-free-rehearsal",
    "headSha": "0649dbc92a228fb67350a7eef864a9c9c612fd3d",
    "jobId": 99176428509,
    "payloadPath": (
        "native/containment/native-shadow-mac3-launcher-v2-successor-"
        "producer-rehearsal-result-arm64-v1.json"
    ),
    "payloadSha256": "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c",
    "payloadSizeBytes": 10168,
    "runAttempt": 1,
    "runId": 33281151298,
    "status": "PASS-NO-IMAGE-PRODUCED",
    "workflow": ".github/workflows/native-shadow-successor-produce-arm64-v3.yml",
}

EXPECTED_BOUNDARIES = {
    "bootableClaim": False,
    "historicalAuthorityZeroStagingEvidenceOnly": True,
    "imageProduced": False,
    "mac4Started": False,
    "productionReadyClaim": False,
    "readbackV3ExecutedByRehearsal": False,
    "servingClaim": False,
}

EXPECTED_RUN_ACCOUNTING = {
    "bootsAllowed": 0,
    "bootsPerformed": 0,
    "freeRehearsalsObserved": 1,
    "imageProductionsAllowed": 0,
    "imageProductionsPerformed": 0,
}

EXPECTED_BINDING_DIRECTION = {
    "cyclicBindingForbidden": True,
    "direction": "historical-fingerprint-binds-predecessors-and-rehearsal-result-only",
    "futureAuthorityBytesBound": False,
    "futureProductionResultBytesBound": False,
    "selfDigestBound": False,
}

EXPECTED_FUTURE_PATHS = [
    "native/containment/native-shadow-mac3-successor-production-authority-arm64-v5.json",
    "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v5.json",
]

EXPECTED_NON_CLAIMS = [
    "that image-production authority exists",
    "that any image has been produced",
    "that any image boots or serves",
    "that readback-v3 ran in the free rehearsal",
    "that the v3 generation is production ready",
    "that MAC.4, testnet, mining, rewards, consensus or P2P have started",
]

DOCS = (
    "docs/mac-first-hidden-linux-execution-plan-v1.md",
    "docs/native-submission-shadow-verification-v1.md",
    "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
)


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def assert_live_regular_file(
    case: unittest.TestCase, row: dict[str, object]
) -> None:
    path = REPO / str(row["path"])
    info = path.lstat()
    case.assertTrue(stat.S_ISREG(info.st_mode), row["path"])
    case.assertFalse(path.is_symlink(), row["path"])
    case.assertEqual(sha256(path), row["sha256"], row["path"])
    case.assertEqual(info.st_size, row["sizeBytes"], row["path"])


def active_lines(path: pathlib.Path) -> list[str]:
    return [
        line
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]


class SuccessorProducerFingerprintArm64V5Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.record = load(FINGERPRINT_PATH)

    def test_record_is_canonical_historical_authority_zero_evidence(self) -> None:
        info = FINGERPRINT_PATH.lstat()
        self.assertTrue(stat.S_ISREG(info.st_mode))
        self.assertFalse(FINGERPRINT_PATH.is_symlink())
        self.assertEqual(info.st_size, FINGERPRINT_SIZE_BYTES)
        self.assertEqual(sha256(FINGERPRINT_PATH), FINGERPRINT_SHA256)
        self.assertEqual(set(self.record), EXPECTED_TOP_LEVEL_KEYS)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v5",
        )
        self.assertEqual(
            self.record["status"],
            "SEALED-AFTER-FREE-REHEARSAL-AUTHORITY-ZERO-HISTORICAL-EVIDENCE",
        )
        self.assertEqual(
            self.record["subject"],
            "Seal the exact authority-zero v3 generation and its one free rehearsal as historical staging evidence, not as a production-ready producer.",
        )
        canonical = (
            json.dumps(self.record, ensure_ascii=False, indent=2, sort_keys=True)
            + "\n"
        ).encode("utf-8")
        self.assertEqual(FINGERPRINT_PATH.read_bytes(), canonical)

    def test_exact_seven_generation_files_match_live_bytes_in_order(self) -> None:
        self.assertEqual(self.record["files"], EXPECTED_FILES)
        self.assertEqual(len(self.record["files"]), 7)
        self.assertEqual(
            len({row["path"] for row in self.record["files"]}), 7
        )
        for row in self.record["files"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(set(row), {"path", "role", "sha256", "sizeBytes"})
                self.assertIs(type(row["sizeBytes"]), int)
                assert_live_regular_file(self, row)

    def test_three_predecessors_are_exact_live_regular_files(self) -> None:
        self.assertEqual(self.record["predecessors"], EXPECTED_PREDECESSORS)
        for row in self.record["predecessors"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(set(row), {"path", "role", "sha256", "sizeBytes"})
                assert_live_regular_file(self, row)
        self.assertEqual(
            REPO / EXPECTED_PREDECESSORS[0]["path"], PREREGISTRATION_PATH
        )
        self.assertEqual(REPO / EXPECTED_PREDECESSORS[1]["path"], CORRECTION_PATH)
        self.assertEqual(REPO / EXPECTED_PREDECESSORS[2]["path"], REHEARSAL_PATH)

    def test_predecessors_preserve_exact_authority_zero_and_hold_values(self) -> None:
        preregistration = load(PREREGISTRATION_PATH)
        correction = load(CORRECTION_PATH)
        self.assertEqual(self.record["authorisations"], EXPECTED_AUTHORISATIONS)
        self.assertEqual(self.record["authorisations"], preregistration["authorisations"])
        self.assertEqual(self.record["authorisations"], correction["authorisations"])
        self.assertEqual(self.record["invariants"], EXPECTED_INVARIANTS)
        self.assertEqual(self.record["invariants"], preregistration["invariants"])
        for key, value in self.record["authorisations"].items():
            if key == "imageProductionRunsAllowed":
                self.assertIs(type(value), int, key)
                self.assertEqual(value, 0, key)
            else:
                self.assertIs(type(value), bool, key)
                self.assertIs(value, False, key)

    def test_rehearsal_lineage_is_exact_and_payload_is_raw_canonical_result(self) -> None:
        self.assertEqual(self.record["rehearsalLineage"], EXPECTED_LINEAGE)
        for key in (
            "artifactId",
            "artifactMemberCount",
            "jobId",
            "payloadSizeBytes",
            "runAttempt",
            "runId",
        ):
            self.assertIs(type(self.record["rehearsalLineage"][key]), int, key)
        rehearsal = load(REHEARSAL_PATH)
        self.assertEqual(
            set(rehearsal),
            {
                "activationAllowed",
                "authorisations",
                "bootableClaim",
                "boundInputs",
                "effects",
                "imageProduced",
                "importClosureCorrectionSha256",
                "measurement",
                "preregistrationSha256",
                "repeatable",
                "schema",
                "status",
            },
        )
        self.assertEqual(
            rehearsal["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal.arm64.v1",
        )
        self.assertEqual(rehearsal["status"], "PASS-NO-IMAGE-PRODUCED")
        self.assertEqual(sha256(REHEARSAL_PATH), EXPECTED_LINEAGE["payloadSha256"])
        self.assertEqual(REHEARSAL_PATH.stat().st_size, EXPECTED_LINEAGE["payloadSizeBytes"])
        canonical = (
            json.dumps(rehearsal, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        ).encode("utf-8")
        self.assertEqual(REHEARSAL_PATH.read_bytes(), canonical)

    def test_rehearsal_effects_and_run_accounting_remain_exactly_zero(self) -> None:
        rehearsal = load(REHEARSAL_PATH)
        self.assertIs(rehearsal["repeatable"], True)
        self.assertIs(rehearsal["imageProduced"], False)
        self.assertIs(rehearsal["bootableClaim"], False)
        self.assertIs(rehearsal["activationAllowed"], False)
        effects = rehearsal["effects"]
        self.assertEqual(effects["allowedArtifact"], "one canonical JSON result only")
        self.assertEqual(effects["allowedImageTools"], [])
        self.assertEqual(
            effects["forbiddenOutputNames"],
            [
                "ATTEMPT-CONSUMED.json",
                "guest-kernel",
                "guest-initrd",
                "guest-root-disk",
            ],
        )
        self.assertEqual(effects["artifactMemberCount"], 1)
        for key in (
            "attemptMarkersCreated",
            "imageEffectCalls",
            "imageFilesCreated",
            "productionOutputDirectoriesCreated",
            "productionOutputsCreated",
        ):
            self.assertIs(type(effects[key]), int, key)
            self.assertEqual(effects[key], 0, key)
        self.assertEqual(
            effects["scratchSnapshotSha256"],
            "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570",
        )
        self.assertIs(effects["scratchTreeUnchanged"], True)
        self.assertEqual(self.record["runAccounting"], EXPECTED_RUN_ACCOUNTING)
        for key, value in self.record["runAccounting"].items():
            self.assertIs(type(value), int, key)
            self.assertEqual(value, 1 if key == "freeRehearsalsObserved" else 0, key)

    def test_boundaries_refuse_every_production_boot_serving_and_mac4_claim(self) -> None:
        self.assertEqual(self.record["boundaries"], EXPECTED_BOUNDARIES)
        self.assertIs(
            self.record["boundaries"]["historicalAuthorityZeroStagingEvidenceOnly"],
            True,
        )
        for key, value in self.record["boundaries"].items():
            if key != "historicalAuthorityZeroStagingEvidenceOnly":
                self.assertIs(value, False, key)
        self.assertEqual(
            self.record["whatThisRecordDoesNotEstablish"], EXPECTED_NON_CLAIMS
        )
        self.assertIn(
            "that the v3 generation is production ready",
            self.record["whatThisRecordDoesNotEstablish"],
        )

    def test_binding_direction_is_one_way_and_has_no_self_or_future_digest(self) -> None:
        self.assertEqual(self.record["bindingDirection"], EXPECTED_BINDING_DIRECTION)
        self.assertIs(self.record["bindingDirection"]["cyclicBindingForbidden"], True)
        for key in (
            "futureAuthorityBytesBound",
            "futureProductionResultBytesBound",
            "selfDigestBound",
        ):
            self.assertIs(self.record["bindingDirection"][key], False, key)
        forbidden_keys = {
            "authoritySha256",
            "futureAuthoritySha256",
            "futureProductionResultSha256",
            "productionResultSha256",
            "selfSha256",
        }

        def visit(value: object) -> None:
            if isinstance(value, dict):
                self.assertFalse(forbidden_keys & set(value))
                for nested in value.values():
                    visit(nested)
            elif isinstance(value, list):
                for nested in value:
                    visit(nested)

        visit(self.record)

    def test_future_authority_and_result_paths_are_named_once_but_do_not_exist(self) -> None:
        self.assertEqual(self.record["uncreatedFuturePaths"], EXPECTED_FUTURE_PATHS)
        encoded = json.dumps(self.record, ensure_ascii=False, sort_keys=True)
        for relative in EXPECTED_FUTURE_PATHS:
            with self.subTest(path=relative):
                self.assertEqual(encoded.count(relative), 1)
                self.assertFalse(os.path.lexists(REPO / relative))

    def test_all_three_documents_and_both_permanent_gates_are_active(self) -> None:
        for relative in DOCS:
            with self.subTest(document=relative):
                text = (REPO / relative).read_text(encoding="utf-8")
                self.assertEqual(text.count(MARKER), 1)
        self_test_lines = active_lines(REPO / "scripts/self-test.sh")
        docs_smoke_lines = active_lines(REPO / "scripts/docs-smoke.sh")
        self.assertEqual(sum(GATE_PATH in line for line in self_test_lines), 1)
        self.assertEqual(
            sum(line == f"require_file {GATE_PATH}" for line in docs_smoke_lines),
            1,
        )
        self.assertEqual(sum(MARKER in line for line in docs_smoke_lines), 1)


if __name__ == "__main__":
    unittest.main()
