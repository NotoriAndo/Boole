#!/usr/bin/env python3
"""Freeze the production-only successor generation at authority zero."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-"
    "production-generation-preregistration-arm64-v1.json"
)
R1_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v1.json"
)
F5_PATH = REPO / (
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-"
    "arm64-v5.json"
)
P1_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
C1_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json"
)

MARKER = "LAUNCHER-V2-SUCCESSOR-PRODUCTION-GENERATION-PREREGISTRATION-ARM64-V1-FROZEN"
GATE = (
    "scripts/test_native_shadow_launcher_v2_successor_production_generation_"
    "preregistration_arm64_v1.py"
)
RECORD_SHA256 = "4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95"
RECORD_SIZE_BYTES = 8_156

TOP_KEYS = {
    "authorisations",
    "bindings",
    "dag",
    "futureGeneration",
    "hardStopConditions",
    "invariants",
    "predecessorDisposition",
    "runs",
    "schema",
    "status",
    "subject",
    "unusedReservedPaths",
    "versionNamespaces",
    "whatThisRecordDoesNotEstablish",
}

AUTHORISATIONS = {
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
RUNS = {
    "bootsAllowed": 0,
    "bootsPerformed": 0,
    "freeRehearsalsPerformedByThisRecord": 0,
    "imageProductionsAllowed": 0,
    "imageProductionsPerformed": 0,
}
INVARIANTS = {
    "BF.7": "HOLD",
    "LLM-MINEABLE-ELIGIBLE-V5": 14160,
    "REWARD_READY": 0,
    "RP0-MD": "HOLD",
    "activationAllowed": False,
    "baseActivation": False,
    "mineable_now": 0,
}

BINDINGS = [
    {
        "path": R1_PATH.relative_to(REPO).as_posix(),
        "role": "raw canonical authority-zero v3 rehearsal result",
        "sha256": "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c",
        "sizeBytes": 10168,
    },
    {
        "path": F5_PATH.relative_to(REPO).as_posix(),
        "role": "historical v3 authority-zero rehearsal fingerprint",
        "sha256": "6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa",
        "sizeBytes": 5458,
    },
]

UNUSED_RESERVED_PATHS = [
    {
        "authorityEverGranted": False,
        "createdAtFreeze": False,
        "path": "native/containment/native-shadow-mac3-successor-production-authority-arm64-v5.json",
        "requiredAbsent": True,
        "reuseForbidden": True,
    },
    {
        "authorityEverGranted": False,
        "createdAtFreeze": False,
        "path": "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v5.json",
        "requiredAbsent": True,
        "reuseForbidden": True,
    },
]

NEW_FILES = [
    {
        "path": "scripts/native_shadow_successor_produce_phase_arm64_v4.py",
        "role": "production-generation authority adapter and orchestrator",
    },
    {
        "path": "scripts/test_native_shadow_successor_produce_phase_arm64_v4.py",
        "role": "production-generation producer contract gate",
    },
    {
        "path": "scripts/native-shadow-successor-produce-arm64-v4.sh",
        "role": "production-generation wrapper",
    },
    {
        "path": ".github/workflows/native-shadow-successor-produce-arm64-v4.yml",
        "role": "production-generation manual workflow",
    },
    {
        "path": "scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py",
        "role": "production-generation workflow contract gate",
    },
]

REUSED_UPSTREAM = [
    {
        "path": "scripts/native_shadow_successor_produce_phase_arm64_v3.py",
        "role": "shared prepare_staging implementation proved by R1 and F5",
        "sha256": "4633fe54c0d44b435b70742c027a83d1354293057787650bb7e91f4a901c46bd",
        "sizeBytes": 33103,
    },
    {
        "path": "scripts/native_shadow_successor_root_disk_readback_arm64_v3.py",
        "role": "open-descriptor readback implementation",
        "sha256": "1463dadb30bb55403b7604161839ac1164a42f56824255767a6c303a69437c4c",
        "sizeBytes": 44652,
    },
    {
        "path": "scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py",
        "role": "readback-v3 security contract gate",
        "sha256": "689fa2d4dd880a0c34aa06bf9685cd60c6fb60b004de9e3dc3c2cdaa86040016",
        "sizeBytes": 44782,
    },
]

RECORD_PATHS = {
    "authority": "native/containment/native-shadow-mac3-successor-production-authority-arm64-v6.json",
    "fingerprint": "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v6.json",
    "productionResult": "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v6.json",
    "rehearsalGate": "scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v2.py",
    "rehearsalResult": "native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v2.json",
}

DAG = {
    "cycleAllowed": False,
    "edgeMeaning": "binder-to-bound-predecessor",
    "edges": [
        {"binder": "P2", "binds": ["R1", "F5"]},
        {"binder": "R2", "binds": ["P2", "R1", "F5", "V4"]},
        {"binder": "F6", "binds": ["P2", "R1", "F5", "R2", "V4"]},
        {"binder": "A6", "binds": ["P2", "R1", "F5", "R2", "F6"]},
        {
            "binder": "RESULT-V6",
            "binds": ["P2", "R1", "F5", "R2", "F6", "A6"],
        },
    ],
    "forbiddenReverseDigestBindings": [
        "P2-to-R2-F6-A6-RESULT-V6",
        "F6-to-A6-RESULT-V6",
        "V4-to-F6-A6-RESULT-V6-digests",
        "A6-to-RESULT-V6-digest",
    ],
}

HARD_STOPS = [
    "R1 or F5 digest, size, canonical shape or regular-file identity differs",
    "R1 reports anything except one canonical PASS-NO-IMAGE-PRODUCED artifact with zero image, marker and production-output effects",
    "F5 does not bind the exact v3 seven-file generation plus P1, C1 and R1",
    "an unused authority-v5 or result-v5 reserved path exists as a file, directory or symbolic link",
    "historical v3 files, R1 or F5 are rewritten, re-sealed or reused as production authority",
    "a production-generation v4 path reuses or aliases a historical generation path",
    "F6 or A6 is created before a fresh R2 proves the exact v4 executable bytes without image, marker or production output",
    "R2 executes a path different from the v4 path later bound by F6",
    "F6 omits P2, R1, F5, R2 or any exact v4 implementation identity",
    "a reverse digest edge or self digest creates a P2, R2, F6, A6 or result-v6 cycle",
    "the authority chain is checked after dependency acquisition, scratch, output-directory, attempt-marker, assembly or image effects",
    "CLI, environment or image-provided values override authority, fingerprint, source-lock or readback bindings",
    "the production generation falls back to v3 or older producer, wrapper, workflow or launcher-v1 paths, or to any readback path other than the explicitly pinned readback-v3",
    "open-file identity, read-only mount, cleanup hard stop, post-write revalidation or create-once readback promotion is weakened",
    "production, boot or MAC.4 is run without a separately reviewed A6 that grants exactly one named run",
    "testnet, mining, reward, consensus or P2P authority is opened by this preregistration",
]


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def assert_live(case: unittest.TestCase, row: dict[str, object]) -> None:
    path = REPO / str(row["path"])
    info = path.lstat()
    case.assertTrue(stat.S_ISREG(info.st_mode), row["path"])
    case.assertFalse(path.is_symlink(), row["path"])
    case.assertEqual(info.st_size, row["sizeBytes"], row["path"])
    case.assertEqual(digest(path), row["sha256"], row["path"])


class ProductionGenerationPreregistrationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = RECORD_PATH.read_bytes()
        self.record = json.loads(self.raw)

    def test_record_is_exact_canonical_authority_zero_preregistration(self) -> None:
        info = RECORD_PATH.lstat()
        self.assertTrue(stat.S_ISREG(info.st_mode))
        self.assertFalse(RECORD_PATH.is_symlink())
        self.assertEqual(info.st_size, RECORD_SIZE_BYTES)
        self.assertEqual(digest(RECORD_PATH), RECORD_SHA256)
        self.assertEqual(set(self.record), TOP_KEYS)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-production-generation-preregistration.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "PRE-REGISTERED-PRODUCTION-GENERATION-NO-IMAGE-PRODUCTION-AUTHORITY",
        )
        canonical = (
            json.dumps(self.record, ensure_ascii=False, indent=2, sort_keys=True)
            + "\n"
        ).encode()
        self.assertEqual(self.raw, canonical)

    def test_r1_and_f5_are_the_only_direct_live_bindings(self) -> None:
        self.assertEqual(self.record["bindings"], BINDINGS)
        for row in self.record["bindings"]:
            self.assertEqual(set(row), {"path", "role", "sha256", "sizeBytes"})
            assert_live(self, row)

    def test_f5_transitively_preserves_p1_c1_and_r1(self) -> None:
        fingerprint = load(F5_PATH)
        expected = {
            P1_PATH.relative_to(REPO).as_posix(): (
                "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
                20145,
            ),
            C1_PATH.relative_to(REPO).as_posix(): (
                "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498",
                10971,
            ),
            R1_PATH.relative_to(REPO).as_posix(): (
                "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c",
                10168,
            ),
        }
        observed = {
            row["path"]: (row["sha256"], row["sizeBytes"])
            for row in fingerprint["predecessors"]
        }
        self.assertEqual(observed, expected)
        for path, (sha256, size) in expected.items():
            assert_live(
                self,
                {"path": path, "sha256": sha256, "sizeBytes": size},
            )

    def test_unused_v5_reservations_remain_absent_and_unreusable(self) -> None:
        self.assertEqual(self.record["unusedReservedPaths"], UNUSED_RESERVED_PATHS)
        encoded = json.dumps(self.record, sort_keys=True)
        for row in UNUSED_RESERVED_PATHS:
            self.assertEqual(encoded.count(row["path"]), 1)
            self.assertFalse(os.path.lexists(REPO / row["path"]))

    def test_future_v4_generation_and_fresh_r2_are_exact(self) -> None:
        future = self.record["futureGeneration"]
        self.assertEqual(
            set(future),
            {
                "implementedByThisRecord",
                "newFiles",
                "recordPaths",
                "requiresFreshR2BeforeFingerprintOrAuthority",
                "reusedPinnedUpstream",
            },
        )
        self.assertIs(future["implementedByThisRecord"], False)
        self.assertIs(future["requiresFreshR2BeforeFingerprintOrAuthority"], True)
        self.assertEqual(future["newFiles"], NEW_FILES)
        self.assertEqual(future["recordPaths"], RECORD_PATHS)
        self.assertEqual(future["reusedPinnedUpstream"], REUSED_UPSTREAM)
        all_paths = [row["path"] for row in NEW_FILES + REUSED_UPSTREAM]
        self.assertEqual(len(all_paths), len(set(all_paths)))
        for row in REUSED_UPSTREAM:
            assert_live(self, row)

    def test_version_namespaces_prevent_v4_history_confusion(self) -> None:
        self.assertEqual(
            self.record["versionNamespaces"],
            {
                "authority": 6,
                "fingerprint": 6,
                "producerGeneration": 4,
                "productionResult": 6,
                "rehearsalResult": 2,
                "withdrawnReservation": 5,
            },
        )

    def test_digest_dag_is_exact_one_way_and_acyclic(self) -> None:
        self.assertEqual(self.record["dag"], DAG)
        graph = {edge["binder"]: set(edge["binds"]) for edge in DAG["edges"]}
        visiting: set[str] = set()
        visited: set[str] = set()

        def visit(node: str) -> None:
            self.assertNotIn(node, visiting, f"digest cycle at {node}")
            if node in visited:
                return
            visiting.add(node)
            for child in graph.get(node, set()):
                visit(child)
            visiting.remove(node)
            visited.add(node)

        for node in graph:
            visit(node)

    def test_predecessor_disposition_and_all_zero_ledgers_are_exact(self) -> None:
        self.assertEqual(
            self.record["predecessorDisposition"],
            {
                "f5MayAuthoriseProduction": False,
                "f5MayBeReusedAsF6": False,
                "f5RemainsHistoricalRehearsalEvidence": True,
                "historicalFreeRehearsalsObserved": 1,
                "p1AndC1RemainBytePreserved": True,
                "r1AndF5RemainBytePreserved": True,
                "supersedesP1FutureReservationForProductionOnly": True,
            },
        )
        self.assertEqual(self.record["authorisations"], AUTHORISATIONS)
        self.assertEqual(self.record["runs"], RUNS)
        self.assertEqual(self.record["invariants"], INVARIANTS)
        for key, value in self.record["authorisations"].items():
            if key == "imageProductionRunsAllowed":
                self.assertIs(type(value), int)
                self.assertEqual(value, 0)
            else:
                self.assertIs(type(value), bool)
                self.assertIs(value, False)
        for value in self.record["runs"].values():
            self.assertIs(type(value), int)
            self.assertEqual(value, 0)

    def test_hard_stops_are_exact_and_no_future_digest_is_embedded(self) -> None:
        self.assertEqual(self.record["hardStopConditions"], HARD_STOPS)
        encoded = json.dumps(self.record, sort_keys=True)
        for forbidden in ("authoritySha256", "fingerprintSha256", "resultSha256"):
            self.assertNotIn(forbidden, encoded)
        self.assertFalse(self.record["whatThisRecordDoesNotEstablish"] == [])

    def test_permanent_gates_and_three_docs_are_active(self) -> None:
        self_test = (REPO / "scripts/self-test.sh").read_text()
        docs_smoke = (REPO / "scripts/docs-smoke.sh").read_text()
        self.assertEqual(self_test.count(GATE), 1)
        self.assertIn(f"require_file {GATE}", docs_smoke)
        self.assertIn(MARKER, docs_smoke)
        for relative in (
            "docs/mac-first-hidden-linux-execution-plan-v1.md",
            "docs/native-submission-shadow-verification-v1.md",
            "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
        ):
            self.assertEqual((REPO / relative).read_text().count(MARKER), 1)


if __name__ == "__main__":
    unittest.main()
