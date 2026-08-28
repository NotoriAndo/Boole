#!/usr/bin/env python3
"""Successor projection of the boot rootfs builder: the fifteen-file staging table.

The sealed successor lock names fifteen tracked files where its predecessor named
ten -- five account files added, two sources superseded.  The builder decides
which sources may appear in a lock by comparing the lock's authority bindings
against a table, exactly, in both directions.  A lock carrying five rows the table
does not know is refused with ``authority binding identity/source set differs``,
which is the refusal the predecessor gives the successor lock today.

The table is therefore widened here rather than there.  The predecessor keeps its
bytes and its ten rows, so the lock it was written for still validates against it;
this module projects the same builder a second time with the wider table.  That is
the same relationship the predecessor has to the frozen arm64 builder, and the
arm64 builder to the frozen original.

Two things are worth reading closely.

The first is that the namespace has to be built again rather than copied.  The
functions inside the projected namespace read ``EXPECTED_AUTHORITY_FILES`` from
the globals they were compiled with, so rebinding the name in a copy of that
namespace would leave every function still reading the predecessor's ten.  The
derived source is therefore executed a second time and the wider tables are bound
into the fresh namespace.  ``INHERITED_INJECTIONS`` names the nine objects the
predecessor injects that this module reuses by identity, and the step-four gate
parses the predecessor's own source to require that the accounting is complete: a
tenth injection there fails the test here rather than going unnoticed.

The second is the nested runtime tree.  The sealed successor declares a second
rootfs staged under ``/var/lib/boole/native-shadow/runtime-rootfs`` with its
content manifest beside it, and the launcher verifies that manifest by a digest
compiled into the launcher itself.  The manifest is derived from the assembled
entries by the *runtime* builder rather than this one: the document records each
entry's closure names, and assembled under the boot closures it would name five
where the launcher expects the runtime three.  ``nested_content_manifest`` calls
the frozen runtime builder for exactly that reason, and refuses when what it
derives is not the document the launcher compiles against.

What this module does not do is merge the nested tree into a build.  The sealed
plan requires the assembled byte and entry totals to be measured rather than
bounded, and that measurement is taken immediately before assembly, which is a
production step.  Nothing here assembles a tree, produces an image or boots one.
"""

from __future__ import annotations

import hashlib
import pathlib
import re
import sys
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_builder_arm64_v1 as arm64
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_v1


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
NESTED_RUNTIME_TREE_ASSEMBLED = False

BOOT_V1 = pathlib.Path(boot_v1.__file__).resolve()
BOOT_V1_SHA256 = "a5dd54198878473c162ec306fbccd6edac8b22f036d9cf84d244b5f010f96d87"

BOOT_SOURCE_LOCK_RELEASE = (
    "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V2-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"
)

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
SUCCESSOR_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)

# The seven rows that differ from the predecessor's table: the five account files
# the guest's own identity contract reads, and the two sources the successor lock
# supersedes.  Both superseded rows keep the guest path the predecessor gave them,
# because only the source bytes moved.
SUCCESSOR_AUTHORITY_FILES = {
    "guest-group": (
        "native/etc/group",
        "/etc/group",
    ),
    "guest-gshadow": (
        "native/etc/gshadow",
        "/etc/gshadow",
    ),
    "guest-nsswitch": (
        "native/etc/nsswitch.conf",
        "/etc/nsswitch.conf",
    ),
    "guest-passwd": (
        "native/etc/passwd",
        "/etc/passwd",
    ),
    "guest-shadow": (
        "native/etc/shadow",
        "/etc/shadow",
    ),
    "launcher-unit": (
        "native/systemd/boole-native-shadow-launcher-v2.service",
        "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
    ),
    "tmpfiles-config": (
        "native/tmpfiles.d/boole-native-shadow-v2.conf",
        "/usr/lib/tmpfiles.d/boole-native-shadow.conf",
    ),
}
BOOT_AUTHORITY_FILES = dict(boot_v1.BOOT_AUTHORITY_FILES, **SUCCESSOR_AUTHORITY_FILES)

