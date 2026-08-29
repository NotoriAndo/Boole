#!/usr/bin/env python3
"""Launcher-v2 projection of the successor boot-rootfs assembler.

The historical v3 projection assembled the image that is already preserved.  It
must continue to accept only the historical launcher.  This module therefore
does not edit or monkeypatch v3: it verifies v3's complete file digest, compiles
the same derived source into a fresh globals mapping, and replaces exactly the
launcher seal in that new mapping.

The result is still only an assembler.  It has no image-production entry point,
does not write a consumed-attempt marker and grants no boot or activation.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_builder_arm64_v1 as arm64
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_v1
from scripts import native_shadow_rootfs_builder_boot_arm64_v2 as boot_v2
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as boot_v3


BOOTABLE_CLAIM = False
IMAGE_PRODUCED_CLAIM = False
ACTIVATION_ALLOWED = False

BOOT_V3 = pathlib.Path(boot_v3.__file__).resolve()
BOOT_V3_SHA256 = "93bd05d06e43cc69f325036d204b7b57721e358dd5c5d5990227ef88c4de8c39"

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
LAUNCHER_V2_RESULT_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
)
LAUNCHER_V2_RESULT_SHA256 = (
    "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08"
)

BOOT_AUTHORITY_FILES = dict(boot_v3.BOOT_AUTHORITY_FILES)
NESTED_RUNTIME_TREE = dict(boot_v3.NESTED_RUNTIME_TREE)
BOOT_SOURCE_LOCK_RELEASE = boot_v3.BOOT_SOURCE_LOCK_RELEASE


class BootLauncherV2ProjectionError(RuntimeError):
    """The predecessor or launcher-v2 seal is not the pre-registered input."""


def _canonical_json(document: Any) -> bytes:
    return (json.dumps(document, sort_keys=True, indent=2) + "\n").encode("utf-8")


def _launcher_v2_seal() -> dict[str, Any]:
    try:
        raw = LAUNCHER_V2_RESULT_PATH.read_bytes()
    except OSError as exc:
        raise BootLauncherV2ProjectionError(
            "launcher-v2 build result is unreadable"
        ) from exc
    digest = hashlib.sha256(raw).hexdigest()
    if digest != LAUNCHER_V2_RESULT_SHA256:
        raise BootLauncherV2ProjectionError(
            f"launcher-v2 build result hashes to {digest}, the projection pins "
            f"{LAUNCHER_V2_RESULT_SHA256}"
        )
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise BootLauncherV2ProjectionError(
            "launcher-v2 build result is not canonical JSON"
        ) from exc
    if raw != _canonical_json(document):
        raise BootLauncherV2ProjectionError(
            "launcher-v2 build result is not canonical JSON"
        )
    launcher = document.get("launcher")
    if not isinstance(launcher, dict):
        raise BootLauncherV2ProjectionError(
            "launcher-v2 build result has no launcher identity"
        )
    expected_keys = {"guestLogicalPath", "sha256", "sizeBytes"}
    if set(launcher) != expected_keys:
        raise BootLauncherV2ProjectionError(
            "launcher-v2 identity fields differ"
        )
    path = launcher.get("guestLogicalPath")
    digest = launcher.get("sha256")
    size = launcher.get("sizeBytes")
    if not isinstance(path, str) or not path.startswith("/"):
        raise BootLauncherV2ProjectionError("launcher-v2 guest path is unusable")
    if (
        not isinstance(digest, str)
        or len(digest) != 64
        or any(character not in "0123456789abcdef" for character in digest)
    ):
        raise BootLauncherV2ProjectionError("launcher-v2 digest is unusable")
    if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
        raise BootLauncherV2ProjectionError("launcher-v2 size is unusable")
    if path != boot_v1.LAUNCHER_GUEST_PATH:
        raise BootLauncherV2ProjectionError(
            "launcher-v2 changed the historical guest path"
        )
    return dict(launcher)


_LAUNCHER_V2_SEAL = _launcher_v2_seal()
LAUNCHER_SHA256 = _LAUNCHER_V2_SEAL["sha256"]
LAUNCHER_SIZE_BYTES = _LAUNCHER_V2_SEAL["sizeBytes"]
LAUNCHER_GUEST_PATH = _LAUNCHER_V2_SEAL["guestLogicalPath"]
LAUNCHER_MODE = boot_v1.LAUNCHER_MODE
ABORT_LAUNCHER_DIGEST_MISMATCH = boot_v1.ABORT_LAUNCHER_DIGEST_MISMATCH


def _derived_source() -> str:
    raw = BOOT_V3.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != BOOT_V3_SHA256:
        raise BootLauncherV2ProjectionError(
            f"boot v3 projection hashes to {digest}, this generation pins "
            f"{BOOT_V3_SHA256}"
        )
    return boot_v3._derived_source()


_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_builder_boot_arm64_v4_impl",
    "__file__": str(pathlib.Path(arm64.__file__).resolve()),
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

_IMPL["EXPECTED_AUTHORITY_FILES"] = dict(
    _IMPL["EXPECTED_AUTHORITY_FILES"], **BOOT_AUTHORITY_FILES
)
_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"] = dict(
    _IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"],
    **boot_v1.BOOT_PROVENANCE_CLOSURE_ROOTS,
)
_IMPL["REQUIRED_PROVENANCE_CLOSURES"] = tuple(
    sorted(_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"])
)
for _name in boot_v2.INHERITED_INJECTIONS:
    if _name == "launcher_entry":
        continue
    _IMPL[_name] = getattr(boot_v1, _name)

_IMPL["LAUNCHER_SHA256"] = LAUNCHER_SHA256
_IMPL["LAUNCHER_SIZE_BYTES"] = LAUNCHER_SIZE_BYTES
_IMPL["LAUNCHER_GUEST_PATH"] = LAUNCHER_GUEST_PATH
_IMPL["LAUNCHER_MODE"] = LAUNCHER_MODE
_IMPL["ABORT_LAUNCHER_DIGEST_MISMATCH"] = ABORT_LAUNCHER_DIGEST_MISMATCH
_IMPL["sha256_hex"] = boot_v1.sha256_hex

# Compile the gate in the new namespace as well.  Assigning a module-level
# function into this mapping would leave its ``__globals__`` pointing at this
# wrapper module and would silently recreate the split-assembler bug.
exec(
    compile(
        """
def launcher_entry(binary):
    if not binary:
        raise RootfsBuildError(
            f\"{ABORT_LAUNCHER_DIGEST_MISMATCH}: the rebuilt launcher is empty\"
        )
    if len(binary) != LAUNCHER_SIZE_BYTES:
        raise RootfsBuildError(
            f\"{ABORT_LAUNCHER_DIGEST_MISMATCH}: the rebuilt launcher is \"
            f\"{len(binary)} bytes, the seal says {LAUNCHER_SIZE_BYTES}\"
        )
    actual = sha256_hex(binary)
    if actual != LAUNCHER_SHA256:
        raise RootfsBuildError(
            f\"{ABORT_LAUNCHER_DIGEST_MISMATCH}: the rebuilt launcher hashes to \"
            f\"{actual}, the seal says {LAUNCHER_SHA256}\"
        )
    return {
        \"path\": LAUNCHER_GUEST_PATH.lstrip(\"/\"),
        \"kind\": \"file\",
        \"mode\": LAUNCHER_MODE,
        \"uid\": 0,
        \"gid\": 0,
        \"raw\": binary,
    }
""",
        __file__ + "<launcher-v2-gate>",
        "exec",
    ),
    _IMPL,
)

RootfsBuildError = _IMPL["RootfsBuildError"]
BUILDER_SHA256 = _IMPL["BUILDER_SHA256"]
LOCK_SCHEMA = _IMPL["LOCK_SCHEMA"]
canonical_json = _IMPL["canonical_json"]
load_json_exact = _IMPL["load_json_exact"]
directory_digest = _IMPL["directory_digest"]

BootProjectionError = boot_v1.BootProjectionError
BootSuccessorProjectionError = boot_v2.BootSuccessorProjectionError
BootMergeProjectionError = boot_v3.BootMergeProjectionError


def validate_source_lock(
    lock: dict[str, Any],
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: Optional[pathlib.Path],
    *,
    require_complete: bool,
) -> dict[str, Any]:
    """Run the inherited guards, ending in this generation's namespace."""

    for closure in lock.get("closureRoots") or []:
        roots = closure.get("logicalRoots") or []
        if list(roots) != sorted(set(roots)):
            raise BootProjectionError(
                f"closure {closure.get('name')!r} is not sorted and unique; "
                "run normalized_runtime_lock first so both orders stay on the record"
            )
    boot_v1.assert_single_architecture(lock)
    return _IMPL["validate_source_lock"](
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        require_complete=require_complete,
    )


def nested_runtime_tree(
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> dict[str, dict[str, Any]]:
    """Reuse v3's sealed nested-tree derivation; it contains no launcher."""

    return boot_v3.nested_runtime_tree(repository_root, artifact_store, gpgv, zstd)


def materialize_staging_tree(
    validated: dict[str, Any],
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    *,
    launcher_binary: Any = None,
    nested_tree: Any = None,
) -> dict[str, dict[str, Any]]:
    """The same fresh assembler object used by a future build call."""

    return _IMPL["_assemble_entries"](
        validated, repository_root, artifact_store, launcher_binary, nested_tree
    )


SUCCESSOR_PROJECTION_SHA256 = hashlib.sha256(
    pathlib.Path(__file__).resolve().read_bytes()
).hexdigest()


def __getattr__(name: str) -> Any:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc
