#!/usr/bin/env python3
"""Production-only launcher-v2 successor image generation.

The module imports only the Python standard library until the complete
P2 -> R2 -> F6 -> A6 chain has been checked.  Repository build modules are
loaded lazily after that boundary by :class:`RepositoryImageBackend`.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import hashlib
import importlib
import importlib.abc
import importlib.machinery
import io
import json
import os
import pathlib
import re
import selectors
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from typing import Any, Iterable, Mapping, Optional, Protocol, Sequence


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]

P2_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-production-"
    "generation-preregistration-arm64-v1.json"
)
P2_SHA256 = "4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95"
P2_SIZE_BYTES = 8156
P3_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-production-"
    "dispatch-fence-correction-arm64-v1.json"
)
P3_SHA256 = "16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b"
P3_SIZE_BYTES = 7295
P3_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-production-dispatch-"
    "fence-correction.arm64.v1"
)
P3_STATUS = "CORRECTED-BEFORE-R2-NO-PRODUCTION-DISPATCH-AUTHORITY"
P3_FIELD = "productionDispatchFenceCorrection"
P3_ATTEMPT_ID_PATTERN = r"^[a-z0-9][a-z0-9._-]{0,127}$"
DISPATCH_CLAIM_SCHEMA = (
    "boole.native-shadow.mac3.successor-production-dispatch-claim.arm64.v1"
)
DISPATCH_CLAIM_REF_PREFIX = (
    "refs/tags/boole-native-shadow-mac3-successor-production-a6-"
)
R1_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v1.json"
)
R1_SHA256 = "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c"
R1_SIZE_BYTES = 10168
F5_PATH = (
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-"
    "arm64-v5.json"
)
F5_SHA256 = "6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa"
F5_SIZE_BYTES = 5458

P1_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
P1_SHA256 = "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec"
P1_SIZE_BYTES = 20145
SOURCE_LOCK_PATH = "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
SOURCE_LOCK_SHA256 = "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"
SOURCE_LOCK_SIZE_BYTES = 359099
BUILDER_AUTHORITY_PATH = (
    "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
)
BUILDER_AUTHORITY_SHA256 = (
    "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e"
)
BUILDER_AUTHORITY_SIZE_BYTES = 4714

R2_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v2.json"
)
F6_PATH = (
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-"
    "arm64-v6.json"
)
A6_PATH = (
    "native/containment/native-shadow-mac3-successor-production-authority-"
    "arm64-v6.json"
)
RESULT_V6_PATH = (
    "native/containment/native-shadow-mac3-successor-image-production-result-"
    "arm64-v6.json"
)
WITHDRAWN_A5_PATH = (
    "native/containment/native-shadow-mac3-successor-production-authority-"
    "arm64-v5.json"
)
WITHDRAWN_RESULT_V5_PATH = (
    "native/containment/native-shadow-mac3-successor-image-production-result-"
    "arm64-v5.json"
)

V4_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v4.py",
    "scripts/test_native_shadow_successor_produce_phase_arm64_v4.py",
    "scripts/native-shadow-successor-produce-arm64-v4.sh",
    ".github/workflows/native-shadow-successor-produce-arm64-v4.yml",
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py",
)
V4_WORKFLOW_PATH = V4_PATHS[3]
R2_GATE_PATH = (
    "scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_"
    "result_arm64_v2.py"
)
REUSED_PINNED_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v3.py",
    "scripts/native_shadow_successor_root_disk_readback_arm64_v3.py",
    "scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py",
)

MODULE_GATE = "scripts.native_shadow_rootfs_portable_boot_arm64_v2"
MODULE_BASE = "scripts.native_shadow_rootfs_builder_boot_arm64_v1"
MODULE_BUILDER_V4 = "scripts.native_shadow_rootfs_builder_boot_arm64_v4"
MODULE_V3 = "scripts.native_shadow_successor_produce_phase_arm64_v3"
MODULE_KERNEL = "scripts.native_shadow_boot_kernel_extract_arm64_v1"
MODULE_INITRD = "scripts.native_shadow_boot_initrd_arm64_v1"
MODULE_WRITER = "scripts.native_shadow_boot_writer_tree_arm64_v1"
MODULE_ROOT_DISK = "scripts.native_shadow_boot_root_disk_arm64_v1"
MODULE_ROOT_EXECUTE = "scripts.native_shadow_boot_root_disk_execute_arm64_v1"
MODULE_IMAGE_VERIFY = "scripts.native_shadow_boot_image_verify_arm64_v1"
MODULE_READBACK_V3 = "scripts.native_shadow_successor_root_disk_readback_arm64_v3"
LOW_LEVEL_MODULES = (
    MODULE_GATE,
    MODULE_BASE,
    MODULE_BUILDER_V4,
    MODULE_V3,
    MODULE_KERNEL,
    MODULE_INITRD,
    MODULE_WRITER,
    MODULE_ROOT_DISK,
    MODULE_ROOT_EXECUTE,
    MODULE_IMAGE_VERIFY,
)

R2_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal.arm64.v2"
)
R2_STATUS = "PASS-NO-IMAGE-PRODUCED"
R2_MEMORY_MAX_BYTES = 8 * 1024 * 1024 * 1024
R2_MEMORY_SWAP_MAX_BYTES = 0
R2_PIDS_MAX = 128
R2_RUNTIME_MAX_SECONDS = 1200
MAX_PROC_CGROUP_BYTES = 4096
MAX_CGROUP_VALUE_BYTES = 64
MAX_CGROUP_EVENTS_BYTES = 1024
F6_SCHEMA = "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v6"
F6_STATUS = "SEALED-AFTER-FRESH-R2-PRODUCTION-GENERATION-NOT-AUTHORISED"
A6_SCHEMA = "boole.native-shadow.mac3.successor-production-authority.arm64.v6"
A6_STATUS = "ONE-NAMED-PRODUCTION-RUN-AUTHORISED-NOT-RUN"

OUTPUT_NAMES = ("guest-kernel", "guest-initrd", "guest-root-disk")
CONSUMED_MARKER_NAME = "ATTEMPT-CONSUMED.json"
UNQUALIFIED_MARKER_NAME = "UNQUALIFIED-DIAGNOSTIC.json"
PENDING_RESULT_NAME = "PRODUCE-RESULT-PENDING-READBACK-V4.json"
QUALIFIED_RESULT_NAME = "PRODUCE-RESULT.json"
READBACK_RESULT_NAME = "SUCCESSOR-ROOT-DISK-READBACK-V3.json"
READBACK_PRIVATE_PENDING_NAME = ".SUCCESSOR-ROOT-DISK-READBACK-V3.pending"
QUALIFICATION_PINNED_NAMES = (
    *OUTPUT_NAMES,
    CONSUMED_MARKER_NAME,
    PENDING_RESULT_NAME,
)
QUALIFIED_REPLICA_NAMES = (
    *QUALIFICATION_PINNED_NAMES,
    READBACK_RESULT_NAME,
    QUALIFIED_RESULT_NAME,
)
REPLICA_PROVENANCE_NAME = "REPLICA-PROVENANCE.json"
REPLICA_PROVENANCE_PARTIAL_NAME = f".{REPLICA_PROVENANCE_NAME}.partial"
RECOVERY_RECORD_NAME = "RECOVERY-IDENTITY.json"
RECOVERY_RECORD_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-production-recovery."
    "arm64.v4"
)
RECOVERY_CLEANUP_CHECKPOINT_NAME = "RECOVERY-CLEANUP-VERIFIED.json"
RECOVERY_CLEANUP_CHECKPOINT_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-production-cleanup-"
    "checkpoint.arm64.v4"
)
REPLICA_PROVENANCE_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-replica-provenance."
    "arm64.v1"
)
REPLICA_ARTIFACT_PREFIX = "native-shadow-successor-v4-replica-"
COLLECTABLE_FILE_MODE = 0o444
COLLECTABLE_OUTPUT_DIRECTORY_MODE = 0o555
COLLECTABLE_PARENT_MODE = 0o711
READBACK_PASS_STATUS = "READBACK-V3-PASS-QUALIFIED-FOR-REPLICA-COMPARISON"
PRODUCTION_PENDING_STATUS = "PRODUCED-PENDING-READBACK-V3"
PENDING_RESULT_SCHEMA = (
    "boole.native-shadow.mac3.launcher-v2-successor-production-pending-"
    "readback.arm64.v4"
)
# The four JSON evidence files are tiny by construction.  A damaged or hostile
# artifact must not turn their convenient whole-file parsing into an unbounded
# memory allocation before fail-closed validation can run.
MAX_METADATA_BYTES = 1024 * 1024
MAX_RECOVERY_RECORD_BYTES = 64 * 1024
MAX_MOUNTINFO_BYTES = 8 * 1024 * 1024
RECOVERY_TMPFS_SIZE_BYTES = 2_147_483_648 * 3
RECOVERY_TMPFS_INODES = 200_000 * 3
# The sealed launcher is about 2 MiB.  Four MiB leaves deliberate format
# headroom while preventing a replaced path from becoming an unbounded read.
MAX_LAUNCHER_BYTES = 4 * 1024 * 1024
SEALED_LAUNCHER_SHA256 = (
    "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"
)
SEALED_LAUNCHER_SIZE_BYTES = 2_025_192
PRODUCTION_QUALIFIED_STATUS = "PRODUCED-AND-READBACK-V3-QUALIFIED"
READBACK_REQUIRED_CHECKS = (
    "kernel-is-arm64",
    "launcher-digest-matches-seal",
    "launcher-service-is-enabled",
    "modes-owners-and-paths-match-the-lock",
    "pid1-is-systemd",
    "replay-node-absent",
    "runtime-mount-points-present",
)


def _collectable_progress_states() -> frozenset[frozenset[str]]:
    consumed = CONSUMED_MARKER_NAME
    progress = (
        (consumed,),
        (consumed, OUTPUT_NAMES[0]),
        (consumed, *OUTPUT_NAMES[:2]),
        (consumed, *OUTPUT_NAMES),
    )
    allowed = {frozenset(names) for names in progress}
    allowed.update(
        frozenset((*names, UNQUALIFIED_MARKER_NAME)) for names in progress
    )
    pending = (*progress[-1], PENDING_RESULT_NAME)
    allowed.add(frozenset(pending))
    allowed.add(frozenset((*pending, UNQUALIFIED_MARKER_NAME)))
    allowed.add(frozenset((*pending, READBACK_RESULT_NAME)))
    allowed.add(
        frozenset((*pending, READBACK_RESULT_NAME, QUALIFIED_RESULT_NAME))
    )
    return frozenset(allowed)


COLLECTABLE_PROGRESS_STATES = _collectable_progress_states()

_PRODUCER_PROGRESS_STATES = tuple(
    frozenset(names)
    for names in (
        (CONSUMED_MARKER_NAME,),
        (CONSUMED_MARKER_NAME, OUTPUT_NAMES[0]),
        (CONSUMED_MARKER_NAME, *OUTPUT_NAMES[:2]),
        (CONSUMED_MARKER_NAME, *OUTPUT_NAMES),
    )
)
_PRODUCTION_PENDING_PREDECESSOR = frozenset(
    (*_PRODUCER_PROGRESS_STATES[-1], PENDING_RESULT_NAME)
)
_CREATE_ONCE_OUTPUT_PREDECESSORS = {
    OUTPUT_NAMES[1]: frozenset((_PRODUCER_PROGRESS_STATES[1],)),
    PENDING_RESULT_NAME: frozenset((_PRODUCER_PROGRESS_STATES[-1],)),
    UNQUALIFIED_MARKER_NAME: frozenset(
        (*_PRODUCER_PROGRESS_STATES, _PRODUCTION_PENDING_PREDECESSOR)
    ),
    QUALIFIED_RESULT_NAME: frozenset(
        (
            frozenset(
                (*_PRODUCTION_PENDING_PREDECESSOR, READBACK_RESULT_NAME)
            ),
        )
    ),
    READBACK_RESULT_NAME: frozenset((_PRODUCTION_PENDING_PREDECESSOR,)),
}

ZERO_AUTHORISATIONS = {
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
ZERO_EFFECTS = {
    "attemptMarkersCreated": 0,
    "bootAttempts": 0,
    "imageOutputsCreated": 0,
    "productionOutputsCreated": 0,
}
ZERO_RUNS = {
    "bootsAllowed": 0,
    "bootsPerformed": 0,
    "imageProductionRunsAllowed": 0,
    "imageProductionRunsPerformed": 0,
}
P3_ZERO_RUNS = {
    "bootsAllowed": 0,
    "bootsPerformed": 0,
    "freeRehearsalsAllowed": 0,
    "freeRehearsalsPerformed": 0,
    "imageProductionsAllowed": 0,
    "imageProductionsPerformed": 0,
    "productionDispatchClaimsAllowed": 0,
    "productionDispatchClaimsCreated": 0,
}


class SuccessorProduceV4Error(RuntimeError):
    """The production generation or its one-way authority chain differs."""


def _module_name_from_relative(relative: str) -> str:
    pure = pathlib.PurePosixPath(relative)
    if pure.suffix != ".py":
        raise SuccessorProduceV4Error(f"bound repository module is not Python: {relative}")
    parts = list(pure.with_suffix("").parts)
    if parts[-1] == "__init__":
        parts.pop()
    if not parts or any(not part.isidentifier() for part in parts):
        raise SuccessorProduceV4Error(f"bound repository module path is invalid: {relative}")
    return ".".join(parts)


@dataclasses.dataclass(frozen=True)
class FileIdentity:
    path: str
    sha256: str
    size_bytes: int

    def record(self) -> dict[str, Any]:
        return {
            "path": self.path,
            "sha256": self.sha256,
            "sizeBytes": self.size_bytes,
        }


@dataclasses.dataclass(frozen=True)
class PinnedOutputIdentity:
    device: int
    inode: int
    mode: int
    mtime_ns: int
    ctime_ns: int
    sha256: str
    size_bytes: int


class PinnedOutputs(Mapping[str, PinnedOutputIdentity]):
    """Names, identities, and still-open descriptors for one fixed output set."""

    def __init__(
        self,
        identities: Mapping[str, PinnedOutputIdentity],
        descriptors: Mapping[str, int],
    ) -> None:
        self._identities = dict(identities)
        self._descriptors = dict(descriptors)

    def __getitem__(self, name: str) -> PinnedOutputIdentity:
        return self._identities[name]

    def __iter__(self):
        return iter(self._identities)

    def __len__(self) -> int:
        return len(self._identities)

    @staticmethod
    def _stat_key(metadata: os.stat_result) -> tuple[int, int, int, int, int, int]:
        return (
            metadata.st_dev,
            metadata.st_ino,
            metadata.st_mode,
            metadata.st_size,
            metadata.st_mtime_ns,
            metadata.st_ctime_ns,
        )

    @staticmethod
    def _identity_key(
        identity: PinnedOutputIdentity,
    ) -> tuple[int, int, int, int, int, int]:
        return (
            identity.device,
            identity.inode,
            identity.mode,
            identity.size_bytes,
            identity.mtime_ns,
            identity.ctime_ns,
        )

    def read_metadata(
        self,
        name: str,
        context: str,
        *,
        max_bytes: int,
    ) -> bytes:
        """Read bounded JSON bytes from the descriptor that was originally pinned."""

        try:
            expected = self._identities[name]
            descriptor = self._descriptors[name]
        except KeyError as exc:
            raise SuccessorProduceV4Error(
                f"{context} was not pinned before validation"
            ) from exc
        if expected.size_bytes > max_bytes:
            raise SuccessorProduceV4Error(f"metadata exceeds byte limit: {name}")
        before = os.fstat(descriptor)
        if self._stat_key(before) != self._identity_key(expected):
            raise SuccessorProduceV4Error(f"{context} changed while it was pinned")
        os.lseek(descriptor, 0, os.SEEK_SET)
        remaining = max_bytes + 1
        chunks: list[bytes] = []
        try:
            while remaining:
                chunk = os.read(descriptor, min(64 * 1024, remaining))
                if not chunk:
                    break
                chunks.append(chunk)
                remaining -= len(chunk)
        except OSError as exc:
            raise SuccessorProduceV4Error(f"{context} is unreadable") from exc
        raw = b"".join(chunks)
        if len(raw) > max_bytes:
            raise SuccessorProduceV4Error(f"metadata exceeds byte limit: {name}")
        after = os.fstat(descriptor)
        if (
            self._stat_key(after) != self._identity_key(expected)
            or len(raw) != expected.size_bytes
            or _sha256(raw) != expected.sha256
        ):
            raise SuccessorProduceV4Error(f"{context} changed while it was pinned")
        return raw


@dataclasses.dataclass(frozen=True)
class PreregisteredGeneration:
    preregistration: Mapping[str, Any]
    rehearsal: Mapping[str, Any]
    historical_fingerprint: Mapping[str, Any]
    dispatch_fence_identity: FileIdentity
    predecessor_identities: tuple[FileIdentity, ...]
    generation_identities: tuple[FileIdentity, ...]
    reused_identities: tuple[FileIdentity, ...]


@dataclasses.dataclass(frozen=True)
class GenerationChain:
    preregistration: Mapping[str, Any]
    rehearsal: Mapping[str, Any]
    historical_fingerprint: Mapping[str, Any]
    fresh_rehearsal: Mapping[str, Any]
    fingerprint: Mapping[str, Any]
    authority: Mapping[str, Any]
    identities: Mapping[str, FileIdentity]
    attempt_id: str
    output_names: tuple[str, ...]


@dataclasses.dataclass(frozen=True)
class DispatchCapability:
    """One immutable annotated-tag claim handed to the production core."""

    claim_ref: str
    ref_object_sha: str
    tag_object_sha: str
    raw_tag_object: bytes
    github_run_id: str
    github_run_attempt: str
    workflow_path: str
    head_sha: str
    head_a6_sha256: str


@dataclasses.dataclass(frozen=True)
class ProductionRequest:
    repository_root: pathlib.Path
    artifact_store: pathlib.Path
    outputs: pathlib.Path
    scratch: pathlib.Path
    gpgv: pathlib.Path
    zstd: pathlib.Path
    launcher: pathlib.Path
    launcher_binary: bytes
    chain: Any
    dispatch_capability: Optional[DispatchCapability] = None


@dataclasses.dataclass(frozen=True)
class PreparedProduction:
    measurement: Mapping[str, Any]
    build_receipt: Mapping[str, Any]
    state: Any


@dataclasses.dataclass(frozen=True)
class RepositoryImportRequest:
    repository_root: pathlib.Path
    chain: Any


class ImageBackend(Protocol):
    def prepare(self, request: ProductionRequest) -> PreparedProduction: ...

    def extract_kernel(
        self, request: ProductionRequest, prepared: PreparedProduction
    ) -> Mapping[str, Any]: ...

    def build_initrd(
        self, request: ProductionRequest, prepared: PreparedProduction
    ) -> bytes: ...

    def build_root_disk(
        self, request: ProductionRequest, prepared: PreparedProduction
    ) -> Mapping[str, Any]: ...

    def verify_images(
        self,
        request: ProductionRequest,
        prepared: PreparedProduction,
        kernel: Mapping[str, Any],
        initrd: bytes,
        root_disk: Mapping[str, Any],
    ) -> Mapping[str, Any]: ...

    def readback(
        self,
        repository_root: pathlib.Path,
        outputs: pathlib.Path,
        chain: GenerationChain,
    ) -> Mapping[str, Any]: ...


class AutoclearReadbackEffects:
    """Keep readback-v3 semantics while making loop cleanup survive unit death."""

    def __init__(self, readback_module: Any) -> None:
        self._module = readback_module
        self._delegate = readback_module.HostReadbackEffects()

    def unmet_requirements(self) -> list[str]:
        return list(self._delegate.unmet_requirements())

    def setup_loop(self, image: Any) -> str:
        procfd = f"/proc/self/fd/{image.descriptor}"
        output = self._module._run(
            [
                self._module.LOSETUP,
                "--find",
                "--show",
                "--read-only",
                "--autoclear",
                procfd,
            ],
            pass_fds=(image.descriptor,),
        ).decode("utf-8", errors="strict").strip()
        if re.fullmatch(r"/dev/loop[0-9]+", output) is None:
            raise self._module.ReadbackV3Error(
                f"losetup returned an unsafe loop name: {output!r}"
            )
        return output

    def mount(self, device: str, mountpoint: pathlib.Path) -> None:
        self._delegate.mount(device, mountpoint)

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, Any]]:
        return dict(self._delegate.read_tree(mountpoint))

    def unmount(self, mountpoint: pathlib.Path) -> None:
        self._delegate.unmount(mountpoint)

    def detach_loop(self, device: str) -> None:
        # Explicit normal-path detach remains required.  --autoclear is the
        # crash-path backstop, not a reason to weaken ordinary cleanup.
        self._delegate.detach_loop(device)


def canonical_json(document: Any) -> bytes:
    return (json.dumps(document, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _safe_path(root: pathlib.Path, relative: str) -> pathlib.Path:
    pure = pathlib.PurePosixPath(relative)
    if not relative or pure.is_absolute() or ".." in pure.parts or pure.as_posix() != relative:
        raise SuccessorProduceV4Error(f"unsafe repository binding path: {relative!r}")
    root = pathlib.Path(root).resolve()
    candidate = root / pathlib.Path(*pure.parts)
    try:
        contained = candidate.resolve(strict=False).is_relative_to(root)
    except AttributeError:  # pragma: no cover - Python before 3.9 fallback
        resolved = candidate.resolve(strict=False)
        contained = root == resolved or root in resolved.parents
    if not contained:
        raise SuccessorProduceV4Error(f"repository binding leaves root: {relative}")
    return candidate


def _read_regular(
    root: pathlib.Path,
    relative: str,
    *,
    max_bytes: Optional[int] = None,
) -> tuple[FileIdentity, bytes]:
    _safe_path(root, relative)
    pure = pathlib.PurePosixPath(relative)
    root_path = pathlib.Path(root).resolve()
    directory_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        directory_flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        directory_flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        directory_flags |= os.O_CLOEXEC
    file_flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        file_flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        file_flags |= os.O_CLOEXEC
    directory_descriptors: list[int] = []
    try:
        root_descriptor = os.open(str(root_path), directory_flags)
        directory_descriptors.append(root_descriptor)
        parent_descriptor = root_descriptor
        for component in pure.parts[:-1]:
            try:
                descriptor = os.open(
                    component, directory_flags, dir_fd=parent_descriptor
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"required binding parent component is not a real directory: {relative}"
                ) from exc
            metadata = os.fstat(descriptor)
            if not stat.S_ISDIR(metadata.st_mode):
                os.close(descriptor)
                raise SuccessorProduceV4Error(
                    f"required binding parent component is not a directory: {relative}"
                )
            directory_descriptors.append(descriptor)
            parent_descriptor = descriptor
        try:
            info = os.stat(
                pure.parts[-1], dir_fd=parent_descriptor, follow_symlinks=False
            )
        except OSError as exc:
            raise SuccessorProduceV4Error(
                f"required binding is absent: {relative}"
            ) from exc
        if not stat.S_ISREG(info.st_mode):
            raise SuccessorProduceV4Error(
                f"required binding is not a regular non-symlink file: {relative}"
            )
        descriptor = os.open(
            pure.parts[-1], file_flags, dir_fd=parent_descriptor
        )
        with os.fdopen(descriptor, "rb") as handle:
            opened = os.fstat(handle.fileno())
            if max_bytes is not None and opened.st_size > max_bytes:
                raise SuccessorProduceV4Error(
                    f"metadata exceeds byte limit: {relative}"
                )
            inspected_identity = (
                info.st_dev,
                info.st_ino,
                info.st_mode,
                info.st_size,
                info.st_mtime_ns,
                info.st_ctime_ns,
            )
            opened_identity = (
                opened.st_dev,
                opened.st_ino,
                opened.st_mode,
                opened.st_size,
                opened.st_mtime_ns,
                opened.st_ctime_ns,
            )
            if opened_identity != inspected_identity:
                raise SuccessorProduceV4Error(
                    f"required binding changed between inspection and open: {relative}"
                )
            raw = (
                handle.read(max_bytes + 1)
                if max_bytes is not None
                else handle.read()
            )
            if max_bytes is not None and len(raw) > max_bytes:
                raise SuccessorProduceV4Error(
                    f"metadata exceeds byte limit: {relative}"
                )
            completed = os.fstat(handle.fileno())
            completed_identity = (
                completed.st_dev,
                completed.st_ino,
                completed.st_mode,
                completed.st_size,
                completed.st_mtime_ns,
                completed.st_ctime_ns,
            )
            if completed_identity != opened_identity or len(raw) != completed.st_size:
                raise SuccessorProduceV4Error(
                    f"required binding changed while it was read: {relative}"
                )
    except SuccessorProduceV4Error:
        raise
    except OSError as exc:
        raise SuccessorProduceV4Error(f"required binding is unreadable: {relative}") from exc
    finally:
        for descriptor in reversed(directory_descriptors):
            try:
                os.close(descriptor)
            except OSError:
                pass
    return FileIdentity(relative, _sha256(raw), len(raw)), raw


def _output_file_flags() -> int:
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    return flags


def _descriptor_output_identity(descriptor: int, context: str) -> PinnedOutputIdentity:
    before = os.fstat(descriptor)
    if not stat.S_ISREG(before.st_mode):
        raise SuccessorProduceV4Error(f"{context} is not a regular file")
    digest = hashlib.sha256()
    os.lseek(descriptor, 0, os.SEEK_SET)
    while True:
        chunk = os.read(descriptor, 1024 * 1024)
        if not chunk:
            break
        digest.update(chunk)
    after = os.fstat(descriptor)
    before_key = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
        before.st_mtime_ns,
        before.st_ctime_ns,
    )
    after_key = (
        after.st_dev,
        after.st_ino,
        after.st_mode,
        after.st_size,
        after.st_mtime_ns,
        after.st_ctime_ns,
    )
    if before_key != after_key:
        raise SuccessorProduceV4Error(f"{context} changed while it was pinned")
    return PinnedOutputIdentity(
        device=after.st_dev,
        inode=after.st_ino,
        mode=after.st_mode,
        mtime_ns=after.st_mtime_ns,
        ctime_ns=after.st_ctime_ns,
        sha256=digest.hexdigest(),
        size_bytes=after.st_size,
    )


@contextlib.contextmanager
def _pinned_outputs(
    outputs: pathlib.Path, names: Sequence[str]
) -> Iterable[PinnedOutputs]:
    """Keep every named qualification input inode open across readback."""

    root = _real_directory(outputs, "production outputs")
    directory_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        directory_flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        directory_flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        directory_flags |= os.O_CLOEXEC
    directory_descriptor = os.open(str(root), directory_flags)
    descriptors: dict[str, int] = {}
    identities: dict[str, PinnedOutputIdentity] = {}
    try:
        for name in names:
            if pathlib.PurePosixPath(name).name != name:
                raise SuccessorProduceV4Error(f"unsafe output name: {name}")
            try:
                descriptor = os.open(
                    name, _output_file_flags(), dir_fd=directory_descriptor
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"qualification output is unreadable: {name}"
                ) from exc
            descriptors[name] = descriptor
            identities[name] = _descriptor_output_identity(
                descriptor, f"qualification output {name}"
            )
        inode_keys = {
            (identity.device, identity.inode) for identity in identities.values()
        }
        if len(inode_keys) != len(identities):
            raise SuccessorProduceV4Error(
                "qualification inputs must use distinct inodes"
            )
        yield PinnedOutputs(identities, descriptors)
    finally:
        for descriptor in descriptors.values():
            try:
                os.close(descriptor)
            except OSError:
                pass
        os.close(directory_descriptor)


def _assert_pinned_outputs_unchanged(
    outputs: pathlib.Path,
    pinned: Mapping[str, PinnedOutputIdentity],
) -> None:
    with _pinned_outputs(outputs, tuple(pinned)) as current:
        if not _strictly_equal(dict(current), dict(pinned)):
            raise SuccessorProduceV4Error(
                "qualification output identity changed across readback publication"
            )


def _sync_produced_images(outputs: pathlib.Path) -> None:
    """Persist backend-created image bytes before any pending verdict is named."""

    root = _real_directory(outputs, "production outputs")
    directory_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        directory_flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        directory_flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        directory_flags |= os.O_CLOEXEC
    directory_descriptor = os.open(str(root), directory_flags)
    try:
        for name in OUTPUT_NAMES:
            try:
                descriptor = os.open(
                    name, _output_file_flags(), dir_fd=directory_descriptor
                )
                try:
                    metadata = os.fstat(descriptor)
                    if not stat.S_ISREG(metadata.st_mode):
                        raise SuccessorProduceV4Error(
                            f"produced image is not a regular file: {name}"
                        )
                    sync = getattr(os, "fdatasync", os.fsync)
                    sync(descriptor)
                finally:
                    os.close(descriptor)
            except SuccessorProduceV4Error:
                raise
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"cannot make produced image durable: {name}"
                ) from exc
        try:
            os.fsync(directory_descriptor)
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "cannot make produced image directory durable"
            ) from exc
    finally:
        os.close(directory_descriptor)


def _load_canonical(root: pathlib.Path, relative: str) -> tuple[FileIdentity, dict[str, Any]]:
    identity, raw = _read_regular(root, relative)
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SuccessorProduceV4Error(f"binding is not canonical JSON: {relative}") from exc
    if not isinstance(document, dict) or raw != canonical_json(document):
        raise SuccessorProduceV4Error(f"binding is not one canonical JSON object: {relative}")
    return identity, document


def _require_identity(actual: FileIdentity, expected: Mapping[str, Any], context: str) -> None:
    if actual.record() != dict(expected):
        raise SuccessorProduceV4Error(f"{context} identity differs")


def _require_exact_keys(document: Mapping[str, Any], expected: set[str], context: str) -> None:
    if set(document) != expected:
        raise SuccessorProduceV4Error(f"{context} keys differ")


def _strictly_equal(actual: Any, expected: Any) -> bool:
    """Compare JSON values without Python's bool/int aliasing."""

    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return set(actual) == set(expected) and all(
            _strictly_equal(actual[key], value) for key, value in expected.items()
        )
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(
            _strictly_equal(left, right) for left, right in zip(actual, expected)
        )
    return actual == expected


def _require_strict_equal(actual: Any, expected: Any, context: str) -> None:
    if not _strictly_equal(actual, expected):
        raise SuccessorProduceV4Error(f"{context} differs")


def _require_absent(root: pathlib.Path, relative: str, context: str) -> None:
    path = _safe_path(root, relative)
    if os.path.lexists(path):
        raise SuccessorProduceV4Error(f"{context} must remain absent: {relative}")


def _identity_rows(identities: Iterable[FileIdentity]) -> list[dict[str, Any]]:
    return [identity.record() for identity in identities]


def dispatch_claim_ref(attempt_id: str) -> str:
    """Return the one fixed tag ref, rejecting Git-ref liveness traps."""

    if not isinstance(attempt_id, str) or re.fullmatch(
        P3_ATTEMPT_ID_PATTERN, attempt_id
    ) is None:
        raise SuccessorProduceV4Error(
            "dispatch claim attempt ID violates the P3 pattern"
        )
    claim_ref = DISPATCH_CLAIM_REF_PREFIX + attempt_id
    if ".." in claim_ref or claim_ref.endswith(".") or claim_ref.endswith(".lock"):
        raise SuccessorProduceV4Error(
            "dispatch claim attempt ID cannot form one valid Git tag"
        )
    return claim_ref


