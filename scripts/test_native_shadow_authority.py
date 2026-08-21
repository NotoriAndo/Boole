#!/usr/bin/env python3
"""Tracked native-checker authority bundle contract.

This test deliberately uses only Git-tracked paths.  A passing run proves the
qualification checker and its public, non-issuable fixture do not depend on the
gitignored experiment archive, a developer home directory, or a stored mining
answer.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER = ROOT / "native" / "checker" / "rust-tuple-struct-project-v1"
FIXTURE = ROOT / "fixtures" / "native-shadow" / "rust-tuple-struct-project-v1"
REGISTRY = ROOT / "fixtures" / "native-shadow" / "registry-v1.json"
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


def qualification_failure_diagnostic(toolchain: pathlib.Path) -> str:
    """Re-run only the public fixture and expose bounded compiler diagnostics.

    The production checker intentionally returns stable reason codes rather than
    compiler output.  If clean-CI qualification unexpectedly fails, this helper
    captures at most the frozen output ceiling from the synthetic, non-issuable
    fixture so the failure can be diagnosed without changing checker semantics.
    """
    module_path = CHECKER / "checker.py"
    spec = importlib.util.spec_from_file_location(
        "boole_native_checker_diagnostic", module_path
    )
    if spec is None or spec.loader is None:
        return "checker import unavailable"
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    captured: list[tuple[int, bytes]] = []
    try:
        spec.loader.exec_module(module)
        policy = module._load_policy()
        contract = module._load_contract(FIXTURE / "task.json")
        submission = module._verify_submission(
            policy, contract, FIXTURE / "accepted.rs"
        )
        original_run = module._run_contained

        def capture_run(*args, **kwargs):
            result = original_run(*args, **kwargs)
            captured.append(result)
            return result

        module._run_contained = capture_run
        with tempfile.TemporaryDirectory(
            prefix="boole-native-authority-diagnostic-"
        ) as scratch:
            try:
                module._judge(
                    policy,
                    contract,
                    submission,
                    toolchain,
                    pathlib.Path(scratch),
                )
            except (module.AuthorityUnavailable, module.SubmissionRejected) as exc:
                exception = f"{type(exc).__name__}: {exc}"
            else:
                exception = "no exception"
    except Exception as exc:  # diagnostic path must not mask the primary failure
        return f"diagnostic failed: {type(exc).__name__}: {exc}"
    finally:
        sys.modules.pop(spec.name, None)
    if not captured:
        return exception + "; no contained output captured"
    code, output = captured[-1]
    text = output.decode("utf-8", errors="replace")
    return f"{exception}; exit={code}; output={text[-8192:]}"


class NativeShadowAuthorityTests(unittest.TestCase):
    maxDiff = None

    def test_clean_ci_gate_runs_this_authority_test(self) -> None:
        self.assertIn(
            "scripts/test_native_shadow_authority.py",
            SELF_TEST.read_text(encoding="utf-8"),
            "the tracked checker authority test must remain in the required self-test gate",
        )

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
            {"schema", "version", "activationAllowed", "purpose", "templates"},
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
                "linuxKernelResourceLimits": True,
                "macosAddressSpaceAndProcessCountLimits": False,
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
                "checkerArtifactHash",
                "policySha256",
                "toolchainChannel",
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
        self.assertEqual(template["policySha256"], release["policySha256"])
        self.assertEqual(template["toolchainChannel"], release["toolchain"]["channel"])
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
                diagnostic = None
                if result["verdict"] != verdict or result["reasonCode"] != reason_code:
                    diagnostic = qualification_failure_diagnostic(toolchain)
                self.assertEqual(
                    result["verdict"], verdict, (answer, result, diagnostic)
                )
                self.assertEqual(
                    result["reasonCode"], reason_code, (answer, result, diagnostic)
                )
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