# The three the predecessor computes from its own tables, and the nine it injects
# as objects.  The gate requires their union to be exactly what the predecessor's
# source injects, so this list cannot silently fall behind.
RECOMPUTED_INJECTIONS = (
    "EXPECTED_AUTHORITY_FILES",
    "EXPECTED_PROVENANCE_CLOSURE_ROOTS",
    "REQUIRED_PROVENANCE_CLOSURES",
)
INHERITED_INJECTIONS = (
    "DANGLING_SYMLINKS",
    "METADATA_EXCEPTIONS",
    "USRMERGE_ROOTS",
    "_dangling_allowed",
    "_metadata_exception",
    "_usrmerge",
    "launcher_entry",
    "read_launcher",
    "runtime_mount_point_entries",
)

NESTED_RUNTIME_TREE = {
    "id": "runtime-rootfs",
    "guestPrefix": "/var/lib/boole/native-shadow/runtime-rootfs",
    "contentManifestGuestPath": "/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json",
    "contentManifestMode": 0o444,
    "contentManifestSchema": "boole.native-shadow.rootfs-content-manifest.arm64.v1",
    "contentManifestSha256": (
        "200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa"
    ),
    "contentManifestSizeBytes": 1285116,
    "drivingSourceLockPath": (
        "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
    ),
    "drivingSourceLockSha256": (
        "829ca81d321d412746cce7a62d59d7e538c394b92c1b6a9a966f3016b73cede0"
    ),
    # The sealed layer size of the runtime image, carried so the declaration and
    # the sealed result cannot drift.  It is the size of a layer that was built,
    # not of a tree that was staged here; the total that decides is measured
    # immediately before assembly.
    "layerSizeBytes": 766556160,
    "layerSizeBytesIsAMeasuredTotal": False,
}


class BootSuccessorProjectionError(RuntimeError):
    """The predecessor projection is not the one this successor expects."""


def _derived_source() -> str:
    raw = BOOT_V1.read_bytes()
    if hashlib.sha256(raw).hexdigest() != BOOT_V1_SHA256:
        raise BootSuccessorProjectionError("boot rootfs builder projection bytes differ")
    return boot_v1._derived_source()


def _assert_injection_accounting() -> None:
    """Every name the predecessor binds into its namespace is reproduced here."""

    source = BOOT_V1.read_text(encoding="utf-8")
    injected = sorted(set(re.findall(r'^_IMPL\["(\w+)"\] = ', source, re.MULTILINE)))
    accounted = sorted(set(RECOMPUTED_INJECTIONS) | set(INHERITED_INJECTIONS))
    if injected != accounted:
        raise BootSuccessorProjectionError(
            "the predecessor projection injects names this successor does not "
            f"reproduce: {sorted(set(injected) ^ set(accounted))}"
        )


_assert_injection_accounting()

_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_builder_boot_arm64_v2_impl",
    "__file__": str(pathlib.Path(arm64.__file__).resolve()),
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

