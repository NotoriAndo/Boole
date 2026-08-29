#!/usr/bin/env python3
"""Freeze the zero-authority launcher-v2 successor producer generation."""
from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-preregistration-arm64-v1.json"
)
PREFLIGHT_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json"
)

EXPECTED_BINDINGS = {
    "native/containment/native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json": (
        "2a2bfa93796e0ec1463e1d144250e3bc4e2f6b9c2486c35846e3b9f70071d19d",
        9_409,
    ),
    "native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json": (
        "bb51f61b044b9ff651282860eb8645dc97e9122bc446cf65f2489bfefbd73173",
        12_632,
    ),
    "native/containment/native-shadow-launcher-source-overlay-arm64-v2.json": (
        "a138cf374459e6c70c591998cae0c974a0ac58965e91d5cbea230f10df7f3970",
        3_782,
    ),
    "native/containment/native-shadow-launcher-build-authority-arm64-v2.json": (
        "1fa2430a04e750d2c3cba22bab03d7a30e2a244300c729ddbd904d282958a5da",
        3_773,
    ),
    "native/containment/native-shadow-launcher-build-result-arm64-v2.json": (
        "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08",
        1_314,
    ),
    "native/containment/native-shadow-launcher-v2-console-evidence-protocol-arm64-v1.json": (
        "f5faaa26b38120e67cbf6748581ac1e793e8ac4f87a629004a025a67b5e725f5",
        1_878,
    ),
    "scripts/native_shadow_launcher_emit_arm64_v2.py": (
        "e75b57adbc0ad19dea314d1fa65b9ec56aaf4e1c369f34405ace0378c6adf044",
        11_293,
    ),
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json": (
        "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f",
        359_099,
    ),
    "native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json": (
        "0542978a6c49287b27c46a836ae3c1aa548d61e4e065b345ebccbb8d8821dedd",
        3_506,
    ),
    "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json": (
        "a9b53199ca519def2232687c096a7fbefeef13a26f68ba44fcb9a3da30d35d18",
        2_536,
    ),
    "scripts/native_shadow_boot_staging_measure_arm64_v1.py": (
        "d7deacc81e1262b8bd6c9b525a2784850db55c7d93425458243daf5d45fc75b1",
        20_420,
    ),
    "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json": (
        "829ca81d321d412746cce7a62d59d7e538c394b92c1b6a9a966f3016b73cede0",
        102_952,
    ),
    "scripts/native_shadow_rootfs_builder_boot_arm64_v4.py": (
        "28a53945226af569468fd0db590fb74419afe7aee20efa97d443254634849770",
        9_415,
    ),
    "scripts/native_shadow_rootfs_portable_boot_arm64_v2.py": (
        "15f88cf286879ae30aae10bb7819aefea91095a819d96c2634ee9ecc4ea2f305",
        3_528,
    ),
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json": (
        "6e4a2ae19493f81a58e6130ea95676967cb08fd7d7f3694eb90f673e6e3a1820",
        4_938,
    ),
    "scripts/native_shadow_boot_image_produce_arm64_v1.py": (
        "5cdc249751a7b8c3128fcff2150059692a00de8590a925f426bc58129056e939",
        11_173,
    ),
    "scripts/native_shadow_boot_image_verify_arm64_v1.py": (
        "3c97808a6dd7b83feb679ca21ce257019b8d549250c1e39ab87e0a6fccdf6e3e",
        12_416,
    ),
    "scripts/native_shadow_boot_root_disk_readback_arm64_v1.py": (
        "9c41473050b34b830ac6758d88d217d8844ce3154686c93875c1493b50b90589",
        9_852,
    ),
    "native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json": (
        "50a76ca2a6926a897006ae0d294509934cb8f6f0b902f09d2dc88941185290cc",
        41_237,
    ),
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v4.json": (
        "de25e3ac769ab4c575fff6b0839a2ccee2273decdbdd15eb87b3ed2a4c90b765",
        4_610,
    ),
    "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json": (
        "0faddb098503bbf17bf94ec36148e6ccf1af8fa1335ba0e5e9c79cd9d573b7dd",
        14_002,
    ),
    "native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json": (
        "2ff7a3a30513092495a2d8b67555b4e974ef75af47de08acfe8c049063549126",
        10_112,
    ),
    "native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json": (
        "74b9507932b4eda97c89753f642bac579593b034b3e9eff24bb5b056c09079a6",
        45_082,
    ),
}

