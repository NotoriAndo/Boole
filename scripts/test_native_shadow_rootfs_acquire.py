#!/usr/bin/env python3
"""Contract tests for the pinned native-shadow rootfs acquisition tool."""

from __future__ import annotations

import hashlib
import pathlib
import tempfile
import unittest
from unittest import mock
from typing import Optional

from scripts import native_shadow_rootfs_acquire as acquire
from scripts import native_shadow_rootfs_builder as rootfs


ROOT = pathlib.Path(__file__).resolve().parents[1]
PLAN = ROOT / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json"
BUILDER = ROOT / "scripts/native_shadow_rootfs_builder.py"
ACQUIRER = ROOT / "scripts/native_shadow_rootfs_acquire.py"


def sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def stanza(
    name: str,
    version: str,
    *,
    depends: str = "",
    pre_depends: str = "",
    provides: str = "",
    architecture: str = "amd64",
    payload: Optional[bytes] = None,
) -> bytes:
    payload = payload if payload is not None else f"deb:{name}:{version}".encode()
    fields = [
        f"Package: {name}",
        f"Version: {version}",
        f"Architecture: {architecture}",
        f"Filename: pool/main/{name[0]}/{name}/{name}_{version}_{architecture}.deb",
        f"Size: {len(payload)}",
        f"SHA256: {sha(payload)}",
    ]
    if depends:
        fields.append(f"Depends: {depends}")
    if pre_depends:
        fields.append(f"Pre-Depends: {pre_depends}")
    if provides:
        fields.append(f"Provides: {provides}")
    return ("\n".join(fields) + "\n").encode()


