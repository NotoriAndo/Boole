#!/usr/bin/env python3
"""Read a launcher-v2 successor root disk against its own sealed generation."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
import pathlib
import platform
import re
import stat
import subprocess
import sys
from dataclasses import dataclass
from typing import Any, Optional, Protocol

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as base_reader


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]

PREREGISTRATION_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
PREREGISTRATION_SHA256 = (
    "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec"
)
IMPORT_CORRECTION_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json"
)
IMPORT_CORRECTION_SHA256 = (
    "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498"
)
SOURCE_LOCK_PATH = (
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)
SOURCE_LOCK_SHA256 = (
    "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"
)
LAUNCHER_RESULT_PATH = (
    "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
)
LAUNCHER_RESULT_SHA256 = (
    "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08"
)
LAUNCHER_SHA256 = "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"
LAUNCHER_SIZE_BYTES = 2_025_192
LAUNCHER_GUEST_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"
MOUNT_OPTIONS = ("ro", "nodev", "noexec", "nosuid")
FILESYSTEM = "ext4"
LOSETUP = "/usr/sbin/losetup"
MOUNT = "/usr/bin/mount"
UMOUNT = "/usr/bin/umount"
HOST_COMMAND_TIMEOUT_SECONDS = 120
RESULT_NAME = "SUCCESSOR-ROOT-DISK-READBACK-V3.json"
QUALIFIED_PENDING_NAME = ".SUCCESSOR-ROOT-DISK-READBACK-V3.pending"
UNQUALIFIED_NAME = "UNQUALIFIED-DIAGNOSTIC.json"
SCHEMA = "boole.native-shadow.successor-root-disk-readback.arm64.v3"
RELEASE = "NATIVE-SHADOW-SUCCESSOR-ROOT-DISK-READBACK-ARM64-V3"
PASS_STATUS = "READBACK-V3-PASS-QUALIFIED-FOR-REPLICA-COMPARISON"
FAILURE_STATUS = "UNQUALIFIED-DIAGNOSTIC"

OUTPUT_FILES = {
    "kernel": "guest-kernel",
    "initrd": "guest-initrd",
    "root-disk": "guest-root-disk",
}
READBACK_CONTRACT_TRUE_KEYS = (
    "failureCannotEnterQualifiedComparison",
    "fallbackToV1Forbidden",
    "qualificationRequiresReadbackPass",
    "v1LauncherMustBeRejected",
    "wrapperCallsOnlyReadbackV3",
)


class ReadbackV3Error(RuntimeError):
    """The readback contract or the image being read does not qualify."""


class CleanupHardStop(ReadbackV3Error):
    """A mounted filesystem or its loop device could not be cleaned up."""


class ReadbackEffects(Protocol):
    """The narrow host effect surface; repository bindings are not injectable."""

    def unmet_requirements(self) -> list[str]: ...

    def setup_loop(self, image: "PinnedFile") -> str: ...

    def mount(self, device: str, mountpoint: pathlib.Path) -> None: ...

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, Any]]: ...

    def unmount(self, mountpoint: pathlib.Path) -> None: ...

    def detach_loop(self, device: str) -> None: ...


def _run(argv: list[str], *, pass_fds: tuple[int, ...] = ()) -> bytes:
    if not argv or not pathlib.Path(argv[0]).is_absolute():
        raise ReadbackV3Error("host command must use an absolute executable path")
    try:
        completed = subprocess.run(
            argv,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            pass_fds=pass_fds,
            timeout=HOST_COMMAND_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        raise ReadbackV3Error(
            f"{argv[0]} exceeded {HOST_COMMAND_TIMEOUT_SECONDS} seconds"
        ) from exc
    except OSError as exc:
        raise ReadbackV3Error(f"host command cannot be executed: {exc}") from exc
    if completed.returncode != 0:
        detail = completed.stderr.decode(errors="replace").strip()
        raise ReadbackV3Error(f"{argv[0]} failed: {detail}")
    return completed.stdout


class HostReadbackEffects:
    """Linux loop/mount effects, kept outside the generation-binding logic."""

    def unmet_requirements(self) -> list[str]:
        unmet = []
        if platform.system() != "Linux":
            unmet.append("Linux, whose ext4 driver reads this filesystem")
        if os.geteuid() != 0:
            unmet.append("root, which is required for loop setup and mounting")
        return unmet

    def setup_loop(self, image: "PinnedFile") -> str:
        procfd = f"/proc/self/fd/{image.descriptor}"
        output = _run(
            [LOSETUP, "--find", "--show", "--read-only", procfd],
            pass_fds=(image.descriptor,),
        ).decode("utf-8", errors="strict").strip()
        if re.fullmatch(r"/dev/loop[0-9]+", output) is None:
            raise ReadbackV3Error(f"losetup returned an unsafe loop name: {output!r}")
        return output

    def mount(self, device: str, mountpoint: pathlib.Path) -> None:
        _run(
            [
                MOUNT,
                "-t",
                FILESYSTEM,
                "-o",
                ",".join(MOUNT_OPTIONS),
                device,
                str(mountpoint),
            ]
        )

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, Any]]:
        return base_reader.tree_from_directory(mountpoint)

    def unmount(self, mountpoint: pathlib.Path) -> None:
        _run([UMOUNT, str(mountpoint)])

    def detach_loop(self, device: str) -> None:
        _run([LOSETUP, "--detach", device])


@dataclass(frozen=True)
class RepositoryBindings:
    preregistration: dict[str, Any]
    import_closure_correction: dict[str, Any]
    source_lock: dict[str, Any]
    launcher_result: dict[str, Any]
    launcher_sha256: str
    launcher_size_bytes: int


@dataclass(frozen=True)
class FileIdentity:
    device: int
    inode: int
    mode: int
    uid: int
    gid: int
    mtime_ns: int
    ctime_ns: int
    sha256: str
    size_bytes: int


@dataclass(frozen=True)
class PinnedFile:
    """One open output inode retained until the readback verdict is durable."""

    descriptor: int
    identity: FileIdentity
    path: pathlib.Path
    role: str


def repository_bindings() -> dict[str, tuple[str, str]]:
    """The only repository bytes this generation accepts as its baselines."""

    return {
        "preregistration": (PREREGISTRATION_PATH, PREREGISTRATION_SHA256),
        "import-closure-correction": (
            IMPORT_CORRECTION_PATH,
            IMPORT_CORRECTION_SHA256,
        ),
        "source-lock-v2": (SOURCE_LOCK_PATH, SOURCE_LOCK_SHA256),
        "launcher-result-v2": (LAUNCHER_RESULT_PATH, LAUNCHER_RESULT_SHA256),
    }


def _read_regular_bytes(path: pathlib.Path) -> bytes:
    try:
        metadata = path.lstat()
    except OSError as exc:
        raise ReadbackV3Error(f"a fixed repository binding is unreadable: {path}") from exc
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        raise ReadbackV3Error(f"a fixed repository binding is not a regular file: {path}")
    try:
        return path.read_bytes()
    except OSError as exc:
        raise ReadbackV3Error(f"a fixed repository binding is unreadable: {path}") from exc


def _bound_json(relative: str, expected_sha256: str) -> dict[str, Any]:
    path = REPOSITORY_ROOT / relative
    raw = _read_regular_bytes(path)
    found = hashlib.sha256(raw).hexdigest()
    if found != expected_sha256:
        raise ReadbackV3Error(
            f"{relative} digest is {found}, not the fixed {expected_sha256}"
        )
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ReadbackV3Error(f"{relative} is not canonical JSON input") from exc
    if not isinstance(document, dict):
        raise ReadbackV3Error(f"{relative} is not a JSON object")
    return document


def _validate_effective_bindings(
    preregistration: dict[str, Any], correction: dict[str, Any]
) -> None:
    """Require the exact 23+18 live direct union, not only its JSON rows."""

    predecessor = preregistration.get("bindings")
    added = correction.get("addedBindings")
    if not isinstance(predecessor, list) or len(predecessor) != 23:
        raise ReadbackV3Error(
            "the preregistration must contain exactly twenty-three bindings"
        )
    if not isinstance(added, list) or len(added) != 18:
        raise ReadbackV3Error(
            "the import-closure correction must contain exactly eighteen bindings"
        )
    rows = predecessor + added
    root = REPOSITORY_ROOT.resolve()
    seen: set[str] = set()
    for row in rows:
        if not isinstance(row, dict) or set(row) != {
            "path",
            "role",
            "sha256",
            "sizeBytes",
        }:
            raise ReadbackV3Error("a preregistered binding has an invalid shape")
        if not isinstance(row.get("role"), str) or not row["role"]:
            raise ReadbackV3Error("a preregistered binding has no role")
        relative = row.get("path")
        pure = pathlib.PurePosixPath(relative) if isinstance(relative, str) else None
        if (
            pure is None
            or pure.is_absolute()
            or ".." in pure.parts
            or pure.as_posix() != relative
            or relative in seen
        ):
            raise ReadbackV3Error(
                "a preregistered binding path is unsafe or repeated"
            )
        seen.add(relative)
        path = root.joinpath(*pure.parts)
        raw = _read_regular_bytes(path)
        try:
            path.resolve().relative_to(root)
        except ValueError as exc:
            raise ReadbackV3Error(
                f"a preregistered binding leaves the repository: {relative}"
            ) from exc
        if type(row.get("sizeBytes")) is not int or len(raw) != row["sizeBytes"]:
            raise ReadbackV3Error(
                f"a preregistered binding size differs: {relative}"
            )
        if hashlib.sha256(raw).hexdigest() != row.get("sha256"):
            raise ReadbackV3Error(
                f"a preregistered binding digest differs: {relative}"
            )
    if len(seen) != 41:
        raise ReadbackV3Error(
            "the effective repository binding union is not exactly forty-one"
        )


def _exact_bool(document: dict[str, Any], key: str, expected: bool, where: str) -> None:
    value = document.get(key)
    if type(value) is not bool:  # bool is an int subclass; the distinction matters here.
        raise ReadbackV3Error(f"{where}.{key} must be a boolean")
    if value is not expected:
        raise ReadbackV3Error(f"{where}.{key} must be {expected!r}")


def _exact_int(document: dict[str, Any], key: str, expected: int, where: str) -> None:
    value = document.get(key)
    if type(value) is not int:
        raise ReadbackV3Error(f"{where}.{key} must be an integer")
    if value != expected:
        raise ReadbackV3Error(f"{where}.{key} must be {expected}")


def _validate_preregistration(document: dict[str, Any]) -> None:
    if document.get("schema") != (
        "boole.native-shadow.mac3.launcher-v2-successor-producer-"
        "preregistration.arm64.v1"
    ):
        raise ReadbackV3Error("the preregistration schema is not launcher-v2 v1")
    if document.get("status") != "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY":
        raise ReadbackV3Error("the preregistration does not carry authority-zero status")

    authorisations = document.get("authorisations")
    runs = document.get("runs")
    generation = document.get("futureGeneration")
    if not isinstance(authorisations, dict) or not isinstance(runs, dict):
        raise ReadbackV3Error("the preregistration authority and run ledgers are absent")
    if not isinstance(generation, dict):
        raise ReadbackV3Error("the preregistration future generation is absent")
    for key in (
        "bootAuthorised",
        "consensusActivated",
        "imageProductionAuthorised",
        "mac4Started",
        "miningActivated",
        "p2pActivated",
        "rewardActivated",
        "testnetStarted",
    ):
        _exact_bool(authorisations, key, False, "authorisations")
    _exact_int(authorisations, "imageProductionRunsAllowed", 0, "authorisations")
    for key in (
        "bootsAllowed",
        "bootsPerformed",
        "freeRehearsalsPerformedByThisRecord",
        "imageProductionsAllowed",
        "imageProductionsPerformed",
    ):
        _exact_int(runs, key, 0, "runs")
    _exact_bool(generation, "implementedByThisRecord", False, "futureGeneration")
    _exact_bool(generation, "newGenerationFilesOnly", True, "futureGeneration")

    contract = generation.get("readbackV3Contract")
    if not isinstance(contract, dict):
        raise ReadbackV3Error("the readback-v3 contract is absent")
    if contract.get("bindingOverridesForbidden") != [
        "cli",
        "environment",
        "image-provided-values",
    ]:
        raise ReadbackV3Error("the readback-v3 binding override refusal changed")
    if contract.get("failureArtifactClass") != "UNQUALIFIED-DIAGNOSTIC":
        raise ReadbackV3Error("the readback-v3 failure artifact class changed")
    for key in READBACK_CONTRACT_TRUE_KEYS:
        _exact_bool(contract, key, True, "readbackV3Contract")
    mount_policy = contract.get("mountPolicy")
    cleanup_policy = contract.get("cleanupPolicy")
    if not isinstance(mount_policy, dict) or not isinstance(cleanup_policy, dict):
        raise ReadbackV3Error("the readback-v3 mount or cleanup policy is absent")
    for key in ("nodev", "noexec", "nosuid", "readOnly"):
        _exact_bool(mount_policy, key, True, "readbackV3Contract.mountPolicy")
    for key in ("cleanupFailureIsHardStop", "loopDeviceDetached", "unmountInFinally"):
        _exact_bool(cleanup_policy, key, True, "readbackV3Contract.cleanupPolicy")

    required = contract.get("requiredBindings")
    expected = [
        {"path": SOURCE_LOCK_PATH, "sha256": SOURCE_LOCK_SHA256},
        {"path": LAUNCHER_RESULT_PATH, "sha256": LAUNCHER_RESULT_SHA256},
    ]
    if required != expected:
        raise ReadbackV3Error("the readback-v3 required bindings changed")


def _validate_import_closure_correction(
    document: dict[str, Any], preregistration: dict[str, Any]
) -> None:
    if document.get("schema") != (
        "boole.native-shadow.mac3.launcher-v2-successor-producer-"
        "import-closure-correction.arm64.v1"
    ):
        raise ReadbackV3Error("the import-closure correction schema differs")
    if document.get("status") != (
        "CORRECTED-BEFORE-REHEARSAL-NO-IMAGE-PRODUCTION-AUTHORITY"
    ):
        raise ReadbackV3Error("the import-closure correction status differs")
    expected_predecessor = {
        "bindingCount": 23,
        "path": PREREGISTRATION_PATH,
        "preservedByteUnchanged": True,
        "sha256": PREREGISTRATION_SHA256,
        "sizeBytes": 20145,
    }
    if document.get("predecessor") != expected_predecessor:
        raise ReadbackV3Error("the import-closure correction predecessor differs")
    if document.get("authorisations") != preregistration.get("authorisations"):
        raise ReadbackV3Error("the import-closure correction authority differs")
    if document.get("runs") != preregistration.get("runs"):
        raise ReadbackV3Error("the import-closure correction run ledger differs")
    if document.get("effectiveBinding") != {
        "addedMissingBindings": 18,
        "bindingVerificationBeforeRepositoryPythonImport": True,
        "effectiveUniqueBindings": 41,
        "predecessorBindings": 23,
        "unionRequired": True,
    }:
        raise ReadbackV3Error("the import-closure correction effective union differs")
    if document.get("grantsAuthority") is not False:
        raise ReadbackV3Error("the import-closure correction grants authority")
    if document.get("futureFingerprintMustBindBothRecords") is not True:
        raise ReadbackV3Error("the import-closure correction lost two-record binding")


def _validate_source_lock(document: dict[str, Any]) -> None:
    if document.get("release") != (
        "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V2-SOURCE-SHAPE-ONLY-"
        "NOT-BOOTABLE"
    ):
        raise ReadbackV3Error("the fixed source lock is not source-lock v2")
    _exact_bool(document, "activationAllowed", False, "sourceLockV2")


def _validate_launcher_result(document: dict[str, Any]) -> None:
    if document.get("schema") != "boole.native-shadow.launcher-build-result.arm64.v2":
        raise ReadbackV3Error("the fixed launcher result is not launcher v2")
    if document.get("release") != "NATIVE-SHADOW-LAUNCHER-BUILD-ARM64-V2":
        raise ReadbackV3Error("the fixed launcher result is not launcher v2")
    _exact_bool(document, "activationAllowed", False, "launcherResultV2")
    _exact_bool(document, "bootableClaim", False, "launcherResultV2")
    _exact_int(document, "independentBuildCount", 2, "launcherResultV2")
    launcher = document.get("launcher")
    if not isinstance(launcher, dict):
        raise ReadbackV3Error("the launcher-v2 result has no launcher record")
    if launcher.get("sha256") != LAUNCHER_SHA256:
        raise ReadbackV3Error("the launcher-v2 executable digest changed")
    if launcher.get("guestLogicalPath") != LAUNCHER_GUEST_PATH:
        raise ReadbackV3Error("the launcher-v2 guest path changed")
    _exact_int(launcher, "sizeBytes", LAUNCHER_SIZE_BYTES, "launcherResultV2.launcher")


def load_repository_bindings() -> RepositoryBindings:
    """Read and validate every fixed generation byte before any image effect."""

    preregistration = _bound_json(PREREGISTRATION_PATH, PREREGISTRATION_SHA256)
    correction = _bound_json(IMPORT_CORRECTION_PATH, IMPORT_CORRECTION_SHA256)
    source_lock = _bound_json(SOURCE_LOCK_PATH, SOURCE_LOCK_SHA256)
    launcher_result = _bound_json(LAUNCHER_RESULT_PATH, LAUNCHER_RESULT_SHA256)
    _validate_preregistration(preregistration)
    _validate_import_closure_correction(correction, preregistration)
    _validate_effective_bindings(preregistration, correction)
    _validate_source_lock(source_lock)
    _validate_launcher_result(launcher_result)
    return RepositoryBindings(
        preregistration=preregistration,
        import_closure_correction=correction,
        source_lock=source_lock,
        launcher_result=launcher_result,
        launcher_sha256=LAUNCHER_SHA256,
        launcher_size_bytes=LAUNCHER_SIZE_BYTES,
    )


def output_paths(outputs: pathlib.Path) -> dict[str, pathlib.Path]:
    """The new generation's three fixed output names, without old authority code."""

    root = pathlib.Path(outputs)
    return {role: root / name for role, name in OUTPUT_FILES.items()}


