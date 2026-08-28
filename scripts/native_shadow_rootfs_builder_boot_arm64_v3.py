#!/usr/bin/env python3
"""Successor projection of the boot rootfs builder: one merge for the nested tree.

The fourth step declared a second rootfs staged under
``/var/lib/boole/native-shadow/runtime-rootfs`` and deliberately did not merge it
into a build, because the assembled totals were still bounds rather than
measurements and taking the measurement is an assembly step.  This step opens that
merge, and opens it in exactly one place.

The place matters.  The boot projection already reserved a point inside
``_assemble_entries`` for entries the lock does not name -- the kernel mount
points are merged there, immediately before ``_ensure_parents`` derives the
directories every member needs.  The nested tree is merged at the same point, one
line later, for the same three reasons: the frozen builder's own ``_merge``
refuses a colliding path in its own words rather than overwriting it, the parent
directories of the nested tree are then derived by the builder rather than
guessed here, and the entry and byte limits at the end of ``_assemble_entries``
see the combined table rather than the boot half alone.

Because ``build_oci_layout`` and ``verify_oci_layout`` call that same function,
production consumes this merge by consuming the module.  A measurement run and a
production run therefore assemble through one code path rather than two that have
to be kept in agreement, which is what the sealed plan requires of them.

``nested_tree`` stays optional and defaults to ``None``.  A caller that supplies
none gets the build the predecessor already produced, byte for byte, so this
projection is a superset rather than a replacement.

What this module still does not do is produce an image.  It assembles an entry
table and, if asked, an OCI layout; no kernel is extracted, no initrd is built,
no root disk is written and no image workflow is dispatched.  Nothing here claims
a bootable image, a serving path or an activation.
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
from scripts import native_shadow_rootfs_portable_arm64_v1 as portable_arm64


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
IMAGE_PRODUCED_CLAIM = False

BOOT_V2 = pathlib.Path(boot_v2.__file__).resolve()
BOOT_V2_SHA256 = "82b96d5a1ab465a710725d580ef58ddb3e1bd4f1db2a11b7e6ccb85fb6acf655"

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]

# Inherited unchanged.  The fifteen-row staging table and the nested tree's own
# declaration were settled by the fourth step; this step only merges.
BOOT_AUTHORITY_FILES = dict(boot_v2.BOOT_AUTHORITY_FILES)
NESTED_RUNTIME_TREE = dict(boot_v2.NESTED_RUNTIME_TREE)
BOOT_SOURCE_LOCK_RELEASE = boot_v2.BOOT_SOURCE_LOCK_RELEASE


class BootMergeProjectionError(RuntimeError):
    """The predecessor projection is not the one this successor expects."""


REPLACEMENTS = (
    # The parameter, threaded the way the predecessor threaded the launcher.
    (
        "def _assemble_entries(\n"
        "    validated: dict[str, Any],\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    launcher_binary: Any = None,\n"
        ") -> dict[str, dict[str, Any]]:\n",
        "def _assemble_entries(\n"
        "    validated: dict[str, Any],\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    launcher_binary: Any = None,\n"
        "    nested_tree: Any = None,\n"
        ") -> dict[str, dict[str, Any]]:\n",
        1,
    ),
    # The merge itself: after the mount points, before the parents are derived.
    # `_merge` is the frozen builder's, so a path that is already occupied by a
    # different entry stops the assembly in the frozen builder's own words.
    (
        '    _merge(entries, runtime_mount_point_entries(), "runtime mount point")\n'
        "    _ensure_parents(entries)\n",
        '    _merge(entries, runtime_mount_point_entries(), "runtime mount point")\n'
        "    if nested_tree is not None:\n"
        '        _merge(entries, nested_tree, "nested runtime tree")\n'
        "    _ensure_parents(entries)\n",
        1,
    ),
    (
        "def build_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    output_dir: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        "    launcher_binary: Any = None,\n"
        ") -> dict[str, Any]:\n",
        "def build_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    output_dir: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        "    launcher_binary: Any = None,\n"
        "    nested_tree: Any = None,\n"
        ") -> dict[str, Any]:\n",
        1,
    ),
    (
        "    entries = _assemble_entries(\n"
        "        validated, repository_root, artifact_store, launcher_binary\n"
        "    )\n",
        "    entries = _assemble_entries(\n"
        "        validated, repository_root, artifact_store, launcher_binary, nested_tree\n"
        "    )\n",
        1,
    ),
    (
        "def verify_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    layout: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        "    launcher_binary: Any = None,\n"
        ") -> dict[str, Any]:\n",
        "def verify_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    layout: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        "    launcher_binary: Any = None,\n"
        "    nested_tree: Any = None,\n"
        ") -> dict[str, Any]:\n",
        1,
    ),
    # A verify rebuilds and compares, so it has to rebuild the same tree.
    (
        "            rebuilt,\n"
        "            trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,\n"
        "            launcher_binary=launcher_binary,\n"
        "        )\n",
        "            rebuilt,\n"
        "            trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,\n"
        "            launcher_binary=launcher_binary,\n"
        "            nested_tree=nested_tree,\n"
        "        )\n",
        1,
    ),
)


def _derived_source() -> str:
    raw = BOOT_V2.read_bytes()
    if hashlib.sha256(raw).hexdigest() != BOOT_V2_SHA256:
        raise BootMergeProjectionError("boot staging-table projection bytes differ")
    source = boot_v2._derived_source()
    for old, new, count in REPLACEMENTS:
        found = source.count(old)
        if found != count:
            raise BootMergeProjectionError(
                f"boot builder source shape differs: expected {count} of "
                f"{old.splitlines()[0]!r}, found {found}"
            )
        source = source.replace(old, new, count)
    return source


_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_builder_boot_arm64_v3_impl",
    "__file__": str(pathlib.Path(arm64.__file__).resolve()),
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

# The widened tables the fourth step computed, bound into this fresh namespace for
# the same reason it had to bind them into its own: the functions read these names
# from the globals they were compiled with.
_IMPL["EXPECTED_AUTHORITY_FILES"] = dict(
    _IMPL["EXPECTED_AUTHORITY_FILES"], **BOOT_AUTHORITY_FILES
)
_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"] = dict(
    _IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"], **boot_v1.BOOT_PROVENANCE_CLOSURE_ROOTS
)
_IMPL["REQUIRED_PROVENANCE_CLOSURES"] = tuple(
    sorted(_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"])
)
for _name in boot_v2.INHERITED_INJECTIONS:
    _IMPL[_name] = getattr(boot_v1, _name)

RootfsBuildError = _IMPL["RootfsBuildError"]
BUILDER_SHA256 = _IMPL["BUILDER_SHA256"]
LOCK_SCHEMA = _IMPL["LOCK_SCHEMA"]
canonical_json = _IMPL["canonical_json"]
load_json_exact = _IMPL["load_json_exact"]
directory_digest = _IMPL["directory_digest"]

BootProjectionError = boot_v1.BootProjectionError
BootSuccessorProjectionError = boot_v2.BootSuccessorProjectionError

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
    """The predecessor's guards, then this namespace's own validation.

    Restated for the same reason the predecessor restated it: the guard has to end
    in the namespace being validated, and this module holds a third one.
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