EXPECTED_HISTORICAL_GENERATION = {
    "scripts/native_shadow_successor_produce_phase_arm64_v2.py": (
        "1c1b99257aa5f2d3f144387f72903fc167d6ba8c8b71a74c1b9a6c845073c1a8",
        72_410,
    ),
    "scripts/native-shadow-successor-produce-arm64.sh": (
        "e63e3e22d876910381b3604be621e7193772cfa842439eef0afe326ba470a07c",
        9_664,
    ),
    ".github/workflows/native-shadow-successor-produce-arm64.yml": (
        "a6ff2019a9e8f95580ebcb82e32d3a12f1a0397bb25912478716772683601b61",
        19_223,
    ),
    "scripts/native_shadow_successor_root_disk_readback_arm64_v2.py": (
        "8ada8ffa5cdf8405973af7e65589a89ee16bdaddc5bb6e7127dac8c607ce8fa0",
        9_523,
    ),
    "scripts/native_shadow_boot_image_produce_arm64_v1.py": (
        "5cdc249751a7b8c3128fcff2150059692a00de8590a925f426bc58129056e939",
        11_173,
    ),
    "scripts/test_native_shadow_successor_root_disk_readback_arm64_v2.py": (
        "b05ddfe4e9bc734903561ffedeca7713a21cbf5142e186f4a29a7ece5bb2aff4",
        28_991,
    ),
}

EXPECTED_FUTURE_FILES = {
    "producer": "scripts/native_shadow_successor_produce_phase_arm64_v3.py",
    "producerGate": "scripts/test_native_shadow_successor_produce_phase_arm64_v3.py",
    "wrapper": "scripts/native-shadow-successor-produce-arm64-v3.sh",
    "workflow": ".github/workflows/native-shadow-successor-produce-arm64-v3.yml",
    "workflowGate": "scripts/test_native_shadow_successor_produce_workflow_arm64_v3.py",
    "readback": "scripts/native_shadow_successor_root_disk_readback_arm64_v3.py",
    "readbackGate": "scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py",
    "producerFingerprint": "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v5.json",
    "freeRehearsalResult": "native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v1.json",
    "futureOneUseAuthority": "native/containment/native-shadow-mac3-successor-production-authority-arm64-v5.json",
    "futureProductionResult": "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v5.json",
}

EXPECTED_TOP_LEVEL_KEYS = {
    "authorisations",
    "bindings",
    "expectedPreflight",
    "freeRehearsal",
    "futureAuthorityBinding",
    "futureGeneration",
    "hardStopConditions",
    "historicalGeneration",
    "invariants",
    "runs",
    "schema",
    "sourcePreflight",
    "status",
    "subject",
    "whatThisRecordDoesNotEstablish",
}

EXPECTED_HARD_STOPS = [
    "any of the twenty-three bound files differs in digest, size or regular-file identity",
    "the tracked preflight payload or its complete CI archive lineage differs",
    "the new generation reuses a historical producer, wrapper, workflow or readback filename",
    "the v3 readback does not bind source-lock v2 and launcher-result v2 before loop setup or mount",
    "the v3 readback accepts launcher v1, falls back to it or accepts an outside binding override",
    "a free rehearsal uses a separate assembly branch or global monkeypatch instead of the production orchestration and assembler",
    "a free rehearsal creates an image, invokes an image effect, creates an attempt marker or production output directory, or retains anything except one canonical JSON member",
    "an authorityless production entry reaches assembly, an output directory, a marker or an image effect",
    "a future authority does not bind this preregistration, the free-rehearsal result and a fingerprint of the exact seven generation files by digest",
    "readback does not unmount in finally, detach its loop device, hard-stop on cleanup failure, or require a passing readback before qualification and replica comparison",
    "any enumerated historical production evidence is rewritten or its known test drift is hidden",
]

