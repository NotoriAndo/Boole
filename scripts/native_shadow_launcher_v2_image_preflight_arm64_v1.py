#!/usr/bin/env python3
"""Repeatable launcher-v2 staging preflight with no image-production authority.

This module proves that the exact sealed launcher-v2 ELF can be placed in the
already sealed successor rootfs tree, that the assembled table and an
independent traversal agree, and that the result remains inside all three
frozen limits.  It deliberately has no image builder, boot entry point,
attempt-consumption marker, or activation path.
"""

from __future__ import annotations

import argparse
import ast
import errno
import hashlib
import json
import os
import pathlib
import posixpath
import stat
import sys
import tempfile
from typing import Any, Mapping, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_staging_measure_arm64_v1 as measurement
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_v1
from scripts import native_shadow_rootfs_builder_boot_arm64_v4 as builder_v4
from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as release_gate_v2


IMAGE_PRODUCED_CLAIM = False
BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
ALLOWED_IMAGE_TOOLS = frozenset()
FORBIDDEN_OUTPUT_NAMES = (
    "ATTEMPT-CONSUMED.json",
    "guest-kernel",
    "guest-initrd",
    "guest-root-disk",
)

SCHEMA = "boole.native-shadow.launcher-v2-image-preflight.arm64.v1"
REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
PREREGISTRATION_PATH = (
    REPOSITORY_ROOT
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json"
)
PREREGISTRATION_SHA256 = (
    "bb51f61b044b9ff651282860eb8645dc97e9122bc446cf65f2489bfefbd73173"
)
SOURCE_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)
SOURCE_LOCK_SHA256 = (
    "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"
)
BUILDER_V4_PATH = pathlib.Path(builder_v4.__file__).resolve()
RELEASE_GATE_V2_PATH = pathlib.Path(release_gate_v2.__file__).resolve()
RELEASE_GATE_V2_SHA256 = (
    "15f88cf286879ae30aae10bb7819aefea91095a819d96c2634ee9ecc4ea2f305"
)
PREFLIGHT_WRAPPER_PATH = pathlib.Path(
    "scripts/native-shadow-launcher-v2-image-preflight-arm64.sh"
)
MEASUREMENT_PATH = pathlib.Path("scripts/native_shadow_boot_staging_measure_arm64_v1.py")
BASE_PROJECTION_PATH = pathlib.Path("scripts/native_shadow_rootfs_builder_boot_arm64_v1.py")
ISOLATION_ARGV_PATH = pathlib.Path("scripts/native_shadow_boot_image_produce_arm64_v1.py")
ISOLATION_AUTHORITY_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)

ASSEMBLER = builder_v4.materialize_staging_tree


class LauncherV2PreflightError(RuntimeError):
    """The free launcher-v2 staging preflight cannot establish its contract."""


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def canonical_json(document: Any) -> bytes:
    return builder_v4.canonical_json(document)


def _load_canonical(path: pathlib.Path, expected_sha256: str, label: str) -> dict:
    try:
        raw = pathlib.Path(path).read_bytes()
    except OSError as exc:
        raise LauncherV2PreflightError(f"{label} is unreadable: {path}") from exc
    actual = _sha256(raw)
    if actual != expected_sha256:
        raise LauncherV2PreflightError(
            f"{label} hashes to {actual}, the preflight pins {expected_sha256}"
        )
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise LauncherV2PreflightError(f"{label} is not JSON") from exc
    if raw != canonical_json(document):
        raise LauncherV2PreflightError(f"{label} is not canonical JSON")
    if not isinstance(document, dict):
        raise LauncherV2PreflightError(f"{label} is not an object")
    return document


