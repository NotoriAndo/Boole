#!/usr/bin/env python3
"""Freeze a signed-metadata ARM64 dependency candidate without boot claims."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import secrets
import stat
import sys
import types
from dataclasses import dataclass
from typing import Any, Optional


SCRIPT_ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPT_ROOT))


PLAN_SCHEMA = "boole.native-shadow.boot-rootfs-dependency-candidate-plan.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-rootfs-dependency-candidate-result.arm64.v1"
STATUS = "DEPENDENCY-CANDIDATE-FROZEN-NOT-BOOT-AUTHORITY"


class DependencyCandidateError(RuntimeError):
    """The candidate inputs differ from the pre-frozen non-authoritative plan."""


@dataclass(frozen=True)
class RuntimeModules:
    """Authority modules compiled from the exact pinned source bytes."""

    acquirer: types.ModuleType
    builder: types.ModuleType
    resolver: types.ModuleType


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False)
        + "\n"
    ).encode("utf-8")


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise DependencyCandidateError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _read_regular_nofollow(path: pathlib.Path, context: str) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(os.fspath(path), flags)
    except OSError as exc:
        raise DependencyCandidateError(f"cannot open {context} without following links") from exc
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise DependencyCandidateError(f"{context} is not a regular file")
        chunks: list[bytes] = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise DependencyCandidateError(f"{context} was truncated")
            chunks.append(chunk)
            remaining -= len(chunk)
        after = os.fstat(descriptor)
        if (
            before.st_dev,
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ):
            raise DependencyCandidateError(f"{context} changed while reading")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def load_plan(path: pathlib.Path) -> dict[str, Any]:
    raw = _read_regular_nofollow(path, "candidate plan")
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_duplicate_keys)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise DependencyCandidateError("candidate plan is not canonical JSON") from exc
    if not isinstance(value, dict) or canonical_json(value) != raw:
        raise DependencyCandidateError("candidate plan is not byte-exact canonical JSON")
    return value


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise DependencyCandidateError(f"{context} keys differ")
    return value


def _digest(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise DependencyCandidateError(f"{context} must be a lowercase SHA-256")
    return value


def _size(value: Any, context: str, *, allow_zero: bool = False) -> int:
    minimum = 0 if allow_zero else 1
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise DependencyCandidateError(f"{context} size/count differs")
    return value


def _pin(value: Any, context: str) -> dict[str, Any]:
    pin = _exact(value, {"sha256", "sizeBytes"}, context)
    _digest(pin["sha256"], f"{context}.sha256")
    _size(pin["sizeBytes"], f"{context}.sizeBytes")
    return pin


def read_pinned_input(path: pathlib.Path, pin: dict[str, Any], context: str) -> bytes:
    expected = _pin(pin, context)
    raw = _read_regular_nofollow(path, context)
    if len(raw) != expected["sizeBytes"] or _sha(raw) != expected["sha256"]:
        raise DependencyCandidateError(f"{context} bytes differ")
    return raw


def write_bytes_atomic(path: pathlib.Path, raw: bytes) -> None:
    absolute = pathlib.Path(os.path.abspath(os.fspath(path)))
    parent = absolute.parent
    if parent.resolve() != parent or not parent.is_dir():
        raise DependencyCandidateError("output parent must be a normalized real directory")
    directory_flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(
        os, "O_CLOEXEC", 0
    )
    directory_descriptor = os.open(os.fspath(parent), directory_flags)
    temporary_name = f".{path.name}.{os.getpid()}.{secrets.token_hex(16)}"
    try:
        try:
            metadata = os.stat(
                absolute.name, dir_fd=directory_descriptor, follow_symlinks=False
            )
        except FileNotFoundError:
            metadata = None
        if metadata is not None and not stat.S_ISREG(metadata.st_mode):
            raise DependencyCandidateError("output is not a regular file")
        flags = (
            os.O_WRONLY
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        descriptor = os.open(
            temporary_name, flags, 0o600, dir_fd=directory_descriptor
        )
        with os.fdopen(descriptor, "wb", closefd=True) as target:
            target.write(raw)
            target.flush()
            os.fsync(target.fileno())
        os.replace(
            temporary_name,
            absolute.name,
            src_dir_fd=directory_descriptor,
            dst_dir_fd=directory_descriptor,
        )
        os.fsync(directory_descriptor)
    except OSError as exc:
        raise DependencyCandidateError("atomic output failed") from exc
    finally:
        try:
            os.unlink(temporary_name, dir_fd=directory_descriptor)
        except FileNotFoundError:
            pass
        finally:
            os.close(directory_descriptor)
    if _read_regular_nofollow(absolute, "output") != raw:
        raise DependencyCandidateError("atomic output bytes differ")


def _exec_source_module(
    name: str, path: pathlib.Path, raw: bytes
) -> types.ModuleType:
    module = types.ModuleType(name)
    module.__file__ = str(path)
    module.__package__ = "scripts"
    exec(compile(raw, str(path), "exec"), module.__dict__)
    return module


def load_pinned_runtime(
    repository_root: pathlib.Path, authority_inputs: dict[str, Any]
) -> RuntimeModules:
    """Compile the runtime only from no-follow reads of the pinned source bytes."""

    if repository_root != SCRIPT_ROOT:
        raise DependencyCandidateError("running repository root differs")
    source_paths = {
        "acquirer": repository_root
        / "scripts/native_shadow_rootfs_acquire_arm64_v1.py",
        "builder": repository_root
        / "scripts/native_shadow_rootfs_builder_arm64_v1.py",
        "resolver": repository_root
        / "scripts/native_shadow_boot_rootfs_resolver_v2.py",
    }
    sources = {
        name: read_pinned_input(path, authority_inputs[name], name)
        for name, path in source_paths.items()
    }

    package_name = "scripts"
    module_names = {
        "builder": f"{package_name}.native_shadow_rootfs_builder_arm64_v1",
        "acquirer": f"{package_name}.native_shadow_rootfs_acquire_arm64_v1",
        "resolver": f"{package_name}.native_shadow_boot_rootfs_resolver_v2",
    }
    sentinel = object()
    previous_modules = {
        name: sys.modules.get(name, sentinel) for name in module_names.values()
    }
    package = sys.modules.get(package_name)
    created_package = package is None
    if package is None:
        package = types.ModuleType(package_name)
        package.__path__ = [str(repository_root / "scripts")]
        sys.modules[package_name] = package
    previous_attributes = {
        short: getattr(package, name.rsplit(".", 1)[1], sentinel)
        for short, name in module_names.items()
    }
    loaded: dict[str, types.ModuleType] = {}
    try:
        for short in ("builder", "acquirer", "resolver"):
            name = module_names[short]
            module = _exec_source_module(name, source_paths[short], sources[short])
            loaded[short] = module
            sys.modules[name] = module
            setattr(package, name.rsplit(".", 1)[1], module)
    except (OSError, RuntimeError, SyntaxError) as exc:
        raise DependencyCandidateError("pinned runtime source could not be loaded") from exc
    finally:
        for short, name in module_names.items():
            previous = previous_modules[name]
            if previous is sentinel:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = previous
            attribute = name.rsplit(".", 1)[1]
            previous_attribute = previous_attributes[short]
            if previous_attribute is sentinel:
                try:
                    delattr(package, attribute)
                except AttributeError:
                    pass
            else:
                setattr(package, attribute, previous_attribute)
        if created_package:
            sys.modules.pop(package_name, None)
    return RuntimeModules(
        acquirer=loaded["acquirer"],
        builder=loaded["builder"],
        resolver=loaded["resolver"],
    )


def _tracked_runtime() -> RuntimeModules:
    plan = load_plan(
        SCRIPT_ROOT
        / "native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json"
    )
    authority = plan.get("authorityInputs")
    if not isinstance(authority, dict):
        raise DependencyCandidateError("authorityInputs keys differ")
    return load_pinned_runtime(SCRIPT_ROOT, authority)


def resolve_for_test(packages_raw: bytes, seeds: list[str]) -> dict[str, Any]:
    """Exercise the same public resolver surface with synthetic signed-index rows."""

    runtime = _tracked_runtime()
    return runtime.resolver.resolve_package_closure_v2(
        packages_raw,
        sorted(seeds),
        "noble-main",
        "main",
        target_os="linux",
        target_architecture="arm64",
        virtual_provider_pins={},
    )


def plan_for_test(
    *,
    seeds: list[str],
    baseline: dict[str, Any],
    expected_package_count: int,
    expected_package_bytes: int,
) -> dict[str, Any]:
    baseline_bytes = sum(row["artifactSizeBytes"] for row in baseline["packages"])
    pin = {"sha256": "0" * 64, "sizeBytes": 1}
    return {
        "activationAllowed": False,
        "authorityInputs": {
            "acquirer": dict(pin),
            "acquisitionPlan": dict(pin),
            "baselineResolution": dict(pin),
            "baselineSourceLock": dict(pin),
            "builder": dict(pin),
            "candidateGenerator": dict(pin),
            "guestInitCompatibility": dict(pin),
            "portablePlan": dict(pin),
            "resolver": dict(pin),
        },
        "boundaries": {
            "bootAuthority": False,
            "imageBuilderAuthorityPresent": False,
            "kernelImageExtracted": False,
            "launcherElfPresent": False,
            "maintainerScriptsExecuted": False,
            "packagePayloadsAcquired": False,
            "packagePayloadsVerified": False,
            "runtimeCompatibilityVerified": False,
        },
        "bootArtifactsWritten": 0,
        "bootableClaim": False,
        "expected": {
            "baselinePackageBytes": baseline_bytes,
            "baselinePackageCount": len(baseline["packages"]),
            "candidatePackageBytes": expected_package_bytes,
            "candidatePackageCount": expected_package_count,
            "newPackageBytes": expected_package_bytes - baseline_bytes,
            "newPackageCount": expected_package_count - len(baseline["packages"]),
        },
        "release": "TEST-ONLY",
        "requiredPackages": {},
        "repository": {
            "architecture": "arm64",
            "component": "main",
            "id": "noble-main",
            "inRelease": {"path": "dists/noble/InRelease", **pin},
            "packagesIndex": {
                "path": "dists/noble/main/binary-arm64/Packages.xz",
                "releasePath": "main/binary-arm64/Packages.xz",
                **pin,
            },
            "snapshotId": "test-snapshot",
            "snapshotTime": "2024-04-25T16:00:00Z",
            "trustedUbuntuFingerprints": ["F" * 40],
        },
        "schema": PLAN_SCHEMA,
        "selection": {
            "seeds": sorted(seeds),
            "virtualProviderPins": {},
        },
        "target": {"architecture": "arm64", "os": "linux"},
        "toolDigests": {"gpgvSha256": "0" * 64, "zstdSha256": "0" * 64},
    }


def build_candidate(
    plan: dict[str, Any],
    baseline: dict[str, Any],
    packages_raw: bytes,
    *,
    signed_metadata_verified: bool,
    runtime: Optional[RuntimeModules] = None,
) -> dict[str, Any]:
    expected_plan_keys = {
        "activationAllowed",
        "authorityInputs",
        "boundaries",
        "bootArtifactsWritten",
        "bootableClaim",
        "expected",
        "release",
        "requiredPackages",
        "repository",
        "schema",
        "selection",
        "target",
        "toolDigests",
    }
    if set(plan) != expected_plan_keys:
        raise DependencyCandidateError("candidate plan keys differ")
    if plan.get("schema") != PLAN_SCHEMA:
        raise DependencyCandidateError("candidate plan schema differs")
    if not isinstance(plan.get("release"), str) or not plan["release"]:
        raise DependencyCandidateError("candidate plan release differs")
    if (
        plan.get("activationAllowed") is not False
        or plan.get("bootableClaim") is not False
        or plan.get("bootArtifactsWritten") != 0
    ):
        raise DependencyCandidateError("candidate plan attempts a boot or activation claim")
    if plan.get("target") != {"architecture": "arm64", "os": "linux"}:
        raise DependencyCandidateError("candidate target differs")
    authority_inputs = _exact(
        plan.get("authorityInputs"),
        {
            "acquirer",
            "acquisitionPlan",
            "baselineResolution",
            "baselineSourceLock",
            "builder",
            "candidateGenerator",
            "guestInitCompatibility",
            "portablePlan",
            "resolver",
        },
        "authorityInputs",
    )
    for name, value in authority_inputs.items():
        _pin(value, f"authorityInputs.{name}")
    boundaries = _exact(
        plan.get("boundaries"),
        {
            "bootAuthority",
            "imageBuilderAuthorityPresent",
            "kernelImageExtracted",
            "launcherElfPresent",
            "maintainerScriptsExecuted",
            "packagePayloadsAcquired",
            "packagePayloadsVerified",
            "runtimeCompatibilityVerified",
        },
        "boundaries",
    )
    if any(value is not False for value in boundaries.values()):
        raise DependencyCandidateError("candidate boundary attempts an authority claim")
    expected = _exact(
        plan.get("expected"),
        {
            "baselinePackageBytes",
            "baselinePackageCount",
            "candidatePackageBytes",
            "candidatePackageCount",
            "newPackageBytes",
            "newPackageCount",
        },
        "expected",
    )
    for name, value in expected.items():
        _size(value, f"expected.{name}", allow_zero=name.startswith("new"))
    repository = _exact(
        plan.get("repository"),
        {
            "architecture",
            "component",
            "id",
            "inRelease",
            "packagesIndex",
            "snapshotId",
            "snapshotTime",
            "trustedUbuntuFingerprints",
        },
        "repository",
    )
    if (
        repository["architecture"] != "arm64"
        or repository["component"] != "main"
        or repository["id"] != "noble-main"
        or not isinstance(repository["snapshotId"], str)
        or not repository["snapshotId"]
        or not isinstance(repository["snapshotTime"], str)
        or not repository["snapshotTime"]
    ):
        raise DependencyCandidateError("repository identity differs")
    inrelease = _exact(
        repository["inRelease"], {"path", "sha256", "sizeBytes"}, "repository.inRelease"
    )
    packages_index = _exact(
        repository["packagesIndex"],
        {"path", "releasePath", "sha256", "sizeBytes"},
        "repository.packagesIndex",
    )
    _digest(inrelease["sha256"], "repository.inRelease.sha256")
    _size(inrelease["sizeBytes"], "repository.inRelease.sizeBytes")
    _digest(packages_index["sha256"], "repository.packagesIndex.sha256")
    _size(packages_index["sizeBytes"], "repository.packagesIndex.sizeBytes")
    fingerprints = repository["trustedUbuntuFingerprints"]
    if (
        not isinstance(fingerprints, list)
        or not fingerprints
        or any(
            not isinstance(value, str)
            or len(value) != 40
            or any(character not in "0123456789ABCDEF" for character in value)
            for value in fingerprints
        )
    ):
        raise DependencyCandidateError("trusted Ubuntu fingerprints differ")
    tools = _exact(
        plan.get("toolDigests"), {"gpgvSha256", "zstdSha256"}, "toolDigests"
    )
    _digest(tools["gpgvSha256"], "toolDigests.gpgvSha256")
    _digest(tools["zstdSha256"], "toolDigests.zstdSha256")
    if signed_metadata_verified is not True:
        raise DependencyCandidateError("signed repository metadata was not verified")
    selection = _exact(
        plan.get("selection"), {"seeds", "virtualProviderPins"}, "selection"
    )
    runtime = runtime or _tracked_runtime()
    try:
        resolved = runtime.resolver.resolve_package_closure_v2(
            packages_raw,
            selection["seeds"],
            "noble-main",
            "main",
            target_os="linux",
            target_architecture="arm64",
            virtual_provider_pins=selection["virtualProviderPins"],
        )
    except (KeyError, runtime.resolver.ResolverV2Error) as exc:
        raise DependencyCandidateError(str(exc)) from exc

    candidate_by_id = {row["packageId"]: row for row in resolved["packages"]}
    candidate_by_name = {row["name"]: row for row in resolved["packages"]}
    if len(candidate_by_name) != len(resolved["packages"]):
        raise DependencyCandidateError("candidate package name is ambiguous")
    required_packages = plan.get("requiredPackages")
    if not isinstance(required_packages, dict):
        raise DependencyCandidateError("requiredPackages keys differ")
    for name, identity in required_packages.items():
        expected_identity = _exact(
            identity,
            {"artifactSha256", "artifactSizeBytes", "packageId", "version"},
            f"requiredPackages.{name}",
        )
        _digest(expected_identity["artifactSha256"], f"requiredPackages.{name}.artifactSha256")
        _size(expected_identity["artifactSizeBytes"], f"requiredPackages.{name}.artifactSizeBytes")
        row = candidate_by_name.get(name)
        actual_identity = (
            {
                "artifactSha256": row["artifactSha256"],
                "artifactSizeBytes": row["artifactSizeBytes"],
                "packageId": row["packageId"],
                "version": row["version"],
            }
            if row is not None
            else None
        )
        if actual_identity != expected_identity:
            raise DependencyCandidateError(f"required package identity differs: {name}")
    baseline_ids: list[str] = []
    for row in baseline.get("packages", []):
        package_id = row.get("packageId")
        if package_id not in candidate_by_id or candidate_by_id[package_id] != row:
            raise DependencyCandidateError("baseline is not an exact-row candidate subset")
        baseline_ids.append(package_id)
    if len(baseline_ids) != len(set(baseline_ids)):
        raise DependencyCandidateError("baseline package identity is duplicated")

    candidate_bytes = sum(row["artifactSizeBytes"] for row in resolved["packages"])
    baseline_bytes = sum(row["artifactSizeBytes"] for row in baseline["packages"])
    new_ids = sorted(set(candidate_by_id) - set(baseline_ids))
    actual = {
        "baselinePackageBytes": baseline_bytes,
        "baselinePackageCount": len(baseline_ids),
        "candidatePackageBytes": candidate_bytes,
        "candidatePackageCount": len(candidate_by_id),
        "newPackageBytes": candidate_bytes - baseline_bytes,
        "newPackageCount": len(new_ids),
    }
    if expected != actual:
        raise DependencyCandidateError("candidate count/byte conservation differs")

    return {
        "activationAllowed": False,
        "authorityInputs": authority_inputs,
        "bootArtifactsWritten": 0,
        "bootableClaim": False,
        "counts": {
            "baselinePackageBytes": baseline_bytes,
            "baselinePackages": len(baseline_ids),
            "candidatePackageBytes": candidate_bytes,
            "candidatePackages": len(candidate_by_id),
            "newPackageBytes": candidate_bytes - baseline_bytes,
            "newPackages": len(new_ids),
        },
        "boundaries": boundaries,
        "newPackageIds": new_ids,
        "planSha256": _sha(canonical_json(plan)),
        "productionByteProvenanceComplete": False,
        "repository": repository,
        "requiredPackages": required_packages,
        "resolution": resolved,
        "schema": RESULT_SCHEMA,
        "selection": selection,
        "signedRepositoryMetadataVerified": True,
        "status": STATUS,
        "toolDigests": tools,
    }


def _canonical_object(raw: bytes, context: str) -> dict[str, Any]:
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_duplicate_keys)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise DependencyCandidateError(f"{context} is not canonical JSON") from exc
    if not isinstance(value, dict) or canonical_json(value) != raw:
        raise DependencyCandidateError(f"{context} is not byte-exact canonical JSON")
    return value


def _repository_from_acquisition_plan(acquisition: dict[str, Any]) -> dict[str, Any]:
    repository = acquisition["repository"]
    return {
        "architecture": repository["architecture"],
        "component": repository["component"],
        "id": repository["id"],
        "inRelease": {
            "path": repository["inRelease"]["path"],
            "sha256": repository["inRelease"]["sha256"],
            "sizeBytes": repository["inRelease"]["sizeBytes"],
        },
        "packagesIndex": {
            "path": repository["packagesIndex"]["path"],
            "releasePath": repository["packagesIndex"]["releasePath"],
            "sha256": repository["packagesIndex"]["sha256"],
            "sizeBytes": repository["packagesIndex"]["sizeBytes"],
        },
        "snapshotId": acquisition["snapshotId"],
        "snapshotTime": acquisition["snapshotTime"],
        "trustedUbuntuFingerprints": acquisition["trustedUbuntuFingerprints"],
    }


def generate_from_cas(
    *,
    plan_path: pathlib.Path,
    repository_root: pathlib.Path,
    cas: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> dict[str, Any]:
    """Replay signed metadata and return a non-authoritative candidate only."""

    root = repository_root.resolve()
    if root != repository_root or not root.is_dir():
        raise DependencyCandidateError("repository root must be a normalized real directory")
    plan = load_plan(plan_path)
    authority = _exact(
        plan.get("authorityInputs"),
        {
            "acquirer",
            "acquisitionPlan",
            "baselineResolution",
            "baselineSourceLock",
            "builder",
            "candidateGenerator",
            "guestInitCompatibility",
            "portablePlan",
            "resolver",
        },
        "authorityInputs",
    )
    for name, value in authority.items():
        _pin(value, f"authorityInputs.{name}")
    runtime = load_pinned_runtime(root, authority)
    paths = {
        "acquisitionPlan": root
        / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json",
        "baselineResolution": root
        / "native/containment/native-shadow-runtime-rootfs-resolution-arm64-v1.json",
        "baselineSourceLock": root
        / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json",
        "candidateGenerator": root
        / "scripts/native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py",
        "guestInitCompatibility": root
        / "native/containment/native-shadow-guest-init-compatibility-arm64-v1.json",
        "portablePlan": root
        / "native/containment/native-shadow-runtime-rootfs-portable-plan-arm64-v1.json",
    }
    raw_inputs = {
        name: read_pinned_input(path, authority[name], name)
        for name, path in paths.items()
    }

    try:
        acquisition = runtime.acquirer.load_plan(
            raw_inputs["acquisitionPlan"],
            root / "scripts/native_shadow_rootfs_builder_arm64_v1.py",
            root / "scripts/native_shadow_rootfs_acquire_arm64_v1.py",
        )
    except (runtime.acquirer.AcquisitionError, runtime.builder.RootfsBuildError) as exc:
        raise DependencyCandidateError(str(exc)) from exc
    if plan["repository"] != _repository_from_acquisition_plan(acquisition):
        raise DependencyCandidateError("signed repository identity differs from candidate plan")
    if plan["toolDigests"] != acquisition["toolDigests"]:
        raise DependencyCandidateError("signed metadata tool digests differ")

    gpgv = gpgv_path.resolve()
    zstd = zstd_path.resolve()
    try:
        replayed_baseline = runtime.acquirer.resolve_from_cas(
            acquisition,
            cas,
            gpgv,
            zstd,
        )
    except (runtime.acquirer.AcquisitionError, runtime.builder.RootfsBuildError) as exc:
        raise DependencyCandidateError(str(exc)) from exc
    baseline = _canonical_object(raw_inputs["baselineResolution"], "baseline resolution")
    replay_digest = _sha(runtime.builder.canonical_json(replayed_baseline))
    if (
        baseline.get("bootstrapResolutionV1Sha256") != replay_digest
        or baseline.get("snapshotId") != replayed_baseline["snapshotId"]
        or baseline.get("snapshotTime") != replayed_baseline["snapshotTime"]
        or baseline.get("keyring") != replayed_baseline["keyring"]
        or baseline.get("seedPackageIds") != replayed_baseline["seedPackageIds"]
        or baseline.get("packages") != replayed_baseline["packages"]
    ):
        raise DependencyCandidateError("baseline resolution does not match signed replay")

    index_spec = acquisition["repository"]["packagesIndex"]
    index_raw = runtime.acquirer._IMPL["_verified_cas_artifact"](cas, index_spec)
    try:
        packages_raw = runtime.builder._packages_payload(
            index_raw,
            index_spec["releasePath"],
            acquisition["networkPolicy"]["maxTotalBytes"],
        )
    except runtime.builder.RootfsBuildError as exc:
        raise DependencyCandidateError(str(exc)) from exc
    return build_candidate(
        plan,
        baseline,
        packages_raw,
        signed_metadata_verified=True,
        runtime=runtime,
    )


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--plan", type=pathlib.Path, required=True)
    parser.add_argument("--repository-root", type=pathlib.Path, required=True)
    parser.add_argument("--cas", type=pathlib.Path, required=True)
    parser.add_argument("--gpgv", type=pathlib.Path, required=True)
    parser.add_argument("--zstd", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        result = generate_from_cas(
            plan_path=args.plan,
            repository_root=args.repository_root,
            cas=args.cas,
            gpgv_path=args.gpgv,
            zstd_path=args.zstd,
        )
        write_bytes_atomic(args.output, canonical_json(result))
    except DependencyCandidateError as exc:
        print(f"native-shadow boot dependency candidate: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
