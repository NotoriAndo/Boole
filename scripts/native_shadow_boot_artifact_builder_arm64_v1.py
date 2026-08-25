#!/usr/bin/env python3
"""Fail-closed preflight for deterministic Linux/arm64 boot artifacts.

This slice deliberately produces no kernel, initrd or root disk.  It audits
the frozen rootfs closure and the existing systemd execution policy while
keeping the still-undefined kernel, systemd guest-closure and image-builder
authorities fail-closed for a later contract slice.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import stat
import sys
from typing import Any, Optional


PLAN_SCHEMA = "boole.native-shadow.boot-artifact-build-plan.arm64.v1"
LOCK_SCHEMA = "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1"
POLICY_SCHEMA = "boole.native-shadow.execution-policy.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-artifact-preflight-result.arm64.v1"
SYSTEMD_UNIT_NAME = "boole-native-shadow-launcher.service"
SYSTEMD_CGROUP_PARENT = (
    "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service"
)


class BootArtifactPreflightError(RuntimeError):
    """The preflight authority or one of its present inputs is invalid."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        indent=2,
        allow_nan=False,
    ).encode("utf-8") + b"\n"


def _duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise BootArtifactPreflightError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _normalized_absolute(path: pathlib.Path) -> pathlib.Path:
    absolute = pathlib.Path(os.path.abspath(os.fspath(path)))
    if sys.platform == "darwin" and len(absolute.parts) > 1 and absolute.parts[1] in {
        "etc",
        "tmp",
        "var",
    }:
        absolute = pathlib.Path("/private").joinpath(*absolute.parts[1:])
    return absolute


def _open_directory_nofollow(path: pathlib.Path, context: str) -> int:
    flags = (
        os.O_RDONLY
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    absolute = _normalized_absolute(path)
    try:
        descriptor = os.open(os.path.sep, flags)
    except OSError as exc:
        raise BootArtifactPreflightError(f"cannot open filesystem root for {context}") from exc
    try:
        for component in absolute.parts[1:]:
            if component in {"", ".", "..", os.path.sep}:
                raise BootArtifactPreflightError(f"{context} path is not normalized")
            try:
                child = os.open(component, flags, dir_fd=descriptor)
            except OSError as exc:
                raise BootArtifactPreflightError(
                    f"{context} directory is absent, unsafe, or contains a symlink: {path}"
                ) from exc
            os.close(descriptor)
            descriptor = child
        if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
            raise BootArtifactPreflightError(f"{context} is not a directory")
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _read_regular_at(
    directory_descriptor: int,
    name: str,
    context: str,
    *,
    executable: bool = False,
    missing_ok: bool = False,
) -> Optional[bytes]:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(name, flags, dir_fd=directory_descriptor)
    except FileNotFoundError:
        if missing_ok:
            return None
        raise BootArtifactPreflightError(f"{context} is absent")
    except OSError as exc:
        raise BootArtifactPreflightError(f"{context} is unsafe or a symlink") from exc
    try:
        metadata_before = os.fstat(descriptor)
        if not stat.S_ISREG(metadata_before.st_mode):
            raise BootArtifactPreflightError(f"{context} is not a regular file")
        if executable and not (metadata_before.st_mode & 0o111):
            raise BootArtifactPreflightError(f"{context} is not executable")
        chunks: list[bytes] = []
        remaining = metadata_before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise BootArtifactPreflightError(f"{context} was truncated")
            chunks.append(chunk)
            remaining -= len(chunk)
        metadata_after = os.fstat(descriptor)
        identity_before = (
            metadata_before.st_dev,
            metadata_before.st_ino,
            metadata_before.st_size,
            metadata_before.st_mtime_ns,
            metadata_before.st_ctime_ns,
        )
        identity_after = (
            metadata_after.st_dev,
            metadata_after.st_ino,
            metadata_after.st_size,
            metadata_after.st_mtime_ns,
            metadata_after.st_ctime_ns,
        )
        if identity_before != identity_after:
            raise BootArtifactPreflightError(f"{context} changed while reading")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def _read_regular_nofollow(
    path: pathlib.Path, context: str, *, executable: bool = False
) -> bytes:
    absolute = _normalized_absolute(path)
    if absolute.name in {"", ".", ".."}:
        raise BootArtifactPreflightError(f"{context} path is invalid")
    parent_descriptor = _open_directory_nofollow(absolute.parent, f"{context} parent")
    try:
        raw = _read_regular_at(
            parent_descriptor,
            absolute.name,
            context,
            executable=executable,
        )
        assert raw is not None
        return raw
    finally:
        os.close(parent_descriptor)


def _load_canonical(path: pathlib.Path, context: str) -> tuple[dict[str, Any], bytes]:
    raw = _read_regular_nofollow(path, context)
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_duplicate_keys)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise BootArtifactPreflightError(f"{context} is not JSON") from exc
    if not isinstance(value, dict) or canonical_json(value) != raw:
        raise BootArtifactPreflightError(f"{context} is not canonical exact JSON")
    return value, raw


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise BootArtifactPreflightError(f"{context} keys differ")
    return value