def nested_runtime_tree(
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> dict[str, dict[str, Any]]:
    """Assemble the sealed runtime rootfs and place it under the guest prefix.

    The runtime rootfs has its own sealed source lock, its own release gate and its
    own builder, and this reads all three rather than reproducing any of them.  The
    lock's bytes are checked against the digest the fourth step sealed, so a lock
    that has moved is refused here rather than measured.
    """

    path = repository_root / NESTED_RUNTIME_TREE["drivingSourceLockPath"]
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != NESTED_RUNTIME_TREE["drivingSourceLockSha256"]:
        raise BootMergeProjectionError(
            f"nested runtime source lock hashes to {digest}, the seal says "
            f"{NESTED_RUNTIME_TREE['drivingSourceLockSha256']}"
        )
    sealed = json.loads(raw.decode("utf-8"))
    runtime, _receipt = portable_arm64.materialize_runtime_lock(sealed, raw, gpgv, zstd)
    runtime_raw = arm64.canonical_json(runtime)
    validated = arm64.validate_source_lock(
        runtime, runtime_raw, repository_root, artifact_store, require_complete=True
    )
    entries = arm64.__getattr__("_assemble_entries")(
        validated, repository_root, artifact_store
    )
    return boot_v2.nested_tree_entries(entries, validated["lock"]["closureRoots"])


def materialize_staging_tree(
    validated: dict[str, Any],
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    *,
    launcher_binary: Any = None,
    nested_tree: Any = None,
) -> dict[str, dict[str, Any]]:
    """The one function that turns a validated lock into the final entry table.

    ``build_oci_layout`` calls the same ``_assemble_entries`` with the same
    arguments, so a measurement taken through here and an image produced through
    there assemble the same tree by construction rather than by agreement.
    """

    return _IMPL["_assemble_entries"](
        validated, repository_root, artifact_store, launcher_binary, nested_tree
    )


def __getattr__(name: str) -> Any:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc
