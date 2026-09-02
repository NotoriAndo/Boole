#!/usr/bin/env python3
"""Build the successor guest as a reversible closed-local development image.

This path deliberately does not consume, emulate, or mint the historical A7
production authority.  It verifies the already-sealed P4 -> R3 -> F7 input
chain, then reuses the same image backend and readback verifier as the locked
production implementation.  Its outputs are disposable engineering artifacts:
they establish neither a boot, a release, nor any activation right.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import fcntl
import hashlib
import importlib
import json
import os
import pathlib
import re
import shutil
import struct
import subprocess
import sys
from collections.abc import Mapping
from typing import Any, Callable, Optional

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_rootfs_builder_boot_arm64_v4 as builder_v4
from scripts import native_shadow_boot_staging_measure_arm64_v1 as staging_measure
from scripts import native_shadow_successor_produce_phase_arm64_v3 as producer_v3
from scripts import native_shadow_successor_produce_phase_arm64_v5 as sealed


SCHEMA = "boole.native-shadow.closed-local-image-to-readiness.arm64.v1"
PREFLIGHT_STATUS = "READY-NO-IMAGE-CREATED"
BUILD_STATUS = "CLOSED-LOCAL-IMAGE-VERIFIED"
ARTIFACT_CLASS = "DISPOSABLE-DEVELOPMENT"
RUN_LABEL_PATTERN = r"[a-z0-9][a-z0-9._-]{0,79}"
LOOP_SET_STATUS64 = 0x4C04
LOOP_GET_STATUS64 = 0x4C05
LO_FLAGS_AUTOCLEAR = 4
LOOP_INFO64_SIZE = 232
LOOP_FLAGS_OFFSET = 52
AUTHORITY_STAGING_PATH = "usr/share/boole/native-shadow"
AUTHORITY_MOUNTED_PATH = "/" + AUTHORITY_STAGING_PATH
AUTHORITY_STAGING_PATHS = (
    AUTHORITY_STAGING_PATH,
    AUTHORITY_STAGING_PATH + "/checkers",
    AUTHORITY_STAGING_PATH + "/checkers/rust-tuple-struct-project-v1",
    AUTHORITY_STAGING_PATH + "/fixtures",
    AUTHORITY_STAGING_PATH
    + "/fixtures/a-rooted-native-mining-e2e-v1-real-history",
)
AUTHORITY_MOUNTED_PATHS = tuple("/" + path for path in AUTHORITY_STAGING_PATHS)
TOOLCHAIN_STAGING_PATHS = (
    "opt/boole/native-checker-toolchain",
    "opt/boole/native-checker-toolchain/bin",
)
TOOLCHAIN_MOUNTED_PATHS = tuple("/" + path for path in TOOLCHAIN_STAGING_PATHS)
FIXED_DIRECTORY_MODE = 0o555
MAC4_RELAY_STAGING_PATH = "usr/libexec/boole/boole-native-shadow-mac4-relay"
MAC4_SERVICE_SOURCE = "native/systemd/boole-native-shadow-mac4-relay.service"
MAC4_SERVICE_STAGING_PATH = (
    "usr/lib/systemd/system/boole-native-shadow-mac4-relay.service"
)
MAC4_SERVICE_ENABLEMENT_PATH = (
    "etc/systemd/system/multi-user.target.wants/"
    "boole-native-shadow-mac4-relay.service"
)
MAC4_CONTRACT_SOURCE = (
    "native/containment/"
    "native-shadow-mac4-authenticated-channel-contract-v1.json"
)
MAC4_CONTRACT_STAGING_PATH = (
    AUTHORITY_STAGING_PATH + "/mac4-channel-contract-v1.json"
)
MAC4_CONTRACT_SHA256 = (
    "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd"
)
MAC4_CONTRACT_SIZE_BYTES = 1_977
MAC4_SERVICE_SHA256 = (
    "394195d0ad7a5bbe3a74f5ffa3a490e617327d21bf054e554327d884f1ef73c4"
)
MAC4_SERVICE_SIZE_BYTES = 938
MAC4_KERNEL_RELEASE = "6.8.0-31-generic"
MAC4_MODULE_DIRECTORY = "usr/lib/modules/" + MAC4_KERNEL_RELEASE
MAC4_MODULE_LOAD_STAGING_PATH = "etc/modules"
MAC4_MODULE_LOAD_BYTES = (
    b"vsock\n"
    b"vmw_vsock_virtio_transport_common\n"
    b"vmw_vsock_virtio_transport\n"
)
MAC4_REQUIRED_MODULE_OBJECTS = (
    "kernel/net/vmw_vsock/vsock.ko.zst",
    "kernel/net/vmw_vsock/vmw_vsock_virtio_transport_common.ko.zst",
    "kernel/net/vmw_vsock/vmw_vsock_virtio_transport.ko.zst",
)
MAC4_REQUIRED_MODULE_INDEX_NAMES = (
    "modules.alias",
    "modules.alias.bin",
    "modules.builtin.alias.bin",
    "modules.builtin.bin",
    "modules.dep",
    "modules.dep.bin",
    "modules.devname",
    "modules.softdep",
    "modules.symbols",
    "modules.symbols.bin",
)
MAC4_SEALED_MODULE_METADATA_NAMES = frozenset(
    ("modules.builtin", "modules.builtin.modinfo", "modules.order")
)
MAC4_OVERLAY_PATHS = (
    MAC4_RELAY_STAGING_PATH,
    MAC4_SERVICE_STAGING_PATH,
    MAC4_SERVICE_ENABLEMENT_PATH,
    MAC4_CONTRACT_STAGING_PATH,
    MAC4_MODULE_LOAD_STAGING_PATH,
)


@dataclasses.dataclass(frozen=True)
class DevelopmentReplayMaterial:
    source: str
    staging_path: str
    sha256: str
    size_bytes: int


DEVELOPMENT_REPLAY_MATERIALS = (
    DevelopmentReplayMaterial(
        "native/containment/"
        "native-shadow-closed-local-replay-registry-overlay-arm64-v1.json",
        AUTHORITY_STAGING_PATH + "/closed-local-replay-registry-overlay-v1.json",
        "2962adef8d1aea9ba1c8466b8e014b71f1ec3c9555ce8b685d58ede6b631fe74",
        5_461,
    ),
    DevelopmentReplayMaterial(
        "native/containment/"
        "native-shadow-closed-local-replay-grant-arm64-v1.json",
        AUTHORITY_STAGING_PATH + "/closed-local-replay-grant-v1.json",
        "bd5cd9fc87e5e47a23e6fa12844ec0c47bdb01ee34090cddff24568c18d7236f",
        4_548,
    ),
    DevelopmentReplayMaterial(
        "native/containment/"
        "native-shadow-closed-local-replay-execution-authority-arm64-v1.json",
        AUTHORITY_STAGING_PATH
        + "/closed-local-replay-execution-authority-v1.json",
        "d220d20b7adaa22357929729d2f0666a8c9cbe50ce8031f90539ba1309950c6b",
        2_106,
    ),
    DevelopmentReplayMaterial(
        "fixtures/native-shadow/"
        "a-rooted-native-mining-e2e-v1-real-history/task.json",
        AUTHORITY_STAGING_PATH
        + "/fixtures/a-rooted-native-mining-e2e-v1-real-history/task.json",
        "f25a8a6d92ac556937eaacbec6d12d9d09be675878eb7d942952b35838ee7c82",
        1_303,
    ),
    DevelopmentReplayMaterial(
        "fixtures/native-shadow/"
        "a-rooted-native-mining-e2e-v1-real-history/anchor.rs",
        AUTHORITY_STAGING_PATH
        + "/fixtures/a-rooted-native-mining-e2e-v1-real-history/anchor.rs",
        "693f62acfa0626a0831c9133a26fcfc1dbb30922c1ab2036231c42a363cfd7fe",
        181,
    ),
)

DEVELOPMENT_DISABLED_SYSTEMD_UNITS = (
    "getty-static.service",
    "getty@.service",
    "ldconfig.service",
    "serial-getty@.service",
)
DEVELOPMENT_SYSTEMD_MASK_TARGET = "/dev/null"
DEVELOPMENT_SYSTEMD_MASK_PATHS = tuple(
    "etc/systemd/system/" + unit for unit in DEVELOPMENT_DISABLED_SYSTEMD_UNITS
)

DEVELOPMENT_DERIVED_DIRECTORY_PATHS = (
    AUTHORITY_STAGING_PATH + "/fixtures",
    AUTHORITY_STAGING_PATH
    + "/fixtures/a-rooted-native-mining-e2e-v1-real-history",
)


class ClosedLocalImageError(RuntimeError):
    pass


def _require_fixed_directory(row: Any, description: str) -> None:
    if (
        not isinstance(row, Mapping)
        or row.get("kind") != "directory"
        or row.get("mode") != FIXED_DIRECTORY_MODE
        or row.get("uid") != 0
        or row.get("gid") != 0
    ):
        raise ClosedLocalImageError(
            f"{description} must be root:root mode 0555"
        )


def _development_replay_entries(
    repository_root: pathlib.Path,
) -> dict[str, dict[str, Any]]:
    root = pathlib.Path(repository_root).resolve()
    entries: dict[str, dict[str, Any]] = {}
    for material in DEVELOPMENT_REPLAY_MATERIALS:
        pure = pathlib.PurePosixPath(material.source)
        if (
            pure.is_absolute()
            or ".." in pure.parts
            or pure.as_posix() != material.source
        ):
            raise ClosedLocalImageError("closed-local replay source path is unsafe")
        candidate = root.joinpath(*pure.parts)
        try:
            resolved = candidate.resolve(strict=True)
            info = candidate.lstat()
            raw = candidate.read_bytes()
        except OSError as exc:
            raise ClosedLocalImageError(
                f"closed-local replay material is unreadable: {material.source}"
            ) from exc
        if resolved != candidate or not candidate.is_file() or candidate.is_symlink():
            raise ClosedLocalImageError(
                f"closed-local replay material is not one regular file: {material.source}"
            )
        if info.st_size != material.size_bytes or len(raw) != material.size_bytes:
            raise ClosedLocalImageError(
                f"closed-local replay material size differs: {material.source}"
            )
        if hashlib.sha256(raw).hexdigest() != material.sha256:
            raise ClosedLocalImageError(
                f"closed-local replay material digest differs: {material.source}"
            )
        entries[material.staging_path] = {
            "path": material.staging_path,
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "raw": raw,
        }
    return entries


def _development_systemd_mask_entries() -> dict[str, dict[str, Any]]:
    return {
        path: {
            "path": path,
            "kind": "symlink",
            "mode": 0o777,
            "uid": 0,
            "gid": 0,
            "target": DEVELOPMENT_SYSTEMD_MASK_TARGET,
        }
        for path in DEVELOPMENT_SYSTEMD_MASK_PATHS
    }


def _pinned_mac4_source(
    repository_root: pathlib.Path,
    relative: str,
    *,
    sha256: str,
    size_bytes: int,
) -> bytes:
    root = pathlib.Path(repository_root).resolve()
    candidate = root.joinpath(*pathlib.PurePosixPath(relative).parts)
    try:
        resolved = candidate.resolve(strict=True)
        raw = candidate.read_bytes()
    except OSError as exc:
        raise ClosedLocalImageError(f"MAC.4 source is unreadable: {relative}") from exc
    if resolved != candidate or not candidate.is_file() or candidate.is_symlink():
        raise ClosedLocalImageError(f"MAC.4 source is not one regular file: {relative}")
    if len(raw) != size_bytes or hashlib.sha256(raw).hexdigest() != sha256:
        raise ClosedLocalImageError(f"MAC.4 source differs from its pin: {relative}")
    return raw


def _development_mac4_entries(
    repository_root: pathlib.Path, relay_binary: bytes
) -> dict[str, dict[str, Any]]:
    """Stage the reversible relay beside, never inside, the sealed launcher."""

    if not isinstance(relay_binary, bytes) or not relay_binary:
        raise ClosedLocalImageError("MAC.4 relay binary is absent")
    service = _pinned_mac4_source(
        repository_root,
        MAC4_SERVICE_SOURCE,
        sha256=MAC4_SERVICE_SHA256,
        size_bytes=MAC4_SERVICE_SIZE_BYTES,
    )
    contract = _pinned_mac4_source(
        repository_root,
        MAC4_CONTRACT_SOURCE,
        sha256=MAC4_CONTRACT_SHA256,
        size_bytes=MAC4_CONTRACT_SIZE_BYTES,
    )
    return {
        MAC4_RELAY_STAGING_PATH: {
            "path": MAC4_RELAY_STAGING_PATH,
            "kind": "file",
            "mode": 0o555,
            "uid": 0,
            "gid": 0,
            "raw": relay_binary,
        },
        MAC4_SERVICE_STAGING_PATH: {
            "path": MAC4_SERVICE_STAGING_PATH,
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "raw": service,
        },
        MAC4_CONTRACT_STAGING_PATH: {
            "path": MAC4_CONTRACT_STAGING_PATH,
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "raw": contract,
        },
        MAC4_SERVICE_ENABLEMENT_PATH: {
            "path": MAC4_SERVICE_ENABLEMENT_PATH,
            "kind": "symlink",
            "mode": 0o777,
            "uid": 0,
            "gid": 0,
            "target": "/" + MAC4_SERVICE_STAGING_PATH,
        },
        MAC4_MODULE_LOAD_STAGING_PATH: {
            "path": MAC4_MODULE_LOAD_STAGING_PATH,
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "raw": MAC4_MODULE_LOAD_BYTES,
        },
    }


def _module_metadata_identities(
    entries: Mapping[str, Mapping[str, Any]],
) -> dict[str, dict[str, Any]]:
    identities: dict[str, dict[str, Any]] = {}
    for path in sorted(entries, key=lambda value: value.encode("utf-8")):
        row = entries[path]
        raw = row.get("raw")
        if (
            row.get("path") != path
            or row.get("kind") != "file"
            or row.get("mode") != 0o444
            or row.get("uid") != 0
            or row.get("gid") != 0
            or not isinstance(raw, bytes)
        ):
            raise ClosedLocalImageError("MAC.4 module metadata entry is invalid")
        identities[path] = {
            "sha256": hashlib.sha256(raw).hexdigest(),
            "sizeBytes": len(raw),
        }
    return identities


def _static_test_module_index_entries(
    _entries: Optional[Mapping[str, Mapping[str, Any]]] = None,
) -> dict[str, dict[str, Any]]:
    """Small deterministic stand-in used only by injected unit-test backends."""

    return {
        MAC4_MODULE_DIRECTORY + "/" + name: {
            "path": MAC4_MODULE_DIRECTORY + "/" + name,
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "raw": (name + "\n").encode("ascii"),
        }
        for name in MAC4_REQUIRED_MODULE_INDEX_NAMES
    }


class HostDepmodModuleIndexGenerator:
    """Generate module lookup tables from the sealed objects without an image.

    The runner's depmod is only a deterministic transformer.  Its complete
    output is folded into the staging tree and later checked again from the
    read-only ext4 image.  Independent image replicas still have to agree byte
    for byte, so a runner-tool drift cannot silently qualify an image.
    """

    def __init__(
        self,
        *,
        depmod: pathlib.Path,
        scratch: pathlib.Path,
        runner: Callable[..., Any] = subprocess.run,
    ) -> None:
        candidate = pathlib.Path(depmod)
        try:
            resolved = candidate.resolve(strict=True)
        except OSError as exc:
            raise ClosedLocalImageError("depmod is unreadable") from exc
        if not resolved.is_file() or not os.access(resolved, os.X_OK):
            raise ClosedLocalImageError("depmod is not one executable file")
        # Keep the caller-visible multicall name.  Ubuntu's /usr/sbin/depmod is
        # a kmod symlink, and resolving it changes argv[0] to "kmod", which no
        # longer selects depmod behavior.
        self._depmod = pathlib.Path(os.path.abspath(candidate))
        self._scratch = pathlib.Path(scratch)
        self._runner = runner
        self._cached_input_digest: Optional[str] = None
        self._cached: Optional[dict[str, dict[str, Any]]] = None

    @staticmethod
    def _input_digest(entries: Mapping[str, Mapping[str, Any]]) -> str:
        digest = hashlib.sha256()
        prefix = MAC4_MODULE_DIRECTORY + "/"
        found = 0
        for path in sorted(entries, key=lambda value: value.encode("utf-8")):
            if path != MAC4_MODULE_DIRECTORY and not path.startswith(prefix):
                continue
            row = entries[path]
            digest.update(path.encode("utf-8") + b"\0")
            digest.update(str(row.get("kind")).encode("ascii") + b"\0")
            raw = row.get("raw")
            if isinstance(raw, bytes):
                digest.update(hashlib.sha256(raw).digest())
            target = row.get("target")
            if isinstance(target, str):
                digest.update(target.encode("utf-8"))
            found += 1
        if found == 0:
            raise ClosedLocalImageError("sealed kernel module tree is absent")
        return digest.hexdigest()

    @staticmethod
    def _write_module_tree(
        root: pathlib.Path, entries: Mapping[str, Mapping[str, Any]]
    ) -> set[str]:
        module_root = root / "lib/modules" / MAC4_KERNEL_RELEASE
        module_root.mkdir(parents=True, mode=0o755)
        prefix = MAC4_MODULE_DIRECTORY + "/"
        original_direct_names: set[str] = set()
        for path in sorted(entries, key=lambda value: value.encode("utf-8")):
            if path != MAC4_MODULE_DIRECTORY and not path.startswith(prefix):
                continue
            relative = "" if path == MAC4_MODULE_DIRECTORY else path[len(prefix) :]
            if not relative:
                continue
            pure = pathlib.PurePosixPath(relative)
            if pure.is_absolute() or ".." in pure.parts:
                raise ClosedLocalImageError("kernel module staging path is unsafe")
            target = module_root.joinpath(*pure.parts)
            row = entries[path]
            kind = row.get("kind")
            if len(pure.parts) == 1:
                original_direct_names.add(pure.name)
            if kind == "directory":
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            if kind == "file" and isinstance(row.get("raw"), bytes):
                with target.open("xb") as stream:
                    stream.write(row["raw"])
                target.chmod(0o444)
                continue
            if kind == "symlink" and isinstance(row.get("target"), str):
                link_target = pathlib.PurePosixPath(row["target"])
                if link_target.is_absolute() or ".." in link_target.parts:
                    # build/source links are irrelevant to dependency indexing;
                    # do not let them become host authority.
                    continue
                target.symlink_to(row["target"])
                continue
            raise ClosedLocalImageError("kernel module staging entry is invalid")
        for relative in MAC4_REQUIRED_MODULE_OBJECTS:
            if not (module_root / pathlib.Path(*pathlib.PurePosixPath(relative).parts)).is_file():
                raise ClosedLocalImageError(
                    "required vsock module object is absent: " + relative
                )
        return original_direct_names

    def __call__(
        self, entries: Mapping[str, Mapping[str, Any]]
    ) -> dict[str, dict[str, Any]]:
        input_digest = self._input_digest(entries)
        if self._cached is not None:
            if input_digest != self._cached_input_digest:
                raise ClosedLocalImageError(
                    "kernel module inputs changed between staging passes"
                )
            return {path: dict(row) for path, row in self._cached.items()}
        work = self._scratch / "mac4-depmod-index-v1"
        if os.path.lexists(work):
            raise ClosedLocalImageError("depmod scratch already exists")
        work.mkdir(mode=0o700)
        try:
            original_names = self._write_module_tree(work, entries)
            command = [str(self._depmod)]
            command.extend(
                ["-b", str(work), "-o", str(work), "-a", MAC4_KERNEL_RELEASE]
            )
            completed = self._runner(
                command,
                cwd=work,
                env={"LANG": "C", "LC_ALL": "C", "PATH": "/usr/bin:/usr/sbin"},
                capture_output=True,
                text=True,
                check=False,
            )
            if completed.returncode != 0:
                raise ClosedLocalImageError(
                    "depmod failed while generating MAC.4 module metadata: "
                    + completed.stderr.strip()
                )
            module_root = work / "lib/modules" / MAC4_KERNEL_RELEASE
            generated: dict[str, dict[str, Any]] = {}
            for candidate in sorted(module_root.glob("modules.*"), key=lambda path: os.fsencode(path.name)):
                if candidate.name in original_names:
                    continue
                if not candidate.is_file() or candidate.is_symlink():
                    raise ClosedLocalImageError("depmod produced a non-regular index")
                raw = candidate.read_bytes()
                path = MAC4_MODULE_DIRECTORY + "/" + candidate.name
                generated[path] = {
                    "path": path,
                    "kind": "file",
                    "mode": 0o444,
                    "uid": 0,
                    "gid": 0,
                    "raw": raw,
                }
            missing = sorted(
                set(MAC4_REQUIRED_MODULE_INDEX_NAMES)
                - {path.rsplit("/", 1)[-1] for path in generated}
            )
            if missing:
                raise ClosedLocalImageError(
                    "depmod omitted required MAC.4 indexes: " + ", ".join(missing)
                )
            self._cached_input_digest = input_digest
            self._cached = generated
            return {path: dict(row) for path, row in generated.items()}
        finally:
            shutil.rmtree(work, ignore_errors=True)

    def identities(self) -> dict[str, dict[str, Any]]:
        if self._cached is None:
            raise ClosedLocalImageError("MAC.4 module metadata was not generated")
        return _module_metadata_identities(self._cached)


def _require_all_fixed_directories(entries: Mapping[str, Any]) -> None:
    for path in AUTHORITY_STAGING_PATHS:
        _require_fixed_directory(entries.get(path), "installed authority directory")
    for path in TOOLCHAIN_STAGING_PATHS:
        _require_fixed_directory(entries.get(path), "installed toolchain directory")


def _development_prepare_staging(
    *,
    validated: dict[str, Any],
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Mapping[str, Any]],
    preregistration: Optional[Mapping[str, Any]] = None,
) -> producer_v3.PreparedStaging:
    """Add the development replay overlay without rewriting the sealed base."""

    if preregistration is None:
        raise ClosedLocalImageError("development staging needs the sealed preregistration")
    expected = preregistration.get("expectedPreflight", {}).get("measurement")
    if not isinstance(expected, dict):
        raise ClosedLocalImageError("sealed staging measurement is absent")
    entries = builder_v4.materialize_staging_tree(
        validated,
        pathlib.Path(repository_root),
        pathlib.Path(artifact_store),
        launcher_binary=launcher_binary,
        nested_tree=nested_tree,
    )
    if not isinstance(entries, Mapping):
        raise ClosedLocalImageError("development staging assembler returned no mapping")

    historical = dict(entries)
    for material in DEVELOPMENT_REPLAY_MATERIALS:
        if historical.pop(material.staging_path, None) is None:
            raise ClosedLocalImageError(
                f"closed-local replay material was not staged: {material.staging_path}"
            )
    for path in DEVELOPMENT_SYSTEMD_MASK_PATHS:
        if historical.pop(path, None) is None:
            raise ClosedLocalImageError(
                f"closed-local systemd mask was not staged: {path}"
            )
    for path in DEVELOPMENT_DERIVED_DIRECTORY_PATHS:
        if historical.pop(path, None) is None:
            raise ClosedLocalImageError(
                f"closed-local replay parent was not derived: {path}"
            )
    for path in MAC4_OVERLAY_PATHS:
        if historical.pop(path, None) is None:
            raise ClosedLocalImageError(f"MAC.4 overlay was not staged: {path}")
    module_prefix = MAC4_MODULE_DIRECTORY + "/"
    generated_paths = [
        path
        for path in historical
        if path.startswith(module_prefix)
        and pathlib.PurePosixPath(path).parent.as_posix() == MAC4_MODULE_DIRECTORY
        and pathlib.PurePosixPath(path).name.startswith("modules.")
        and pathlib.PurePosixPath(path).name
        not in MAC4_SEALED_MODULE_METADATA_NAMES
    ]
    for path in generated_paths:
        historical.pop(path)
    missing_indexes = sorted(
        set(MAC4_REQUIRED_MODULE_INDEX_NAMES)
        - {path.rsplit("/", 1)[-1] for path in generated_paths}
    )
    if missing_indexes:
        raise ClosedLocalImageError(
            "MAC.4 generated module metadata is incomplete: "
            + ", ".join(missing_indexes)
        )
    historical_measurement = staging_measure.builder_totals(historical)
    if not producer_v3._strict_equal(historical_measurement, expected):
        raise ClosedLocalImageError(
            "development replay overlay changed the sealed historical staging tree"
        )
    measurement = staging_measure.builder_totals(dict(entries))
    return producer_v3.PreparedStaging(entries=entries, measurement=measurement)


@contextlib.contextmanager
def _development_fixed_directory_contract(
    repository_root: pathlib.Path,
    relay_binary: bytes,
    module_index_generator: Callable[
        [Mapping[str, Mapping[str, Any]]], dict[str, dict[str, Any]]
    ],
):
    """Correct and verify derived parents with fixed runtime contracts.

    The sealed source lock tracks files beneath the installed authority and
    toolchain directories but carries no rows for these directories themselves.
    The inherited assembler therefore derives them with the generic 0755 parent
    mode, while their runtime readers require exact 0555.  Scope the correction
    to this reversible development lane so historical sealed producers remain
    byte-preserved.
    """

    namespace = builder_v4.materialize_staging_tree.__globals__.get("_IMPL")
    if not isinstance(namespace, dict):
        raise ClosedLocalImageError("development builder namespace is unavailable")
    original_ensure = namespace.get("_ensure_parents")
    original_assemble = namespace.get("_assemble_entries")
    original_prepare_staging = producer_v3.prepare_staging
    if not callable(original_ensure) or not callable(original_assemble):
        raise ClosedLocalImageError("development parent derivation is unavailable")
    replay_entries = _development_replay_entries(repository_root)
    systemd_mask_entries = _development_systemd_mask_entries()
    mac4_entries = _development_mac4_entries(repository_root, relay_binary)

    def ensure_parents(entries):
        original_ensure(entries)
        for path in (*AUTHORITY_STAGING_PATHS, *TOOLCHAIN_STAGING_PATHS):
            row = entries.get(path)
            if (
                isinstance(row, Mapping)
                and row.get("kind") == "directory"
                and row.get("mode") == 0o755
                and row.get("uid") == 0
                and row.get("gid") == 0
            ):
                row = dict(row, mode=FIXED_DIRECTORY_MODE)
                entries[path] = row
            if row is not None:
                description = (
                    "installed authority directory"
                    if path in AUTHORITY_STAGING_PATHS
                    else "installed toolchain directory"
                )
                _require_fixed_directory(row, description)

    def assemble_entries(*args, **kwargs):
        entries = original_assemble(*args, **kwargs)
        if not isinstance(entries, dict):
            raise ClosedLocalImageError("development assembler returned no mutable mapping")
        overlay_entries = {**replay_entries, **systemd_mask_entries, **mac4_entries}
        collisions = sorted(set(entries).intersection(overlay_entries))
        if collisions:
            raise ClosedLocalImageError(
                "closed-local development overlay collides with sealed staging: "
                + ", ".join(collisions)
            )
        entries.update({path: dict(row) for path, row in overlay_entries.items()})
        module_indexes = module_index_generator(entries)
        index_collisions = sorted(set(entries).intersection(module_indexes))
        if index_collisions:
            raise ClosedLocalImageError(
                "generated MAC.4 module metadata collides with sealed staging: "
                + ", ".join(index_collisions)
            )
        entries.update({path: dict(row) for path, row in module_indexes.items()})
        ensure_parents(entries)
        _require_all_fixed_directories(entries)
        return entries

    namespace["_ensure_parents"] = ensure_parents
    namespace["_assemble_entries"] = assemble_entries
    producer_v3.prepare_staging = _development_prepare_staging
    try:
        yield
    finally:
        producer_v3.prepare_staging = original_prepare_staging
        namespace["_assemble_entries"] = original_assemble
        namespace["_ensure_parents"] = original_ensure


@dataclasses.dataclass(frozen=True)
class DevelopmentGenerationChain:
    correction: Mapping[str, Any]
    fresh_rehearsal: Mapping[str, Any]
    fingerprint: Mapping[str, Any]
    identities: Mapping[str, sealed.FileIdentity]
    import_identities: tuple[sealed.FileIdentity, ...]
    output_names: tuple[str, ...] = sealed.OUTPUT_NAMES


def _authorisations() -> dict[str, Any]:
    return {
        **dict(sealed.ZERO_AUTHORISATIONS),
        "activationAllowed": False,
        "imageProductionAuthorised": False,
    }


def _claims() -> dict[str, bool]:
    return {
        "activation": False,
        "bootVerified": False,
        "consensus": False,
        "mining": False,
        "p2p": False,
        "productionRelease": False,
        "reward": False,
        "serving": False,
        "testnet": False,
    }


def _identity_rows(identities) -> list[dict[str, Any]]:
    return [identity.record() for identity in identities]


def verify_development_generation_chain(
    repository_root: pathlib.Path,
) -> DevelopmentGenerationChain:
    """Verify P4 -> R3 -> F7 while requiring A7 and result-v7 to be absent.

    P4/R3/F7 remain historical facts about the exact low-level backend.  A7 is
    a production-only one-shot and is intentionally neither required nor
    simulated by this reversible development path.
    """

    root = pathlib.Path(repository_root).resolve()
    preregistered = sealed.verify_preregistered_generation(root)
    sealed._require_absent(root, sealed.A7_PATH, "historical A7 production authority")
    sealed._require_absent(root, sealed.RESULT_V7_PATH, "historical result-v7")

    p4_id = preregistered.correction_identity
    p4 = preregistered.correction
    generation = _identity_rows(preregistered.generation_identities)
    reused = _identity_rows(preregistered.reused_identities)
    dispatch_fence = p4_id.record()

    r3_id, r3 = sealed._load_canonical(root, sealed.R3_PATH)
    r3_keys = {
        "activationAllowed",
        "authorisations",
        "bootableClaim",
        "boundInputs",
        "effects",
        "executionEnvelope",
        "generationFiles",
        sealed.P4_FIELD,
        "measurement",
        "predecessors",
        "repeatable",
        "reusedPinnedUpstream",
        "schema",
        "status",
    }
    sealed._require_exact_keys(r3, r3_keys, "development R3")
    if r3["schema"] != sealed.R3_SCHEMA or r3["status"] != sealed.R3_STATUS:
        raise ClosedLocalImageError("R3 schema or status differs")
    if r3["activationAllowed"] is not False or r3["bootableClaim"] is not False:
        raise ClosedLocalImageError("R3 claims activation or boot")
    sealed._require_strict_equal(
        r3["authorisations"], sealed.ZERO_AUTHORISATIONS, "R3 authority"
    )
    sealed._require_strict_equal(r3["effects"], sealed.ZERO_EFFECTS, "R3 effects")
    sealed._require_strict_equal(
        r3[sealed.P4_FIELD], dispatch_fence, "R3 P4 identity"
    )
    if r3["predecessors"] != [p4_id.record()]:
        raise ClosedLocalImageError("R3 predecessors differ")
    if r3["generationFiles"] != generation:
        raise ClosedLocalImageError("R3 generation files differ")
    if r3["reusedPinnedUpstream"] != reused:
        raise ClosedLocalImageError("R3 reused inputs differ")
    if r3["boundInputs"] != [p4_id.record(), *generation, *reused]:
        raise ClosedLocalImageError("R3 bound-input union differs")
    if r3["repeatable"] is not True or not isinstance(r3["measurement"], dict):
        raise ClosedLocalImageError("R3 repeatability evidence differs")

    p1_identity, p1 = sealed._load_canonical(root, sealed.P1_PATH)
    if (
        p1_identity.sha256 != sealed.P1_SHA256
        or p1_identity.size_bytes != sealed.P1_SIZE_BYTES
    ):
        raise ClosedLocalImageError("P1 identity differs")
    try:
        expected_measurement = p1["expectedPreflight"]["measurement"]
    except (KeyError, TypeError) as exc:
        raise ClosedLocalImageError("P1 has no sealed staging measurement") from exc
    sealed._require_strict_equal(
        r3["measurement"], expected_measurement, "R3 staging measurement"
    )

    f7_id, f7 = sealed._load_canonical(root, sealed.F7_PATH)
    f7_keys = {
        "authorisations",
        "boundaries",
        "files",
        sealed.P4_FIELD,
        "predecessors",
        "rehearsalGate",
        "schema",
        "status",
        "subject",
        "whatThisRecordDoesNotEstablish",
    }
    sealed._require_exact_keys(f7, f7_keys, "development F7")
    if f7["schema"] != sealed.F7_SCHEMA or f7["status"] != sealed.F7_STATUS:
        raise ClosedLocalImageError("F7 schema or status differs")
    sealed._require_strict_equal(
        f7["authorisations"], sealed.ZERO_AUTHORISATIONS, "F7 authority"
    )
    sealed._require_strict_equal(
        f7["boundaries"],
        {
            "activationAllowed": False,
            "bootableClaim": False,
            "servingClaim": False,
        },
        "F7 boundaries",
    )
    sealed._require_strict_equal(
        f7[sealed.P4_FIELD], dispatch_fence, "F7 P4 identity"
    )
    if f7["files"] != generation:
        raise ClosedLocalImageError("F7 generation files differ")
    if f7["predecessors"] != [p4_id.record(), r3_id.record()]:
        raise ClosedLocalImageError("F7 predecessors differ")
    gate_identity = sealed._read_regular(root, sealed.R3_GATE_PATH)[0]
    if f7["rehearsalGate"] != gate_identity.record():
        raise ClosedLocalImageError("F7 rehearsal gate differs")

    return DevelopmentGenerationChain(
        correction=p4,
        fresh_rehearsal=r3,
        fingerprint=f7,
        identities={"P4": p4_id, "R3": r3_id, "F7": f7_id},
        import_identities=preregistered.import_identities,
    )


def _real_empty_directory(path: pathlib.Path, context: str) -> pathlib.Path:
    try:
        resolved = pathlib.Path(path).resolve(strict=True)
    except OSError as exc:
        raise ClosedLocalImageError(f"{context} is not a real directory") from exc
    if not resolved.is_dir() or pathlib.Path(path).is_symlink():
        raise ClosedLocalImageError(f"{context} is not a real directory")
    if any(resolved.iterdir()):
        raise ClosedLocalImageError(f"{context} is not empty")
    return resolved


def _launcher_bytes(path: pathlib.Path, *, require_sealed: bool) -> bytes:
    if not require_sealed:
        return b"fake-launcher"
    try:
        raw = pathlib.Path(path).read_bytes()
    except OSError as exc:
        raise ClosedLocalImageError("launcher is unreadable") from exc
    if not raw:
        raise ClosedLocalImageError("launcher is empty")
    return raw


def _request(
    *,
    chain: DevelopmentGenerationChain,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    backend: Optional[sealed.ImageBackend],
) -> sealed.ProductionRequest:
    return sealed.ProductionRequest(
        repository_root=pathlib.Path(repository_root).resolve(),
        artifact_store=artifact_store,
        outputs=outputs,
        scratch=scratch,
        gpgv=pathlib.Path(gpgv),
        zstd=pathlib.Path(zstd),
        launcher=pathlib.Path(launcher),
        launcher_binary=_launcher_bytes(launcher, require_sealed=backend is None),
        chain=chain,
    )


def _set_loop_autoclear(
    device: str,
    *,
    opener: Callable[[str, int], int] = os.open,
    closer: Callable[[int], None] = os.close,
    ioctl: Callable[..., Any] = fcntl.ioctl,
) -> None:
    """Set the kernel autoclear flag without relying on a nonexistent CLI flag."""

    if re.fullmatch(r"/dev/loop[0-9]+", device) is None:
        raise ClosedLocalImageError("loop device name is unsafe")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = opener(device, flags)
    try:
        info = bytearray(LOOP_INFO64_SIZE)
        ioctl(descriptor, LOOP_GET_STATUS64, info, True)
        current = struct.unpack_from("=I", info, LOOP_FLAGS_OFFSET)[0]
        struct.pack_into(
            "=I", info, LOOP_FLAGS_OFFSET, current | LO_FLAGS_AUTOCLEAR
        )
        ioctl(descriptor, LOOP_SET_STATUS64, bytes(info))
    finally:
        closer(descriptor)


class DevelopmentAutoclearReadbackEffects:
    """Use the portable loop ioctl and retain explicit normal-path cleanup."""

    def __init__(
        self,
        readback_module: Any,
        *,
        autoclear_setter: Callable[[str], None] = _set_loop_autoclear,
        expected_module_metadata: Optional[
            Mapping[str, Mapping[str, Any]]
        ] = None,
    ) -> None:
        self._readback_module = readback_module
        self._delegate = readback_module.HostReadbackEffects()
        self._set_autoclear = autoclear_setter
        self._autoclear_devices: set[str] = set()
        self._expected_module_metadata = dict(
            expected_module_metadata
            if expected_module_metadata is not None
            else _module_metadata_identities(_static_test_module_index_entries())
        )

    def unmet_requirements(self) -> list[str]:
        return list(self._delegate.unmet_requirements())

    def setup_loop(self, image: Any) -> str:
        device = self._delegate.setup_loop(image)
        try:
            self._set_autoclear(device)
        except BaseException:
            self._delegate.detach_loop(device)
            raise
        self._autoclear_devices.add(device)
        return device

    def mount(self, device: str, mountpoint: pathlib.Path) -> None:
        self._delegate.mount(device, mountpoint)

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, Any]]:
        tree = dict(self._delegate.read_tree(mountpoint))
        for path in AUTHORITY_MOUNTED_PATHS:
            _require_fixed_directory(
                tree.get(path), "installed authority directory"
            )
        for path in TOOLCHAIN_MOUNTED_PATHS:
            _require_fixed_directory(
                tree.get(path), "installed toolchain directory"
            )
        for material in DEVELOPMENT_REPLAY_MATERIALS:
            path = "/" + material.staging_path
            row = tree.get(path)
            if (
                not isinstance(row, Mapping)
                or row.get("kind") != "file"
                or row.get("mode") != 0o444
                or row.get("uid") != 0
                or row.get("gid") != 0
                or row.get("sha256") != material.sha256
            ):
                raise ClosedLocalImageError(
                    f"closed-local replay material differs: {path}"
                )
        for path in DEVELOPMENT_SYSTEMD_MASK_PATHS:
            mounted = "/" + path
            row = tree.get(mounted)
            if (
                not isinstance(row, Mapping)
                or row.get("kind") != "symlink"
                or row.get("mode") != 0o777
                or row.get("uid") != 0
                or row.get("gid") != 0
                or row.get("target") != DEVELOPMENT_SYSTEMD_MASK_TARGET
            ):
                raise ClosedLocalImageError(
                    f"closed-local systemd mask differs: {mounted}"
                )
        module_load = tree.get("/" + MAC4_MODULE_LOAD_STAGING_PATH)
        if (
            not isinstance(module_load, Mapping)
            or module_load.get("kind") != "file"
            or module_load.get("mode") != 0o444
            or module_load.get("uid") != 0
            or module_load.get("gid") != 0
            or module_load.get("sha256")
            != hashlib.sha256(MAC4_MODULE_LOAD_BYTES).hexdigest()
        ):
            raise ClosedLocalImageError("MAC.4 module load contract differs")
        for path, identity in self._expected_module_metadata.items():
            mounted = "/" + path
            row = tree.get(mounted)
            if (
                not isinstance(row, Mapping)
                or row.get("kind") != "file"
                or row.get("mode") != 0o444
                or row.get("uid") != 0
                or row.get("gid") != 0
                or row.get("sha256") != identity.get("sha256")
            ):
                raise ClosedLocalImageError(
                    f"MAC.4 module metadata differs: {mounted}"
                )
        return tree

    def unmount(self, mountpoint: pathlib.Path) -> None:
        self._delegate.unmount(mountpoint)

    def detach_loop(self, device: str) -> None:
        try:
            self._delegate.detach_loop(device)
        except BaseException as exc:
            error_type = getattr(self._readback_module, "ReadbackV3Error", None)
            expected = (
                "failed: losetup: %s: detach failed: No such device or address"
                % device
            )
            already_autocleared = (
                device in self._autoclear_devices
                and isinstance(error_type, type)
                and isinstance(exc, error_type)
                and str(exc).endswith(expected)
            )
            if not already_autocleared:
                raise
        finally:
            self._autoclear_devices.discard(device)


class DevelopmentRepositoryImageBackend(sealed.RepositoryImageBackend):
    """Scope the runner-compatible readback adapter to this reversible lane."""

    def __init__(
        self,
        *,
        module_loader,
        relay_binary: bytes,
        module_index_generator: Callable[
            [Mapping[str, Mapping[str, Any]]], dict[str, dict[str, Any]]
        ],
    ):
        super().__init__(module_loader=module_loader)
        self._relay_binary = relay_binary
        self._module_index_generator = module_index_generator

    def prepare(self, request):
        repository_root = getattr(request, "repository_root", REPOSITORY_ROOT)
        with _development_fixed_directory_contract(
            repository_root, self._relay_binary, self._module_index_generator
        ):
            prepared = super().prepare(request)
        identities = getattr(self._module_index_generator, "identities", None)
        self._module_metadata = (
            identities()
            if callable(identities)
            else _module_metadata_identities(
                self._module_index_generator({})
            )
        )
        return prepared

    def readback(self, repository_root, outputs, chain):
        historical = sealed.AutoclearReadbackEffects
        sealed.AutoclearReadbackEffects = lambda module: DevelopmentAutoclearReadbackEffects(
            module, expected_module_metadata=self._module_metadata
        )
        try:
            return super().readback(repository_root, outputs, chain)
        finally:
            sealed.AutoclearReadbackEffects = historical


def _development_backend(
    relay_binary: bytes = b"fake-relay",
    *,
    depmod: Optional[pathlib.Path] = None,
    scratch: Optional[pathlib.Path] = None,
    module_index_generator: Optional[
        Callable[[Mapping[str, Mapping[str, Any]]], dict[str, dict[str, Any]]]
    ] = None,
) -> sealed.RepositoryImageBackend:
    # The production loader correctly refuses every repository module that is
    # not named by the historical F7 fingerprint.  This new orchestrator is
    # intentionally outside that old production fingerprint, so the
    # reversible lane uses the backend's injected-loader seam.  The workflow
    # supplies a root-owned, non-writable checkout, while
    # verify_development_generation_chain hashes the complete bound import
    # closure before this loader is reached.
    if module_index_generator is None:
        if depmod is None:
            module_index_generator = _static_test_module_index_entries
        else:
            if scratch is None:
                raise ClosedLocalImageError("depmod generation needs scratch")
            module_index_generator = HostDepmodModuleIndexGenerator(
                depmod=depmod, scratch=scratch
            )
    return DevelopmentRepositoryImageBackend(
        module_loader=importlib.import_module,
        relay_binary=relay_binary,
        module_index_generator=module_index_generator,
    )


def _mac4_relay_bytes(
    path: Optional[pathlib.Path], *, require_real: bool
) -> bytes:
    if not require_real:
        return b"fake-relay"
    if path is None:
        raise ClosedLocalImageError("real image backend requires --mac4-relay")
    candidate = pathlib.Path(path)
    try:
        resolved = candidate.resolve(strict=True)
        raw = candidate.read_bytes()
    except OSError as exc:
        raise ClosedLocalImageError("MAC.4 relay binary is unreadable") from exc
    if resolved != candidate or not candidate.is_file() or candidate.is_symlink():
        raise ClosedLocalImageError("MAC.4 relay binary is not one regular file")
    if len(raw) < 64 or raw[:4] != b"\x7fELF" or raw[4:6] != b"\x02\x01":
        raise ClosedLocalImageError("MAC.4 relay is not one 64-bit little-endian ELF")
    if int.from_bytes(raw[18:20], "little") != 183:
        raise ClosedLocalImageError("MAC.4 relay ELF is not aarch64")
    return raw


def preflight(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    mac4_relay: Optional[pathlib.Path] = None,
    depmod: Optional[pathlib.Path] = None,
    backend: Optional[sealed.ImageBackend] = None,
) -> dict[str, Any]:
    chain = verify_development_generation_chain(repository_root)
    store = pathlib.Path(artifact_store).resolve(strict=True)
    if not store.is_dir():
        raise ClosedLocalImageError("artifact store is not a directory")
    scratch_root = _real_empty_directory(scratch, "preflight scratch")
    if os.path.lexists(outputs):
        raise ClosedLocalImageError("preflight output path already exists")
    request = _request(
        chain=chain,
        repository_root=repository_root,
        artifact_store=store,
        outputs=pathlib.Path(outputs),
        scratch=scratch_root,
        gpgv=gpgv,
        zstd=zstd,
        launcher=launcher,
        backend=backend,
    )
    relay_binary = _mac4_relay_bytes(mac4_relay, require_real=backend is None)
    selected = (
        _development_backend(
            relay_binary, depmod=depmod, scratch=scratch_root
        )
        if backend is None
        else backend
    )
    prepared = selected.prepare(request)
    if not isinstance(prepared, sealed.PreparedProduction):
        raise ClosedLocalImageError("image backend returned no prepared staging")
    if os.path.lexists(outputs):
        raise ClosedLocalImageError("preflight created an output path")
    document = {
        "artifactClass": ARTIFACT_CLASS,
        "authorisations": _authorisations(),
        "claims": _claims(),
        "effects": {"imagesCreated": 0, "machinesStarted": 0},
        "measurement": dict(prepared.measurement),
        "schema": SCHEMA,
        "status": PREFLIGHT_STATUS,
    }
    metadata = getattr(selected, "_module_metadata", None)
    if isinstance(metadata, Mapping):
        document["mac4ModuleMetadata"] = dict(metadata)
    return document


def _output_manifest(outputs: pathlib.Path) -> list[dict[str, Any]]:
    rows = []
    for name in sealed.OUTPUT_NAMES:
        identity, _ = sealed._read_regular(outputs, name)
        rows.append(identity.record())
    return rows


def _publish_result(path: pathlib.Path, document: Mapping[str, Any]) -> None:
    raw = sealed.canonical_json(document)
    target = pathlib.Path(path)
    if os.path.lexists(target):
        raise ClosedLocalImageError("closed-local result already exists")
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name(target.name + ".partial")
    if os.path.lexists(temporary):
        raise ClosedLocalImageError("closed-local result partial already exists")
    try:
        with temporary.open("xb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, target)
    except BaseException:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
        raise


def build(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    result: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    run_label: str,
    mac4_relay: Optional[pathlib.Path] = None,
    depmod: Optional[pathlib.Path] = None,
    backend: Optional[sealed.ImageBackend] = None,
) -> dict[str, Any]:
    if re.fullmatch(RUN_LABEL_PATTERN, run_label) is None:
        raise ClosedLocalImageError("development run label is invalid")
    chain = verify_development_generation_chain(repository_root)
    if backend is None and (os.geteuid() != 0 or os.getegid() != 0):
        raise ClosedLocalImageError("real image backend requires root:root")
    store = pathlib.Path(artifact_store).resolve(strict=True)
    if not store.is_dir():
        raise ClosedLocalImageError("artifact store is not a directory")
    scratch_root = _real_empty_directory(scratch, "build scratch")
    output_root = pathlib.Path(outputs)
    if os.path.lexists(output_root):
        raise ClosedLocalImageError("fresh closed-local output path already exists")
    request = _request(
        chain=chain,
        repository_root=repository_root,
        artifact_store=store,
        outputs=output_root,
        scratch=scratch_root,
        gpgv=gpgv,
        zstd=zstd,
        launcher=launcher,
        backend=backend,
    )
    relay_binary = _mac4_relay_bytes(mac4_relay, require_real=backend is None)
    selected = (
        _development_backend(
            relay_binary, depmod=depmod, scratch=scratch_root
        )
        if backend is None
        else backend
    )
    prepared = selected.prepare(request)
    if not isinstance(prepared, sealed.PreparedProduction):
        raise ClosedLocalImageError("image backend returned no prepared staging")
    output_root.mkdir(mode=0o700)
    kernel = selected.extract_kernel(request, prepared)
    initrd = selected.build_initrd(request, prepared)
    if not isinstance(initrd, bytes) or not initrd:
        raise ClosedLocalImageError("initrd builder returned no bytes")
    sealed._publish_bytes_once(output_root / "guest-initrd", initrd)
    root_disk = selected.build_root_disk(request, prepared)
    verification = selected.verify_images(
        request, prepared, kernel, initrd, root_disk
    )
    if not isinstance(verification, Mapping) or verification.get("passed") is not True:
        raise ClosedLocalImageError("produced image verification did not pass")
    sealed._sync_produced_images(output_root)
    readback = selected.readback(repository_root, output_root, chain)
    if (
        not isinstance(readback, Mapping)
        or readback.get("status") != sealed.READBACK_PASS_STATUS
        or readback.get("mayEnterQualification") is not True
        or readback.get("qualifiedForReplicaComparison") is not True
    ):
        raise ClosedLocalImageError("root-disk readback did not pass")
    manifest = _output_manifest(output_root)
    document = {
        "artifactClass": ARTIFACT_CLASS,
        "authorisations": _authorisations(),
        "buildReceipt": dict(prepared.build_receipt),
        "claims": _claims(),
        "effects": {"imagesCreated": 1, "machinesStarted": 0},
        "generation": {
            name: chain.identities[name].record() for name in ("P4", "R3", "F7")
        },
        "manifest": manifest,
        "measurement": dict(prepared.measurement),
        "readback": dict(readback),
        "runLabel": run_label,
        "schema": SCHEMA,
        "status": BUILD_STATUS,
        "verification": dict(verification),
    }
    metadata = getattr(selected, "_module_metadata", None)
    if isinstance(metadata, Mapping):
        document["mac4ModuleMetadata"] = dict(metadata)
    _publish_result(result, document)
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    child = parser.add_subparsers(dest="mode", required=True)
    for mode in ("preflight", "build"):
        command = child.add_parser(mode)
        command.add_argument(
            "--repository-root", type=pathlib.Path, default=pathlib.Path.cwd()
        )
        command.add_argument("--cas", type=pathlib.Path, required=True)
        command.add_argument("--outputs", type=pathlib.Path, required=True)
        command.add_argument("--scratch", type=pathlib.Path, required=True)
        command.add_argument("--gpgv", type=pathlib.Path, required=True)
        command.add_argument("--zstd", type=pathlib.Path, required=True)
        command.add_argument("--launcher", type=pathlib.Path, required=True)
        command.add_argument("--mac4-relay", type=pathlib.Path, required=True)
        command.add_argument("--depmod", type=pathlib.Path, required=True)
        command.add_argument("--result", type=pathlib.Path, required=True)
        if mode == "build":
            command.add_argument("--run-label", required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    common = {
        "repository_root": options.repository_root,
        "artifact_store": options.cas,
        "outputs": options.outputs,
        "scratch": options.scratch,
        "gpgv": options.gpgv,
        "zstd": options.zstd,
        "launcher": options.launcher,
        "mac4_relay": options.mac4_relay,
        "depmod": options.depmod,
    }
    try:
        if options.mode == "preflight":
            document = preflight(**common)
        else:
            document = build(
                result=options.result,
                run_label=options.run_label,
                **common,
            )
        if options.mode == "preflight":
            _publish_result(options.result, document)
        print(
            "native-shadow closed-local image-to-readiness: "
            f"{options.mode} PASS"
        )
        return 0
    except (ClosedLocalImageError, sealed.SuccessorProduceV5Error, OSError) as exc:
        print(
            f"native-shadow closed-local image-to-readiness: FAIL: {exc}",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
