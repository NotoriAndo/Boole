#!/usr/bin/env python3
"""RED-first contracts for the ARM64 boot-rootfs dependency candidate."""

from __future__ import annotations

import hashlib
import copy
import contextlib
import io
import json
import pathlib
import sys
import tempfile
import types
import unittest
from unittest import mock

from scripts import native_shadow_boot_rootfs_dependency_candidate_arm64_v1 as candidate


ROOT = pathlib.Path(__file__).resolve().parents[1]
PLAN = ROOT / "native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json"
RESULT = ROOT / "native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json"
BASELINE = ROOT / "native/containment/native-shadow-runtime-rootfs-resolution-arm64-v1.json"
GENERATOR = ROOT / "scripts/native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py"


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def stanza(name: str, *, depends: str = "", size: int = 10) -> bytes:
    payload = (name.encode() + b"-") * size
    fields = [
        f"Package: {name}",
        "Version: 1",
        "Architecture: arm64",
        f"Filename: pool/main/{name[0]}/{name}/{name}_1_arm64.deb",
        f"Size: {len(payload)}",
        f"SHA256: {_sha(payload)}",
    ]
    if depends:
        fields.append(f"Depends: {depends}")
    return ("\n".join(fields) + "\n").encode()


class NativeShadowBootRootfsDependencyCandidateTests(unittest.TestCase):
    def test_candidate_preserves_the_baseline_and_never_claims_boot_authority(self) -> None:
        packages_raw = b"\n".join(
            [stanza("base"), stanza("systemd", depends="base")]
        )
        baseline = candidate.resolve_for_test(packages_raw, ["base"])
        plan = candidate.plan_for_test(
            seeds=["base", "systemd"],
            baseline=baseline,
            expected_package_count=2,
            expected_package_bytes=sum(
                row["artifactSizeBytes"]
                for row in candidate.resolve_for_test(
                    packages_raw, ["base", "systemd"]
                )["packages"]
            ),
        )

        result = candidate.build_candidate(
            plan,
            baseline,
            packages_raw,
            signed_metadata_verified=True,
        )

        self.assertEqual(result["status"], "DEPENDENCY-CANDIDATE-FROZEN-NOT-BOOT-AUTHORITY")
        self.assertEqual(result["counts"]["candidatePackages"], 2)
        self.assertEqual(result["counts"]["baselinePackages"], 1)
        self.assertEqual(result["counts"]["newPackages"], 1)
        self.assertFalse(result["activationAllowed"])
        self.assertFalse(result["bootableClaim"])
        self.assertFalse(result["productionByteProvenanceComplete"])
        self.assertEqual(result["bootArtifactsWritten"], 0)

    def test_unsigned_metadata_or_any_positive_boot_flag_fails_closed(self) -> None:
        packages_raw = stanza("base")
        baseline = candidate.resolve_for_test(packages_raw, ["base"])
        plan = candidate.plan_for_test(
            seeds=["base"],
            baseline=baseline,
            expected_package_count=1,
            expected_package_bytes=baseline["packages"][0]["artifactSizeBytes"],
        )
        with self.assertRaisesRegex(candidate.DependencyCandidateError, "not verified"):
            candidate.build_candidate(
                plan,
                baseline,
                packages_raw,
                signed_metadata_verified=False,
            )
        for field, positive in (
            ("activationAllowed", True),
            ("bootableClaim", True),
            ("bootArtifactsWritten", 1),
        ):
            with self.subTest(field=field):
                changed = copy.deepcopy(plan)
                changed[field] = positive
                with self.assertRaisesRegex(candidate.DependencyCandidateError, "claim"):
                    candidate.build_candidate(
                        changed,
                        baseline,
                        packages_raw,
                        signed_metadata_verified=True,
                    )

    def test_plan_shape_is_exact_and_unknown_fields_fail_closed(self) -> None:
        packages_raw = stanza("base")
        baseline = candidate.resolve_for_test(packages_raw, ["base"])
        plan = candidate.plan_for_test(
            seeds=["base"],
            baseline=baseline,
            expected_package_count=1,
            expected_package_bytes=baseline["packages"][0]["artifactSizeBytes"],
        )
        plan["unexpected"] = "must-stop"
        with self.assertRaisesRegex(candidate.DependencyCandidateError, "keys differ"):
            candidate.build_candidate(
                plan,
                baseline,
                packages_raw,
                signed_metadata_verified=True,
            )

    def test_plan_file_must_be_byte_exact_canonical_json(self) -> None:
        with tempfile.TemporaryDirectory() as raw_directory:
            path = pathlib.Path(raw_directory) / "plan.json"
            path.write_text('{"schema":"wrong"}\n', encoding="utf-8")
            with self.assertRaisesRegex(candidate.DependencyCandidateError, "canonical"):
                candidate.load_plan(path)

    def test_required_package_identity_must_match_the_signed_index_row(self) -> None:
        packages_raw = stanza("base")
        baseline = candidate.resolve_for_test(packages_raw, ["base"])
        plan = candidate.plan_for_test(
            seeds=["base"],
            baseline=baseline,
            expected_package_count=1,
            expected_package_bytes=baseline["packages"][0]["artifactSizeBytes"],
        )
        row = baseline["packages"][0]
        plan["requiredPackages"] = {
            "base": {
                "artifactSha256": "0" * 64,
                "artifactSizeBytes": row["artifactSizeBytes"],
                "packageId": row["packageId"],
                "version": row["version"],
            }
        }
        with self.assertRaisesRegex(candidate.DependencyCandidateError, "required package identity"):
            candidate.build_candidate(
                plan,
                baseline,
                packages_raw,
                signed_metadata_verified=True,
            )

    def test_authority_input_requires_exact_bytes_and_rejects_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = pathlib.Path(raw_directory)
            source = directory / "source.json"
            source.write_bytes(b"authority\n")
            pin = {"sha256": _sha(b"authority\n"), "sizeBytes": 10}
            self.assertEqual(
                candidate.read_pinned_input(source, pin, "test authority"),
                b"authority\n",
            )
            with self.assertRaisesRegex(candidate.DependencyCandidateError, "differ"):
                candidate.read_pinned_input(
                    source,
                    {"sha256": "0" * 64, "sizeBytes": 10},
                    "test authority",
                )
            link = directory / "link.json"
            link.symlink_to(source)
            with self.assertRaisesRegex(candidate.DependencyCandidateError, "without following"):
                candidate.read_pinned_input(link, pin, "test authority")

    def test_runtime_modules_must_be_loaded_from_the_pinned_repository_root(self) -> None:
        plan = candidate.load_plan(PLAN)
        fake = types.ModuleType(
            "scripts.native_shadow_boot_rootfs_resolver_v2"
        )
        fake.VALUE = "poisoned-import-cache"
        with mock.patch.dict(
            sys.modules,
            {"scripts.native_shadow_boot_rootfs_resolver_v2": fake},
        ):
            runtime = candidate.load_pinned_runtime(
                ROOT, plan["authorityInputs"]
            )
        self.assertIsNot(runtime.resolver, fake)
        self.assertEqual(
            pathlib.Path(runtime.resolver.__file__),
            ROOT / "scripts/native_shadow_boot_rootfs_resolver_v2.py",
        )
        with tempfile.TemporaryDirectory() as raw_directory:
            with self.assertRaisesRegex(
                candidate.DependencyCandidateError,
                "running repository root differs",
            ):
                candidate.load_pinned_runtime(
                    pathlib.Path(raw_directory), plan["authorityInputs"]
                )

    def test_cli_rejects_an_output_symlink_without_changing_its_target(self) -> None:
        with tempfile.TemporaryDirectory(dir=ROOT) as raw_directory:
            directory = pathlib.Path(raw_directory)
            target = directory / "target.json"
            target.write_bytes(b"keep-me\n")
            output = directory / "result-link.json"
            output.symlink_to(target)
            with mock.patch.object(candidate, "generate_from_cas", return_value={}):
                with contextlib.redirect_stderr(io.StringIO()):
                    return_code = candidate.main(
                        [
                            "--plan",
                            str(directory / "plan.json"),
                            "--repository-root",
                            str(ROOT),
                            "--cas",
                            str(directory / "cas"),
                            "--gpgv",
                            str(directory / "gpgv"),
                            "--zstd",
                            str(directory / "zstd"),
                            "--output",
                            str(output),
                        ]
                    )
            self.assertEqual(return_code, 1)
            self.assertEqual(target.read_bytes(), b"keep-me\n")

    def test_tracked_candidate_is_canonical_conserved_and_path_independent(self) -> None:
        plan = candidate.load_plan(PLAN)
        generator_raw = GENERATOR.read_bytes()
        self.assertEqual(
            plan["authorityInputs"]["candidateGenerator"],
            {"sha256": _sha(generator_raw), "sizeBytes": len(generator_raw)},
        )
        result_raw = RESULT.read_bytes()
        result = json.loads(result_raw)
        self.assertEqual(candidate.canonical_json(result), result_raw)
        self.assertEqual(result["planSha256"], _sha(candidate.canonical_json(plan)))
        self.assertEqual(result["authorityInputs"], plan["authorityInputs"])
        self.assertEqual(
            result["counts"],
            {
                "baselinePackageBytes": 66_992_762,
                "baselinePackages": 56,
                "candidatePackageBytes": 208_936_876,
                "candidatePackages": 191,
                "newPackageBytes": 141_944_114,
                "newPackages": 135,
            },
        )
        baseline = json.loads(BASELINE.read_bytes())
        baseline_rows = {row["packageId"]: row for row in baseline["packages"]}
        candidate_rows = {
            row["packageId"]: row for row in result["resolution"]["packages"]
        }
        self.assertEqual(len(candidate_rows), 191)
        self.assertEqual(
            {key: candidate_rows[key] for key in baseline_rows},
            baseline_rows,
        )
        self.assertEqual(
            result["newPackageIds"],
            sorted(set(candidate_rows) - set(baseline_rows)),
        )
        self.assertNotIn("/private/tmp", result_raw.decode("utf-8"))
        self.assertFalse(result["bootableClaim"])
        self.assertFalse(result["activationAllowed"])

    def test_self_test_runs_the_dependency_candidate_suite(self) -> None:
        body = (ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        self.assertIn(
            "scripts/test_native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py",
            body,
        )


if __name__ == "__main__":
    unittest.main()
