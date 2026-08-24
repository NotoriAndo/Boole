#!/usr/bin/env python3
"""Host-independent arm64 authority projection of portable rootfs v2."""

from __future__ import annotations

import hashlib
import pathlib
import sys
from typing import Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))


LEGACY = pathlib.Path(__file__).with_name("native_shadow_rootfs_portable_v2.py")
LEGACY_SHA256 = "11fe7f5672655cbfcf88e830d34ccc5b35274857df06cc123ed05e775bcd4fc3"

REPLACEMENTS = (
    ("native_shadow_rootfs_builder", "native_shadow_rootfs_builder_arm64_v1", 3),
    ("native_shadow_rootfs_acquire", "native_shadow_rootfs_acquire_arm64_v1", 4),
    ("boole.native-shadow.runtime-rootfs-source-lock.v1", "boole.native-shadow.runtime-rootfs-source-lock.arm64.runtime.v1", 1),
    ("boole.native-shadow.runtime-rootfs-source-lock.v2", "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1", 1),
    ("boole.native-shadow.runtime-rootfs-portable-plan.v2", "boole.native-shadow.runtime-rootfs-portable-plan.arm64.v1", 1),
    ("boole.native-shadow.runtime-rootfs-resolution.v1", "boole.native-shadow.runtime-rootfs-resolution.arm64.runtime.v1", 1),
    ("boole.native-shadow.runtime-rootfs-resolution.v2", "boole.native-shadow.runtime-rootfs-resolution.arm64.v1", 1),
    ("boole.native-shadow.runtime-rootfs-replay-expectation.v2", "boole.native-shadow.runtime-rootfs-replay-expectation.arm64.v1", 1),
    ("NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-V2-PORTABLE-NOT-ACTIVATABLE", "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-ARM64-V1-PORTABLE-NOT-ACTIVATABLE", 1),
    ("NATIVE-SHADOW-RUNTIME-ROOTFS-PORTABLE-SUCCESSOR-V2-QUALIFICATION", "NATIVE-SHADOW-RUNTIME-ROOTFS-PORTABLE-SUCCESSOR-ARM64-V1-QUALIFICATION", 1),
    ("NATIVE-SHADOW-RUNTIME-ROOTFS-RESOLUTION-V2-PORTABLE", "NATIVE-SHADOW-RUNTIME-ROOTFS-RESOLUTION-ARM64-V1-PORTABLE", 2),
    ("NATIVE-SHADOW-RUNTIME-ROOTFS-REPLAY-EXPECTATION-V2-FROZEN", "NATIVE-SHADOW-RUNTIME-ROOTFS-REPLAY-EXPECTATION-ARM64-V1-FROZEN", 1),
    ("native-shadow-runtime-rootfs-acquisition-plan-v1.json", "native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json", 2),
    ("native-shadow-runtime-rootfs-source-lock-v1.json", "native-shadow-runtime-rootfs-source-lock-arm64-v1-scaffold.json", 2),
    ("native-shadow-runtime-rootfs-portable-plan-v2.json", "native-shadow-runtime-rootfs-portable-plan-arm64-v1.json", 1),
    ("native-shadow-runtime-rootfs-resolution-v2.json", "native-shadow-runtime-rootfs-resolution-arm64-v1.json", 1),
    ("native-shadow-runtime-rootfs-source-lock-v2.json", "native-shadow-runtime-rootfs-source-lock-arm64-v1.json", 1),
    ("native-shadow-runtime-rootfs-replay-expectation-v2.json", "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json", 1),
    (
        '            "expectedOutput",\n            "mismatchPolicy",',
        '            "expectedOutput",\n            "mismatchPolicy",\n            "platform",',
        1,
    ),
    (
        '        or expectation["productionByteProvenanceComplete"] is not False',
        '        or expectation["productionByteProvenanceComplete"] is not False\n        or expectation["platform"] != {"architecture": "arm64", "os": "linux"}',
        1,
    ),
    ("x86_64-linux-gnu", "aarch64-linux-gnu", 4),
    ("librustc_driver-da0d54ffe246e605.so", "librustc_driver-88e5010b55632922.so", 2),
)


def _derived_source() -> str:
    raw = LEGACY.read_bytes()
    if hashlib.sha256(raw).hexdigest() != LEGACY_SHA256:
        raise RuntimeError("frozen x86 portable authority bytes differ")
    source = raw.decode("utf-8")
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise RuntimeError(
                f"arm64 portable projection count differs for {old!r}: "
                f"{actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


_IMPL: dict[str, object] = {
    "__name__": "scripts.native_shadow_rootfs_portable_arm64_v1_impl",
    "__file__": __file__,
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

# Linux/arm64 Noble has no /usr/lib64 directory.  Keeping the x86 usrmerge
# alias would create a dangling rootfs link, so the successor removes that one
# architecture-specific entry while preserving every other portable alias.
_IMPL["PORTABLE_V2_DERIVED_ENTRIES"] = tuple(
    row
    for row in _IMPL["PORTABLE_V2_DERIVED_ENTRIES"]
    if row["logicalPath"] != "/lib64"
)

PortableAuthorityError = _IMPL["PortableAuthorityError"]
portable_source_lock_from_v1 = _IMPL["portable_source_lock_from_v1"]
portable_resolution_from_runtime = _IMPL["portable_resolution_from_runtime"]
materialize_runtime_lock = _IMPL["materialize_runtime_lock"]
runtime_plan_from_v1 = _IMPL["runtime_plan_from_v1"]
load_authority_set = _IMPL["load_authority_set"]
verify_replay_output = _IMPL["verify_replay_output"]


def __getattr__(name: str) -> object:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
