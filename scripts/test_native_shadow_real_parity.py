#!/usr/bin/env python3
"""Real frozen-accept parity contract for the tracked native checker.

This test proves, from Git-tracked files only, that the answer-free tracked
`RUST-TUPLE-STRUCT-CHECKER-V1` release independently reaches ACCEPT on the one
real historical model candidate recorded by census Entries 27/28, and REJECTs
the same task under an empty answer, a one-value mutation of the real answer,
a constant answer and cross-task binding. It does not depend on the
gitignored private experiment archive, a developer home directory, or a
stored mining answer beyond the frozen, permanently non-issuable fixture
tracked here.

This is `REAL-FROZEN-ACCEPT-PARITY-V1`. It closes one prerequisite of
`docs/native-submission-shadow-verification-v1.md` section 4 (the tracked
checker reproducing the frozen real ACCEPT case). It does not implement the
`boole-node` route, does not enable activation, and does not change
`mineable_now`.

Deliberately self-contained (no import of sibling test modules): this file
must keep working whether it is run alone or alongside the other
`scripts/test_native_shadow_*.py` modules in one `python3 -m unittest`
invocation, regardless of how each gets resolved to a module name.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER = ROOT / "native" / "checker" / "rust-tuple-struct-project-v1"
FIXTURE = ROOT / "fixtures" / "native-shadow" / "a-rooted-native-mining-e2e-v1-real-history"
PUBLIC_FIXTURE = ROOT / "fixtures" / "native-shadow" / "rust-tuple-struct-project-v1"
SELF_TEST = ROOT / "scripts" / "self-test.sh"

FORBIDDEN_TOKENS = (
    b"/Users/",
    b"local-docs/",
    b"raw_final_reply",
    b"reference_patch",
    b"contestant_audit",
    b"session_id",
    b"chat_id",
)


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


def _run_checker(task: pathlib.Path, submission: pathlib.Path, toolchain: pathlib.Path,
                  scratch: str) -> dict:
    proc = subprocess.run(
        [
            "python3",
            str(CHECKER / "checker.py"),
            "--task",
            str(task),
            "--submission",
            str(submission),
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
    if proc.returncode != 0:
        raise AssertionError(proc.stderr or proc.stdout)
    return json.loads(proc.stdout)


class NativeShadowRealParityTests(unittest.TestCase):
    maxDiff = None

    def test_clean_ci_gate_runs_this_parity_test(self) -> None:
        self.assertIn(
            "scripts/test_native_shadow_real_parity.py",
            SELF_TEST.read_text(encoding="utf-8"),
            "the real frozen-accept parity test must remain in the required self-test gate",
        )

    def test_bundle_is_complete_and_contains_no_private_authority(self) -> None:
        self.assertTrue(FIXTURE.is_dir(), "real-history fixture is missing")
        sums = parse_sums(FIXTURE / "SHA256SUMS")
        actual: dict[str, pathlib.Path] = {}
        for candidate in sorted(FIXTURE.rglob("*")):
            self.assertFalse(candidate.is_symlink(), f"symlink forbidden: {candidate}")
            if candidate.is_file() and candidate.name != "SHA256SUMS":
                actual[candidate.relative_to(FIXTURE).as_posix()] = candidate
        self.assertEqual(set(sums), set(actual), "SHA256SUMS must cover every file")
        for rel, candidate in actual.items():
            self.assertEqual(sums[rel], sha256(candidate), f"digest drift: {rel}")
            data = candidate.read_bytes()
            for token in FORBIDDEN_TOKENS:
                self.assertNotIn(token, data, f"private authority token in {candidate}")

        provenance = json.loads((FIXTURE / "PROVENANCE.json").read_text(encoding="utf-8"))
        self.assertTrue(provenance["nonIssuable"])
        self.assertFalse(provenance["activationAllowed"])
        self.assertTrue(provenance["containsRealMiningAnswer"])
        self.assertFalse(provenance["containsWitness"])
        self.assertFalse(provenance["containsModelTranscript"])
        for value in provenance["publishabilityCheck"].values():
            self.assertTrue(value, provenance["publishabilityCheck"])

        task = json.loads((FIXTURE / "task.json").read_text(encoding="utf-8"))
        self.assertTrue(task["nonIssuable"])

        parity = json.loads((FIXTURE / "FROZEN-PARITY.json").read_text(encoding="utf-8"))
        self.assertEqual(parity["trackedTaskIdentity"]["templateId"], task["templateId"])
        self.assertEqual(
            parity["trackedTaskIdentity"]["challengeSha256"], task["challengeSha256"]
        )
        self.assertEqual(
            parity["trackedTaskIdentity"]["taskSha256"], sha256(FIXTURE / "task.json")
        )
        self.assertEqual(parity["historicalRecord"]["anchorSha256"], task["anchor"]["sha256"])
        self.assertEqual(parity["historicalRecord"]["taskSeed"], task["taskSeed"])
        self.assertEqual(
            parity["trackedCheckerIdentity"]["checkerArtifactHash"], checker_artifact_hash()
        )
        self.assertEqual(
            parity["trackedCheckerIdentity"]["checkerSha256"], sha256(CHECKER / "checker.py")
        )
        self.assertEqual(
            parity["trackedCheckerIdentity"]["policySha256"], sha256(CHECKER / "policy.json")
        )

    def test_tracked_checker_reproduces_the_frozen_real_accept_and_negative_controls(
        self,
    ) -> None:
        toolchain = resolve_toolchain_bin()
        task = FIXTURE / "task.json"
        parity = json.loads((FIXTURE / "FROZEN-PARITY.json").read_text(encoding="utf-8"))
        task_digest = sha256(task)

        with tempfile.TemporaryDirectory(prefix="boole-native-real-parity-test-") as scratch:
            for answer, expected in parity["expectedVerdicts"].items():
                result = _run_checker(task, FIXTURE / answer, toolchain, scratch)
                self.assertEqual(result["verdict"], expected["verdict"], (answer, result))
                self.assertEqual(
                    result["reasonCode"], expected["reasonCode"], (answer, result)
                )
                self.assertEqual(result["checkerTaskId"], "real-frozen-accept-parity-v1")
                self.assertEqual(result["taskDigest"], task_digest)
                for token in FORBIDDEN_TOKENS:
                    self.assertNotIn(token, json.dumps(result).encode("utf-8"))

            # This is the normalized parity check against the frozen sealed
            # verdict: the sealed system said "ACCEPT / run", the tracked
            # checker must say "accepted / accepted" for the same real
            # candidate under its own real task identity.
            accept_normalized = parity["normalizedParity"]["trackedAccept"].split(" / ")
            self.assertEqual(
                parity["expectedVerdicts"]["accepted.rs"],
                {"verdict": accept_normalized[0], "reasonCode": accept_normalized[1]},
            )

            binding = parity["crossTaskBinding"]["realAcceptedAgainstSyntheticPublicFixtureTask"]
            result = _run_checker(
                PUBLIC_FIXTURE / "task.json", FIXTURE / "accepted.rs", toolchain, scratch
            )
            self.assertEqual(result["verdict"], binding["verdict"], result)
            self.assertEqual(result["reasonCode"], binding["reasonCode"], result)

            binding = parity["crossTaskBinding"]["syntheticAcceptedAgainstThisRealTask"]
            result = _run_checker(task, PUBLIC_FIXTURE / "accepted.rs", toolchain, scratch)
            self.assertEqual(result["verdict"], binding["verdict"], result)
            self.assertEqual(result["reasonCode"], binding["reasonCode"], result)


if __name__ == "__main__":
    unittest.main()