def _digest(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise BootArtifactPreflightError(f"{context} must be a lowercase SHA-256")
    return value


def _size(value: Any, context: str, *, allow_null: bool = False) -> Optional[int]:
    if allow_null and value is None:
        return None
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise BootArtifactPreflightError(f"{context} must be a positive integer")
    return value


def _pin(value: Any, name: str, expected_format: str) -> dict[str, Any]:
    pin = _exact(value, {"format", "sha256", "sizeBytes"}, f"inputs.{name}")
    if pin["format"] != expected_format:
        raise BootArtifactPreflightError(f"inputs.{name}.format differs")
    digest = pin["sha256"]
    size = _size(pin["sizeBytes"], f"inputs.{name}.sizeBytes", allow_null=True)
    if digest is None:
        if size is not None:
            raise BootArtifactPreflightError(f"inputs.{name} is only partly pinned")
    else:
        _digest(digest, f"inputs.{name}.sha256")
        if size is None:
            raise BootArtifactPreflightError(f"inputs.{name} is only partly pinned")
    return pin


def _unresolved_authority(value: Any, name: str, expected_format: str) -> dict[str, Any]:
    pin = _pin(value, name, expected_format)
    if pin["sha256"] is not None or pin["sizeBytes"] is not None:
        raise BootArtifactPreflightError(
            f"inputs.{name} cannot be populated before its authority contract exists"
        )
    return pin


def _validate_plan(value: Any) -> dict[str, Any]:
    plan = _exact(
        value,
        {
            "activationAllowed",
            "bootFormatVersion",
            "guestExecutionPolicy",
            "guestDownloadMaxBytes",
            "inputs",
            "release",
            "rootfsSourceLock",
            "schema",
        },
        "boot artifact plan",
    )
    if plan["schema"] != PLAN_SCHEMA:
        raise BootArtifactPreflightError("boot artifact plan schema differs")
    if plan["activationAllowed"] is not False:
        raise BootArtifactPreflightError("boot artifact plan must not allow activation")
    if plan["bootFormatVersion"] != 1:
        raise BootArtifactPreflightError("bootFormatVersion differs")
    if plan["guestDownloadMaxBytes"] != 2_147_483_648:
        raise BootArtifactPreflightError("guest download cap differs")
    if not isinstance(plan["release"], str) or not plan["release"]:
        raise BootArtifactPreflightError("boot artifact plan release is empty")
    rootfs = _exact(
        plan["rootfsSourceLock"],
        {"artifactCount", "sha256", "totalBytes"},
        "rootfsSourceLock",
    )
    _digest(rootfs["sha256"], "rootfsSourceLock.sha256")
    _size(rootfs["artifactCount"], "rootfsSourceLock.artifactCount")
    _size(rootfs["totalBytes"], "rootfsSourceLock.totalBytes")
    policy = _exact(
        plan["guestExecutionPolicy"],
        {"cgroupParent", "sha256", "systemdRequired", "unitName"},
        "guestExecutionPolicy",
    )
    _digest(policy["sha256"], "guestExecutionPolicy.sha256")
    if policy["systemdRequired"] is not True:
        raise BootArtifactPreflightError("guest execution policy must require systemd")
    if policy["unitName"] != SYSTEMD_UNIT_NAME:
        raise BootArtifactPreflightError("guest execution policy unit name differs")
    if policy["cgroupParent"] != SYSTEMD_CGROUP_PARENT:
        raise BootArtifactPreflightError("guest execution policy cgroup parent differs")
    inputs = _exact(
        plan["inputs"],
        {"imageBuilderToolchain", "kernel", "systemdGuestClosure"},
        "inputs",
    )
    _unresolved_authority(
        inputs["imageBuilderToolchain"],
        "imageBuilderToolchain",
        "initrd-ext4-builder-authority-v1",
    )
    _unresolved_authority(inputs["kernel"], "kernel", "linux-arm64-image")
    _unresolved_authority(
        inputs["systemdGuestClosure"],
        "systemdGuestClosure",
        "systemd-rootfs-closure-authority-v1",
    )
    return plan


def load_plan(path: pathlib.Path) -> dict[str, Any]:
    value, _ = _load_canonical(path, "boot artifact plan")
    return _validate_plan(value)


def _validate_source_lock(
    value: Any, raw: bytes, plan: dict[str, Any]
) -> list[dict[str, Any]]:
    if value.get("schema") != LOCK_SCHEMA:
        raise BootArtifactPreflightError("rootfs source lock schema differs")
    if value.get("activationAllowed") is not False:
        raise BootArtifactPreflightError("rootfs source lock must not allow activation")
    if hashlib.sha256(raw).hexdigest() != plan["rootfsSourceLock"]["sha256"]:
        raise BootArtifactPreflightError("rootfs source lock digest differs")
    platform = value.get("platform")
    if not isinstance(platform, dict) or platform != {
        "debArchitecture": "arm64",
        "ociArchitecture": "arm64",
        "os": "linux",
        "rustTarget": "aarch64-unknown-linux-gnu",
    }:
        raise BootArtifactPreflightError("rootfs source lock platform differs")
    rows = value.get("artifacts")
    if not isinstance(rows, list):
        raise BootArtifactPreflightError("rootfs source lock artifacts are not a list")
    artifacts: list[dict[str, Any]] = []
    seen: set[str] = set()
    identifiers: list[str] = []
    for index, raw_row in enumerate(rows):
        row = _exact(raw_row, {"id", "kind", "sha256", "sizeBytes"}, f"artifacts[{index}]")
        identifier = row["id"]
        if not isinstance(identifier, str) or not identifier or identifier in seen:
            raise BootArtifactPreflightError("rootfs artifact IDs are invalid or duplicated")
        if not isinstance(row["kind"], str) or not row["kind"]:
            raise BootArtifactPreflightError(f"rootfs artifact kind is invalid: {identifier}")
        _digest(row["sha256"], f"rootfs artifact digest: {identifier}")
        _size(row["sizeBytes"], f"rootfs artifact size: {identifier}")
        seen.add(identifier)
        identifiers.append(identifier)
        artifacts.append(row)
    if identifiers != sorted(identifiers):
        raise BootArtifactPreflightError("rootfs artifacts are not sorted by ID")
    if len(artifacts) != plan["rootfsSourceLock"]["artifactCount"]:
        raise BootArtifactPreflightError("rootfs artifact count differs")
    if sum(row["sizeBytes"] for row in artifacts) != plan["rootfsSourceLock"]["totalBytes"]:
        raise BootArtifactPreflightError("rootfs artifact byte total differs")
    return artifacts


def _validate_execution_policy(value: Any, raw: bytes, plan: dict[str, Any]) -> None:
    if value.get("schema") != POLICY_SCHEMA:
        raise BootArtifactPreflightError("guest execution policy schema differs")
    if value.get("activationAllowed") is not False:
        raise BootArtifactPreflightError("guest execution policy must not allow activation")
    if hashlib.sha256(raw).hexdigest() != plan["guestExecutionPolicy"]["sha256"]:
        raise BootArtifactPreflightError("guest execution policy digest differs")
    platform = value.get("platform")
    if not isinstance(platform, dict):
        raise BootArtifactPreflightError("guest execution policy platform is absent")
    if platform.get("operatingSystem") != "linux":
        raise BootArtifactPreflightError("guest execution policy operating system differs")
    if platform.get("architecture") != "aarch64":
        raise BootArtifactPreflightError("guest execution policy architecture differs")
    if platform.get("systemdRequired") is not True:
        raise BootArtifactPreflightError("guest execution policy does not require systemd")
    crash_recovery = value.get("crashRecovery")
    if (
        not isinstance(crash_recovery, dict)
        or crash_recovery.get("cgroupParent") != SYSTEMD_CGROUP_PARENT
    ):
        raise BootArtifactPreflightError("guest execution policy cgroup parent differs")
    privilege = value.get("privilege")
    systemd_unit = privilege.get("systemdUnit") if isinstance(privilege, dict) else None
    if (
        not isinstance(systemd_unit, dict)
        or systemd_unit.get("UnitName") != SYSTEMD_UNIT_NAME
    ):
        raise BootArtifactPreflightError("guest execution policy systemd unit differs")


def _cas_sha_directory(cas: pathlib.Path) -> int:
    return _open_directory_nofollow(cas / "sha256", "CAS sha256")


def _artifact_state(
    artifact: dict[str, Any], sha_directories: list[int]
) -> tuple[str, int]:
    digest = artifact["sha256"]
    matches: list[bytes] = []
    for directory in sha_directories:
        raw = _read_regular_at(
            directory,
            digest,
            f"CAS artifact {artifact['id']}",
            missing_ok=True,
        )
        if raw is None:
            continue
        if len(raw) != artifact["sizeBytes"] or hashlib.sha256(raw).hexdigest() != digest:
            raise BootArtifactPreflightError(
                f"CAS artifact digest/size differs: {artifact['id']}"
            )
        matches.append(raw)
    if not matches:
        return "missing", 0
    if any(raw != matches[0] for raw in matches[1:]):
        raise BootArtifactPreflightError(f"CAS roots disagree: {artifact['id']}")
    return "present", len(matches[0])


def audit_inputs(
    plan_path: pathlib.Path,
    source_lock_path: pathlib.Path,
    execution_policy_path: pathlib.Path,
    cas_roots: list[pathlib.Path],
) -> dict[str, Any]:
    plan, plan_raw = _load_canonical(plan_path, "boot artifact plan")
    plan = _validate_plan(plan)
    lock, lock_raw = _load_canonical(source_lock_path, "rootfs source lock")
    artifacts = _validate_source_lock(lock, lock_raw, plan)
    policy, policy_raw = _load_canonical(
        execution_policy_path, "guest execution policy"
    )
    _validate_execution_policy(policy, policy_raw, plan)
    sha_directories: list[int] = []
    try:
        for path in cas_roots:
            sha_directories.append(_cas_sha_directory(path))
        present_count = 0
        present_bytes = 0
        missing: list[dict[str, Any]] = []
        for artifact in artifacts:
            state, size = _artifact_state(artifact, sha_directories)
            if state == "present":
                present_count += 1
                present_bytes += size
            else:
                missing.append(artifact)
    finally:
        for descriptor in sha_directories:
            os.close(descriptor)

    missing_authorities = sorted(plan["inputs"])
    missing_bytes = sum(row["sizeBytes"] for row in missing)
    return {
        "activationAllowed": False,
        "artifactsWritten": 0,
        "bootableClaim": False,
        "inputSummary": {
            "expectedArtifacts": len(artifacts),
            "expectedBytes": sum(row["sizeBytes"] for row in artifacts),
            "invalidArtifacts": 0,
            "missingArtifacts": len(missing),
            "missingBytes": missing_bytes,
            "presentArtifacts": present_count,
            "presentBytes": present_bytes,
        },
        "guestExecutionPolicySha256": hashlib.sha256(policy_raw).hexdigest(),
        "missingArtifactIds": sorted(row["id"] for row in missing),
        "missingInputAuthorities": missing_authorities,
        "planSha256": hashlib.sha256(plan_raw).hexdigest(),
        "rootfsSourceLockSha256": hashlib.sha256(lock_raw).hexdigest(),
        "schema": RESULT_SCHEMA,
        "status": "BLOCKED_MISSING_INPUTS",
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    audit = subparsers.add_parser("audit")
    audit.add_argument("--plan", type=pathlib.Path, required=True)
    audit.add_argument("--source-lock", type=pathlib.Path, required=True)
    audit.add_argument("--execution-policy", type=pathlib.Path, required=True)
    audit.add_argument("--cas", type=pathlib.Path, action="append", required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = audit_inputs(
            args.plan,
            args.source_lock,
            args.execution_policy,
            args.cas,
        )
    except BootArtifactPreflightError as exc:
        print(f"native-shadow boot artifact preflight: {exc}", file=sys.stderr)
        return 2
    sys.stdout.buffer.write(canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