def _real_directory(path: pathlib.Path, context: str) -> pathlib.Path:
    path = pathlib.Path(path)
    try:
        metadata = path.lstat()
    except OSError as exc:
        raise ReadbackV3Error(f"the {context} is absent or unreadable: {path}") from exc
    if not stat.S_ISDIR(metadata.st_mode) or path.is_symlink():
        raise ReadbackV3Error(
            f"the {context} is not a real non-symlink directory: {path}"
        )
    return path


def _open_output_descriptor(path: pathlib.Path, role: str) -> int:
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        return os.open(str(path), flags)
    except OSError as exc:
        raise ReadbackV3Error(
            f"the {role} output is not a regular file (non-symlink required) or is unreadable: {path}"
        ) from exc


def _descriptor_metadata(metadata: os.stat_result) -> tuple[int, ...]:
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def _identity_from_descriptor(
    descriptor: int, path: pathlib.Path, role: str
) -> FileIdentity:
    """Hash one open inode and require the public path still names that inode."""

    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ReadbackV3Error(
                f"the {role} output is not a regular file: {path}"
            )
        found = hashlib.sha256()
        offset = 0
        while True:
            block = os.pread(descriptor, 1 << 20, offset)
            if not block:
                break
            found.update(block)
            offset += len(block)
        after = os.fstat(descriptor)
        if _descriptor_metadata(before) != _descriptor_metadata(after):
            raise ReadbackV3Error(f"the {role} output changed while it was hashed")
        path_info = path.lstat()
        if path.is_symlink() or not stat.S_ISREG(path_info.st_mode):
            raise ReadbackV3Error(
                f"the {role} output is not a regular non-symlink file: {path}"
            )
        if _descriptor_metadata(path_info) != _descriptor_metadata(after):
            raise ReadbackV3Error(
                f"the {role} output path changed while it was hashed"
            )
        return FileIdentity(
            device=after.st_dev,
            inode=after.st_ino,
            mode=after.st_mode,
            uid=after.st_uid,
            gid=after.st_gid,
            mtime_ns=after.st_mtime_ns,
            ctime_ns=after.st_ctime_ns,
            sha256=found.hexdigest(),
            size_bytes=after.st_size,
        )
    except OSError as exc:
        raise ReadbackV3Error(f"the {role} output cannot be read safely") from exc