def _dispatch_runtime_context(
    chain: GenerationChain,
    *,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> None:
    if not isinstance(github_run_id, str) or re.fullmatch(
        r"[1-9][0-9]*", github_run_id
    ) is None:
        raise SuccessorProduceV4Error("dispatch claim GitHub run ID differs")
    if type(github_run_attempt) is not str or github_run_attempt != "1":
        raise SuccessorProduceV4Error("dispatch claim GitHub run attempt differs")
    if type(workflow_path) is not str or workflow_path != V4_WORKFLOW_PATH:
        raise SuccessorProduceV4Error("dispatch claim workflow path differs")
    if not isinstance(head_sha, str) or re.fullmatch(
        r"[0-9a-f]{40}", head_sha
    ) is None:
        raise SuccessorProduceV4Error("dispatch claim head SHA differs")
    if not isinstance(head_a6_sha256, str) or re.fullmatch(
        r"[0-9a-f]{64}", head_a6_sha256
    ) is None:
        raise SuccessorProduceV4Error("dispatch claim HEAD A6 digest differs")
    if head_a6_sha256 != chain.identities["A6"].sha256:
        raise SuccessorProduceV4Error("dispatch claim HEAD A6 digest differs")


def dispatch_claim_document(
    chain: GenerationChain,
    *,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> dict[str, str]:
    """Build the exact side-effect-free message for a future atomic tag create."""

    if not isinstance(chain, GenerationChain):
        raise SuccessorProduceV4Error("dispatch claim has no verified A6 chain")
    _dispatch_runtime_context(
        chain,
        github_run_id=github_run_id,
        github_run_attempt=github_run_attempt,
        workflow_path=workflow_path,
        head_sha=head_sha,
        head_a6_sha256=head_a6_sha256,
    )
    dispatch_claim_ref(chain.attempt_id)
    return {
        "a6Sha256": chain.identities["A6"].sha256,
        "attemptId": chain.attempt_id,
        "githubRunId": github_run_id,
        "headSha": head_sha,
        "schema": DISPATCH_CLAIM_SCHEMA,
        "workflowPath": workflow_path,
    }


def dispatch_claim_message(
    chain: GenerationChain,
    *,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> bytes:
    """Encode the tag message as sorted compact UTF-8 JSON without a newline."""

    document = dispatch_claim_document(
        chain,
        github_run_id=github_run_id,
        github_run_attempt=github_run_attempt,
        workflow_path=workflow_path,
        head_sha=head_sha,
        head_a6_sha256=head_a6_sha256,
    )
    return json.dumps(document, sort_keys=True, separators=(",", ":")).encode(
        "utf-8"
    )


def verify_dispatch_claim(
    chain: GenerationChain,
    *,
    claim_ref: str,
    raw_message: bytes,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> dict[str, str]:
    """Recheck one repository claim against live A6 and workflow context."""

    expected_ref = dispatch_claim_ref(chain.attempt_id)
    if claim_ref != expected_ref:
        raise SuccessorProduceV4Error("dispatch claim ref differs")
    expected = dispatch_claim_document(
        chain,
        github_run_id=github_run_id,
        github_run_attempt=github_run_attempt,
        workflow_path=workflow_path,
        head_sha=head_sha,
        head_a6_sha256=head_a6_sha256,
    )
    expected_raw = json.dumps(
        expected, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    if not isinstance(raw_message, bytes) or raw_message != expected_raw:
        raise SuccessorProduceV4Error("dispatch claim message differs")
    return expected


def _git_dispatch_read(
    repository_root: pathlib.Path,
    arguments: Sequence[str],
    *,
    max_bytes: int,
) -> bytes:
    """Read one bounded local Git fact with no inherited configuration."""

    supplied_root = pathlib.Path(repository_root)
    try:
        supplied_info = supplied_root.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository is unavailable"
        ) from exc
    if supplied_root.is_symlink() or not stat.S_ISDIR(supplied_info.st_mode):
        raise SuccessorProduceV4Error("dispatch claim live repository differs")
    try:
        root = supplied_root.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository root differs"
        ) from exc
    if max_bytes < 1 or max_bytes > MAX_METADATA_BYTES + 1:
        raise SuccessorProduceV4Error("dispatch claim Git read bound differs")
    command = (
        "/usr/bin/git",
        "--no-replace-objects",
        "-c",
        f"safe.directory={root}",
        "-C",
        str(root),
        *arguments,
    )
    environment = {
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_SYSTEM": "/dev/null",
        "GIT_OPTIONAL_LOCKS": "0",
        "HOME": "/nonexistent",
        "LANG": "C",
        "LC_ALL": "C",
        "PATH": "/usr/bin:/bin",
    }
    process: Optional[subprocess.Popen[bytes]] = None
    selector = selectors.DefaultSelector()
    streams: dict[int, tuple[bytearray, int]] = {}
    try:
        process = subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
        )
        if process.stdout is None or process.stderr is None:
            raise SuccessorProduceV4Error(
                "dispatch claim live repository is unavailable"
            )
        stdout = bytearray()
        stderr = bytearray()
        streams[process.stdout.fileno()] = (stdout, max_bytes)
        streams[process.stderr.fileno()] = (stderr, 16_384)
        selector.register(process.stdout, selectors.EVENT_READ)
        selector.register(process.stderr, selectors.EVENT_READ)
        deadline = time.monotonic() + 10
        while streams:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                process.kill()
                process.wait()
                raise SuccessorProduceV4Error(
                    "dispatch claim live repository is unavailable"
                )
            events = selector.select(remaining)
            if not events:
                continue
            for key, _ in events:
                descriptor = key.fd
                buffer, limit = streams[descriptor]
                chunk = os.read(descriptor, min(65_536, limit - len(buffer) + 1))
                if not chunk:
                    selector.unregister(key.fileobj)
                    del streams[descriptor]
                    continue
                buffer.extend(chunk)
                if len(buffer) > limit:
                    process.kill()
                    process.wait()
                    raise SuccessorProduceV4Error(
                        "dispatch claim live repository output exceeds its bound"
                    )
        return_code = process.wait(timeout=max(0.1, deadline - time.monotonic()))
    except (OSError, subprocess.TimeoutExpired) as exc:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        raise SuccessorProduceV4Error(
            "dispatch claim live repository is unavailable"
        ) from exc
    finally:
        selector.close()
        if process is not None:
            if process.stdout is not None:
                process.stdout.close()
            if process.stderr is not None:
                process.stderr.close()
    if return_code != 0:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository ref differs"
        )
    return bytes(stdout)


def _git_dispatch_scalar(
    repository_root: pathlib.Path,
    arguments: Sequence[str],
    context: str,
) -> str:
    raw = _git_dispatch_read(repository_root, arguments, max_bytes=4_096)
    try:
        value = raw.decode("ascii")
    except UnicodeDecodeError as exc:
        raise SuccessorProduceV4Error(
            f"dispatch claim live {context} differs"
        ) from exc
    if not value.endswith("\n") or "\n" in value[:-1] or not value[:-1]:
        raise SuccessorProduceV4Error(
            f"dispatch claim live {context} differs"
        )
    return value[:-1]


def _verify_live_dispatch_repository(
    *,
    repository_root: pathlib.Path,
    chain: GenerationChain,
    claim_ref: str,
    ref_object_sha: str,
    tag_object_sha: str,
    raw_tag_object: bytes,
    head_sha: str,
) -> None:
    """Require the capability bytes to be reachable from the exact live ref."""

    supplied_root = pathlib.Path(repository_root)
    try:
        root_info = supplied_root.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository is unavailable"
        ) from exc
    if supplied_root.is_symlink() or not stat.S_ISDIR(root_info.st_mode):
        raise SuccessorProduceV4Error("dispatch claim live repository differs")
    try:
        root = supplied_root.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository root differs"
        ) from exc
    git_control = root / ".git"
    try:
        git_info = git_control.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository control path differs"
        ) from exc
    if git_control.is_symlink() or not (
        stat.S_ISDIR(git_info.st_mode) or stat.S_ISREG(git_info.st_mode)
    ):
        raise SuccessorProduceV4Error(
            "dispatch claim live repository control path differs"
        )
    root_identity = (
        root_info.st_dev,
        root_info.st_ino,
        stat.S_IFMT(root_info.st_mode),
    )
    git_identity = (
        git_info.st_dev,
        git_info.st_ino,
        stat.S_IFMT(git_info.st_mode),
    )

    def require_same_repository() -> None:
        try:
            current_root = root.lstat()
            current_git = git_control.lstat()
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "dispatch claim live repository changed"
            ) from exc
        if root.is_symlink() or git_control.is_symlink() or (
            current_root.st_dev,
            current_root.st_ino,
            stat.S_IFMT(current_root.st_mode),
        ) != root_identity or (
            current_git.st_dev,
            current_git.st_ino,
            stat.S_IFMT(current_git.st_mode),
        ) != git_identity:
            raise SuccessorProduceV4Error(
                "dispatch claim live repository changed"
            )
    top_level = _git_dispatch_scalar(root, ("rev-parse", "--show-toplevel"), "root")
    try:
        observed_top_level = pathlib.Path(top_level).resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live repository root differs"
        ) from exc
    if observed_top_level != root:
        raise SuccessorProduceV4Error("dispatch claim live repository root differs")

    observed_head = _git_dispatch_scalar(
        root, ("rev-parse", "--verify", "HEAD^{commit}"), "HEAD"
    )
    if observed_head != head_sha:
        raise SuccessorProduceV4Error("dispatch claim live HEAD differs")
    a6_object = f"{head_sha}:{A6_PATH}"
    raw_a6_size = _git_dispatch_scalar(
        root, ("cat-file", "-s", a6_object), "A6 size"
    )
    try:
        a6_size = int(raw_a6_size, 10)
    except ValueError as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live A6 size differs"
        ) from exc
    if a6_size != chain.identities["A6"].size_bytes or a6_size > MAX_METADATA_BYTES:
        raise SuccessorProduceV4Error("dispatch claim live A6 size differs")
    raw_a6 = _git_dispatch_read(
        root,
        ("cat-file", "blob", a6_object),
        max_bytes=a6_size,
    )
    if (
        len(raw_a6) != a6_size
        or _sha256(raw_a6) != chain.identities["A6"].sha256
    ):
        raise SuccessorProduceV4Error("dispatch claim live A6 differs")

    observed_ref = _git_dispatch_scalar(
        root, ("rev-parse", "--verify", claim_ref), "ref"
    )
    observed_tag = _git_dispatch_scalar(
        root, ("rev-parse", "--verify", f"{claim_ref}^{{tag}}"), "tag"
    )
    if observed_ref != ref_object_sha or observed_tag != tag_object_sha:
        raise SuccessorProduceV4Error("dispatch claim live ref object differs")
    if _git_dispatch_scalar(
        root, ("cat-file", "-t", tag_object_sha), "object type"
    ) != "tag":
        raise SuccessorProduceV4Error("dispatch claim live object type differs")
    raw_tag_size = _git_dispatch_scalar(
        root, ("cat-file", "-s", tag_object_sha), "tag size"
    )
    try:
        tag_size = int(raw_tag_size, 10)
    except ValueError as exc:
        raise SuccessorProduceV4Error(
            "dispatch claim live tag size differs"
        ) from exc
    if tag_size != len(raw_tag_object) or tag_size > 16_384:
        raise SuccessorProduceV4Error("dispatch claim live tag size differs")
    observed_raw_tag = _git_dispatch_read(
        root,
        ("cat-file", "tag", tag_object_sha),
        max_bytes=tag_size,
    )
    if observed_raw_tag != raw_tag_object:
        raise SuccessorProduceV4Error("dispatch claim live tag bytes differ")
    if _git_dispatch_scalar(
        root, ("rev-parse", "--verify", f"{claim_ref}^{{commit}}"), "target"
    ) != head_sha:
        raise SuccessorProduceV4Error("dispatch claim live target differs")

    require_same_repository()
    final_head = _git_dispatch_scalar(
        root, ("rev-parse", "--verify", "HEAD^{commit}"), "final HEAD"
    )
    final_ref = _git_dispatch_scalar(
        root, ("rev-parse", "--verify", claim_ref), "final ref"
    )
    require_same_repository()
    if final_head != head_sha or final_ref != tag_object_sha:
        raise SuccessorProduceV4Error("dispatch claim live repository changed")


def verify_dispatch_tag_object(
    chain: GenerationChain,
    *,
    repository_root: pathlib.Path,
    claim_ref: str,
    ref_object_sha: str,
    tag_object_sha: str,
    raw_tag_object: bytes,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> dict[str, str]:
    """Bind the fixed ref to one exact annotated tag and target commit."""

    expected_ref = dispatch_claim_ref(chain.attempt_id)
    if claim_ref != expected_ref:
        raise SuccessorProduceV4Error("dispatch claim ref differs")
    for label, value in (
        ("ref object", ref_object_sha),
        ("tag object", tag_object_sha),
    ):
        if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{40}", value) is None:
            raise SuccessorProduceV4Error(f"dispatch claim {label} SHA differs")
    if ref_object_sha != tag_object_sha:
        raise SuccessorProduceV4Error("dispatch claim ref object differs")
    if not isinstance(raw_tag_object, bytes) or not raw_tag_object:
        raise SuccessorProduceV4Error("dispatch claim annotated tag differs")
    if len(raw_tag_object) > 16_384 or b"\0" in raw_tag_object:
        raise SuccessorProduceV4Error("dispatch claim annotated tag differs")
    # This repository's object format is SHA-1, as also fixed by the 40-hex
    # commit and tag IDs above.  Trust still comes from the exact tag bytes,
    # live A6 SHA-256 and target commit checks; this reproduces Git's object ID.
    computed_sha = hashlib.sha1(
        b"tag "
        + str(len(raw_tag_object)).encode("ascii")
        + b"\0"
        + raw_tag_object
    ).hexdigest()
    if computed_sha != tag_object_sha:
        raise SuccessorProduceV4Error("dispatch claim tag object SHA differs")
    header, separator, raw_message = raw_tag_object.partition(b"\n\n")
    if not separator or b"\n\n" in header:
        raise SuccessorProduceV4Error("dispatch claim annotated tag differs")
    headers = header.split(b"\n")
    expected_tag_name = expected_ref[len("refs/tags/") :].encode("utf-8")
    expected_headers = (
        b"object " + head_sha.encode("ascii"),
        b"type commit",
        b"tag " + expected_tag_name,
    )
    if len(headers) != 4 or tuple(headers[:3]) != expected_headers:
        raise SuccessorProduceV4Error("dispatch claim annotated tag differs")
    if not headers[3].startswith(b"tagger ") or len(headers[3]) <= len(b"tagger "):
        raise SuccessorProduceV4Error("dispatch claim annotated tag differs")
    claim = verify_dispatch_claim(
        chain,
        claim_ref=claim_ref,
        raw_message=raw_message,
        github_run_id=github_run_id,
        github_run_attempt=github_run_attempt,
        workflow_path=workflow_path,
        head_sha=head_sha,
        head_a6_sha256=head_a6_sha256,
    )
    _verify_live_dispatch_repository(
        repository_root=repository_root,
        chain=chain,
        claim_ref=claim_ref,
        ref_object_sha=ref_object_sha,
        tag_object_sha=tag_object_sha,
        raw_tag_object=raw_tag_object,
        head_sha=head_sha,
    )
    return claim


def _verified_dispatch_capability(
    repository_root: pathlib.Path,
    chain: GenerationChain,
    capability: Optional[Mapping[str, Any]],
) -> DispatchCapability:
    """Freeze and verify the exact guard-created tag capability."""

    if capability is None:
        raise SuccessorProduceV4Error("production dispatch capability is required")
    expected_keys = {
        "claim_ref",
        "ref_object_sha",
        "tag_object_sha",
        "raw_tag_object",
        "github_run_id",
        "github_run_attempt",
        "workflow_path",
        "head_sha",
        "head_a6_sha256",
    }
    if not isinstance(capability, Mapping) or set(capability) != expected_keys:
        raise SuccessorProduceV4Error("production dispatch capability shape differs")
    raw_tag_object = capability["raw_tag_object"]
    if type(raw_tag_object) is not bytes:
        raise SuccessorProduceV4Error("production dispatch capability bytes differ")
    try:
        frozen = DispatchCapability(
            claim_ref=capability["claim_ref"],
            ref_object_sha=capability["ref_object_sha"],
            tag_object_sha=capability["tag_object_sha"],
            raw_tag_object=bytes(raw_tag_object),
            github_run_id=capability["github_run_id"],
            github_run_attempt=capability["github_run_attempt"],
            workflow_path=capability["workflow_path"],
            head_sha=capability["head_sha"],
            head_a6_sha256=capability["head_a6_sha256"],
        )
    except (KeyError, TypeError) as exc:
        raise SuccessorProduceV4Error(
            "production dispatch capability shape differs"
        ) from exc
    _reverify_dispatch_capability(repository_root, chain, frozen)
    return frozen


def _reverify_dispatch_capability(
    repository_root: pathlib.Path,
    chain: GenerationChain,
    capability: DispatchCapability,
) -> dict[str, str]:
    """Recheck frozen bytes without reopening a path or consulting the network."""

    if not isinstance(capability, DispatchCapability):
        raise SuccessorProduceV4Error("production dispatch capability differs")
    return verify_dispatch_tag_object(
        chain,
        repository_root=repository_root,
        claim_ref=capability.claim_ref,
        ref_object_sha=capability.ref_object_sha,
        tag_object_sha=capability.tag_object_sha,
        raw_tag_object=capability.raw_tag_object,
        github_run_id=capability.github_run_id,
        github_run_attempt=capability.github_run_attempt,
        workflow_path=capability.workflow_path,
        head_sha=capability.head_sha,
        head_a6_sha256=capability.head_a6_sha256,
    )


def verify_preregistered_generation(
    repository_root: pathlib.Path,
) -> PreregisteredGeneration:
    """Verify P2, its predecessors and the exact production-generation files."""

    root = pathlib.Path(repository_root).resolve()
    _require_absent(root, WITHDRAWN_A5_PATH, "withdrawn authority-v5")
    _require_absent(root, WITHDRAWN_RESULT_V5_PATH, "withdrawn result-v5")
    p2_id, p2 = _load_canonical(root, P2_PATH)
    try:
        p3_id, p3 = _load_canonical(root, P3_PATH)
    except SuccessorProduceV4Error as exc:
        raise SuccessorProduceV4Error(
            f"P3 dispatch-fence correction is invalid: {exc}"
        ) from exc
    r1_id, r1 = _load_canonical(root, R1_PATH)
    f5_id, f5 = _load_canonical(root, F5_PATH)
    for actual, expected, size, label in (
        (p2_id, P2_SHA256, P2_SIZE_BYTES, "P2"),
        (p3_id, P3_SHA256, P3_SIZE_BYTES, "P3"),
        (r1_id, R1_SHA256, R1_SIZE_BYTES, "R1"),
        (f5_id, F5_SHA256, F5_SIZE_BYTES, "F5"),
    ):
        if actual.sha256 != expected or actual.size_bytes != size:
            raise SuccessorProduceV4Error(f"{label} pinned identity differs")
    if p2.get("schema") != (
        "boole.native-shadow.mac3.launcher-v2-successor-production-generation-"
        "preregistration.arm64.v1"
    ):
        raise SuccessorProduceV4Error("P2 schema differs")
    if p2.get("status") != (
        "PRE-REGISTERED-PRODUCTION-GENERATION-NO-IMAGE-PRODUCTION-AUTHORITY"
    ):
        raise SuccessorProduceV4Error("P2 status differs")
    if p3.get("schema") != P3_SCHEMA or p3.get("status") != P3_STATUS:
        raise SuccessorProduceV4Error("P3 schema or status differs")
    _require_strict_equal(
        p3.get("authorisations"), ZERO_AUTHORISATIONS, "P3 authority"
    )
    _require_strict_equal(p3.get("runs"), P3_ZERO_RUNS, "P3 run accounting")
    attempt_pattern = (
        p3.get("claimFence", {})
        .get("repositoryClaim", {})
        .get("attemptIdPattern")
    )
    if attempt_pattern != P3_ATTEMPT_ID_PATTERN:
        raise SuccessorProduceV4Error("P3 attempt ID pattern differs")
    if r1.get("schema") != (
        "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal.arm64.v1"
    ) or r1.get("status") != R2_STATUS:
        raise SuccessorProduceV4Error("R1 is not the pinned authority-zero rehearsal")
    _require_strict_equal(r1.get("authorisations"), ZERO_AUTHORISATIONS, "R1 authority")
    if f5.get("schema") != (
        "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v5"
    ) or f5.get("status") != (
        "SEALED-AFTER-FREE-REHEARSAL-AUTHORITY-ZERO-HISTORICAL-EVIDENCE"
    ):
        raise SuccessorProduceV4Error("F5 is not the pinned historical fingerprint")
    _require_strict_equal(f5.get("authorisations"), ZERO_AUTHORISATIONS, "F5 authority")

    v4_identities = [_read_regular(root, path)[0] for path in V4_PATHS]
    reused_identities = [_read_regular(root, path)[0] for path in REUSED_PINNED_PATHS]
    declared_v4 = [row.get("path") for row in p2.get("futureGeneration", {}).get("newFiles", [])]
    if declared_v4 != list(V4_PATHS):
        raise SuccessorProduceV4Error("P2 does not declare the exact five v4 paths")
    declared_reused = [
        {key: row.get(key) for key in ("path", "sha256", "sizeBytes")}
        for row in p2.get("futureGeneration", {}).get("reusedPinnedUpstream", [])
        if isinstance(row, Mapping)
    ]
    if declared_reused != _identity_rows(reused_identities):
        raise SuccessorProduceV4Error("P2 reused upstream identities differ")
    return PreregisteredGeneration(
        preregistration=p2,
        rehearsal=r1,
        historical_fingerprint=f5,
        dispatch_fence_identity=p3_id,
        predecessor_identities=(p2_id, r1_id, f5_id),
        generation_identities=tuple(v4_identities),
        reused_identities=tuple(reused_identities),
    )


def verify_generation_chain(repository_root: pathlib.Path) -> GenerationChain:
    """Verify the acyclic production chain before any build or output effect."""

    root = pathlib.Path(repository_root).resolve()
    preregistered = verify_preregistered_generation(root)
    _require_absent(root, RESULT_V6_PATH, "future result-v6")
    p2_id, r1_id, f5_id = preregistered.predecessor_identities
    p2 = preregistered.preregistration
    r1 = preregistered.rehearsal
    f5 = preregistered.historical_fingerprint
    v4_identities = list(preregistered.generation_identities)
    reused_identities = list(preregistered.reused_identities)
    dispatch_fence = preregistered.dispatch_fence_identity.record()
    gate_identity = _read_regular(root, R2_GATE_PATH)[0]

    r2_id, r2 = _load_canonical(root, R2_PATH)
    r2_keys = {
        "activationAllowed", "authorisations", "bootableClaim", "boundInputs",
        "effects", "executionEnvelope", "generationFiles", "measurement",
        "predecessors", "repeatable", P3_FIELD, "reusedPinnedUpstream", "schema",
        "status",
    }
    _require_exact_keys(r2, r2_keys, "R2")
    if r2["schema"] != R2_SCHEMA or r2["status"] != R2_STATUS:
        raise SuccessorProduceV4Error("R2 schema or status differs")
    if r2["activationAllowed"] is not False or r2["bootableClaim"] is not False:
        raise SuccessorProduceV4Error("R2 claims activation or boot")
    _require_strict_equal(r2["authorisations"], ZERO_AUTHORISATIONS, "R2 authority")
    _require_strict_equal(r2["effects"], ZERO_EFFECTS, "R2 effect accounting")
    _require_strict_equal(
        r2["executionEnvelope"],
        _expected_cgroup_execution_envelope(),
        "R2 execution envelope",
    )
    _require_strict_equal(
        r2[P3_FIELD], dispatch_fence, "R2 dispatch-fence correction"
    )
    predecessors = _identity_rows((p2_id, r1_id, f5_id))
    generation = _identity_rows(v4_identities)
    reused = _identity_rows(reused_identities)
    if r2["predecessors"] != predecessors:
        raise SuccessorProduceV4Error("R2 predecessors differ")
    if r2["generationFiles"] != generation:
        raise SuccessorProduceV4Error("R2 does not bind the exact five v4 files")
    if r2["reusedPinnedUpstream"] != reused:
        raise SuccessorProduceV4Error("R2 reused upstream bindings differ")
    if r2["boundInputs"] != [*predecessors, *generation, *reused]:
        raise SuccessorProduceV4Error("R2 bound-input union differs")
    if r2["repeatable"] is not True or not isinstance(r2["measurement"], dict):
        raise SuccessorProduceV4Error("R2 rehearsal result differs")
    p1_identity, p1 = _load_canonical(root, P1_PATH)
    if (
        p1_identity.sha256 != P1_SHA256
        or p1_identity.size_bytes != P1_SIZE_BYTES
    ):
        raise SuccessorProduceV4Error("P1 identity differs for R2 measurement")
    try:
        expected_measurement = p1["expectedPreflight"]["measurement"]
    except (KeyError, TypeError) as exc:
        raise SuccessorProduceV4Error(
            "P1 has no sealed staging measurement"
        ) from exc
    _require_strict_equal(
        r2["measurement"], expected_measurement, "R2 staging measurement"
    )

    f6_id, f6 = _load_canonical(root, F6_PATH)
    f6_keys = {
        "authorisations", "boundaries", "files", "predecessors", "rehearsalGate",
        P3_FIELD, "schema", "status", "subject", "whatThisRecordDoesNotEstablish",
    }
    _require_exact_keys(f6, f6_keys, "F6")
    if f6["schema"] != F6_SCHEMA or f6["status"] != F6_STATUS:
        raise SuccessorProduceV4Error("F6 schema or status differs")
    _require_strict_equal(f6["authorisations"], ZERO_AUTHORISATIONS, "F6 authority")
    _require_strict_equal(
        f6[P3_FIELD], dispatch_fence, "F6 dispatch-fence correction"
    )
    _require_strict_equal(
        f6["boundaries"],
        {
            "activationAllowed": False,
            "bootableClaim": False,
            "servingClaim": False,
        },
        "F6 boundaries",
    )
    if f6["files"] != generation:
        raise SuccessorProduceV4Error("F6 does not bind the exact five v4 files")
    if f6["predecessors"] != [*predecessors, r2_id.record()]:
        raise SuccessorProduceV4Error("F6 predecessors differ")
    if f6["rehearsalGate"] != gate_identity.record():
        raise SuccessorProduceV4Error("F6 rehearsal gate differs")
    if not isinstance(f6["subject"], str) or not f6["subject"]:
        raise SuccessorProduceV4Error("F6 subject is empty")
    if not isinstance(f6["whatThisRecordDoesNotEstablish"], list):
        raise SuccessorProduceV4Error("F6 non-claims differ")

    a6_id, a6 = _load_canonical(root, A6_PATH)
    a6_keys = {
        "authorisations", "boundaries", "grant", "predecessors", "runs", "schema",
        P3_FIELD, "status", "subject",
    }
    _require_exact_keys(a6, a6_keys, "A6")
    if a6["schema"] != A6_SCHEMA or a6["status"] != A6_STATUS:
        raise SuccessorProduceV4Error("A6 schema or status differs")
    _require_strict_equal(
        a6[P3_FIELD], dispatch_fence, "A6 dispatch-fence correction"
    )
    expected_authorisations = dict(
        ZERO_AUTHORISATIONS,
        imageProductionAuthorised=True,
        imageProductionRunsAllowed=1,
    )
    if not _strictly_equal(a6["authorisations"], expected_authorisations):
        raise SuccessorProduceV4Error("A6 grants more or less than one image production")
    expected_runs = dict(ZERO_RUNS, imageProductionRunsAllowed=1)
    if not _strictly_equal(a6["runs"], expected_runs):
        raise SuccessorProduceV4Error("A6 run accounting differs")
    if a6["predecessors"] != [*predecessors, r2_id.record(), f6_id.record()]:
        raise SuccessorProduceV4Error("A6 predecessors differ")
    if not isinstance(a6["subject"], str) or not a6["subject"]:
        raise SuccessorProduceV4Error("A6 subject is empty")
    grant = a6["grant"]
    if not isinstance(grant, dict) or set(grant) != {
        "attemptId", "outputNames", "replicas", "resultPath",
        "workflowDispatchesAllowed", "workflowPath",
    }:
        raise SuccessorProduceV4Error("A6 grant shape differs")
    attempt_id = grant.get("attemptId")
    if not isinstance(attempt_id, str) or re.fullmatch(
        P3_ATTEMPT_ID_PATTERN, attempt_id
    ) is None:
        raise SuccessorProduceV4Error("A6 attempt ID violates the dispatch fence")
    dispatch_claim_ref(attempt_id)
    if grant.get("workflowPath") != V4_WORKFLOW_PATH:
        raise SuccessorProduceV4Error("A6 grants a different workflow")
    if type(grant.get("workflowDispatchesAllowed")) is not int or grant.get(
        "workflowDispatchesAllowed"
    ) != 1:
        raise SuccessorProduceV4Error("A6 dispatch count differs")
    if type(grant.get("replicas")) is not int or grant.get("replicas") != 2:
        raise SuccessorProduceV4Error("A6 replica count differs")
    if grant.get("resultPath") != RESULT_V6_PATH:
        raise SuccessorProduceV4Error("A6 result path differs")
    if grant.get("outputNames") != list(OUTPUT_NAMES):
        raise SuccessorProduceV4Error("A6 output names differ")
    if not _strictly_equal(a6["boundaries"], {
        "activationAllowed": False,
        "bootableClaim": False,
        "servingClaim": False,
    }):
        raise SuccessorProduceV4Error("A6 boundaries differ")

    return GenerationChain(
        preregistration=p2,
        rehearsal=r1,
        historical_fingerprint=f5,
        fresh_rehearsal=r2,
        fingerprint=f6,
        authority=a6,
        identities={
            "P2": p2_id,
            "R1": r1_id,
            "F5": f5_id,
            "R2": r2_id,
            "F6": f6_id,
            "A6": a6_id,
        },
        attempt_id=attempt_id,
        output_names=OUTPUT_NAMES,
    )


def _fsync_directory(directory: pathlib.Path) -> None:
    try:
        descriptor = os.open(str(directory), os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    except OSError as exc:
        raise SuccessorProduceV4Error(
            f"cannot make directory update durable: {directory}"
        ) from exc


def _publish_bytes_once(path: pathlib.Path, raw: bytes, *, mode: int = 0o444) -> None:
    """Publish complete bytes without replacing an existing name."""

    path = pathlib.Path(path)
    parent = path.parent
    try:
        info = parent.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(f"output parent is absent: {parent}") from exc
    if not stat.S_ISDIR(info.st_mode) or parent.is_symlink():
        raise SuccessorProduceV4Error(f"output parent is not a real directory: {parent}")
    if os.path.lexists(path):
        raise SuccessorProduceV4Error(f"create-once output already exists: {path.name}")
    handle = tempfile.NamedTemporaryFile(
        dir=str(parent), prefix=f".{path.name}.partial.", delete=False
    )
    partial = pathlib.Path(handle.name)
    try:
        with handle:
            handle.write(raw)
            handle.flush()
            os.fchmod(handle.fileno(), mode)
            os.fsync(handle.fileno())
        try:
            os.link(partial, path, follow_symlinks=False)
        except (FileExistsError, OSError) as exc:
            raise SuccessorProduceV4Error(
                f"create-once output cannot be published: {path.name}"
            ) from exc
        partial.unlink()
        _fsync_directory(parent)
    except BaseException:
        if os.path.lexists(partial):
            partial.unlink()
        raise


def _publish_json_once(path: pathlib.Path, document: Mapping[str, Any]) -> None:
    _publish_bytes_once(path, canonical_json(dict(document)))


def _read_exact_publication_at(
    directory_descriptor: int,
    name: str,
    *,
    expected_raw: bytes,
    expected_uid: int,
    expected_gid: int,
    allowed_modes: frozenset[int],
    allowed_links: frozenset[int],
) -> tuple[os.stat_result, bytes]:
    """Read one held create-once member without following or reopening a path."""

    descriptor: Optional[int] = None
    try:
        descriptor = os.open(
            name,
            _output_file_flags(),
            dir_fd=directory_descriptor,
        )
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_nlink not in allowed_links
            or before.st_uid != expected_uid
            or before.st_gid != expected_gid
            or stat.S_IMODE(before.st_mode) not in allowed_modes
            or before.st_size > len(expected_raw)
        ):
            raise SuccessorProduceV4Error(
                f"create-once publication identity differs: {name}"
            )
        raw = bytearray()
        while len(raw) <= len(expected_raw):
            chunk = os.read(
                descriptor,
                min(64 * 1024, len(expected_raw) + 1 - len(raw)),
            )
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        if (
            len(raw) != after.st_size
            or (
                before.st_dev,
                before.st_ino,
                before.st_mode,
                before.st_nlink,
                before.st_size,
                before.st_mtime_ns,
                before.st_ctime_ns,
            )
            != (
                after.st_dev,
                after.st_ino,
                after.st_mode,
                after.st_nlink,
                after.st_size,
                after.st_mtime_ns,
                after.st_ctime_ns,
            )
        ):
            raise SuccessorProduceV4Error(
                f"create-once publication changed while read: {name}"
            )
        reached = os.stat(
            name,
            dir_fd=directory_descriptor,
            follow_symlinks=False,
        )
        if (
            not stat.S_ISREG(reached.st_mode)
            or (reached.st_dev, reached.st_ino)
            != (after.st_dev, after.st_ino)
            or reached.st_nlink != after.st_nlink
            or reached.st_uid != expected_uid
            or reached.st_gid != expected_gid
            or stat.S_IMODE(reached.st_mode) not in allowed_modes
        ):
            raise SuccessorProduceV4Error(
                f"create-once publication path changed: {name}"
            )
        return after, bytes(raw)
    except OSError as exc:
        raise SuccessorProduceV4Error(
            f"create-once publication is unreadable: {name}"
        ) from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)


