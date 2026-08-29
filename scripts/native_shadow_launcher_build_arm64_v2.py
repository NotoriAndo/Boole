#!/usr/bin/env python3
"""Fail-closed primitives for the launcher-v2 overlay build.

The historical launcher source and its v1 authority stay untouched.  A v2
build exports that tree, applies complete replacement files only inside the
temporary export, and compiles with a fresh Cargo home whose real path is
remapped to a fixed logical root.  The resulting ELF is scanned before any
digest can be called a result.

This first slice deliberately contains no image producer and grants no boot.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Callable, Mapping

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_launcher_build_arm64_v1 as v1


CONTAINMENT = REPO_ROOT / "native" / "containment"
TOOL_PATH = pathlib.Path(__file__).resolve()
AUTHORITY_PATH = CONTAINMENT / "native-shadow-launcher-build-authority-arm64-v2.json"
RESULT_PATH = CONTAINMENT / "native-shadow-launcher-build-result-arm64-v2.json"
SOURCE_OVERLAY_PATH = (
    REPO_ROOT
    / "native"
    / "containment"
    / "native-shadow-launcher-source-overlay-arm64-v2.json"
)
SOURCE_OVERLAY_SHA256 = "a138cf374459e6c70c591998cae0c974a0ac58965e91d5cbea230f10df7f3970"

AUTHORITY_SCHEMA = "boole.native-shadow.launcher-build-authority.arm64.v2"
RESULT_SCHEMA = "boole.native-shadow.launcher-build-result.arm64.v2"
RELEASE = "NATIVE-SHADOW-LAUNCHER-BUILD-ARM64-V2"
RESULT_STATUS = "LAUNCHER-V2-ELF-BUILT-BYTE-IDENTICAL-PRODUCER-PATHS-ZERO-NO-IMAGE-NO-BOOT"
GENERATOR_PATH = "scripts/native_shadow_launcher_build_arm64_v2.py"
AUTHORITY_SHA256 = "1fa2430a04e750d2c3cba22bab03d7a30e2a244300c729ddbd904d282958a5da"
SOURCE_OVERLAY_STATUS = "LAUNCHER-V2-SOURCE-OVERLAY-FROZEN-NOT-BUILT-NO-IMAGE-NO-BOOT"
SOURCE_OVERLAY_WHY = (
    "The v1 authority live-pins the historical launcher source used by the "
    "preserved image. Launcher v2 therefore changes only a temporary exported "
    "tree and leaves every v1 source and record byte untouched."
)
SOURCE_PROPERTIES = {
    "dropFailureMatrixIsTableDriven": True,
    "guestRecordsAreObservationsNotVerdicts": True,
    "prerequisiteField": "resolved",
    "submissionExecutionObservedByClosedBoot": False,
    "supervisorSnapshotIncludesAllIdentitySlotsAndFiveCapabilitySets": True,
}


class LauncherV2BuildError(RuntimeError):
    """A v2 overlay, environment or artifact violates the frozen boundary."""


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def launcher_v2_generator_sha256(raw: bytes) -> str:
    marker = b'AUTHORITY_SHA256 = "'
    start = raw.index(marker) + len(marker)
    return sha256_bytes(raw[:start] + b"0" * 64 + raw[start + 64 :])


def load_source_overlay(
    path: pathlib.Path = SOURCE_OVERLAY_PATH,
) -> dict[str, Any]:
    raw = path.read_bytes()
    if sha256_bytes(raw) != SOURCE_OVERLAY_SHA256:
        raise LauncherV2BuildError("launcher-v2 source overlay differs from its pin")
    return validate_source_overlay(json.loads(raw.decode("utf-8")))


def validate_source_overlay(record: Any) -> dict[str, Any]:
    required = {
        "activationAllowed",
        "appendOnly",
        "baseV1Authority",
        "bootAuthorisation",
        "completeFiles",
        "exactReplacements",
        "imageProductionAuthorisation",
        "overlayRoot",
        "record",
        "schema",
        "sharedConsoleFixture",
        "sourceProperties",
        "status",
        "whyOverlay",
    }
    if not isinstance(record, dict) or set(record) != required:
        raise LauncherV2BuildError("source overlay keys differ from the contract")
    if record["record"] != "native-shadow-launcher-source-overlay-arm64-v2":
        raise LauncherV2BuildError("source overlay identity differs")
    if record["schema"] != 1 or record["appendOnly"] is not True:
        raise LauncherV2BuildError("source overlay schema differs")
    if any(
        record[name] is not False
        for name in (
            "activationAllowed",
            "bootAuthorisation",
            "imageProductionAuthorisation",
        )
    ):
        raise LauncherV2BuildError("source overlay cannot grant execution")
    base = record["baseV1Authority"]
    if base != {
        "path": "native/containment/native-shadow-launcher-build-authority-arm64-v1.json",
        "sha256": v1.AUTHORITY_SHA256,
    }:
        raise LauncherV2BuildError("source overlay is not based on the frozen v1 authority")
    if record["overlayRoot"] != "native/launcher-v2-overlay":
        raise LauncherV2BuildError("source overlay root differs")
    if record["status"] != SOURCE_OVERLAY_STATUS:
        raise LauncherV2BuildError("source overlay status differs")
    if record["whyOverlay"] != SOURCE_OVERLAY_WHY:
        raise LauncherV2BuildError("source overlay rationale differs")
    if record["sourceProperties"] != SOURCE_PROPERTIES:
        raise LauncherV2BuildError("source overlay properties differ")
    complete = record["completeFiles"]
    replacements = record["exactReplacements"]
    if not isinstance(complete, list) or not complete:
        raise LauncherV2BuildError("source overlay has no complete files")
    if not isinstance(replacements, list) or not replacements:
        raise LauncherV2BuildError("source overlay has no exact replacements")
    destinations = [row.get("destination") for row in complete + replacements]
    if len(set(destinations)) != len(destinations):
        raise LauncherV2BuildError("source overlay destination is duplicated")
    return record


def load_authority(path: pathlib.Path = AUTHORITY_PATH) -> dict[str, Any]:
    raw = path.read_bytes()
    if v1.canonical_json(json.loads(raw.decode("utf-8"))) != raw:
        raise LauncherV2BuildError("launcher-v2 authority is not canonical JSON")
    if sha256_bytes(raw) != AUTHORITY_SHA256:
        raise LauncherV2BuildError("launcher-v2 authority differs from its pin")
    return validate_authority(json.loads(raw.decode("utf-8")))


def validate_authority(authority: Any) -> dict[str, Any]:
    required = {
        "activationAllowed",
        "baseV1Authority",
        "bootableClaim",
        "boundaries",
        "build",
        "determinism",
        "generator",
        "platform",
        "release",
        "schema",
        "sourceOverlay",
        "toolchain",
    }
    if not isinstance(authority, dict) or set(authority) != required:
        raise LauncherV2BuildError("launcher-v2 authority keys differ")
    if authority["schema"] != AUTHORITY_SCHEMA or authority["release"] != RELEASE:
        raise LauncherV2BuildError("launcher-v2 authority identity differs")
    if authority["activationAllowed"] is not False or authority["bootableClaim"] is not False:
        raise LauncherV2BuildError("launcher-v2 authority cannot activate or claim boot")
    if (
        not isinstance(authority["boundaries"], dict)
        or set(authority["boundaries"]) != v1.BOUNDARY_KEYS
    ):
        raise LauncherV2BuildError("launcher-v2 authority boundary names differ")
    if any(value is not False for value in authority["boundaries"].values()):
        raise LauncherV2BuildError("launcher-v2 authority opens a later boundary")
    if authority["baseV1Authority"] != {
        "path": "native/containment/native-shadow-launcher-build-authority-arm64-v1.json",
        "sha256": v1.AUTHORITY_SHA256,
    }:
        raise LauncherV2BuildError("launcher-v2 authority lost its v1 base")
    if authority["sourceOverlay"] != {
        "path": SOURCE_OVERLAY_PATH.relative_to(REPO_ROOT).as_posix(),
        "sha256": SOURCE_OVERLAY_SHA256,
    }:
        raise LauncherV2BuildError("launcher-v2 authority lost its source overlay")
    if authority["platform"] != v1.PLATFORM or authority["toolchain"] != v1.TOOLCHAIN:
        raise LauncherV2BuildError("launcher-v2 platform or toolchain differs")
    build = authority["build"]
    expected_build = dict(v1.BUILD)
    expected_build["rustflags"] = [
        "--remap-path-prefix",
        "{sourceRoot}=/boole/launcher-build",
        "--remap-path-prefix",
        "{cargoHome}=/boole/cargo-home",
    ]
    expected_build["postprocessCommand"] = None
    expected_build["testCommand"] = [
        "cargo",
        "test",
        "--locked",
        "--offline",
        "--target",
        "aarch64-unknown-linux-gnu",
        "-p",
        "boole-native-shadow-launcher",
        "--features",
        "linux-arm64-authority",
        "--lib",
        "--bins",
    ]
    if build != expected_build:
        raise LauncherV2BuildError("launcher-v2 build recipe differs")
    determinism = authority["determinism"]
    expected_determinism = dict(v1.DETERMINISM)
    expected_determinism.update(
        {
            "forbidBinaryPostprocessing": True,
            "freshCargoHomePerBuild": True,
            "producerPathClasses": [
                "ambient-home",
                "cargo-home",
                "repository-root",
                "rustup-home",
                "source-root",
            ],
        }
    )
    if determinism != expected_determinism:
        raise LauncherV2BuildError("launcher-v2 determinism contract differs")
    generator = authority["generator"]
    if generator != {
        "path": GENERATOR_PATH,
        "sha256": launcher_v2_generator_sha256(TOOL_PATH.read_bytes()),
    }:
        raise LauncherV2BuildError("launcher-v2 generator differs from its pin")
    return authority


def _absolute(path: pathlib.Path, label: str) -> pathlib.Path:
    path = pathlib.Path(path)
    if not path.is_absolute():
        raise LauncherV2BuildError(f"{label} must be an absolute path")
    return path


def build_environment(
    authority: Mapping[str, Any],
    *,
    source_root: pathlib.Path,
    cargo_home: pathlib.Path,
    ambient: Mapping[str, str] | None = None,
) -> dict[str, str]:
    """Create the exact environment for one offline successor build."""

    source_root = _absolute(source_root, "source root")
    cargo_home = _absolute(cargo_home, "Cargo home")
    ambient = dict(os.environ if ambient is None else ambient)
    declared = authority["determinism"]["declaredEnvironment"]
    environment = {
        name: value
        for name, value in declared.items()
        if value is not None
    }
    environment["PATH"] = ambient.get("PATH", "/usr/bin:/bin")
    environment["CARGO_HOME"] = str(cargo_home)
    for name in ("HOME", "RUSTUP_HOME"):
        value = ambient.get(name)
        if value:
            environment[name] = value

    replacements = {
        "{sourceRoot}": str(source_root),
        "{cargoHome}": str(cargo_home),
    }
    flags = []
    for part in authority["build"]["rustflags"]:
        rendered = part
        for marker, value in replacements.items():
            rendered = rendered.replace(marker, value)
        if "{" in rendered or "}" in rendered:
            raise LauncherV2BuildError("a rustflag contains an unresolved path marker")
        flags.append(rendered)
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    return environment


def assert_no_producer_paths(
    artifact: bytes, roots: Mapping[str, pathlib.Path]
) -> dict[str, int]:
    """Refuse an ELF containing any real producer root.

    Only labels and zero counts leave this function.  A raw host path is useful
    for an attacker and unnecessary in an append-only result record.
    """

    counts: dict[str, int] = {}
    for label, path in roots.items():
        path = _absolute(pathlib.Path(path), label)
        needle = os.fsencode(path)
        if not needle:
            raise LauncherV2BuildError(f"{label} is empty")
        count = artifact.count(needle)
        counts[label] = count
        if count:
            raise LauncherV2BuildError(
                f"launcher artifact retains producer path class {label}"
            )
    return counts


def _relative_file(value: Any, label: str) -> pathlib.PurePosixPath:
    if not isinstance(value, str) or not value:
        raise LauncherV2BuildError(f"{label} is not a relative file path")
    path = pathlib.PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise LauncherV2BuildError(f"{label} is not a relative file path")
    return path


def apply_complete_file_overlay(
    rows: list[dict[str, Any]],
    overlay_root: pathlib.Path,
    exported_root: pathlib.Path,
) -> None:
    """Copy pinned complete files into an exported source tree."""

    overlay_root = _absolute(pathlib.Path(overlay_root), "overlay root")
    exported_root = _absolute(pathlib.Path(exported_root), "exported root")
    destinations: set[pathlib.PurePosixPath] = set()
    prepared = []
    for row in rows:
        if not isinstance(row, dict) or set(row) != {
            "destination",
            "sha256",
            "sizeBytes",
            "source",
        }:
            raise LauncherV2BuildError("overlay row keys differ from the contract")
        source_relative = _relative_file(row["source"], "overlay source")
        destination_relative = _relative_file(row["destination"], "overlay destination")
        if destination_relative in destinations:
            raise LauncherV2BuildError("overlay destination is duplicated")
        destinations.add(destination_relative)
        source = overlay_root.joinpath(*source_relative.parts)
        if source.is_symlink() or not source.is_file():
            raise LauncherV2BuildError("overlay source is not a regular file")
        raw = source.read_bytes()
        if row["sizeBytes"] != len(raw) or row["sha256"] != sha256_bytes(raw):
            raise LauncherV2BuildError("overlay source bytes differ from the pin")
        destination = exported_root.joinpath(*destination_relative.parts)
        prepared.append((destination, raw))

    # Validate every row before changing the export, so a late bad row cannot
    # leave a half-applied source tree that a caller might accidentally build.
    for destination, raw in prepared:
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_name(destination.name + ".boole-v2-new")
        if temporary.exists() or temporary.is_symlink():
            raise LauncherV2BuildError("overlay temporary path already exists")
        temporary.write_bytes(raw)
        temporary.replace(destination)


def apply_exact_source_replacement(
    original: bytes,
    *,
    before: bytes,
    after: bytes,
    label: str,
) -> bytes:
    """Apply one pinned textual successor change, never a best-effort patch."""

    count = original.count(before)
    if count != 1:
        raise LauncherV2BuildError(
            f"{label} predecessor block occurs {count} times instead of exactly once"
        )
    return original.replace(before, after, 1)


def apply_source_overlay(
    record: dict[str, Any],
    *,
    exported_root: pathlib.Path,
    repo_root: pathlib.Path = REPO_ROOT,
) -> dict[str, int]:
    """Materialize the frozen successor only inside one temporary export."""

    record = validate_source_overlay(record)
    exported_root = _absolute(pathlib.Path(exported_root), "exported root")
    repo_root = _absolute(pathlib.Path(repo_root), "repository root")
    overlay_root = repo_root / record["overlayRoot"]
    apply_complete_file_overlay(record["completeFiles"], overlay_root, exported_root)

    for row in record["exactReplacements"]:
        required = {
            "after",
            "before",
            "destination",
            "predecessorSha256",
            "requiredMatchCount",
            "successorSha256",
            "successorSizeBytes",
        }
        if not isinstance(row, dict) or set(row) != required:
            raise LauncherV2BuildError("exact replacement keys differ from the contract")
        if row["requiredMatchCount"] != 1:
            raise LauncherV2BuildError("exact replacement must match exactly once")
        destination_relative = _relative_file(
            row["destination"], "exact replacement destination"
        )
        destination = exported_root.joinpath(*destination_relative.parts)
        if destination.is_symlink() or not destination.is_file():
            raise LauncherV2BuildError("exact replacement destination is not a regular file")
        original = destination.read_bytes()
        if sha256_bytes(original) != row["predecessorSha256"]:
            raise LauncherV2BuildError("exact replacement predecessor differs from its pin")
        blocks = {}
        for name in ("before", "after"):
            block = row[name]
            if not isinstance(block, dict) or set(block) != {
                "sha256",
                "sizeBytes",
                "source",
            }:
                raise LauncherV2BuildError(f"{name} block keys differ")
            relative = _relative_file(block["source"], f"{name} block source")
            source = overlay_root.joinpath(*relative.parts)
            if source.is_symlink() or not source.is_file():
                raise LauncherV2BuildError(f"{name} block is not a regular file")
            raw = source.read_bytes()
            if len(raw) != block["sizeBytes"] or sha256_bytes(raw) != block["sha256"]:
                raise LauncherV2BuildError(f"{name} block differs from its pin")
            blocks[name] = raw
        successor = apply_exact_source_replacement(
            original,
            before=blocks["before"],
            after=blocks["after"],
            label=row["destination"],
        )
        if (
            len(successor) != row["successorSizeBytes"]
            or sha256_bytes(successor) != row["successorSha256"]
        ):
            raise LauncherV2BuildError("exact replacement successor differs from its pin")
        temporary = destination.with_name(destination.name + ".boole-v2-new")
        if temporary.exists() or temporary.is_symlink():
            raise LauncherV2BuildError("exact replacement temporary path already exists")
        temporary.write_bytes(successor)
        temporary.replace(destination)

    fixture = record["sharedConsoleFixture"]
    fixture_path = repo_root / fixture["path"]
    fixture_raw = fixture_path.read_bytes()
    if (
        len(fixture_raw) != fixture["sizeBytes"]
        or sha256_bytes(fixture_raw) != fixture["sha256"]
    ):
        raise LauncherV2BuildError("shared console fixture differs from its pin")
    exported_fixture = exported_root / fixture["path"]
    if not exported_fixture.is_file() or exported_fixture.read_bytes() != fixture_raw:
        raise LauncherV2BuildError("exported shared console fixture differs")
    return {
        "completeFiles": len(record["completeFiles"]),
        "exactReplacements": len(record["exactReplacements"]),
    }


def compare_two_artifacts(
    builder: Callable[[int], bytes],
    producer_roots: Mapping[str, pathlib.Path],
) -> dict[str, Any]:
    """Run exactly two builds, scan both, and refuse any byte difference."""

    artifacts = [builder(0), builder(1)]
    scans = [assert_no_producer_paths(raw, producer_roots) for raw in artifacts]
    if artifacts[0] != artifacts[1]:
        raise LauncherV2BuildError(
            "independent launcher-v2 builds are not byte-identical"
        )
    return {
        "buildCount": 2,
        "producerPathHits": scans[0],
        "sha256": sha256_bytes(artifacts[0]),
        "sizeBytes": len(artifacts[0]),
    }


def _export_source_tree(destination: pathlib.Path, *, repo_root: pathlib.Path) -> None:
    archive = destination / ".source.tar"
    with archive.open("wb") as handle:
        completed = subprocess.run(
            ["git", "archive", "--format=tar", "HEAD"],
            cwd=str(repo_root),
            stdin=subprocess.DEVNULL,
            stdout=handle,
            stderr=subprocess.PIPE,
            check=False,
            timeout=300,
        )
    if completed.returncode != 0:
        raise LauncherV2BuildError("git archive of launcher-v2 source failed")
    shutil.unpack_archive(str(archive), str(destination), format="tar")
    archive.unlink()


def verify_effective_sources(
    overlay: dict[str, Any],
    *,
    exported_root: pathlib.Path,
) -> list[str]:
    """Re-hash the complete compiled source surface after overlay application."""

    base = v1.load_authority()
    expected = {
        row["path"]: (row["sha256"], row["sizeBytes"])
        for row in base["sourceFiles"]
    }
    for row in overlay["completeFiles"]:
        expected[row["destination"]] = (row["sha256"], row["sizeBytes"])
    for row in overlay["exactReplacements"]:
        expected[row["destination"]] = (
            row["successorSha256"],
            row["successorSizeBytes"],
        )
    drifted = []
    for relative, (digest, size) in sorted(expected.items()):
        candidate = exported_root / relative
        if candidate.is_symlink() or not candidate.is_file():
            drifted.append(relative)
            continue
        raw = candidate.read_bytes()
        if len(raw) != size or sha256_bytes(raw) != digest:
            drifted.append(relative)
    return drifted


def producer_roots(
    *,
    source_root: pathlib.Path,
    cargo_home: pathlib.Path,
    repo_root: pathlib.Path,
    ambient: Mapping[str, str] | None = None,
) -> dict[str, pathlib.Path]:
    ambient = dict(os.environ if ambient is None else ambient)
    home = ambient.get("HOME")
    if not home:
        raise LauncherV2BuildError("ambient HOME is required for producer-path refusal")
    rustup = ambient.get("RUSTUP_HOME") or str(pathlib.Path(home) / ".rustup")
    roots = {
        "ambient-home": pathlib.Path(home),
        "cargo-home": pathlib.Path(cargo_home),
        "repository-root": pathlib.Path(repo_root),
        "rustup-home": pathlib.Path(rustup),
        "source-root": pathlib.Path(source_root),
    }
    for label, path in roots.items():
        _absolute(path, label)
    return roots


def build_once(
    authority: dict[str, Any],
    *,
    build_root: pathlib.Path,
    repo_root: pathlib.Path = REPO_ROOT,
    ambient: Mapping[str, str] | None = None,
) -> tuple[bytes, dict[str, int]]:
    """Build once from a fresh export and a fresh controlled Cargo home."""

    build_root = _absolute(pathlib.Path(build_root), "build root")
    source_root = build_root / "source"
    cargo_home = build_root / "cargo-home"
    source_root.mkdir(parents=True, exist_ok=False)
    cargo_home.mkdir(parents=True, exist_ok=False)
    _export_source_tree(source_root, repo_root=repo_root)

    base_authority = v1.load_authority()
    drifted = v1.verify_sources(base_authority, repo_root=source_root)
    if drifted:
        raise LauncherV2BuildError(
            "exported v1 base differs before overlay: " + ", ".join(sorted(drifted))
        )
    overlay = load_source_overlay()
    apply_source_overlay(overlay, exported_root=source_root, repo_root=repo_root)
    drifted = verify_effective_sources(overlay, exported_root=source_root)
    if drifted:
        raise LauncherV2BuildError(
            "effective launcher-v2 source differs: " + ", ".join(sorted(drifted))
        )

    environment = build_environment(
        authority,
        source_root=source_root,
        cargo_home=cargo_home,
        ambient=ambient,
    )
    prefetch = subprocess.run(
        authority["build"]["prefetchCommand"],
        cwd=str(source_root),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=1800,
        env=environment,
        close_fds=True,
    )
    if prefetch.returncode != 0:
        tail = prefetch.stderr.decode("utf-8", errors="replace")[-2000:]
        raise LauncherV2BuildError(f"launcher-v2 dependency prefetch failed:\n{tail}")
    tested = subprocess.run(
        authority["build"]["testCommand"],
        cwd=str(source_root),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=3600,
        env=environment,
        close_fds=True,
    )
    if tested.returncode != 0:
        tail = tested.stderr.decode("utf-8", errors="replace")[-4000:]
        raise LauncherV2BuildError(f"launcher-v2 overlay tests failed:\n{tail}")
    completed = subprocess.run(
        authority["build"]["command"],
        cwd=str(source_root),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=3600,
        env=environment,
        close_fds=True,
    )
    if completed.returncode != 0:
        tail = completed.stderr.decode("utf-8", errors="replace")[-4000:]
        raise LauncherV2BuildError(f"launcher-v2 build failed:\n{tail}")
    if authority["build"]["postprocessCommand"] is not None:
        raise LauncherV2BuildError("launcher-v2 binary postprocessing is forbidden")
    artifact = source_root / authority["build"]["artifactRelativePath"]
    if artifact.is_symlink() or not artifact.is_file():
        raise LauncherV2BuildError("launcher-v2 build produced no regular artifact")
    raw = artifact.read_bytes()
    roots = producer_roots(
        source_root=source_root,
        cargo_home=cargo_home,
        repo_root=repo_root,
        ambient=ambient,
    )
    return raw, assert_no_producer_paths(raw, roots)


def build_twice(
    authority: dict[str, Any],
    *,
    repo_root: pathlib.Path = REPO_ROOT,
) -> dict[str, Any]:
    artifacts = []
    scans = []
    for index in range(2):
        with tempfile.TemporaryDirectory(prefix=f"boole-launcher-v2-{index}-") as raw:
            artifact, scan = build_once(
                authority,
                build_root=pathlib.Path(raw),
                repo_root=repo_root,
            )
        artifacts.append(artifact)
        scans.append(scan)
    if artifacts[0] != artifacts[1]:
        raise LauncherV2BuildError(
            "independent launcher-v2 builds are not byte-identical; never retry to force a match"
        )
    if scans[0] != scans[1] or any(scans[0].values()):
        raise LauncherV2BuildError("launcher-v2 producer-path scans differ or are nonzero")
    return {
        "buildCount": 2,
        "producerPathHits": scans[0],
        "sha256": sha256_bytes(artifacts[0]),
        "sizeBytes": len(artifacts[0]),
    }


def build_result(
    authority: dict[str, Any],
    built: dict[str, Any],
    identity: dict[str, str],
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "authoritySha256": AUTHORITY_SHA256,
        "bootableClaim": False,
        "boundaries": dict(authority["boundaries"]),
        "independentBuildCount": built["buildCount"],
        "launcher": {
            "guestLogicalPath": authority["build"]["guestLogicalPath"],
            "sha256": built["sha256"],
            "sizeBytes": built["sizeBytes"],
        },
        "observedToolchain": dict(identity),
        "overlaySourceTestRuns": 2,
        "producerPathHits": dict(built["producerPathHits"]),
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "sourceOverlaySha256": SOURCE_OVERLAY_SHA256,
        "status": RESULT_STATUS,
    }


def seal_or_reprove(
    result: dict[str, Any],
    *,
    result_path: pathlib.Path = RESULT_PATH,
) -> str:
    raw = v1.canonical_json(result)
    if result_path.exists():
        if result_path.read_bytes() != raw:
            raise LauncherV2BuildError(
                "launcher-v2 build differs from its sealed result; never overwrite it"
            )
        return "re-proved"
    v1.payload._write_result_once(result_path, raw)
    return "sealed"


def _require_arm64_linux() -> None:
    uname = os.uname()
    if uname.sysname != "Linux" or uname.machine not in {"aarch64", "arm64"}:
        raise LauncherV2BuildError(
            f"launcher-v2 build requires Linux arm64; this host is {uname.sysname} {uname.machine}"
        )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--build", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    authority = load_authority()
    overlay = load_source_overlay()
    base = v1.load_authority()
    drifted = v1.verify_sources(base)
    if drifted:
        raise LauncherV2BuildError(
            "historical v1 source drifted: " + ", ".join(sorted(drifted))
        )
    validate_source_overlay(overlay)
    if args.check:
        print(
            f"launcher-v2 authority: {AUTHORITY_SHA256} overlay={SOURCE_OVERLAY_SHA256} built=no"
        )
        return 0
    _require_arm64_linux()
    identity = v1.verify_toolchain(base)
    built = build_twice(authority)
    disposition = seal_or_reprove(build_result(authority, built, identity))
    print(
        f"launcher-v2 build: {RESULT_STATUS} builds=2 identical=yes "
        f"producer-path-hits=0 {disposition} sha256={built['sha256']} "
        f"bytes={built['sizeBytes']}"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover
    try:
        raise SystemExit(main())
    except LauncherV2BuildError as error:
        print(f"launcher-v2 build refused: {error}", file=sys.stderr)
        raise SystemExit(1) from error