@contextlib.contextmanager
def _pin_output(path: pathlib.Path, role: str):
    """Keep the exact verified inode open across loop setup and verification."""

    descriptor = _open_output_descriptor(path, role)
    try:
        yield PinnedFile(
            descriptor=descriptor,
            identity=_identity_from_descriptor(descriptor, path, role),
            path=path,
            role=role,
        )
    finally:
        os.close(descriptor)


def _file_identity(path: pathlib.Path, role: str) -> FileIdentity:
    """Compatibility helper for one-shot callers; verification keeps pins open."""

    with _pin_output(path, role) as pinned:
        return pinned.identity


def _require_outputs_unchanged(
    pinned: dict[str, PinnedFile],
    phase: str,
) -> None:
    """Re-hash all three retained inodes and require paths still name them."""

    for role, output in pinned.items():
        observed = _identity_from_descriptor(output.descriptor, output.path, role)
        if observed != output.identity:
            raise ReadbackV3Error(
                f"{role} identity or digest changed {phase}"
            )


def _read_regular_output(output: PinnedFile) -> bytes:
    raw = bytearray()
    offset = 0
    try:
        while True:
            block = os.pread(output.descriptor, 1 << 20, offset)
            if not block:
                break
            raw.extend(block)
            offset += len(block)
    except OSError as exc:
        raise ReadbackV3Error(f"the {output.role} output cannot be read") from exc
    if (
        len(raw) != output.identity.size_bytes
        or hashlib.sha256(raw).hexdigest() != output.identity.sha256
    ):
        raise ReadbackV3Error(
            f"the {output.role} output changed while it was read"
        )
    return bytes(raw)