def _publish_exact_bytes_once_at(
    directory_descriptor: int,
    *,
    final_name: str,
    partial_name: str,
    expected_raw: bytes,
    expected_uid: int,
    expected_gid: int,
) -> None:
    """Converge one exact fixed-name publication across process crashes."""

    if (
        not isinstance(expected_raw, bytes)
        or not expected_raw
        or len(expected_raw) > MAX_METADATA_BYTES
    ):
        raise SuccessorProduceV4Error(
            "create-once publication bytes differ"
        )
    members = frozenset(os.listdir(directory_descriptor))
    final_present = final_name in members
    partial_present = partial_name in members

    final_info: Optional[os.stat_result] = None
    if final_present:
        final_info, final_raw = _read_exact_publication_at(
            directory_descriptor,
            final_name,
            expected_raw=expected_raw,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            allowed_modes=(
                frozenset((COLLECTABLE_FILE_MODE,))
                if partial_present
                else frozenset((0o400, COLLECTABLE_FILE_MODE))
            ),
            allowed_links=frozenset((2 if partial_present else 1,)),
        )
        if final_raw != expected_raw:
            raise SuccessorProduceV4Error(
                "create-once publication final claim differs"
            )
        if (
            not partial_present
            and stat.S_IMODE(final_info.st_mode) == 0o400
        ):
            descriptor = os.open(
                final_name,
                _output_file_flags(),
                dir_fd=directory_descriptor,
            )
            try:
                held = os.fstat(descriptor)
                if (held.st_dev, held.st_ino) != (
                    final_info.st_dev,
                    final_info.st_ino,
                ):
                    raise SuccessorProduceV4Error(
                        "create-once publication final identity changed"
                    )
                os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
            final_info, final_raw = _read_exact_publication_at(
                directory_descriptor,
                final_name,
                expected_raw=expected_raw,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                allowed_modes=frozenset((COLLECTABLE_FILE_MODE,)),
                allowed_links=frozenset((1,)),
            )
            if final_raw != expected_raw:
                raise SuccessorProduceV4Error(
                    "create-once publication sealed claim differs"
                )

    if partial_present:
        partial_info, partial_raw = _read_exact_publication_at(
            directory_descriptor,
            partial_name,
            expected_raw=expected_raw,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            allowed_modes=(
                frozenset((COLLECTABLE_FILE_MODE,))
                if final_present
                else frozenset((0o400, COLLECTABLE_FILE_MODE))
            ),
            allowed_links=frozenset((2 if final_present else 1,)),
        )
        if final_present:
            if (
                final_info is None
                or (partial_info.st_dev, partial_info.st_ino)
                != (final_info.st_dev, final_info.st_ino)
                or partial_raw != expected_raw
            ):
                raise SuccessorProduceV4Error(
                    "create-once publication linked identity differs"
                )
        elif partial_raw == expected_raw:
            descriptor = os.open(
                partial_name,
                _output_file_flags(),
                dir_fd=directory_descriptor,
            )
            try:
                os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
            partial_info, partial_raw = _read_exact_publication_at(
                directory_descriptor,
                partial_name,
                expected_raw=expected_raw,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                allowed_modes=frozenset((COLLECTABLE_FILE_MODE,)),
                allowed_links=frozenset((1,)),
            )
            if partial_raw != expected_raw:
                raise SuccessorProduceV4Error(
                    "create-once publication staged claim differs"
                )
        elif expected_raw.startswith(partial_raw):
            os.unlink(partial_name, dir_fd=directory_descriptor)
            os.fsync(directory_descriptor)
            partial_present = False
        else:
            raise SuccessorProduceV4Error(
                "create-once publication partial claim differs"
            )

    if not final_present:
        if not partial_present:
            flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
            if hasattr(os, "O_NOFOLLOW"):
                flags |= os.O_NOFOLLOW
            if hasattr(os, "O_CLOEXEC"):
                flags |= os.O_CLOEXEC
            descriptor: Optional[int] = None
            try:
                descriptor = os.open(
                    partial_name,
                    flags,
                    0o400,
                    dir_fd=directory_descriptor,
                )
                view = memoryview(expected_raw)
                while view:
                    written = os.write(descriptor, view)
                    if written <= 0:
                        raise OSError("short create-once publication write")
                    view = view[written:]
                os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
                os.fsync(descriptor)
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "create-once publication could not be staged"
                ) from exc
            finally:
                if descriptor is not None:
                    os.close(descriptor)
        try:
            os.link(
                partial_name,
                final_name,
                src_dir_fd=directory_descriptor,
                dst_dir_fd=directory_descriptor,
                follow_symlinks=False,
            )
            os.fsync(directory_descriptor)
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "create-once publication could not be linked"
            ) from exc

    if partial_name in frozenset(os.listdir(directory_descriptor)):
        os.unlink(partial_name, dir_fd=directory_descriptor)
        os.fsync(directory_descriptor)
    final_info, final_raw = _read_exact_publication_at(
        directory_descriptor,
        final_name,
        expected_raw=expected_raw,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        allowed_modes=frozenset((COLLECTABLE_FILE_MODE,)),
        allowed_links=frozenset((1,)),
    )
    del final_info
    if final_raw != expected_raw:
        raise SuccessorProduceV4Error(
            "create-once publication final claim differs"
        )


def _recovery_directory_flags() -> int:
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    return flags


def _open_absolute_recovery_directory(path: pathlib.Path, context: str) -> int:
    path = pathlib.Path(path)
    if not path.is_absolute() or ".." in path.parts:
        raise SuccessorProduceV4Error(f"{context} recovery path differs")
    descriptor: Optional[int] = None
    try:
        descriptor = os.open(os.path.sep, _recovery_directory_flags())
        for component in path.parts[1:]:
            next_descriptor = os.open(
                component,
                _recovery_directory_flags(),
                dir_fd=descriptor,
            )
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except OSError as exc:
        if descriptor is not None:
            os.close(descriptor)
        raise SuccessorProduceV4Error(
            f"{context} recovery path has a non-directory or symlink component"
        ) from exc


def _require_recovery_identity(
    value: tuple[int, int], context: str
) -> tuple[int, int]:
    if (
        not isinstance(value, tuple)
        or len(value) != 2
        or any(type(item) is not int or item < 0 for item in value)
    ):
        raise SuccessorProduceV4Error(f"{context} recovery identity differs")
    return value


def _require_recovery_directory(
    descriptor: int,
    *,
    inspected: os.stat_result,
    expected_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    context: str,
) -> os.stat_result:
    opened = os.fstat(descriptor)
    if not stat.S_ISDIR(inspected.st_mode) or not stat.S_ISDIR(opened.st_mode):
        raise SuccessorProduceV4Error(f"{context} recovery directory differs")
    inspected_identity = (inspected.st_dev, inspected.st_ino)
    opened_identity = (opened.st_dev, opened.st_ino)
    if inspected_identity != opened_identity or opened_identity != expected_identity:
        raise SuccessorProduceV4Error(f"{context} recovery identity differs")
    if opened.st_uid != expected_uid or opened.st_gid != expected_gid:
        raise SuccessorProduceV4Error(f"{context} recovery owner differs")
    if stat.S_IMODE(opened.st_mode) != 0o700:
        raise SuccessorProduceV4Error(f"{context} recovery mode differs")
    return opened


def _open_recovery_directory(
    path: pathlib.Path,
    *,
    expected_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    context: str,
) -> tuple[int, os.stat_result]:
    path = pathlib.Path(path)
    if not path.is_absolute() or ".." in path.parts:
        raise SuccessorProduceV4Error(f"{context} recovery path differs")
    try:
        inspected = path.lstat()
        descriptor = _open_absolute_recovery_directory(path, context)
    except OSError as exc:
        raise SuccessorProduceV4Error(
            f"{context} recovery directory is unavailable"
        ) from exc
    try:
        opened = _require_recovery_directory(
            descriptor,
            inspected=inspected,
            expected_identity=expected_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context=context,
        )
    except BaseException:
        os.close(descriptor)
        raise
    return descriptor, opened


def _open_recovery_staging(
    scratch_descriptor: int,
    *,
    expected_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
) -> tuple[int, os.stat_result]:
    try:
        inspected = os.stat(
            "staging", dir_fd=scratch_descriptor, follow_symlinks=False
        )
        descriptor = os.open(
            "staging", _recovery_directory_flags(), dir_fd=scratch_descriptor
        )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "staging mount recovery directory is unavailable"
        ) from exc
    try:
        opened = _require_recovery_directory(
            descriptor,
            inspected=inspected,
            expected_identity=expected_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="staging mount",
        )
    except BaseException:
        os.close(descriptor)
        raise
    return descriptor, opened


def _recheck_recovery_directory_names(
    *,
    scratch: pathlib.Path,
    scratch_descriptor: int,
    scratch_identity: tuple[int, int],
    staging_descriptor: int,
    staging_identity: tuple[int, int],
    outputs_parent: pathlib.Path,
    parent_descriptor: int,
    parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
) -> None:
    """Prove that every live name still reaches the directory held open."""

    try:
        scratch_reached = scratch.lstat()
        staging_reached = os.stat(
            "staging", dir_fd=scratch_descriptor, follow_symlinks=False
        )
        parent_reached = outputs_parent.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production recovery directory path changed"
        ) from exc
    _require_recovery_directory(
        scratch_descriptor,
        inspected=scratch_reached,
        expected_identity=scratch_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        context="scratch",
    )
    _require_recovery_directory(
        staging_descriptor,
        inspected=staging_reached,
        expected_identity=staging_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        context="staging mount",
    )
    _require_recovery_directory(
        parent_descriptor,
        inspected=parent_reached,
        expected_identity=parent_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        context="output parent",
    )


def _parse_tmpfs_size_bytes(value: str) -> int:
    match = re.fullmatch(r"([0-9]+)([kKmMgG]?)", value)
    if match is None:
        raise SuccessorProduceV4Error("production recovery tmpfs cap differs")
    factor = {
        "": 1,
        "k": 1024,
        "m": 1024 * 1024,
        "g": 1024 * 1024 * 1024,
    }[match.group(2).lower()]
    return int(match.group(1), 10) * factor


def _require_recovery_tmpfs_caps(super_options: list[str]) -> None:
    size_options = [
        option.removeprefix("size=")
        for option in super_options
        if option.startswith("size=")
    ]
    inode_options = [
        option.removeprefix("nr_inodes=")
        for option in super_options
        if option.startswith("nr_inodes=")
    ]
    if len(size_options) != 1 or len(inode_options) != 1:
        raise SuccessorProduceV4Error("production recovery tmpfs caps differ")
    if _parse_tmpfs_size_bytes(size_options[0]) != RECOVERY_TMPFS_SIZE_BYTES:
        raise SuccessorProduceV4Error("production recovery tmpfs caps differ")
    if re.fullmatch(r"[0-9]+", inode_options[0]) is None:
        raise SuccessorProduceV4Error("production recovery tmpfs caps differ")
    if int(inode_options[0], 10) != RECOVERY_TMPFS_INODES:
        raise SuccessorProduceV4Error("production recovery tmpfs caps differ")


def _normalise_recovery_mount_identity(
    mount_identity: Mapping[str, Any],
    *,
    recovery_stem: str,
    staging_path: pathlib.Path,
) -> dict[str, Any]:
    if re.fullmatch(r"boole-nsv4-[0-9a-f]{40}-r[12]", recovery_stem) is None:
        raise SuccessorProduceV4Error("production recovery stem differs")
    if not isinstance(mount_identity, Mapping):
        raise SuccessorProduceV4Error("production recovery mount differs")
    required_keys = {
        "fileSystemType",
        "majorMinor",
        "mountId",
        "mountOptions",
        "mountPoint",
        "parentId",
        "root",
        "source",
        "superOptions",
    }
    if set(mount_identity) != required_keys:
        raise SuccessorProduceV4Error("production recovery mount fields differ")
    document = dict(mount_identity)
    scalar_names = required_keys - {"mountOptions", "superOptions"}
    if any(type(document[name]) is not str for name in scalar_names):
        raise SuccessorProduceV4Error("production recovery mount value differs")
    if document["fileSystemType"] != "tmpfs":
        raise SuccessorProduceV4Error("production recovery mount type differs")
    if re.fullmatch(r"[0-9]+:[0-9]+", document["majorMinor"]) is None:
        raise SuccessorProduceV4Error("production recovery mount device differs")
    for name in ("mountId", "parentId"):
        if re.fullmatch(r"[0-9]+", document[name]) is None:
            raise SuccessorProduceV4Error("production recovery mount id differs")
    if document["mountId"] == document["parentId"]:
        raise SuccessorProduceV4Error("production recovery mount parent differs")
    if document["mountPoint"] != str(staging_path):
        raise SuccessorProduceV4Error("production recovery mount point differs")
    if document["root"] != "/":
        raise SuccessorProduceV4Error("production recovery mount root differs")
    if document["source"] != recovery_stem:
        raise SuccessorProduceV4Error("production recovery mount source differs")
    for name in ("mountOptions", "superOptions"):
        options = document[name]
        if (
            not isinstance(options, list)
            or not options
            or any(
                type(option) is not str
                or not option
                or "," in option
                or any(ord(character) < 33 or ord(character) > 126 for character in option)
                for option in options
            )
            or options != sorted(set(options), key=os.fsencode)
        ):
            raise SuccessorProduceV4Error(
                f"production recovery mount {name} differ"
            )
    mount_options = set(document["mountOptions"])
    if not {"rw", "nodev", "nosuid"}.issubset(mount_options) or {
        "ro",
        "dev",
        "suid",
    } & mount_options:
        raise SuccessorProduceV4Error("production recovery mount options differ")
    _require_recovery_tmpfs_caps(document["superOptions"])
    return document


def _decode_recovery_mount_field(value: str) -> str:
    for encoded, decoded in (
        ("\\040", " "),
        ("\\011", "\t"),
        ("\\012", "\n"),
        ("\\134", "\\"),
    ):
        value = value.replace(encoded, decoded)
    return value


def _parse_live_recovery_mountinfo(
    raw: bytes,
    staging_path: pathlib.Path,
) -> list[dict[str, Any]]:
    if type(raw) is not bytes:
        raise SuccessorProduceV4Error(
            "live production recovery mountinfo bytes differ"
        )
    if len(raw) > MAX_MOUNTINFO_BYTES:
        raise SuccessorProduceV4Error(
            "live production recovery mountinfo exceeds byte limit"
        )
    try:
        lines = raw.decode("utf-8").splitlines()
    except UnicodeDecodeError as exc:
        raise SuccessorProduceV4Error(
            "live production recovery mountinfo is not UTF-8"
        ) from exc
    target = str(staging_path)
    matches: list[dict[str, Any]] = []
    for line in lines:
        fields = line.split(" ")
        try:
            separator = fields.index("-")
        except ValueError as exc:
            raise SuccessorProduceV4Error(
                "live production recovery mountinfo line has no separator"
            ) from exc
        if separator < 6 or len(fields) < separator + 4:
            raise SuccessorProduceV4Error(
                "live production recovery mountinfo line shape differs"
            )
        mount_point = _decode_recovery_mount_field(fields[4])
        if mount_point != target:
            continue
        matches.append(
            {
                "fileSystemType": fields[separator + 1],
                "majorMinor": fields[2],
                "mountId": fields[0],
                "mountOptions": sorted(fields[5].split(","), key=os.fsencode),
                "mountPoint": mount_point,
                "parentId": fields[1],
                "root": _decode_recovery_mount_field(fields[3]),
                "source": _decode_recovery_mount_field(fields[separator + 2]),
                "superOptions": sorted(
                    fields[separator + 3].split(","), key=os.fsencode
                ),
            }
        )
    return matches


def _read_live_recovery_mount_matches(
    staging_path: pathlib.Path,
) -> list[dict[str, Any]]:
    flags = os.O_RDONLY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    descriptor: Optional[int] = None
    try:
        descriptor = os.open("/proc/self/mountinfo", flags)
        raw = bytearray()
        while len(raw) <= MAX_MOUNTINFO_BYTES:
            chunk = os.read(
                descriptor,
                min(64 * 1024, MAX_MOUNTINFO_BYTES + 1 - len(raw)),
            )
            if not chunk:
                break
            raw.extend(chunk)
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "live production recovery mountinfo is unavailable"
        ) from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)
    return _parse_live_recovery_mountinfo(bytes(raw), staging_path)


def _read_live_recovery_mount_identity(staging_path: pathlib.Path) -> dict[str, Any]:
    matches = _read_live_recovery_mount_matches(staging_path)
    if len(matches) != 1:
        raise SuccessorProduceV4Error(
            "the exact live production recovery mount differs"
        )
    return matches[0]


def _require_absent_recovery_mount(staging_path: pathlib.Path) -> None:
    if _read_live_recovery_mount_matches(staging_path):
        raise SuccessorProduceV4Error(
            "production recovery mount remains after unmount"
        )


def _require_live_recovery_mount(
    *,
    staging_path: pathlib.Path,
    staging_info: os.stat_result,
    recovery_stem: str,
    expected: Mapping[str, Any],
) -> None:
    expected_device = f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}"
    if expected.get("majorMinor") != expected_device:
        raise SuccessorProduceV4Error(
            "production recovery mount device does not match staging"
        )
    observed = _normalise_recovery_mount_identity(
        _read_live_recovery_mount_identity(staging_path),
        recovery_stem=recovery_stem,
        staging_path=staging_path,
    )
    _require_strict_equal(
        observed,
        dict(expected),
        "live production recovery mount identity differs",
    )


def _recovery_record_document(
    *,
    scratch: pathlib.Path,
    scratch_info: os.stat_result,
    staging_info: os.stat_result,
    outputs_parent: pathlib.Path,
    parent_info: os.stat_result,
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
    mount_identity: Mapping[str, Any],
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "bootableClaim": False,
        "directories": {
            "outputsParent": {
                "device": parent_info.st_dev,
                "inode": parent_info.st_ino,
                "modeOctal": "0700",
                "path": str(outputs_parent),
            },
            "scratch": {
                "device": scratch_info.st_dev,
                "inode": scratch_info.st_ino,
                "modeOctal": "0700",
                "path": str(scratch),
            },
            "staging": {
                "device": staging_info.st_dev,
                "inode": staging_info.st_ino,
                "modeOctal": "0700",
                "path": str(scratch / "staging"),
            },
        },
        "mountIdentity": dict(mount_identity),
        "owner": {"gid": expected_gid, "uid": expected_uid},
        "outputs": {
            "leafName": "outputs",
            "path": str(outputs_parent / "outputs"),
        },
        "recordName": RECOVERY_RECORD_NAME,
        "recoveryStem": recovery_stem,
        "schema": RECOVERY_RECORD_SCHEMA,
        "units": {
            "preflight": f"{recovery_stem}-preflight.service",
            "produce": f"{recovery_stem}-produce.service",
            "qualify": f"{recovery_stem}-qualify.service",
        },
        "writtenBefore": [
            "staging child directories",
            "transient systemd units",
            "attempt-consumed marker",
            "image output files",
        ],
    }


def _publish_recovery_record_at(
    scratch_descriptor: int, document: Mapping[str, Any]
) -> None:
    raw = canonical_json(dict(document))
    if len(raw) > MAX_RECOVERY_RECORD_BYTES:
        raise SuccessorProduceV4Error("production recovery record exceeds byte limit")
    partial_name = f".{RECOVERY_RECORD_NAME}.partial"
    members = frozenset(os.listdir(scratch_descriptor))
    if RECOVERY_RECORD_NAME in members:
        raise SuccessorProduceV4Error(
            "create-once recovery record already exists"
        )
    if members != frozenset(("staging",)):
        raise SuccessorProduceV4Error("production recovery scratch members differ")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    descriptor: Optional[int] = None
    linked = False
    try:
        descriptor = os.open(partial_name, flags, 0o400, dir_fd=scratch_descriptor)
        view = memoryview(raw)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise OSError("short recovery record write")
            view = view[written:]
        os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.link(
            partial_name,
            RECOVERY_RECORD_NAME,
            src_dir_fd=scratch_descriptor,
            dst_dir_fd=scratch_descriptor,
            follow_symlinks=False,
        )
        linked = True
        os.unlink(partial_name, dir_fd=scratch_descriptor)
        os.fsync(scratch_descriptor)
    except (FileExistsError, OSError) as exc:
        raise SuccessorProduceV4Error(
            "create-once recovery record could not be published"
        ) from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if not linked:
            try:
                os.unlink(partial_name, dir_fd=scratch_descriptor)
            except OSError:
                pass


def _read_recovery_record_at(
    scratch_descriptor: int,
    *,
    expected_uid: int,
    expected_gid: int,
    allowed_links: frozenset[int] = frozenset((1,)),
) -> dict[str, Any]:
    try:
        descriptor = os.open(
            RECOVERY_RECORD_NAME,
            _output_file_flags(),
            dir_fd=scratch_descriptor,
        )
    except OSError as exc:
        raise SuccessorProduceV4Error("production recovery record is absent") from exc
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_nlink not in allowed_links
            or before.st_uid != expected_uid
            or before.st_gid != expected_gid
            or stat.S_IMODE(before.st_mode) != COLLECTABLE_FILE_MODE
            or before.st_size > MAX_RECOVERY_RECORD_BYTES
        ):
            raise SuccessorProduceV4Error(
                "production recovery record mode or identity differs"
            )
        raw = bytearray()
        while len(raw) <= MAX_RECOVERY_RECORD_BYTES:
            chunk = os.read(
                descriptor,
                min(
                    64 * 1024,
                    MAX_RECOVERY_RECORD_BYTES + 1 - len(raw),
                ),
            )
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        if (
            len(raw) > MAX_RECOVERY_RECORD_BYTES
            or len(raw) != after.st_size
            or (
                before.st_dev,
                before.st_ino,
                before.st_mode,
                before.st_size,
                before.st_mtime_ns,
                before.st_ctime_ns,
            )
            != (
                after.st_dev,
                after.st_ino,
                after.st_mode,
                after.st_size,
                after.st_mtime_ns,
                after.st_ctime_ns,
            )
        ):
            raise SuccessorProduceV4Error(
                "production recovery record changed while read"
            )
        try:
            document = json.loads(bytes(raw).decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SuccessorProduceV4Error(
                "production recovery record is not JSON"
            ) from exc
        if not isinstance(document, dict) or bytes(raw) != canonical_json(document):
            raise SuccessorProduceV4Error(
                "production recovery record is not canonical"
            )
        try:
            reached = os.stat(
                RECOVERY_RECORD_NAME,
                dir_fd=scratch_descriptor,
                follow_symlinks=False,
            )
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "production recovery record path changed"
            ) from exc
        if (
            not stat.S_ISREG(reached.st_mode)
            or (reached.st_dev, reached.st_ino) != (after.st_dev, after.st_ino)
            or reached.st_nlink not in allowed_links
            or reached.st_uid != expected_uid
            or reached.st_gid != expected_gid
            or stat.S_IMODE(reached.st_mode) != COLLECTABLE_FILE_MODE
        ):
            raise SuccessorProduceV4Error(
                "production recovery record path identity differs"
            )
        return document
    finally:
        os.close(descriptor)


def _validate_recovery_record_partial(
    scratch_descriptor: int,
    *,
    partial_name: str,
    expected_uid: int,
    expected_gid: int,
    expected_inode: Optional[tuple[int, int]] = None,
    expected_links: int = 1,
) -> os.stat_result:
    try:
        info = os.stat(
            partial_name,
            dir_fd=scratch_descriptor,
            follow_symlinks=False,
        )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production recovery record partial changed"
        ) from exc
    if (
        not stat.S_ISREG(info.st_mode)
        or info.st_nlink != expected_links
        or info.st_uid != expected_uid
        or info.st_gid != expected_gid
        or stat.S_IMODE(info.st_mode) not in (0o400, COLLECTABLE_FILE_MODE)
        or info.st_size > MAX_RECOVERY_RECORD_BYTES
        or (
            expected_inode is not None
            and (info.st_dev, info.st_ino) != expected_inode
        )
    ):
        raise SuccessorProduceV4Error(
            "production recovery record partial identity differs"
        )
    return info


def reconcile_production_recovery_record_publication(
    *,
    scratch: pathlib.Path,
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
    mount_identity: Mapping[str, Any],
) -> str:
    """Converge the two create-once record publication crash boundaries."""

    if pathlib.Path(scratch).name != recovery_stem:
        raise SuccessorProduceV4Error("production recovery stem differs")
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    scratch_descriptor: Optional[int] = None
    staging_descriptor: Optional[int] = None
    parent_descriptor: Optional[int] = None
    try:
        scratch_info = pathlib.Path(scratch).lstat()
        staging_info = pathlib.Path(scratch, "staging").lstat()
        scratch_descriptor, opened_scratch = _open_recovery_directory(
            pathlib.Path(scratch),
            expected_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        staging_descriptor, opened_staging = _open_recovery_staging(
            scratch_descriptor,
            expected_identity=(staging_info.st_dev, staging_info.st_ino),
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        parent_descriptor, opened_parent = _open_recovery_directory(
            pathlib.Path(outputs_parent),
            expected_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="output parent",
        )
        normalised_mount = _normalise_recovery_mount_identity(
            mount_identity,
            recovery_stem=recovery_stem,
            staging_path=pathlib.Path(scratch, "staging"),
        )
        _require_live_recovery_mount(
            staging_path=pathlib.Path(scratch, "staging"),
            staging_info=opened_staging,
            recovery_stem=recovery_stem,
            expected=normalised_mount,
        )
        partial_name = f".{RECOVERY_RECORD_NAME}.partial"
        members = frozenset(os.listdir(scratch_descriptor))
        base = frozenset(("staging",))
        record_present = RECOVERY_RECORD_NAME in members
        partial_present = partial_name in members
        allowed = {
            base,
            frozenset((*base, partial_name)),
            frozenset((*base, RECOVERY_RECORD_NAME)),
            frozenset((*base, RECOVERY_RECORD_NAME, partial_name)),
        }
        if members not in allowed:
            raise SuccessorProduceV4Error(
                "production recovery record publication members differ"
            )
        if not record_present:
            if os.listdir(staging_descriptor) or os.listdir(parent_descriptor):
                raise SuccessorProduceV4Error(
                    "incomplete production recovery acquired child state"
                )
            if partial_present:
                _validate_recovery_record_partial(
                    scratch_descriptor,
                    partial_name=partial_name,
                    expected_uid=expected_uid,
                    expected_gid=expected_gid,
                )
                os.unlink(partial_name, dir_fd=scratch_descriptor)
                os.fsync(scratch_descriptor)
            _recheck_recovery_directory_names(
                scratch=pathlib.Path(scratch),
                scratch_descriptor=scratch_descriptor,
                scratch_identity=(opened_scratch.st_dev, opened_scratch.st_ino),
                staging_descriptor=staging_descriptor,
                staging_identity=(opened_staging.st_dev, opened_staging.st_ino),
                outputs_parent=pathlib.Path(outputs_parent),
                parent_descriptor=parent_descriptor,
                parent_identity=(opened_parent.st_dev, opened_parent.st_ino),
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
            if frozenset(os.listdir(scratch_descriptor)) != base:
                raise SuccessorProduceV4Error(
                    "incomplete production recovery members changed"
                )
            return "incomplete-no-record"

        document = _read_recovery_record_at(
            scratch_descriptor,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            allowed_links=(
                frozenset((2,)) if partial_present else frozenset((1,))
            ),
        )
        expected_document = _recovery_record_document(
            scratch=pathlib.Path(scratch),
            scratch_info=opened_scratch,
            staging_info=opened_staging,
            outputs_parent=pathlib.Path(outputs_parent),
            parent_info=opened_parent,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            recovery_stem=recovery_stem,
            mount_identity=normalised_mount,
        )
        _require_strict_equal(
            document,
            expected_document,
            "production recovery record",
        )
        if partial_present:
            record_info = os.stat(
                RECOVERY_RECORD_NAME,
                dir_fd=scratch_descriptor,
                follow_symlinks=False,
            )
            _validate_recovery_record_partial(
                scratch_descriptor,
                partial_name=partial_name,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                expected_inode=(record_info.st_dev, record_info.st_ino),
                expected_links=2,
            )
            os.unlink(partial_name, dir_fd=scratch_descriptor)
            os.fsync(scratch_descriptor)
            _read_recovery_record_at(
                scratch_descriptor,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
        _recheck_recovery_directory_names(
            scratch=pathlib.Path(scratch),
            scratch_descriptor=scratch_descriptor,
            scratch_identity=(opened_scratch.st_dev, opened_scratch.st_ino),
            staging_descriptor=staging_descriptor,
            staging_identity=(opened_staging.st_dev, opened_staging.st_ino),
            outputs_parent=pathlib.Path(outputs_parent),
            parent_descriptor=parent_descriptor,
            parent_identity=(opened_parent.st_dev, opened_parent.st_ino),
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        return "record-ready"
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production recovery record publication changed"
        ) from exc
    finally:
        if parent_descriptor is not None:
            os.close(parent_descriptor)
        if staging_descriptor is not None:
            os.close(staging_descriptor)
        if scratch_descriptor is not None:
            os.close(scratch_descriptor)


def discard_incomplete_production_recovery(
    *,
    scratch: pathlib.Path,
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
) -> str:
    """Remove only a pre-record, already-unmounted claim-bound scratch tree."""

    scratch = pathlib.Path(scratch)
    if scratch.name != recovery_stem or re.fullmatch(
        r"boole-nsv4-[0-9a-f]{40}-r[12]", recovery_stem
    ) is None:
        raise SuccessorProduceV4Error("production recovery stem differs")
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    output_descriptor, _ = _open_recovery_directory(
        pathlib.Path(outputs_parent),
        expected_identity=parent_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        context="output parent",
    )
    if os.listdir(output_descriptor):
        os.close(output_descriptor)
        raise SuccessorProduceV4Error(
            "incomplete production recovery has output state"
        )
    os.close(output_descriptor)
    if not os.path.lexists(scratch):
        return "already-absent"
    try:
        scratch_info = scratch.lstat()
        scratch_descriptor, opened_scratch = _open_recovery_directory(
            scratch,
            expected_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        recovery_root_descriptor = _open_absolute_recovery_directory(
            scratch.parent, "production recovery root"
        )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "incomplete production recovery directory is unavailable"
        ) from exc
    staging_descriptor: Optional[int] = None
    try:
        partial_name = f".{RECOVERY_RECORD_NAME}.partial"
        members = frozenset(os.listdir(scratch_descriptor))
        allowed = {
            frozenset(),
            frozenset(("staging",)),
            frozenset((partial_name,)),
            frozenset(("staging", partial_name)),
        }
        if members not in allowed:
            raise SuccessorProduceV4Error(
                "incomplete production recovery members differ"
            )
        if partial_name in members:
            _validate_recovery_record_partial(
                scratch_descriptor,
                partial_name=partial_name,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
        if "staging" in members:
            staging_info = os.stat(
                "staging", dir_fd=scratch_descriptor, follow_symlinks=False
            )
            staging_descriptor, opened_staging = _open_recovery_staging(
                scratch_descriptor,
                expected_identity=(staging_info.st_dev, staging_info.st_ino),
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
            if opened_staging.st_dev != opened_scratch.st_dev:
                raise SuccessorProduceV4Error(
                    "incomplete production staging is still mounted"
                )
            if os.listdir(staging_descriptor):
                raise SuccessorProduceV4Error(
                    "incomplete production staging is not empty"
                )
        reached = os.stat(
            recovery_stem,
            dir_fd=recovery_root_descriptor,
            follow_symlinks=False,
        )
        if (
            not stat.S_ISDIR(reached.st_mode)
            or (reached.st_dev, reached.st_ino)
            != (opened_scratch.st_dev, opened_scratch.st_ino)
        ):
            raise SuccessorProduceV4Error(
                "incomplete production recovery path changed"
            )
        if partial_name in members:
            os.unlink(partial_name, dir_fd=scratch_descriptor)
            os.fsync(scratch_descriptor)
        if staging_descriptor is not None:
            os.close(staging_descriptor)
            staging_descriptor = None
            os.rmdir("staging", dir_fd=scratch_descriptor)
            os.fsync(scratch_descriptor)
        if os.listdir(scratch_descriptor):
            raise SuccessorProduceV4Error(
                "incomplete production recovery members changed"
            )
        os.rmdir(recovery_stem, dir_fd=recovery_root_descriptor)
        os.fsync(recovery_root_descriptor)
        return "discarded-incomplete"
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "incomplete production recovery could not be discarded"
        ) from exc
    finally:
        if staging_descriptor is not None:
            os.close(staging_descriptor)
        os.close(recovery_root_descriptor)
        os.close(scratch_descriptor)


def _read_cleanup_checkpoint_at(
    scratch_descriptor: int,
    *,
    expected_uid: int,
    expected_gid: int,
) -> dict[str, Any]:
    try:
        descriptor = os.open(
            RECOVERY_CLEANUP_CHECKPOINT_NAME,
            _output_file_flags(),
            dir_fd=scratch_descriptor,
        )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production cleanup checkpoint is absent"
        ) from exc
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_nlink != 1
            or before.st_uid != expected_uid
            or before.st_gid != expected_gid
            or stat.S_IMODE(before.st_mode) != COLLECTABLE_FILE_MODE
            or before.st_size > MAX_RECOVERY_RECORD_BYTES
        ):
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint mode or identity differs"
            )
        raw = bytearray()
        while len(raw) <= MAX_RECOVERY_RECORD_BYTES:
            chunk = os.read(
                descriptor,
                min(64 * 1024, MAX_RECOVERY_RECORD_BYTES + 1 - len(raw)),
            )
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        before_key = (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        )
        after_key = (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        if (
            len(raw) > MAX_RECOVERY_RECORD_BYTES
            or len(raw) != after.st_size
            or before_key != after_key
        ):
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint changed while read"
            )
        try:
            document = json.loads(bytes(raw).decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint is not JSON"
            ) from exc
        if not isinstance(document, dict) or bytes(raw) != canonical_json(document):
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint is not canonical"
            )
        reached = os.stat(
            RECOVERY_CLEANUP_CHECKPOINT_NAME,
            dir_fd=scratch_descriptor,
            follow_symlinks=False,
        )
        if (
            not stat.S_ISREG(reached.st_mode)
            or (reached.st_dev, reached.st_ino) != (after.st_dev, after.st_ino)
            or reached.st_nlink != 1
            or reached.st_uid != expected_uid
            or reached.st_gid != expected_gid
            or stat.S_IMODE(reached.st_mode) != COLLECTABLE_FILE_MODE
        ):
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint path identity differs"
            )
        return document
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production cleanup checkpoint path changed"
        ) from exc
    finally:
        os.close(descriptor)


def _cleanup_checkpoint_document(
    *,
    scratch: pathlib.Path,
    scratch_info: os.stat_result,
    outputs_parent: pathlib.Path,
    parent_info: os.stat_result,
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
    recovery_record: Mapping[str, Any],
) -> dict[str, Any]:
    recovery_raw = canonical_json(dict(recovery_record))
    return {
        "activationAllowed": False,
        "bootableClaim": False,
        "directories": {
            "outputsParent": {
                "device": parent_info.st_dev,
                "inode": parent_info.st_ino,
                "path": str(outputs_parent),
            },
            "scratch": {
                "device": scratch_info.st_dev,
                "inode": scratch_info.st_ino,
                "path": str(scratch),
            },
        },
        "outputs": {
            "leafName": "outputs",
            "path": str(outputs_parent / "outputs"),
        },
        "owner": {"gid": expected_gid, "uid": expected_uid},
        "recoveryRecord": {
            "name": RECOVERY_RECORD_NAME,
            "sha256": _sha256(recovery_raw),
            "sizeBytes": len(recovery_raw),
        },
        "recoveryStem": recovery_stem,
        "schema": RECOVERY_CLEANUP_CHECKPOINT_SCHEMA,
        "status": "LIVE-MOUNT-VERIFIED-BEFORE-UNMOUNT",
        "unitsStoppedBeforeThisCheckpoint": True,
        "writtenBefore": "the production tmpfs unmount",
    }


def _publish_cleanup_checkpoint_at(
    scratch_descriptor: int, document: Mapping[str, Any]
) -> None:
    raw = canonical_json(dict(document))
    if len(raw) > MAX_RECOVERY_RECORD_BYTES:
        raise SuccessorProduceV4Error(
            "production cleanup checkpoint exceeds byte limit"
        )
    partial_name = f".{RECOVERY_CLEANUP_CHECKPOINT_NAME}.partial"
    members = frozenset(os.listdir(scratch_descriptor))
    if RECOVERY_CLEANUP_CHECKPOINT_NAME in members:
        observed = _read_cleanup_checkpoint_at(
            scratch_descriptor,
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
        )
        _require_strict_equal(
            observed,
            dict(document),
            "production cleanup checkpoint differs",
        )
        return
    if members == frozenset(("staging", RECOVERY_RECORD_NAME, partial_name)):
        os.unlink(partial_name, dir_fd=scratch_descriptor)
        os.fsync(scratch_descriptor)
        members = frozenset(("staging", RECOVERY_RECORD_NAME))
    if members != frozenset(("staging", RECOVERY_RECORD_NAME)):
        raise SuccessorProduceV4Error(
            "production cleanup checkpoint scratch members differ"
        )
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    descriptor: Optional[int] = None
    linked = False
    try:
        descriptor = os.open(partial_name, flags, 0o400, dir_fd=scratch_descriptor)
        view = memoryview(raw)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise OSError("short cleanup checkpoint write")
            view = view[written:]
        os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.link(
            partial_name,
            RECOVERY_CLEANUP_CHECKPOINT_NAME,
            src_dir_fd=scratch_descriptor,
            dst_dir_fd=scratch_descriptor,
            follow_symlinks=False,
        )
        linked = True
        os.unlink(partial_name, dir_fd=scratch_descriptor)
        os.fsync(scratch_descriptor)
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production cleanup checkpoint could not be published"
        ) from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if not linked:
            try:
                os.unlink(partial_name, dir_fd=scratch_descriptor)
            except OSError:
                pass


def _reconcile_cleanup_checkpoint_partial(
    *,
    scratch: pathlib.Path,
    expected_uid: int,
    expected_gid: int,
) -> None:
    """Converge only the two known crash windows of checkpoint publication."""

    scratch = pathlib.Path(scratch)
    scratch_stat = scratch.lstat()
    scratch_identity = (scratch_stat.st_dev, scratch_stat.st_ino)
    descriptor: Optional[int] = None
    partial_name = f".{RECOVERY_CLEANUP_CHECKPOINT_NAME}.partial"
    try:
        descriptor, _ = _open_recovery_directory(
            scratch,
            expected_identity=scratch_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        members = frozenset(os.listdir(descriptor))
        base = frozenset(("staging", RECOVERY_RECORD_NAME))
        if members in (
            base,
            frozenset((*base, RECOVERY_CLEANUP_CHECKPOINT_NAME)),
        ):
            return
        partial_only = frozenset((*base, partial_name))
        linked_pair = frozenset(
            (*base, partial_name, RECOVERY_CLEANUP_CHECKPOINT_NAME)
        )
        if members not in (partial_only, linked_pair):
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint partial state differs"
            )
        partial_info = os.stat(
            partial_name, dir_fd=descriptor, follow_symlinks=False
        )
        if (
            not stat.S_ISREG(partial_info.st_mode)
            or partial_info.st_uid != expected_uid
            or partial_info.st_gid != expected_gid
            or stat.S_IMODE(partial_info.st_mode) not in (0o400, 0o444)
            or partial_info.st_size > MAX_RECOVERY_RECORD_BYTES
        ):
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint partial identity differs"
            )
        if members == linked_pair:
            final_info = os.stat(
                RECOVERY_CLEANUP_CHECKPOINT_NAME,
                dir_fd=descriptor,
                follow_symlinks=False,
            )
            if (
                not stat.S_ISREG(final_info.st_mode)
                or (final_info.st_dev, final_info.st_ino)
                != (partial_info.st_dev, partial_info.st_ino)
                or final_info.st_nlink != 2
                or partial_info.st_nlink != 2
                or final_info.st_uid != expected_uid
                or final_info.st_gid != expected_gid
                or stat.S_IMODE(final_info.st_mode) != COLLECTABLE_FILE_MODE
                or final_info.st_size > MAX_RECOVERY_RECORD_BYTES
            ):
                raise SuccessorProduceV4Error(
                    "production cleanup checkpoint linked partial differs"
                )
        elif partial_info.st_nlink != 1:
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint partial link count differs"
            )
        os.unlink(partial_name, dir_fd=descriptor)
        os.fsync(descriptor)
        reached = scratch.lstat()
        if (reached.st_dev, reached.st_ino) != scratch_identity:
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint scratch path changed"
            )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production cleanup checkpoint partial recovery failed"
        ) from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)


