#!/usr/bin/env python3
"""Audit the frozen Linux/arm64 guest-init compatibility contract.

This tool is intentionally read-only.  It reports whether a source lock has
the named and source-bound structural requirements of the frozen systemd guest
contract.  It does not verify a signed dependency closure, a runtime authority
boundary, kernel compatibility or bootability, and it never writes an artifact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
from typing import Any

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_artifact_builder_arm64_v1 as boot


CONTRACT_SCHEMA = "boole.native-shadow.guest-init-compatibility.arm64.v1"
SOURCE_LOCK_SCHEMA = "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1"
SOURCE_CLOSURE_SCHEMA = (
    "boole.native-shadow.runtime-rootfs-source-closure-result.arm64.v1"
)
POLICY_SCHEMA = "boole.native-shadow.execution-policy.arm64.v1"
BOOT_PLAN_SCHEMA = "boole.native-shadow.boot-artifact-build-plan.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.guest-init-compatibility-result.arm64.v1"
UNIT_NAME = "boole-native-shadow-launcher.service"
CGROUP_PARENT = "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service"
TRACKED_FILE_KEYS = {"gid", "logicalPath", "mode", "sha256", "sourcePath", "uid"}
DERIVED_ENTRY_KEYS = {"gid", "kind", "logicalPath", "mode", "target", "uid"}
ARTIFACT_KEYS = {"id", "kind", "sha256", "sizeBytes"}
UBUNTU_PACKAGE_KEYS = {
    "architecture",
    "artifactId",
    "component",
    "dependencyResolutions",
    "depends",
    "essential",
    "indexStanzaSha256",
    "multiArch",
    "name",
    "packageId",
    "poolPath",
    "preDepends",
    "provides",
    "repositoryId",
    "sourceName",
    "sourceVersion",
    "version",
}


class GuestInitCompatibilityError(RuntimeError):
    """A frozen authority or compatibility invariant is invalid."""


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _digest(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise GuestInitCompatibilityError(f"{context} must be a lowercase SHA-256")
    return value


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise GuestInitCompatibilityError(f"{context} keys differ")
    return value


def _load_canonical(path: pathlib.Path, context: str) -> tuple[dict[str, Any], bytes]:
    try:
        return boot._load_canonical(path, context)
    except boot.BootArtifactPreflightError as exc:
        raise GuestInitCompatibilityError(str(exc)) from exc


def _read_raw(path: pathlib.Path, context: str) -> bytes:
    try:
        return boot._read_regular_nofollow(path, context)
    except boot.BootArtifactPreflightError as exc:
        raise GuestInitCompatibilityError(str(exc)) from exc


def _validate_bindings(value: Any) -> dict[str, str]:
    bindings = _exact(
        value,
        {
            "baselineRootfsSourceClosureResultSha256",
            "baselineRootfsSourceLockSha256",
            "bootArtifactAuditPlanSha256",
            "executionPolicySha256",
            "launcherUnitSha256",
            "sysusersConfigSha256",
            "tmpfilesConfigSha256",
        },
        "authorityBindings",
    )
    return {key: _digest(raw, f"authorityBindings.{key}") for key, raw in bindings.items()}


def _validate_tracked_requirements(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or len(value) != 5:
        raise GuestInitCompatibilityError("trackedFileRequirements differ")
    expected = [
        ("empty-machine-id", "/etc/machine-id", "0444"),
        (
            "launcher-unit",
            "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
            "0444",
        ),
        ("sysusers-config", "/usr/lib/sysusers.d/boole-native-shadow.conf", "0444"),
        ("tmpfiles-config", "/usr/lib/tmpfiles.d/boole-native-shadow.conf", "0444"),
        (
            "launcher-binary",
            "/usr/libexec/boole/boole-native-shadow-launcher",
            "0755",
        ),
    ]
    result: list[dict[str, Any]] = []
    for index, (raw, wanted) in enumerate(zip(value, expected)):
        row = _exact(
            raw,
            {"gid", "logicalPath", "mode", "role", "sha256", "uid"},
            f"trackedFileRequirements[{index}]",
        )
        role, logical_path, mode = wanted
        if (row["role"], row["logicalPath"], row["mode"]) != wanted:
            raise GuestInitCompatibilityError("tracked file role/path/mode differs")
        if role == "launcher-binary":
            if row["sha256"] is not None:
                raise GuestInitCompatibilityError(
                    "launcher binary digest belongs to the later input-authority slice"
                )
        else:
            _digest(row["sha256"], f"trackedFileRequirements[{index}].sha256")
        if row["uid"] != 0 or row["gid"] != 0:
            raise GuestInitCompatibilityError("required tracked file ownership differs")
        result.append(row)
    return result


def _validate_contract(value: Any) -> dict[str, Any]:
    contract = _exact(
        value,
        {
            "activationAllowed",
            "authorityBindings",
            "authorityBoundary",
            "bootableClaim",
            "derivedEntryRequirements",
            "filesystemLayout",
            "initSystem",
            "kernelCompatibility",
            "release",
            "schema",
            "trackedFileRequirements",
            "verificationBoundary",
        },
        "guest-init contract",
    )
    if contract["schema"] != CONTRACT_SCHEMA:
        raise GuestInitCompatibilityError("guest-init contract schema differs")
    if contract["activationAllowed"] is not False or contract["bootableClaim"] is not False:
        raise GuestInitCompatibilityError("guest-init contract must remain non-activatable")
    if not isinstance(contract["release"], str) or not contract["release"].endswith(
        "CONTRACT-FROZEN-MATERIALIZATION-BLOCKED"
    ):
        raise GuestInitCompatibilityError("guest-init contract release differs")
    _validate_bindings(contract["authorityBindings"])
    boundary = _exact(
        contract["authorityBoundary"],
        {
            "durableJournalOwner",
            "guestNodeAuthorityAllowed",
            "hostGuestTransport",
            "replayNodeUnitAllowed",
        },
        "authorityBoundary",
    )
    if boundary != {
        "durableJournalOwner": "mac-host-node",
        "guestNodeAuthorityAllowed": False,
        "hostGuestTransport": "deferred-to-MAC.3-authenticated-bounded-channel",
        "replayNodeUnitAllowed": False,
    }:
        raise GuestInitCompatibilityError("guest authority boundary differs")
    init = _exact(
        contract["initSystem"],
        {
            "accountProvisioning",
            "dynamicNumericIdsAllowed",
            "kind",
            "pid1Path",
            "requiredPackageSeed",
            "serviceTarget",
            "staticPid1Allowed",
            "tmpfilesProvisioning",
        },
        "initSystem",
    )
    if init != {
        "accountProvisioning": "systemd-sysusers-at-every-boot",
        "dynamicNumericIdsAllowed": True,
        "kind": "systemd",
        "pid1Path": "/usr/lib/systemd/systemd",
        "requiredPackageSeed": "systemd",
        "serviceTarget": "multi-user.target",
        "staticPid1Allowed": False,
        "tmpfilesProvisioning": "systemd-tmpfiles-at-every-boot",
    }:
        raise GuestInitCompatibilityError("systemd init contract differs")
    layout = _exact(
        contract["filesystemLayout"],
        {"machineId", "readOnlyMounts", "rootDiskReadOnly", "writableMounts"},
        "filesystemLayout",
    )
    if layout != {
        "machineId": "empty-root-file-with-ephemeral-runtime-identity",
        "readOnlyMounts": [{"filesystem": "sysfs", "path": "/sys"}],
        "rootDiskReadOnly": True,
        "writableMounts": [
            {"filesystem": "devtmpfs", "path": "/dev", "persistence": "boot"},
            {"filesystem": "proc", "path": "/proc", "persistence": "boot"},
            {"filesystem": "tmpfs", "path": "/run", "persistence": "boot"},
            {
                "filesystem": "cgroup2",
                "path": "/sys/fs/cgroup",
                "persistence": "boot",
            },
            {"filesystem": "tmpfs", "path": "/tmp", "persistence": "boot"},
            {
                "filesystem": "tmpfs",
                "path": "/var/lib/boole",
                "persistence": "boot",
            },
        ],
    }:
        raise GuestInitCompatibilityError("guest filesystem layout differs")
    kernel = _exact(
        contract["kernelCompatibility"],
        {
            "cgroupControllers",
            "cgroupVersion",
            "minimumLandlockAbi",
            "requiredFacilities",
        },
        "kernelCompatibility",
    )
    if kernel != {
        "cgroupControllers": ["cpu", "memory", "pids"],
        "cgroupVersion": 2,
        "minimumLandlockAbi": 3,
        "requiredFacilities": [
            "cgroup-freeze",
            "cgroup-kill",
            "clone3-clone-into-cgroup",
            "mount-namespace",
            "pid-namespace",
            "pidfd",
            "seccomp-filter",
        ],
    }:
        raise GuestInitCompatibilityError("kernel compatibility requirements differ")
    derived = contract["derivedEntryRequirements"]
    if derived != [
        {
            "logicalPath": "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service",
            "role": "launcher-enablement",
            "target": "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
        }
    ]:
        raise GuestInitCompatibilityError("launcher enablement requirement differs")
    _validate_tracked_requirements(contract["trackedFileRequirements"])
    if contract["verificationBoundary"] != {
        "compatibleStatusIsBootClaim": False,
        "currentOciRootfsIsGuestRootDisk": False,
        "hostSystemdGateIsGuestBootEvidence": False,
        "materializationMayUseNetwork": False,
        "outputsAllowed": False,
        "sourceShapeStatusIsAuthorityBoundaryEvidence": False,
        "sourceShapeStatusIsRuntimeCompatibilityEvidence": False,
        "sourceShapeStatusIsSignedClosureEvidence": False,
    }:
        raise GuestInitCompatibilityError("verification boundary differs")
    return contract


def load_contract(path: pathlib.Path) -> dict[str, Any]:
    value, _ = _load_canonical(path, "guest-init compatibility contract")
    return _validate_contract(value)


def _require_digest(raw: bytes, expected: str, context: str) -> None:
    if _sha(raw) != expected:
        raise GuestInitCompatibilityError(f"{context} digest differs")


def _validate_policy(value: dict[str, Any]) -> None:
    if value.get("schema") != POLICY_SCHEMA or value.get("activationAllowed") is not False:
        raise GuestInitCompatibilityError("execution policy identity differs")
    if value.get("platform") != {
        "architecture": "aarch64",
        "cgroupVersion": 2,
        "minimumLandlockAbi": 3,
        "operatingSystem": "linux",
        "systemdRequired": True,
    }:
        raise GuestInitCompatibilityError("execution policy platform differs")
    crash = value.get("crashRecovery")
    if not isinstance(crash, dict) or crash.get("cgroupParent") != CGROUP_PARENT:
        raise GuestInitCompatibilityError("execution policy cgroup parent differs")
    privilege = value.get("privilege")
    unit = privilege.get("systemdUnit") if isinstance(privilege, dict) else None
    if not isinstance(unit, dict) or unit.get("UnitName") != UNIT_NAME:
        raise GuestInitCompatibilityError("execution policy unit name differs")
    if unit.get("Delegate") != ["cpu", "memory", "pids"]:
        raise GuestInitCompatibilityError("systemd delegation differs")


def _validate_deployment_bytes(
    unit_raw: bytes, sysusers_raw: bytes, tmpfiles_raw: bytes
) -> None:
    try:
        unit = unit_raw.decode("utf-8")
        sysusers = sysusers_raw.decode("utf-8")
        tmpfiles = tmpfiles_raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise GuestInitCompatibilityError("deployment input is not UTF-8") from exc
    required_unit_lines = {
        "Type=exec",
        "ExecStart=/usr/libexec/boole/boole-native-shadow-launcher",
        "User=root",
        "Group=root",
        "Slice=system.slice",
        "Delegate=cpu memory pids",
        "WantedBy=multi-user.target",
    }
    if not required_unit_lines.issubset(set(unit.splitlines())):
        raise GuestInitCompatibilityError("launcher unit semantics differ")
    if "boole-native-shadow-replay-node.service" in unit:
        raise GuestInitCompatibilityError("guest launcher unit imports replay-node authority")
    required_accounts = {
        'g boole-node -',
        'u boole-node - "Boole native-shadow node" /nonexistent /usr/sbin/nologin',
        'g boole-native-checker -',
        'u boole-native-checker - "Boole native-shadow checker" /nonexistent /bin/false',
    }
    if set(sysusers.splitlines()) != required_accounts:
        raise GuestInitCompatibilityError("sysusers account contract differs")
    required_tmpfiles = {
        "d /run/boole 0755 root root -",
        "d /run/boole/native-shadow 2750 root boole-node -",
        "d /var/lib/boole 0755 root root -",
        "d /var/lib/boole/native-shadow 0755 root root -",
        "d /var/lib/boole/native-shadow/node-state 0700 boole-node boole-node -",
    }
    if set(tmpfiles.splitlines()) != required_tmpfiles:
        raise GuestInitCompatibilityError("tmpfiles layout contract differs")


def _mode(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 4
        or value[0] != "0"
        or any(character not in "01234567" for character in value[1:])
    ):
        raise GuestInitCompatibilityError(f"{context} is not an octal mode")
    return value


def _absolute_guest_path(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.startswith("/"):
        raise GuestInitCompatibilityError(f"{context} is not an absolute guest path")
    path = pathlib.PurePosixPath(value)
    if str(path) != value or ".." in path.parts or "." in path.parts:
        raise GuestInitCompatibilityError(f"{context} is not normalized")
    return value


def _safe_relative_path(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value or value.startswith("/"):
        raise GuestInitCompatibilityError(f"{context} is not repository-relative")
    path = pathlib.PurePosixPath(value)
    if str(path) != value or any(part in {"", ".", ".."} for part in path.parts):
        raise GuestInitCompatibilityError(f"{context} is not normalized")
    return value


def _reject_forbidden_guest_authority(logical_path: str, target: str = "") -> None:
    values = (logical_path.lower(), target.lower())
    if any("replay-node" in value for value in values):
        raise GuestInitCompatibilityError("forbidden guest authority entry")
    basename = pathlib.PurePosixPath(logical_path).name.lower()
    if basename in {"boole-node", "boole-node.service"}:
        raise GuestInitCompatibilityError("forbidden guest authority entry")


def _tracked_roles(
    source_lock: dict[str, Any], requirements: list[dict[str, Any]]
) -> list[str]:
    rows = source_lock.get("trackedFiles")
    if not isinstance(rows, list):
        raise GuestInitCompatibilityError("source lock trackedFiles are absent")
    raw_bindings = source_lock.get("authorityBindings")
    if not isinstance(raw_bindings, list):
        raise GuestInitCompatibilityError("source lock authority bindings are absent")
    binding_ids: list[str] = []
    bound_sources: set[tuple[str, str]] = set()
    for index, raw in enumerate(raw_bindings):
        binding = _exact(
            raw,
            {"id", "sha256", "sourcePath"},
            f"source lock authorityBindings[{index}]",
        )
        if not isinstance(binding["id"], str) or not binding["id"]:
            raise GuestInitCompatibilityError("source lock authority binding ID is invalid")
        source = _safe_relative_path(
            binding["sourcePath"],
            f"source lock authorityBindings[{index}].sourcePath",
        )
        digest = _digest(
            binding["sha256"], f"source lock authorityBindings[{index}].sha256"
        )
        binding_ids.append(binding["id"])
        if (source, digest) in bound_sources:
            raise GuestInitCompatibilityError("source lock authority binding is duplicated")
        bound_sources.add((source, digest))
    if binding_ids != sorted(set(binding_ids)):
        raise GuestInitCompatibilityError(
            "source lock authority binding IDs are not sorted and unique"
        )
    by_path: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(rows):
        row = _exact(raw, TRACKED_FILE_KEYS, f"trackedFiles[{index}]")
        logical_path = _absolute_guest_path(
            row["logicalPath"], f"trackedFiles[{index}].logicalPath"
        )
        if logical_path in by_path:
            raise GuestInitCompatibilityError("source lock tracked file path is duplicated")
        if row["uid"] != 0 or row["gid"] != 0:
            raise GuestInitCompatibilityError("source lock tracked file must be root-owned")
        _mode(row["mode"], f"trackedFiles[{index}].mode")
        digest = _digest(row["sha256"], f"trackedFiles[{index}].sha256")
        source = _safe_relative_path(
            row["sourcePath"], f"trackedFiles[{index}].sourcePath"
        )
        if (source, digest) not in bound_sources:
            raise GuestInitCompatibilityError(
                "tracked file is not bound by source lock authority"
            )
        _reject_forbidden_guest_authority(logical_path)
        by_path[logical_path] = row
    if list(by_path) != sorted(by_path):
        raise GuestInitCompatibilityError("source lock tracked files are not sorted")
    missing: list[str] = []
    for requirement in requirements:
        row = by_path.get(requirement["logicalPath"])
        role = requirement["role"]
        if row is None:
            missing.append(f"tracked-file:{role}")
            continue
        if row.get("mode") != requirement["mode"]:
            raise GuestInitCompatibilityError(f"tracked file mode differs: {role}")
        if row.get("uid") != requirement["uid"] or row.get("gid") != requirement["gid"]:
            raise GuestInitCompatibilityError(f"tracked file ownership differs: {role}")
        digest = row.get("sha256")
        _digest(digest, f"tracked file digest: {role}")
        expected = requirement["sha256"]
        if expected is not None and digest != expected:
            raise GuestInitCompatibilityError(f"tracked file digest differs: {role}")
    return missing


def _derived_roles(
    source_lock: dict[str, Any], requirements: list[dict[str, Any]]
) -> list[str]:
    rows = source_lock.get("derivedEntries")
    if not isinstance(rows, list):
        raise GuestInitCompatibilityError("source lock derivedEntries are absent")
    by_path: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(rows):
        row = _exact(raw, DERIVED_ENTRY_KEYS, f"derivedEntries[{index}]")
        logical_path = _absolute_guest_path(
            row["logicalPath"], f"derivedEntries[{index}].logicalPath"
        )
        if logical_path in by_path:
            raise GuestInitCompatibilityError(
                "source lock derived entries are invalid or duplicated"
            )
        if row["uid"] != 0 or row["gid"] != 0:
            raise GuestInitCompatibilityError("source lock derived entry must be root-owned")
        _mode(row["mode"], f"derivedEntries[{index}].mode")
        if not isinstance(row["target"], str) or not row["target"]:
            raise GuestInitCompatibilityError("source lock derived entry target is invalid")
        _reject_forbidden_guest_authority(logical_path, row["target"])
        by_path[logical_path] = row
    if list(by_path) != sorted(by_path):
        raise GuestInitCompatibilityError("source lock derived entries are not sorted")
    missing: list[str] = []
    for requirement in requirements:
        row = by_path.get(requirement["logicalPath"])
        role = requirement["role"]
        if row is None:
            missing.append(f"derived-entry:{role}")
            continue
        if (
            row.get("kind") != "symlink"
            or row.get("target") != requirement["target"]
            or row.get("mode") != "0777"
            or row.get("uid") != 0
            or row.get("gid") != 0
        ):
            raise GuestInitCompatibilityError(f"derived entry differs: {role}")
    return missing


def _validate_source_lock_identity(source_lock: dict[str, Any]) -> None:
    _exact(
        source_lock,
        {
            "activationAllowed",
            "artifacts",
            "authorityBindings",
            "buildRecipe",
            "closureRoots",
            "derivedEntries",
            "platform",
            "release",
            "rust",
            "schema",
            "trackedFiles",
            "ubuntu",
        },
        "rootfs source lock",
    )
    if source_lock.get("schema") != SOURCE_LOCK_SCHEMA or source_lock.get(
        "activationAllowed"
    ) is not False:
        raise GuestInitCompatibilityError("rootfs source lock identity differs")
    if source_lock.get("platform") != {
        "debArchitecture": "arm64",
        "ociArchitecture": "arm64",
        "os": "linux",
        "rustTarget": "aarch64-unknown-linux-gnu",
    }:
        raise GuestInitCompatibilityError("rootfs source lock platform differs")


def _ubuntu_package_rows(source_lock: dict[str, Any]) -> list[dict[str, Any]]:
    artifacts = source_lock.get("artifacts")
    if not isinstance(artifacts, list):
        raise GuestInitCompatibilityError("source lock artifacts are invalid")
    artifacts_by_id: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(artifacts):
        row = _exact(raw, ARTIFACT_KEYS, f"artifacts[{index}]")
        identifier = row["id"]
        if not isinstance(identifier, str) or not identifier or identifier in artifacts_by_id:
            raise GuestInitCompatibilityError(
                "source lock artifact IDs are invalid or duplicated"
            )
        if not isinstance(row["kind"], str) or not row["kind"]:
            raise GuestInitCompatibilityError("source lock artifact kind is invalid")
        _digest(row["sha256"], f"artifacts[{index}].sha256")
        if (
            not isinstance(row["sizeBytes"], int)
            or isinstance(row["sizeBytes"], bool)
            or row["sizeBytes"] < 0
        ):
            raise GuestInitCompatibilityError("source lock artifact size is invalid")
        artifacts_by_id[identifier] = row
    if list(artifacts_by_id) != sorted(artifacts_by_id):
        raise GuestInitCompatibilityError("source lock artifacts are not sorted")
    ubuntu = source_lock.get("ubuntu")
    packages = ubuntu.get("packages") if isinstance(ubuntu, dict) else None
    if not isinstance(packages, list):
        raise GuestInitCompatibilityError("source lock packages are invalid")
    result: list[dict[str, Any]] = []
    package_ids: set[str] = set()
    for index, raw in enumerate(packages):
        row = _exact(raw, UBUNTU_PACKAGE_KEYS, f"Ubuntu package row {index}")
        for key in (
            "architecture",
            "artifactId",
            "component",
            "depends",
            "indexStanzaSha256",
            "multiArch",
            "name",
            "packageId",
            "poolPath",
            "preDepends",
            "provides",
            "repositoryId",
            "sourceName",
            "sourceVersion",
            "version",
        ):
            if not isinstance(row[key], str):
                raise GuestInitCompatibilityError(f"Ubuntu package row {key} is invalid")
        if row["architecture"] not in {"arm64", "all"}:
            raise GuestInitCompatibilityError("Ubuntu package architecture differs")
        if not isinstance(row["essential"], bool) or not isinstance(
            row["dependencyResolutions"], list
        ):
            raise GuestInitCompatibilityError("Ubuntu package metadata is invalid")
        _digest(row["indexStanzaSha256"], "Ubuntu package index stanza digest")
        package_id = row["packageId"]
        if package_id != row["artifactId"] or package_id in package_ids:
            raise GuestInitCompatibilityError("Ubuntu package IDs are invalid or duplicated")
        artifact = artifacts_by_id.get(package_id)
        if artifact is None or artifact["kind"] != "deb":
            raise GuestInitCompatibilityError("Ubuntu package artifact is absent")
        package_ids.add(package_id)
        result.append(row)
    if [row["packageId"] for row in result] != sorted(package_ids):
        raise GuestInitCompatibilityError("Ubuntu package rows are not sorted")
    for index, row in enumerate(result):
        for resolution_index, raw_resolution in enumerate(row["dependencyResolutions"]):
            resolution = _exact(
                raw_resolution,
                {"alternativeIndex", "field", "groupIndex", "packageId"},
                f"Ubuntu package row {index} dependency resolution {resolution_index}",
            )
            if (
                not isinstance(resolution["alternativeIndex"], int)
                or isinstance(resolution["alternativeIndex"], bool)
                or resolution["alternativeIndex"] < 0
                or not isinstance(resolution["groupIndex"], int)
                or isinstance(resolution["groupIndex"], bool)
                or resolution["groupIndex"] < 0
                or resolution["field"] not in {"Depends", "Pre-Depends"}
                or resolution["packageId"] not in package_ids
            ):
                raise GuestInitCompatibilityError(
                    "Ubuntu package dependency resolution is invalid"
                )
    return result


def _audit_source_shape(
    contract: dict[str, Any], source_lock: dict[str, Any], source_lock_raw: bytes
) -> dict[str, Any]:
    _validate_source_lock_identity(source_lock)
    ubuntu = source_lock.get("ubuntu")
    if not isinstance(ubuntu, dict):
        raise GuestInitCompatibilityError("source lock Ubuntu closure is absent")
    seeds = ubuntu.get("seeds")
    if not isinstance(seeds, list) or not all(isinstance(item, str) for item in seeds):
        raise GuestInitCompatibilityError("source lock package seeds are invalid")
    if seeds != sorted(set(seeds)):
        raise GuestInitCompatibilityError("source lock package seeds are not sorted and unique")
    packages = _ubuntu_package_rows(source_lock)
    seed_package_ids = ubuntu.get("seedPackageIds")
    if (
        not isinstance(seed_package_ids, list)
        or not all(isinstance(item, str) for item in seed_package_ids)
        or seed_package_ids != sorted(set(seed_package_ids))
    ):
        raise GuestInitCompatibilityError(
            "source lock seed package IDs are invalid or duplicated"
        )
    packages_by_id = {row["packageId"]: row for row in packages}
    if any(package_id not in packages_by_id for package_id in seed_package_ids):
        raise GuestInitCompatibilityError("source lock seed package is absent")
    if sorted(packages_by_id[package_id]["name"] for package_id in seed_package_ids) != seeds:
        raise GuestInitCompatibilityError("source lock seed names and package IDs differ")
    systemd_rows = [row for row in packages if row["name"] == "systemd"]
    missing: list[str] = []
    if "systemd" not in seeds or not systemd_rows:
        missing.append("package-seed:systemd")
    elif len(systemd_rows) != 1:
        raise GuestInitCompatibilityError("systemd package row is duplicated")
    else:
        systemd = systemd_rows[0]
        if systemd["sourceName"] != "systemd" or not pathlib.PurePosixPath(
            systemd["poolPath"]
        ).name.startswith("systemd_"):
            raise GuestInitCompatibilityError("systemd package row identity differs")
    missing.extend(
        _tracked_roles(source_lock, contract["trackedFileRequirements"])
    )
    missing.extend(
        _derived_roles(source_lock, contract["derivedEntryRequirements"])
    )
    missing.sort()
    status = (
        "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS"
        if missing
        else "SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED"
    )
    return {
        "activationAllowed": False,
        "artifactsWritten": 0,
        "authorityBoundaryVerified": False,
        "bootableClaim": False,
        "missingRoles": missing,
        "runtimeCompatibilityVerified": False,
        "schema": RESULT_SCHEMA,
        "signedClosureVerified": False,
        "sourceLockSha256": _sha(source_lock_raw),
        "status": status,
    }


def audit_successor_source_shape(
    contract_path: pathlib.Path, candidate_source_lock_path: pathlib.Path
) -> dict[str, Any]:
    """Check an independently pinned successor lock against this contract.

    This function does not make the candidate authoritative.  The later input
    authority must pin the candidate digest separately before materialization.
    """

    contract = load_contract(contract_path)
    source_lock, source_lock_raw = _load_canonical(
        candidate_source_lock_path, "candidate rootfs source lock"
    )
    return _audit_source_shape(contract, source_lock, source_lock_raw)


def audit_guest_init(
    contract_path: pathlib.Path,
    source_lock_path: pathlib.Path,
    source_closure_path: pathlib.Path,
    policy_path: pathlib.Path,
    boot_plan_path: pathlib.Path,
    launcher_unit_path: pathlib.Path,
    sysusers_path: pathlib.Path,
    tmpfiles_path: pathlib.Path,
) -> dict[str, Any]:
    contract = load_contract(contract_path)
    bindings = contract["authorityBindings"]
    source_lock, source_lock_raw = _load_canonical(source_lock_path, "rootfs source lock")
    source_closure, source_closure_raw = _load_canonical(
        source_closure_path, "rootfs source closure result"
    )
    policy, policy_raw = _load_canonical(policy_path, "execution policy")
    boot_plan, boot_plan_raw = _load_canonical(boot_plan_path, "boot artifact audit plan")
    unit_raw = _read_raw(launcher_unit_path, "launcher unit")
    sysusers_raw = _read_raw(sysusers_path, "sysusers config")
    tmpfiles_raw = _read_raw(tmpfiles_path, "tmpfiles config")
    _require_digest(
        source_lock_raw,
        bindings["baselineRootfsSourceLockSha256"],
        "rootfs source lock",
    )
    _require_digest(
        source_closure_raw,
        bindings["baselineRootfsSourceClosureResultSha256"],
        "rootfs source closure result",
    )
    _require_digest(policy_raw, bindings["executionPolicySha256"], "execution policy")
    _require_digest(
        boot_plan_raw,
        bindings["bootArtifactAuditPlanSha256"],
        "boot artifact audit plan",
    )
    _require_digest(unit_raw, bindings["launcherUnitSha256"], "launcher unit")
    _require_digest(sysusers_raw, bindings["sysusersConfigSha256"], "sysusers config")
    _require_digest(tmpfiles_raw, bindings["tmpfilesConfigSha256"], "tmpfiles config")
    _validate_source_lock_identity(source_lock)
    if source_closure.get("schema") != SOURCE_CLOSURE_SCHEMA or source_closure.get(
        "activationAllowed"
    ) is not False:
        raise GuestInitCompatibilityError("rootfs source closure identity differs")
    if boot_plan.get("schema") != BOOT_PLAN_SCHEMA or boot_plan.get(
        "activationAllowed"
    ) is not False:
        raise GuestInitCompatibilityError("boot artifact audit plan identity differs")
    if any(
        boot_plan.get("inputs", {}).get(role, {}).get("sha256") is not None
        for role in ("imageBuilderToolchain", "kernel", "systemdGuestClosure")
    ):
        raise GuestInitCompatibilityError("audit-only boot plan gained a populated authority")
    _validate_policy(policy)
    _validate_deployment_bytes(unit_raw, sysusers_raw, tmpfiles_raw)
    return _audit_source_shape(contract, source_lock, source_lock_raw)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Audit the frozen Linux/arm64 guest-init compatibility contract."
    )
    parser.add_argument("--contract", type=pathlib.Path, required=True)
    parser.add_argument("--source-lock", type=pathlib.Path, required=True)
    parser.add_argument("--source-closure", type=pathlib.Path, required=True)
    parser.add_argument("--policy", type=pathlib.Path, required=True)
    parser.add_argument("--boot-plan", type=pathlib.Path, required=True)
    parser.add_argument("--launcher-unit", type=pathlib.Path, required=True)
    parser.add_argument("--sysusers", type=pathlib.Path, required=True)
    parser.add_argument("--tmpfiles", type=pathlib.Path, required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = audit_guest_init(
            args.contract,
            args.source_lock,
            args.source_closure,
            args.policy,
            args.boot_plan,
            args.launcher_unit,
            args.sysusers,
            args.tmpfiles,
        )
    except GuestInitCompatibilityError as exc:
        print(f"guest-init compatibility audit failed: {exc}", file=sys.stderr)
        return 2
    sys.stdout.buffer.write(boot.canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