def _cleanup(
    *,
    effects: ReadbackEffects,
    mountpoint: pathlib.Path,
    mount_attempted: bool,
    device: Optional[str],
    primary: Optional[BaseException],
) -> None:
    failures: list[str] = []
    if mount_attempted:
        try:
            effects.unmount(mountpoint)
        except BaseException as exc:
            failures.append(f"unmount failed: {exc}")
    if device is not None:
        try:
            effects.detach_loop(device)
        except BaseException as exc:
            failures.append(f"loop detach failed: {exc}")
    if failures:
        message = "; ".join(failures)
        if primary is not None:
            message += f"; original readback failure: {primary}"
        raise CleanupHardStop(message) from primary


def _read_mounted_tree(
    *, image: PinnedFile, mountpoint: pathlib.Path, effects: ReadbackEffects
) -> dict[str, dict[str, Any]]:
    """Attach, mount, read, and clean; cleanup failure always wins as a hard stop."""

    device: Optional[str] = None
    mount_attempted = False
    primary: Optional[BaseException] = None
    tree: Optional[dict[str, dict[str, Any]]] = None
    try:
        device = effects.setup_loop(image)
        mount_attempted = True
        effects.mount(device, mountpoint)
        tree = effects.read_tree(mountpoint)
    except BaseException as exc:
        primary = exc
    finally:
        _cleanup(
            effects=effects,
            mountpoint=mountpoint,
            mount_attempted=mount_attempted,
            device=device,
            primary=primary,
        )
    if primary is not None:
        raise primary
    if tree is None:
        raise ReadbackV3Error("the mounted tree reader returned no tree")
    return tree