_IMPL["EXPECTED_AUTHORITY_FILES"] = dict(
    _IMPL["EXPECTED_AUTHORITY_FILES"], **BOOT_AUTHORITY_FILES
)
_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"] = dict(
    _IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"], **boot_v1.BOOT_PROVENANCE_CLOSURE_ROOTS
)
_IMPL["REQUIRED_PROVENANCE_CLOSURES"] = tuple(
    sorted(_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"])
)
for _name in INHERITED_INJECTIONS:
    _IMPL[_name] = getattr(boot_v1, _name)

RootfsBuildError = _IMPL["RootfsBuildError"]
BUILDER_SHA256 = _IMPL["BUILDER_SHA256"]
LOCK_SCHEMA = _IMPL["LOCK_SCHEMA"]
canonical_json = _IMPL["canonical_json"]
load_json_exact = _IMPL["load_json_exact"]
directory_digest = _IMPL["directory_digest"]

# Raised by the guards this projection shares with its predecessor, so a caller
# that catches the predecessor's class still catches them.
BootProjectionError = boot_v1.BootProjectionError

SUCCESSOR_PROJECTION_SHA256 = hashlib.sha256(
    pathlib.Path(__file__).resolve().read_bytes()
).hexdigest()


def validate_source_lock(
    lock: dict[str, Any],
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: Optional[pathlib.Path],
    *,
    require_complete: bool,
) -> dict[str, Any]:
    """The predecessor's guards, then the widened namespace's own validation.

    The guard is restated rather than inherited because the predecessor's version
    ends by calling its own namespace, which is the narrower table this module
    exists to widen.  The gate requires both modules to refuse an unsorted closure
    with the same words, so the two cannot drift apart silently.
    """

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


def nested_content_manifest(
    entries: dict[str, dict[str, Any]],
    closure_roots: list,
    *,
    sha256: Optional[str] = NESTED_RUNTIME_TREE["contentManifestSha256"],
    size: Optional[int] = NESTED_RUNTIME_TREE["contentManifestSizeBytes"],
) -> tuple[dict[str, Any], bytes]:
    """Derive the nested tree's own manifest, by the builder that will read it.

    The frozen runtime builder writes this document, not this one: it records the
    closure each entry belongs to, and the closures that have to appear are the
    runtime three the launcher compiles against rather than the boot five.

    ``sha256`` and ``size`` default to the values sealed into the launcher and
    into the replay expectation.  Passing ``None`` derives the document without
    comparing it, which is for reading its shape and never for a build.
    """

    if not entries:
        raise BootSuccessorProjectionError("the nested runtime tree has no entries")
    for path in entries:
        if not path or path.startswith("/"):
            raise BootSuccessorProjectionError(
                f"nested runtime entry is already rooted: {path!r}"
            )
    manifest = arm64.__getattr__("_entry_manifest")(entries, closure_roots)
    raw = canonical_json(manifest)
    schema = NESTED_RUNTIME_TREE["contentManifestSchema"]
    if manifest.get("schema") != schema:
        raise BootSuccessorProjectionError(
            f"nested content manifest schema differs: {manifest.get('schema')!r}"
        )
    if size is not None and len(raw) != size:
        raise BootSuccessorProjectionError(
            f"nested content manifest is {len(raw)} bytes, the seal says {size}"
        )
    if sha256 is not None:
        derived = hashlib.sha256(raw).hexdigest()
        if derived != sha256:
            raise BootSuccessorProjectionError(
                f"nested content manifest hashes to {derived}, the seal says {sha256}"
            )
    return manifest, raw


def nested_tree_entries(
    entries: dict[str, dict[str, Any]],
    closure_roots: list,
    *,
    sha256: Optional[str] = NESTED_RUNTIME_TREE["contentManifestSha256"],
    size: Optional[int] = NESTED_RUNTIME_TREE["contentManifestSizeBytes"],
) -> dict[str, dict[str, Any]]:
    """Place an assembled runtime tree under the guest prefix, manifest beside it.

    A symlink keeps the text it was written with and moves where that text
    resolves: the entry now lives one prefix deeper, and at use time the launcher
    reads the tree as its own root, so the text is right in both readings while
    the recorded resolution has to follow the move.

    Parent directories are left to the frozen builder, which derives them from the
    entries it holds.  A caller that merges this in has to do so before that step
    rather than after it.
    """

    _, raw = nested_content_manifest(entries, closure_roots, sha256=sha256, size=size)
    prefix = NESTED_RUNTIME_TREE["guestPrefix"].lstrip("/")
    manifest_path = NESTED_RUNTIME_TREE["contentManifestGuestPath"].lstrip("/")
    staged: dict[str, dict[str, Any]] = {}
    for path, entry in sorted(entries.items()):
        nested_path = f"{prefix}/{path}"
        moved = dict(entry, path=nested_path)
        if entry["kind"] == "symlink":
            moved["resolvedTarget"] = f"{prefix}/{entry['resolvedTarget']}"
        elif entry["kind"] == "hardlink":
            moved["target"] = f"{prefix}/{entry['target']}"
        staged[nested_path] = moved
    if manifest_path in staged:
        raise BootSuccessorProjectionError(
            "the nested runtime tree covers its own content manifest path"
        )
    staged[manifest_path] = {
        "path": manifest_path,
        "kind": "file",
        "mode": NESTED_RUNTIME_TREE["contentManifestMode"],
        "uid": 0,
        "gid": 0,
        "raw": raw,
    }
    return staged


def __getattr__(name: str) -> Any:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc
