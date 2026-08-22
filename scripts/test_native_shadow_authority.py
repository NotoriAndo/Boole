#!/usr/bin/env python3
"""Tracked native-checker authority bundle contract.

This test deliberately uses only Git-tracked paths.  A passing run proves the
qualification checker and its public, non-issuable fixture do not depend on the
gitignored experiment archive, a developer home directory, or a stored mining
answer.
"""

from __future__ import annotations

import ast
import hashlib
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER = ROOT / "native" / "checker" / "rust-tuple-struct-project-v1"
FIXTURE = ROOT / "fixtures" / "native-shadow" / "rust-tuple-struct-project-v1"
REGISTRY = ROOT / "fixtures" / "native-shadow" / "registry-v1.json"
EXECUTION_POLICY = (
    ROOT / "native" / "containment" / "native-shadow-execution-policy-v1.json"
)
TOOLCHAIN_IDENTITY = (
    ROOT / "native" / "containment" / "native-shadow-toolchain-identity-v1.json"
)
IMPLEMENTATION_SPEC = (
    ROOT / "docs" / "node-native-shadow-binding-containment-implementation-spec-v1.md"
)
SELF_TEST = ROOT / "scripts" / "self-test.sh"


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_sums(path: pathlib.Path) -> dict[str, str]:
    entries: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        digest, rel = line.split("  ", 1)
        if rel in entries:
            raise AssertionError(f"duplicate SHA256SUMS entry: {rel}")
        entries[rel] = digest
    return entries


def reject_duplicate_json_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    parsed: dict[str, object] = {}
    for key, value in pairs:
        if key in parsed:
            raise AssertionError(f"duplicate JSON key: {key}")
        parsed[key] = value
    return parsed