class NativeShadowRootfsAcquireTests(unittest.TestCase):
    def test_acquirer_authority_digest_normalizes_only_expected_plan_hash(self) -> None:
        raw = ACQUIRER.read_bytes()
        digest = acquire.acquirer_authority_sha256(raw)
        changed_plan_hash = raw.replace(
            acquire.EXPECTED_PLAN_SHA256.encode("ascii"), b"f" * 64, 1
        )
        self.assertEqual(acquire.acquirer_authority_sha256(changed_plan_hash), digest)
        self.assertNotEqual(acquire.acquirer_authority_sha256(raw + b"# mutation\n"), digest)

    def test_tracked_plan_is_canonical_exact_and_inactive(self) -> None:
        raw = PLAN.read_bytes()
        plan = acquire.load_plan(raw, BUILDER, ACQUIRER)
        self.assertFalse(plan["activationAllowed"])
        self.assertEqual(plan["release"], acquire.EXPECTED_RELEASE)
        self.assertEqual(plan["snapshotId"], "20240425T160000Z")
        self.assertEqual(plan["repository"]["suite"], "noble")
        self.assertEqual(plan["repository"]["component"], "main")
        self.assertEqual(len(plan["rustArtifacts"]), 3)

        for old, new in (
            (b"20240425T160000Z", b"latest"),
            (b"https://snapshot.ubuntu.com", b"http://snapshot.ubuntu.com"),
            (plan["builderSha256"].encode(), b"00" * 32),
        ):
            with self.subTest(new=new[:16]):
                with self.assertRaises(acquire.AcquisitionError):
                    acquire.load_plan(raw.replace(old, new, 1), BUILDER, ACQUIRER)

        with tempfile.TemporaryDirectory() as tmp:
            changed_acquirer = pathlib.Path(tmp) / "native_shadow_rootfs_acquire.py"
            changed_acquirer.write_bytes(ACQUIRER.read_bytes() + b"# unauthorized mutation\n")
            with self.assertRaisesRegex(acquire.AcquisitionError, "acquirer digest"):
                acquire.load_plan(raw, BUILDER, changed_acquirer)

    def test_resolver_is_order_independent_and_prefers_direct_package(self) -> None:
        app = stanza(
            "app",
            "1:2.0-1",
            depends="runtime (>= 2.0) | fallback",
            pre_depends="base",
        )
        direct = stanza("runtime", "2.1-1")
        provider = stanza("provider", "9.0-1", provides="runtime (= 9.0-1)")
        base = stanza("base", "1.0~rc1-2")
        packages = [app, direct, provider, base]
        first = acquire.resolve_package_closure(
            b"\n".join(packages), ["app"], "noble-main", "main"
        )
        second = acquire.resolve_package_closure(
            b"\n".join(reversed(packages)), ["app"], "noble-main", "main"
        )
        self.assertEqual(rootfs.canonical_json(first), rootfs.canonical_json(second))
        names = {item["name"] for item in first["packages"]}
        self.assertEqual(names, {"app", "base", "runtime"})
        self.assertNotIn("provider", names)
        app_row = next(item for item in first["packages"] if item["name"] == "app")
        choices = {
            (item["field"], item["groupIndex"]): item["packageId"]
            for item in app_row["dependencyResolutions"]
        }
        runtime_id = next(item["packageId"] for item in first["packages"] if item["name"] == "runtime")
        self.assertEqual(choices[("Depends", 0)], runtime_id)

    def test_resolver_stops_on_ambiguity_qualifiers_and_unresolved_groups(self) -> None:
        cases = {
            "ambiguous": [
                stanza("app", "1", depends="virtual-runtime"),
                stanza("provider-a", "1", provides="virtual-runtime"),
                stanza("provider-b", "1", provides="virtual-runtime"),
            ],
            "qualifier": [
                stanza("app", "1", depends="runtime:any"),
                stanza("runtime", "1"),
            ],
            "unresolved": [stanza("app", "1", depends="missing")],
        }
        for name, rows in cases.items():
            with self.subTest(name=name), self.assertRaises(acquire.AcquisitionError):
                acquire.resolve_package_closure(
                    b"\n".join(rows), ["app"], "noble-main", "main"
                )

    def test_resolver_reports_malformed_package_size_as_acquisition_error(self) -> None:
        malformed = stanza("app", "1").replace(b"Size: 9", b"Size: not-a-number")
        with self.assertRaisesRegex(acquire.AcquisitionError, "package size"):
            acquire.resolve_package_closure(
                malformed, ["app"], "noble-main", "main"
            )

    def test_local_cas_import_is_atomic_exact_and_rejects_symlinks(self) -> None:
        raw = b"frozen artifact bytes"
        spec = {"sha256": sha(raw), "sizeBytes": len(raw)}
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / "source"
            source.write_bytes(raw)
            cas = root / "cas"
            stored = acquire.import_local_artifact(cas, spec, source)
            self.assertEqual(stored.read_bytes(), raw)
            self.assertEqual(stored, cas / "sha256" / sha(raw))
            self.assertEqual(acquire.import_local_artifact(cas, spec, source), stored)

            wrong = root / "wrong"
            wrong.write_bytes(raw + b"!")
            with self.assertRaises(acquire.AcquisitionError):
                acquire.import_local_artifact(cas, spec, wrong)

            link = root / "link"
            link.symlink_to(source)
            with self.assertRaises(acquire.AcquisitionError):
                acquire.import_local_artifact(cas, spec, link)

    def test_cas_and_canonical_outputs_reject_directory_or_output_symlinks(self) -> None:
        raw = b"authority bytes"
        spec = {"sha256": sha(raw), "sizeBytes": len(raw)}
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / "source"
            source.write_bytes(raw)

            real_cas = root / "real-cas"
            real_cas.mkdir()
            linked_cas = root / "linked-cas"
            linked_cas.symlink_to(real_cas, target_is_directory=True)
            with self.assertRaisesRegex(acquire.AcquisitionError, "CAS"):
                acquire.import_local_artifact(linked_cas, spec, source)

            ancestor_target = root / "ancestor-target"
            ancestor_target.mkdir()
            linked_ancestor = root / "linked-ancestor"
            linked_ancestor.symlink_to(ancestor_target, target_is_directory=True)
            with self.assertRaisesRegex(acquire.AcquisitionError, "CAS"):
                acquire.import_local_artifact(
                    linked_ancestor / "nested-cas", spec, source
                )
            self.assertFalse((ancestor_target / "nested-cas").exists())

            cas = root / "cas"
            cas.mkdir()
            real_sha = root / "real-sha"
            real_sha.mkdir()
            (cas / "sha256").symlink_to(real_sha, target_is_directory=True)
            with self.assertRaisesRegex(acquire.AcquisitionError, "CAS"):
                acquire.import_local_artifact(cas, spec, source)

            output_target = root / "output-target.json"
            output_target.write_text("do not replace", encoding="utf-8")
            output_link = root / "resolution.json"
            output_link.symlink_to(output_target)
            with self.assertRaisesRegex(acquire.AcquisitionError, "symlink"):
                acquire._write_canonical(output_link, {"safe": True})
            self.assertEqual(output_target.read_text(encoding="utf-8"), "do not replace")

            real_output_dir = root / "real-output"
            real_output_dir.mkdir()
            linked_output_dir = root / "linked-output"
            linked_output_dir.symlink_to(real_output_dir, target_is_directory=True)
            with self.assertRaisesRegex(acquire.AcquisitionError, "directory"):
                acquire._write_canonical(linked_output_dir / "result.json", {"safe": True})

            linked_output_ancestor = root / "linked-output-ancestor"
            linked_output_ancestor.symlink_to(
                real_output_dir, target_is_directory=True
            )
            with self.assertRaisesRegex(acquire.AcquisitionError, "directory"):
                acquire._write_canonical(
                    linked_output_ancestor / "nested" / "result.json",
                    {"safe": True},
                )
            self.assertFalse((real_output_dir / "nested").exists())

    def test_artifact_budget_rejects_declared_and_actual_overruns(self) -> None:
        policy = {"maxArtifactBytes": 10, "maxTotalBytes": 15}
        with self.assertRaisesRegex(acquire.AcquisitionError, "maxArtifactBytes"):
            acquire.ArtifactBudget(
                policy, [{"artifactId": "large", "sizeBytes": 11}]
            )
        with self.assertRaisesRegex(acquire.AcquisitionError, "maxTotalBytes"):
            acquire.ArtifactBudget(
                policy,
                [
                    {"artifactId": "first", "sizeBytes": 8},
                    {"artifactId": "second", "sizeBytes": 8},
                ],
            )

        budget = acquire.ArtifactBudget(
            policy,
            [{"artifactId": "next", "sizeBytes": 7}],
            initial_declared_bytes=8,
            initial_actual_bytes=8,
        )
        budget.account(7, "next")
        with self.assertRaisesRegex(acquire.AcquisitionError, "actual.*maxTotalBytes"):
            budget.account(1, "unexpected-extra-byte")

    def test_resolution_replay_requires_canonical_byte_equality(self) -> None:
        canonical = {
            "schema": acquire.RESOLUTION_SCHEMA,
            "packages": [{"packageId": "deb-authority", "depends": "safe"}],
        }
        tampered = {
            "schema": acquire.RESOLUTION_SCHEMA,
            "packages": [{"packageId": "deb-authority", "depends": "tampered"}],
        }
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            resolution_path = root / "RESOLUTION.json"
            resolution_path.write_bytes(rootfs.canonical_json(tampered))
            with mock.patch.object(acquire, "resolve_from_cas", return_value=canonical):
                with self.assertRaisesRegex(acquire.AcquisitionError, "byte-for-byte"):
                    acquire.replay_resolution_from_cas(
                        {},
                        resolution_path,
                        root / "cas",
                        root / "gpgv",
                        root / "zstd",
                    )

            resolution_path.write_bytes(rootfs.canonical_json(canonical))
            with mock.patch.object(acquire, "resolve_from_cas", return_value=canonical):
                self.assertEqual(
                    acquire.replay_resolution_from_cas(
                        {},
                        resolution_path,
                        root / "cas",
                        root / "gpgv",
                        root / "zstd",
                    ),
                    canonical,
                )

    def test_candidate_replay_requires_exact_raw_equality(self) -> None:
        expected = {"schema": "candidate", "release": "qualification", "safe": True}
        tampered = {"schema": "candidate", "release": "qualification", "safe": False}
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            candidate_path = root / "CANDIDATE.json"
            candidate_path.write_bytes(rootfs.canonical_json(tampered))
            with mock.patch.object(
                acquire,
                "seal_candidate",
                return_value=(expected, rootfs.canonical_json(expected)),
            ):
                with self.assertRaisesRegex(acquire.AcquisitionError, "byte-for-byte"):
                    acquire.replay_candidate_lock(
                        {},
                        root / "scaffold.json",
                        {},
                        root / "cas",
                        root,
                        root / "gpgv",
                        root / "zstd",
                        candidate_path,
                    )

            candidate_path.write_bytes(rootfs.canonical_json(expected))
            with mock.patch.object(
                acquire,
                "seal_candidate",
                return_value=(expected, rootfs.canonical_json(expected)),
            ):
                self.assertEqual(
                    acquire.replay_candidate_lock(
                        {},
                        root / "scaffold.json",
                        {},
                        root / "cas",
                        root,
                        root / "gpgv",
                        root / "zstd",
                        candidate_path,
                    ),
                    expected,
                )


if __name__ == "__main__":
    unittest.main()
