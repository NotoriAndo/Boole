#!/usr/bin/env python3
"""Contract tests for host-independent native-shadow rootfs authority v2."""

from __future__ import annotations

import copy
import hashlib
import pathlib
import tempfile
import unittest

from scripts import native_shadow_rootfs_portable_v2 as portable
from scripts import native_shadow_rootfs_acquire as acquire
from scripts import native_shadow_rootfs_builder as rootfs


ROOT = pathlib.Path(__file__).resolve().parents[1]
PORTABLE_PLAN = ROOT / "native/containment/native-shadow-runtime-rootfs-portable-plan-v2.json"
PORTABLE_RESOLUTION = ROOT / "native/containment/native-shadow-runtime-rootfs-resolution-v2.json"
PORTABLE_LOCK = ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-v2.json"
REPLAY_EXPECTATION = ROOT / "native/containment/native-shadow-runtime-rootfs-replay-expectation-v2.json"


def _v1_candidate(gpgv_path: str, gpgv_sha256: str, zstd_path: str, zstd_sha256: str):
    return {
        "schema": "boole.native-shadow.runtime-rootfs-source-lock.v1",
        "release": "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-COMPLETE-NOT-ACTIVATABLE",
        "activationAllowed": False,
        "ubuntu": {
            "verification": {
                "gpgvPath": gpgv_path,
                "gpgvSha256": gpgv_sha256,
            }
        },
        "buildRecipe": {
            "zstdPath": zstd_path,
            "zstdSha256": zstd_sha256,
        },
    }


class NativeShadowRootfsPortableV2Tests(unittest.TestCase):
    def test_portable_successor_supplies_runtime_loader_aliases(self) -> None:
        candidate = _v1_candidate(
            "/ignored/gpgv",
            "a" * 64,
            "/ignored/zstd",
            "b" * 64,
        )
        candidate["derivedEntries"] = []

        portable_lock = portable.portable_source_lock_from_v1(candidate)

        self.assertEqual(
            portable_lock["derivedEntries"],
            [
                {
                    "logicalPath": "/lib",
                    "kind": "symlink",
                    "target": "usr/lib",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/lib64",
                    "kind": "symlink",
                    "target": "usr/lib64",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/bin/as",
                    "kind": "symlink",
                    "target": "x86_64-linux-gnu-as",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/bin/ld",
                    "kind": "symlink",
                    "target": "x86_64-linux-gnu-ld",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/lib/x86_64-linux-gnu/libLLVM.so.22.1-rust-1.99.0-nightly",
                    "kind": "symlink",
                    "target": "../../../opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/lib/x86_64-linux-gnu/librustc_driver-da0d54ffe246e605.so",
                    "kind": "symlink",
                    "target": "../../../opt/boole/native-checker-toolchain/lib/librustc_driver-da0d54ffe246e605.so",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
            ],
        )
        runtime_lock = copy.deepcopy(portable_lock)
        self.assertEqual(
            portable.runtime_lock_v1_equivalent(runtime_lock)["derivedEntries"],
            [],
        )

        runtime_lock["derivedEntries"][0]["target"] = "wrong/lib64"
        with self.assertRaisesRegex(
            portable.PortableAuthorityError, "successor aliases"
        ):
            portable.runtime_lock_v1_equivalent(runtime_lock)

    def test_linux_replay_installs_the_fixed_qualification_account_before_chroot(self) -> None:
        passwd = (
            ROOT / "native/containment/native-shadow-runtime-passwd-v2"
        ).read_bytes()
        self.assertEqual(
            passwd,
            b"nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n",
        )

        replay = (
            ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
        ).read_text(encoding="utf-8")
        install = (
            'install -m 0444 -o 0 -g 0 "$runtime_passwd" '
            '"$rootfs/etc/passwd"'
        )
        self.assertIn(install, replay)
        self.assertLess(replay.index(install), replay.index("chroot --groups=''"))
        self.assertIn('cmp --silent "$runtime_passwd" "$rootfs/etc/passwd"', replay)

    def test_linux_replay_delegates_checker_adjudication_to_the_real_launcher_service(self) -> None:
        replay = (
            ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
        ).read_text(encoding="utf-8")
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")

        manager_call = "./scripts/native-shadow-manager-cgroup-gate.sh"
        self.assertIn(manager_call, replay)
        self.assertIn("--closed-local-replay-rootfs", replay)
        self.assertIn(
            "native-shadow-closed-local-replay-report:accepted:accepted:accepted:cleanup=true",
            manager,
        )
        self.assertIn(
            "native-shadow-closed-local-replay-report:tampered:deterministic_reject:compile_or_hidden_test_failed:cleanup=true",
            manager,
        )
        self.assertIn(
            "native-shadow-closed-local-replay-report:constant:deterministic_reject:compile_or_hidden_test_failed:cleanup=true",
            manager,
        )

    def test_linux_replay_rejects_rootfs_drift_before_any_checker_report(self) -> None:
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")

        mutation = 'sudo python3 - "$mutation_target"'
        report_guard = "request-time rootfs mutation produced a checker Report"
        drift_reason = "runtime rootfs replay identity drifted"
        restore = 'sudo cp --preserve=all "$mutation_backup" "$mutation_target"'
        self.assertIn(mutation, manager)
        self.assertIn(report_guard, manager)
        self.assertIn(drift_reason, manager)
        self.assertIn(restore, manager)
        mutation_index = manager.index(mutation)
        report_guard_index = manager.index(report_guard, mutation_index)
        restore_index = manager.index(restore, report_guard_index)
        self.assertLess(mutation_index, report_guard_index)
        self.assertLess(report_guard_index, restore_index)

    def test_linux_replay_socket_timeout_preserves_the_service_failure_reason(self) -> None:
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")

        timeout_reason = "fixed qualification socket did not appear"
        timeout_index = manager.index(timeout_reason)
        diagnostic = (
            'sudo systemctl show "$unit_name" '
            '--property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :'
        )
        journal = 'sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :'
        self.assertIn(diagnostic, manager)
        self.assertIn(journal, manager)
        self.assertLess(manager.index(diagnostic), timeout_index)
        self.assertLess(manager.index(journal, manager.index(diagnostic)), timeout_index)

    def test_linux_replay_mounts_a_private_proc_for_the_frozen_lld_wrapper(self) -> None:
        replay = (
            ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
        ).read_text(encoding="utf-8")

        mount_proc = 'mount -t proc -o nosuid,nodev,noexec proc "$rootfs/proc"'
        first_checker = 'chroot --groups=\'\' --userspec=65534:65534 "$rootfs"'
        unmount_proc = 'umount "$rootfs/proc"'
        self.assertIn(mount_proc, replay)
        self.assertIn(unmount_proc, replay)
        self.assertLess(replay.index(mount_proc), replay.index(first_checker))
        self.assertLess(replay.index(first_checker), replay.index(unmount_proc))

    def test_portable_source_lock_bytes_ignore_runtime_tool_path_and_digest(self) -> None:
        first = _v1_candidate(
            "/host-a/bin/gpgv",
            "a" * 64,
            "/host-a/bin/zstd",
            "b" * 64,
        )
        second = _v1_candidate(
            "/different-host/tools/gpgv",
            "c" * 64,
            "/different-host/tools/zstd",
            "d" * 64,
        )

        first_portable = portable.portable_source_lock_from_v1(copy.deepcopy(first))
        second_portable = portable.portable_source_lock_from_v1(copy.deepcopy(second))

        self.assertEqual(
            rootfs.canonical_json(first_portable),
            rootfs.canonical_json(second_portable),
        )
        self.assertFalse(first_portable["activationAllowed"])
        self.assertEqual(
            first_portable["ubuntu"]["verification"],
            {"toolRole": "gpgv"},
        )
        self.assertEqual(first_portable["buildRecipe"]["zstdToolRole"], "zstd")
        self.assertNotIn("zstdPath", first_portable["buildRecipe"])
        self.assertNotIn("zstdSha256", first_portable["buildRecipe"])

    def test_runtime_tools_are_bound_only_in_the_run_receipt(self) -> None:
        portable_lock = portable.portable_source_lock_from_v1(
            _v1_candidate("/ignored/gpgv", "a" * 64, "/ignored/zstd", "b" * 64)
        )
        portable_raw = rootfs.canonical_json(portable_lock)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            gpgv = root / "gpgv"
            zstd = root / "zstd"
            gpgv_raw = b"#!/bin/sh\necho 'gpgv test 1.0'\n"
            zstd_raw = b"#!/bin/sh\necho 'zstd test 2.0'\n"
            gpgv.write_bytes(gpgv_raw)
            zstd.write_bytes(zstd_raw)
            gpgv.chmod(0o500)
            zstd.chmod(0o500)

            runtime_lock, receipt = portable.materialize_runtime_lock(
                portable_lock,
                portable_raw,
                gpgv,
                zstd,
            )

        self.assertEqual(rootfs.canonical_json(portable_lock), portable_raw)
        self.assertEqual(
            receipt["portableSourceLockSha256"], hashlib.sha256(portable_raw).hexdigest()
        )
        self.assertEqual(receipt["tools"]["gpgv"]["path"], str(gpgv.resolve()))
        self.assertEqual(
            receipt["tools"]["gpgv"]["sha256"],
            hashlib.sha256(gpgv_raw).hexdigest(),
        )
        self.assertEqual(receipt["tools"]["gpgv"]["version"], "gpgv test 1.0")
        self.assertEqual(receipt["tools"]["zstd"]["path"], str(zstd.resolve()))
        self.assertEqual(receipt["tools"]["zstd"]["version"], "zstd test 2.0")
        self.assertFalse(receipt["activationAllowed"])
        self.assertEqual(runtime_lock["schema"], rootfs.LOCK_SCHEMA)
        self.assertEqual(runtime_lock["ubuntu"]["verification"]["gpgvPath"], str(gpgv.resolve()))
        self.assertEqual(runtime_lock["buildRecipe"]["zstdPath"], str(zstd.resolve()))
        self.assertNotIn(str(gpgv.resolve()), portable_raw.decode("utf-8"))
        self.assertNotIn(str(zstd.resolve()), portable_raw.decode("utf-8"))

    def test_tracked_portable_successor_is_exact_inactive_and_host_independent(self) -> None:
        authority = portable.load_authority_set(
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
            ROOT / "scripts/native_shadow_rootfs_builder.py",
        )

        self.assertFalse(authority["plan"]["activationAllowed"])
        self.assertFalse(authority["sourceLock"]["activationAllowed"])
        self.assertFalse(authority["expectation"]["activationAllowed"])
        self.assertFalse(
            authority["expectation"]["productionByteProvenanceComplete"]
        )
        self.assertEqual(
            authority["plan"]["bootstrapAuthority"]["acquisitionPlanV1Sha256"],
            "09b1bb420c3c2317872e8408384b388d4fc8909554af15e759f12684d23db1c8",
        )
        self.assertEqual(
            authority["plan"]["bootstrapAuthority"]["completeSourceLockV1Sha256"],
            "40880be22275155346dab292644943d06817f08f90bb9dee592659aa1fe0588c",
        )
        forbidden = (
            b"/opt/homebrew/",
            b"/Users/",
            b"f1c71affd4ce40e3c5a53b8cb0ac9601fbcd31d6834b732dd0c7b0145dce1995",
            b"aff8169fb421bb925fb16c44a7e0143fa2c7a941dc45cce76b15062a2ce54917",
        )
        for path in (
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
        ):
            raw = path.read_bytes()
            for token in forbidden:
                self.assertNotIn(token, raw, f"host tool identity leaked into {path.name}")

    def test_replay_expectation_rejects_output_mismatch_without_adoption(self) -> None:
        authority = portable.load_authority_set(
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
            ROOT / "scripts/native_shadow_rootfs_builder.py",
        )
        build_receipt = portable.expected_build_receipt(authority["expectation"])
        portable.verify_replay_output(authority["expectation"], build_receipt)

        changed = copy.deepcopy(build_receipt)
        changed["layerDigest"] = "sha256:" + "f" * 64
        with self.assertRaisesRegex(portable.PortableAuthorityError, "layerDigest"):
            portable.verify_replay_output(authority["expectation"], changed)

    def test_source_lock_package_bytes_are_cross_bound_to_signed_resolution(self) -> None:
        source_lock = rootfs.load_json_exact(
            PORTABLE_LOCK.read_bytes(), "portable source lock", require_canonical=True
        )
        expectation = rootfs.load_json_exact(
            REPLAY_EXPECTATION.read_bytes(), "replay expectation", require_canonical=True
        )
        deb = next(item for item in source_lock["artifacts"] if item["kind"] == "deb")
        deb["sha256"] = "f" * 64
        changed_raw = rootfs.canonical_json(source_lock)
        expectation["authority"]["portableSourceLockSha256"] = hashlib.sha256(
            changed_raw
        ).hexdigest()

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            changed_lock = root / "source-lock.json"
            changed_expectation = root / "expectation.json"
            changed_lock.write_bytes(changed_raw)
            changed_expectation.write_bytes(rootfs.canonical_json(expectation))
            with self.assertRaisesRegex(
                portable.PortableAuthorityError, "package closure"
            ):
                portable.load_authority_set(
                    PORTABLE_PLAN,
                    PORTABLE_RESOLUTION,
                    changed_lock,
                    changed_expectation,
                    ROOT / "scripts/native_shadow_rootfs_builder.py",
                )

    def test_runtime_resolution_tool_identity_normalizes_to_one_portable_resolution(self) -> None:
        plan = rootfs.load_json_exact(
            PORTABLE_PLAN.read_bytes(), "portable plan", require_canonical=True
        )
        v1_resolution = {
            "schema": acquire.RESOLUTION_SCHEMA,
            "snapshotId": "20240425T160000Z",
            "snapshotTime": "2024-04-25T16:00:00Z",
            "planSha256": "a" * 64,
            "keyring": {"artifactId": "keyring", "sha256": "b" * 64, "sizeBytes": 1},
            "packages": [],
            "seedPackageIds": [],
        }
        second = copy.deepcopy(v1_resolution)
        second["planSha256"] = "c" * 64

        first_portable = portable.portable_resolution_from_runtime(
            v1_resolution,
            plan,
            PORTABLE_PLAN.read_bytes(),
        )
        second_portable = portable.portable_resolution_from_runtime(
            second,
            plan,
            PORTABLE_PLAN.read_bytes(),
        )
        self.assertEqual(
            rootfs.canonical_json(first_portable),
            rootfs.canonical_json(second_portable),
        )
        self.assertEqual(
            first_portable["bootstrapResolutionV1Sha256"],
            plan["bootstrapAuthority"]["signedResolutionV1Sha256"],
        )

    def test_replay_receipts_bind_builder_and_ephemeral_lock_to_portable_authority(self) -> None:
        authority = portable.load_authority_set(
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
            ROOT / "scripts/native_shadow_rootfs_builder.py",
        )
        expectation = authority["expectation"]
        build_receipt = {
            **portable.expected_build_receipt(expectation),
            "activationAllowed": False,
            "productionByteProvenanceComplete": False,
            "builderSha256": expectation["authority"]["builderSha256"],
            "sourceLockSha256": "a" * 64,
        }
        run_receipt = {
            "activationAllowed": False,
            "productionByteProvenanceComplete": False,
            "ephemeralRuntimeLock": True,
            "runtimeLockSha256": "a" * 64,
            "portableSourceLockSha256": expectation["authority"][
                "portableSourceLockSha256"
            ],
            "authority": {
                key: expectation["authority"][key]
                for key in (
                    "builderSha256",
                    "portablePlanSha256",
                    "portableResolutionSha256",
                    "portableSourceLockSha256",
                )
            },
        }
        portable.verify_replay_receipts(expectation, build_receipt, run_receipt)

        changed = copy.deepcopy(build_receipt)
        changed["builderSha256"] = "f" * 64
        with self.assertRaisesRegex(portable.PortableAuthorityError, "builder"):
            portable.verify_replay_receipts(expectation, changed, run_receipt)


if __name__ == "__main__":
    unittest.main()