def _canonical_json(value: Any) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _stream_digest(path: pathlib.Path) -> str:
    found = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            found.update(block)
    return found.hexdigest()


def _require_absent(path: pathlib.Path, context: str) -> None:
    try:
        path.lstat()
    except FileNotFoundError:
        return
    except OSError as exc:
        raise ReadbackV3Error(f"the {context} path cannot be inspected") from exc
    raise ReadbackV3Error(
        f"the {context} already exists and is never overwritten: {path}"
    )


def _write_document_once(path: pathlib.Path, document: dict[str, Any]) -> None:
    """Create one fixed result without following a symlink or replacing bytes."""

    parent = _real_directory(path.parent, "result parent")
    _require_absent(path, "result")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    raw = _canonical_json(document)
    try:
        descriptor = os.open(str(parent / path.name), flags, 0o444)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
    except OSError as exc:
        raise ReadbackV3Error(
            f"the fixed result cannot be written exactly once: {path}"
        ) from exc


def _remove_private_pending(path: pathlib.Path) -> None:
    try:
        metadata = path.lstat()
        if path.is_symlink() or not stat.S_ISREG(metadata.st_mode):
            raise CleanupHardStop(
                "the qualified-result pending path ceased to be a regular file"
            )
        path.unlink()
    except FileNotFoundError:
        return
    except CleanupHardStop:
        raise
    except OSError as exc:
        raise CleanupHardStop(
            f"qualified-result pending cleanup failed: {exc}"
        ) from exc


def _publish_qualified_result_once(
    *,
    output_root: pathlib.Path,
    document: dict[str, Any],
    pinned: dict[str, PinnedFile],
) -> None:
    """Stage PASS bytes privately, recheck outputs, then publish one hard link."""

    pending = output_root / QUALIFIED_PENDING_NAME
    result = output_root / RESULT_NAME
    _require_absent(pending, "qualified-result pending")
    _write_document_once(pending, document)
    published = False
    try:
        _require_outputs_unchanged(pinned, "while the qualified result was staged")
        try:
            os.link(pending, result, follow_symlinks=False)
            published = True
        except OSError as exc:
            raise ReadbackV3Error(
                f"the qualified result cannot be published exactly once: {result}"
            ) from exc
    finally:
        try:
            _remove_private_pending(pending)
        except CleanupHardStop as cleanup_failure:
            if published:
                try:
                    result.unlink()
                except OSError as rollback_failure:
                    raise CleanupHardStop(
                        "qualified-result pending cleanup and fixed-name rollback "
                        f"both failed: {cleanup_failure}; {rollback_failure}"
                    ) from cleanup_failure
            raise


def _fixed_mountpoint(outputs: pathlib.Path) -> pathlib.Path:
    parent = _real_directory(outputs.parent, "outputs parent")
    mountpoint = parent / "successor-root-disk-readback-v3"
    _require_absent(mountpoint, "fresh mountpoint")
    try:
        os.mkdir(mountpoint, 0o700)
        info = mountpoint.lstat()
    except OSError as exc:
        raise ReadbackV3Error("the fixed mountpoint cannot be created") from exc
    if (
        mountpoint.is_symlink()
        or not stat.S_ISDIR(info.st_mode)
        or stat.S_IMODE(info.st_mode) != 0o700
    ):
        raise ReadbackV3Error("the fixed mountpoint is not a private 0700 directory")
    return mountpoint


def _remove_mountpoint(
    mountpoint: pathlib.Path, primary: Optional[BaseException] = None
) -> None:
    try:
        mountpoint.rmdir()
    except OSError as exc:
        detail = f"fixed mountpoint cleanup failed: {exc}"
        if primary is not None:
            detail += f"; original readback failure: {primary}"
        raise CleanupHardStop(detail) from primary


def _result_document(
    *,
    report: dict[str, Any],
    image: pathlib.Path,
    image_identity: FileIdentity,
    entry_count: int,
    passed: bool,
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "artifactClass": "QUALIFIED-READBACK" if passed else FAILURE_STATUS,
        "bootableClaim": False,
        "entryCount": entry_count,
        "guestBootVerified": False,
        "image": {
            "name": image.name,
            "sha256": image_identity.sha256,
            "sizeBytes": image_identity.size_bytes,
        },
        "importClosureCorrection": {
            "path": IMPORT_CORRECTION_PATH,
            "sha256": IMPORT_CORRECTION_SHA256,
        },
        "launcherResult": {
            "path": LAUNCHER_RESULT_PATH,
            "sha256": LAUNCHER_RESULT_SHA256,
            "launcherSha256": LAUNCHER_SHA256,
        },
        "mayEnterQualification": passed,
        "producerPreregistration": {
            "path": PREREGISTRATION_PATH,
            "sha256": PREREGISTRATION_SHA256,
        },
        "qualifiedForReplicaComparison": passed,
        "release": RELEASE,
        "schema": SCHEMA,
        "sourceLock": {"path": SOURCE_LOCK_PATH, "sha256": SOURCE_LOCK_SHA256},
        "status": PASS_STATUS if passed else FAILURE_STATUS,
        "verification": report,
    }


def _failure_document(
    *,
    image: pathlib.Path,
    image_identity: FileIdentity,
    failure: BaseException,
    stage: str,
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "artifactClass": FAILURE_STATUS,
        "bootableClaim": False,
        "entryCount": 0,
        "failure": f"{type(failure).__name__}: {failure}",
        "failureStage": stage,
        "guestBootVerified": False,
        "image": {
            "name": image.name,
            "sha256": image_identity.sha256,
            "sizeBytes": image_identity.size_bytes,
        },
        "importClosureCorrection": {
            "path": IMPORT_CORRECTION_PATH,
            "sha256": IMPORT_CORRECTION_SHA256,
        },
        "launcherResult": {
            "path": LAUNCHER_RESULT_PATH,
            "sha256": LAUNCHER_RESULT_SHA256,
            "launcherSha256": LAUNCHER_SHA256,
        },
        "mayEnterQualification": False,
        "producerPreregistration": {
            "path": PREREGISTRATION_PATH,
            "sha256": PREREGISTRATION_SHA256,
        },
        "qualifiedForReplicaComparison": False,
        "release": RELEASE,
        "schema": SCHEMA,
        "sourceLock": {"path": SOURCE_LOCK_PATH, "sha256": SOURCE_LOCK_SHA256},
        "status": FAILURE_STATUS,
        "verification": {"checks": [], "passed": False},
    }


def _write_unqualified(outputs: pathlib.Path, document: dict[str, Any]) -> None:
    diagnostic = {
        "artifactClass": FAILURE_STATUS,
        "mayBeAdopted": False,
        "mayBeBooted": False,
        "mayEnterQualification": False,
        "qualifiedForReplicaComparison": False,
        "readback": document,
        "status": FAILURE_STATUS,
    }
    _write_document_once(pathlib.Path(outputs) / UNQUALIFIED_NAME, diagnostic)


def _validate_verification_report(report: Any) -> bool:
    """Accept only the exact seven-check verifier report shape."""

    if not isinstance(report, dict) or set(report) != {
        "activationAllowed",
        "bootableClaim",
        "checks",
        "guestBootVerified",
        "passed",
    }:
        raise ReadbackV3Error("verification report schema differs")
    for key in ("activationAllowed", "bootableClaim", "guestBootVerified"):
        if type(report[key]) is not bool or report[key] is not False:
            raise ReadbackV3Error(
                f"verification {key} must be the literal boolean false"
            )
    if type(report["passed"]) is not bool:
        raise ReadbackV3Error("verification passed must be a literal boolean")
    checks = report["checks"]
    if not isinstance(checks, list) or len(checks) != len(image_verify.REQUIRED_CHECKS):
        raise ReadbackV3Error("verification check count differs")
    observed: list[str] = []
    all_ok = True
    for row in checks:
        if not isinstance(row, dict) or set(row) != {"detail", "id", "ok"}:
            raise ReadbackV3Error("verification check schema differs")
        if not isinstance(row["detail"], str) or not isinstance(row["id"], str):
            raise ReadbackV3Error("verification check text fields differ")
        if type(row["ok"]) is not bool:
            raise ReadbackV3Error("verification check ok must be a literal boolean")
        observed.append(row["id"])
        all_ok = all_ok and row["ok"]
    if observed != sorted(image_verify.REQUIRED_CHECKS):
        raise ReadbackV3Error("verification check identifiers differ")
    if report["passed"] is not all_ok:
        raise ReadbackV3Error("verification passed disagrees with its checks")
    return report["passed"]


def assert_qualified_for_replica_comparison(
    document: dict[str, Any],
    *,
    expected_image: FileIdentity,
    expected_entry_count: int,
) -> None:
    """Require the exact result and its independently observed image identity."""

    expected_keys = {
        "activationAllowed",
        "artifactClass",
        "bootableClaim",
        "entryCount",
        "guestBootVerified",
        "image",
        "importClosureCorrection",
        "launcherResult",
        "mayEnterQualification",
        "producerPreregistration",
        "qualifiedForReplicaComparison",
        "release",
        "schema",
        "sourceLock",
        "status",
        "verification",
    }
    if not isinstance(document, dict) or set(document) != expected_keys:
        raise ReadbackV3Error("readback promotion document shape differs")
    if document.get("schema") != SCHEMA or document.get("release") != RELEASE:
        raise ReadbackV3Error("readback promotion schema or release differs")
    for key in ("activationAllowed", "bootableClaim", "guestBootVerified"):
        if type(document[key]) is not bool or document[key] is not False:
            raise ReadbackV3Error(f"readback promotion {key} must be false")
    if document.get("status") != PASS_STATUS:
        raise ReadbackV3Error("readback status is not the v3 passing status")
    if document.get("artifactClass") != "QUALIFIED-READBACK":
        raise ReadbackV3Error("readback artifact class is not qualified")
    if type(document.get("mayEnterQualification")) is not bool or document.get(
        "mayEnterQualification"
    ) is not True:
        raise ReadbackV3Error("readback is barred from qualification")
    if type(document.get("qualifiedForReplicaComparison")) is not bool or document.get(
        "qualifiedForReplicaComparison"
    ) is not True:
        raise ReadbackV3Error("readback is barred from replica comparison")
    if type(expected_entry_count) is not int or expected_entry_count < 0:
        raise ReadbackV3Error("expected entry count is invalid")
    if type(document.get("entryCount")) is not int or document.get(
        "entryCount"
    ) != expected_entry_count:
        raise ReadbackV3Error("readback entry count differs")
    expected_image_record = {
        "name": OUTPUT_FILES["root-disk"],
        "sha256": expected_image.sha256,
        "sizeBytes": expected_image.size_bytes,
    }
    if document.get("image") != expected_image_record:
        raise ReadbackV3Error("readback image identity differs")
    if document.get("sourceLock") != {
        "path": SOURCE_LOCK_PATH,
        "sha256": SOURCE_LOCK_SHA256,
    }:
        raise ReadbackV3Error("readback source-lock binding differs")
    if document.get("producerPreregistration") != {
        "path": PREREGISTRATION_PATH,
        "sha256": PREREGISTRATION_SHA256,
    }:
        raise ReadbackV3Error("readback preregistration binding differs")
    if document.get("importClosureCorrection") != {
        "path": IMPORT_CORRECTION_PATH,
        "sha256": IMPORT_CORRECTION_SHA256,
    }:
        raise ReadbackV3Error("readback import-correction binding differs")
    if document.get("launcherResult") != {
        "path": LAUNCHER_RESULT_PATH,
        "sha256": LAUNCHER_RESULT_SHA256,
        "launcherSha256": LAUNCHER_SHA256,
    }:
        raise ReadbackV3Error("readback launcher binding differs")
    report = document.get("verification")
    if _validate_verification_report(report) is not True:
        raise ReadbackV3Error("readback verification did not pass")


def _verify_pinned(
    *,
    sealed: RepositoryBindings,
    output_root: pathlib.Path,
    produced: dict[str, pathlib.Path],
    pinned: dict[str, PinnedFile],
    effects: Optional[ReadbackEffects],
) -> dict[str, Any]:
    identities = {role: output.identity for role, output in pinned.items()}
    inode_keys = {(row.device, row.inode) for row in identities.values()}
    if len(inode_keys) != len(identities):
        raise ReadbackV3Error(
            "kernel, initrd and root-disk outputs must be distinct inodes"
        )
    image = produced["root-disk"]
    image_pin = pinned["root-disk"]
    image_before = identities["root-disk"]
    result = output_root / RESULT_NAME
    pending = output_root / QUALIFIED_PENDING_NAME
    diagnostic = output_root / UNQUALIFIED_NAME
    _require_absent(result, "fixed readback result")
    _require_absent(pending, "qualified-result pending")
    _require_absent(diagnostic, "fixed unqualified diagnostic")
    mountpoint_path = output_root.parent / "successor-root-disk-readback-v3"
    _require_absent(mountpoint_path, "fresh mountpoint")

    host: ReadbackEffects = HostReadbackEffects() if effects is None else effects
    unmet = host.unmet_requirements()
    if unmet:
        raise ReadbackV3Error("reading the image back requires " + "; ".join(unmet))

    where = _fixed_mountpoint(output_root)
    try:
        tree = _read_mounted_tree(image=image_pin, mountpoint=where, effects=host)
    except BaseException as failure:
        if isinstance(failure, CleanupHardStop):
            raise
        try:
            _remove_mountpoint(where, failure)
        except CleanupHardStop:
            raise
        document = _failure_document(
            image=image,
            image_identity=image_before,
            failure=failure,
            stage="readback-effects",
        )
        _write_document_once(result, document)
        _write_unqualified(output_root, document)
        raise
    _remove_mountpoint(where)

    try:
        _require_outputs_unchanged(pinned, "during readback")
    except BaseException as failure:
        document = _failure_document(
            image=image,
            image_identity=image_before,
            failure=failure,
            stage="output-identity",
        )
        _write_document_once(result, document)
        _write_unqualified(output_root, document)
        raise

    try:
        report = image_verify.verify_tree(
            tree=tree,
            expectations=image_verify.expectations_from_lock(sealed.source_lock),
            launcherSha256=sealed.launcher_sha256,
            kernel=_read_regular_output(pinned["kernel"]),
        )
        passed = _validate_verification_report(report)
    except BaseException as failure:
        document = _failure_document(
            image=image,
            image_identity=image_before,
            failure=failure,
            stage="tree-verification",
        )
        _write_document_once(result, document)
        _write_unqualified(output_root, document)
        raise
    try:
        _require_outputs_unchanged(
            pinned,
            "during tree verification",
        )
    except BaseException as failure:
        document = _failure_document(
            image=image,
            image_identity=image_before,
            failure=failure,
            stage="post-verification-output-identity",
        )
        _write_document_once(result, document)
        _write_unqualified(output_root, document)
        raise
    document = _result_document(
        report=report,
        image=image,
        image_identity=image_before,
        entry_count=len(tree),
        passed=passed,
    )
    if not passed:
        _write_document_once(result, document)
        _write_unqualified(output_root, document)
        raise ReadbackV3Error("root-disk readback failed and is UNQUALIFIED-DIAGNOSTIC")
    assert_qualified_for_replica_comparison(
        document,
        expected_image=image_before,
        expected_entry_count=len(tree),
    )
    try:
        _publish_qualified_result_once(
            output_root=output_root,
            document=document,
            pinned=pinned,
        )
    except BaseException as failure:
        if isinstance(failure, CleanupHardStop):
            raise
        failed = _failure_document(
            image=image,
            image_identity=image_before,
            failure=failure,
            stage="qualified-result-publication",
        )
        _write_document_once(result, failed)
        _write_unqualified(output_root, failed)
        raise
    return document


def verify(
    *,
    outputs: pathlib.Path,
    effects: Optional[ReadbackEffects] = None,
) -> dict[str, Any]:
    """Read a v3 root disk only after its repository generation is re-sealed."""

    # These repository bytes are read before requirements, output lookup, loop
    # setup, mountpoint creation, or any other image-side effect.
    sealed = load_repository_bindings()

    output_root = _real_directory(pathlib.Path(outputs), "outputs directory")
    produced = output_paths(output_root)
    with contextlib.ExitStack() as stack:
        pinned = {
            role: stack.enter_context(_pin_output(path, role))
            for role, path in produced.items()
        }
        return _verify_pinned(
            sealed=sealed,
            output_root=output_root,
            produced=produced,
            pinned=pinned,
            effects=effects,
        )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--outputs", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    try:
        document = verify(
            outputs=options.outputs,
        )
    except (ReadbackV3Error, image_verify.ImageVerifyError) as exc:
        print(f"native-shadow successor root disk readback v3: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "native-shadow successor root disk readback v3: PASS "
        f"({document['entryCount']} entries)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
