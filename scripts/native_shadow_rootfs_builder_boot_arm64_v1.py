#!/usr/bin/env python3
"""Boot projection of the frozen Linux/arm64 native-shadow rootfs builder.

The boot source lock pins ``buildRecipe.builderSha256`` to the arm64 builder's
own bytes, so the builder that reads it has to be exactly that file.  This
module therefore projects the same bytes -- ``BUILDER_SHA256`` here equals the
pin -- and records its own digest separately as ``BOOT_PROJECTION_SHA256``,
because the widening below is not covered by the pin and should not pretend to
be.

Four kinds of change, in decreasing order of ordinariness.

The first two are what a projection is for: the authority-file table grows from
six entries to ten (machine-id, the launcher unit, and the sysusers and tmpfiles
configuration the unit needs), and the provenance-closure table grows from three
to five (the guest init/launcher closure and the guest kernel/module closure).
Both are configuration for a new release, not a weakening of any check.

The third is an ordering normalisation.  One closure in the sealed lock lists
its logical roots in an order that is alphabetical but not byte-sorted, and the
frozen builder compares against ``sorted(set(roots))``.  The sealed bytes are
not touched: ``normalized_runtime_lock`` sorts a copy and records the declared
and normalised orders side by side, and refuses outright if sorting would also
drop a duplicate, because that half of the builder's check must stay real.

The fourth touches the frozen builder's dependency reading, and is the one worth
reading closely.

``_split_dependency_groups`` rejects architecture-qualified and build-profile
dependencies -- correctly -- but applies the pattern to the whole comma-joined
field.  ``python3 (<< 3.13), python3 (>= 3.12~)`` then matches, because the
``<`` of one constraint and the ``>`` of a later one look like a profile once
everything between them is ignored.  Across all 5816 dependency fields in the
frozen Packages index this fires 98 times and is wrong all 98 times; the same
scan finds no real qualifier anywhere in the index.  The fix applies the same
pattern to each alternative instead of the joined string.  A real ``[arm64]`` or
``<!nocheck>`` lives entirely inside one alternative, so nothing that the
original rejects survives the narrower scope.

``_dependency_matches`` refuses every Multi-Arch qualifier.  Five packages in
this closure depend on ``python3:any``, and all five resolve to the same
``python3``.  ``:any`` means "satisfied by that package from any architecture";
this closure holds exactly one concrete architecture, so ``:any`` and the bare
name select the same candidate.  That equivalence is a fact about this closure
rather than a general one, so it is guarded twice: ``assert_single_architecture``
refuses a second concrete architecture, and the projected matcher refuses
``:any`` against a provider that does not declare ``Multi-Arch: allowed``.
``:native`` stays unsupported.

Projecting a builder is not building an image.  Nothing here writes one.
"""

from __future__ import annotations

import copy
import hashlib
import pathlib
import sys
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_builder_arm64_v1 as arm64


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

ARM64_BUILDER = pathlib.Path(arm64.__file__).resolve()
ARM64_BUILDER_SHA256 = "180e893e9643c6fab110016119679b96a5ddf56785cd398b51c8cf8352615ef4"

BOOT_SOURCE_LOCK_RELEASE = (
    "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"
)

BOOT_AUTHORITY_FILES = {
    "guest-machine-id": (
        "native/etc/machine-id",
        "/etc/machine-id",
    ),
    "launcher-unit": (
        "native/systemd/boole-native-shadow-launcher.service",
        "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
    ),
    "sysusers-config": (
        "native/sysusers.d/boole-native-shadow.conf",
        "/usr/lib/sysusers.d/boole-native-shadow.conf",
    ),
    "tmpfiles-config": (
        "native/tmpfiles.d/boole-native-shadow.conf",
        "/usr/lib/tmpfiles.d/boole-native-shadow.conf",
    ),
}
BOOT_PROVENANCE_CLOSURE_ROOTS = {
    "guest-init-and-launcher-file-manifest": (
        "/etc/systemd",
        "/usr/lib/systemd",
        "/usr/lib/sysusers.d",
        "/usr/lib/tmpfiles.d",
        "/usr/libexec/boole",
    ),
    "guest-kernel-and-module-file-manifest": (
        "/boot",
        "/usr/lib/modules",
    ),
}

ARCHITECTURE_INDEPENDENT = "all"
MULTI_ARCH_ANY_REQUIREMENT = "allowed"

REPLACEMENTS = (
    (
        '    if re.search(r"\\[[^\\]]*\\]|<[^<>=]+>", value):\n'
        '        raise RootfsBuildError("architecture/profile-qualified runtime dependency is unsupported")\n'
        "    groups: list[list[str]] = []\n"
        '    for raw_group in value.split(","):\n'
        '        alternatives = [item.strip() for item in raw_group.split("|")]\n'
        "        if not alternatives or any(not item for item in alternatives):\n"
        '            raise RootfsBuildError("Ubuntu dependency group syntax differs")\n'
        "        groups.append(alternatives)\n",
        "    groups: list[list[str]] = []\n"
        '    for raw_group in value.split(","):\n'
        '        alternatives = [item.strip() for item in raw_group.split("|")]\n'
        "        if not alternatives or any(not item for item in alternatives):\n"
        '            raise RootfsBuildError("Ubuntu dependency group syntax differs")\n'
        "        for alternative in alternatives:\n"
        '            if re.search(r"\\[[^\\]]*\\]|<[^<>=]+>", alternative):\n'
        '                raise RootfsBuildError("architecture/profile-qualified runtime dependency is unsupported")\n'
        "        groups.append(alternatives)\n",
        1,
    ),
    (
        '    if match.group("qualifier") is not None:\n'
        '        raise RootfsBuildError("Ubuntu Multi-Arch dependency qualifier is not supported in v1")\n',
        '    if match.group("qualifier") not in (None, ":any"):\n'
        '        raise RootfsBuildError("Ubuntu Multi-Arch dependency qualifier is not supported in v1")\n'
        '    if match.group("qualifier") == ":any" and candidate.get("multiArch") != "allowed":\n'
        '        raise RootfsBuildError("Ubuntu :any dependency needs a Multi-Arch: allowed provider")\n',
        1,
    ),
)


