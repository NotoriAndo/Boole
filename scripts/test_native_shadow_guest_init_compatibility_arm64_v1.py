#!/usr/bin/env python3
"""Contract tests for the Linux/arm64 guest-init compatibility boundary."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

from scripts import native_shadow_guest_init_compatibility_arm64_v1 as guest_init


ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTRACT = (
    ROOT
    / "native/containment/native-shadow-guest-init-compatibility-arm64-v1.json"
)
SOURCE_LOCK = (
    ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
)
SOURCE_CLOSURE = (
    ROOT
    / "native/containment/native-shadow-runtime-rootfs-source-closure-result-arm64-v1.json"
)
POLICY = ROOT / "native/containment/native-shadow-execution-policy-arm64-v1.json"
BOOT_PLAN = (
    ROOT
    / "native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json"
)
UNIT = ROOT / "native/systemd/boole-native-shadow-launcher.service"
SYSUSERS = ROOT / "native/sysusers.d/boole-native-shadow.conf"
TMPFILES = ROOT / "native/tmpfiles.d/boole-native-shadow.conf"


def _canonical(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        indent=2,
        allow_nan=False,
    ).encode("utf-8") + b"\n"


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


class NativeShadowGuestInitCompatibilityArm64V1Tests(unittest.TestCase):
    def _successor_shape_lock(self) -> dict[str, object]:
        lock = json.loads(SOURCE_LOCK.read_bytes())
        lock["ubuntu"]["seeds"] = sorted(lock["ubuntu"]["seeds"] + ["systemd"])
        package_id = "deb-" + "1" * 64
        lock["artifacts"].append(
            {
                "id": package_id,
                "kind": "deb",
                "sha256": "2" * 64,
                "sizeBytes": 1,
            }
        )
        lock["artifacts"].sort(key=lambda row: row["id"])
        systemd = dict(lock["ubuntu"]["packages"][0])
        systemd.update(
            {
                "artifactId": package_id,
                "dependencyResolutions": [],
                "name": "systemd",
                "packageId": package_id,
                "poolPath": "pool/main/s/systemd/systemd_synthetic_arm64.deb",
                "sourceName": "systemd",
            }
        )
        lock["ubuntu"]["packages"].append(systemd)
        lock["ubuntu"]["packages"].sort(key=lambda row: row["packageId"])
        lock["ubuntu"]["seedPackageIds"] = sorted(
            lock["ubuntu"]["seedPackageIds"] + [package_id]
        )
        contract = guest_init.load_contract(CONTRACT)
        for role in contract["trackedFileRequirements"]:
            source_path = f"synthetic/{role['role']}"
            digest = role["sha256"] or "1" * 64
            lock["trackedFiles"].append(
                {
                    "gid": role["gid"],
                    "logicalPath": role["logicalPath"],
                    "mode": role["mode"],
                    "sha256": digest,
                    "sourcePath": source_path,
                    "uid": role["uid"],
                }
            )
            lock["authorityBindings"].append(
                {
                    "id": f"guest-init-{role['role']}",
                    "sha256": digest,
                    "sourcePath": source_path,
                }
            )
        lock["authorityBindings"].sort(key=lambda row: row["id"])
        lock["trackedFiles"].sort(key=lambda row: row["logicalPath"])
        for role in contract["derivedEntryRequirements"]:
            lock["derivedEntries"].append(
                {
                    "gid": 0,
                    "kind": "symlink",
                    "logicalPath": role["logicalPath"],
                    "mode": "0777",
                    "target": role["target"],
                    "uid": 0,
                }
            )
        lock["derivedEntries"].sort(key=lambda row: row["logicalPath"])
        return lock

    def _audit(
        self,
        *,
        contract: pathlib.Path = CONTRACT,
        source_lock: pathlib.Path = SOURCE_LOCK,
        source_closure: pathlib.Path = SOURCE_CLOSURE,
        policy: pathlib.Path = POLICY,
        boot_plan: pathlib.Path = BOOT_PLAN,
        unit: pathlib.Path = UNIT,
        sysusers: pathlib.Path = SYSUSERS,
        tmpfiles: pathlib.Path = TMPFILES,
    ) -> dict[str, object]:
        return guest_init.audit_guest_init(
            contract,
            source_lock,
            source_closure,
            policy,
            boot_plan,
            unit,
            sysusers,
            tmpfiles,
        )

    def test_tracked_contract_is_canonical_non_activatable_and_exactly_bound(self) -> None:
        raw = CONTRACT.read_bytes()
        contract = json.loads(raw)
        self.assertEqual(raw, _canonical(contract))
        self.assertEqual(
            contract["schema"],
            "boole.native-shadow.guest-init-compatibility.arm64.v1",
        )
        self.assertIs(contract["activationAllowed"], False)
        self.assertIs(contract["bootableClaim"], False)
        self.assertEqual(
            contract["authorityBindings"],
            {
                "bootArtifactAuditPlanSha256": _sha(BOOT_PLAN.read_bytes()),
                "executionPolicySha256": _sha(POLICY.read_bytes()),
                "launcherUnitSha256": _sha(UNIT.read_bytes()),
                "baselineRootfsSourceClosureResultSha256": _sha(
                    SOURCE_CLOSURE.read_bytes()
                ),
                "baselineRootfsSourceLockSha256": _sha(SOURCE_LOCK.read_bytes()),
                "sysusersConfigSha256": _sha(SYSUSERS.read_bytes()),
                "tmpfilesConfigSha256": _sha(TMPFILES.read_bytes()),
            },
        )

    def test_contract_excludes_a_guest_node_and_static_pid1_shortcuts(self) -> None:
        contract = guest_init.load_contract(CONTRACT)
        boundary = contract["authorityBoundary"]
        self.assertIs(boundary["guestNodeAuthorityAllowed"], False)
        self.assertIs(boundary["replayNodeUnitAllowed"], False)
        self.assertEqual(boundary["durableJournalOwner"], "mac-host-node")
        init = contract["initSystem"]
        self.assertEqual(init["kind"], "systemd")
        self.assertEqual(init["pid1Path"], "/usr/lib/systemd/systemd")
        self.assertIs(init["staticPid1Allowed"], False)
        self.assertEqual(init["requiredPackageSeed"], "systemd")

    def test_contract_keeps_root_read_only_and_names_every_writable_mount(self) -> None:
        contract = guest_init.load_contract(CONTRACT)
        layout = contract["filesystemLayout"]
        self.assertIs(layout["rootDiskReadOnly"], True)
        self.assertEqual(
            layout["writableMounts"],
            [
                {"filesystem": "devtmpfs", "path": "/dev", "persistence": "boot"},
                {"filesystem": "proc", "path": "/proc", "persistence": "boot"},
                {"filesystem": "tmpfs", "path": "/run", "persistence": "boot"},
                {"filesystem": "cgroup2", "path": "/sys/fs/cgroup", "persistence": "boot"},
                {"filesystem": "tmpfs", "path": "/tmp", "persistence": "boot"},
                {"filesystem": "tmpfs", "path": "/var/lib/boole", "persistence": "boot"},
            ],
        )
        self.assertEqual(layout["readOnlyMounts"], [{"filesystem": "sysfs", "path": "/sys"}])
        self.assertEqual(layout["machineId"], "empty-root-file-with-ephemeral-runtime-identity")

    def test_current_runtime_lock_is_not_a_guest_init_closure(self) -> None:
        result = self._audit()
        self.assertEqual(result["status"], "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS")
        self.assertEqual(
            result["missingRoles"],
            [
                "derived-entry:launcher-enablement",
                "package-seed:systemd",
                "tracked-file:empty-machine-id",
                "tracked-file:launcher-binary",
                "tracked-file:launcher-unit",
                "tracked-file:sysusers-config",
                "tracked-file:tmpfiles-config",
            ],
        )
        self.assertEqual(result["artifactsWritten"], 0)
        self.assertIs(result["bootableClaim"], False)
        self.assertIs(result["activationAllowed"], False)
        self.assertIs(result["signedClosureVerified"], False)
        self.assertIs(result["runtimeCompatibilityVerified"], False)
        self.assertIs(result["authorityBoundaryVerified"], False)
        self.assertNotIn("guestNodeAuthorityAllowed", result)

    def test_direct_cli_reports_the_same_fail_closed_result(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/native_shadow_guest_init_compatibility_arm64_v1.py"),
                "--contract",
                str(CONTRACT),
                "--source-lock",
                str(SOURCE_LOCK),
                "--source-closure",
                str(SOURCE_CLOSURE),
                "--policy",
                str(POLICY),
                "--boot-plan",
                str(BOOT_PLAN),
                "--launcher-unit",
                str(UNIT),
                "--sysusers",
                str(SYSUSERS),
                "--tmpfiles",
                str(TMPFILES),
            ],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        result = json.loads(completed.stdout)
        self.assertEqual(result["status"], "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS")
        self.assertEqual(result["artifactsWritten"], 0)

    def test_successor_shape_with_requirements_present_remains_unverified(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = pathlib.Path(raw_root)
            lock = self._successor_shape_lock()
            lock_path = root / "source-lock.json"
            lock_raw = _canonical(lock)
            lock_path.write_bytes(lock_raw)
            result = guest_init.audit_successor_source_shape(CONTRACT, lock_path)
        self.assertEqual(
            result["status"], "SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED"
        )
        self.assertEqual(result["missingRoles"], [])
        self.assertEqual(result["artifactsWritten"], 0)
        self.assertIs(result["bootableClaim"], False)
        self.assertIs(result["signedClosureVerified"], False)
        self.assertIs(result["runtimeCompatibilityVerified"], False)
        self.assertIs(result["authorityBoundaryVerified"], False)
        self.assertNotIn("guestNodeAuthorityAllowed", result)

    def test_bare_systemd_name_is_not_a_package_closure_shape(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            lock = self._successor_shape_lock()
            lock["ubuntu"]["packages"][-1] = {"name": "systemd"}
            path = pathlib.Path(raw_root) / "source-lock.json"
            path.write_bytes(_canonical(lock))
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "Ubuntu package row .* keys differ",
            ):
                guest_init.audit_successor_source_shape(CONTRACT, path)

    def test_systemd_package_must_be_a_bound_seed_package(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            lock = self._successor_shape_lock()
            systemd_id = next(
                row["packageId"]
                for row in lock["ubuntu"]["packages"]
                if row["name"] == "systemd"
            )
            lock["ubuntu"]["seedPackageIds"].remove(systemd_id)
            path = pathlib.Path(raw_root) / "source-lock.json"
            path.write_bytes(_canonical(lock))
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "seed names and package IDs differ",
            ):
                guest_init.audit_successor_source_shape(CONTRACT, path)

    def test_successor_shape_rejects_unknown_top_level_fields(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            lock = self._successor_shape_lock()
            lock["unreviewedExpansion"] = True
            path = pathlib.Path(raw_root) / "source-lock.json"
            path.write_bytes(_canonical(lock))
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "rootfs source lock keys differ",
            ):
                guest_init.audit_successor_source_shape(CONTRACT, path)

    def test_required_tracked_file_must_be_root_owned_and_safely_sourced(self) -> None:
        for field, value in (("uid", 31337), ("gid", 31337), ("sourcePath", "../../untrusted")):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as raw_root:
                lock = self._successor_shape_lock()
                row = next(
                    item
                    for item in lock["trackedFiles"]
                    if item["logicalPath"] == "/etc/machine-id"
                )
                row[field] = value
                path = pathlib.Path(raw_root) / "source-lock.json"
                path.write_bytes(_canonical(lock))
                with self.assertRaises(guest_init.GuestInitCompatibilityError):
                    guest_init.audit_successor_source_shape(CONTRACT, path)

    def test_successor_shape_rejects_extra_guest_node_service_enablement(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            lock = self._successor_shape_lock()
            lock["derivedEntries"].append(
                {
                    "gid": 0,
                    "kind": "symlink",
                    "logicalPath": "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-replay-node.service",
                    "mode": "0777",
                    "target": "/usr/lib/systemd/system/boole-native-shadow-replay-node.service",
                    "uid": 0,
                }
            )
            lock["derivedEntries"].sort(key=lambda row: row["logicalPath"])
            path = pathlib.Path(raw_root) / "source-lock.json"
            path.write_bytes(_canonical(lock))
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "forbidden guest authority entry",
            ):
                guest_init.audit_successor_source_shape(CONTRACT, path)

    def test_successor_shape_rejects_guest_node_binary(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            lock = self._successor_shape_lock()
            source_path = "synthetic/replay-node"
            digest = "3" * 64
            lock["authorityBindings"].append(
                {
                    "id": "guest-replay-node",
                    "sha256": digest,
                    "sourcePath": source_path,
                }
            )
            lock["authorityBindings"].sort(key=lambda row: row["id"])
            lock["trackedFiles"].append(
                {
                    "gid": 0,
                    "logicalPath": "/usr/libexec/boole/boole-native-shadow-replay-node",
                    "mode": "0755",
                    "sha256": digest,
                    "sourcePath": source_path,
                    "uid": 0,
                }
            )
            lock["trackedFiles"].sort(key=lambda row: row["logicalPath"])
            path = pathlib.Path(raw_root) / "source-lock.json"
            path.write_bytes(_canonical(lock))
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "forbidden guest authority entry",
            ):
                guest_init.audit_successor_source_shape(CONTRACT, path)

    def test_future_mac3_transport_unit_is_not_preemptively_forbidden(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            lock = self._successor_shape_lock()
            source_path = "synthetic/host-guest-transport.service"
            digest = "4" * 64
            lock["authorityBindings"].append(
                {
                    "id": "future-host-guest-transport-unit",
                    "sha256": digest,
                    "sourcePath": source_path,
                }
            )
            lock["authorityBindings"].sort(key=lambda row: row["id"])
            unit_path = (
                "/usr/lib/systemd/system/"
                "boole-native-shadow-host-guest-transport.service"
            )
            lock["trackedFiles"].append(
                {
                    "gid": 0,
                    "logicalPath": unit_path,
                    "mode": "0444",
                    "sha256": digest,
                    "sourcePath": source_path,
                    "uid": 0,
                }
            )
            lock["trackedFiles"].sort(key=lambda row: row["logicalPath"])
            lock["derivedEntries"].append(
                {
                    "gid": 0,
                    "kind": "symlink",
                    "logicalPath": (
                        "/etc/systemd/system/multi-user.target.wants/"
                        "boole-native-shadow-host-guest-transport.service"
                    ),
                    "mode": "0777",
                    "target": unit_path,
                    "uid": 0,
                }
            )
            lock["derivedEntries"].sort(key=lambda row: row["logicalPath"])
            path = pathlib.Path(raw_root) / "source-lock.json"
            path.write_bytes(_canonical(lock))
            result = guest_init.audit_successor_source_shape(CONTRACT, path)
        self.assertEqual(
            result["status"], "SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED"
        )

    def test_authority_byte_drift_is_a_hard_error(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            unit = pathlib.Path(raw_root) / "launcher.service"
            unit.write_bytes(UNIT.read_bytes() + b"# drift\n")
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "launcher unit digest differs",
            ):
                self._audit(unit=unit)

    def test_policy_and_unit_semantics_are_cross_checked(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = pathlib.Path(raw_root)
            policy = json.loads(POLICY.read_bytes())
            policy["privilege"]["systemdUnit"]["Delegate"] = ["cpu", "memory"]
            policy_path = root / "policy.json"
            policy_raw = _canonical(policy)
            policy_path.write_bytes(policy_raw)
            contract = json.loads(CONTRACT.read_bytes())
            contract["authorityBindings"]["executionPolicySha256"] = _sha(policy_raw)
            contract_path = root / "contract.json"
            contract_path.write_bytes(_canonical(contract))
            with self.assertRaisesRegex(
                guest_init.GuestInitCompatibilityError,
                "systemd delegation differs",
            ):
                self._audit(contract=contract_path, policy=policy_path)

    def test_contract_rejects_replay_node_or_static_pid1_expansion(self) -> None:
        mutations = (
            (
                "static pid1",
                lambda value: value["initSystem"].update(staticPid1Allowed=True),
            ),
            (
                "guest node authority",
                lambda value: value["authorityBoundary"].update(
                    guestNodeAuthorityAllowed=True
                ),
            ),
        )
        for label, mutate in mutations:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as raw_root:
                value = json.loads(CONTRACT.read_bytes())
                mutate(value)
                path = pathlib.Path(raw_root) / "contract.json"
                path.write_bytes(_canonical(value))
                with self.assertRaises(guest_init.GuestInitCompatibilityError):
                    guest_init.load_contract(path)

    def test_self_test_runs_the_guest_init_contract_suite(self) -> None:
        self_test = (ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        self.assertIn(
            "scripts/test_native_shadow_guest_init_compatibility_arm64_v1.py",
            self_test,
        )


if __name__ == "__main__":
    unittest.main()