def checker_artifact_hash() -> str:
    digest = hashlib.sha256()
    for rel in ("checker.py", "policy.json"):
        digest.update(rel.encode("utf-8"))
        digest.update(b"\0")
        digest.update((CHECKER / rel).read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def resolve_toolchain_bin() -> pathlib.Path:
    override = os.environ.get("BOOLE_NATIVE_TOOLCHAIN_BIN")
    if override:
        return pathlib.Path(override)
    raise AssertionError(
        "BOOLE_NATIVE_TOOLCHAIN_BIN must name the exact rust-lang CI per-commit "
        "toolchain bin directory"
    )


class NativeShadowAuthorityTests(unittest.TestCase):
    maxDiff = None

    def test_clean_ci_gate_runs_this_authority_test(self) -> None:
        self.assertIn(
            "scripts/test_native_shadow_authority.py",
            SELF_TEST.read_text(encoding="utf-8"),
            "the tracked checker authority test must remain in the required self-test gate",
        )

    def test_checker_reason_vocabulary_matches_execution_policy(self) -> None:
        source = (CHECKER / "checker.py").read_text(encoding="utf-8")
        tree = ast.parse(source)
        deterministic: set[str] = set()
        retryable: set[str] = set()

        for node in ast.walk(tree):
            if isinstance(node, ast.Raise) and isinstance(node.exc, ast.Call):
                call = node.exc
                if (
                    isinstance(call.func, ast.Name)
                    and call.args
                    and isinstance(call.args[0], ast.Constant)
                    and isinstance(call.args[0].value, str)
                ):
                    if call.func.id == "SubmissionRejected":
                        deterministic.add(call.args[0].value)
                    elif call.func.id == "AuthorityUnavailable":
                        retryable.add(call.args[0].value)
            if (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Name)
                and node.func.id == "_read_json"
                and len(node.args) >= 2
                and isinstance(node.args[1], ast.Constant)
                and isinstance(node.args[1].value, str)
            ):
                retryable.add(node.args[1].value)

        infrastructure = next(
            node
            for node in tree.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "_infrastructure_failure_reason"
        )
        retryable.update(
            node.value.value
            for node in ast.walk(infrastructure)
            if isinstance(node, ast.Return)
            and isinstance(node.value, ast.Constant)
            and isinstance(node.value.value, str)
        )
        retryable.add("checker_internal_error")

        policy = json.loads(EXECUTION_POLICY.read_text(encoding="utf-8"))
        vocabulary = policy["ipc"]["objectContracts"]["checkerReasonVocabulary"]
        self.assertEqual(vocabulary["accepted"], ["accepted"])
        self.assertEqual(set(vocabulary["deterministic_reject"]), deterministic)
        self.assertEqual(set(vocabulary["retryable_unavailable"]), retryable)

    def test_execution_policy_is_exact_tracked_authority_and_registry_bound(self) -> None:
        metadata = EXECUTION_POLICY.lstat()
        self.assertFalse(EXECUTION_POLICY.is_symlink())
        self.assertTrue(EXECUTION_POLICY.is_file())
        self.assertGreater(metadata.st_size, 0)

        raw = EXECUTION_POLICY.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()
        self.assertTrue(raw.endswith(b"\n"))
        self.assertNotIn(b"\r\n", raw)
        for forbidden in (b"PENDING", b"PLACEHOLDER", b"TODO", b"TBD"):
            self.assertNotIn(forbidden, raw)
        policy = json.loads(raw, object_pairs_hook=reject_duplicate_json_keys)
        registry = json.loads(REGISTRY.read_text(encoding="utf-8"))
        spec = IMPLEMENTATION_SPEC.read_text(encoding="utf-8")

        self.assertEqual(registry["executionPolicySha256"], digest)
        self.assertEqual(
            registry["toolchainIdentitySha256"], sha256(TOOLCHAIN_IDENTITY)
        )
        self.assertIn(digest, spec)
        self.assertEqual(
            set(policy),
            {
                "schema",
                "release",
                "activationAllowed",
                "purpose",
                "platform",
                "accounts",
                "installation",
                "environment",
                "ipc",
                "checkerInvocation",
                "privilege",
                "resourceLimits",
                "workspace",
                "spawn",
                "seccomp",
                "landlock",
                "crashRecovery",
                "outcomeMapping",
            },
        )
        self.assertEqual(
            policy["schema"], "boole.native-shadow.execution-policy.v1"
        )
        self.assertEqual(
            policy["release"], "NATIVE-SHADOW-EXECUTION-POLICY-V1-QUALIFICATION"
        )
        self.assertFalse(policy["activationAllowed"])

        self.assertEqual(
            policy["platform"],
            {
                "operatingSystem": "linux",
                "architecture": "x86_64",
                "cgroupVersion": 2,
                "systemdRequired": True,
                "minimumLandlockAbi": 3,
            },
        )
        accounts = policy["accounts"]
        self.assertEqual(
            accounts["resolution"],
            "getpwnam_r-and-getgrnam_r-at-launcher-start",
        )
        self.assertEqual(accounts["userLookup"], "getpwnam_r")
        self.assertEqual(accounts["groupLookup"], "getgrnam_r-and-getgrgid_r")
        self.assertEqual(accounts["supplementaryGroupsLookup"], "getgrouplist")
        self.assertTrue(accounts["primaryGroupMustMatchPasswdGid"])
        self.assertEqual(accounts["node"]["name"], "boole-node")
        self.assertEqual(accounts["node"]["primaryGroup"], "boole-node")
        self.assertEqual(accounts["node"]["supplementaryGroupCount"], 0)
        self.assertEqual(accounts["checker"]["name"], "boole-native-checker")
        self.assertEqual(
            accounts["checker"]["primaryGroup"], "boole-native-checker"
        )
        self.assertTrue(accounts["requireDistinctUid"])
        self.assertTrue(accounts["requireDistinctPrimaryGid"])
        self.assertTrue(accounts["numericIdsBoundForProcessLifetime"])
        self.assertEqual(accounts["checker"]["supplementaryGroupCount"], 0)

        installation = policy["installation"]
        self.assertEqual(
            installation["launcherPath"],
            "/usr/libexec/boole/boole-native-shadow-launcher",
        )
        self.assertEqual(
            installation["policyPath"],
            "/usr/share/boole/native-shadow/execution-policy-v1.json",
        )
        self.assertEqual(
            installation["registryPath"],
            "/usr/share/boole/native-shadow/registry-v1.json",
        )
        self.assertEqual(installation["authorityDirectoryMode"], "0555")
        self.assertEqual(
            installation["socketPath"],
            "/run/boole/native-shadow/launcher.sock",
        )
        self.assertEqual(installation["socketDirectoryMode"], "2750")
        self.assertEqual(
            installation["launcherLockPath"],
            "/run/boole/native-shadow/launcher.lock",
        )
        self.assertEqual(installation["launcherLockOwner"], "root")
        self.assertEqual(installation["launcherLockGroup"], "boole-node")
        self.assertEqual(installation["launcherLockMode"], "0600")
        self.assertEqual(installation["socketMode"], "0660")
        self.assertFalse(installation["nodeMayWriteSocketDirectory"])
        self.assertEqual(
            installation["socketCreation"],
            {
                "lock": (
                    "flock:LOCK_EX|LOCK_NB:/run/boole/native-shadow/"
                    "launcher.lock:root:boole-node:0600"
                ),
                "lockOpen": (
                    "openat-verified-socket-directory-fd:launcher.lock:"
                    "O_CREAT|O_RDWR|O_CLOEXEC|O_NOFOLLOW:0600"
                ),
                "lockValidation": (
                    "fstat-regular-nlink1-root:boole-node-mode0600-before-flock"
                ),
                "bindUmask": "0117",
                "groupInheritance": "setgid-directory",
                "postBindAction": "chmod-0660-then-lstat-verify-root:boole-node-socket",
                "staleSocketRemoval": (
                    "only-under-lock-after-lstat-root:boole-node-socket-verification"
                ),
            },
        )
        self.assertEqual(
            installation["checkerEntrypoint"],
            "/usr/share/boole/native-shadow/checkers/"
            "rust-tuple-struct-project-v1/checker.py",
        )
        self.assertEqual(
            installation["pythonInterpreterPath"], "/usr/bin/python3.12"
        )
        self.assertEqual(
            installation["toolchainIdentityPath"],
            "/usr/share/boole/native-shadow/toolchain-identity-v1.json",
        )
        self.assertEqual(
            installation["rootOwnedPathValidation"],
            {
                "followSymlinks": False,
                "requiredOwnerUid": 0,
                "requiredGroupGid": 0,
                "forbidGroupWrite": True,
                "forbidOtherWrite": True,
                "validateEveryAncestor": True,
                "validateBeforeBindAndBeforeEachExecution": True,
                "appliesTo": [
                    "launcherPath",
                    "authorityDirectory",
                    "policyPath",
                    "registryPath",
                    "checkerRoot",
                    "checkerEntrypoint",
                    "checkerPolicyPath",
                    "checkerReleaseManifestPath",
                    "fixtureRoot",
                    "toolchainBin",
                    "toolchainIdentityPath",
                    "pythonInterpreterPath",
                    "workspaceMountpoint",
                ],
                "runtimeSocketUsesSeparateOwnershipContract": True,
            },
        )
        self.assertEqual(
            installation["workspaceMountpoint"],
            {
                "path": "/work",
                "owner": "root",
                "group": "root",
                "mode": "0555",
                "mustBeMountpointBeforeExecution": False,
            },
        )
        self.assertEqual(
            installation["installedArtifactContracts"],
            {
                "launcherPath": "regular-nonsymlink-root:root:0755",
                "policyPath": "regular-nonsymlink-root:root:0444",
                "registryPath": "regular-nonsymlink-root:root:0444",
                "checkerRoot": "directory-nonsymlink-root:root:0555",
                "checkerEntrypoint": "regular-nonsymlink-root:root:0444",
                "checkerPolicyPath": "regular-nonsymlink-root:root:0444",
                "checkerReleaseManifestPath": (
                    "regular-nonsymlink-root:root:0444"
                ),
                "fixtureRoot": (
                    "directory-nonsymlink-root:root:0555;"
                    "descendant-authority-files-0444-no-symlinks"
                ),
                "toolchainBin": (
                    "directory-nonsymlink-root:root:0555;"
                    "content-modes-deferred-to-provenance-manifest"
                ),
                "toolchainIdentityPath": (
                    "regular-nonsymlink-root:root:0444"
                ),
                "pythonInterpreterPath": "regular-nonsymlink-root:root:0755",
                "workspaceMountpoint": "directory-nonsymlink-root:root:0555",
            },
        )

        environment = policy["environment"]
        self.assertTrue(environment["clearInherited"])
        self.assertFalse(environment["requestOverridesAllowed"])
        self.assertEqual(
            environment["fixedLiterals"]["PATH"],
            "/usr/bin:/bin:/usr/sbin:/sbin",
        )
        self.assertEqual(environment["fixedLiterals"]["LC_ALL"], "C")
        self.assertNotIn("PYTHONHASHSEED", environment["fixedLiterals"])
        self.assertNotIn("PYTHONDONTWRITEBYTECODE", environment["fixedLiterals"])
        self.assertEqual(environment["workspaceDerived"], {"HOME": "home", "TMPDIR": "tmp"})

        ipc = policy["ipc"]
        self.assertEqual(ipc["transport"], "unix-stream")
        self.assertEqual(ipc["peerCredentials"], "SO_PEERCRED")
        self.assertEqual(
            ipc["peerCredentialChecks"],
            {
                "launcherAcceptsUid": "resolved-boole-node-uid-only",
                "launcherAcceptsGid": "resolved-boole-node-primary-gid-only",
                "nodeAcceptsUid": 0,
                "nodeAcceptsGid": 0,
                "requirePeerPid": True,
                "validateBeforeFirstFrame": True,
            },
        )
        self.assertEqual(ipc["framing"], "u32-be-length-prefixed-json")
        self.assertTrue(ipc["oneRequestPerConnection"])
        self.assertEqual(
            ipc["messageSequence"],
            [
                "node:hello",
                "launcher:ready",
                "node:execute",
                "node:shutdown-write",
                "launcher:report",
                "launcher:shutdown-write",
                "mutual-eof",
            ],
        )
        self.assertEqual(ipc["binaryEncoding"], "lowercase-hex")
        self.assertEqual(
            ipc["qualificationHandshake"],
            {
                "requiredPolicyActivationAllowed": False,
                "messageSequence": [
                    "node:qualification-hello",
                    "launcher:qualification-ready",
                    "node:shutdown-write",
                    "launcher:shutdown-write",
                    "mutual-eof",
                ],
                "nodeDurableStateChangeAllowed": False,
                "launcherSpawnAllowed": False,
                "launcherReportAllowed": False,
                "launcherInstanceIdSource": (
                    "getrandom:32-bytes:no-fallback-at-launcher-startup"
                ),
                "preExecuteEofAction": "clean-close-without-report",
                "executeFrameActionUnderThisRelease": (
                    "protocol-error-close-without-report-or-spawn"
                ),
            },
        )
        self.assertEqual(
            ipc["frameRules"],
            {
                "payloadEncoding": "utf-8-json-object",
                "rejectBom": True,
                "rejectDuplicateKeys": True,
                "rejectUnknownFields": True,
                "rejectAllJsonFloats": True,
                "rejectNonFiniteNumbers": True,
                "rejectBytesBetweenFrames": True,
                "rejectTrailingBytesAfterExpectedFinalFrame": True,
                "requireExecuteEofBeforeSpawn": True,
                "requireReportEofBeforeAccept": True,
                "hashLengthPrefix": True,
            },
        )
        self.assertEqual(ipc["nonceBytes"], 32)
        self.assertEqual(
            ipc["nonceSource"],
            "node-getrandom:32-bytes:no-fallback-per-connection",
        )
        self.assertEqual(ipc["maxRawAnswerBytes"], 16_384)
        self.assertGreater(ipc["maxRequestFrameBytes"], ipc["maxRawAnswerBytes"])
        self.assertEqual(ipc["executionResponseTimeoutMillis"], 115_000)
        self.assertIn(
            "requestDigestHex-in-hello-ready-report-equals-requestDigest-of-execute-frame",
            ipc["messageBindingRules"],
        )
        self.assertEqual(
            ipc["digestRules"]["requestDigest"],
            {
                "algorithm": "sha256",
                "domainPrefixUtf8Nul": "boole.native-shadow.launcher.request.v1",
                "input": "domain-prefix-nul-then-u32be-length-prefix-then-exact-execute-json-payload-bytes",
                "encoding": "64-lowercase-hex",
            },
        )
        self.assertEqual(
            ipc["digestRules"]["submissionDigest"]["fieldEncoding"],
            {
                "familyVersion": "u32be-length-prefixed-utf8-bytes",
                "templateId": "u32be-length-prefixed-64-lowercase-ascii-hex-bytes",
                "challengeSha256": "u32be-length-prefixed-64-lowercase-ascii-hex-bytes",
                "epoch": "u64be-no-length-prefix",
                "rawAnswer": "u32be-length-prefixed-decoded-utf8-bytes",
            },
        )
        messages = ipc["messages"]
        self.assertEqual(
            messages["qualificationHello"]["requiredFields"],
            {
                "schema": (
                    "literal:boole.native-shadow.launcher.qualification-hello.v1"
                ),
                "nonceHex": "lower-hex:32-bytes",
                "executionPolicyDigestHex": "lower-hex:32-bytes",
                "toolchainIdentityDigestHex": "lower-hex:32-bytes",
                "registryDigestHex": "lower-hex:32-bytes",
            },
        )
        self.assertNotIn(
            "requestDigestHex",
            messages["qualificationHello"]["requiredFields"],
        )
        self.assertNotIn(
            "requestLengthBytes",
            messages["qualificationHello"]["requiredFields"],
        )
        self.assertEqual(
            messages["qualificationReady"]["requiredFields"]
            ["activationAllowed"],
            "literal-bool:false",
        )
        for recovery_field, expected in {
            "startupRecoveryComplete": "literal-bool:true",
            "activeExecutionLeaves": "literal-u32:0",
            "unexpectedDirectCgroupChildren": "literal-u32:0",
            "managerSubgroupVerified": "literal-bool:true",
            "launcherInstanceIdHex": "lower-hex:32-bytes",
        }.items():
            self.assertEqual(
                messages["qualificationReady"]["requiredFields"][recovery_field],
                expected,
            )
        for authority_field in (
            "executionPolicyDigestHex",
            "toolchainIdentityDigestHex",
            "registryDigestHex",
        ):
            self.assertIn(
                authority_field,
                messages["qualificationReady"]["requiredFields"],
            )
        self.assertEqual(
            messages["hello"]["requiredFields"],
            {
                "schema": "literal:boole.native-shadow.launcher.hello.v1",
                "nonceHex": "lower-hex:32-bytes",
                "requestDigestHex": "lower-hex:32-bytes",
                "requestLengthBytes": "u32:1..131072",
                "executionPolicyDigestHex": "lower-hex:32-bytes",
            },
        )
        self.assertEqual(
            messages["execute"]["requiredFields"]["rawAnswerBase64"],
            "canonical-rfc4648-base64:decoded-max-16384-bytes",
        )
        self.assertEqual(
            messages["execute"]["requiredFields"]["submissionSourceBase64"],
            "canonical-rfc4648-base64:decoded-max-16384-bytes",
        )
        self.assertEqual(
            messages["ready"]["requiredFields"]["activationAllowed"],
            "literal-bool:false",
        )
        for gid_field in ("launcherGid", "nodeGid", "checkerGid"):
            self.assertIn(gid_field, messages["ready"]["requiredFields"])
            self.assertIn(gid_field, messages["report"]["requiredFields"])
        self.assertEqual(
            messages["report"]["requiredFields"]["cleanup"],
            "object:cleanup-v1",
        )
        self.assertIn(
            "report-launcherPid-equals-ready-launcherPid-and-node-observed-SO_PEERCRED-pid",
            ipc["messageBindingRules"],
        )
        self.assertIn(
            "report-authorityBindings-equal-execute-fields-and-installed-authority-bytes",
            ipc["messageBindingRules"],
        )

        objects = ipc["objectContracts"]
        self.assertEqual(
            objects["wait-status-v1"]["requiredFields"]["kind"],
            "enum:exited|signaled",
        )
        self.assertEqual(
            objects["checker-result-v1"]["requiredFields"]["status"],
            "enum:valid-checker-result|invalid-or-nonconforming-output|"
            "output-limit-exceeded|no-complete-output",
        )
        self.assertEqual(
            objects["checker-parsed-result-v1"]["requiredFields"]["reasonCode"],
            "enum-by-verdict:checkerReasonVocabulary",
        )
        self.assertEqual(
            objects["checkerReasonVocabulary"],
            {
                "accepted": ["accepted"],
                "deterministic_reject": [
                    "compile_or_hidden_test_failed",
                    "forbidden_construct",
                    "malformed_patch_region",
                    "outside_patch_modified",
                    "patch_line_limit_exceeded",
                    "patch_size_exceeded",
                    "submission_unreadable",
                ],
                "retryable_unavailable": [
                    "anchor_digest_mismatch",
                    "anchor_size_exceeded",
                    "anchor_unavailable",
                    "checker_internal_error",
                    "contained_process_unavailable",
                    "policy_contract_mismatch",
                    "policy_unavailable",
                    "resource_memory_limit",
                    "resource_output_limit",
                    "resource_process_limit",
                    "resource_process_terminated",
                    "resource_wall_limit",
                    "scratch_root_required",
                    "scratch_root_unavailable",
                    "scratch_workspace_unavailable",
                    "task_binding_mismatch",
                    "task_contract_invalid",
                    "toolchain_identity_mismatch",
                    "toolchain_probe_failed",
                    "toolchain_unavailable",
                ],
            },
        )
        self.assertEqual(
            objects["checker-result-v1"]["crossFieldInvariants"],
            [
                "report-exists-only-after-child-created-no-setup-error-record-and-cleanup-success",
                "valid-checker-result-iff-exited-zero-not-timed-out-stderr-empty-stdout-one-json-line-and-parsed-nonnull",
                "output-limit-exceeded-iff-resourceObservations.outputLimitExceeded-true",
                "invalid-or-nonconforming-output-requires-parsed-null",
                "no-complete-output-requires-parsed-null",
            ],
        )

        checker = policy["checkerInvocation"]
        self.assertFalse(checker["executionAllowedUnderThisRelease"])
        self.assertEqual(checker["taskSelection"], "trusted-registry-four-tuple-only")
        self.assertEqual(
            checker["taskOpen"],
            "openat2-fixture-root-resolve-beneath-no-symlinks-no-magiclinks",
        )
        self.assertEqual(checker["anchorPathRequirements"], "literal:anchor.rs")
        self.assertEqual(
            checker["submissionSource"], "decoded-submissionSourceBase64"
        )
        self.assertEqual(checker["intakeOwner"], "boole-node")
        self.assertFalse(checker["launcherReextractsIntake"])
        self.assertEqual(
            checker["intake"]["version"],
            "RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1",
        )
        self.assertEqual(
            checker["executePrerequisite"],
            "durable-InFlightV3-with-matching-operationId-before-connect",
        )
        self.assertEqual(
            checker["argv"],
            [
                "/usr/bin/python3.12",
                "-I",
                "-S",
                "/usr/share/boole/native-shadow/checkers/"
                "rust-tuple-struct-project-v1/checker.py",
                "--task",
                "/work/task.json",
                "--submission",
                "/work/submission.rs",
                "--toolchain-bin",
                "/opt/boole/native-checker-toolchain/bin",
                "--scratch-root",
                "/work/scratch",
            ],
        )
        self.assertFalse(checker["requestMaySelectPath"])
        self.assertTrue(checker["verifyTaskSha256BeforeExec"])
        self.assertTrue(checker["verifyCheckerReleaseBeforeExec"])
        self.assertEqual(
            checker["authorityMaterialization"],
            {
                "source": "parent-verified-sealed-read-only-memfds",
                "actor": "privileged-cloned-child-before-privilege-drop",
                "task": "/work/task.json:root:boole-native-checker:0440",
                "anchor": "/work/anchor.rs:root:boole-native-checker:0440",
                "submission": "/work/submission.rs:root:boole-native-checker:0440",
                "scratch": "/work/scratch:root:boole-native-checker:2770:initially-empty",
                "verifyAfterWrite": "bytes-owner-group-mode-and-empty-scratch",
                "closeAuthorityFdsBeforeExec": True,
            },
        )
        checker_source = (CHECKER / "checker.py").read_text(encoding="utf-8")
        checker_result_schema = "boole.native-shadow.checker-result.v1"
        self.assertIn(f'RESULT_SCHEMA = "{checker_result_schema}"', checker_source)
        self.assertEqual(
            ipc["objectContracts"]["checker-parsed-result-v1"]
            ["requiredFields"]["schema"],
            f"literal:{checker_result_schema}",
        )

        toolchain_metadata = TOOLCHAIN_IDENTITY.lstat()
        self.assertFalse(TOOLCHAIN_IDENTITY.is_symlink())
        self.assertTrue(TOOLCHAIN_IDENTITY.is_file())
        self.assertGreater(toolchain_metadata.st_size, 0)
        toolchain_raw = TOOLCHAIN_IDENTITY.read_bytes()
        self.assertTrue(toolchain_raw.endswith(b"\n"))
        self.assertNotIn(b"\r\n", toolchain_raw)
        for forbidden in (b"PENDING", b"PLACEHOLDER", b"TODO", b"TBD"):
            self.assertNotIn(forbidden, toolchain_raw)
        toolchain_identity = json.loads(
            toolchain_raw,
            object_pairs_hook=reject_duplicate_json_keys,
        )
        self.assertEqual(
            toolchain_identity["schema"],
            "boole.native-shadow.toolchain-identity.v1",
        )
        self.assertFalse(toolchain_identity["activationAllowed"])
        self.assertEqual(
            toolchain_identity["rust"]["rustcCommitHash"],
            "e7795af6d2449fb05a6393c3320ced873a999eb3",
        )
        self.assertEqual(
            toolchain_identity["python"]["requiredVersionPrefix"], "Python 3.12."
        )
        self.assertEqual(toolchain_identity["python"]["path"], "/usr/bin/python3.12")
        runtime_verification = toolchain_identity["runtimeVerification"]
        self.assertFalse(runtime_verification["productionByteProvenanceComplete"])
        self.assertFalse(runtime_verification["executionAllowedBeforeProvenanceClosure"])
        self.assertEqual(
            runtime_verification["activationBlockers"],
            [
                "installed-rust-toolchain-file-manifest",
                "python-interpreter-and-stdlib-file-manifest",
                "system-linker-and-runtime-file-manifest",
            ],
        )

        privilege = policy["privilege"]
        exact_caps = ["CAP_SETGID", "CAP_SETUID", "CAP_SETPCAP", "CAP_SYS_ADMIN"]
        self.assertEqual(privilege["launcherCapabilitySets"]["effective"], exact_caps)
        self.assertEqual(privilege["launcherCapabilitySets"]["permitted"], exact_caps)
        self.assertEqual(privilege["launcherCapabilitySets"]["bounding"], exact_caps)
        self.assertEqual(privilege["launcherCapabilitySets"]["inheritable"], [])
        self.assertEqual(privilege["launcherCapabilitySets"]["ambient"], [])
        self.assertNotIn("CAP_DAC_OVERRIDE", exact_caps)
        self.assertTrue(privilege["startupSelfCheckBeforeSocketBind"])
        self.assertTrue(privilege["failIfCapabilitySetDiffers"])
        self.assertEqual(
            privilege["systemdUnit"]["CapabilityBoundingSet"], exact_caps
        )
        self.assertEqual(privilege["systemdUnit"]["AmbientCapabilities"], [])
        self.assertTrue(privilege["systemdUnit"]["Delegate"])
        self.assertEqual(privilege["nodeCapabilities"], [])
        self.assertEqual(privilege["checkerCapabilities"], [])
        self.assertTrue(privilege["noNewPrivileges"])

        limits = policy["resourceLimits"]
        self.assertEqual(limits["concurrency"], 1)
        self.assertEqual(limits["pidsMax"], 128)
        self.assertEqual(limits["memoryMaxBytes"], 2_147_483_648)
        self.assertEqual(limits["memorySwapMaxBytes"], 0)
        self.assertTrue(limits["memoryOomGroup"])
        self.assertEqual(limits["cpuMax"], "max 100000")
        self.assertEqual(limits["cpuTotalUsec"], 120_000_000)
        self.assertEqual(limits["cpuUsagePollMillis"], 10)
        self.assertEqual(limits["cpuLimitAction"], "freeze-kill-at-or-above-limit")
        self.assertEqual(limits["outerWallMillis"], 100_000)
        self.assertEqual(limits["cleanupDeadlineMillis"], 10_000)

        workspace = policy["workspace"]
        self.assertEqual(workspace["filesystem"], "tmpfs")
        self.assertEqual(workspace["sizeBytes"], 536_870_912)
        self.assertEqual(workspace["inodeLimit"], 8_192)
        self.assertEqual(workspace["mode"], "2750")
        self.assertEqual(workspace["owner"], "root")
        self.assertEqual(workspace["group"], "boole-native-checker")
        self.assertEqual(workspace["mountFlags"], ["nosuid", "nodev"])
        self.assertNotIn("noexec", workspace["mountFlags"])
        self.assertEqual(
            workspace["checkerTemporaryLayout"],
            "/work/scratch/boole-native-check-*/{cargo-home,target,home,tmp,src}",
        )
        self.assertNotIn("cargoHomeRelativePath", workspace)
        self.assertNotIn("targetRelativePath", workspace)

        spawn = policy["spawn"]
        self.assertEqual(spawn["syscall"], "clone3")
        self.assertEqual(
            spawn["flags"],
            ["CLONE_INTO_CGROUP", "CLONE_NEWNS", "CLONE_NEWPID", "CLONE_PIDFD"],
        )
        self.assertEqual(spawn["exitSignal"], "SIGCHLD")
        self.assertTrue(spawn["childIsPid1InNewPidNamespace"])
        self.assertEqual(spawn["fallback"], "none-fail-before-child-without-report")
        self.assertEqual(
            spawn["setupStatusChannel"],
            "close-on-exec-error-record-pipe;eof-means-no-recorded-setup-error-not-proof-of-exec",
        )
        self.assertEqual(spawn["operationIdSource"], "getrandom:32-bytes:no-fallback")
        self.assertEqual(spawn["operationIdCollisionAction"], "fail-before-child")
        self.assertEqual(
            spawn["preExecSequence"],
            [
                "clone3-into-leaf-cgroup-and-private-mount-pid-namespaces-with-pidfd",
                "mount-root-recursive-private-then-private-proc-and-work-tmpfs",
                "materialize-sealed-authority-memfds-and-verify-workspace-contract",
                "close-authority-and-control-fds-except-stdio-and-setup-status-pipe",
                "drop-supplementary-groups-uid-gid-capabilities-and-set-no-new-privileges",
                "apply-node-owned-outer-rlimits",
                "apply-seccomp-and-landlock",
                "exec-checker-and-close-success-status-pipe-on-cloexec",
            ],
        )

        seccomp = policy["seccomp"]
        self.assertEqual(seccomp["defaultAction"], "allow")
        self.assertEqual(seccomp["denyAction"], "errno:EACCES")
        self.assertEqual(seccomp["failureMode"], "fail-closed")
        self.assertEqual(seccomp["deniedSyscalls"], sorted(seccomp["deniedSyscalls"]))
        self.assertEqual(len(seccomp["deniedSyscalls"]), len(set(seccomp["deniedSyscalls"])))
        for syscall in (
            "mount",
            "umount2",
            "unshare",
            "setns",
            "ptrace",
            "socket",
            "connect",
        ):
            self.assertIn(syscall, seccomp["deniedSyscalls"])

        landlock = policy["landlock"]
        self.assertEqual(landlock["minimumAbi"], 3)
        self.assertEqual(landlock["failureMode"], "fail-closed")
        self.assertFalse(landlock["readAccessRestricted"])
        self.assertNotIn("read_file", landlock["handledAccess"])
        self.assertNotIn("read_dir", landlock["handledAccess"])
        self.assertEqual(landlock["writeAllow"], ["workspace"])
        self.assertIn("workspace", landlock["executeAllow"])
        self.assertIn("/usr/bin", landlock["executeAllow"])
        self.assertNotIn("/usr", landlock["executeAllow"])
        self.assertNotIn(
            "cannot open or write any path under `/sys/fs/cgroup`",
            spec,
        )

        crash = policy["crashRecovery"]
        self.assertEqual(crash["operationIdBytes"], 32)
        self.assertEqual(
            crash["cgroupParent"],
            "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service",
        )
        self.assertEqual(crash["managerSubgroupName"], "manager")
        self.assertEqual(
            crash["managerSubgroupContract"],
            {
                "path": "/sys/fs/cgroup/system.slice/"
                "boole-native-shadow-launcher.service/manager",
                "createOrReuse": "exact-direct-child-directory-only",
                "reuseRequires": (
                    "cgroup.events-populated-zero;cgroup.procs-and-cgroup.threads-empty;"
                    "no-child-cgroups-before-moving-current-launcher"
                ),
                "postMoveRequires": (
                    "only-current-launcher-pid-in-cgroup.procs-and-no-child-cgroups"
                ),
            },
        )
        self.assertEqual(crash["executionLeafParent"], "service-cgroup-root")
        self.assertEqual(
            crash["startupCgroupSequence"],
            [
                "create-manager-subgroup",
                "move-launcher-process-to-manager/cgroup.procs",
                "verify-service-root-cgroup.procs-empty",
                "enable-cpu-memory-pids-in-service-root-cgroup.subtree_control",
                "verify-controller-readback",
            ],
        )
        self.assertEqual(crash["recoveryScanExcludes"], ["manager"])
        self.assertEqual(
            crash["unexpectedDirectChildAction"], "fail-closed-without-ready"
        )
        self.assertEqual(crash["cgroupLeafPrefix"], "run-")
        self.assertEqual(
            crash["cgroupLeafName"], "run-<64-lowercase-hex-operation-id>"
        )
        self.assertEqual(crash["cleanupOwner"], "launcher")
        self.assertEqual(
            crash["resourceObservationSnapshot"],
            "after-terminal-wait-and-any-required-kill-after-populated-zero-before-leaf-removal",
        )
        self.assertEqual(
            crash["qualificationReadyBarrier"],
            {
                "launcherCleansAllRunLeavesBeforeSocketBind": True,
                "unexpectedDirectChildAction": "fail-closed-without-ready",
                "readyRequiresStartupRecoveryComplete": True,
                "readyRequiresActiveExecutionLeaves": 0,
                "readyRequiresUnexpectedDirectCgroupChildren": 0,
                "readyRequiresManagerSubgroupVerified": True,
                "nodeRollbackStuckInFlightOnlyAfterAuthenticatedBarrier": True,
                "nodeRouteMustRemainClosedDuringRollback": True,
                "nodeOsLockRequiredDuringRollback": True,
                "noBarrierAction": "leave-stuck-in-flight-withheld",
            },
        )
        self.assertTrue(crash["cleanupRequiredBeforeResponse"])
        self.assertEqual(
            crash["normalCleanupVerification"],
            [
                "direct-child-reaped",
                "cgroup.events:populated=0",
                "launcher-pidfd-and-namespace-fds-closed",
                "leaf-removed",
            ],
        )
        self.assertEqual(
            crash["startupRecoveryVerification"],
            [
                "cgroup.events:populated=0",
                "cgroup.procs-and-cgroup.threads-empty",
                "leaf-removed",
            ],
        )
        self.assertNotIn(
            "namespace-reference-count=0",
            crash["normalCleanupVerification"]
            + crash["startupRecoveryVerification"],
        )
        self.assertNotIn(
            "production launcher binary, closed request format/authentication, "
            "installation ownership, dedicated UID/GID, minimal capability set and "
            "crash-recovery protocol are still open",
            spec,
        )

        outcomes = policy["outcomeMapping"]
        self.assertEqual(outcomes["authority"], "boole-node")
        self.assertTrue(outcomes["terminalOutcomesPersistEvidence"])
        self.assertTrue(outcomes["terminalOutcomesConsumeChallenge"])
        self.assertFalse(outcomes["retryablePersistsEvidence"])
        self.assertFalse(outcomes["retryableConsumesChallenge"])
        self.assertEqual(
            outcomes["default"],
            {
                "outcome": "RetryableUnavailable",
                "reason": "containment_environment_unavailable",
                "persistEvidence": False,
                "consumeChallenge": False,
            },
        )
        self.assertEqual(
            outcomes["orderedRules"],
            [
                {
                    "when": "missing-malformed-binding-invalid-report-or-setup-failure-before-checker-exec",
                    "outcome": "RetryableUnavailable",
                    "reason": "containment_environment_unavailable",
                },
                {
                    "when": "outer-timeout-or-valid-checker-resource_wall_limit",
                    "outcome": "RetryableUnavailable",
                    "reason": "containment_wall_clock_kill",
                },
                {
                    "when": "signaled-or-valid-checker-resource_process_terminated",
                    "outcome": "RetryableUnavailable",
                    "reason": "containment_killed",
                },
                {
                    "when": "valid-checker-resource_process_limit-and-pids-events-max-positive",
                    "outcome": "DeterministicReject",
                    "reason": "submission_resource_ceiling_breach",
                },
                {
                    "when": "valid-checker-resource_memory_limit-and-memory-events-max-positive",
                    "outcome": "DeterministicReject",
                    "reason": "submission_resource_ceiling_breach",
                },
                {
                    "when": "valid-checker-resource_process_limit-or-resource_memory_limit-without-matching-positive-counter",
                    "outcome": "DeterministicReject",
                    "reason": "checker_reported_reason_unconfirmed",
                },
                {
                    "when": "valid-checker-accepted",
                    "outcome": "ACCEPT",
                    "reason": "accepted",
                },
                {
                    "when": "valid-checker-deterministic_reject",
                    "outcome": "DeterministicReject",
                    "reason": "checker_rejected",
                },
                {
                    "when": "valid-checker-checker_internal_error",
                    "outcome": "RetryableUnavailable",
                    "reason": "checker_internal_error",
                },
                {
                    "when": "any-other-valid-checker-retryable_unavailable",
                    "outcome": "RetryableUnavailable",
                    "reason": "containment_environment_unavailable",
                },
            ],
        )

    def test_shared_uid_process_limit_is_neither_applied_nor_claimed(self) -> None:
        checker_source = (CHECKER / "checker.py").read_text(encoding="utf-8")
        policy = json.loads((CHECKER / "policy.json").read_text(encoding="utf-8"))
        release = json.loads(
            (CHECKER / "RELEASE-MANIFEST.json").read_text(encoding="utf-8")
        )
        self.assertNotRegex(
            checker_source, r"setrlimit\(\s*resource\.RLIMIT_NPROC"
        )
        self.assertNotIn("processes", policy["resourceLimits"])
        self.assertEqual(
            release["containment"],
            {
                "linuxAddressSpaceLimit": True,
                "linuxProcessCountLimit": False,
                "macosAddressSpaceLimit": False,
                "macosProcessCountLimit": False,
                "productionQualified": False,
            },
        )

    def test_infrastructure_failures_are_not_semantic_rejections(self) -> None:
        module_path = CHECKER / "checker.py"
        spec = importlib.util.spec_from_file_location(
            "boole_native_checker_failure_classification", module_path
        )
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        try:
            spec.loader.exec_module(module)
            policy = module._load_policy()
            contract = module._load_contract(FIXTURE / "task.json")
            submission = module._verify_submission(
                policy, contract, FIXTURE / "accepted.rs"
            )
            cases = (
                (
                    101,
                    (
                        b"= note: terminate called after throwing an instance of "
                        b"'std::system_error'\n"
                        b"what(): Resource temporarily unavailable"
                    ),
                    module.AuthorityUnavailable,
                    "resource_process_limit",
                ),
                (
                    101,
                    b"fatal error: cannot allocate memory",
                    module.AuthorityUnavailable,
                    "resource_memory_limit",
                ),
                (
                    -9,
                    b"",
                    module.AuthorityUnavailable,
                    "resource_process_terminated",
                ),
                (
                    101,
                    b"test failed: assertion left == right",
                    module.SubmissionRejected,
                    "compile_or_hidden_test_failed",
                ),
                (
                    101,
                    b'error: source contains "Resource temporarily unavailable"',
                    module.SubmissionRejected,
                    "compile_or_hidden_test_failed",
                ),
                (
                    101,
                    (
                        b"error[E0308]: mismatched types\n"
                        b"1 | let _: u8 = \"terminate called after throwing\";\n"
                        b"2 | let _: u8 = \"Resource temporarily unavailable\";"
                    ),
                    module.SubmissionRejected,
                    "compile_or_hidden_test_failed",
                ),
                (
                    101,
                    b'1 | let _: u8 = "fatal error: cannot allocate memory";',
                    module.SubmissionRejected,
                    "compile_or_hidden_test_failed",
                ),
            )
            for code, output, error_type, reason_code in cases:
                with self.subTest(code=code, output=output):
                    with tempfile.TemporaryDirectory(
                        prefix="boole-native-failure-classification-"
                    ) as scratch:
                        with mock.patch.object(
                            module, "_run_contained", return_value=(code, output)
                        ):
                            with self.assertRaises(error_type) as raised:
                                module._judge(
                                    policy,
                                    contract,
                                    submission,
                                    pathlib.Path("/toolchain/bin"),
                                    pathlib.Path(scratch),
                                )
                    self.assertEqual(raised.exception.reason_code, reason_code)
            with tempfile.TemporaryDirectory(
                prefix="boole-native-success-classification-"
            ) as scratch:
                with mock.patch.object(
                    module,
                    "_run_contained",
                    return_value=(
                        0,
                        (
                            b"= note: terminate called after throwing an instance of "
                            b"'std::system_error'\n"
                            b"what(): Resource temporarily unavailable"
                        ),
                    ),
                ):
                    module._judge(
                        policy,
                        contract,
                        submission,
                        pathlib.Path("/toolchain/bin"),
                        pathlib.Path(scratch),
                    )
        finally:
            sys.modules.pop(spec.name, None)

    def test_hidden_seed_derivation_matches_the_sealed_family_contract(self) -> None:
        module_path = CHECKER / "checker.py"
        spec = importlib.util.spec_from_file_location("boole_native_checker", module_path)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        try:
            spec.loader.exec_module(module)
        finally:
            sys.modules.pop(spec.name, None)

        task = json.loads((FIXTURE / "task.json").read_text(encoding="utf-8"))
        commitment = module._domain_hash(
            "acfr-v1/commitment",
            task["checkerTaskId"],
            (FIXTURE / "accepted.rs").read_bytes(),
        )
        self.assertEqual(
            commitment,
            "735e2ee876b17b5e03cb1620583d4cbd8da921bf52660e6a66cc763f16ec7dc2",
        )
        test_seed = module._domain_hash(
            "acfr-v1/test-seed", task["taskSeed"], commitment
        )
        self.assertEqual(
            test_seed,
            "6b05685cdf7aa7adeb2cb626b58e4c7932996d32731ffb09f543df35df42dcee",
        )
        stream = module._SplitMix64(test_seed)
        self.assertEqual(
            [stream.next() for _ in range(6)],
            [
                5856373129791052970,
                17084724835982486404,
                460178006474723195,
                784214485967479434,
                7348096430732716692,
                7320932845687228871,
            ],
        )

    def test_bundle_is_complete_and_contains_no_private_authority(self) -> None:
        self.assertTrue(CHECKER.is_dir(), "tracked checker release is missing")
        self.assertTrue(FIXTURE.is_dir(), "tracked public fixture is missing")
        self.assertTrue(REGISTRY.is_file(), "tracked strict registry is missing")

        forbidden = (
            b"/Users/",
            b"local-docs/",
            b"raw_final_reply",
            b"reference_patch",
            b"contestant_audit",
            b"session_id",
            b"chat_id",
        )
        for root in (CHECKER, FIXTURE):
            sums = parse_sums(root / "SHA256SUMS")
            actual: dict[str, pathlib.Path] = {}
            for candidate in sorted(root.rglob("*")):
                self.assertFalse(candidate.is_symlink(), f"symlink forbidden: {candidate}")
                if candidate.is_file() and candidate.name != "SHA256SUMS":
                    actual[candidate.relative_to(root).as_posix()] = candidate
            self.assertEqual(set(sums), set(actual), "SHA256SUMS must cover every file")
            for rel, candidate in actual.items():
                self.assertEqual(sums[rel], sha256(candidate), f"digest drift: {rel}")
                data = candidate.read_bytes()
                for token in forbidden:
                    self.assertNotIn(token, data, f"private authority token in {candidate}")

        registry_data = REGISTRY.read_bytes()
        for token in forbidden:
            self.assertNotIn(token, registry_data, "private authority token in registry")
        registry = json.loads(REGISTRY.read_text(encoding="utf-8"))
        self.assertEqual(
            set(registry),
            {
                "schema",
                "version",
                "activationAllowed",
                "purpose",
                "executionPolicySha256",
                "toolchainIdentitySha256",
                "templates",
            },
        )
        self.assertEqual(registry["schema"], "boole.native-shadow.registry.v1")
        self.assertEqual(registry["version"], "NATIVE-SHADOW-QUALIFICATION-REGISTRY-V1")
        self.assertEqual(registry["purpose"], "clean-checkout checker qualification only")
        self.assertFalse(registry["activationAllowed"])
        self.assertEqual(len(registry["templates"]), 1)
        self.assertTrue(registry["templates"][0]["nonIssuable"])

        release = json.loads((CHECKER / "RELEASE-MANIFEST.json").read_text(encoding="utf-8"))
        self.assertEqual(
            set(release),
            {
                "schema",
                "release",
                "familyVersion",
                "activationAllowed",
                "checkerArtifactHash",
                "checkerSha256",
                "policySha256",
                "toolchain",
                "qualificationFixture",
                "containment",
            },
        )
        self.assertEqual(release["schema"], "boole.native-checker.release.v1")
        self.assertEqual(release["release"], "RUST-TUPLE-STRUCT-CHECKER-V1-QUALIFICATION")
        self.assertFalse(release["activationAllowed"])
        self.assertEqual(release["checkerArtifactHash"], checker_artifact_hash())
        self.assertEqual(release["checkerSha256"], sha256(CHECKER / "checker.py"))
        self.assertEqual(release["policySha256"], sha256(CHECKER / "policy.json"))
        self.assertEqual(
            release["qualificationFixture"],
            {
                "path": "fixtures/native-shadow/rust-tuple-struct-project-v1/task.json",
                "taskSha256": sha256(FIXTURE / "task.json"),
                "nonIssuable": True,
            },
        )
        self.assertEqual(
            release["containment"],
            {
                "linuxAddressSpaceLimit": True,
                "linuxProcessCountLimit": False,
                "macosAddressSpaceLimit": False,
                "macosProcessCountLimit": False,
                "productionQualified": False,
            },
        )
        policy = json.loads((CHECKER / "policy.json").read_text(encoding="utf-8"))
        self.assertEqual(
            release["toolchain"],
            {
                "channel": policy["toolchain"]["channel"],
                "rustcCommitHash": policy["toolchain"]["rustcCommitHash"],
                "cargoCommitHash": policy["toolchain"]["cargoCommitHash"],
                "source": (
                    "https://ci-artifacts.rust-lang.org/rustc-builds/"
                    "e7795af6d2449fb05a6393c3320ced873a999eb3/"
                ),
                "linuxX8664ArtifactSha256": {
                    "rustc-nightly-x86_64-unknown-linux-gnu.tar.xz": (
                        "12cd470422b39da22a7b8c2f069c25e66200d5a46c1be5dac0bfe7620ed0d415"
                    ),
                    "rust-std-nightly-x86_64-unknown-linux-gnu.tar.xz": (
                        "fd04194fb361ef69735a0b722fcaf6d9b49a339944f485aebcc4c172adb5c339"
                    ),
                    "cargo-nightly-x86_64-unknown-linux-gnu.tar.xz": (
                        "53e718c828a16746abdf3f8fb6f4c75ce5494a6f547ef6f02d45d72faef4c426"
                    ),
                },
                "compilerBinariesIncluded": False,
            },
        )
        template = registry["templates"][0]
        self.assertEqual(
            set(template),
            {
                "familyVersion",
                "templateId",
                "semanticLocator",
                "anchorSha256",
                "taskPath",
                "taskSha256",
                "checkerRelease",
                "checkerReleaseManifestSha256",
                "checkerArtifactHash",
                "policySha256",
                "toolchainChannel",
                "intakeVersion",
                "challengeSha256",
                "epoch",
                "nonIssuable",
            },
        )
        self.assertEqual(template["checkerArtifactHash"], checker_artifact_hash())
        self.assertEqual(template["taskSha256"], sha256(FIXTURE / "task.json"))
        task = json.loads((FIXTURE / "task.json").read_text(encoding="utf-8"))
        provenance = json.loads(
            (FIXTURE / "PROVENANCE.json").read_text(encoding="utf-8")
        )
        self.assertEqual(release["familyVersion"], task["familyVersion"])
        self.assertEqual(policy["release"], release["release"])
        self.assertEqual(provenance["nonIssuable"], task["nonIssuable"])
        self.assertEqual(template["familyVersion"], task["familyVersion"])
        self.assertEqual(template["templateId"], task["templateId"])
        self.assertEqual(template["semanticLocator"], task["anchor"]["semanticLocator"])
        self.assertEqual(template["anchorSha256"], task["anchor"]["sha256"])
        self.assertEqual(template["taskPath"], "rust-tuple-struct-project-v1/task.json")
        self.assertEqual(template["checkerRelease"], release["release"])
        self.assertEqual(
            template["checkerReleaseManifestSha256"],
            sha256(CHECKER / "RELEASE-MANIFEST.json"),
        )
        self.assertEqual(template["policySha256"], release["policySha256"])
        self.assertEqual(template["toolchainChannel"], release["toolchain"]["channel"])
        self.assertEqual(
            template["intakeVersion"],
            "RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1",
        )
        self.assertEqual(template["challengeSha256"], task["challengeSha256"])
        self.assertEqual(template["epoch"], 0)
        self.assertEqual(template["nonIssuable"], task["nonIssuable"])

    def test_tracked_checker_reproduces_public_semantics_without_local_docs(self) -> None:
        toolchain = resolve_toolchain_bin()
        checker = CHECKER / "checker.py"
        task = FIXTURE / "task.json"
        task_contract = json.loads(task.read_text(encoding="utf-8"))
        task_digest = sha256(task)

        expected = {
            "accepted.rs": ("accepted", "accepted"),
            "constant.rs": ("deterministic_reject", "compile_or_hidden_test_failed"),
            "filesystem_probe.rs": ("deterministic_reject", "forbidden_construct"),
            "scanner_confusion.rs": ("deterministic_reject", "forbidden_construct"),
            "tampered.rs": ("deterministic_reject", "outside_patch_modified"),
            "empty.rs": ("deterministic_reject", "malformed_patch_region"),
        }
        with tempfile.TemporaryDirectory(prefix="boole-native-authority-test-") as scratch:
            for answer, (verdict, reason_code) in expected.items():
                proc = subprocess.run(
                    [
                        "python3",
                        str(checker),
                        "--task",
                        str(task),
                        "--submission",
                        str(FIXTURE / answer),
                        "--toolchain-bin",
                        str(toolchain),
                        "--scratch-root",
                        scratch,
                    ],
                    cwd=ROOT,
                    check=False,
                    capture_output=True,
                    text=True,
                    env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"},
                )
                self.assertEqual(proc.returncode, 0, proc.stderr or proc.stdout)
                result = json.loads(proc.stdout)
                self.assertEqual(result["verdict"], verdict, (answer, result))
                self.assertEqual(result["reasonCode"], reason_code, (answer, result))
                self.assertEqual(result["checkerTaskId"], task_contract["checkerTaskId"])
                self.assertEqual(result["taskDigest"], task_digest)
                self.assertNotIn("local-docs", json.dumps(result, sort_keys=True))

    def test_wrong_toolchain_is_unavailable_not_a_semantic_reject(self) -> None:
        checker = CHECKER / "checker.py"
        proc = subprocess.run(
            [
                "python3",
                str(checker),
                "--task",
                str(FIXTURE / "task.json"),
                "--submission",
                str(FIXTURE / "accepted.rs"),
                "--toolchain-bin",
                str(pathlib.Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()) / "bin"),
            ],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr or proc.stdout)
        result = json.loads(proc.stdout)
        self.assertEqual(result["verdict"], "retryable_unavailable")
        self.assertEqual(result["reasonCode"], "toolchain_identity_mismatch")


if __name__ == "__main__":
    unittest.main()