def load_preregistration() -> dict:
    """Load the exact zero-authority record written before this implementation."""

    document = _load_canonical(
        PREREGISTRATION_PATH, PREREGISTRATION_SHA256, "launcher-v2 preregistration"
    )
    authorities = document.get("authorisations")
    if not isinstance(authorities, dict) or any(authorities.values()):
        raise LauncherV2PreflightError(
            "the preregistration grants an authority this preflight may not have"
        )
    if document.get("runsPerformed") != 0:
        raise LauncherV2PreflightError("the preregistration run count is not zero")
    boundary = document.get("preflight")
    if not isinstance(boundary, dict):
        raise LauncherV2PreflightError("the preregistration has no preflight boundary")
    if boundary.get("allowedImageTools") != []:
        raise LauncherV2PreflightError("the preregistration permits an image tool")
    if boundary.get("forbiddenNames") != list(FORBIDDEN_OUTPUT_NAMES):
        raise LauncherV2PreflightError("the preregistration output boundary differs")
    return document


def expected_projection() -> dict:
    projection = load_preregistration().get("expectedProjection")
    if not isinstance(projection, dict):
        raise LauncherV2PreflightError("the preregistration has no expected projection")
    return projection


def verify_bound_inputs(
    preregistration: Mapping[str, Any], repository_root: pathlib.Path
) -> list[dict[str, Any]]:
    """Re-read every S1 binding before any launcher or payload is opened."""

    rows = preregistration.get("bindings")
    if not isinstance(rows, list) or not rows:
        raise LauncherV2PreflightError("the preregistration has no bound inputs")
    repository_root = pathlib.Path(repository_root).resolve()
    observed: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            raise LauncherV2PreflightError("a bound input is not an object")
        relative = row.get("path")
        if not isinstance(relative, str) or not relative:
            raise LauncherV2PreflightError("a bound input has no path")
        pure = pathlib.PurePosixPath(relative)
        if pure.is_absolute() or ".." in pure.parts or pure.as_posix() != relative:
            raise LauncherV2PreflightError(f"bound input path is unsafe: {relative!r}")
        if relative in seen:
            raise LauncherV2PreflightError(f"bound input is repeated: {relative}")
        seen.add(relative)
        path = repository_root / pathlib.Path(*pure.parts)
        try:
            info = path.lstat()
        except OSError as exc:
            raise LauncherV2PreflightError(f"bound input is unreadable: {relative}") from exc
        if not stat.S_ISREG(info.st_mode) or path.is_symlink():
            raise LauncherV2PreflightError(f"bound input is not a regular file: {relative}")
        resolved = path.resolve()
        if not resolved.is_relative_to(repository_root):
            raise LauncherV2PreflightError(f"bound input leaves the repository: {relative}")
        raw = path.read_bytes()
        identity = {
            "path": relative,
            "sha256": _sha256(raw),
            "sizeBytes": len(raw),
        }
        expected = {
            "path": relative,
            "sha256": row.get("sha256"),
            "sizeBytes": row.get("sizeBytes"),
        }
        if identity != expected:
            raise LauncherV2PreflightError(
                f"bound input differs from preregistration: {relative}"
            )
        observed.append(identity)
    return sorted(observed, key=lambda item: item["path"].encode("utf-8"))


def require_projected_limits(totals: Mapping[str, Any]) -> dict:
    """Apply the preregistered limits without changing or deriving them."""

    limits = expected_projection()["limits"]
    result = dict(totals)
    for key, limit_key in (
        ("entries", "maxEntries"),
        ("largestFileBytes", "maxFileBytes"),
        ("payloadBytes", "maxTotalBytes"),
    ):
        value = result.get(key)
        limit = limits.get(limit_key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise LauncherV2PreflightError(f"projected {key} is unusable: {value!r}")
        if not isinstance(limit, int) or isinstance(limit, bool) or value > limit:
            raise LauncherV2PreflightError(
                f"projected {key} exceeds {limit_key}: {value!r} > {limit!r}"
            )
    return result


def require_expected_totals(totals: Mapping[str, Any]) -> dict:
    """Require the exact preregistered projection before applying its limits."""

    projected = {
        key: totals.get(key)
        for key in ("entries", "largestFileBytes", "payloadBytes")
    }
    expected = expected_projection()["withLauncherV2"]
    if projected != expected:
        raise LauncherV2PreflightError(
            f"launcher-v2 totals differ: {projected!r} != {expected!r}"
        )
    return require_projected_limits(projected)


def assemble(
    *,
    validated: dict,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Any],
) -> dict:
    """Forward the validated bytes to the one exported successor assembler."""

    return ASSEMBLER(
        validated,
        pathlib.Path(repository_root),
        pathlib.Path(artifact_store),
        launcher_binary=launcher_binary,
        nested_tree=nested_tree,
    )


def _assert_exact_module(path: pathlib.Path, expected: str, label: str) -> None:
    actual = _sha256(path.read_bytes())
    if actual != expected:
        raise LauncherV2PreflightError(
            f"{label} hashes to {actual}, the preflight pins {expected}"
        )


def _source_closure(
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> tuple[dict, dict]:
    _assert_exact_module(
        RELEASE_GATE_V2_PATH, RELEASE_GATE_V2_SHA256, "successor release gate"
    )
    source = _load_canonical(SOURCE_LOCK_PATH, SOURCE_LOCK_SHA256, "source lock")
    source_raw = SOURCE_LOCK_PATH.read_bytes()
    try:
        runtime, _receipt = release_gate_v2.materialize_runtime_lock(
            source, source_raw, pathlib.Path(gpgv), pathlib.Path(zstd)
        )
        lock, lock_raw, _normalization = boot_v1.normalized_runtime_lock(runtime)
        validated = builder_v4.validate_source_lock(
            lock,
            lock_raw,
            pathlib.Path(repository_root),
            pathlib.Path(artifact_store),
            require_complete=True,
        )
    except Exception as exc:
        if isinstance(exc, LauncherV2PreflightError):
            raise
        raise LauncherV2PreflightError(str(exc)) from exc
    return lock, validated


def _entry_without_raw(entry: Mapping[str, Any]) -> dict:
    return {key: value for key, value in entry.items() if key != "raw"}


def _baseline_without_launcher(
    successor: Mapping[str, Mapping[str, Any]],
) -> dict[str, Mapping[str, Any]]:
    """Remove the launcher and only the now-empty parents it caused to exist.

    Re-running the 1.77 GB assembler without the launcher would hold a second
    copy of most package bytes in memory.  The projection already proves that
    launcher insertion is its only changed input, so derive the sealed baseline
    from the one real assembly and then require every baseline total exactly.
    """

    result = dict(successor)
    launcher_path = expected_projection()["launcherGuestPath"].lstrip("/")
    launcher = result.pop(launcher_path, None)
    if not isinstance(launcher, dict) or launcher.get("kind") != "file":
        raise LauncherV2PreflightError("the assembled tree has no launcher-v2 file")
    projection = expected_projection()
    parent_count = (
        projection["withLauncherV2"]["entries"]
        - projection["withoutLauncher"]["entries"]
        - 1
    )
    if parent_count < 0:
        raise LauncherV2PreflightError("the preregistered launcher entry delta is negative")
    parent = posixpath.dirname(launcher_path)
    for _ in range(parent_count):
        entry = result.get(parent)
        descendants = any(path.startswith(parent + "/") for path in result)
        if entry is None or entry.get("kind") != "directory" or descendants:
            raise LauncherV2PreflightError(
                f"the preregistered launcher-only parent is not empty: {parent}"
            )
        result.pop(parent)
        parent = posixpath.dirname(parent)
    return result


def require_launcher_on_disk(
    path: pathlib.Path, expected_raw: bytes, expected_metadata: Mapping[str, Any]
) -> dict[str, Any]:
    """Independently re-read launcher bytes and Unix metadata from staging."""

    try:
        raw = path.read_bytes()
        info = path.lstat()
    except OSError as exc:
        raise LauncherV2PreflightError("launcher-v2 is absent from staging") from exc
    if raw != expected_raw:
        raise LauncherV2PreflightError("launcher-v2 bytes changed in the written tree")
    observed = {
        "gid": info.st_gid,
        "kind": "file" if stat.S_ISREG(info.st_mode) else "other",
        "mode": stat.S_IMODE(info.st_mode),
        "path": expected_metadata["path"],
        "uid": info.st_uid,
    }
    if observed != expected_metadata:
        raise LauncherV2PreflightError(
            f"launcher-v2 on-disk metadata differs: {observed!r}"
        )
    return observed


def _assert_projection(
    baseline: Mapping[str, Mapping[str, Any]],
    successor: Mapping[str, Mapping[str, Any]],
) -> tuple[dict, dict]:
    projection = expected_projection()
    baseline_totals = measurement.builder_totals(dict(baseline))
    if baseline_totals != projection["withoutLauncher"]:
        raise LauncherV2PreflightError(
            "the launcher-free staging tree differs from the preregistered projection"
        )

    common = set(baseline) & set(successor)
    changed_common = [path for path in common if baseline[path] != successor[path]]
    removed = sorted(set(baseline) - set(successor))
    added = sorted(set(successor) - set(baseline))
    launcher_path = projection["launcherGuestPath"].lstrip("/")
    if changed_common or removed or launcher_path not in added:
        raise LauncherV2PreflightError(
            "launcher-v2 changed something other than adding its sealed entry and parents"
        )
    for path in added:
        entry = successor[path]
        if path == launcher_path:
            if _entry_without_raw(entry) != projection["launcherMetadata"]:
                raise LauncherV2PreflightError("launcher-v2 metadata differs")
        elif entry.get("kind") != "directory" or not launcher_path.startswith(path + "/"):
            raise LauncherV2PreflightError(f"unexpected launcher projection entry: {path}")

    totals = measurement.builder_totals(dict(successor))
    projected = {
        "entries": totals["entries"],
        "largestFileBytes": totals["largestFileBytes"],
        "payloadBytes": totals["payloadBytes"],
    }
    require_expected_totals(projected)
    return baseline_totals, totals


def assert_no_image_path() -> None:
    """Audit the executable import and call graph before assembling anything."""

    source = pathlib.Path(__file__).read_text(encoding="utf-8")
    tree = ast.parse(source)
    forbidden_import_fragments = (
        "native_shadow_boot_kernel_extract",
        "native_shadow_boot_initrd",
        "native_shadow_boot_root_disk",
        "native_shadow_boot_image_produce",
        "native_shadow_successor_produce",
        "subprocess",
    )
    imported: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            imported.extend(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom):
            imported.append(node.module or "")
            imported.extend(alias.name for alias in node.names)
    joined = "\n".join(imported)
    for fragment in forbidden_import_fragments:
        if fragment in joined:
            raise LauncherV2PreflightError(
                f"preflight import graph reaches forbidden fragment: {fragment}"
            )
    if ALLOWED_IMAGE_TOOLS:
        raise LauncherV2PreflightError("preflight unexpectedly permits image tools")
    forbidden_calls = {"eval", "exec", "compile", "__import__", "system", "popen"}
    forbidden_prefixes = ("exec", "spawn", "posix_spawn", "fork")
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        function = node.func
        name = None
        if isinstance(function, ast.Name):
            name = function.id
        elif isinstance(function, ast.Attribute):
            name = function.attr
        if name in forbidden_calls or (
            isinstance(name, str) and name.startswith(forbidden_prefixes)
        ):
            raise LauncherV2PreflightError(
                f"preflight source reaches a process or dynamic-code gateway: {name}"
            )


def write_result_once(path: pathlib.Path, document: Mapping[str, Any]) -> str:
    """Create canonical result bytes once; existing names and symlinks fail closed."""

    path = pathlib.Path(path)
    if not path.is_absolute():
        raise LauncherV2PreflightError("result path must be absolute")
    if not path.parent.is_dir() or path.parent.is_symlink():
        raise LauncherV2PreflightError("result parent must be an existing directory")
    raw = canonical_json(document)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    try:
        descriptor = os.open(path, flags, 0o444)
    except OSError as exc:
        if exc.errno == errno.EEXIST:
            raise LauncherV2PreflightError(
                f"a result name already exists and is not replaced: {path}"
            ) from exc
        raise LauncherV2PreflightError(f"cannot create result: {path}") from exc
    try:
        with os.fdopen(descriptor, "wb", closefd=True) as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    except BaseException:
        try:
            path.unlink()
        except OSError:
            pass
        raise
    return _sha256(raw)


def preflight(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    staging: pathlib.Path,
    launcher_path: pathlib.Path,
    result_path: pathlib.Path,
) -> dict:
    """Assemble, write, re-read and record the exact launcher-v2 staging tree."""

    assert_no_image_path()
    preregistration = load_preregistration()
    repository_root = pathlib.Path(repository_root).resolve()
    bound_inputs = verify_bound_inputs(preregistration, repository_root)
    artifact_store = pathlib.Path(artifact_store).resolve()
    staging = pathlib.Path(staging)
    launcher_path = pathlib.Path(launcher_path)
    result_path = pathlib.Path(result_path)
    if staging.exists() or staging.is_symlink():
        raise LauncherV2PreflightError(f"staging path already exists: {staging}")
    if result_path.exists() or result_path.is_symlink():
        raise LauncherV2PreflightError(f"result name already exists: {result_path}")
    if not artifact_store.is_dir() or artifact_store.is_symlink():
        raise LauncherV2PreflightError("artifact store is absent or is a symlink")
    if not launcher_path.is_file() or launcher_path.is_symlink():
        raise LauncherV2PreflightError("launcher-v2 input is absent or is a symlink")
    launcher_binary = launcher_path.read_bytes()

    staging.parent.mkdir(parents=True, exist_ok=True)
    measurement.assert_case_sensitive(staging.parent)
    temporary = staging.parent / "tmp"
    temporary.mkdir()
    tempfile.tempdir = str(temporary)
    try:
        gpgv = measurement.assert_replay_tool(pathlib.Path(gpgv))
        zstd = measurement.assert_replay_tool(pathlib.Path(zstd))
    except measurement.StagingMeasurementError as exc:
        raise LauncherV2PreflightError(str(exc)) from exc
    lock, validated = _source_closure(
        repository_root, artifact_store, gpgv, zstd
    )
    nested = builder_v4.nested_runtime_tree(
        repository_root, artifact_store, gpgv, zstd
    )
    successor = assemble(
        validated=validated,
        repository_root=repository_root,
        artifact_store=artifact_store,
        launcher_binary=launcher_binary,
        nested_tree=nested,
    )
    baseline = _baseline_without_launcher(successor)
    baseline_totals, computed = _assert_projection(baseline, successor)

    recipe = lock["buildRecipe"]
    measurement.write_staging_tree(successor, staging, recipe["canonicalMtime"])
    walked = measurement.traverse_staging_tree(staging)
    try:
        measurement.assert_measurements_agree(computed, walked)
        measurement.assert_within_limits(recipe, walked)
    except measurement.StagingMeasurementError as exc:
        raise LauncherV2PreflightError(str(exc)) from exc
    launcher_relative = builder_v4.LAUNCHER_GUEST_PATH.lstrip("/")
    launcher_on_disk = staging / launcher_relative
    expected_metadata = expected_projection()["launcherMetadata"]
    require_launcher_on_disk(launcher_on_disk, launcher_binary, expected_metadata)

    document = build_result_document(
        preregistration=preregistration,
        computed=computed,
        walked=walked,
        launcher_binary=launcher_binary,
        baseline_totals=baseline_totals,
        nested_manifest=measurement.nested_manifest_on_disk(staging),
        bound_inputs=bound_inputs,
        gpgv=gpgv,
        zstd=zstd,
        repository_root=repository_root,
    )
    write_result_once(result_path, document)
    return document


def build_result_document(
    *,
    preregistration: Mapping[str, Any],
    computed: Mapping[str, Any],
    walked: Mapping[str, Any],
    launcher_binary: bytes,
    baseline_totals: Mapping[str, Any],
    nested_manifest: Mapping[str, Any],
    bound_inputs: list[dict[str, Any]],
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    repository_root: pathlib.Path,
) -> dict:
    """Build the path- and clock-free result that repeat runs must reproduce."""

    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "authorisations": dict(preregistration["authorisations"]),
        "bootableClaim": BOOTABLE_CLAIM,
        "boundInputs": [dict(row) for row in bound_inputs],
        "builderInternal": dict(computed),
        "imageProduced": IMAGE_PRODUCED_CLAIM,
        "independentTraversal": dict(walked),
        "launcher": {
            "guestLogicalPath": builder_v4.LAUNCHER_GUEST_PATH,
            "sha256": _sha256(launcher_binary),
            "sizeBytes": len(launcher_binary),
        },
        "limits": dict(expected_projection()["limits"]),
        "nestedContentManifest": dict(nested_manifest),
        "preregistrationSha256": PREREGISTRATION_SHA256,
        "projection": {
            "baseline": dict(baseline_totals),
            "withLauncherV2": expected_projection()["withLauncherV2"],
        },
        "provenance": {
            "repositoryFiles": [
                _repository_file_identity(repository_root, path)
                for path in (
                    pathlib.Path(__file__).resolve().relative_to(REPOSITORY_ROOT),
                    PREFLIGHT_WRAPPER_PATH,
                    pathlib.Path(builder_v4.__file__).resolve().relative_to(REPOSITORY_ROOT),
                    MEASUREMENT_PATH,
                    BASE_PROJECTION_PATH,
                    pathlib.Path(release_gate_v2.__file__).resolve().relative_to(
                        REPOSITORY_ROOT
                    ),
                    ISOLATION_ARGV_PATH,
                    ISOLATION_AUTHORITY_PATH,
                )
            ],
            "sourceLockSha256": SOURCE_LOCK_SHA256,
            "tools": [
                _tool_identity("gpgv", gpgv),
                _tool_identity("zstd", zstd),
            ],
        },
        "repeatable": True,
        "schema": SCHEMA,
        "status": "PASS-NO-IMAGE-PRODUCED",
    }


def _repository_file_identity(
    repository_root: pathlib.Path, relative: pathlib.Path
) -> dict[str, Any]:
    path = pathlib.Path(repository_root) / relative
    raw = path.read_bytes()
    return {
        "path": relative.as_posix(),
        "sha256": _sha256(raw),
        "sizeBytes": len(raw),
    }


def _tool_identity(role: str, path: pathlib.Path) -> dict[str, Any]:
    resolved = pathlib.Path(path).resolve()
    if not resolved.is_file() or resolved.is_symlink():
        raise LauncherV2PreflightError(f"{role} is not a regular resolved tool")
    raw = resolved.read_bytes()
    return {
        "basename": resolved.name,
        "role": role,
        "sha256": _sha256(raw),
        "sizeBytes": len(raw),
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo-root", type=pathlib.Path, default=REPOSITORY_ROOT)
    parser.add_argument("--cas", type=pathlib.Path, required=True)
    parser.add_argument("--gpgv", type=pathlib.Path, required=True)
    parser.add_argument("--zstd", type=pathlib.Path, required=True)
    parser.add_argument("--staging", type=pathlib.Path, required=True)
    parser.add_argument("--launcher", dest="launcher_path", type=pathlib.Path, required=True)
    parser.add_argument("--result", dest="result_path", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    arguments = _parser().parse_args(argv)
    document = preflight(
        repository_root=arguments.repo_root,
        artifact_store=arguments.cas,
        gpgv=arguments.gpgv,
        zstd=arguments.zstd,
        staging=arguments.staging,
        launcher_path=arguments.launcher_path,
        result_path=arguments.result_path,
    )
    print(
        "native-shadow-launcher-v2-image-preflight-arm64: PASS: "
        f"{arguments.result_path} sha256={_sha256(canonical_json(document))}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except LauncherV2PreflightError as exc:
        print(
            f"native-shadow-launcher-v2-image-preflight-arm64: FAIL: {exc}",
            file=sys.stderr,
        )
        raise SystemExit(2)
