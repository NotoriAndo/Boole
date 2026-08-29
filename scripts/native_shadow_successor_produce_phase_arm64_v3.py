#!/usr/bin/env python3
"""Authority-zero core for the launcher-v2 successor image producer.

This generation deliberately stops before image production.  It freezes one
staging orchestration that a repeatable, no-image rehearsal and a future
authority-bound production entry will share.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import stat
import sys
import tempfile
from dataclasses import dataclass
from typing import Any, Callable, Mapping, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_staging_measure_arm64_v1 as staging_measure
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as base_projection
from scripts import native_shadow_rootfs_builder_boot_arm64_v4 as builder_v4
from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as release_gate


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
PREREGISTRATION_RELATIVE = pathlib.Path(
    "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
PREREGISTRATION_SHA256 = (
    "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec"
)
PREREGISTRATION_SIZE_BYTES = 20145
PREREGISTRATION_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-producer-"
    "preregistration.arm64.v1"
)
PREREGISTRATION_STATUS = "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY"
IMPORT_CORRECTION_RELATIVE = pathlib.Path(
    "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json"
)
IMPORT_CORRECTION_SHA256 = (
    "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498"
)
IMPORT_CORRECTION_SIZE_BYTES = 10971
IMPORT_CORRECTION_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-producer-"
    "import-closure-correction.arm64.v1"
)
IMPORT_CORRECTION_STATUS = (
    "CORRECTED-BEFORE-REHEARSAL-NO-IMAGE-PRODUCTION-AUTHORITY"
)
REHEARSAL_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal.arm64.v1"
)
FORBIDDEN_OUTPUT_NAMES = (
    "ATTEMPT-CONSUMED.json",
    "guest-kernel",
    "guest-initrd",
    "guest-root-disk",
)
SOURCE_LOCK_RELATIVE = pathlib.Path(
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)
SOURCE_LOCK_SHA256 = (
    "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"
)
LAUNCHER_SHA256 = "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"
LAUNCHER_SIZE_BYTES = 2_025_192

EXPECTED_AUTHORISATIONS = {
    "bootAuthorised": False,
    "consensusActivated": False,
    "imageProductionAuthorised": False,
    "imageProductionRunsAllowed": 0,
    "mac4Started": False,
    "miningActivated": False,
    "p2pActivated": False,
    "rewardActivated": False,
    "testnetStarted": False,
}
EXPECTED_RUNS = {
    "bootsAllowed": 0,
    "bootsPerformed": 0,
    "freeRehearsalsPerformedByThisRecord": 0,
    "imageProductionsAllowed": 0,
    "imageProductionsPerformed": 0,
}


class SuccessorProducerV3Error(RuntimeError):
    """The generation cannot preserve its pre-registered boundary."""


@dataclass(frozen=True)
class ProductionEffects:
    """Irreversible effects that remain unreachable without a future chain."""

    create_output_directory: Callable[..., Any]
    write_attempt_marker: Callable[..., Any]
    produce_images: Callable[..., Any]


@dataclass(frozen=True)
class PreparedStaging:
    """The one assembled mapping and the exact measurement established for it."""

    entries: Mapping[str, Mapping[str, Any]]
    measurement: dict[str, Any]


FUTURE_CHAIN_RELATIVES = (
    pathlib.Path(
        "native/containment/"
        "native-shadow-mac3-successor-producer-fingerprint-arm64-v5.json"
    ),
    pathlib.Path(
        "native/containment/"
        "native-shadow-mac3-launcher-v2-successor-producer-"
        "rehearsal-result-arm64-v1.json"
    ),
    pathlib.Path(
        "native/containment/"
        "native-shadow-mac3-successor-production-authority-arm64-v5.json"
    ),
)


def canonical_json(document: Any) -> bytes:
    return (json.dumps(document, sort_keys=True, indent=2) + "\n").encode("utf-8")


def _strict_equal(observed: Any, expected: Any) -> bool:
    if type(observed) is not type(expected):
        return False
    if isinstance(expected, dict):
        return set(observed) == set(expected) and all(
            _strict_equal(observed[key], value) for key, value in expected.items()
        )
    if isinstance(expected, list):
        return len(observed) == len(expected) and all(
            _strict_equal(left, right) for left, right in zip(observed, expected)
        )
    return observed == expected


def load_preregistration(
    repository_root: Optional[pathlib.Path] = None,
) -> dict[str, Any]:
    """Read the exact canonical authority-zero record by its fixed repo path."""

    root = REPOSITORY_ROOT if repository_root is None else pathlib.Path(repository_root)
    path = root / PREREGISTRATION_RELATIVE
    try:
        info = path.lstat()
        raw = path.read_bytes()
    except OSError as exc:
        raise SuccessorProducerV3Error("producer preregistration is unreadable") from exc
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise SuccessorProducerV3Error(
            "producer preregistration is not a regular non-symlink file"
        )
    if len(raw) != PREREGISTRATION_SIZE_BYTES:
        raise SuccessorProducerV3Error(
            "producer preregistration size differs from the frozen identity"
        )
    digest = hashlib.sha256(raw).hexdigest()
    if digest != PREREGISTRATION_SHA256:
        raise SuccessorProducerV3Error(
            "producer preregistration digest differs from the frozen identity"
        )
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SuccessorProducerV3Error(
            "producer preregistration is not JSON"
        ) from exc
    if not isinstance(document, dict) or raw != canonical_json(document):
        raise SuccessorProducerV3Error(
            "producer preregistration is not one canonical JSON object"
        )
    if document.get("schema") != PREREGISTRATION_SCHEMA:
        raise SuccessorProducerV3Error("producer preregistration schema differs")
    if document.get("status") != PREREGISTRATION_STATUS:
        raise SuccessorProducerV3Error("producer preregistration status differs")
    if not _strict_equal(document.get("authorisations"), EXPECTED_AUTHORISATIONS):
        raise SuccessorProducerV3Error(
            "producer preregistration grants or changes an authority"
        )
    if not _strict_equal(document.get("runs"), EXPECTED_RUNS):
        raise SuccessorProducerV3Error(
            "producer preregistration run accounting is not all-zero"
        )
    return document


def load_import_closure_correction(
    preregistration: Mapping[str, Any],
    repository_root: Optional[pathlib.Path] = None,
) -> dict[str, Any]:
    """Read the append-only correction and bind it to its exact predecessor."""

    root = REPOSITORY_ROOT if repository_root is None else pathlib.Path(repository_root)
    predecessor_raw = canonical_json(preregistration)
    if (
        len(predecessor_raw) != PREREGISTRATION_SIZE_BYTES
        or hashlib.sha256(predecessor_raw).hexdigest() != PREREGISTRATION_SHA256
    ):
        raise SuccessorProducerV3Error(
            "producer import-closure correction predecessor bytes differ"
        )
    path = root / IMPORT_CORRECTION_RELATIVE
    try:
        info = path.lstat()
        raw = path.read_bytes()
    except OSError as exc:
        raise SuccessorProducerV3Error(
            "producer import-closure correction is unreadable"
        ) from exc
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise SuccessorProducerV3Error(
            "producer import-closure correction is not a regular non-symlink file"
        )
    if len(raw) != IMPORT_CORRECTION_SIZE_BYTES:
        raise SuccessorProducerV3Error(
            "producer import-closure correction size differs"
        )
    if hashlib.sha256(raw).hexdigest() != IMPORT_CORRECTION_SHA256:
        raise SuccessorProducerV3Error(
            "producer import-closure correction digest differs"
        )
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SuccessorProducerV3Error(
            "producer import-closure correction is not JSON"
        ) from exc
    if not isinstance(document, dict) or raw != canonical_json(document):
        raise SuccessorProducerV3Error(
            "producer import-closure correction is not one canonical JSON object"
        )
    if document.get("schema") != IMPORT_CORRECTION_SCHEMA:
        raise SuccessorProducerV3Error("producer import-closure correction schema differs")
    if document.get("status") != IMPORT_CORRECTION_STATUS:
        raise SuccessorProducerV3Error("producer import-closure correction status differs")
    expected_predecessor = {
        "bindingCount": 23,
        "path": PREREGISTRATION_RELATIVE.as_posix(),
        "preservedByteUnchanged": True,
        "sha256": PREREGISTRATION_SHA256,
        "sizeBytes": PREREGISTRATION_SIZE_BYTES,
    }
    if not _strict_equal(document.get("predecessor"), expected_predecessor):
        raise SuccessorProducerV3Error(
            "producer import-closure correction predecessor differs"
        )
    if not _strict_equal(document.get("authorisations"), EXPECTED_AUTHORISATIONS):
        raise SuccessorProducerV3Error(
            "producer import-closure correction grants or changes an authority"
        )
    if not _strict_equal(document.get("authorisations"), preregistration.get("authorisations")):
        raise SuccessorProducerV3Error(
            "producer import-closure correction authority differs from predecessor"
        )
    if not _strict_equal(document.get("runs"), EXPECTED_RUNS):
        raise SuccessorProducerV3Error(
            "producer import-closure correction run accounting is not all-zero"
        )
    if not _strict_equal(document.get("runs"), preregistration.get("runs")):
        raise SuccessorProducerV3Error(
            "producer import-closure correction runs differ from predecessor"
        )
    expected_effective = {
        "addedMissingBindings": 18,
        "bindingVerificationBeforeRepositoryPythonImport": True,
        "effectiveUniqueBindings": 41,
        "predecessorBindings": 23,
        "unionRequired": True,
    }
    if not _strict_equal(document.get("effectiveBinding"), expected_effective):
        raise SuccessorProducerV3Error(
            "producer import-closure correction effective binding differs"
        )
    if document.get("grantsAuthority") is not False:
        raise SuccessorProducerV3Error(
            "producer import-closure correction grants authority"
        )
    if document.get("futureFingerprintMustBindBothRecords") is not True:
        raise SuccessorProducerV3Error(
            "producer import-closure correction lost two-record binding"
        )
    return document


def verify_bound_inputs(
    preregistration: Mapping[str, Any],
    correction: Mapping[str, Any],
    repository_root: Optional[pathlib.Path] = None,
) -> list[dict[str, Any]]:
    """Re-read the exact 23+18 direct union before any generation effect."""

    root = (
        REPOSITORY_ROOT
        if repository_root is None
        else pathlib.Path(repository_root).resolve()
    )
    predecessor_rows = preregistration.get("bindings")
    added_rows = correction.get("addedBindings")
    if not isinstance(predecessor_rows, list) or len(predecessor_rows) != 23:
        raise SuccessorProducerV3Error(
            "producer preregistration does not bind exactly twenty-three inputs"
        )
    if not isinstance(added_rows, list) or len(added_rows) != 18:
        raise SuccessorProducerV3Error(
            "producer import-closure correction does not add exactly eighteen inputs"
        )
    rows = predecessor_rows + added_rows
    observed: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in rows:
        if not isinstance(row, dict) or set(row) != {
            "path",
            "role",
            "sha256",
            "sizeBytes",
        }:
            raise SuccessorProducerV3Error("a producer binding has an invalid shape")
        if not isinstance(row.get("role"), str) or not row["role"]:
            raise SuccessorProducerV3Error("a producer binding has no role")
        relative = row.get("path")
        if not isinstance(relative, str) or not relative:
            raise SuccessorProducerV3Error("a producer binding has no path")
        pure = pathlib.PurePosixPath(relative)
        if pure.is_absolute() or ".." in pure.parts or pure.as_posix() != relative:
            raise SuccessorProducerV3Error(
                f"producer binding path is unsafe: {relative!r}"
            )
        if relative in seen:
            raise SuccessorProducerV3Error(
                f"producer binding is repeated: {relative}"
            )
        seen.add(relative)
        path = root / pathlib.Path(*pure.parts)
        try:
            info = path.lstat()
            raw = path.read_bytes()
        except OSError as exc:
            raise SuccessorProducerV3Error(
                f"producer binding is unreadable: {relative}"
            ) from exc
        if not stat.S_ISREG(info.st_mode) or path.is_symlink():
            raise SuccessorProducerV3Error(
                f"producer binding is not a regular file: {relative}"
            )
        try:
            resolved = path.resolve()
            contained = resolved.is_relative_to(root)
        except AttributeError:  # pragma: no cover - Python before 3.9 fallback
            contained = root == resolved or root in resolved.parents
        if not contained:
            raise SuccessorProducerV3Error(
                f"producer binding leaves the repository: {relative}"
            )
        identity = {
            "path": relative,
            "sha256": hashlib.sha256(raw).hexdigest(),
            "sizeBytes": len(raw),
        }
        expected = {
            "path": relative,
            "sha256": row.get("sha256"),
            "sizeBytes": row.get("sizeBytes"),
        }
        if not _strict_equal(identity, expected):
            raise SuccessorProducerV3Error(
                f"producer binding differs from preregistration: {relative}"
            )
        observed.append(identity)
    if len(seen) != 41:
        raise SuccessorProducerV3Error(
            "producer effective binding union is not exactly forty-one inputs"
        )
    return sorted(observed, key=lambda row: row["path"].encode("utf-8"))


def prepare_staging(
    *,
    validated: dict[str, Any],
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Mapping[str, Any]],
    preregistration: Optional[Mapping[str, Any]] = None,
) -> PreparedStaging:
    """Assemble once and require the pre-registered S2 staging measurement.

    Rehearsal and future production call this same function.  Effects are
    The assembler and measurer are the exact bound module objects.  Effects may
    be injected at the future image boundary, but the facts being measured may
    not be supplied by a caller.
    """

    record = load_preregistration() if preregistration is None else preregistration
    expected = record.get("expectedPreflight", {}).get("measurement")
    if not isinstance(expected, dict):
        raise SuccessorProducerV3Error(
            "producer preregistration has no expected staging measurement"
        )
    entries = builder_v4.materialize_staging_tree(
        validated,
        pathlib.Path(repository_root),
        pathlib.Path(artifact_store),
        launcher_binary=launcher_binary,
        nested_tree=nested_tree,
    )
    if not isinstance(entries, Mapping):
        raise SuccessorProducerV3Error("staging assembler did not return a mapping")
    measured = staging_measure.builder_totals(entries)
    if not isinstance(measured, Mapping) or not _strict_equal(dict(measured), expected):
        raise SuccessorProducerV3Error(
            "assembled staging measurement differs from the S2 preflight contract"
        )
    return PreparedStaging(entries=entries, measurement=dict(measured))


def _scratch_snapshot(root: pathlib.Path) -> list[dict[str, Any]]:
    """Describe every extant scratch entry without following symlinks."""

    root = pathlib.Path(root)
    try:
        root_info = root.lstat()
    except OSError as exc:
        raise SuccessorProducerV3Error("rehearsal scratch root is unreadable") from exc
    if not stat.S_ISDIR(root_info.st_mode) or root.is_symlink():
        raise SuccessorProducerV3Error(
            "rehearsal scratch root is not a regular directory"
        )
    rows: list[dict[str, Any]] = []
    pending = [root]
    while pending:
        directory = pending.pop()
        try:
            with os.scandir(directory) as iterator:
                children = sorted(iterator, key=lambda item: os.fsencode(item.name))
        except OSError as exc:
            raise SuccessorProducerV3Error(
                "rehearsal scratch tree cannot be traversed"
            ) from exc
        child_directories: list[pathlib.Path] = []
        for child in children:
            path = pathlib.Path(child.path)
            try:
                info = path.lstat()
            except OSError as exc:
                raise SuccessorProducerV3Error(
                    "rehearsal scratch entry cannot be read"
                ) from exc
            relative = path.relative_to(root).as_posix()
            row: dict[str, Any] = {
                "gid": info.st_gid,
                "mode": stat.S_IMODE(info.st_mode),
                "path": relative,
                "uid": info.st_uid,
            }
            if stat.S_ISDIR(info.st_mode):
                row["kind"] = "directory"
                child_directories.append(path)
            elif stat.S_ISREG(info.st_mode):
                digest = hashlib.sha256()
                try:
                    with path.open("rb") as handle:
                        for block in iter(lambda: handle.read(1 << 20), b""):
                            digest.update(block)
                except OSError as exc:
                    raise SuccessorProducerV3Error(
                        "rehearsal scratch file cannot be read"
                    ) from exc
                row.update(
                    {
                        "kind": "file",
                        "sha256": digest.hexdigest(),
                        "sizeBytes": info.st_size,
                    }
                )
            elif stat.S_ISLNK(info.st_mode):
                row.update({"kind": "symlink", "target": os.readlink(path)})
            else:
                row["kind"] = "other"
            rows.append(row)
        pending.extend(reversed(child_directories))
    return sorted(rows, key=lambda row: os.fsencode(row["path"]))


def run_free_rehearsal(
    *,
    validated: dict[str, Any],
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Mapping[str, Any]],
    scratch_root: pathlib.Path,
) -> bytes:
    """Run the shared assembly path and return its sole canonical JSON artifact.

    No output-directory, marker, image, boot, network, or process effect is
    accepted by this interface.  The caller receives bytes and decides where a
    CI artifact member is stored.
    """

    preregistration = load_preregistration()
    correction = load_import_closure_correction(preregistration)
    bound_inputs = verify_bound_inputs(preregistration, correction)
    before = _scratch_snapshot(scratch_root)
    if before:
        raise SuccessorProducerV3Error(
            "free rehearsal scratch root must start fresh and empty"
        )
    try:
        prepared = prepare_staging(
            validated=validated,
            repository_root=repository_root,
            artifact_store=artifact_store,
            launcher_binary=launcher_binary,
            nested_tree=nested_tree,
            preregistration=preregistration,
        )
    finally:
        after = _scratch_snapshot(scratch_root)
        if not _strict_equal(before, after):
            raise SuccessorProducerV3Error(
                "free rehearsal scratch tree changed; an output effect escaped"
            )
    scratch_digest = hashlib.sha256(canonical_json(before)).hexdigest()
    return canonical_json(
        {
            "activationAllowed": False,
            "authorisations": dict(preregistration["authorisations"]),
            "bootableClaim": False,
            "boundInputs": bound_inputs,
            "effects": {
                "allowedArtifact": "one canonical JSON result only",
                "allowedImageTools": [],
                "artifactMemberCount": 1,
                "attemptMarkersCreated": 0,
                "forbiddenOutputNames": list(FORBIDDEN_OUTPUT_NAMES),
                "imageEffectCalls": 0,
                "imageFilesCreated": 0,
                "productionOutputDirectoriesCreated": 0,
                "productionOutputsCreated": 0,
                "scratchSnapshotSha256": scratch_digest,
                "scratchTreeUnchanged": True,
            },
            "imageProduced": False,
            "importClosureCorrectionSha256": IMPORT_CORRECTION_SHA256,
            "measurement": prepared.measurement,
            "preregistrationSha256": PREREGISTRATION_SHA256,
            "repeatable": True,
            "schema": REHEARSAL_SCHEMA,
            "status": "PASS-NO-IMAGE-PRODUCED",
        }
    )


def _real_directory(path: pathlib.Path, context: str) -> pathlib.Path:
    path = pathlib.Path(path)
    try:
        info = path.lstat()
    except OSError as exc:
        raise SuccessorProducerV3Error(f"{context} is unreadable: {path}") from exc
    if not stat.S_ISDIR(info.st_mode) or path.is_symlink():
        raise SuccessorProducerV3Error(
            f"{context} is not a real non-symlink directory: {path}"
        )
    return path


def read_launcher_v2(path: pathlib.Path) -> bytes:
    """Read the rebuilt launcher only when it is the sealed launcher-v2 ELF."""

    path = pathlib.Path(path)
    try:
        info = path.lstat()
        raw = path.read_bytes()
    except OSError as exc:
        raise SuccessorProducerV3Error("launcher-v2 binary is unreadable") from exc
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise SuccessorProducerV3Error(
            "launcher-v2 binary is not a regular non-symlink file"
        )
    if len(raw) != LAUNCHER_SIZE_BYTES:
        raise SuccessorProducerV3Error(
            f"launcher-v2 size is {len(raw)}, not the sealed {LAUNCHER_SIZE_BYTES}"
        )
    digest = hashlib.sha256(raw).hexdigest()
    if digest != LAUNCHER_SHA256:
        raise SuccessorProducerV3Error(
            f"launcher-v2 digest is {digest}, not the sealed {LAUNCHER_SHA256}"
        )
    return raw


def prepare_real_inputs(
    *,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    """Derive the one source-lock-v2 input set consumed by builder v4."""

    store = _real_directory(artifact_store, "artifact store")
    try:
        gpgv = staging_measure.assert_replay_tool(pathlib.Path(gpgv))
        zstd = staging_measure.assert_replay_tool(pathlib.Path(zstd))
    except staging_measure.StagingMeasurementError as exc:
        raise SuccessorProducerV3Error(str(exc)) from exc

    source = REPOSITORY_ROOT / SOURCE_LOCK_RELATIVE
    try:
        source_info = source.lstat()
        raw = source.read_bytes()
    except OSError as exc:
        raise SuccessorProducerV3Error("source-lock v2 is unreadable") from exc
    if not stat.S_ISREG(source_info.st_mode) or source.is_symlink():
        raise SuccessorProducerV3Error(
            "source-lock v2 is not a regular non-symlink file"
        )
    if hashlib.sha256(raw).hexdigest() != SOURCE_LOCK_SHA256:
        raise SuccessorProducerV3Error("source-lock v2 digest differs")
    try:
        sealed = builder_v4.load_json_exact(
            raw, "source-lock v2", require_canonical=True
        )
        runtime, _receipt = release_gate.materialize_runtime_lock(
            sealed, raw, gpgv, zstd
        )
        normalized, normalized_raw, _record = base_projection.normalized_runtime_lock(
            runtime
        )
        validated = builder_v4.validate_source_lock(
            normalized,
            normalized_raw,
            REPOSITORY_ROOT,
            store,
            require_complete=True,
        )
        nested = builder_v4.nested_runtime_tree(REPOSITORY_ROOT, store, gpgv, zstd)
    except Exception as exc:
        raise SuccessorProducerV3Error(
            f"source-lock-v2 materialization failed: {exc}"
        ) from exc
    return validated, nested


def run_bound_rehearsal(
    *,
    artifact_store: pathlib.Path,
    launcher: pathlib.Path,
    scratch_root: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> bytes:
    """Run acquisition-free assembly while accounting for every scratch effect."""

    preregistration = load_preregistration()
    correction = load_import_closure_correction(preregistration)
    verify_bound_inputs(preregistration, correction)
    scratch = _real_directory(scratch_root, "rehearsal scratch root")
    before = _scratch_snapshot(scratch)
    if before:
        raise SuccessorProducerV3Error(
            "free rehearsal scratch root must start fresh and empty"
        )
    previous_tempdir = tempfile.tempdir
    try:
        tempfile.tempdir = str(scratch)
        validated, nested_tree = prepare_real_inputs(
            artifact_store=artifact_store, gpgv=gpgv, zstd=zstd
        )
        launcher_binary = read_launcher_v2(launcher)
        result = run_free_rehearsal(
            validated=validated,
            repository_root=REPOSITORY_ROOT,
            artifact_store=pathlib.Path(artifact_store),
            launcher_binary=launcher_binary,
            nested_tree=nested_tree,
            scratch_root=scratch,
        )
    finally:
        tempfile.tempdir = previous_tempdir
        after = _scratch_snapshot(scratch)
        if not _strict_equal(before, after):
            raise SuccessorProducerV3Error(
                "free rehearsal scratch tree changed outside the shared assembly"
            )
    return result


def _verify_future_chain_before_any_effect() -> None:
    """Keep absent files and unimplemented chain consumption fail-closed."""

    absent = [
        relative.as_posix()
        for relative in FUTURE_CHAIN_RELATIVES
        if not (REPOSITORY_ROOT / relative).is_file()
        or (REPOSITORY_ROOT / relative).is_symlink()
    ]
    if absent:
        raise SuccessorProducerV3Error(
            "future production chain is absent; authorityless production stops "
            "before assembly, output directory, attempt marker and image effect: "
            + ", ".join(absent)
        )

    # The fixed chain does not exist in this slice.  Keeping this separate stop
    # prevents mere file appearance from accidentally turning absence checking
    # into authority.  A successor must replace this stop with complete
    # one-way digest verification before the shared prepare_staging call below
    # can become reachable.
    raise SuccessorProducerV3Error(
        "future production chain consumption is not implemented; production "
        "stops before assembly, output directory, attempt marker and image effect"
    )


def production_entry(
    *,
    validated: dict[str, Any],
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Mapping[str, Any]],
    production_effects: ProductionEffects,
) -> None:
    """Fail before assembly or effects until the one-way future chain exists.

    The paths are fixed constants.  Neither CLI options nor environment values
    can substitute an authority, fingerprint, preregistration, or assembler.
    """

    preregistration = load_preregistration()
    correction = load_import_closure_correction(preregistration)
    verify_bound_inputs(preregistration, correction)
    _verify_future_chain_before_any_effect()

    # The verifier above is fail-closed in S3-B.  Its successor must make this
    # continuation reachable only after complete one-way digest verification.
    # Rehearsal and production share this public staging orchestration; no
    # second production assembler exists.
    prepared = prepare_staging(  # pragma: no cover - authority is absent in S3-B
        validated=validated,
        repository_root=repository_root,
        artifact_store=artifact_store,
        launcher_binary=launcher_binary,
        nested_tree=nested_tree,
        preregistration=preregistration,
    )
    output = production_effects.create_output_directory()
    production_effects.write_attempt_marker(output)
    production_effects.produce_images(output, prepared)


def production_check() -> None:
    """Refuse an authorityless generation without touching any host build input."""

    preregistration = load_preregistration()
    correction = load_import_closure_correction(preregistration)
    verify_bound_inputs(preregistration, correction)
    _verify_future_chain_before_any_effect()


def _write_result_once(path: pathlib.Path, raw: bytes) -> None:
    path = pathlib.Path(path)
    if not path.is_absolute():
        raise SuccessorProducerV3Error("rehearsal result path must be absolute")
    parent = _real_directory(path.parent, "rehearsal result parent")
    if path.exists() or path.is_symlink():
        raise SuccessorProducerV3Error(
            "rehearsal result already exists and is not overwritten"
        )
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(str(parent / path.name), flags, 0o444)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
    except OSError as exc:
        raise SuccessorProducerV3Error("rehearsal result cannot be written once") from exc


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    subparsers = parser.add_subparsers(dest="mode", required=True)
    subparsers.add_parser(
        "production-check",
        help="refuse before host inputs while the future one-use chain is absent",
    )
    rehearsal = subparsers.add_parser(
        "rehearsal", help="assemble and measure without any image effect"
    )
    rehearsal.add_argument("--cas", type=pathlib.Path, required=True)
    rehearsal.add_argument("--launcher", type=pathlib.Path, required=True)
    rehearsal.add_argument("--scratch", type=pathlib.Path, required=True)
    rehearsal.add_argument("--gpgv", type=pathlib.Path, required=True)
    rehearsal.add_argument("--zstd", type=pathlib.Path, required=True)
    rehearsal.add_argument("--result", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    try:
        if options.mode == "production-check":
            production_check()
            raise SuccessorProducerV3Error(
                "authorityless production-check unexpectedly returned"
            )
        result = pathlib.Path(options.result)
        scratch = pathlib.Path(options.scratch)
        if result.exists() or result.is_symlink():
            raise SuccessorProducerV3Error(
                "rehearsal result already exists and is not overwritten"
            )
        try:
            result.resolve().relative_to(scratch.resolve())
        except ValueError:
            pass
        else:
            raise SuccessorProducerV3Error(
                "rehearsal result must be outside the exhaustively observed scratch"
            )
        raw = run_bound_rehearsal(
            artifact_store=options.cas,
            launcher=options.launcher,
            scratch_root=scratch,
            gpgv=options.gpgv,
            zstd=options.zstd,
        )
        document = json.loads(raw.decode("utf-8"))
        if raw != canonical_json(document):
            raise SuccessorProducerV3Error("rehearsal result is not canonical JSON")
        _write_result_once(result, raw)
    except Exception as exc:
        print(f"native-shadow successor producer v3: FAIL: {exc}", file=sys.stderr)
        return 2
    print(f"native-shadow successor producer v3: PASS: {result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