EXPECTED_NON_CLAIMS = [
    "that the future v3 producer generation has been implemented",
    "that a free rehearsal has run",
    "that any image-production authority exists",
    "that any image has been produced or booted",
    "that MAC.4, testnet, mining, rewards, consensus or P2P have started",
]

EXPECTED_FINGERPRINTED_GENERATION_FILES = [
    EXPECTED_FUTURE_FILES[key]
    for key in (
        "producer",
        "producerGate",
        "wrapper",
        "workflow",
        "workflowGate",
        "readback",
        "readbackGate",
    )
]


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class LauncherV2SuccessorProducerPreregistrationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.record = load(RECORD_PATH)

    def test_record_is_canonical_zero_authority_preregistration(self) -> None:
        self.assertEqual(set(self.record), EXPECTED_TOP_LEVEL_KEYS)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-preregistration.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY",
        )
        canonical = (
            json.dumps(self.record, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
        ).encode("utf-8")
        self.assertEqual(RECORD_PATH.read_bytes(), canonical)
        self.assertEqual(
            self.record["subject"],
            "Freeze the launcher-v2 successor producer, free rehearsal and readback-v3 boundary before any producer implementation or one-use authority exists.",
        )
        self.assertEqual(self.record["hardStopConditions"], EXPECTED_HARD_STOPS)
        self.assertEqual(
            self.record["whatThisRecordDoesNotEstablish"], EXPECTED_NON_CLAIMS
        )

    def test_exactly_twenty_three_bound_inputs_match_live_bytes(self) -> None:
        bindings = self.record["bindings"]
        self.assertEqual(len(bindings), 23)
        self.assertEqual({row["path"] for row in bindings}, set(EXPECTED_BINDINGS))
        self.assertEqual(len({row["path"] for row in bindings}), len(bindings))
        for row in bindings:
            with self.subTest(path=row["path"]):
                self.assertEqual(set(row), {"path", "role", "sha256", "sizeBytes"})
                path = pathlib.Path(row["path"])
                self.assertFalse(path.is_absolute())
                self.assertNotIn("..", path.parts)
                expected_sha, expected_size = EXPECTED_BINDINGS[row["path"]]
                self.assertEqual(row["sha256"], expected_sha)
                self.assertIs(type(row["sizeBytes"]), int)
                self.assertEqual(row["sizeBytes"], expected_size)
                live = REPO / row["path"]
                info = live.lstat()
                self.assertTrue(stat.S_ISREG(info.st_mode))
                self.assertFalse(live.is_symlink())
                self.assertEqual(sha256(live), expected_sha)
                self.assertEqual(len(live.read_bytes()), expected_size)

    def test_source_preflight_payload_and_ci_lineage_are_exact(self) -> None:
        source = self.record["sourcePreflight"]
        self.assertEqual(
            source,
            {
                "archiveSha256": "beb2920dcfe11ae0f827b73245a8a15bf9e7b055809ad23fac953cef4ed633c8",
                "archiveSizeBytes": 3079,
                "artifactId": 9720614194,
                "artifactMemberCount": 1,
                "artifactMemberName": "PREFLIGHT-RESULT.json",
                "artifactName": "launcher-v2-image-preflight-result",
                "headSha": "6e95d5a73a17dda26adb006cd2c0de5129a1921d",
                "jobId": 99153889500,
                "payloadPath": PREFLIGHT_PATH.relative_to(REPO).as_posix(),
                "payloadSha256": "2a2bfa93796e0ec1463e1d144250e3bc4e2f6b9c2486c35846e3b9f70071d19d",
                "payloadSizeBytes": 9409,
                "pullRequest": 303,
                "runAttempt": 1,
                "runId": 33272680385,
                "status": "PASS-NO-IMAGE-PRODUCED",
                "workflow": ".github/workflows/ci.yml",
            },
        )
        for key in (
            "archiveSizeBytes",
            "artifactId",
            "artifactMemberCount",
            "jobId",
            "payloadSizeBytes",
            "pullRequest",
            "runAttempt",
            "runId",
        ):
            self.assertIs(type(source[key]), int, key)
        payload = load(PREFLIGHT_PATH)
        self.assertEqual(payload["status"], source["status"])
        self.assertEqual(sha256(PREFLIGHT_PATH), source["payloadSha256"])
        self.assertEqual(len(PREFLIGHT_PATH.read_bytes()), source["payloadSizeBytes"])

    def test_every_authority_and_run_budget_has_an_exact_false_or_zero_type(self) -> None:
        authorisations = self.record["authorisations"]
        self.assertEqual(
            set(authorisations),
            {
                "bootAuthorised",
                "consensusActivated",
                "imageProductionAuthorised",
                "imageProductionRunsAllowed",
                "mac4Started",
                "miningActivated",
                "p2pActivated",
                "rewardActivated",
                "testnetStarted",
            },
        )
        for key, value in authorisations.items():
            if key == "imageProductionRunsAllowed":
                self.assertIs(type(value), int)
                self.assertEqual(value, 0)
            else:
                self.assertIs(type(value), bool, key)
                self.assertIs(value, False, key)
        runs = self.record["runs"]
        self.assertEqual(
            set(runs),
            {
                "bootsAllowed",
                "bootsPerformed",
                "freeRehearsalsPerformedByThisRecord",
                "imageProductionsAllowed",
                "imageProductionsPerformed",
            },
        )
        for key, value in runs.items():
            self.assertIs(type(value), int, key)
            self.assertEqual(value, 0, key)

    def test_free_rehearsal_cannot_create_an_image_marker_or_output(self) -> None:
        rehearsal = self.record["freeRehearsal"]
        self.assertEqual(
            set(rehearsal),
            {
                "allowedArtifact",
                "allowedImageTools",
                "artifactMemberCount",
                "attemptMarkersCreated",
                "authoritylessProductionEntry",
                "createsProductionOutputDirectory",
                "effectsAreDependencyInjected",
                "exhaustiveScratchSnapshotRequired",
                "forbiddenOutputNames",
                "globalMonkeypatchForbidden",
                "imageEffectCallsAllowed",
                "imageFilesCreated",
                "measurementMustMatchExpectedPreflight",
                "orchestrationCallable",
                "productionOutputsCreated",
                "repeatable",
                "requiresImageProductionAuthority",
                "sameAssemblerObject",
                "sameProducerAndAssemblerAsFutureProduction",
                "separateRehearsalAssemblyBranchForbidden",
            },
        )
        self.assertIs(rehearsal["repeatable"], True)
        self.assertIs(rehearsal["requiresImageProductionAuthority"], False)
        self.assertIs(rehearsal["sameProducerAndAssemblerAsFutureProduction"], True)
        self.assertIs(rehearsal["createsProductionOutputDirectory"], False)
        self.assertEqual(
            rehearsal["allowedArtifact"], "one canonical JSON result only"
        )
        self.assertIs(rehearsal["globalMonkeypatchForbidden"], True)
        self.assertIs(rehearsal["effectsAreDependencyInjected"], True)
        self.assertIs(rehearsal["separateRehearsalAssemblyBranchForbidden"], True)
        self.assertIs(rehearsal["measurementMustMatchExpectedPreflight"], True)
        self.assertIs(rehearsal["exhaustiveScratchSnapshotRequired"], True)
        self.assertEqual(rehearsal["orchestrationCallable"], "prepare_staging")
        self.assertEqual(
            rehearsal["sameAssemblerObject"],
            "scripts.native_shadow_rootfs_builder_boot_arm64_v4.materialize_staging_tree",
        )
        self.assertIs(type(rehearsal["artifactMemberCount"]), int)
        self.assertEqual(rehearsal["artifactMemberCount"], 1)
        self.assertIs(type(rehearsal["imageEffectCallsAllowed"]), int)
        self.assertEqual(rehearsal["imageEffectCallsAllowed"], 0)
        for key in (
            "imageFilesCreated",
            "attemptMarkersCreated",
            "productionOutputsCreated",
        ):
            self.assertIs(type(rehearsal[key]), int, key)
            self.assertEqual(rehearsal[key], 0, key)
        self.assertEqual(rehearsal["allowedImageTools"], [])
        self.assertEqual(
            rehearsal["forbiddenOutputNames"],
            [
                "ATTEMPT-CONSUMED.json",
                "guest-kernel",
                "guest-initrd",
                "guest-root-disk",
            ],
        )
        refusal = rehearsal["authoritylessProductionEntry"]
        self.assertEqual(
            set(refusal),
            {
                "refusesBeforeAssembly",
                "refusesBeforeAttemptMarker",
                "refusesBeforeImageEffect",
                "refusesBeforeOutputDirectory",
                "refusesBeforeProductionOutput",
            },
        )
        self.assertIs(refusal["refusesBeforeAssembly"], True)
        self.assertIs(refusal["refusesBeforeAttemptMarker"], True)
        self.assertIs(refusal["refusesBeforeImageEffect"], True)
        self.assertIs(refusal["refusesBeforeOutputDirectory"], True)
        self.assertIs(refusal["refusesBeforeProductionOutput"], True)

    def test_historical_v2_generation_is_pinned_and_must_stay_unchanged(self) -> None:
        history = self.record["historicalGeneration"]
        self.assertEqual(
            set(history),
            {
                "files",
                "knownHistoricalProducerV2TestDrift",
                "mustStayByteUnchanged",
            },
        )
        self.assertIs(history["mustStayByteUnchanged"], True)
        pins = history["files"]
        self.assertEqual({row["path"] for row in pins}, set(EXPECTED_HISTORICAL_GENERATION))
        self.assertEqual(len(pins), 6)
        for row in pins:
            with self.subTest(path=row["path"]):
                self.assertEqual(set(row), {"path", "role", "sha256", "sizeBytes"})
                expected_sha, expected_size = EXPECTED_HISTORICAL_GENERATION[row["path"]]
                self.assertEqual(row["sha256"], expected_sha)
                self.assertEqual(row["sizeBytes"], expected_size)
                live = REPO / row["path"]
                info = live.lstat()
                self.assertTrue(stat.S_ISREG(info.st_mode))
                self.assertFalse(live.is_symlink())
                self.assertEqual(sha256(live), expected_sha)
                self.assertEqual(len(live.read_bytes()), expected_size)

    def test_known_historical_producer_gate_drift_is_inherited_not_hidden(self) -> None:
        source = load(
            REPO
            / "native/containment/"
            "native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json"
        )["generation"]["knownHistoricalFingerprintDrift"]
        inherited = self.record["historicalGeneration"][
            "knownHistoricalProducerV2TestDrift"
        ]
        self.assertEqual(inherited, source)
        self.assertIs(inherited["stillPinsLiveBytes"], False)
        drift = inherited["pinsThatNoLongerMatchTheLiveFile"]
        self.assertEqual(len(drift), 1)
        self.assertEqual(
            drift[0]["path"],
            "scripts/test_native_shadow_successor_produce_phase_arm64_v2.py",
        )
        self.assertNotEqual(
            sha256(REPO / drift[0]["path"]), drift[0]["sealedSha256"]
        )

    def test_new_generation_uses_only_the_preregistered_v3_and_v5_names(self) -> None:
        generation = self.record["futureGeneration"]
        self.assertEqual(
            set(generation),
            {
                "files",
                "implementedByThisRecord",
                "newGenerationFilesOnly",
                "readbackV3Contract",
                "reuseHistoricalFilenames",
            },
        )
        self.assertEqual(generation["files"], EXPECTED_FUTURE_FILES)
        self.assertIs(generation["implementedByThisRecord"], False)
        self.assertIs(generation["newGenerationFilesOnly"], True)
        self.assertIs(generation["reuseHistoricalFilenames"], False)

    def test_future_readback_binds_v2_source_lock_and_launcher_and_rejects_v1(self) -> None:
        contract = self.record["futureGeneration"]["readbackV3Contract"]
        self.assertEqual(
            set(contract),
            {
                "bindingOverridesForbidden",
                "cleanupPolicy",
                "exactCallEdges",
                "failureArtifactClass",
                "failureCannotEnterQualifiedComparison",
                "fallbackToV1Forbidden",
                "forbiddenHistoricalCallees",
                "mountPolicy",
                "qualificationRequiresReadbackPass",
                "rejectedHistoricalLauncher",
                "requiredBindings",
                "v1LauncherMustBeRejected",
                "verifyDigestsBefore",
                "wrapperCallsOnlyReadbackV3",
                "wrapperReadbackPath",
            },
        )
        self.assertEqual(
            contract["requiredBindings"],
            [
                {
                    "path": "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json",
                    "sha256": EXPECTED_BINDINGS[
                        "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
                    ][0],
                },
                {
                    "path": "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
                    "sha256": EXPECTED_BINDINGS[
                        "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
                    ][0],
                },
            ],
        )
        rejected = contract["rejectedHistoricalLauncher"]
        self.assertEqual(
            rejected,
            {
                "path": "native/containment/native-shadow-launcher-build-result-arm64-v1.json",
                "sha256": "eca743b903a6ef22ef214a14890042edaee3afd80af11c97503c255b67c0764c",
                "sizeBytes": 1028,
            },
        )
        rejected_path = REPO / rejected["path"]
        rejected_info = rejected_path.lstat()
        self.assertTrue(stat.S_ISREG(rejected_info.st_mode))
        self.assertFalse(rejected_path.is_symlink())
        self.assertEqual(sha256(rejected_path), rejected["sha256"])
        self.assertIs(contract["fallbackToV1Forbidden"], True)
        self.assertIs(contract["v1LauncherMustBeRejected"], True)
        self.assertEqual(
            contract["bindingOverridesForbidden"],
            ["cli", "environment", "image-provided-values"],
        )
        self.assertEqual(
            contract["verifyDigestsBefore"], ["loop-device-setup", "mount"]
        )
        self.assertEqual(
            contract["mountPolicy"],
            {"nodev": True, "noexec": True, "nosuid": True, "readOnly": True},
        )
        for value in contract["mountPolicy"].values():
            self.assertIs(type(value), bool)
            self.assertIs(value, True)
        self.assertEqual(contract["failureArtifactClass"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertEqual(
            contract["cleanupPolicy"],
            {
                "cleanupFailureIsHardStop": True,
                "loopDeviceDetached": True,
                "unmountInFinally": True,
            },
        )
        for value in contract["cleanupPolicy"].values():
            self.assertIs(type(value), bool)
            self.assertIs(value, True)
        self.assertIs(contract["failureCannotEnterQualifiedComparison"], True)
        self.assertIs(contract["qualificationRequiresReadbackPass"], True)
        self.assertIs(contract["wrapperCallsOnlyReadbackV3"], True)
        self.assertEqual(
            contract["wrapperReadbackPath"], EXPECTED_FUTURE_FILES["readback"]
        )
        self.assertEqual(
            contract["exactCallEdges"],
            [
                {
                    "caller": EXPECTED_FUTURE_FILES["workflow"],
                    "callee": EXPECTED_FUTURE_FILES["wrapper"],
                },
                {
                    "caller": EXPECTED_FUTURE_FILES["wrapper"],
                    "callee": EXPECTED_FUTURE_FILES["producer"],
                },
                {
                    "caller": EXPECTED_FUTURE_FILES["wrapper"],
                    "callee": EXPECTED_FUTURE_FILES["readback"],
                },
            ],
        )
        self.assertEqual(
            contract["forbiddenHistoricalCallees"],
            [
                "scripts/native-shadow-successor-produce-arm64.sh",
                "scripts/native_shadow_successor_root_disk_readback_arm64_v2.py",
            ],
        )

    def test_future_authority_binding_is_strictly_one_way(self) -> None:
        boundary = self.record["futureAuthorityBinding"]
        self.assertEqual(
            set(boundary),
            {
                "authorityGrantedByThisRecord",
                "authorityImplementedByThisRecord",
                "cyclicBindingForbidden",
                "direction",
                "futureAuthorityMustBindFreeRehearsalResultByDigest",
                "futureAuthorityMustBindProducerFingerprintByDigest",
                "futureAuthorityMustBindThisRecordByDigest",
                "generationFilesMustNotBindFutureAuthorityOrFingerprintBytes",
                "producerFingerprintBindsFutureAuthorityBytes",
                "producerFingerprintMustBindExactGenerationFiles",
                "producerFingerprintMustBindThisRecordByDigest",
                "productionEntryMustVerifyChainBefore",
                "thisRecordBindsFutureAuthorityBytes",
            },
        )
        self.assertEqual(
            boundary["direction"],
            "future-authority-binds-this-preregistration",
        )
        self.assertIs(boundary["futureAuthorityMustBindThisRecordByDigest"], True)
        self.assertIs(
            boundary["futureAuthorityMustBindProducerFingerprintByDigest"], True
        )
        self.assertIs(
            boundary["futureAuthorityMustBindFreeRehearsalResultByDigest"], True
        )
        self.assertIs(boundary["producerFingerprintMustBindThisRecordByDigest"], True)
        self.assertIs(boundary["producerFingerprintBindsFutureAuthorityBytes"], False)
        self.assertIs(
            boundary["generationFilesMustNotBindFutureAuthorityOrFingerprintBytes"],
            True,
        )
        self.assertEqual(
            boundary["producerFingerprintMustBindExactGenerationFiles"],
            EXPECTED_FINGERPRINTED_GENERATION_FILES,
        )
        self.assertEqual(
            boundary["productionEntryMustVerifyChainBefore"],
            ["assembly", "output-directory", "attempt-marker", "image-effect"],
        )
        self.assertIs(boundary["thisRecordBindsFutureAuthorityBytes"], False)
        self.assertIs(boundary["cyclicBindingForbidden"], True)
        self.assertIs(boundary["authorityImplementedByThisRecord"], False)
        self.assertIs(boundary["authorityGrantedByThisRecord"], False)

    def test_preflight_measurements_are_inputs_not_image_or_boot_claims(self) -> None:
        payload = load(PREFLIGHT_PATH)
        expected = self.record["expectedPreflight"]
        self.assertEqual(
            set(expected),
            {
                "activationAllowed",
                "bootableClaim",
                "imageProduced",
                "launcher",
                "measurement",
                "nestedContentManifest",
            },
        )
        self.assertEqual(expected["launcher"], payload["launcher"])
        self.assertEqual(expected["measurement"], payload["builderInternal"])
        self.assertEqual(expected["nestedContentManifest"], payload["nestedContentManifest"])
        for key in ("imageProduced", "bootableClaim", "activationAllowed"):
            self.assertIs(expected[key], payload[key], key)
            self.assertIs(expected[key], False, key)

    def test_hold_invariants_remain_exact(self) -> None:
        self.assertEqual(
            self.record["invariants"],
            {
                "BF.7": "HOLD",
                "LLM-MINEABLE-ELIGIBLE-V5": 14160,
                "REWARD_READY": 0,
                "RP0-MD": "HOLD",
                "activationAllowed": False,
                "baseActivation": False,
                "mineable_now": 0,
            },
        )
        for key in ("LLM-MINEABLE-ELIGIBLE-V5", "REWARD_READY", "mineable_now"):
            self.assertIs(type(self.record["invariants"][key]), int, key)
        for key in ("activationAllowed", "baseActivation"):
            self.assertIs(type(self.record["invariants"][key]), bool, key)
            self.assertIs(self.record["invariants"][key], False, key)

    def test_s2_result_rederivation_gate_remains_registered(self) -> None:
        lines = [
            line
            for line in (REPO / "scripts/self-test.sh").read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
        source_gate = "scripts/test_native_shadow_launcher_v2_image_preflight_result_arm64_v1.py"
        this_gate = "scripts/test_native_shadow_launcher_v2_successor_producer_preregistration_arm64_v1.py"
        self.assertEqual(sum(source_gate in line for line in lines), 1)
        self.assertEqual(sum(this_gate in line for line in lines), 1)


if __name__ == "__main__":
    unittest.main()