class BootProjectionError(RuntimeError):
    """The boot source lock does not satisfy what this projection guards."""


def _derived_source() -> str:
    raw = ARM64_BUILDER.read_bytes()
    if hashlib.sha256(raw).hexdigest() != ARM64_BUILDER_SHA256:
        raise BootProjectionError("frozen arm64 rootfs builder bytes differ")
    source = arm64._derived_source()
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise BootProjectionError(
                f"boot builder projection count differs: {actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


# ``BUILDER_SOURCE_BYTES`` reads ``__file__``, and the sealed lock pins the arm64
# builder's digest, so the projected namespace has to name that file.  This
# module's own contribution is recorded below as ``BOOT_PROJECTION_SHA256``.
_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_builder_boot_arm64_v1_impl",
    "__file__": str(ARM64_BUILDER),
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

_IMPL["EXPECTED_AUTHORITY_FILES"] = dict(
    _IMPL["EXPECTED_AUTHORITY_FILES"], **BOOT_AUTHORITY_FILES
)
_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"] = dict(
    _IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"], **BOOT_PROVENANCE_CLOSURE_ROOTS
)
_IMPL["REQUIRED_PROVENANCE_CLOSURES"] = tuple(
    sorted(_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"])
)

RootfsBuildError = _IMPL["RootfsBuildError"]
BUILDER_SHA256 = _IMPL["BUILDER_SHA256"]
LOCK_SCHEMA = _IMPL["LOCK_SCHEMA"]
canonical_json = _IMPL["canonical_json"]
load_json_exact = _IMPL["load_json_exact"]
build_oci_layout = _IMPL["build_oci_layout"]
verify_oci_layout = _IMPL["verify_oci_layout"]
directory_digest = _IMPL["directory_digest"]

BOOT_PROJECTION_SHA256 = hashlib.sha256(
    pathlib.Path(__file__).resolve().read_bytes()
).hexdigest()


def concrete_architectures(lock: dict[str, Any]) -> tuple[str, ...]:
    """Every architecture in the closure that is not ``all``."""

    try:
        packages = lock["ubuntu"]["packages"]
    except (KeyError, TypeError) as exc:
        raise BootProjectionError("boot source lock has no Ubuntu package closure") from exc
    found = {
        package.get("architecture")
        for package in packages
        if package.get("architecture") != ARCHITECTURE_INDEPENDENT
    }
    if None in found:
        raise BootProjectionError("an Ubuntu package in the closure declares no architecture")
    return tuple(sorted(found))


def assert_single_architecture(lock: dict[str, Any]) -> str:
    """``:any`` equals the bare name only while one architecture is present."""

    found = concrete_architectures(lock)
    if len(found) != 1:
        raise BootProjectionError(
            "boot closure must hold exactly one concrete architecture, found: "
            + (", ".join(found) or "none")
        )
    declared = lock.get("platform", {}).get("debArchitecture")
    if declared != found[0]:
        raise BootProjectionError(
            f"closure architecture {found[0]!r} differs from platform {declared!r}"
        )
    return found[0]


def normalized_runtime_lock(
    lock: dict[str, Any],
) -> tuple[dict[str, Any], bytes, dict[str, Any]]:
    """Sort each closure's logical roots and keep both orders on the record."""

    result = copy.deepcopy(lock)
    closures = result.get("closureRoots")
    if not isinstance(closures, list) or not closures:
        raise BootProjectionError("boot source lock has no provenance closures")
    rows: list[dict[str, Any]] = []
    for closure in closures:
        declared = closure.get("logicalRoots")
        if not isinstance(declared, list) or not declared:
            raise BootProjectionError("a provenance closure has no logical roots")
        if len(set(declared)) != len(declared):
            raise BootProjectionError(
                f"duplicate logical root in closure: {closure.get('name')!r}"
            )
        normalized = sorted(declared)
        closure["logicalRoots"] = normalized
        rows.append(
            {
                "declaredOrder": list(declared),
                "name": closure.get("name"),
                "normalizedOrder": list(normalized),
                "reordered": list(declared) != normalized,
            }
        )
    record = {
        "closureRoots": rows,
        "reorderedClosures": [row["name"] for row in rows if row["reordered"]],
    }
    return result, canonical_json(result), record


def validate_source_lock(
    lock: dict[str, Any],
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: Optional[pathlib.Path],
    *,
    require_complete: bool,
) -> dict[str, Any]:
    """Guard what the widening depends on, then hand the frozen builder the lock."""

    for closure in lock.get("closureRoots") or []:
        roots = closure.get("logicalRoots") or []
        if list(roots) != sorted(set(roots)):
            raise BootProjectionError(
                f"closure {closure.get('name')!r} is not sorted and unique; "
                "run normalized_runtime_lock first so both orders stay on the record"
            )
    assert_single_architecture(lock)
    return _IMPL["validate_source_lock"](
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        require_complete=require_complete,
    )


def __getattr__(name: str) -> Any:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