def publish_production_recovery_record(
    *,
    scratch: pathlib.Path,
    expected_scratch_identity: tuple[int, int],
    expected_staging_identity: tuple[int, int],
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
    mount_identity: Mapping[str, Any],
) -> dict[str, Any]:
    """Create the durable identity record before any transient child runs."""

    scratch = pathlib.Path(scratch)
    outputs_parent = pathlib.Path(outputs_parent)
    scratch_identity = _require_recovery_identity(
        expected_scratch_identity, "scratch"
    )
    staging_identity = _require_recovery_identity(
        expected_staging_identity, "staging mount"
    )
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    if type(expected_uid) is not int or expected_uid < 0:
        raise SuccessorProduceV4Error("production recovery uid differs")
    if type(expected_gid) is not int or expected_gid < 0:
        raise SuccessorProduceV4Error("production recovery gid differs")
    normalised_mount = _normalise_recovery_mount_identity(
        mount_identity,
        recovery_stem=recovery_stem,
        staging_path=scratch / "staging",
    )
    scratch_descriptor: Optional[int] = None
    staging_descriptor: Optional[int] = None
    parent_descriptor: Optional[int] = None
    try:
        scratch_descriptor, scratch_info = _open_recovery_directory(
            scratch,
            expected_identity=scratch_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        scratch_members = frozenset(os.listdir(scratch_descriptor))
        if RECOVERY_RECORD_NAME in scratch_members:
            raise SuccessorProduceV4Error(
                "create-once recovery record already exists"
            )
        if scratch_members != frozenset(("staging",)):
            raise SuccessorProduceV4Error(
                "production recovery scratch members differ"
            )
        staging_descriptor, staging_info = _open_recovery_staging(
            scratch_descriptor,
            expected_identity=staging_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        _require_live_recovery_mount(
            staging_path=scratch / "staging",
            staging_info=staging_info,
            recovery_stem=recovery_stem,
            expected=normalised_mount,
        )
        if os.listdir(staging_descriptor):
            raise SuccessorProduceV4Error(
                "production recovery staging must be empty before publication"
            )
        parent_descriptor, parent_info = _open_recovery_directory(
            outputs_parent,
            expected_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="output parent",
        )
        if os.listdir(parent_descriptor):
            raise SuccessorProduceV4Error(
                "production recovery output parent members differ"
            )
        common = pathlib.Path(
            os.path.commonpath((str(scratch), str(outputs_parent)))
        )
        if common == scratch or common == outputs_parent:
            raise SuccessorProduceV4Error(
                "production recovery output parent overlaps scratch"
            )
        if len({scratch_identity, staging_identity, parent_identity}) != 3:
            raise SuccessorProduceV4Error(
                "production recovery directories share an identity"
            )
        document = _recovery_record_document(
            scratch=scratch,
            scratch_info=scratch_info,
            staging_info=staging_info,
            outputs_parent=outputs_parent,
            parent_info=parent_info,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            recovery_stem=recovery_stem,
            mount_identity=normalised_mount,
        )
        _publish_recovery_record_at(scratch_descriptor, document)
        observed = _read_recovery_record_at(
            scratch_descriptor,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        _require_strict_equal(
            observed, document, "production recovery record differs after publication"
        )
        if frozenset(os.listdir(scratch_descriptor)) != frozenset(
            ("staging", RECOVERY_RECORD_NAME)
        ):
            raise SuccessorProduceV4Error(
                "production recovery scratch members changed"
            )
        if os.listdir(staging_descriptor):
            raise SuccessorProduceV4Error(
                "production recovery staging changed before child execution"
            )
        if os.listdir(parent_descriptor):
            raise SuccessorProduceV4Error(
                "production recovery output parent changed before child execution"
            )
        _require_live_recovery_mount(
            staging_path=scratch / "staging",
            staging_info=staging_info,
            recovery_stem=recovery_stem,
            expected=normalised_mount,
        )
        _recheck_recovery_directory_names(
            scratch=scratch,
            scratch_descriptor=scratch_descriptor,
            scratch_identity=scratch_identity,
            staging_descriptor=staging_descriptor,
            staging_identity=staging_identity,
            outputs_parent=outputs_parent,
            parent_descriptor=parent_descriptor,
            parent_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        return document
    finally:
        if parent_descriptor is not None:
            os.close(parent_descriptor)
        if staging_descriptor is not None:
            os.close(staging_descriptor)
        if scratch_descriptor is not None:
            os.close(scratch_descriptor)


def verify_production_recovery_record(
    *,
    scratch: pathlib.Path,
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
    mount_identity: Mapping[str, Any],
) -> dict[str, Any]:
    """Re-bind the exact live directories and mount to the durable record."""

    scratch = pathlib.Path(scratch)
    outputs_parent = pathlib.Path(outputs_parent)
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    try:
        scratch_stat = scratch.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error("scratch recovery directory is absent") from exc
    scratch_identity = (scratch_stat.st_dev, scratch_stat.st_ino)
    scratch_descriptor: Optional[int] = None
    staging_descriptor: Optional[int] = None
    parent_descriptor: Optional[int] = None
    try:
        scratch_descriptor, scratch_info = _open_recovery_directory(
            scratch,
            expected_identity=scratch_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        allowed_members = {
            frozenset(("staging", RECOVERY_RECORD_NAME)),
            frozenset(
                (
                    "staging",
                    RECOVERY_RECORD_NAME,
                    RECOVERY_CLEANUP_CHECKPOINT_NAME,
                )
            ),
        }
        if frozenset(os.listdir(scratch_descriptor)) not in allowed_members:
            raise SuccessorProduceV4Error(
                "production recovery scratch members differ"
            )
        try:
            staging_stat = os.stat(
                "staging", dir_fd=scratch_descriptor, follow_symlinks=False
            )
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "staging mount recovery directory is absent"
            ) from exc
        staging_identity = (staging_stat.st_dev, staging_stat.st_ino)
        staging_descriptor, staging_info = _open_recovery_staging(
            scratch_descriptor,
            expected_identity=staging_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        parent_descriptor, parent_info = _open_recovery_directory(
            outputs_parent,
            expected_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="output parent",
        )
        normalised_mount = _normalise_recovery_mount_identity(
            mount_identity,
            recovery_stem=recovery_stem,
            staging_path=scratch / "staging",
        )
        _require_live_recovery_mount(
            staging_path=scratch / "staging",
            staging_info=staging_info,
            recovery_stem=recovery_stem,
            expected=normalised_mount,
        )
        expected = _recovery_record_document(
            scratch=scratch,
            scratch_info=scratch_info,
            staging_info=staging_info,
            outputs_parent=outputs_parent,
            parent_info=parent_info,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            recovery_stem=recovery_stem,
            mount_identity=normalised_mount,
        )
        observed = _read_recovery_record_at(
            scratch_descriptor,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        _require_strict_equal(
            observed, expected, "production recovery record or mount differs"
        )
        if frozenset(os.listdir(scratch_descriptor)) not in allowed_members:
            raise SuccessorProduceV4Error(
                "production recovery scratch members changed"
            )
        _require_live_recovery_mount(
            staging_path=scratch / "staging",
            staging_info=staging_info,
            recovery_stem=recovery_stem,
            expected=normalised_mount,
        )
        _recheck_recovery_directory_names(
            scratch=scratch,
            scratch_descriptor=scratch_descriptor,
            scratch_identity=scratch_identity,
            staging_descriptor=staging_descriptor,
            staging_identity=staging_identity,
            outputs_parent=outputs_parent,
            parent_descriptor=parent_descriptor,
            parent_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        return observed
    finally:
        if parent_descriptor is not None:
            os.close(parent_descriptor)
        if staging_descriptor is not None:
            os.close(staging_descriptor)
        if scratch_descriptor is not None:
            os.close(scratch_descriptor)


def publish_production_cleanup_checkpoint(
    *,
    scratch: pathlib.Path,
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
    mount_identity: Mapping[str, Any],
) -> dict[str, Any]:
    """Durably prove live recovery state immediately before unmount."""

    _reconcile_cleanup_checkpoint_partial(
        scratch=pathlib.Path(scratch),
        expected_uid=expected_uid,
        expected_gid=expected_gid,
    )
    observed_recovery = verify_production_recovery_record(
        scratch=scratch,
        outputs_parent=outputs_parent,
        expected_parent_identity=expected_parent_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        recovery_stem=recovery_stem,
        mount_identity=mount_identity,
    )
    scratch = pathlib.Path(scratch)
    outputs_parent = pathlib.Path(outputs_parent)
    scratch_info = scratch.lstat()
    scratch_identity = (scratch_info.st_dev, scratch_info.st_ino)
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    scratch_descriptor: Optional[int] = None
    parent_descriptor: Optional[int] = None
    try:
        scratch_descriptor, scratch_opened = _open_recovery_directory(
            scratch,
            expected_identity=scratch_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        parent_descriptor, parent_opened = _open_recovery_directory(
            outputs_parent,
            expected_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="output parent",
        )
        members = frozenset(os.listdir(scratch_descriptor))
        allowed = {
            frozenset(("staging", RECOVERY_RECORD_NAME)),
            frozenset(
                (
                    "staging",
                    RECOVERY_RECORD_NAME,
                    RECOVERY_CLEANUP_CHECKPOINT_NAME,
                )
            ),
            frozenset(
                (
                    "staging",
                    RECOVERY_RECORD_NAME,
                    f".{RECOVERY_CLEANUP_CHECKPOINT_NAME}.partial",
                )
            ),
        }
        if members not in allowed:
            raise SuccessorProduceV4Error(
                "production cleanup checkpoint scratch members differ"
            )
        document = _cleanup_checkpoint_document(
            scratch=scratch,
            scratch_info=scratch_opened,
            outputs_parent=outputs_parent,
            parent_info=parent_opened,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            recovery_stem=recovery_stem,
            recovery_record=observed_recovery,
        )
        _publish_cleanup_checkpoint_at(scratch_descriptor, document)
        observed = _read_cleanup_checkpoint_at(
            scratch_descriptor,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        _require_strict_equal(
            observed,
            document,
            "production cleanup checkpoint differs after publication",
        )
        verify_production_recovery_record(
            scratch=scratch,
            outputs_parent=outputs_parent,
            expected_parent_identity=expected_parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            recovery_stem=recovery_stem,
            mount_identity=mount_identity,
        )
        return document
    finally:
        if parent_descriptor is not None:
            os.close(parent_descriptor)
        if scratch_descriptor is not None:
            os.close(scratch_descriptor)


def verify_production_recovery_after_unmount(
    *,
    scratch: pathlib.Path,
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
) -> dict[str, Any]:
    """Verify the pre-unmount checkpoint after the tmpfs is already gone."""

    scratch = pathlib.Path(scratch)
    outputs_parent = pathlib.Path(outputs_parent)
    _require_absent_recovery_mount(scratch / "staging")
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    try:
        scratch_stat = scratch.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "post-unmount recovery scratch is absent"
        ) from exc
    scratch_identity = (scratch_stat.st_dev, scratch_stat.st_ino)
    scratch_descriptor: Optional[int] = None
    staging_descriptor: Optional[int] = None
    parent_descriptor: Optional[int] = None
    try:
        scratch_descriptor, scratch_info = _open_recovery_directory(
            scratch,
            expected_identity=scratch_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="scratch",
        )
        expected_members = frozenset(
            (
                "staging",
                RECOVERY_RECORD_NAME,
                RECOVERY_CLEANUP_CHECKPOINT_NAME,
            )
        )
        if frozenset(os.listdir(scratch_descriptor)) != expected_members:
            raise SuccessorProduceV4Error(
                "post-unmount recovery scratch members differ"
            )
        staging_stat = os.stat(
            "staging", dir_fd=scratch_descriptor, follow_symlinks=False
        )
        staging_identity = (staging_stat.st_dev, staging_stat.st_ino)
        staging_descriptor, _ = _open_recovery_staging(
            scratch_descriptor,
            expected_identity=staging_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        if os.listdir(staging_descriptor):
            raise SuccessorProduceV4Error(
                "post-unmount recovery staging is not empty"
            )
        parent_descriptor, parent_info = _open_recovery_directory(
            outputs_parent,
            expected_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="output parent",
        )
        recovery_record = _read_recovery_record_at(
            scratch_descriptor,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        expected = _cleanup_checkpoint_document(
            scratch=scratch,
            scratch_info=scratch_info,
            outputs_parent=outputs_parent,
            parent_info=parent_info,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            recovery_stem=recovery_stem,
            recovery_record=recovery_record,
        )
        observed = _read_cleanup_checkpoint_at(
            scratch_descriptor,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        _require_strict_equal(
            observed,
            expected,
            "post-unmount recovery checkpoint differs",
        )
        _recheck_recovery_directory_names(
            scratch=scratch,
            scratch_descriptor=scratch_descriptor,
            scratch_identity=scratch_identity,
            staging_descriptor=staging_descriptor,
            staging_identity=staging_identity,
            outputs_parent=outputs_parent,
            parent_descriptor=parent_descriptor,
            parent_identity=parent_identity,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
        )
        return observed
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "post-unmount recovery directory changed"
        ) from exc
    finally:
        if parent_descriptor is not None:
            os.close(parent_descriptor)
        if staging_descriptor is not None:
            os.close(staging_descriptor)
        if scratch_descriptor is not None:
            os.close(scratch_descriptor)


def remove_verified_production_recovery(
    *,
    scratch: pathlib.Path,
    outputs_parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    recovery_stem: str,
) -> str:
    """Rename a verified cleanup tree, then resume bounded deletion safely."""

    scratch = pathlib.Path(scratch)
    outputs_parent = pathlib.Path(outputs_parent)
    if scratch.name != recovery_stem or re.fullmatch(
        r"boole-nsv4-[0-9a-f]{40}-r[12]", recovery_stem
    ) is None:
        raise SuccessorProduceV4Error("production recovery stem differs")
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    output_descriptor, _ = _open_recovery_directory(
        outputs_parent,
        expected_identity=parent_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        context="output parent",
    )
    recovery_root_descriptor = _open_absolute_recovery_directory(
        scratch.parent, "production recovery root"
    )
    tombstone_name = f".{recovery_stem}.cleanup"
    tombstone_descriptor: Optional[int] = None
    staging_descriptor: Optional[int] = None
    try:
        root_info = os.fstat(recovery_root_descriptor)
        if (
            root_info.st_uid != expected_uid
            or root_info.st_gid != expected_gid
            or stat.S_IMODE(root_info.st_mode) != 0o700
        ):
            raise SuccessorProduceV4Error(
                "production recovery root identity differs"
            )

        def member_exists(name: str) -> bool:
            try:
                os.stat(
                    name,
                    dir_fd=recovery_root_descriptor,
                    follow_symlinks=False,
                )
                return True
            except FileNotFoundError:
                return False
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "production recovery root member changed"
                ) from exc

        live_exists = member_exists(recovery_stem)
        tombstone_exists = member_exists(tombstone_name)
        if live_exists and tombstone_exists:
            raise SuccessorProduceV4Error(
                "production recovery live tree and tombstone coexist"
            )
        if not live_exists and not tombstone_exists:
            return "already-absent"
        if live_exists:
            verify_production_recovery_after_unmount(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=parent_identity,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                recovery_stem=recovery_stem,
            )
            reached = os.stat(
                recovery_stem,
                dir_fd=recovery_root_descriptor,
                follow_symlinks=False,
            )
            if not stat.S_ISDIR(reached.st_mode):
                raise SuccessorProduceV4Error(
                    "verified production recovery path differs"
                )
            os.rename(
                recovery_stem,
                tombstone_name,
                src_dir_fd=recovery_root_descriptor,
                dst_dir_fd=recovery_root_descriptor,
            )
            os.fsync(recovery_root_descriptor)

        tombstone_stat = os.stat(
            tombstone_name,
            dir_fd=recovery_root_descriptor,
            follow_symlinks=False,
        )
        tombstone_descriptor = os.open(
            tombstone_name,
            _recovery_directory_flags(),
            dir_fd=recovery_root_descriptor,
        )
        opened_tombstone = _require_recovery_directory(
            tombstone_descriptor,
            inspected=tombstone_stat,
            expected_identity=(tombstone_stat.st_dev, tombstone_stat.st_ino),
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            context="production recovery tombstone",
        )
        members = frozenset(os.listdir(tombstone_descriptor))
        full = frozenset(
            (
                "staging",
                RECOVERY_RECORD_NAME,
                RECOVERY_CLEANUP_CHECKPOINT_NAME,
            )
        )
        allowed = {
            full,
            frozenset(("staging", RECOVERY_RECORD_NAME)),
            frozenset(("staging",)),
            frozenset(),
        }
        if members not in allowed:
            raise SuccessorProduceV4Error(
                "production recovery tombstone members differ"
            )
        if "staging" in members:
            staging_info = os.stat(
                "staging",
                dir_fd=tombstone_descriptor,
                follow_symlinks=False,
            )
            staging_descriptor, opened_staging = _open_recovery_staging(
                tombstone_descriptor,
                expected_identity=(staging_info.st_dev, staging_info.st_ino),
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
            if opened_staging.st_dev != opened_tombstone.st_dev:
                raise SuccessorProduceV4Error(
                    "production recovery tombstone staging is mounted"
                )
            if os.listdir(staging_descriptor):
                raise SuccessorProduceV4Error(
                    "production recovery tombstone staging is not empty"
                )
        if RECOVERY_RECORD_NAME in members:
            recovery_record = _read_recovery_record_at(
                tombstone_descriptor,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
            if (
                recovery_record.get("schema") != RECOVERY_RECORD_SCHEMA
                or recovery_record.get("recoveryStem") != recovery_stem
                or recovery_record.get("directories", {})
                .get("scratch", {})
                .get("path")
                != str(scratch)
            ):
                raise SuccessorProduceV4Error(
                    "production recovery tombstone record differs"
                )
        else:
            recovery_record = None
        if RECOVERY_CLEANUP_CHECKPOINT_NAME in members:
            if recovery_record is None:
                raise SuccessorProduceV4Error(
                    "production recovery tombstone checkpoint lacks record"
                )
            observed_checkpoint = _read_cleanup_checkpoint_at(
                tombstone_descriptor,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
            expected_checkpoint = _cleanup_checkpoint_document(
                scratch=scratch,
                scratch_info=opened_tombstone,
                outputs_parent=outputs_parent,
                parent_info=os.fstat(output_descriptor),
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                recovery_stem=recovery_stem,
                recovery_record=recovery_record,
            )
            _require_strict_equal(
                observed_checkpoint,
                expected_checkpoint,
                "production recovery tombstone checkpoint",
            )
        reached_tombstone = os.stat(
            tombstone_name,
            dir_fd=recovery_root_descriptor,
            follow_symlinks=False,
        )
        if (
            not stat.S_ISDIR(reached_tombstone.st_mode)
            or (reached_tombstone.st_dev, reached_tombstone.st_ino)
            != (opened_tombstone.st_dev, opened_tombstone.st_ino)
        ):
            raise SuccessorProduceV4Error(
                "production recovery tombstone path changed"
            )
        if RECOVERY_CLEANUP_CHECKPOINT_NAME in members:
            os.unlink(
                RECOVERY_CLEANUP_CHECKPOINT_NAME,
                dir_fd=tombstone_descriptor,
            )
            os.fsync(tombstone_descriptor)
        if RECOVERY_RECORD_NAME in members:
            os.unlink(RECOVERY_RECORD_NAME, dir_fd=tombstone_descriptor)
            os.fsync(tombstone_descriptor)
        if staging_descriptor is not None:
            os.close(staging_descriptor)
            staging_descriptor = None
            os.rmdir("staging", dir_fd=tombstone_descriptor)
            os.fsync(tombstone_descriptor)
        if os.listdir(tombstone_descriptor):
            raise SuccessorProduceV4Error(
                "production recovery tombstone members remain"
            )
        os.close(tombstone_descriptor)
        tombstone_descriptor = None
        os.rmdir(tombstone_name, dir_fd=recovery_root_descriptor)
        os.fsync(recovery_root_descriptor)
        return "removed-verified"
    except SuccessorProduceV4Error:
        raise
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "verified production recovery cleanup was interrupted"
        ) from exc
    finally:
        if staging_descriptor is not None:
            os.close(staging_descriptor)
        if tombstone_descriptor is not None:
            os.close(tombstone_descriptor)
        os.close(recovery_root_descriptor)
        os.close(output_descriptor)


def _mkdir_durable_once(path: pathlib.Path, *, mode: int = 0o755) -> None:
    """Create one directory and persist its name before children are written."""

    path = pathlib.Path(path)
    parent = _real_directory(path.parent, "output parent")
    if os.path.lexists(path):
        raise SuccessorProduceV4Error(f"fresh output directory already exists: {path}")
    try:
        path.mkdir(mode=mode)
        _fsync_directory(parent)
    except BaseException:
        try:
            path.rmdir()
        except OSError:
            pass
        raise


def _marker_document(chain: GenerationChain) -> dict[str, Any]:
    return {
        "attemptId": chain.attempt_id,
        "authoritySha256": chain.identities["A6"].sha256,
        "consumed": True,
        "outputNames": list(chain.output_names),
        "schema": (
            "boole.native-shadow.mac3.launcher-v2-successor-production-attempt-"
            "consumed.arm64.v4"
        ),
        "writtenBefore": "the first image output file",
    }


def _read_exact_output_marker_at(
    outputs_descriptor: int,
    *,
    expected_raw: bytes,
    expected_uid: int,
    expected_gid: int,
    allowed_links: frozenset[int],
) -> os.stat_result:
    """Read and bind the consumed marker through one already-open directory."""

    try:
        descriptor = os.open(
            CONSUMED_MARKER_NAME,
            _output_file_flags(),
            dir_fd=outputs_descriptor,
        )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production output consumed marker is unreadable"
        ) from exc
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_nlink not in allowed_links
            or before.st_uid != expected_uid
            or before.st_gid != expected_gid
            or stat.S_IMODE(before.st_mode) != COLLECTABLE_FILE_MODE
            or before.st_size > MAX_METADATA_BYTES
        ):
            raise SuccessorProduceV4Error(
                "production output consumed marker identity differs"
            )
        raw = bytearray()
        while len(raw) <= MAX_METADATA_BYTES:
            chunk = os.read(
                descriptor,
                min(64 * 1024, MAX_METADATA_BYTES + 1 - len(raw)),
            )
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        before_key = (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_nlink,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        )
        after_key = (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_nlink,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        if (
            len(raw) > MAX_METADATA_BYTES
            or len(raw) != after.st_size
            or before_key != after_key
            or bytes(raw) != expected_raw
        ):
            raise SuccessorProduceV4Error(
                "production output consumed marker claim differs"
            )
        try:
            reached = os.stat(
                CONSUMED_MARKER_NAME,
                dir_fd=outputs_descriptor,
                follow_symlinks=False,
            )
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "production output consumed marker path changed"
            ) from exc
        if (
            not stat.S_ISREG(reached.st_mode)
            or (reached.st_dev, reached.st_ino) != (after.st_dev, after.st_ino)
            or reached.st_nlink not in allowed_links
            or reached.st_uid != expected_uid
            or reached.st_gid != expected_gid
            or stat.S_IMODE(reached.st_mode) != COLLECTABLE_FILE_MODE
        ):
            raise SuccessorProduceV4Error(
                "production output consumed marker path identity differs"
            )
        return after
    finally:
        os.close(descriptor)


def reconcile_production_output_state(
    *,
    repository_root: pathlib.Path,
    parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
) -> str:
    """Converge only known marker publication crashes without weakening A6.

    The output parent is outside the disposable tmpfs.  A process can die
    between writing a create-once marker's temporary inode, linking its final
    name and unlinking the temporary name.  This routine recognizes exactly
    those states through held descriptors.  Unknown names and a marker for a
    different authority are rejected before any mutation.
    """

    parent = pathlib.Path(parent)
    parent_identity = _require_recovery_identity(
        expected_parent_identity, "output parent"
    )
    chain = verify_generation_chain(pathlib.Path(repository_root).resolve())
    expected_marker = canonical_json(_marker_document(chain))
    try:
        inspected_parent = parent.lstat()
        parent_descriptor = _open_absolute_recovery_directory(
            parent, "output parent"
        )
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "production output parent is unavailable"
        ) from exc
    outputs_descriptor: Optional[int] = None
    try:
        opened_parent = os.fstat(parent_descriptor)
        parent_mode = stat.S_IMODE(opened_parent.st_mode)
        if (
            not stat.S_ISDIR(inspected_parent.st_mode)
            or not stat.S_ISDIR(opened_parent.st_mode)
            or (inspected_parent.st_dev, inspected_parent.st_ino)
            != (opened_parent.st_dev, opened_parent.st_ino)
            or (opened_parent.st_dev, opened_parent.st_ino) != parent_identity
            or opened_parent.st_uid != expected_uid
            or opened_parent.st_gid != expected_gid
            or parent_mode not in (0o700, COLLECTABLE_PARENT_MODE)
        ):
            raise SuccessorProduceV4Error(
                "production output parent recovery identity differs"
            )

        def require_absolute_parent_rebind(expected_mode: int) -> None:
            """Prove the public path still names the held parent inode."""

            rebound_descriptor: Optional[int] = None
            try:
                reached = parent.lstat()
                rebound_descriptor = _open_absolute_recovery_directory(
                    parent, "output parent"
                )
                rebound = os.fstat(rebound_descriptor)
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "production output parent path changed"
                ) from exc
            finally:
                if rebound_descriptor is not None:
                    os.close(rebound_descriptor)
            expected_key = (opened_parent.st_dev, opened_parent.st_ino)
            if (
                not stat.S_ISDIR(reached.st_mode)
                or not stat.S_ISDIR(rebound.st_mode)
                or (reached.st_dev, reached.st_ino) != expected_key
                or (rebound.st_dev, rebound.st_ino) != expected_key
                or expected_key != parent_identity
                or reached.st_uid != expected_uid
                or reached.st_gid != expected_gid
                or rebound.st_uid != expected_uid
                or rebound.st_gid != expected_gid
                or stat.S_IMODE(reached.st_mode) != expected_mode
                or stat.S_IMODE(rebound.st_mode) != expected_mode
            ):
                raise SuccessorProduceV4Error(
                    "production output parent path identity changed"
                )

        parent_members = frozenset(os.listdir(parent_descriptor))
        allowed_parent_members = (
            frozenset(),
            frozenset(("outputs",)),
            frozenset(("outputs", REPLICA_PROVENANCE_NAME)),
            frozenset(("outputs", REPLICA_PROVENANCE_PARTIAL_NAME)),
            frozenset(
                (
                    "outputs",
                    REPLICA_PROVENANCE_NAME,
                    REPLICA_PROVENANCE_PARTIAL_NAME,
                )
            ),
        )
        if parent_members not in allowed_parent_members:
            raise SuccessorProduceV4Error(
                "production output parent member set differs"
            )
        if not parent_members:
            if parent_mode != 0o700:
                raise SuccessorProduceV4Error(
                    "empty production output parent is unexpectedly sealed"
                )
            require_absolute_parent_rebind(0o700)
            return "unconsumed"
        try:
            outputs_info = os.stat(
                "outputs", dir_fd=parent_descriptor, follow_symlinks=False
            )
            outputs_descriptor = os.open(
                "outputs",
                _recovery_directory_flags(),
                dir_fd=parent_descriptor,
            )
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "production outputs recovery directory is unavailable"
            ) from exc
        opened_outputs = os.fstat(outputs_descriptor)
        outputs_mode = stat.S_IMODE(opened_outputs.st_mode)
        allowed_outputs_modes = (
            frozenset((COLLECTABLE_OUTPUT_DIRECTORY_MODE,))
            if parent_mode == COLLECTABLE_PARENT_MODE
            else frozenset((0o700, 0o755, COLLECTABLE_OUTPUT_DIRECTORY_MODE))
        )
        if (
            not stat.S_ISDIR(outputs_info.st_mode)
            or not stat.S_ISDIR(opened_outputs.st_mode)
            or (outputs_info.st_dev, outputs_info.st_ino)
            != (opened_outputs.st_dev, opened_outputs.st_ino)
            or opened_outputs.st_uid != expected_uid
            or opened_outputs.st_gid != expected_gid
            or outputs_mode not in allowed_outputs_modes
        ):
            raise SuccessorProduceV4Error(
                "production outputs recovery identity differs"
            )
        if parent_mode == COLLECTABLE_PARENT_MODE and parent_members not in (
            frozenset(("outputs",)),
            frozenset(("outputs", REPLICA_PROVENANCE_NAME)),
        ):
            raise SuccessorProduceV4Error(
                "sealed production output parent member set differs"
            )
        names = frozenset(os.listdir(outputs_descriptor))
        partial_prefix = f".{CONSUMED_MARKER_NAME}.partial."
        partial_names = tuple(
            name
            for name in names
            if name.startswith(partial_prefix)
            and re.fullmatch(
                re.escape(partial_prefix) + r"[A-Za-z0-9_]{8,}", name
            )
            is not None
        )
        unknown_partial_names = tuple(
            name
            for name in names
            if name.startswith(f".{CONSUMED_MARKER_NAME}.partial")
            and name not in partial_names
        )
        if unknown_partial_names or len(partial_names) > 1:
            raise SuccessorProduceV4Error(
                "production output marker partial member set differs"
            )
        partial_name = partial_names[0] if partial_names else None
        marker_present = CONSUMED_MARKER_NAME in names

        if not marker_present:
            expected_names = frozenset((partial_name,)) if partial_name else frozenset()
            if (
                names != expected_names
                or parent_mode != 0o700
                or outputs_mode != 0o700
            ):
                raise SuccessorProduceV4Error(
                    "unconsumed production output member set differs"
                )
            if partial_name is not None:
                try:
                    partial_info = os.stat(
                        partial_name,
                        dir_fd=outputs_descriptor,
                        follow_symlinks=False,
                    )
                except OSError as exc:
                    raise SuccessorProduceV4Error(
                        "production output marker partial changed"
                    ) from exc
                if (
                    not stat.S_ISREG(partial_info.st_mode)
                    or partial_info.st_nlink != 1
                    or partial_info.st_uid != expected_uid
                    or partial_info.st_gid != expected_gid
                    or stat.S_IMODE(partial_info.st_mode) not in (0o400, 0o444, 0o600)
                    or partial_info.st_size > MAX_METADATA_BYTES
                ):
                    raise SuccessorProduceV4Error(
                        "production output marker partial identity differs"
                    )
                os.unlink(partial_name, dir_fd=outputs_descriptor)
                os.fsync(outputs_descriptor)
            if os.listdir(outputs_descriptor):
                raise SuccessorProduceV4Error(
                    "unconsumed production output members changed"
                )
            reached_outputs = os.stat(
                "outputs", dir_fd=parent_descriptor, follow_symlinks=False
            )
            if (
                not stat.S_ISDIR(reached_outputs.st_mode)
                or (reached_outputs.st_dev, reached_outputs.st_ino)
                != (opened_outputs.st_dev, opened_outputs.st_ino)
            ):
                raise SuccessorProduceV4Error(
                    "production outputs recovery path changed"
                )
            os.rmdir("outputs", dir_fd=parent_descriptor)
            os.fsync(parent_descriptor)
            if os.listdir(parent_descriptor):
                raise SuccessorProduceV4Error(
                    "unconsumed production output parent changed"
                )
            require_absolute_parent_rebind(0o700)
            return "unconsumed"

        marker_links = frozenset((1, 2)) if partial_name else frozenset((1,))
        marker_info = _read_exact_output_marker_at(
            outputs_descriptor,
            expected_raw=expected_marker,
            expected_uid=expected_uid,
            expected_gid=expected_gid,
            allowed_links=marker_links,
        )
        names_without_partial = (
            frozenset(name for name in names if name != partial_name)
            if partial_name
            else names
        )
        if partial_name is not None:
            try:
                partial_info = os.stat(
                    partial_name,
                    dir_fd=outputs_descriptor,
                    follow_symlinks=False,
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "production output marker partial changed"
                ) from exc
            if (
                not stat.S_ISREG(partial_info.st_mode)
                or (partial_info.st_dev, partial_info.st_ino)
                != (marker_info.st_dev, marker_info.st_ino)
                or partial_info.st_nlink != 2
                or partial_info.st_uid != expected_uid
                or partial_info.st_gid != expected_gid
                or stat.S_IMODE(partial_info.st_mode) != COLLECTABLE_FILE_MODE
            ):
                raise SuccessorProduceV4Error(
                    "production output marker partial identity differs"
                )
            os.unlink(partial_name, dir_fd=outputs_descriptor)
            os.fsync(outputs_descriptor)
            _read_exact_output_marker_at(
                outputs_descriptor,
                expected_raw=expected_marker,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                allowed_links=frozenset((1,)),
            )

        publication_partials: list[tuple[str, str]] = []
        recognized_partial_names: set[str] = set()
        for public_name in (
            OUTPUT_NAMES[1],
            UNQUALIFIED_MARKER_NAME,
            PENDING_RESULT_NAME,
            QUALIFIED_RESULT_NAME,
        ):
            prefix = f".{public_name}.partial."
            for name in names_without_partial:
                if (
                    name.startswith(prefix)
                    and re.fullmatch(
                        re.escape(prefix) + r"[A-Za-z0-9_]{8,}", name
                    )
                    is not None
                ):
                    publication_partials.append((name, public_name))
                    recognized_partial_names.add(name)
        if READBACK_PRIVATE_PENDING_NAME in names_without_partial:
            publication_partials.append(
                (READBACK_PRIVATE_PENDING_NAME, READBACK_RESULT_NAME)
            )
            recognized_partial_names.add(READBACK_PRIVATE_PENDING_NAME)
        partial_like_names = {
            name
            for name in names_without_partial
            if (
                (name.startswith(".") and ".partial" in name)
                or name == READBACK_PRIVATE_PENDING_NAME
            )
        }
        if (
            partial_like_names != recognized_partial_names
            or len(publication_partials) > 1
        ):
            raise SuccessorProduceV4Error(
                "production output publication partial member set differs"
            )
        if publication_partials:
            publication_partial, public_name = publication_partials[0]
            expected_publication_mode = (
                0o755
                if public_name
                in (READBACK_RESULT_NAME, QUALIFIED_RESULT_NAME)
                else 0o700
            )
            if outputs_mode != expected_publication_mode:
                raise SuccessorProduceV4Error(
                    "production output publication directory mode differs"
                )
            final_present = public_name in names_without_partial
            predecessor = frozenset(
                name
                for name in names_without_partial
                if name not in (publication_partial, public_name)
            )
            if predecessor not in _CREATE_ONCE_OUTPUT_PREDECESSORS[public_name]:
                raise SuccessorProduceV4Error(
                    "production output publication partial stage differs"
                )
            prospective_names = frozenset(
                name
                for name in names_without_partial
                if name != publication_partial
            )
            if prospective_names not in COLLECTABLE_PROGRESS_STATES:
                raise SuccessorProduceV4Error(
                    "production output publication successor state differs"
                )
            try:
                partial_info = os.stat(
                    publication_partial,
                    dir_fd=outputs_descriptor,
                    follow_symlinks=False,
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "production output publication partial changed"
                ) from exc
            if final_present:
                try:
                    final_info = os.stat(
                        public_name,
                        dir_fd=outputs_descriptor,
                        follow_symlinks=False,
                    )
                except OSError as exc:
                    raise SuccessorProduceV4Error(
                        "production output publication final changed"
                    ) from exc
                if (
                    not stat.S_ISREG(partial_info.st_mode)
                    or not stat.S_ISREG(final_info.st_mode)
                    or (partial_info.st_dev, partial_info.st_ino)
                    != (final_info.st_dev, final_info.st_ino)
                    or partial_info.st_nlink != 2
                    or final_info.st_nlink != 2
                    or partial_info.st_uid != expected_uid
                    or partial_info.st_gid != expected_gid
                    or final_info.st_uid != expected_uid
                    or final_info.st_gid != expected_gid
                    or stat.S_IMODE(partial_info.st_mode)
                    != COLLECTABLE_FILE_MODE
                    or stat.S_IMODE(final_info.st_mode)
                    != COLLECTABLE_FILE_MODE
                ):
                    raise SuccessorProduceV4Error(
                        "production output publication partial identity differs"
                    )
            elif (
                not stat.S_ISREG(partial_info.st_mode)
                or partial_info.st_nlink != 1
                or partial_info.st_uid != expected_uid
                or partial_info.st_gid != expected_gid
                or (
                    publication_partial == READBACK_PRIVATE_PENDING_NAME
                    and stat.S_IMODE(partial_info.st_mode)
                    != COLLECTABLE_FILE_MODE
                )
                or (
                    publication_partial != READBACK_PRIVATE_PENDING_NAME
                    and stat.S_IMODE(partial_info.st_mode) not in (0o444, 0o600)
                )
            ):
                raise SuccessorProduceV4Error(
                    "production output publication partial identity differs"
                )
            os.unlink(publication_partial, dir_fd=outputs_descriptor)
            os.fsync(outputs_descriptor)
            names_without_partial = prospective_names
            if final_present:
                reached_final = os.stat(
                    public_name,
                    dir_fd=outputs_descriptor,
                    follow_symlinks=False,
                )
                if (
                    (reached_final.st_dev, reached_final.st_ino)
                    != (partial_info.st_dev, partial_info.st_ino)
                    or reached_final.st_nlink != 1
                    or reached_final.st_uid != expected_uid
                    or reached_final.st_gid != expected_gid
                    or stat.S_IMODE(reached_final.st_mode)
                    != COLLECTABLE_FILE_MODE
                ):
                    raise SuccessorProduceV4Error(
                        "production output publication final identity differs"
                    )
            _read_exact_output_marker_at(
                outputs_descriptor,
                expected_raw=expected_marker,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                allowed_links=frozenset((1,)),
            )

        success_pending_seal = (
            parent_mode == 0o700
            and names_without_partial == frozenset(QUALIFIED_REPLICA_NAMES)
        )
        if (
            not success_pending_seal
            and parent_mode == 0o700
            and parent_members != frozenset(("outputs",))
        ):
            raise SuccessorProduceV4Error(
                "unsealed failure output parent contains success provenance"
            )
        if names_without_partial not in COLLECTABLE_PROGRESS_STATES:
            raise SuccessorProduceV4Error(
                "consumed production output member set differs"
            )
        if frozenset(os.listdir(outputs_descriptor)) != names_without_partial:
            raise SuccessorProduceV4Error(
                "consumed production output members changed"
            )

        if success_pending_seal:
            inode_keys: set[tuple[int, int]] = set()
            success_descriptors: dict[str, int] = {}
            try:
                for name in sorted(names_without_partial, key=os.fsencode):
                    try:
                        descriptor = os.open(
                            name,
                            _output_file_flags(),
                            dir_fd=outputs_descriptor,
                        )
                    except OSError as exc:
                        raise SuccessorProduceV4Error(
                            f"successful production output member is unreadable: {name}"
                        ) from exc
                    success_descriptors[name] = descriptor
                    info = _sealed_regular_file(
                        descriptor,
                        name=f"outputs/{name}",
                        expected_uid=expected_uid,
                        expected_gid=expected_gid,
                        inode_keys=inode_keys,
                    )
                    allowed_modes = (
                        frozenset((0o600, COLLECTABLE_FILE_MODE))
                        if outputs_mode == 0o700
                        else frozenset((COLLECTABLE_FILE_MODE,))
                    )
                    if stat.S_IMODE(info.st_mode) not in allowed_modes:
                        raise SuccessorProduceV4Error(
                            f"successful production output member mode differs: {name}"
                        )
                if frozenset(os.listdir(outputs_descriptor)) != names_without_partial:
                    raise SuccessorProduceV4Error(
                        "successful production output members changed"
                    )
                reached_outputs = os.stat(
                    "outputs",
                    dir_fd=parent_descriptor,
                    follow_symlinks=False,
                )
                if (
                    not stat.S_ISDIR(reached_outputs.st_mode)
                    or (reached_outputs.st_dev, reached_outputs.st_ino)
                    != (opened_outputs.st_dev, opened_outputs.st_ino)
                    or reached_outputs.st_uid != expected_uid
                    or reached_outputs.st_gid != expected_gid
                    or stat.S_IMODE(reached_outputs.st_mode) != outputs_mode
                ):
                    raise SuccessorProduceV4Error(
                        "successful production outputs path identity changed"
                    )
                require_absolute_parent_rebind(0o700)
                return "success-pending-seal"
            finally:
                for descriptor in success_descriptors.values():
                    os.close(descriptor)

        if outputs_mode in (0o755, COLLECTABLE_OUTPUT_DIRECTORY_MODE):
            inode_keys: set[tuple[int, int]] = set()
            identities: dict[str, PinnedOutputIdentity] = {}
            descriptors: dict[str, int] = {}
            try:
                for name in sorted(names_without_partial, key=os.fsencode):
                    try:
                        descriptor = os.open(
                            name,
                            _output_file_flags(),
                            dir_fd=outputs_descriptor,
                        )
                    except OSError as exc:
                        raise SuccessorProduceV4Error(
                            f"sealed production output member is unreadable: {name}"
                        ) from exc
                    descriptors[name] = descriptor
                    info = _sealed_regular_file(
                        descriptor,
                        name=f"outputs/{name}",
                        expected_uid=expected_uid,
                        expected_gid=expected_gid,
                        inode_keys=inode_keys,
                    )
                    if stat.S_IMODE(info.st_mode) != COLLECTABLE_FILE_MODE:
                        raise SuccessorProduceV4Error(
                            f"sealed production output member mode differs: {name}"
                        )
                    identities[name] = _descriptor_output_identity(
                        descriptor,
                        f"sealed production output member outputs/{name}",
                    )

                if REPLICA_PROVENANCE_NAME in parent_members:
                    if names_without_partial != frozenset(QUALIFIED_REPLICA_NAMES):
                        raise SuccessorProduceV4Error(
                            "sealed production provenance output member set differs"
                        )
                    try:
                        provenance_descriptor = os.open(
                            REPLICA_PROVENANCE_NAME,
                            _output_file_flags(),
                            dir_fd=parent_descriptor,
                        )
                    except OSError as exc:
                        raise SuccessorProduceV4Error(
                            "sealed production provenance is unreadable"
                        ) from exc
                    try:
                        provenance_info = _sealed_regular_file(
                            provenance_descriptor,
                            name=REPLICA_PROVENANCE_NAME,
                            expected_uid=expected_uid,
                            expected_gid=expected_gid,
                            inode_keys=inode_keys,
                        )
                        if (
                            stat.S_IMODE(provenance_info.st_mode)
                            != COLLECTABLE_FILE_MODE
                        ):
                            raise SuccessorProduceV4Error(
                                "sealed production provenance mode differs"
                            )
                        provenance_identity = _descriptor_output_identity(
                            provenance_descriptor,
                            "sealed production provenance",
                        )
                        raw_provenance = PinnedOutputs(
                            {REPLICA_PROVENANCE_NAME: provenance_identity},
                            {REPLICA_PROVENANCE_NAME: provenance_descriptor},
                        ).read_metadata(
                            REPLICA_PROVENANCE_NAME,
                            "sealed production provenance",
                            max_bytes=MAX_METADATA_BYTES,
                        )
                    finally:
                        os.close(provenance_descriptor)
                    try:
                        provenance = json.loads(raw_provenance.decode("utf-8"))
                    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                        raise SuccessorProduceV4Error(
                            "sealed production provenance is not JSON"
                        ) from exc
                    if (
                        not isinstance(provenance, Mapping)
                        or raw_provenance != canonical_json(provenance)
                        or set(provenance)
                        != {
                            "activationAllowed",
                            "artifactName",
                            "attemptId",
                            "authoritySha256",
                            "bootableClaim",
                            "dispatchClaim",
                            "github",
                            "qualifiedTree",
                            "replica",
                            "schema",
                        }
                        or provenance.get("schema") != REPLICA_PROVENANCE_SCHEMA
                        or provenance.get("attemptId") != chain.attempt_id
                        or provenance.get("authoritySha256")
                        != chain.identities["A6"].sha256
                        or provenance.get("activationAllowed") is not False
                        or provenance.get("bootableClaim") is not False
                    ):
                        raise SuccessorProduceV4Error(
                            "sealed production provenance claim differs"
                        )
                    members = [
                        {
                            "name": name,
                            "sha256": identities[name].sha256,
                            "sizeBytes": identities[name].size_bytes,
                        }
                        for name in QUALIFIED_REPLICA_NAMES
                    ]
                    expected_tree = {
                        "manifestSha256": _sha256(canonical_json(members)),
                        "members": members,
                        "relativePath": "outputs",
                    }
                    _require_strict_equal(
                        provenance.get("qualifiedTree"),
                        expected_tree,
                        "sealed production provenance tree differs",
                    )
            finally:
                for descriptor in descriptors.values():
                    os.close(descriptor)

        reached_outputs = os.stat(
            "outputs", dir_fd=parent_descriptor, follow_symlinks=False
        )
        if (
            not stat.S_ISDIR(reached_outputs.st_mode)
            or (reached_outputs.st_dev, reached_outputs.st_ino)
            != (opened_outputs.st_dev, opened_outputs.st_ino)
            or stat.S_IMODE(reached_outputs.st_mode) != outputs_mode
        ):
            raise SuccessorProduceV4Error(
                "production outputs recovery path changed"
            )
        require_absolute_parent_rebind(parent_mode)
        return "sealed" if parent_mode == COLLECTABLE_PARENT_MODE else "consumed"
    finally:
        if outputs_descriptor is not None:
            os.close(outputs_descriptor)
        os.close(parent_descriptor)


def _write_consumed_marker(request: ProductionRequest) -> pathlib.Path:
    marker = request.outputs / CONSUMED_MARKER_NAME
    document = _marker_document(request.chain)
    _publish_json_once(marker, document)
    try:
        sys.stdout.write(f"{CONSUMED_MARKER_NAME}\n")
        sys.stdout.write(canonical_json(document).decode("utf-8"))
        sys.stdout.flush()
    except OSError:
        pass
    return marker


def _make_outputs_collectable(outputs: pathlib.Path) -> None:
    root = _real_directory(outputs, "production outputs")
    try:
        entries = {entry.name: entry for entry in os.scandir(root)}
    except OSError as exc:
        raise SuccessorProduceV4Error(
            f"output collectability traversal failed: {exc}"
        ) from exc

    observed = frozenset(entries)
    if observed not in COLLECTABLE_PROGRESS_STATES:
        raise SuccessorProduceV4Error("output collectability member set differs")

    directory_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        directory_flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        directory_flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        directory_flags |= os.O_CLOEXEC
    directory_descriptor = os.open(str(root), directory_flags)
    opened: list[int] = []
    inode_keys: set[tuple[int, int]] = set()
    try:
        directory_info = os.fstat(directory_descriptor)
        if (
            directory_info.st_uid != os.geteuid()
            or directory_info.st_gid != os.getegid()
        ):
            raise SuccessorProduceV4Error(
                "output collectability owner differs"
            )
        for name in sorted(entries, key=os.fsencode):
            try:
                descriptor = os.open(
                    name, _output_file_flags(), dir_fd=directory_descriptor
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"output collectability member is unreadable: {name}"
                ) from exc
            opened.append(descriptor)
            info = os.fstat(descriptor)
            if not stat.S_ISREG(info.st_mode):
                raise SuccessorProduceV4Error(
                    f"output collectability contains unsafe file kind: {name}"
                )
            if info.st_nlink != 1:
                raise SuccessorProduceV4Error(
                    f"output collectability link count differs: {name}"
                )
            inode_key = (info.st_dev, info.st_ino)
            if inode_key in inode_keys:
                raise SuccessorProduceV4Error(
                    "output collectability members share an inode"
                )
            inode_keys.add(inode_key)
            if info.st_uid != os.geteuid() or info.st_gid != os.getegid():
                raise SuccessorProduceV4Error(
                    f"output collectability owner differs: {name}"
                )
            mode = stat.S_IMODE(info.st_mode)
            if mode & (
                stat.S_ISUID | stat.S_ISGID | stat.S_ISVTX | 0o111 | 0o022
            ):
                raise SuccessorProduceV4Error(
                    f"output collectability has unsafe mode: {name}"
                )
            try:
                os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"output collectability chmod failed: {name}: {exc}"
                ) from exc
            after = os.fstat(descriptor)
            if (
                (after.st_dev, after.st_ino) != inode_key
                or after.st_nlink != 1
                or after.st_uid != os.geteuid()
                or after.st_gid != os.getegid()
                or stat.S_IMODE(after.st_mode) != COLLECTABLE_FILE_MODE
            ):
                raise SuccessorProduceV4Error(
                    f"output collectability seal differs: {name}"
                )
            os.fsync(descriptor)
        if frozenset(os.listdir(directory_descriptor)) != observed:
            raise SuccessorProduceV4Error(
                "output collectability member set changed during sealing"
            )
        try:
            os.fchmod(directory_descriptor, 0o755)
            os.fsync(directory_descriptor)
        except OSError as exc:
            raise SuccessorProduceV4Error(
                f"output collectability directory chmod failed: {exc}"
            ) from exc
    finally:
        for descriptor in opened:
            try:
                os.close(descriptor)
            except OSError:
                pass
        os.close(directory_descriptor)


def _write_unqualified(
    request: ProductionRequest, failure: BaseException
) -> Optional[pathlib.Path]:
    path = request.outputs / UNQUALIFIED_MARKER_NAME
    if os.path.lexists(path):
        return None
    files = []
    for candidate in sorted(request.outputs.iterdir(), key=lambda item: os.fsencode(item.name)):
        try:
            info = candidate.lstat()
        except OSError:
            continue
        if stat.S_ISREG(info.st_mode) and candidate.name not in {
            CONSUMED_MARKER_NAME,
            UNQUALIFIED_MARKER_NAME,
        } and not candidate.name.startswith("."):
            files.append(candidate.name)
    document = {
        "attemptConsumed": True,
        "attemptId": request.chain.attempt_id,
        "authoritySha256": request.chain.identities["A6"].sha256,
        "failure": f"{type(failure).__name__}: {failure}",
        "filesKept": files,
        "mayBeAdopted": False,
        "mayBeBooted": False,
        "qualifiedImage": False,
        "schema": (
            "boole.native-shadow.mac3.launcher-v2-successor-production-"
            "unqualified-diagnostic.arm64.v4"
        ),
        "status": "UNQUALIFIED-DIAGNOSTIC",
    }
    _publish_json_once(path, document)
    try:
        sys.stdout.write(f"{UNQUALIFIED_MARKER_NAME}\n")
        sys.stdout.write(canonical_json(document).decode("utf-8"))
        sys.stdout.flush()
    except OSError:
        pass
    return path


@contextlib.contextmanager
def _consumed_attempt(request: ProductionRequest):
    _write_consumed_marker(request)
    primary: Optional[BaseException] = None
    try:
        yield
    except BaseException as failure:
        primary = failure
        try:
            _write_unqualified(request, failure)
        except BaseException as diagnostic_failure:
            try:
                sys.stderr.write(
                    f"{UNQUALIFIED_MARKER_NAME}: cannot record: {diagnostic_failure}\n"
                )
            except OSError:
                pass
        raise
    finally:
        try:
            _make_outputs_collectable(request.outputs)
        except BaseException as collectability_failure:
            if primary is not None:
                raise SuccessorProduceV4Error(
                    "output collectability failed after original failure "
                    f"{type(primary).__name__}: {primary}; "
                    f"collectability failure: {collectability_failure}"
                ) from primary
            raise


def _manifest(outputs: pathlib.Path, names: Sequence[str]) -> dict[str, str]:
    if tuple(names) != OUTPUT_NAMES:
        raise SuccessorProduceV4Error("image manifest does not name exactly three outputs")
    with _pinned_outputs(outputs, OUTPUT_NAMES) as pinned:
        result: dict[str, str] = {}
        for name in OUTPUT_NAMES:
            identity = pinned[name]
            if identity.size_bytes == 0:
                raise SuccessorProduceV4Error(f"image output is empty: {name}")
            result[name] = identity.sha256
        return result


def _pending_result(
    request: ProductionRequest,
    prepared: PreparedProduction,
    kernel: Mapping[str, Any],
    root_disk: Mapping[str, Any],
    verification: Mapping[str, Any],
    manifest: Mapping[str, str],
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "attemptId": request.chain.attempt_id,
        "authoritySha256": request.chain.identities["A6"].sha256,
        "bootableClaim": False,
        "builderMeasurement": dict(prepared.measurement),
        "generationBindings": {
            name: request.chain.identities[name].record()
            for name in ("P2", "R1", "F5", "R2", "F6", "A6")
        },
        "kernel": dict(kernel),
        "outputManifest": [
            {"name": name, "sha256": manifest[name]} for name in OUTPUT_NAMES
        ],
        "qualifiedForReplicaComparison": False,
        "readbackRequired": READBACK_RESULT_NAME,
        "rootDisk": dict(root_disk),
        "schema": PENDING_RESULT_SCHEMA,
        "status": PRODUCTION_PENDING_STATUS,
        "verification": dict(verification),
    }


def _expected_readback_entry_count(repository_root: pathlib.Path) -> int:
    identity, preregistration = _load_canonical(repository_root, P1_PATH)
    if identity.sha256 != P1_SHA256 or identity.size_bytes != P1_SIZE_BYTES:
        raise SuccessorProduceV4Error("P1 pinned identity differs for readback count")
    try:
        staging_entries = preregistration["expectedPreflight"]["measurement"][
            "entries"
        ]
    except (KeyError, TypeError) as exc:
        raise SuccessorProduceV4Error(
            "P1 has no sealed staging entry count"
        ) from exc
    if type(staging_entries) is not int or staging_entries < 0:
        raise SuccessorProduceV4Error("P1 staging entry count differs")
    # The readback walker counts the ext4 filesystem root in addition to the
    # sealed staging rows.  This relationship is independent of the document
    # being admitted and yields 17,677 for the pinned P1.
    return staging_entries + 1


def _validate_readback_verification(report: Any) -> None:
    expected_keys = {
        "activationAllowed",
        "bootableClaim",
        "checks",
        "guestBootVerified",
        "passed",
    }
    if not isinstance(report, Mapping) or set(report) != expected_keys:
        raise SuccessorProduceV4Error("readback-v3 verification schema differs")
    for key in ("activationAllowed", "bootableClaim", "guestBootVerified"):
        if type(report[key]) is not bool or report[key] is not False:
            raise SuccessorProduceV4Error(
                f"readback-v3 verification {key} differs"
            )
    if type(report["passed"]) is not bool:
        raise SuccessorProduceV4Error("readback-v3 verification passed differs")
    checks = report["checks"]
    if not isinstance(checks, list) or len(checks) != len(
        READBACK_REQUIRED_CHECKS
    ):
        raise SuccessorProduceV4Error(
            "readback-v3 verification check count differs"
        )
    observed: list[str] = []
    all_ok = True
    for row in checks:
        if not isinstance(row, Mapping) or set(row) != {"detail", "id", "ok"}:
            raise SuccessorProduceV4Error(
                "readback-v3 verification check schema differs"
            )
        if not isinstance(row["detail"], str) or not isinstance(row["id"], str):
            raise SuccessorProduceV4Error(
                "readback-v3 verification check text differs"
            )
        if type(row["ok"]) is not bool:
            raise SuccessorProduceV4Error(
                "readback-v3 verification check boolean differs"
            )
        observed.append(row["id"])
        all_ok = all_ok and row["ok"]
    if tuple(observed) != READBACK_REQUIRED_CHECKS:
        raise SuccessorProduceV4Error(
            "readback-v3 verification check identifiers differ"
        )
    if report["passed"] is not all_ok or report["passed"] is not True:
        raise SuccessorProduceV4Error("readback-v3 verification did not pass")


def _validate_pending_evidence(
    document: Mapping[str, Any],
    pinned: Mapping[str, PinnedOutputIdentity],
) -> None:
    kernel = document.get("kernel")
    if not isinstance(kernel, Mapping) or set(kernel) != {
        "activationAllowed",
        "bootableClaim",
        "kernel",
    }:
        raise SuccessorProduceV4Error("pending kernel evidence differs")
    if kernel.get("activationAllowed") is not False or kernel.get(
        "bootableClaim"
    ) is not False:
        raise SuccessorProduceV4Error("pending kernel evidence differs")
    kernel_identity = pinned["guest-kernel"]
    _require_strict_equal(
        kernel.get("kernel"),
        {
            "architecture": "aarch64",
            "magicOffset": 0x38,
            "name": "guest-kernel",
            "sha256": kernel_identity.sha256,
            "sizeBytes": kernel_identity.size_bytes,
        },
        "pending kernel identity",
    )

    root_disk = document.get("rootDisk")
    if not isinstance(root_disk, Mapping) or set(root_disk) != {
        "activationAllowed",
        "bootableClaim",
        "image",
    }:
        raise SuccessorProduceV4Error("pending root-disk evidence differs")
    if root_disk.get("activationAllowed") is not False or root_disk.get(
        "bootableClaim"
    ) is not False:
        raise SuccessorProduceV4Error("pending root-disk evidence differs")
    root_disk_identity = pinned["guest-root-disk"]
    _require_strict_equal(
        root_disk.get("image"),
        {
            "name": "guest-root-disk",
            "sha256": root_disk_identity.sha256,
            "sizeBytes": root_disk_identity.size_bytes,
        },
        "pending root-disk identity",
    )

    try:
        _validate_readback_verification(document.get("verification"))
    except SuccessorProduceV4Error as exc:
        raise SuccessorProduceV4Error("pending verification differs") from exc


def _assert_readback_pass(
    document: Mapping[str, Any], expected_image: FileIdentity, expected_entry_count: int
) -> None:
    if not isinstance(document, Mapping):
        raise SuccessorProduceV4Error("readback-v3 returned no document")
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
    _require_exact_keys(document, expected_keys, "readback-v3")
    if document.get("schema") != (
        "boole.native-shadow.successor-root-disk-readback.arm64.v3"
    ) or document.get("release") != (
        "NATIVE-SHADOW-SUCCESSOR-ROOT-DISK-READBACK-ARM64-V3"
    ):
        raise SuccessorProduceV4Error("readback-v3 schema or release differs")
    for key in ("activationAllowed", "bootableClaim", "guestBootVerified"):
        if document.get(key) is not False:
            raise SuccessorProduceV4Error(f"readback-v3 {key} differs")
    if document.get("status") != READBACK_PASS_STATUS:
        raise SuccessorProduceV4Error("readback-v3 did not pass")
    if document.get("artifactClass") != "QUALIFIED-READBACK":
        raise SuccessorProduceV4Error("readback-v3 artifact is not qualified")
    if document.get("mayEnterQualification") is not True or document.get(
        "qualifiedForReplicaComparison"
    ) is not True:
        raise SuccessorProduceV4Error("readback-v3 barred replica comparison")
    if (
        type(expected_entry_count) is not int
        or expected_entry_count < 0
        or type(document.get("entryCount")) is not int
        or document.get("entryCount") != expected_entry_count
    ):
        raise SuccessorProduceV4Error("readback-v3 entry count differs")
    _require_strict_equal(
        document.get("image"),
        {
            "name": "guest-root-disk",
            "sha256": expected_image.sha256,
            "sizeBytes": expected_image.size_bytes,
        },
        "readback-v3 image identity",
    )
    _require_strict_equal(
        document.get("sourceLock"),
        {"path": SOURCE_LOCK_PATH, "sha256": SOURCE_LOCK_SHA256},
        "readback-v3 source-lock binding",
    )
    _require_strict_equal(
        document.get("producerPreregistration"),
        {"path": P1_PATH, "sha256": P1_SHA256},
        "readback-v3 producer binding",
    )
    _require_strict_equal(
        document.get("importClosureCorrection"),
        {
            "path": (
                "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-import-closure-correction-arm64-v1.json"
            ),
            "sha256": (
                "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498"
            ),
        },
        "readback-v3 import-closure binding",
    )
    _require_strict_equal(
        document.get("launcherResult"),
        {
            "launcherSha256": (
                "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"
            ),
            "path": "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
            "sha256": (
                "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08"
            ),
        },
        "readback-v3 launcher binding",
    )
    _validate_readback_verification(document.get("verification"))


def _real_directory(path: pathlib.Path, context: str) -> pathlib.Path:
    path = pathlib.Path(path)
    try:
        info = path.lstat()
    except OSError as exc:
        raise SuccessorProduceV4Error(f"{context} is absent: {path}") from exc
    if not stat.S_ISDIR(info.st_mode) or path.is_symlink():
        raise SuccessorProduceV4Error(f"{context} is not a real directory: {path}")
    return path


def _require_empty_real_directory(path: pathlib.Path, context: str) -> None:
    path = _real_directory(path, context)
    before = path.lstat()
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    descriptor: Optional[int] = None
    try:
        descriptor = os.open(path, flags)
        reached = os.fstat(descriptor)
        if (reached.st_dev, reached.st_ino) != (before.st_dev, before.st_ino):
            raise SuccessorProduceV4Error(f"{context} identity changed")
        if os.listdir(descriptor):
            raise SuccessorProduceV4Error(f"{context} must be exactly empty")
    except SuccessorProduceV4Error:
        raise
    except OSError as exc:
        raise SuccessorProduceV4Error(f"{context} could not be inspected") from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)


@contextlib.contextmanager
def _pinned_temporary_directory(
    scratch: pathlib.Path,
) -> Iterable[pathlib.Path]:
    """Keep every implicit tempfile under the already-authorised scratch root."""

    scratch_root = _real_directory(scratch, "scratch root")
    previous = tempfile.tempdir
    environment_names = ("TMPDIR", "TMP", "TEMP")
    previous_environment = {
        name: os.environ.get(name) for name in environment_names
    }
    with tempfile.TemporaryDirectory(prefix="tmp-v4-", dir=scratch_root) as raw:
        temporary_root = _real_directory(pathlib.Path(raw), "v4 temporary root")
        tempfile.tempdir = str(temporary_root)
        for name in environment_names:
            os.environ[name] = str(temporary_root)
        try:
            yield temporary_root
        finally:
            for name, value in previous_environment.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value
            tempfile.tempdir = previous


def _launcher_bytes(
    path: pathlib.Path, *, require_sealed: bool = False
) -> bytes:
    launcher = pathlib.Path(path)
    try:
        _, raw = _read_regular(
            launcher.parent,
            launcher.name,
            max_bytes=MAX_LAUNCHER_BYTES,
        )
    except SuccessorProduceV4Error:
        raise
    except OSError as exc:
        raise SuccessorProduceV4Error("launcher-v2 binary is unreadable") from exc
    if not raw:
        raise SuccessorProduceV4Error(
            "launcher-v2 binary is not a nonempty regular file"
        )
    if require_sealed and (
        len(raw) != SEALED_LAUNCHER_SIZE_BYTES
        or _sha256(raw) != SEALED_LAUNCHER_SHA256
    ):
        raise SuccessorProduceV4Error("launcher-v2 sealed identity differs")
    return raw


def _expected_cgroup_execution_envelope() -> dict[str, Any]:
    return {
        "cgroupV2": {
            "equalAtBeforeAndAfterObservations": True,
            "leafControlsKernelObserved": True,
            "limitEventsKernelObserved": True,
            "memoryHighEvents": 0,
            "memoryMaxBytes": R2_MEMORY_MAX_BYTES,
            "memoryMaxEvents": 0,
            "memoryOomEvents": 0,
            "memoryOomKillEvents": 0,
            "memorySwapMaxBytes": R2_MEMORY_SWAP_MAX_BYTES,
            "pidsMaxEvents": 0,
            "pidsMax": R2_PIDS_MAX,
            "requestedUnitMembershipMatched": True,
        },
        "systemdRuntimeMaxSec": {
            "evidence": "source-pinned-request-and-exact-unit-membership-at-exec",
            "execReachedRequestedUnit": True,
            "kernelObserved": False,
            "managerValueQueried": False,
            "requestedSeconds": R2_RUNTIME_MAX_SECONDS,
            "sourcePinnedRequestPresent": True,
        },
    }


def _read_bounded_descriptor(fd: int, limit: int, context: str) -> bytes:
    chunks: list[bytes] = []
    consumed = 0
    while consumed <= limit:
        chunk = os.read(fd, min(64, limit + 1 - consumed))
        if not chunk:
            break
        chunks.append(chunk)
        consumed += len(chunk)
    raw = b"".join(chunks)
    if len(raw) > limit:
        raise SuccessorProduceV4Error(f"{context} exceeds its byte limit")
    return raw


def _read_cgroup_execution_observation(
    *,
    expected_systemd_unit: str,
    proc_cgroup_path: pathlib.Path = pathlib.Path("/proc/self/cgroup"),
    cgroup_root: pathlib.Path = pathlib.Path("/sys/fs/cgroup"),
    mountinfo_path: pathlib.Path = pathlib.Path("/proc/self/mountinfo"),
) -> tuple[str, dict[str, int]]:
    """Read the live cgroup-v2 caps without trusting wrapper arguments."""

    proc_fd = -1
    mountinfo_fd = -1
    root_fd = -1
    current_fd = -1
    try:
        if re.fullmatch(
            r"boole-nsv4-rehearsal-[A-Za-z0-9]{6}\.service",
            expected_systemd_unit,
        ) is None:
            raise SuccessorProduceV4Error(
                "the expected rehearsal systemd unit name differs"
            )
        proc_fd = os.open(os.fspath(proc_cgroup_path), os.O_RDONLY | os.O_NOFOLLOW)
        proc_info = os.fstat(proc_fd)
        if not stat.S_ISREG(proc_info.st_mode):
            raise SuccessorProduceV4Error("the process cgroup record is not regular")
        raw_membership = _read_bounded_descriptor(
            proc_fd, MAX_PROC_CGROUP_BYTES, "the process cgroup record"
        )
        try:
            membership = raw_membership.decode("ascii")
        except UnicodeDecodeError as exc:
            raise SuccessorProduceV4Error(
                "the process cgroup record is not ASCII"
            ) from exc
        match = re.fullmatch(r"0::(/[^\n]*)\n", membership)
        if match is None:
            raise SuccessorProduceV4Error(
                "the process is not in one exact cgroup-v2 membership"
            )
        membership_path = match.group(1)
        expected_membership = f"/system.slice/{expected_systemd_unit}"
        if membership_path != expected_membership:
            raise SuccessorProduceV4Error(
                "the process cgroup-v2 membership differs from the requested unit"
            )
        relative = membership_path.removeprefix("/")
        components = relative.split("/") if relative else []
        if not components or any(
            not component or component in (".", "..") for component in components
        ):
            raise SuccessorProduceV4Error("the process cgroup-v2 path differs")

        mountinfo_fd = os.open(
            os.fspath(mountinfo_path), os.O_RDONLY | os.O_NOFOLLOW
        )
        mountinfo_info = os.fstat(mountinfo_fd)
        if not stat.S_ISREG(mountinfo_info.st_mode):
            raise SuccessorProduceV4Error("the process mountinfo is not regular")
        raw_mountinfo = _read_bounded_descriptor(
            mountinfo_fd, MAX_MOUNTINFO_BYTES, "the process mountinfo"
        )
        cgroup_mounts = _parse_live_recovery_mountinfo(
            raw_mountinfo, pathlib.Path(cgroup_root)
        )
        if (
            len(cgroup_mounts) != 1
            or cgroup_mounts[0]["fileSystemType"] != "cgroup2"
            or cgroup_mounts[0]["root"] != "/"
        ):
            raise SuccessorProduceV4Error(
                "the kernel cgroup root is not one cgroup-v2 mount"
            )

        root_fd = os.open(
            os.fspath(cgroup_root),
            os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
        )
        current_fd = os.dup(root_fd)
        for component in components:
            following = os.open(
                component,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=current_fd,
            )
            os.close(current_fd)
            current_fd = following

        observed: dict[str, int] = {}
        for file_name, field_name, expected in (
            ("memory.max", "memoryMaxBytes", R2_MEMORY_MAX_BYTES),
            ("memory.swap.max", "memorySwapMaxBytes", R2_MEMORY_SWAP_MAX_BYTES),
            ("pids.max", "pidsMax", R2_PIDS_MAX),
        ):
            value_fd = -1
            try:
                value_fd = os.open(
                    file_name,
                    os.O_RDONLY | os.O_NOFOLLOW,
                    dir_fd=current_fd,
                )
                value_info = os.fstat(value_fd)
                if not stat.S_ISREG(value_info.st_mode):
                    raise SuccessorProduceV4Error(
                        f"the cgroup {file_name} value is not regular"
                    )
                raw_value = _read_bounded_descriptor(
                    value_fd,
                    MAX_CGROUP_VALUE_BYTES,
                    f"the cgroup {file_name} value",
                )
            finally:
                if value_fd >= 0:
                    os.close(value_fd)
            if re.fullmatch(rb"(?:0|[1-9][0-9]*)\n", raw_value) is None:
                raise SuccessorProduceV4Error(
                    f"the cgroup {file_name} limit is not one finite integer"
                )
            value = int(raw_value[:-1], 10)
            if value != expected:
                raise SuccessorProduceV4Error(
                    f"the kernel-observed cgroup {file_name} limit differs"
                )
            observed[field_name] = value

        for file_name, required_fields in (
            (
                "memory.events.local",
                {
                    "high": "memoryHighEvents",
                    "max": "memoryMaxEvents",
                    "oom": "memoryOomEvents",
                    "oom_kill": "memoryOomKillEvents",
                },
            ),
            ("pids.events", {"max": "pidsMaxEvents"}),
        ):
            events_fd = -1
            try:
                events_fd = os.open(
                    file_name,
                    os.O_RDONLY | os.O_NOFOLLOW,
                    dir_fd=current_fd,
                )
                events_info = os.fstat(events_fd)
                if not stat.S_ISREG(events_info.st_mode):
                    raise SuccessorProduceV4Error(
                        f"the cgroup {file_name} value is not regular"
                    )
                raw_events = _read_bounded_descriptor(
                    events_fd,
                    MAX_CGROUP_EVENTS_BYTES,
                    f"the cgroup {file_name} value",
                )
            finally:
                if events_fd >= 0:
                    os.close(events_fd)
            try:
                event_text = raw_events.decode("ascii")
            except UnicodeDecodeError as exc:
                raise SuccessorProduceV4Error(
                    f"the cgroup {file_name} limit events are not ASCII"
                ) from exc
            if not event_text.endswith("\n"):
                raise SuccessorProduceV4Error(
                    f"the cgroup {file_name} limit events differ"
                )
            parsed_events: dict[str, int] = {}
            for line in event_text.splitlines():
                match = re.fullmatch(r"([a-z_]+) (0|[1-9][0-9]*)", line)
                if match is None or match.group(1) in parsed_events:
                    raise SuccessorProduceV4Error(
                        f"the cgroup {file_name} limit events differ"
                    )
                parsed_events[match.group(1)] = int(match.group(2), 10)
            if not required_fields.keys() <= parsed_events.keys():
                raise SuccessorProduceV4Error(
                    f"the cgroup {file_name} limit events differ"
                )
            for event_name, field_name in required_fields.items():
                value = parsed_events[event_name]
                if value != 0:
                    raise SuccessorProduceV4Error(
                        f"the cgroup {file_name} limit event is nonzero"
                    )
                observed[field_name] = value
    except SuccessorProduceV4Error:
        raise
    except OSError as exc:
        raise SuccessorProduceV4Error(
            "the kernel-observed cgroup-v2 envelope is unreadable"
        ) from exc
    finally:
        if current_fd >= 0:
            os.close(current_fd)
        if root_fd >= 0:
            os.close(root_fd)
        if mountinfo_fd >= 0:
            os.close(mountinfo_fd)
        if proc_fd >= 0:
            os.close(proc_fd)

    return membership_path, observed


def rehearse(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    expected_systemd_unit: str,
    backend: Optional[ImageBackend] = None,
) -> dict[str, Any]:
    """Run shared staging with no authority, marker, output directory or image."""

    root = pathlib.Path(repository_root).resolve()
    preregistered = verify_preregistered_generation(root)
    for relative, context in (
        (R2_PATH, "fresh R2"),
        (F6_PATH, "future F6"),
        (A6_PATH, "future A6"),
        (RESULT_V6_PATH, "future result-v6"),
    ):
        _require_absent(root, relative, context)
    store = _real_directory(artifact_store, "artifact store")
    scratch_root = _real_directory(scratch, "scratch root")
    no_outputs = scratch_root / ".rehearsal-must-not-create-outputs"
    _require_absent(scratch_root, no_outputs.name, "rehearsal output sentinel")
    _require_empty_real_directory(scratch_root, "rehearsal scratch")
    request = ProductionRequest(
        repository_root=root,
        artifact_store=store,
        outputs=no_outputs,
        scratch=scratch_root,
        gpgv=pathlib.Path(gpgv),
        zstd=pathlib.Path(zstd),
        launcher=pathlib.Path(launcher),
        launcher_binary=_launcher_bytes(launcher, require_sealed=backend is None),
        chain=preregistered,
    )
    execution_before = _read_cgroup_execution_observation(
        expected_systemd_unit=expected_systemd_unit
    )
    selected: ImageBackend = RepositoryImageBackend() if backend is None else backend
    with _pinned_temporary_directory(scratch_root):
        prepared = selected.prepare(request)
    execution_after = _read_cgroup_execution_observation(
        expected_systemd_unit=expected_systemd_unit
    )
    if execution_after != execution_before:
        raise SuccessorProduceV4Error(
            "the kernel-observed cgroup-v2 envelope changed during staging"
        )
    if os.path.lexists(no_outputs):
        raise SuccessorProduceV4Error("rehearsal created a production output directory")
    if not isinstance(prepared, PreparedProduction):
        raise SuccessorProduceV4Error("rehearsal backend returned no prepared staging")
    predecessors = _identity_rows(preregistered.predecessor_identities)
    generation = _identity_rows(preregistered.generation_identities)
    reused = _identity_rows(preregistered.reused_identities)
    return {
        "activationAllowed": False,
        "authorisations": dict(ZERO_AUTHORISATIONS),
        "bootableClaim": False,
        "boundInputs": [*predecessors, *generation, *reused],
        "effects": dict(ZERO_EFFECTS),
        "executionEnvelope": _expected_cgroup_execution_envelope(),
        "generationFiles": generation,
        "measurement": dict(prepared.measurement),
        "predecessors": predecessors,
        P3_FIELD: preregistered.dispatch_fence_identity.record(),
        "repeatable": True,
        "reusedPinnedUpstream": reused,
        "schema": R2_SCHEMA,
        "status": R2_STATUS,
    }


def preflight(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    backend: Optional[ImageBackend] = None,
) -> dict[str, Any]:
    """Exercise the production assembly after A6 without spending its run."""

    root = pathlib.Path(repository_root).resolve()
    chain = verify_generation_chain(root)
    store = _real_directory(artifact_store, "artifact store")
    scratch_root = _real_directory(scratch, "scratch root")
    no_outputs = scratch_root / ".preflight-must-not-create-outputs"
    _require_absent(scratch_root, no_outputs.name, "preflight output sentinel")
    _require_empty_real_directory(scratch_root, "preflight scratch")
    request = ProductionRequest(
        repository_root=root,
        artifact_store=store,
        outputs=no_outputs,
        scratch=scratch_root,
        gpgv=pathlib.Path(gpgv),
        zstd=pathlib.Path(zstd),
        launcher=pathlib.Path(launcher),
        launcher_binary=_launcher_bytes(launcher, require_sealed=backend is None),
        chain=chain,
    )
    selected: ImageBackend = RepositoryImageBackend() if backend is None else backend
    with _pinned_temporary_directory(scratch_root):
        prepared = selected.prepare(request)
    if os.path.lexists(no_outputs):
        raise SuccessorProduceV4Error("preflight created a production output directory")
    if not isinstance(prepared, PreparedProduction):
        raise SuccessorProduceV4Error("preflight backend returned no prepared staging")
    return {
        "activationAllowed": False,
        "attemptId": chain.attempt_id,
        "authoritySha256": chain.identities["A6"].sha256,
        "bootableClaim": False,
        "buildReceipt": dict(prepared.build_receipt),
        "effects": dict(ZERO_EFFECTS),
        "measurement": dict(prepared.measurement),
        "outputsCreated": False,
        "schema": (
            "boole.native-shadow.mac3.launcher-v2-successor-production-preflight."
            "arm64.v4"
        ),
        "status": "PASS-NO-IMAGE-PRODUCED",
    }


def produce(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    backend: Optional[ImageBackend] = None,
    dispatch_capability: Optional[Mapping[str, Any]] = None,
) -> dict[str, Any]:
    """Produce once and admit the result only across the readback-v3 edge."""

    chain = verify_generation_chain(repository_root)
    frozen_dispatch = _verified_dispatch_capability(
        repository_root, chain, dispatch_capability
    )
    if backend is None and (os.geteuid() != 0 or os.getegid() != 0):
        raise SuccessorProduceV4Error("real production backend requires root:root")
    store = _real_directory(artifact_store, "artifact store")
    scratch_root = _real_directory(scratch, "scratch root")
    _require_empty_real_directory(scratch_root, "production scratch")
    output_root = pathlib.Path(outputs)
    if os.path.lexists(output_root):
        raise SuccessorProduceV4Error("fresh output directory already exists")
    request = ProductionRequest(
        repository_root=pathlib.Path(repository_root).resolve(),
        artifact_store=store,
        outputs=output_root,
        scratch=scratch_root,
        gpgv=pathlib.Path(gpgv),
        zstd=pathlib.Path(zstd),
        launcher=pathlib.Path(launcher),
        launcher_binary=_launcher_bytes(launcher, require_sealed=backend is None),
        chain=chain,
        dispatch_capability=frozen_dispatch,
    )
    selected: ImageBackend = RepositoryImageBackend() if backend is None else backend
    with _pinned_temporary_directory(scratch_root):
        prepared = selected.prepare(request)
        if not isinstance(prepared, PreparedProduction):
            raise SuccessorProduceV4Error(
                "production backend returned no prepared staging"
            )

        # Recheck the immutable guard-created tag bytes at the last point before
        # the output directory and durable attempt marker can exist.
        _reverify_dispatch_capability(repository_root, chain, frozen_dispatch)
        _mkdir_durable_once(output_root)
        with _consumed_attempt(request):
            kernel = selected.extract_kernel(request, prepared)
            initrd = selected.build_initrd(request, prepared)
            if not isinstance(initrd, bytes) or not initrd:
                raise SuccessorProduceV4Error("initrd builder returned no bytes")
            _publish_bytes_once(output_root / "guest-initrd", initrd)
            root_disk = selected.build_root_disk(request, prepared)
            verification = selected.verify_images(
                request, prepared, kernel, initrd, root_disk
            )
            if (
                not isinstance(verification, Mapping)
                or verification.get("passed") is not True
            ):
                raise SuccessorProduceV4Error(
                    "produced image verification did not pass"
                )
            _sync_produced_images(output_root)
            manifest = _manifest(output_root, OUTPUT_NAMES)
            pending = _pending_result(
                request,
                prepared,
                kernel,
                root_disk,
                verification,
                manifest,
            )
            _publish_json_once(output_root / PENDING_RESULT_NAME, pending)
            return pending


def _load_pending(
    outputs: pathlib.Path,
    pending_path: pathlib.Path,
    chain: GenerationChain,
    pinned: PinnedOutputs,
) -> dict[str, Any]:
    expected = outputs / PENDING_RESULT_NAME
    if pathlib.Path(pending_path) != expected:
        raise SuccessorProduceV4Error("pending result path differs from the fixed name")
    raw = pinned.read_metadata(
        PENDING_RESULT_NAME,
        "pending result",
        max_bytes=MAX_METADATA_BYTES,
    )
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SuccessorProduceV4Error("pending result is not JSON") from exc
    if not isinstance(document, dict) or raw != canonical_json(document):
        raise SuccessorProduceV4Error("pending result is not canonical JSON")
    expected_keys = {
        "activationAllowed",
        "attemptId",
        "authoritySha256",
        "bootableClaim",
        "builderMeasurement",
        "generationBindings",
        "kernel",
        "outputManifest",
        "qualifiedForReplicaComparison",
        "readbackRequired",
        "rootDisk",
        "schema",
        "status",
        "verification",
    }
    _require_exact_keys(document, expected_keys, "pending result")
    if document["schema"] != PENDING_RESULT_SCHEMA:
        raise SuccessorProduceV4Error("pending result schema differs")
    if document["status"] != PRODUCTION_PENDING_STATUS:
        raise SuccessorProduceV4Error("pending result status differs")
    if document["qualifiedForReplicaComparison"] is not False:
        raise SuccessorProduceV4Error("pending result claims qualification")
    if document["attemptId"] != chain.attempt_id:
        raise SuccessorProduceV4Error("pending result attempt differs")
    if document["authoritySha256"] != chain.identities["A6"].sha256:
        raise SuccessorProduceV4Error("pending result authority differs")
    expected_bindings = {
        name: chain.identities[name].record()
        for name in ("P2", "R1", "F5", "R2", "F6", "A6")
    }
    if document["generationBindings"] != expected_bindings:
        raise SuccessorProduceV4Error("pending result generation bindings differ")
    try:
        expected_measurement = chain.fresh_rehearsal["measurement"]
    except (KeyError, TypeError) as exc:
        raise SuccessorProduceV4Error(
            "R2 has no sealed staging measurement"
        ) from exc
    if not isinstance(expected_measurement, Mapping):
        raise SuccessorProduceV4Error("pending builder measurement differs")
    _require_strict_equal(
        document["builderMeasurement"],
        dict(expected_measurement),
        "pending builder measurement",
    )
    if document["readbackRequired"] != READBACK_RESULT_NAME:
        raise SuccessorProduceV4Error("pending result names another readback")
    if document["activationAllowed"] is not False or document["bootableClaim"] is not False:
        raise SuccessorProduceV4Error("pending result claims activation or boot")
    if not set(OUTPUT_NAMES).issubset(pinned):
        raise SuccessorProduceV4Error("pending output pins omit an image")
    expected_manifest = [
        {"name": name, "sha256": pinned[name].sha256} for name in OUTPUT_NAMES
    ]
    if document["outputManifest"] != expected_manifest:
        raise SuccessorProduceV4Error("pending output manifest differs from image bytes")
    _validate_pending_evidence(document, pinned)
    return document


def _load_canonical_output(
    outputs: pathlib.Path,
    name: str,
    context: str,
    *,
    pinned: Optional[PinnedOutputs] = None,
) -> dict[str, Any]:
    if pinned is None:
        with _pinned_outputs(outputs, (name,)) as opened:
            document = _load_canonical_output(
                outputs, name, context, pinned=opened
            )
            _assert_pinned_outputs_unchanged(outputs, opened)
            return document
    raw = pinned.read_metadata(name, context, max_bytes=MAX_METADATA_BYTES)
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SuccessorProduceV4Error(f"{context} is not JSON") from exc
    if not isinstance(document, dict) or raw != canonical_json(document):
        raise SuccessorProduceV4Error(f"{context} is not canonical JSON")
    return document


def _qualified_result_document(
    chain: GenerationChain,
    pending_document: Mapping[str, Any],
    readback: Mapping[str, Any],
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "artifactClass": "QUALIFIED-PRODUCTION",
        "attemptId": chain.attempt_id,
        "authoritySha256": chain.identities["A6"].sha256,
        "bootableClaim": False,
        "outputManifest": pending_document["outputManifest"],
        "pendingResult": {
            "name": PENDING_RESULT_NAME,
            "sha256": _sha256(canonical_json(dict(pending_document))),
        },
        "qualifiedForReplicaComparison": True,
        "readback": dict(readback),
        "schema": (
            "boole.native-shadow.mac3.launcher-v2-successor-production-qualified-"
            "readback.arm64.v4"
        ),
        "status": PRODUCTION_QUALIFIED_STATUS,
    }


def qualify(
    *,
    repository_root: pathlib.Path,
    outputs: pathlib.Path,
    pending: pathlib.Path,
    result: pathlib.Path,
    backend: Optional[ImageBackend] = None,
) -> dict[str, Any]:
    """Outside PrivateDevices, run readback-v3 once and publish qualification."""

    output_argument = pathlib.Path(outputs)
    result_path = pathlib.Path(result)
    if result_path != output_argument / QUALIFIED_RESULT_NAME:
        raise SuccessorProduceV4Error("qualified result path differs from the fixed name")
    root = pathlib.Path(repository_root).resolve()
    chain = verify_generation_chain(root)
    output_root = _real_directory(output_argument, "production outputs")
    expected_entry_count = _expected_readback_entry_count(root)
    with _pinned_outputs(output_root, QUALIFICATION_PINNED_NAMES) as pinned:
        marker = _load_canonical_output(
            output_root,
            CONSUMED_MARKER_NAME,
            "consumed marker",
            pinned=pinned,
        )
        _require_strict_equal(
            marker, _marker_document(chain), "consumed marker differs from A6"
        )
        pending_document = _load_pending(
            output_root, pathlib.Path(pending), chain, pinned
        )
        _assert_pinned_outputs_unchanged(output_root, pinned)
        pinned_root_disk = pinned["guest-root-disk"]
        root_disk_identity = FileIdentity(
            "guest-root-disk",
            pinned_root_disk.sha256,
            pinned_root_disk.size_bytes,
        )
        if os.path.lexists(result_path):
            existing = _load_canonical_output(
                output_root, QUALIFIED_RESULT_NAME, "qualified result"
            )
            embedded = existing.get("readback")
            if not isinstance(embedded, Mapping):
                raise SuccessorProduceV4Error("qualified result has no readback")
            _assert_readback_pass(
                embedded, root_disk_identity, expected_entry_count
            )
            expected = _qualified_result_document(chain, pending_document, embedded)
            _require_strict_equal(existing, expected, "qualified result")
            _assert_pinned_outputs_unchanged(output_root, pinned)
            return existing
        if os.path.lexists(output_root / READBACK_RESULT_NAME):
            readback = _load_canonical_output(
                output_root, READBACK_RESULT_NAME, "existing readback-v3 result"
            )
        else:
            selected: ImageBackend = (
                RepositoryImageBackend() if backend is None else backend
            )
            readback = selected.readback(root, output_root, chain)
        _assert_readback_pass(readback, root_disk_identity, expected_entry_count)
        _assert_pinned_outputs_unchanged(output_root, pinned)
        qualified = _qualified_result_document(chain, pending_document, readback)
        _publish_json_once(result_path, qualified)
        _assert_pinned_outputs_unchanged(output_root, pinned)
        return qualified


def _qualified_replica_members(outputs: pathlib.Path) -> tuple[str, ...]:
    root = _real_directory(outputs, "qualified replica")
    try:
        with os.scandir(root) as iterator:
            entries = tuple(iterator)
    except OSError as exc:
        raise SuccessorProduceV4Error(
            f"qualified replica is unreadable: {root}"
        ) from exc
    names = tuple(sorted((entry.name for entry in entries), key=os.fsencode))
    expected = tuple(sorted(QUALIFIED_REPLICA_NAMES, key=os.fsencode))
    if names != expected:
        raise SuccessorProduceV4Error("qualified replica members differ")
    if any(not entry.is_file(follow_symlinks=False) for entry in entries):
        raise SuccessorProduceV4Error("qualified replica members differ")
    return names


def _validate_qualified_replica(
    repository_root: pathlib.Path,
    outputs: pathlib.Path,
    chain: GenerationChain,
    expected_entry_count: int,
    pinned: Optional[PinnedOutputs] = None,
) -> dict[str, FileIdentity]:
    output_root = _real_directory(outputs, "qualified replica")
    _qualified_replica_members(output_root)
    if pinned is None:
        with _pinned_outputs(output_root, QUALIFIED_REPLICA_NAMES) as opened:
            result = _validate_qualified_replica(
                repository_root,
                output_root,
                chain,
                expected_entry_count,
                opened,
            )
            _assert_pinned_outputs_unchanged(output_root, opened)
            _qualified_replica_members(output_root)
            return result
    if set(pinned) != set(QUALIFIED_REPLICA_NAMES):
        raise SuccessorProduceV4Error("qualified replica pins differ")
    marker = _load_canonical_output(
        output_root,
        CONSUMED_MARKER_NAME,
        "consumed marker",
        pinned=pinned,
    )
    _require_strict_equal(
        marker, _marker_document(chain), "consumed marker differs from A6"
    )
    pending = _load_pending(
        output_root, output_root / PENDING_RESULT_NAME, chain, pinned
    )
    root_disk = pinned["guest-root-disk"]
    readback = _load_canonical_output(
        output_root,
        READBACK_RESULT_NAME,
        "readback-v3 result",
        pinned=pinned,
    )
    _assert_readback_pass(
        readback,
        FileIdentity("guest-root-disk", root_disk.sha256, root_disk.size_bytes),
        expected_entry_count,
    )
    qualified = _load_canonical_output(
        output_root,
        QUALIFIED_RESULT_NAME,
        "qualified result",
        pinned=pinned,
    )
    _require_strict_equal(
        qualified,
        _qualified_result_document(chain, pending, readback),
        "qualified result",
    )
    return {
        name: FileIdentity(name, identity.sha256, identity.size_bytes)
        for name, identity in pinned.items()
    }


def compare_qualified_replicas(
    *,
    repository_root: pathlib.Path,
    left: pathlib.Path,
    right: pathlib.Path,
) -> dict[str, Any]:
    """Validate two complete qualified replicas and compare every file digest."""

    left_root = _real_directory(left, "left qualified replica")
    right_root = _real_directory(right, "right qualified replica")
    left_info = left_root.stat()
    right_info = right_root.stat()
    if (left_info.st_dev, left_info.st_ino) == (
        right_info.st_dev,
        right_info.st_ino,
    ):
        raise SuccessorProduceV4Error(
            "qualified replicas must use distinct directories"
        )
    root = pathlib.Path(repository_root).resolve()
    chain = verify_generation_chain(root)
    expected_entry_count = _expected_readback_entry_count(root)
    _qualified_replica_members(left_root)
    _qualified_replica_members(right_root)
    with _pinned_outputs(
        left_root, QUALIFIED_REPLICA_NAMES
    ) as left_pinned, _pinned_outputs(
        right_root, QUALIFIED_REPLICA_NAMES
    ) as right_pinned:
        left_inodes = {
            (identity.device, identity.inode) for identity in left_pinned.values()
        }
        right_inodes = {
            (identity.device, identity.inode) for identity in right_pinned.values()
        }
        if left_inodes & right_inodes:
            raise SuccessorProduceV4Error(
                "qualified replica files must not share inodes"
            )
        left_files = _validate_qualified_replica(
            root, left_root, chain, expected_entry_count, left_pinned
        )
        right_files = _validate_qualified_replica(
            root, right_root, chain, expected_entry_count, right_pinned
        )
        if {
            name: identity.record() for name, identity in left_files.items()
        } != {
            name: identity.record() for name, identity in right_files.items()
        }:
            raise SuccessorProduceV4Error("qualified replica contents differ")
        try:
            _assert_pinned_outputs_unchanged(left_root, left_pinned)
            _assert_pinned_outputs_unchanged(right_root, right_pinned)
        except SuccessorProduceV4Error as exc:
            raise SuccessorProduceV4Error(
                "qualified replica changed during replica comparison"
            ) from exc
        _qualified_replica_members(left_root)
        _qualified_replica_members(right_root)
    return {
        "activationAllowed": False,
        "attemptId": chain.attempt_id,
        "authoritySha256": chain.identities["A6"].sha256,
        "bootableClaim": False,
        "outputs": [
            {
                "name": name,
                "sha256": left_files[name].sha256,
                "sizeBytes": left_files[name].size_bytes,
            }
            for name in OUTPUT_NAMES
        ],
        "replicasCompared": 2,
        "schema": (
            "boole.native-shadow.mac3.launcher-v2-successor-qualified-replica-"
            "comparison.arm64.v4"
        ),
        "status": "TWO-QUALIFIED-REPLICAS-IDENTICAL",
    }


def _qualified_tree_document(
    repository_root: pathlib.Path,
    outputs: pathlib.Path,
    chain: GenerationChain,
) -> dict[str, Any]:
    """Describe one fully validated seven-file tree without loading images."""

    output_root = _real_directory(outputs, "qualified replica outputs")
    if output_root.name != "outputs":
        raise SuccessorProduceV4Error("replica provenance output path differs")
    expected_entry_count = _expected_readback_entry_count(repository_root)
    with _pinned_outputs(output_root, QUALIFIED_REPLICA_NAMES) as pinned:
        identities = _validate_qualified_replica(
            repository_root,
            output_root,
            chain,
            expected_entry_count,
            pinned,
        )
        _assert_pinned_outputs_unchanged(output_root, pinned)
        _qualified_replica_members(output_root)
    members = [
        {
            "name": name,
            "sha256": identities[name].sha256,
            "sizeBytes": identities[name].size_bytes,
        }
        for name in QUALIFIED_REPLICA_NAMES
    ]
    return {
        "manifestSha256": _sha256(canonical_json(members)),
        "members": members,
        "relativePath": "outputs",
    }


def replica_provenance_document(
    *,
    repository_root: pathlib.Path,
    outputs: pathlib.Path,
    replica_ordinal: int,
    strategy_job_index: int,
    strategy_job_total: int,
    github_job: str,
    artifact_name: str,
    claim_ref: str,
    ref_object_sha: str,
    tag_object_sha: str,
    raw_tag_object: bytes,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> dict[str, Any]:
    """Bind one qualified tree to one logical matrix child and dispatch tag."""

    root = pathlib.Path(repository_root).resolve()
    chain = verify_generation_chain(root)
    replicas = chain.authority.get("grant", {}).get("replicas")
    if type(replicas) is not int or replicas != 2:
        raise SuccessorProduceV4Error("replica provenance A6 count differs")
    if type(replica_ordinal) is not int or replica_ordinal not in (1, 2):
        raise SuccessorProduceV4Error("replica provenance ordinal differs")
    if type(strategy_job_index) is not int or strategy_job_index != replica_ordinal - 1:
        raise SuccessorProduceV4Error("replica provenance strategy index differs")
    if type(strategy_job_total) is not int or strategy_job_total != replicas:
        raise SuccessorProduceV4Error("replica provenance strategy total differs")
    if github_job != "produce":
        raise SuccessorProduceV4Error("replica provenance job differs")
    expected_artifact = REPLICA_ARTIFACT_PREFIX + str(replica_ordinal)
    if artifact_name != expected_artifact:
        raise SuccessorProduceV4Error("replica provenance artifact differs")
    claim = verify_dispatch_tag_object(
        chain,
        repository_root=root,
        claim_ref=claim_ref,
        ref_object_sha=ref_object_sha,
        tag_object_sha=tag_object_sha,
        raw_tag_object=raw_tag_object,
        github_run_id=github_run_id,
        github_run_attempt=github_run_attempt,
        workflow_path=workflow_path,
        head_sha=head_sha,
        head_a6_sha256=head_a6_sha256,
    )
    message = dispatch_claim_message(
        chain,
        github_run_id=github_run_id,
        github_run_attempt=github_run_attempt,
        workflow_path=workflow_path,
        head_sha=head_sha,
        head_a6_sha256=head_a6_sha256,
    )
    return {
        "activationAllowed": False,
        "artifactName": artifact_name,
        "attemptId": chain.attempt_id,
        "authoritySha256": chain.identities["A6"].sha256,
        "bootableClaim": False,
        "dispatchClaim": {
            "messageSha256": _sha256(message),
            "ref": claim_ref,
            "tagObjectSha": tag_object_sha,
        },
        "github": {
            "headSha": claim["headSha"],
            "job": github_job,
            "runAttempt": github_run_attempt,
            "runId": claim["githubRunId"],
            "workflowPath": claim["workflowPath"],
        },
        "qualifiedTree": _qualified_tree_document(root, outputs, chain),
        "replica": {
            "ordinal": replica_ordinal,
            "strategyJobIndex": strategy_job_index,
            "strategyJobTotal": strategy_job_total,
        },
        "schema": REPLICA_PROVENANCE_SCHEMA,
    }


def publish_replica_provenance(
    *,
    result: pathlib.Path,
    **arguments: Any,
) -> dict[str, Any]:
    outputs = pathlib.Path(arguments["outputs"])
    expected = outputs.parent / REPLICA_PROVENANCE_NAME
    if pathlib.Path(result) != expected:
        raise SuccessorProduceV4Error("replica provenance result path differs")
    document = replica_provenance_document(**arguments)
    _publish_json_once(expected, document)
    return document


def _sealed_regular_file(
    descriptor: int,
    *,
    name: str,
    expected_uid: int,
    expected_gid: int,
    inode_keys: set[tuple[int, int]],
) -> os.stat_result:
    """Validate one already-open bundle member before changing any mode."""

    info = os.fstat(descriptor)
    if not stat.S_ISREG(info.st_mode):
        raise SuccessorProduceV4Error(
            f"collectable replica bundle contains unsafe file kind: {name}"
        )
    if info.st_nlink != 1:
        raise SuccessorProduceV4Error(
            f"collectable replica bundle link count differs: {name}"
        )
    inode_key = (info.st_dev, info.st_ino)
    if inode_key in inode_keys:
        raise SuccessorProduceV4Error(
            "collectable replica bundle members share an inode"
        )
    inode_keys.add(inode_key)
    if info.st_uid != expected_uid or info.st_gid != expected_gid:
        raise SuccessorProduceV4Error(
            f"collectable replica bundle owner differs: {name}"
        )
    mode = stat.S_IMODE(info.st_mode)
    if mode & (stat.S_ISUID | stat.S_ISGID | stat.S_ISVTX | 0o111 | 0o022):
        raise SuccessorProduceV4Error(
            f"collectable replica bundle has unsafe mode: {name}"
        )
    return info


def seal_collectable_replica_bundle(
    *,
    parent: pathlib.Path,
    successful: bool,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    expected_provenance: Optional[Mapping[str, Any]] = None,
    expected_failure_marker: Optional[Mapping[str, Any]] = None,
    publish_provenance: bool = False,
) -> None:
    """Seal one exact replica bundle, opening parent traversal only last."""

    if type(successful) is not bool:
        raise SuccessorProduceV4Error("collectable replica bundle status differs")
    if type(publish_provenance) is not bool:
        raise SuccessorProduceV4Error(
            "collectable replica bundle provenance publication differs"
        )
    if publish_provenance and not successful:
        raise SuccessorProduceV4Error(
            "failure collection cannot publish success provenance"
        )
    if successful and expected_failure_marker is not None:
        raise SuccessorProduceV4Error(
            "successful collection cannot use a failure marker claim"
        )
    if not successful and expected_failure_marker is None:
        raise SuccessorProduceV4Error(
            "failure collection requires an exact marker claim"
        )
    if successful and expected_provenance is None:
        raise SuccessorProduceV4Error(
            "successful collection requires verified provenance"
        )
    if (
        not isinstance(expected_parent_identity, tuple)
        or len(expected_parent_identity) != 2
        or any(type(value) is not int for value in expected_parent_identity)
    ):
        raise SuccessorProduceV4Error(
            "collectable replica bundle parent identity differs"
        )
    root = _real_directory(parent, "collectable replica bundle parent")
    directory_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        directory_flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        directory_flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        directory_flags |= os.O_CLOEXEC
    parent_descriptor = os.open(str(root), directory_flags)
    output_descriptor: Optional[int] = None
    file_descriptors: dict[str, int] = {}
    provenance_descriptor: Optional[int] = None
    try:
        parent_info = os.fstat(parent_descriptor)
        if (parent_info.st_dev, parent_info.st_ino) != expected_parent_identity:
            raise SuccessorProduceV4Error(
                "collectable replica bundle parent identity changed"
            )
        if parent_info.st_uid != expected_uid or parent_info.st_gid != expected_gid:
            raise SuccessorProduceV4Error(
                "collectable replica bundle parent owner differs"
            )
        if stat.S_IMODE(parent_info.st_mode) not in (
            0o700,
            COLLECTABLE_PARENT_MODE,
        ):
            raise SuccessorProduceV4Error(
                "collectable replica bundle parent mode differs"
            )
        parent_names = frozenset(os.listdir(parent_descriptor))
        expected_success_parent = frozenset(
            ("outputs", REPLICA_PROVENANCE_NAME)
        )
        if successful:
            expected_initial_parents = (
                (
                    frozenset(("outputs",)),
                    frozenset(("outputs", REPLICA_PROVENANCE_PARTIAL_NAME)),
                    expected_success_parent,
                    frozenset(
                        (
                            "outputs",
                            REPLICA_PROVENANCE_NAME,
                            REPLICA_PROVENANCE_PARTIAL_NAME,
                        )
                    ),
                )
                if publish_provenance
                else (expected_success_parent,)
            )
            if parent_names not in expected_initial_parents:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle parent members differ"
                )
        elif parent_names != frozenset(("outputs",)):
            raise SuccessorProduceV4Error(
                "collectable replica bundle parent members differ"
            )

        try:
            output_descriptor = os.open(
                "outputs", directory_flags, dir_fd=parent_descriptor
            )
        except OSError as exc:
            raise SuccessorProduceV4Error(
                "collectable replica bundle outputs differ"
            ) from exc
        output_info = os.fstat(output_descriptor)
        if not stat.S_ISDIR(output_info.st_mode):
            raise SuccessorProduceV4Error(
                "collectable replica bundle outputs differ"
            )
        if output_info.st_uid != expected_uid or output_info.st_gid != expected_gid:
            raise SuccessorProduceV4Error(
                "collectable replica bundle outputs owner differs"
            )
        output_mode = stat.S_IMODE(output_info.st_mode)
        if output_mode & (stat.S_ISUID | stat.S_ISGID | stat.S_ISVTX | 0o022):
            raise SuccessorProduceV4Error(
                "collectable replica bundle outputs mode differs"
            )
        output_names = frozenset(os.listdir(output_descriptor))
        if successful:
            if output_names != frozenset(QUALIFIED_REPLICA_NAMES):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle output member set differs"
                )
        elif output_names not in COLLECTABLE_PROGRESS_STATES:
            raise SuccessorProduceV4Error(
                "collectable replica bundle output member set differs"
            )

        inode_keys: set[tuple[int, int]] = set()
        initial_stats: dict[str, os.stat_result] = {}
        initial_identities: dict[str, PinnedOutputIdentity] = {}
        for name in sorted(output_names, key=os.fsencode):
            try:
                descriptor = os.open(
                    name, _output_file_flags(), dir_fd=output_descriptor
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"collectable replica bundle member is unreadable: {name}"
                ) from exc
            file_descriptors[name] = descriptor
            info = _sealed_regular_file(
                descriptor,
                name=f"outputs/{name}",
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                inode_keys=inode_keys,
            )
            initial_stats[name] = info
            if successful:
                initial_identities[name] = _descriptor_output_identity(
                    descriptor,
                    f"collectable replica bundle member outputs/{name}",
                )

        if not successful and expected_failure_marker is not None:
            if not isinstance(expected_failure_marker, Mapping):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle marker claim differs"
                )
            marker_descriptor = file_descriptors.get(CONSUMED_MARKER_NAME)
            if marker_descriptor is None:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle consumed marker is absent"
                )
            marker_identity = _descriptor_output_identity(
                marker_descriptor,
                "collectable replica bundle consumed marker",
            )
            marker_pin = PinnedOutputs(
                {CONSUMED_MARKER_NAME: marker_identity},
                {CONSUMED_MARKER_NAME: marker_descriptor},
            )
            observed_marker = marker_pin.read_metadata(
                CONSUMED_MARKER_NAME,
                "collectable replica bundle consumed marker",
                max_bytes=MAX_METADATA_BYTES,
            )
            if observed_marker != canonical_json(dict(expected_failure_marker)):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle marker claim differs"
                )

        def require_final_failure_marker() -> None:
            if successful:
                return
            held = os.fstat(file_descriptors[CONSUMED_MARKER_NAME])
            observed = _read_exact_output_marker_at(
                output_descriptor,
                expected_raw=canonical_json(dict(expected_failure_marker)),
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                allowed_links=frozenset((1,)),
            )
            if (observed.st_dev, observed.st_ino) != (
                held.st_dev,
                held.st_ino,
            ):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle marker path changed"
                )

        if successful:
            expected_members = [
                {
                    "name": name,
                    "sha256": initial_identities[name].sha256,
                    "sizeBytes": initial_identities[name].size_bytes,
                }
                for name in QUALIFIED_REPLICA_NAMES
            ]
            expected_tree = {
                "manifestSha256": _sha256(canonical_json(expected_members)),
                "members": expected_members,
                "relativePath": "outputs",
            }
            _require_strict_equal(
                expected_provenance.get("qualifiedTree"),
                expected_tree,
                "collectable replica bundle provenance tree differs",
            )

        provenance_identity: Optional[PinnedOutputIdentity] = None
        if publish_provenance:
            raw_provenance = canonical_json(dict(expected_provenance))
            _publish_exact_bytes_once_at(
                parent_descriptor,
                final_name=REPLICA_PROVENANCE_NAME,
                partial_name=REPLICA_PROVENANCE_PARTIAL_NAME,
                expected_raw=raw_provenance,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
            )
            # Reopen read-only through the held parent so validation and the
            # eventual artifact path are tied to the same inode.
            try:
                provenance_descriptor = os.open(
                    REPLICA_PROVENANCE_NAME,
                    _output_file_flags(),
                    dir_fd=parent_descriptor,
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle provenance is unreadable"
                ) from exc
            parent_names = frozenset(os.listdir(parent_descriptor))
            if parent_names != expected_success_parent:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle parent members differ"
                )
        elif REPLICA_PROVENANCE_NAME in parent_names:
            try:
                provenance_descriptor = os.open(
                    REPLICA_PROVENANCE_NAME,
                    _output_file_flags(),
                    dir_fd=parent_descriptor,
                )
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle provenance is unreadable"
                ) from exc
        elif successful or expected_provenance is not None:
            raise SuccessorProduceV4Error(
                "collectable replica bundle provenance is absent"
            )

        if provenance_descriptor is not None:
            _sealed_regular_file(
                provenance_descriptor,
                name=REPLICA_PROVENANCE_NAME,
                expected_uid=expected_uid,
                expected_gid=expected_gid,
                inode_keys=inode_keys,
            )
            provenance_identity = _descriptor_output_identity(
                provenance_descriptor,
                "collectable replica bundle provenance",
            )
            if expected_provenance is not None:
                pinned = PinnedOutputs(
                    {REPLICA_PROVENANCE_NAME: provenance_identity},
                    {REPLICA_PROVENANCE_NAME: provenance_descriptor},
                )
                raw = pinned.read_metadata(
                    REPLICA_PROVENANCE_NAME,
                    "collectable replica bundle provenance",
                    max_bytes=MAX_METADATA_BYTES,
                )
                try:
                    observed_provenance = json.loads(raw.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                    raise SuccessorProduceV4Error(
                        "collectable replica bundle provenance is not JSON"
                    ) from exc
                if raw != canonical_json(observed_provenance):
                    raise SuccessorProduceV4Error(
                        "collectable replica bundle provenance is not canonical"
                    )
                _require_strict_equal(
                    observed_provenance,
                    dict(expected_provenance),
                    "collectable replica bundle provenance differs",
                )

        # Every name, inode, owner and mode is accepted before the first mode
        # mutation.  Seal open descriptors only; never fix a path discovered
        # after validation.
        for name, descriptor in file_descriptors.items():
            os.fchmod(descriptor, COLLECTABLE_FILE_MODE)
            os.fsync(descriptor)
            after = os.fstat(descriptor)
            before = initial_stats[name]
            if (
                (after.st_dev, after.st_ino) != (before.st_dev, before.st_ino)
                or after.st_nlink != 1
                or after.st_uid != expected_uid
                or after.st_gid != expected_gid
                or stat.S_IMODE(after.st_mode) != COLLECTABLE_FILE_MODE
            ):
                raise SuccessorProduceV4Error(
                    f"collectable replica bundle seal differs: outputs/{name}"
                )
            if successful:
                completed = _descriptor_output_identity(
                    descriptor,
                    f"sealed collectable replica bundle member outputs/{name}",
                )
                initial = initial_identities[name]
                if (
                    (completed.device, completed.inode)
                    != (initial.device, initial.inode)
                    or completed.sha256 != initial.sha256
                    or completed.size_bytes != initial.size_bytes
                    or completed.mtime_ns != initial.mtime_ns
                ):
                    raise SuccessorProduceV4Error(
                        f"collectable replica bundle content changed: outputs/{name}"
                    )
        require_final_failure_marker()
        if provenance_descriptor is not None:
            os.fchmod(provenance_descriptor, COLLECTABLE_FILE_MODE)
            os.fsync(provenance_descriptor)
            after = os.fstat(provenance_descriptor)
            if (
                provenance_identity is None
                or (after.st_dev, after.st_ino)
                != (provenance_identity.device, provenance_identity.inode)
                or after.st_nlink != 1
                or after.st_uid != expected_uid
                or after.st_gid != expected_gid
                or stat.S_IMODE(after.st_mode) != COLLECTABLE_FILE_MODE
            ):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle provenance seal differs"
                )
        if frozenset(os.listdir(output_descriptor)) != output_names:
            raise SuccessorProduceV4Error(
                "collectable replica bundle output members changed during sealing"
            )
        if frozenset(os.listdir(parent_descriptor)) != parent_names:
            raise SuccessorProduceV4Error(
                "collectable replica bundle parent members changed during sealing"
            )
        os.fchmod(output_descriptor, COLLECTABLE_OUTPUT_DIRECTORY_MODE)
        os.fsync(output_descriptor)
        sealed_output = os.fstat(output_descriptor)
        if (
            (sealed_output.st_dev, sealed_output.st_ino)
            != (output_info.st_dev, output_info.st_ino)
            or sealed_output.st_uid != expected_uid
            or sealed_output.st_gid != expected_gid
            or stat.S_IMODE(sealed_output.st_mode)
            != COLLECTABLE_OUTPUT_DIRECTORY_MODE
        ):
            raise SuccessorProduceV4Error(
                "collectable replica bundle outputs seal differs"
            )

        def require_final_path_bindings(expected_parent_mode: int) -> None:
            if frozenset(os.listdir(output_descriptor)) != output_names:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle output members changed during sealing"
                )
            if frozenset(os.listdir(parent_descriptor)) != parent_names:
                raise SuccessorProduceV4Error(
                    "collectable replica bundle parent members changed during sealing"
                )
            for name, descriptor in file_descriptors.items():
                held = os.fstat(descriptor)
                try:
                    reached = os.stat(
                        name,
                        dir_fd=output_descriptor,
                        follow_symlinks=False,
                    )
                except OSError as exc:
                    raise SuccessorProduceV4Error(
                        f"collectable replica bundle path changed: outputs/{name}"
                    ) from exc
                if (
                    not stat.S_ISREG(reached.st_mode)
                    or (reached.st_dev, reached.st_ino)
                    != (held.st_dev, held.st_ino)
                    or reached.st_nlink != 1
                    or reached.st_uid != expected_uid
                    or reached.st_gid != expected_gid
                    or stat.S_IMODE(reached.st_mode) != COLLECTABLE_FILE_MODE
                ):
                    raise SuccessorProduceV4Error(
                        f"collectable replica bundle path identity changed: outputs/{name}"
                    )
            if provenance_descriptor is not None:
                held = os.fstat(provenance_descriptor)
                try:
                    reached = os.stat(
                        REPLICA_PROVENANCE_NAME,
                        dir_fd=parent_descriptor,
                        follow_symlinks=False,
                    )
                except OSError as exc:
                    raise SuccessorProduceV4Error(
                        "collectable replica bundle provenance path changed"
                    ) from exc
                if (
                    not stat.S_ISREG(reached.st_mode)
                    or (reached.st_dev, reached.st_ino)
                    != (held.st_dev, held.st_ino)
                    or reached.st_nlink != 1
                    or reached.st_uid != expected_uid
                    or reached.st_gid != expected_gid
                    or stat.S_IMODE(reached.st_mode) != COLLECTABLE_FILE_MODE
                ):
                    raise SuccessorProduceV4Error(
                        "collectable replica bundle provenance path identity changed"
                    )
            reached_output = os.stat(
                "outputs", dir_fd=parent_descriptor, follow_symlinks=False
            )
            if (
                not stat.S_ISDIR(reached_output.st_mode)
                or (reached_output.st_dev, reached_output.st_ino)
                != (sealed_output.st_dev, sealed_output.st_ino)
                or reached_output.st_uid != expected_uid
                or reached_output.st_gid != expected_gid
                or stat.S_IMODE(reached_output.st_mode)
                != COLLECTABLE_OUTPUT_DIRECTORY_MODE
            ):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle outputs path identity changed"
                )
            reached_parent = os.stat(root, follow_symlinks=False)
            if (
                not stat.S_ISDIR(reached_parent.st_mode)
                or (reached_parent.st_dev, reached_parent.st_ino)
                != expected_parent_identity
                or reached_parent.st_uid != expected_uid
                or reached_parent.st_gid != expected_gid
                or stat.S_IMODE(reached_parent.st_mode) != expected_parent_mode
            ):
                raise SuccessorProduceV4Error(
                    "collectable replica bundle parent path identity changed"
                )

        require_final_path_bindings(stat.S_IMODE(parent_info.st_mode))
        require_final_failure_marker()
        # Opening traversal of the parent is the last mutation.
        os.fchmod(parent_descriptor, COLLECTABLE_PARENT_MODE)
        sealed_parent = os.fstat(parent_descriptor)
        if (
            (sealed_parent.st_dev, sealed_parent.st_ino)
            != expected_parent_identity
            or sealed_parent.st_uid != expected_uid
            or sealed_parent.st_gid != expected_gid
            or stat.S_IMODE(sealed_parent.st_mode) != COLLECTABLE_PARENT_MODE
        ):
            raise SuccessorProduceV4Error(
                "collectable replica bundle parent seal differs"
            )
        require_final_path_bindings(COLLECTABLE_PARENT_MODE)
        require_final_failure_marker()
        os.fsync(parent_descriptor)
    finally:
        if provenance_descriptor is not None:
            os.close(provenance_descriptor)
        for descriptor in file_descriptors.values():
            os.close(descriptor)
        if output_descriptor is not None:
            os.close(output_descriptor)
        os.close(parent_descriptor)


def publish_and_seal_replica_bundle(
    *,
    parent: pathlib.Path,
    expected_parent_identity: tuple[int, int],
    expected_uid: int,
    expected_gid: int,
    result: pathlib.Path,
    **arguments: Any,
) -> dict[str, Any]:
    """Validate, publish provenance, and seal one complete qualified bundle."""

    outputs = pathlib.Path(arguments["outputs"])
    if outputs.parent != pathlib.Path(parent):
        raise SuccessorProduceV4Error(
            "replica provenance parent differs from the bundle parent"
        )
    expected_result = outputs.parent / REPLICA_PROVENANCE_NAME
    if pathlib.Path(result) != expected_result:
        raise SuccessorProduceV4Error("replica provenance result path differs")
    document = replica_provenance_document(**arguments)
    repository_root = pathlib.Path(arguments["repository_root"])
    chain = verify_generation_chain(repository_root)
    verify_dispatch_tag_object(
        chain,
        repository_root=repository_root,
        claim_ref=arguments["claim_ref"],
        ref_object_sha=arguments["ref_object_sha"],
        tag_object_sha=arguments["tag_object_sha"],
        raw_tag_object=arguments["raw_tag_object"],
        github_run_id=arguments["github_run_id"],
        github_run_attempt=arguments["github_run_attempt"],
        workflow_path=arguments["workflow_path"],
        head_sha=arguments["head_sha"],
        head_a6_sha256=arguments["head_a6_sha256"],
    )
    seal_collectable_replica_bundle(
        parent=parent,
        successful=True,
        expected_parent_identity=expected_parent_identity,
        expected_uid=expected_uid,
        expected_gid=expected_gid,
        expected_provenance=document,
        publish_provenance=True,
    )
    return document


def _replica_bundle(outputs_bundle: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    root = _real_directory(outputs_bundle, "replica bundle")
    try:
        entries = {entry.name: entry for entry in os.scandir(root)}
    except OSError as exc:
        raise SuccessorProduceV4Error("replica bundle differs") from exc
    if set(entries) != {"outputs", REPLICA_PROVENANCE_NAME}:
        raise SuccessorProduceV4Error("replica bundle differs")
    if not entries["outputs"].is_dir(follow_symlinks=False) or not entries[
        REPLICA_PROVENANCE_NAME
    ].is_file(follow_symlinks=False):
        raise SuccessorProduceV4Error("replica bundle differs")
    return root / "outputs", root / REPLICA_PROVENANCE_NAME


def compare_provenanced_replicas(
    *,
    repository_root: pathlib.Path,
    left_bundle: pathlib.Path,
    right_bundle: pathlib.Path,
    claim_ref: str,
    ref_object_sha: str,
    tag_object_sha: str,
    raw_tag_object: bytes,
    github_run_id: str,
    github_run_attempt: str,
    workflow_path: str,
    head_sha: str,
    head_a6_sha256: str,
) -> dict[str, Any]:
    """Verify two logical matrix-child envelopes, then compare their bytes."""

    left_root = _real_directory(left_bundle, "left replica bundle")
    right_root = _real_directory(right_bundle, "right replica bundle")
    if (left_root.stat().st_dev, left_root.stat().st_ino) == (
        right_root.stat().st_dev,
        right_root.stat().st_ino,
    ):
        raise SuccessorProduceV4Error("replica bundle differs")
    left_outputs, _ = _replica_bundle(left_root)
    right_outputs, _ = _replica_bundle(right_root)
    shared = {
        "repository_root": repository_root,
        "claim_ref": claim_ref,
        "ref_object_sha": ref_object_sha,
        "tag_object_sha": tag_object_sha,
        "raw_tag_object": raw_tag_object,
        "github_run_id": github_run_id,
        "github_run_attempt": github_run_attempt,
        "workflow_path": workflow_path,
        "head_sha": head_sha,
        "head_a6_sha256": head_a6_sha256,
    }
    with _pinned_outputs(
        left_root, (REPLICA_PROVENANCE_NAME,)
    ) as left_provenance_pin, _pinned_outputs(
        right_root, (REPLICA_PROVENANCE_NAME,)
    ) as right_provenance_pin:
        left_identity = left_provenance_pin[REPLICA_PROVENANCE_NAME]
        right_identity = right_provenance_pin[REPLICA_PROVENANCE_NAME]
        if (left_identity.device, left_identity.inode) == (
            right_identity.device,
            right_identity.inode,
        ):
            raise SuccessorProduceV4Error("replica provenance differs")
        left_document = _load_canonical_output(
            left_root,
            REPLICA_PROVENANCE_NAME,
            "left replica provenance",
            pinned=left_provenance_pin,
        )
        right_document = _load_canonical_output(
            right_root,
            REPLICA_PROVENANCE_NAME,
            "right replica provenance",
            pinned=right_provenance_pin,
        )
        expected_left = replica_provenance_document(
            outputs=left_outputs,
            replica_ordinal=1,
            strategy_job_index=0,
            strategy_job_total=2,
            github_job="produce",
            artifact_name=REPLICA_ARTIFACT_PREFIX + "1",
            **shared,
        )
        expected_right = replica_provenance_document(
            outputs=right_outputs,
            replica_ordinal=2,
            strategy_job_index=1,
            strategy_job_total=2,
            github_job="produce",
            artifact_name=REPLICA_ARTIFACT_PREFIX + "2",
            **shared,
        )
        _require_strict_equal(
            left_document, expected_left, "replica provenance"
        )
        _require_strict_equal(
            right_document, expected_right, "replica provenance"
        )
        comparison = compare_qualified_replicas(
            repository_root=repository_root,
            left=left_outputs,
            right=right_outputs,
        )
        _assert_pinned_outputs_unchanged(left_root, left_provenance_pin)
        _assert_pinned_outputs_unchanged(right_root, right_provenance_pin)
        _replica_bundle(left_root)
        _replica_bundle(right_root)
        _require_strict_equal(
            replica_provenance_document(
                outputs=left_outputs,
                replica_ordinal=1,
                strategy_job_index=0,
                strategy_job_total=2,
                github_job="produce",
                artifact_name=REPLICA_ARTIFACT_PREFIX + "1",
                **shared,
            ),
            expected_left,
            "replica provenance after comparison",
        )
        _require_strict_equal(
            replica_provenance_document(
                outputs=right_outputs,
                replica_ordinal=2,
                strategy_job_index=1,
                strategy_job_total=2,
                github_job="produce",
                artifact_name=REPLICA_ARTIFACT_PREFIX + "2",
                **shared,
            ),
            expected_right,
            "replica provenance after comparison",
        )
    return {
        **comparison,
        "logicalReplicaJobsVerified": True,
        "physicalRunnerIndependenceClaim": False,
        "replicaProvenanceSha256": [
            left_identity.sha256,
            right_identity.sha256,
        ],
        "status": (
            "TWO-DISTINCT-MATRIX-REPLICA-JOBS-QUALIFIED-AND-IDENTICAL"
        ),
    }


class _VerifiedRepositoryLoader(importlib.abc.Loader):
    def __init__(
        self,
        delegate: Any,
        root: pathlib.Path,
        relative: str,
        expected: Mapping[str, Any],
    ) -> None:
        self._delegate = delegate
        self._root = root
        self._relative = relative
        self._expected = expected

    def create_module(self, spec):
        creator = getattr(self._delegate, "create_module", None)
        return None if creator is None else creator(spec)

    def exec_module(self, module) -> None:
        executor = getattr(self._delegate, "exec_module", None)
        if executor is None:
            raise SuccessorProduceV4Error(
                f"repository module has no controlled loader: {self._relative}"
            )
        before, _ = _read_regular(self._root, self._relative)
        _require_identity(before, self._expected, "pre-import repository module")
        executor(module)
        after, _ = _read_regular(self._root, self._relative)
        _require_identity(after, self._expected, "post-import repository module")


class _VerifiedRepositoryFinder(importlib.abc.MetaPathFinder):
    def __init__(
        self,
        root: pathlib.Path,
        allowed: Mapping[str, tuple[str, Mapping[str, Any]]],
    ) -> None:
        self._root = root.resolve()
        self._allowed = allowed

    def find_spec(self, fullname, path=None, target=None):
        spec = importlib.machinery.PathFinder.find_spec(fullname, path, target)
        if spec is None or spec.origin in (None, "built-in", "frozen"):
            return None
        origin = pathlib.Path(spec.origin)
        try:
            resolved = origin.resolve()
            under_root = resolved.is_relative_to(self._root)
        except (AttributeError, OSError):
            resolved = origin.resolve()
            under_root = resolved == self._root or self._root in resolved.parents
        if not under_root:
            return None
        allowed = self._allowed.get(fullname)
        if allowed is None:
            raise SuccessorProduceV4Error(
                f"unbound repository import rejected before execution: {fullname}"
            )
        relative, expected = allowed
        expected_path = _safe_path(self._root, relative)
        if resolved != expected_path.resolve():
            raise SuccessorProduceV4Error(
                f"repository import resolves to another path: {fullname}"
            )
        if spec.loader is None:
            raise SuccessorProduceV4Error(
                f"repository import has no loader: {fullname}"
            )
        identity, _ = _read_regular(self._root, relative)
        _require_identity(identity, expected, "pre-exec repository module")
        spec.loader = _VerifiedRepositoryLoader(
            spec.loader, self._root, relative, expected
        )
        return spec


class RepositoryImageBackend:
    """Lazy low-level backend; repository imports happen after chain verification."""

    def __init__(self, module_loader=None) -> None:
        self._production_loader = module_loader is None
        self._module_loader = (
            importlib.import_module if module_loader is None else module_loader
        )
        self._owned_modules: set[str] = set()

    @staticmethod
    def _bound_python_rows(chain: Any) -> dict[str, Mapping[str, Any]]:
        rows: list[Any] = []
        rehearsal = getattr(chain, "rehearsal", {})
        if isinstance(rehearsal, Mapping):
            rows.extend(rehearsal.get("boundInputs", []))
        fresh = getattr(chain, "fresh_rehearsal", {})
        if isinstance(fresh, Mapping):
            rows.extend(fresh.get("boundInputs", []))
            rows.extend(fresh.get("generationFiles", []))
            rows.extend(fresh.get("reusedPinnedUpstream", []))
        reused = getattr(chain, "reused_identities", ())
        rows.extend(
            identity.record() if isinstance(identity, FileIdentity) else identity
            for identity in reused
        )
        generation = getattr(chain, "generation_identities", ())
        rows.extend(
            identity.record() if isinstance(identity, FileIdentity) else identity
            for identity in generation
        )
        found: dict[str, Mapping[str, Any]] = {}
        for row in rows:
            if not isinstance(row, Mapping):
                continue
            relative = row.get("path")
            if not isinstance(relative, str) or not relative.endswith(".py"):
                continue
            exact = {
                key: row.get(key) for key in ("path", "sha256", "sizeBytes")
            }
            if set(row).issuperset(exact) and relative in found:
                _require_strict_equal(found[relative], exact, "duplicate module binding")
            found[relative] = exact
        return found

    def _controlled_imports(
        self, request: ProductionRequest, names: Sequence[str]
    ) -> dict[str, Any]:
        if not self._production_loader:
            return {name: self._module_loader(name) for name in names}

        rows = self._bound_python_rows(request.chain)
        if not rows:
            raise SuccessorProduceV4Error("production chain binds no Python modules")
        allowed: dict[str, tuple[str, Mapping[str, Any]]] = {}
        for relative, row in rows.items():
            module_name = _module_name_from_relative(relative)
            if module_name in allowed:
                raise SuccessorProduceV4Error(
                    f"two paths bind one repository module: {module_name}"
                )
            identity, _ = _read_regular(request.repository_root, relative)
            _require_identity(identity, row, "bound repository module")
            allowed[module_name] = (relative, row)
        missing = [name for name in names if name not in allowed]
        if missing:
            raise SuccessorProduceV4Error(
                "requested repository modules are unbound: " + ", ".join(missing)
            )
        preloaded = sorted(
            name
            for name in allowed
            if (
                name in sys.modules
                and name not in self._owned_modules
                and allowed[name][0] != V4_PATHS[0]
            )
        )
        if preloaded:
            raise SuccessorProduceV4Error(
                "bound repository module was already loaded: " + ", ".join(preloaded)
            )

        resolved_root = request.repository_root.resolve()
        bootstrap_path = _safe_path(request.repository_root, V4_PATHS[0]).resolve()
        bootstrap_row = rows.get(V4_PATHS[0])
        for name, module in tuple(sys.modules.items()):
            module_file = getattr(module, "__file__", None)
            if module_file is None:
                continue
            try:
                resolved = pathlib.Path(module_file).resolve()
                under_root = resolved.is_relative_to(resolved_root)
            except AttributeError:
                under_root = resolved == resolved_root or resolved_root in resolved.parents
            except OSError as exc:
                raise SuccessorProduceV4Error(
                    f"preloaded repository module path is unreadable: {name}"
                ) from exc
            if not under_root:
                continue
            if name in self._owned_modules:
                row_info = allowed.get(name)
                if row_info is None:
                    if resolved != bootstrap_path or bootstrap_row is None:
                        raise SuccessorProduceV4Error(
                            f"backend-owned repository module is no longer bound: {name}"
                        )
                    relative, row = V4_PATHS[0], bootstrap_row
                else:
                    relative, row = row_info
                if resolved != _safe_path(request.repository_root, relative).resolve():
                    raise SuccessorProduceV4Error(
                        f"backend-owned repository module path differs: {name}"
                    )
                identity, _ = _read_regular(request.repository_root, relative)
                _require_identity(identity, row, "backend-owned repository module")
                continue
            if (
                name == __name__
                and resolved == bootstrap_path
                and bootstrap_row is not None
            ):
                identity, _ = _read_regular(request.repository_root, V4_PATHS[0])
                _require_identity(
                    identity, bootstrap_row, "verified producer bootstrap module"
                )
                self._owned_modules.add(name)
                continue
            raise SuccessorProduceV4Error(
                f"preloaded repository module is not backend-owned: {name}"
            )

        root_text = str(request.repository_root.resolve())
        before_modules = set(sys.modules)
        before_owned = set(self._owned_modules)
        old_path = list(sys.path)
        finder = _VerifiedRepositoryFinder(request.repository_root, allowed)
        try:
            sys.path.insert(0, root_text)
            sys.meta_path.insert(0, finder)
            try:
                modules = {name: self._module_loader(name) for name in names}
            finally:
                sys.meta_path[:] = [
                    entry for entry in sys.meta_path if entry is not finder
                ]
                sys.path[:] = old_path

            for relative, row in rows.items():
                identity, _ = _read_regular(request.repository_root, relative)
                _require_identity(
                    identity, row, "post-import bound repository module"
                )
            for name in set(sys.modules) - before_modules:
                module = sys.modules.get(name)
                module_file = getattr(module, "__file__", None)
                if module_file is None:
                    continue
                resolved = pathlib.Path(module_file).resolve()
                try:
                    under_root = resolved.is_relative_to(
                        request.repository_root.resolve()
                    )
                except AttributeError:
                    under_root = (
                        resolved == request.repository_root.resolve()
                        or request.repository_root.resolve() in resolved.parents
                    )
                if not under_root:
                    continue
                allowed_row = allowed.get(name)
                if allowed_row is None:
                    raise SuccessorProduceV4Error(
                        f"unbound repository module executed: {name}"
                    )
                relative, row = allowed_row
                if resolved != _safe_path(request.repository_root, relative).resolve():
                    raise SuccessorProduceV4Error(
                        f"repository module loaded outside verified path: {name}"
                    )
                identity, _ = _read_regular(request.repository_root, relative)
                _require_identity(
                    identity, row, "post-import loaded repository module"
                )
                self._owned_modules.add(name)
            self._owned_modules.update(modules)
            return modules
        except BaseException:
            sys.meta_path[:] = [entry for entry in sys.meta_path if entry is not finder]
            sys.path[:] = old_path
            for name in set(sys.modules) - before_modules:
                sys.modules.pop(name, None)
            self._owned_modules = before_owned
            raise

    def _modules(self, request: ProductionRequest) -> dict[str, Any]:
        modules = self._controlled_imports(request, LOW_LEVEL_MODULES)
        rehearsal = getattr(request.chain, "rehearsal", {})
        bound = {
            row.get("path"): row
            for row in rehearsal.get("boundInputs", [])
            if isinstance(row, Mapping)
        }
        for name, module in modules.items():
            module_file = getattr(module, "__file__", None)
            if module_file is None:  # Explicitly injected fake module in unit tests.
                continue
            expected_relative = name.replace(".", "/") + ".py"
            expected_path = _safe_path(request.repository_root, expected_relative)
            if pathlib.Path(module_file).resolve() != expected_path.resolve():
                raise SuccessorProduceV4Error(
                    f"repository module loaded outside the verified root: {name}"
                )
            row = bound.get(expected_relative)
            if row is not None:
                identity, _ = _read_regular(request.repository_root, expected_relative)
                _require_identity(identity, row, f"R1-bound module {name}")
        return modules

    @staticmethod
    def _pinned_json(
        root: pathlib.Path,
        relative: str,
        digest: str,
        size_bytes: int,
        context: str,
    ) -> tuple[dict[str, Any], bytes]:
        identity, raw = _read_regular(root, relative)
        if identity.sha256 != digest or identity.size_bytes != size_bytes:
            raise SuccessorProduceV4Error(f"{context} pinned identity differs")
        try:
            document = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SuccessorProduceV4Error(f"{context} is not JSON") from exc
        if not isinstance(document, dict) or raw != canonical_json(document):
            raise SuccessorProduceV4Error(f"{context} is not canonical JSON")
        return document, raw

    @staticmethod
    def _layer_bytes(oci: pathlib.Path, receipt: Mapping[str, Any]) -> bytes:
        value = receipt.get("layerDigest")
        if not isinstance(value, str) or not value.startswith("sha256:"):
            raise SuccessorProduceV4Error("build receipt names no sha256 layer")
        digest = value.removeprefix("sha256:")
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise SuccessorProduceV4Error("build receipt layer digest is invalid")
        relative = f"blobs/sha256/{digest}"
        identity, raw = _read_regular(oci, relative)
        if identity.sha256 != digest or not raw:
            raise SuccessorProduceV4Error("verified OCI layer identity differs")
        return raw

    @staticmethod
    def _extract_layer(layer: bytes, destination: pathlib.Path) -> int:
        if os.path.lexists(destination):
            raise SuccessorProduceV4Error("fresh extracted tree already exists")
        try:
            with tarfile.open(fileobj=io.BytesIO(layer), mode="r:") as archive:
                members = archive.getmembers()
                by_name: dict[str, tarfile.TarInfo] = {}

                def normalized_link_target(
                    member: tarfile.TarInfo, *, relative_to_parent: bool
                ) -> str:
                    target = pathlib.PurePosixPath(member.linkname)
                    if target.is_absolute():
                        raise SuccessorProduceV4Error(
                            f"verified layer {member.name!r} link escapes"
                        )
                    parts = (
                        [*pathlib.PurePosixPath(member.name).parent.parts, *target.parts]
                        if relative_to_parent
                        else list(target.parts)
                    )
                    stack: list[str] = []
                    for part in parts:
                        if part in ("", "."):
                            continue
                        if part == "..":
                            if not stack:
                                raise SuccessorProduceV4Error(
                                    f"verified layer {member.name!r} link escapes"
                                )
                            stack.pop()
                        else:
                            stack.append(part)
                    return "/".join(stack)

                for member in members:
                    pure = pathlib.PurePosixPath(member.name)
                    if (
                        not member.name
                        or pure.is_absolute()
                        or ".." in pure.parts
                        or pure.as_posix() != member.name.rstrip("/")
                    ):
                        raise SuccessorProduceV4Error(
                            f"verified layer contains unsafe member: {member.name!r}"
                        )
                    normalized = member.name.rstrip("/")
                    if normalized in by_name:
                        raise SuccessorProduceV4Error(
                            f"verified layer repeats member: {normalized}"
                        )
                    if not (
                        member.isdir()
                        or member.isreg()
                        or member.issym()
                        or member.islnk()
                    ):
                        raise SuccessorProduceV4Error(
                            f"verified layer contains special member: {member.name!r}"
                        )
                    by_name[normalized] = member

                symlink_targets: dict[str, str] = {}
                for normalized, member in by_name.items():
                    parents = pathlib.PurePosixPath(normalized).parents
                    for parent in parents:
                        if parent == pathlib.PurePosixPath("."):
                            continue
                        ancestor = by_name.get(parent.as_posix())
                        if ancestor is not None and ancestor.issym():
                            raise SuccessorProduceV4Error(
                                "verified layer member traverses a symlink parent: "
                                f"{member.name!r}"
                            )
                    if member.issym():
                        symlink_targets[normalized] = normalized_link_target(
                            member, relative_to_parent=True
                        )
                    elif member.islnk():
                        target = normalized_link_target(
                            member, relative_to_parent=False
                        )
                        target_member = by_name.get(target)
                        if target_member is None or not target_member.isreg():
                            raise SuccessorProduceV4Error(
                                "verified layer hardlink does not name one regular member: "
                                f"{member.name!r}"
                            )
                        for parent in pathlib.PurePosixPath(target).parents:
                            if parent == pathlib.PurePosixPath("."):
                                continue
                            ancestor = by_name.get(parent.as_posix())
                            if ancestor is not None and ancestor.issym():
                                raise SuccessorProduceV4Error(
                                    "verified layer hardlink traverses a symlink: "
                                    f"{member.name!r}"
                                )

                def resolve_symlink_path(path: str, seen: tuple[str, ...]) -> str:
                    parts = pathlib.PurePosixPath(path).parts
                    for index in range(1, len(parts) + 1):
                        prefix = "/".join(parts[:index])
                        target = symlink_targets.get(prefix)
                        if target is None:
                            continue
                        if prefix in seen:
                            chain = " -> ".join((*seen, prefix))
                            raise SuccessorProduceV4Error(
                                f"verified layer contains symlink cycle: {chain}"
                            )
                        suffix = "/".join(parts[index:])
                        redirected = target if not suffix else f"{target}/{suffix}"
                        return resolve_symlink_path(redirected, (*seen, prefix))
                    return path

                for symlink, target in symlink_targets.items():
                    resolve_symlink_path(target, (symlink,))

                destination.mkdir(mode=0o755)
                extra: dict[str, Any] = {"numeric_owner": True}
                if hasattr(tarfile, "data_filter"):
                    extra["filter"] = "data"
                archive.extractall(destination, **extra)
                return len(members)
        except SuccessorProduceV4Error:
            raise
        except (tarfile.TarError, OSError) as exc:
            raise SuccessorProduceV4Error("verified OCI layer cannot be extracted") from exc

    def prepare(self, request: ProductionRequest) -> PreparedProduction:
        modules = self._modules(request)
        gate = modules[MODULE_GATE]
        base = modules[MODULE_BASE]
        builder = modules[MODULE_BUILDER_V4]
        v3 = modules[MODULE_V3]
        writer = modules[MODULE_WRITER]

        source_lock, source_raw = self._pinned_json(
            request.repository_root,
            SOURCE_LOCK_PATH,
            SOURCE_LOCK_SHA256,
            SOURCE_LOCK_SIZE_BYTES,
            "source-lock-v2",
        )
        preregistration, _ = self._pinned_json(
            request.repository_root,
            P1_PATH,
            P1_SHA256,
            P1_SIZE_BYTES,
            "P1 shared-staging preregistration",
        )
        builder_authority, _ = self._pinned_json(
            request.repository_root,
            BUILDER_AUTHORITY_PATH,
            BUILDER_AUTHORITY_SHA256,
            BUILDER_AUTHORITY_SIZE_BYTES,
            "root-disk builder authority",
        )
        try:
            sealed = builder.load_json_exact(
                source_raw, "source-lock v2", require_canonical=True
            )
            if sealed != source_lock:
                raise SuccessorProduceV4Error("builder parsed different source-lock bytes")
            runtime, runtime_receipt = gate.materialize_runtime_lock(
                sealed, source_raw, request.gpgv, request.zstd
            )
            normalized, normalized_raw, normalization = base.normalized_runtime_lock(
                runtime
            )
            validated = builder.validate_source_lock(
                normalized,
                normalized_raw,
                request.repository_root,
                request.artifact_store,
                require_complete=True,
            )
            nested = builder.nested_runtime_tree(
                request.repository_root,
                request.artifact_store,
                request.gpgv,
                request.zstd,
            )
            prepare_staging = getattr(v3, "prepare_staging")
            function_globals = getattr(prepare_staging, "__globals__", {})
            if (
                "builder_v4" in function_globals
                and function_globals.get("builder_v4") is not builder
            ):
                raise SuccessorProduceV4Error(
                    "v3.prepare_staging and image layout do not share builder-v4"
                )
            prepared = prepare_staging(
                validated=validated,
                repository_root=request.repository_root,
                artifact_store=request.artifact_store,
                launcher_binary=request.launcher_binary,
                nested_tree=nested,
                preregistration=preregistration,
            )
        except SuccessorProduceV4Error:
            raise
        except Exception as exc:
            raise SuccessorProduceV4Error(
                f"shared v3 staging preparation failed: {exc}"
            ) from exc
        if not hasattr(prepared, "entries") or not hasattr(prepared, "measurement"):
            raise SuccessorProduceV4Error("v3.prepare_staging returned no sealed staging")

        oci = request.scratch / "oci-v4"
        tree = request.scratch / "tree-v4"
        writer_tree = request.scratch / "writer-v4"
        for candidate in (oci, tree, writer_tree):
            if os.path.lexists(candidate):
                raise SuccessorProduceV4Error(
                    f"production scratch member already exists: {candidate.name}"
                )
        try:
            build_receipt = builder.build_oci_layout(
                normalized,
                normalized_raw,
                request.repository_root,
                request.artifact_store,
                oci,
                launcher_binary=request.launcher_binary,
                nested_tree=nested,
            )
            layer = self._layer_bytes(oci, build_receipt)
            layer_entry_count = self._extract_layer(layer, tree)
            writer_receipt = writer.materialize(
                cas_roots=[request.artifact_store],
                zstd=request.zstd,
                writer_tree=writer_tree,
            )
        except SuccessorProduceV4Error:
            raise
        except Exception as exc:
            raise SuccessorProduceV4Error(
                f"low-level image preparation failed: {exc}"
            ) from exc
        return PreparedProduction(
            measurement=dict(prepared.measurement),
            build_receipt={
                "oci": dict(build_receipt),
                "normalization": normalization,
                "runtime": runtime_receipt,
                "writer": writer_receipt,
            },
            state={
                "builderAuthority": builder_authority,
                "layer": layer,
                "layerEntryCount": layer_entry_count,
                "modules": modules,
                "normalizedLock": normalized,
                "tree": tree,
                "writerTree": writer_tree,
            },
        )

    @staticmethod
    def _state(prepared: PreparedProduction) -> Mapping[str, Any]:
        if not isinstance(prepared.state, Mapping):
            raise SuccessorProduceV4Error("prepared repository state differs")
        return prepared.state

    def extract_kernel(
        self, request: ProductionRequest, prepared: PreparedProduction
    ) -> Mapping[str, Any]:
        state = self._state(prepared)
        kernel_module = state["modules"][MODULE_KERNEL]
        try:
            result, disposition = kernel_module.extract(
                cas_roots=[request.artifact_store],
                zstd_path=request.zstd,
                out_dir=request.outputs,
                result_path=request.scratch / "kernel-extract-result-v4.json",
            )
        except Exception as exc:
            raise SuccessorProduceV4Error(f"kernel extraction failed: {exc}") from exc
        kernel = result.get("kernel") if isinstance(result, Mapping) else None
        if not isinstance(kernel, Mapping):
            raise SuccessorProduceV4Error("kernel extractor returned no identity")
        if disposition != "matched-the-seal":
            raise SuccessorProduceV4Error("kernel extraction disposition differs")
        if result.get("activationAllowed") is not False or result.get(
            "bootableClaim"
        ) is not False:
            raise SuccessorProduceV4Error(
                "kernel extractor returned an activation or boot claim"
            )
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "kernel": dict(kernel),
        }

    def build_initrd(
        self, request: ProductionRequest, prepared: PreparedProduction
    ) -> bytes:
        state = self._state(prepared)
        initrd_module = state["modules"][MODULE_INITRD]
        try:
            raw = initrd_module.initrd_bytes(state["layer"])
        except Exception as exc:
            raise SuccessorProduceV4Error(f"initrd construction failed: {exc}") from exc
        if not isinstance(raw, bytes) or not raw:
            raise SuccessorProduceV4Error("initrd construction returned no bytes")
        return raw

    @staticmethod
    def _inspector_path(
        authority: Mapping[str, Any], tree: pathlib.Path
    ) -> pathlib.Path:
        rows = authority.get("toolBinaries")
        if not isinstance(rows, list):
            raise SuccessorProduceV4Error("builder authority pins no tools")
        matched = [
            row
            for row in rows
            if isinstance(row, Mapping) and row.get("role") == "ext4-image-inspector"
        ]
        if len(matched) != 1:
            raise SuccessorProduceV4Error(
                "builder authority must pin one ext4 image inspector"
            )
        member = matched[0].get("memberPath")
        if not isinstance(member, str) or not member.startswith("./"):
            raise SuccessorProduceV4Error("builder authority inspector path differs")
        return tree / member.removeprefix("./")

    def build_root_disk(
        self, request: ProductionRequest, prepared: PreparedProduction
    ) -> Mapping[str, Any]:
        state = self._state(prepared)
        modules = state["modules"]
        root_disk = modules[MODULE_ROOT_DISK]
        executor = modules[MODULE_ROOT_EXECUTE]
        writer = modules[MODULE_WRITER]
        tree = pathlib.Path(state["tree"])
        writer_tree = pathlib.Path(state["writerTree"])
        layer = state["layer"]
        try:
            entries = root_disk.layer_entries(layer)
            size_bytes = root_disk.required_bytes(entries)
            plan = root_disk.root_disk_plan(
                layer=layer,
                mke2fs=str(writer_tree / writer.WRITER_TREE_PATH),
                debugfs=str(self._inspector_path(state["builderAuthority"], tree)),
                e2fsck=str(tree / root_disk.E2FSCK_MEMBER_PATH.removeprefix("./")),
                config=str(tree / "etc/mke2fs.conf"),
                image=str(request.outputs / "guest-root-disk"),
                staging=str(request.scratch / "staging-v4"),
                sizeBytes=size_bytes,
            )
            result = executor.execute(plan, layer, tree, writer_tree)
        except SuccessorProduceV4Error:
            raise
        except Exception as exc:
            raise SuccessorProduceV4Error(f"root-disk construction failed: {exc}") from exc
        if not isinstance(result, Mapping) or not isinstance(result.get("image"), Mapping):
            raise SuccessorProduceV4Error("root-disk executor returned no image identity")
        required_evidence = (
            "fsck",
            "loaderEvidence",
            "timeAudit",
            "toolDigests",
            "writerTime",
        )
        missing = [name for name in required_evidence if name not in result]
        if missing:
            raise SuccessorProduceV4Error(
                "root-disk executor omitted evidence: " + ", ".join(missing)
            )
        if result.get("activationAllowed") is not False or result.get(
            "bootableClaim"
        ) is not False:
            raise SuccessorProduceV4Error(
                "root-disk executor returned an activation or boot claim"
            )
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "image": dict(result["image"]),
        }

    def verify_images(
        self,
        request: ProductionRequest,
        prepared: PreparedProduction,
        kernel: Mapping[str, Any],
        initrd: bytes,
        root_disk: Mapping[str, Any],
    ) -> Mapping[str, Any]:
        del kernel, root_disk
        state = self._state(prepared)
        verifier = state["modules"][MODULE_IMAGE_VERIFY]
        _, kernel_raw = _read_regular(request.outputs, "guest-kernel")
        try:
            report = verifier.verify_tree(
                tree=verifier.tree_from_initrd(initrd),
                expectations=verifier.expectations_from_lock(state["normalizedLock"]),
                launcherSha256=_sha256(request.launcher_binary),
                kernel=kernel_raw,
            )
            verifier.assert_passed(report)
        except Exception as exc:
            raise SuccessorProduceV4Error(f"produced image verification failed: {exc}") from exc
        if not isinstance(report, Mapping) or report.get("passed") is not True:
            raise SuccessorProduceV4Error("produced image verification did not pass")
        return dict(report)

    def readback(
        self,
        repository_root: pathlib.Path,
        outputs: pathlib.Path,
        chain: GenerationChain,
    ) -> Mapping[str, Any]:
        # The readback module owns open-descriptor pinning, a read-only mount,
        # cleanup hard stops, post-read identity checks and create-once
        # promotion.  Calling it here, once, is the qualification edge.
        import_request = RepositoryImportRequest(
            repository_root=pathlib.Path(repository_root).resolve(),
            chain=chain,
        )
        readback = self._controlled_imports(
            import_request, (MODULE_READBACK_V3,)
        )[MODULE_READBACK_V3]
        module_file = getattr(readback, "__file__", None)
        if module_file is not None:
            expected = _safe_path(repository_root, REUSED_PINNED_PATHS[1])
            if pathlib.Path(module_file).resolve() != expected.resolve():
                raise SuccessorProduceV4Error("readback-v3 loaded outside verified root")
            identity, _ = _read_regular(repository_root, REUSED_PINNED_PATHS[1])
            bound = {
                row.get("path"): row
                for row in chain.fresh_rehearsal.get("reusedPinnedUpstream", [])
                if isinstance(row, Mapping)
            }
            _require_identity(
                identity,
                bound[REUSED_PINNED_PATHS[1]],
                "R2-bound readback-v3",
            )
        try:
            result = readback.verify(
                outputs=outputs,
                effects=AutoclearReadbackEffects(readback),
            )
        except Exception as exc:
            raise SuccessorProduceV4Error(f"readback-v3 failed: {exc}") from exc
        if not isinstance(result, Mapping):
            raise SuccessorProduceV4Error("readback-v3 returned no result")
        return dict(result)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    subparsers = parser.add_subparsers(dest="mode", required=True)
    for mode in ("verify-bindings", "production-check"):
        child = subparsers.add_parser(mode)
        child.add_argument("--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT)
    for mode in (
        "publish-recovery-record",
        "verify-recovery-record",
        "publish-cleanup-checkpoint",
        "verify-recovery-after-unmount",
        "reconcile-recovery-record-publication",
        "discard-incomplete-recovery",
        "remove-verified-recovery",
    ):
        child = subparsers.add_parser(mode)
        child.add_argument("--scratch", type=pathlib.Path, required=True)
        child.add_argument("--outputs-parent", type=pathlib.Path, required=True)
        child.add_argument("--parent-device", type=int, required=True)
        child.add_argument("--parent-inode", type=int, required=True)
        child.add_argument("--recovery-stem", required=True)
        if mode == "publish-recovery-record":
            child.add_argument("--scratch-device", type=int, required=True)
            child.add_argument("--scratch-inode", type=int, required=True)
            child.add_argument("--staging-device", type=int, required=True)
            child.add_argument("--staging-inode", type=int, required=True)
    for mode in ("dispatch-claim-message", "dispatch-claim-verify"):
        child = subparsers.add_parser(mode)
        child.add_argument(
            "--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT
        )
        child.add_argument("--github-run-id", required=True)
        child.add_argument("--github-run-attempt", required=True)
        child.add_argument("--workflow-path", required=True)
        child.add_argument("--head-sha", required=True)
        child.add_argument("--head-a6-sha256", required=True)
        if mode == "dispatch-claim-verify":
            child.add_argument("--claim-ref", required=True)
            child.add_argument("--ref-object-sha", required=True)
            child.add_argument("--tag-object-sha", required=True)
    for mode in ("rehearsal", "preflight", "produce"):
        child = subparsers.add_parser(mode)
        child.add_argument("--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT)
        child.add_argument("--cas", type=pathlib.Path, required=True)
        child.add_argument("--launcher", type=pathlib.Path, required=True)
        child.add_argument("--scratch", type=pathlib.Path, required=True)
        child.add_argument("--gpgv", type=pathlib.Path, required=True)
        child.add_argument("--zstd", type=pathlib.Path, required=True)
        child.add_argument("--result", type=pathlib.Path, required=True)
        if mode == "rehearsal":
            child.add_argument("--expected-systemd-unit", required=True)
        if mode == "produce":
            child.add_argument("--outputs", type=pathlib.Path, required=True)
            child.add_argument("--claim-ref", required=True)
            child.add_argument("--ref-object-sha", required=True)
            child.add_argument("--tag-object-sha", required=True)
            child.add_argument("--github-run-id", required=True)
            child.add_argument("--github-run-attempt", required=True)
            child.add_argument("--workflow-path", required=True)
            child.add_argument("--head-sha", required=True)
            child.add_argument("--head-a6-sha256", required=True)
    qualify_parser = subparsers.add_parser("qualify")
    qualify_parser.add_argument(
        "--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT
    )
    qualify_parser.add_argument("--outputs", type=pathlib.Path, required=True)
    qualify_parser.add_argument("--pending", type=pathlib.Path, required=True)
    qualify_parser.add_argument("--result", type=pathlib.Path, required=True)
    seal_parser = subparsers.add_parser("seal-replica-bundle")
    seal_parser.add_argument(
        "--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT
    )
    seal_parser.add_argument("--parent", type=pathlib.Path, required=True)
    seal_parser.add_argument("--parent-device", type=int, required=True)
    seal_parser.add_argument("--parent-inode", type=int, required=True)
    seal_parser.add_argument(
        "--successful", choices=("no",), required=True
    )
    output_recovery_parser = subparsers.add_parser("reconcile-output-state")
    output_recovery_parser.add_argument(
        "--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT
    )
    output_recovery_parser.add_argument(
        "--parent", type=pathlib.Path, required=True
    )
    output_recovery_parser.add_argument(
        "--parent-device", type=int, required=True
    )
    output_recovery_parser.add_argument(
        "--parent-inode", type=int, required=True
    )
    for mode in (
        "publish-and-seal-replica",
        "compare-provenanced-replicas",
    ):
        child = subparsers.add_parser(mode)
        child.add_argument(
            "--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT
        )
        child.add_argument("--claim-ref", required=True)
        child.add_argument("--ref-object-sha", required=True)
        child.add_argument("--tag-object-sha", required=True)
        child.add_argument("--github-run-id", required=True)
        child.add_argument("--github-run-attempt", required=True)
        child.add_argument("--workflow-path", required=True)
        child.add_argument("--head-sha", required=True)
        child.add_argument("--head-a6-sha256", required=True)
        if mode == "publish-and-seal-replica":
            child.add_argument("--outputs", type=pathlib.Path, required=True)
            child.add_argument("--result", type=pathlib.Path, required=True)
            child.add_argument("--replica-ordinal", type=int, required=True)
            child.add_argument("--strategy-job-index", type=int, required=True)
            child.add_argument("--strategy-job-total", type=int, required=True)
            child.add_argument("--github-job", required=True)
            child.add_argument("--artifact-name", required=True)
            child.add_argument("--parent", type=pathlib.Path, required=True)
            child.add_argument("--parent-device", type=int, required=True)
            child.add_argument("--parent-inode", type=int, required=True)
        else:
            child.add_argument("--left-bundle", type=pathlib.Path, required=True)
            child.add_argument("--right-bundle", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    try:
        if options.mode == "verify-bindings":
            verify_preregistered_generation(options.repository_root)
            print("native-shadow successor producer v4: bindings verified")
            return 0
        if options.mode == "production-check":
            chain = verify_generation_chain(options.repository_root)
            print(
                "native-shadow successor producer v4: production authority verified "
                f"({chain.attempt_id})"
            )
            return 0
        if options.mode in (
            "publish-recovery-record",
            "verify-recovery-record",
            "publish-cleanup-checkpoint",
            "verify-recovery-after-unmount",
            "reconcile-recovery-record-publication",
            "discard-incomplete-recovery",
            "remove-verified-recovery",
        ):
            shared = {
                "scratch": options.scratch,
                "outputs_parent": options.outputs_parent,
                "expected_parent_identity": (
                    options.parent_device,
                    options.parent_inode,
                ),
                "expected_uid": os.geteuid(),
                "expected_gid": os.getegid(),
                "recovery_stem": options.recovery_stem,
            }
            mount_identity = None
            if options.mode not in (
                "verify-recovery-after-unmount",
                "discard-incomplete-recovery",
                "remove-verified-recovery",
            ):
                raw_mount_identity = sys.stdin.buffer.read(
                    MAX_RECOVERY_RECORD_BYTES + 1
                )
                if len(raw_mount_identity) > MAX_RECOVERY_RECORD_BYTES:
                    raise SuccessorProduceV4Error(
                        "production recovery mount input exceeds byte limit"
                    )
                try:
                    mount_identity = json.loads(raw_mount_identity.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                    raise SuccessorProduceV4Error(
                        "production recovery mount input is not JSON"
                    ) from exc
            if options.mode == "publish-recovery-record":
                document = publish_production_recovery_record(
                    expected_scratch_identity=(
                        options.scratch_device,
                        options.scratch_inode,
                    ),
                    expected_staging_identity=(
                        options.staging_device,
                        options.staging_inode,
                    ),
                    mount_identity=mount_identity,
                    **shared,
                )
                label = "recovery record published"
            elif options.mode == "verify-recovery-record":
                document = verify_production_recovery_record(
                    mount_identity=mount_identity, **shared
                )
                label = "recovery record verified"
            elif options.mode == "publish-cleanup-checkpoint":
                document = publish_production_cleanup_checkpoint(
                    mount_identity=mount_identity, **shared
                )
                label = "cleanup checkpoint published"
            elif options.mode == "verify-recovery-after-unmount":
                document = verify_production_recovery_after_unmount(**shared)
                label = "post-unmount recovery verified"
            elif options.mode == "reconcile-recovery-record-publication":
                state = reconcile_production_recovery_record_publication(
                    mount_identity=mount_identity,
                    **shared,
                )
                sys.stdout.write(state)
                return 0
            elif options.mode == "discard-incomplete-recovery":
                state = discard_incomplete_production_recovery(**shared)
                sys.stdout.write(state)
                return 0
            else:
                state = remove_verified_production_recovery(**shared)
                sys.stdout.write(state)
                return 0
            print(
                f"native-shadow successor producer v4: {label} "
                f"sha256={_sha256(canonical_json(document))}"
            )
            return 0
        if options.mode == "dispatch-claim-message":
            chain = verify_generation_chain(options.repository_root)
            raw_message = dispatch_claim_message(
                chain,
                github_run_id=options.github_run_id,
                github_run_attempt=options.github_run_attempt,
                workflow_path=options.workflow_path,
                head_sha=options.head_sha,
                head_a6_sha256=options.head_a6_sha256,
            )
            sys.stdout.buffer.write(raw_message)
            return 0
        if options.mode == "dispatch-claim-verify":
            chain = verify_generation_chain(options.repository_root)
            raw_tag_object = sys.stdin.buffer.read(16_385)
            if len(raw_tag_object) > 16_384:
                raise SuccessorProduceV4Error("dispatch claim annotated tag differs")
            verify_dispatch_tag_object(
                chain,
                repository_root=options.repository_root,
                claim_ref=options.claim_ref,
                ref_object_sha=options.ref_object_sha,
                tag_object_sha=options.tag_object_sha,
                raw_tag_object=raw_tag_object,
                github_run_id=options.github_run_id,
                github_run_attempt=options.github_run_attempt,
                workflow_path=options.workflow_path,
                head_sha=options.head_sha,
                head_a6_sha256=options.head_a6_sha256,
            )
            print(
                "native-shadow successor producer v4: dispatch claim verified "
                f"({chain.attempt_id})"
            )
            return 0
        if options.mode == "qualify":
            document = qualify(
                repository_root=options.repository_root,
                outputs=options.outputs,
                pending=options.pending,
                result=options.result,
            )
            print(
                "native-shadow successor producer v4: qualify PASS "
                f"sha256={_sha256(canonical_json(document))}"
            )
            return 0
        if options.mode == "seal-replica-bundle":
            chain = verify_generation_chain(options.repository_root)
            seal_collectable_replica_bundle(
                parent=options.parent,
                successful=False,
                expected_parent_identity=(
                    options.parent_device,
                    options.parent_inode,
                ),
                expected_uid=0,
                expected_gid=0,
                expected_failure_marker=_marker_document(chain),
            )
            print("native-shadow successor producer v4: replica bundle sealed")
            return 0
        if options.mode == "reconcile-output-state":
            state = reconcile_production_output_state(
                repository_root=options.repository_root,
                parent=options.parent,
                expected_parent_identity=(
                    options.parent_device,
                    options.parent_inode,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            )
            sys.stdout.write(state)
            return 0
        if options.mode in (
            "publish-and-seal-replica",
            "compare-provenanced-replicas",
        ):
            raw_tag_object = sys.stdin.buffer.read(16_385)
            if len(raw_tag_object) > 16_384:
                raise SuccessorProduceV4Error(
                    "dispatch claim annotated tag differs"
                )
            dispatch = {
                "repository_root": options.repository_root,
                "claim_ref": options.claim_ref,
                "ref_object_sha": options.ref_object_sha,
                "tag_object_sha": options.tag_object_sha,
                "raw_tag_object": raw_tag_object,
                "github_run_id": options.github_run_id,
                "github_run_attempt": options.github_run_attempt,
                "workflow_path": options.workflow_path,
                "head_sha": options.head_sha,
                "head_a6_sha256": options.head_a6_sha256,
            }
            if options.mode == "publish-and-seal-replica":
                publish_arguments = {
                    "outputs": options.outputs,
                    "replica_ordinal": options.replica_ordinal,
                    "strategy_job_index": options.strategy_job_index,
                    "strategy_job_total": options.strategy_job_total,
                    "github_job": options.github_job,
                    "artifact_name": options.artifact_name,
                    **dispatch,
                }
                document = publish_and_seal_replica_bundle(
                    parent=options.parent,
                    expected_parent_identity=(
                        options.parent_device,
                        options.parent_inode,
                    ),
                    expected_uid=0,
                    expected_gid=0,
                    result=options.result,
                    **publish_arguments,
                )
                label = "replica provenance and bundle seal"
            else:
                document = compare_provenanced_replicas(
                    left_bundle=options.left_bundle,
                    right_bundle=options.right_bundle,
                    **dispatch,
                )
                label = "provenanced replica comparison"
            print(
                f"native-shadow successor producer v4: {label} PASS "
                f"sha256={_sha256(canonical_json(document))}"
            )
            return 0
        common = {
            "repository_root": options.repository_root,
            "artifact_store": options.cas,
            "scratch": options.scratch,
            "gpgv": options.gpgv,
            "zstd": options.zstd,
            "launcher": options.launcher,
        }
        production_dispatch = None
        if options.mode == "produce":
            expected_pending = pathlib.Path(options.outputs) / PENDING_RESULT_NAME
            if pathlib.Path(options.result) != expected_pending:
                raise SuccessorProduceV4Error(
                    "produce result must be the fixed pending-readback path"
                )
            raw_tag_object = sys.stdin.buffer.read(16_385)
            if len(raw_tag_object) > 16_384:
                raise SuccessorProduceV4Error(
                    "production dispatch capability annotated tag differs"
                )
            production_dispatch = {
                "claim_ref": options.claim_ref,
                "ref_object_sha": options.ref_object_sha,
                "tag_object_sha": options.tag_object_sha,
                "raw_tag_object": raw_tag_object,
                "github_run_id": options.github_run_id,
                "github_run_attempt": options.github_run_attempt,
                "workflow_path": options.workflow_path,
                "head_sha": options.head_sha,
                "head_a6_sha256": options.head_a6_sha256,
            }
        if options.mode == "rehearsal":
            document = rehearse(
                expected_systemd_unit=options.expected_systemd_unit,
                **common,
            )
        elif options.mode == "preflight":
            document = preflight(**common)
        else:
            document = produce(
                outputs=options.outputs,
                dispatch_capability=production_dispatch,
                **common,
            )
        if options.mode != "produce":
            _publish_json_once(options.result, document)
        print(
            f"native-shadow successor producer v4: {options.mode} PASS "
            f"sha256={_sha256(canonical_json(document))}"
        )
        return 0
    except SuccessorProduceV4Error as exc:
        print(f"native-shadow successor producer v4: FAIL: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
