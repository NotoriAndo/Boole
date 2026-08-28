#!/usr/bin/env python3
"""Produce the successor boot image, from the second source lock, exactly once.

The historical phase beside this one reads the first boot source lock, hands it
to the first release gate, and assembles through a builder call that passes no
nested tree.  It is what reproduces the image that already booted and it is left
untouched, down to the byte.  It also cannot produce the successor, because the
three things the successor exists to add -- a guest account database, a nested
runtime tree with its content manifest, and a launcher unit whose output reaches
the console the host already collects -- are not in what it assembles.

So this is a second path rather than an edit to the first one.  It reads the
second lock, hands it to the second release gate, and assembles through the
latest projection with the nested tree as a required argument.  Two things follow
from that and both are enforced here rather than described:

The staging tree this path builds is the one the sealed measurement measured.
Not an equivalent tree -- the same one, assembled by the same function object,
reached through the same namespace.  ``assert_shared_assembler`` checks the
identity of the mapping, not the equality of two copies, because two copies can
drift and one object cannot.  The totals are frozen in this file from the sealed
record, and a run that reaches different ones fails instead of adopting them.

The other is that the expensive step happens once.  The line that says so is a
marker this file writes on purpose, atomically, immediately before the first
image file: a refusal raised before it has cost nothing and may be repeated, and
a failure raised after it has spent the only attempt there is, whatever happens
next.  It used to be the output directory, until a production failed in the one
case that reading did not name -- the directory existed because the isolation
needs it to exist, and no output file was ever written.  Everything that can be
checked is therefore checked on the near side of the marker, which is why
``preflight`` exists at all: it does the whole assembly and the whole walk, and
it cannot reach an image tool or the marker, because its call graph never
touches either.

Neither the launcher source nor its sealed binary is rebuilt or modified here.
The rebuilt bytes arrive as an argument and are matched against the seal.
"""

from __future__ import annotations

import ast
import contextlib
import hashlib
import json
import os
import pathlib
import stat
import sys
import tempfile
from typing import Any, Iterable, Mapping, Optional

# A workflow runs this file as `python3 scripts/<name>.py`, which puts `scripts/`
# on the path and not the root the package below is under.  The predecessor phase
# carries the same line for the same reason.
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_initrd_arm64_v1 as initrd
from scripts import native_shadow_boot_kernel_extract_arm64_v1 as kernel_extract
from scripts import native_shadow_boot_produce_phase_arm64_v1 as historical
from scripts import native_shadow_boot_image_produce_arm64_v1 as producer
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk
from scripts import native_shadow_boot_root_disk_execute_arm64_v1 as root_disk_execute
from scripts import native_shadow_boot_staging_measure_arm64_v1 as measurement
from scripts import native_shadow_boot_writer_tree_arm64_v1 as writer_tree_module
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as base
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as staging
from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as gate


RELEASE = "NATIVE-SHADOW-SUCCESSOR-PRODUCE-PHASE-ARM64-V2"

BOOTABLE_CLAIM = False
SERVING_CLAIM = False
IMAGE_PRODUCED_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_BOOT_VERIFIED = False

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPOSITORY_ROOT / "native/containment"
# The third authority: one further attempt, granted after two were spent under
# the second.  The second is not edited and not repointed -- it is bound by
# digest in the table below, along with the two records that count those
# attempts, so a run that would need any of them changed stops instead.
AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v3.json"
)
AUTHORITY_SHA256 = "0ff5000c0cea751a32d88d79028a2b53380262551517fe9ec2c1072df35afe06"
SOURCE_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v2.json"
SOURCE_LOCK_SHA256 = "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"
SOURCE_LOCK_RELEASE = (
    "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V2-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"
)
MEASUREMENT_PATH = (
    CONTAINMENT / "native-shadow-boot-staging-tree-measurement-arm64-v1.json"
)
MEASUREMENT_SHA256 = "a9b53199ca519def2232687c096a7fbefeef13a26f68ba44fcb9a3da30d35d18"
LAUNCHER_BUILD_RESULT_PATH = (
    CONTAINMENT / "native-shadow-launcher-build-result-arm64-v1.json"
)

RELEASE_GATE_SHA256 = "15f88cf286879ae30aae10bb7819aefea91095a819d96c2634ee9ecc4ea2f305"
BUILDER_SHA256 = "93bd05d06e43cc69f325036d204b7b57721e358dd5c5d5990227ef88c4de8c39"

# The base projection is not a competing builder: the latest one is this one's
# source with replacements applied.  The sealed measurement reaches it for the
# closure normalizer and for the launcher seal, and this path reaches it for the
# same two things and nothing else.  ``assert_base_projection_scope`` enforces it.
BASE_PROJECTION_ALLOWED = frozenset(
    {
        "BootProjectionError",
        "LAUNCHER_GUEST_PATH",
        "LAUNCHER_SHA256",
        "LAUNCHER_SIZE_BYTES",
        "normalized_runtime_lock",
    }
)

LAUNCHER_GUEST_PATH = base.LAUNCHER_GUEST_PATH
LAUNCHER_SHA256 = base.LAUNCHER_SHA256
LAUNCHER_SIZE_BYTES = base.LAUNCHER_SIZE_BYTES

LAUNCHER_UNIT_GUEST_PATH = "/usr/lib/systemd/system/boole-native-shadow-launcher.service"
LAUNCHER_UNIT_SOURCE = "native/systemd/boole-native-shadow-launcher-v2.service"
SUPERSEDED_LAUNCHER_UNIT_SOURCE = "native/systemd/boole-native-shadow-launcher.service"
LAUNCHER_UNIT_SHA256 = "4c31bce411c9999b8e877977ce8787d0716a977316ae0a7677240b987181bd55"

# Exactly four, and the correction they came from: the launcher is a root
# supervisor holding these; the answer and the checker it starts are what get
# dropped to the sealed unprivileged account.  This is the subject being fixed,
# not the containment being loosened.
LAUNCHER_BOUNDING_CAPABILITIES = (
    "CAP_SETGID",
    "CAP_SETUID",
    "CAP_SETPCAP",
    "CAP_SYS_ADMIN",
)
# `WantedBy=` in the unit file is what the unit asks for; this link is what
# systemd acts on.  A tree with the unit and without the link holds a launcher
# that is installed and never started, which looks identical to a working image
# until the guest boots in silence.
LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH = (
    "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service"
)
LAUNCHER_UNIT_ENABLEMENT_MODE = 0o777
LAUNCHER_UNIT_REQUIRED = {
    "ExecStart": LAUNCHER_GUEST_PATH,
    "User": "root",
    "Group": "root",
    "AmbientCapabilities": "",
    "StandardOutput": "journal+console",
    "StandardError": "journal+console",
    "WantedBy": "multi-user.target",
}

CONTENT_MANIFEST_GUEST_PATH = staging.NESTED_RUNTIME_TREE["contentManifestGuestPath"]
CONTENT_MANIFEST_SHA256 = staging.NESTED_RUNTIME_TREE["contentManifestSha256"]
CONTENT_MANIFEST_SIZE_BYTES = staging.NESTED_RUNTIME_TREE["contentManifestSizeBytes"]
CONTENT_MANIFEST_MODE = 0o444

# The five files that make the guest able to answer "who is this user".  Without
# them the launcher can drop privilege and then find nothing to resolve, which is
# a runtime failure with no build-time symptom, so each is required by name.
ACCOUNT_DATABASE = (
    {
        "guestPath": "/etc/group",
        "sourcePath": "native/etc/group",
        "mode": 0o444,
        "sha256": "511fb0f6573fd67e0070eab6873655f3f51aaa78b36bacadbd49d30a854f40b2",
    },
    {
        "guestPath": "/etc/gshadow",
        "sourcePath": "native/etc/gshadow",
        "mode": 0o400,
        "sha256": "d32024a972c5341df5574542db6c7f2c8fe595c393b125d6a6c4d59c7dbde06a",
    },
    {
        "guestPath": "/etc/nsswitch.conf",
        "sourcePath": "native/etc/nsswitch.conf",
        "mode": 0o444,
        "sha256": "796450cf1faebb3a577bda80918be349ca1ee9b9bfb9fe7f7cbb7dfbbb177b36",
    },
    {
        "guestPath": "/etc/passwd",
        "sourcePath": "native/etc/passwd",
        "mode": 0o444,
        "sha256": "d7b3ce429ca6ed85a23d1810691719e607fd50e6ce54d3ea6d307829ca66b8ab",
    },
    {
        "guestPath": "/etc/shadow",
        "sourcePath": "native/etc/shadow",
        "mode": 0o400,
        "sha256": "6380bdaffa703d9f96db5876e9f758c888dc4652bb145ce3ac52266141756218",
    },
)

# Frozen from the sealed measurement before this path existed.  A run that walks
# to different numbers fails; it does not adopt them.
EXPECTED_WITHOUT_LAUNCHER = {
    "byKind": {"directory": 1736, "file": 15101, "symlink": 837},
    "caseFoldedSiblings": 20,
    "duplicatePaths": 0,
    "entries": 17674,
    "largestFileBytes": 160096808,
    "largestFilePath": (
        "opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly"
    ),
    "pathCollisions": 0,
    "pathManifestSha256": (
        "a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736"
    ),
    "payloadBytes": 1771449867,
    "symlinkEscapes": 0,
}
EXPECTED_WITH_LAUNCHER = {
    "entries": 17676,
    "largestFileBytes": 160096808,
    "payloadBytes": 1773456499,
}
LIMITS = {
    "maxEntries": 200000,
    "maxFileBytes": 536870912,
    "maxTotalBytes": 2147483648,
}

# The difference between the two totals, one row each, so the two added entries
# are readable as things rather than as an arithmetic step.
PRODUCTION_BOUND_ADDITIONS = (
    {
        "guestPath": os.path.dirname(LAUNCHER_GUEST_PATH),
        "kind": "directory",
        "sizeBytes": 0,
        "why": "the parent the launcher needs, absent from the measured tree",
    },
    {
        "guestPath": LAUNCHER_GUEST_PATH,
        "kind": "file",
        "sizeBytes": LAUNCHER_SIZE_BYTES,
        "why": "the rebuilt launcher, digest-matched against its seal",
    },
)

# Both lists belong to the sealed measurement.  Taking them by reference rather
# than by copy is why this file has no occasion to write an image tool's name.
ALLOWED_REPLAY_TOOLS = measurement.ALLOWED_REPLAY_TOOLS
FORBIDDEN_IN_PREFLIGHT = measurement.FORBIDDEN_EXECUTABLES

# Reusing the historical phase's image steps is what keeps two paths from
# becoming two different disks.  What must stay unreachable is whatever over
# there decides which lock is being built -- computed below from that module's
# own text rather than kept as a list somebody has to remember to update.
HISTORICAL_LOCK_CONSTANT = "BOOT_SOURCE_LOCK_PATH"


class SuccessorProduceError(RuntimeError):
    """An input, a total, or a boundary is not what was pre-registered."""


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _module_source(module: Any) -> str:
    return pathlib.Path(module.__file__).read_text(encoding="utf-8")


def _read_sealed(path: pathlib.Path, expected: str, what: str) -> dict:
    try:
        raw = pathlib.Path(path).read_bytes()
    except OSError as exc:
        raise SuccessorProduceError(f"the {what} is unreadable: {path}") from exc
    digest = _sha256(raw)
    if digest != expected:
        raise SuccessorProduceError(
            f"the {what} hashes to {digest}, the pre-registration says {expected}"
        )
    return json.loads(raw.decode("utf-8"))


def authority(*, path: Optional[pathlib.Path] = None) -> dict:
    """The pre-registration, refused unless it is byte for byte the sealed one."""

    return _read_sealed(
        path or AUTHORITY_PATH, AUTHORITY_SHA256, "successor production authority"
    )


def assert_bound_inputs(document: Mapping[str, Any], repository_root: pathlib.Path) -> int:
    """Every file the pre-registration bound, still at the digest it bound."""

    rows = document["boundInputDigests"]["files"]
    for row in rows:
        path = pathlib.Path(repository_root) / row["path"]
        try:
            digest = _sha256(path.read_bytes())
        except OSError as exc:
            raise SuccessorProduceError(
                f"a bound input is missing: {row['path']}"
            ) from exc
        if digest != row["sha256"]:
            raise SuccessorProduceError(
                f"{row['path']} hashes to {digest}, the pre-registration bound "
                f"{row['sha256']}"
            )
    return len(rows)


def assert_successor_release(lock: Mapping[str, Any]) -> None:
    """The second lock, by its own release string, with activation still off."""

    if lock.get("release") != SOURCE_LOCK_RELEASE:
        raise SuccessorProduceError(
            f"this path builds {SOURCE_LOCK_RELEASE!r} and was given "
            f"{lock.get('release')!r}"
        )
    if lock.get("activationAllowed") is not False:
        raise SuccessorProduceError("a source lock with activation allowed is refused")


def sealed_source_lock(*, path: Optional[pathlib.Path] = None) -> dict:
    """The second boot source lock, digest-checked and release-checked."""

    lock = _read_sealed(
        path or SOURCE_LOCK_PATH, SOURCE_LOCK_SHA256, "successor boot source lock"
    )
    assert_successor_release(lock)
    return lock


def _predecessor_names() -> tuple:
    """The predecessor's release and file name, spelled by substitution.

    Written out as literals they would be the very strings this refuses, and the
    check would fail on itself.  Deriving them from the successor's own names
    keeps the predecessor unnameable here while still being able to refuse it.
    """

    older = SOURCE_LOCK_RELEASE.replace("-V2-", "-V1-")
    filename = SOURCE_LOCK_PATH.name.replace("-v2.", "-v1.")
    return (older, filename)


def assert_no_lock_fallback() -> None:
    """Nothing here can reach the first lock, so nothing can fall back to it."""

    source = _module_source(_this_module())
    for named in _predecessor_names():
        if named in source:
            raise SuccessorProduceError(
                f"this path names the predecessor source lock ({named}), so a "
                "fallback between the two is expressible"
            )


def _this_module() -> Any:
    return sys.modules[__name__]


def assert_module_digest(module: Any, expected: str) -> None:
    """A projection is what it was pinned as, or it is not used."""

    digest = _sha256(pathlib.Path(module.__file__).read_bytes())
    if digest != expected:
        raise SuccessorProduceError(
            f"{pathlib.Path(module.__file__).name} hashes to {digest}, this path "
            f"was pinned to {expected}"
        )


def release_gate() -> Any:
    """The second release gate, which refuses the first lock before any tool."""

    assert_module_digest(gate, RELEASE_GATE_SHA256)
    return gate


def builder() -> Any:
    """The latest staging projection, the one the sealed measurement measured."""

    assert_module_digest(staging, BUILDER_SHA256)
    if staging.SUCCESSOR_PROJECTION_SHA256 != BUILDER_SHA256:
        raise SuccessorProduceError(
            "the staging projection disagrees with its own self-hash"
        )
    return staging


def base_projection() -> Any:
    """The first projection in the chain, for the seal and the normalizer only."""

    assert_base_projection_scope()
    return base


def historical_phase() -> Any:
    """The image steps, shared so that two paths cannot make two disks."""

    assert_historical_phase_scope()
    return historical


def shared_namespace() -> dict:
    """The one mapping both the measurement and this path assemble through."""

    return builder()._IMPL


def _attribute_names(module: Any, alias: str) -> set:
    """Every attribute this file reaches for on a given imported module."""

    tree = ast.parse(_module_source(_this_module()))
    reached = set()
    for node in ast.walk(tree):
        if (
            isinstance(node, ast.Attribute)
            and isinstance(node.value, ast.Name)
            and node.value.id == alias
        ):
            reached.add(node.attr)
    return reached


def assert_base_projection_scope() -> None:
    """The base projection is reached for the seal and the normalizer, or not."""

    reached = _attribute_names(base, "base")
    beyond = sorted(reached - BASE_PROJECTION_ALLOWED)
    if beyond:
        raise SuccessorProduceError(
            "this path reaches the base projection for more than the launcher seal "
            f"and the closure normalizer: {beyond}"
        )


def _historical_phase_refused() -> frozenset:
    """Whatever over there names the historical lock, whatever it is called.

    Derived from that module's text so that a helper added there later which
    reads the first lock becomes unreachable from here without anyone having to
    notice.  Deriving it also keeps those names out of this file.
    """

    tree = ast.parse(_module_source(historical))
    refused = set()
    for node in tree.body:
        if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        names = {
            inner.id for inner in ast.walk(node) if isinstance(inner, ast.Name)
        }
        if HISTORICAL_LOCK_CONSTANT in names:
            refused.add(node.name)
    return frozenset(refused)


HISTORICAL_PHASE_REFUSED = _historical_phase_refused()


def assert_historical_phase_scope() -> None:
    """Reuse its image steps; never reach what chooses the lock it builds."""

    reached = _attribute_names(historical, "historical")
    forbidden = sorted(reached & HISTORICAL_PHASE_REFUSED)
    if forbidden:
        raise SuccessorProduceError(
            "this path reaches into the historical phase for something that reads "
            f"the predecessor lock: {forbidden}"
        )


def assert_shared_assembler(*, namespace: Optional[dict] = None) -> None:
    """One assembler object, reached from both sides -- identity, not equality.

    Two mappings that compare equal today can be edited apart tomorrow, and the
    symptom would be an image that disagrees with a measurement nobody re-ran.
    So this asks whether the production entry point and the measured staging
    function resolve their globals through the same dict object.
    """

    latest = builder()
    if namespace is None:
        namespace = latest._IMPL
    if namespace is not latest._IMPL:
        raise SuccessorProduceError(
            "the assembler namespace given is not the projection's own mapping; a "
            "copy that merely compares equal is refused"
        )
    if latest.materialize_staging_tree.__globals__.get("_IMPL") is not namespace:
        raise SuccessorProduceError(
            "the measured staging function does not resolve through this namespace"
        )
    for name in ("build_oci_layout", "_assemble_entries"):
        function = namespace.get(name)
        if function is None or function.__globals__ is not namespace:
            raise SuccessorProduceError(
                f"{name} does not resolve its globals through this namespace"
            )
    if measurement.builder is not latest:
        raise SuccessorProduceError(
            "the sealed measurement assembles through a different projection than "
            "this path does"
        )


def _staged(entries: Mapping[str, Any], guest_path: str, what: str) -> Mapping[str, Any]:
    """Whatever is staged at that guest path, of whatever kind."""

    entry = entries.get(guest_path.lstrip("/"))
    if entry is None:
        raise SuccessorProduceError(f"the staging tree has no {what}: {guest_path}")
    return entry


def _entry(entries: Mapping[str, Any], guest_path: str, what: str) -> Mapping[str, Any]:
    entry = _staged(entries, guest_path, what)
    if entry.get("kind") != "file":
        raise SuccessorProduceError(
            f"{guest_path} is staged as {entry.get('kind')!r}, and {what} must be a file"
        )
    return entry


def _staged_bytes(entry: Mapping[str, Any], guest_path: str) -> bytes:
    """The content that will actually be written, and nothing standing in for it.

    A staged entry carries its bytes and not a digest of them: the digest is
    computed when the layer is written.  Reading a ``sha256`` key here would
    therefore read ``None`` on every real entry and compare it against a sealed
    value, which is a check that can only ever fail or, if written the other way
    round, one that can never fail.  Hashing the bytes is the only form of this
    question that has an answer.
    """

    raw = entry.get("raw")
    if not isinstance(raw, (bytes, bytearray)):
        raise SuccessorProduceError(f"{guest_path} is staged without its bytes")
    return bytes(raw)


def assert_account_database(entries: Mapping[str, Any]) -> None:
    """All five, at the sealed digests, owned by root, readable as sealed."""

    for row in ACCOUNT_DATABASE:
        entry = _entry(entries, row["guestPath"], "guest account file")
        if entry.get("uid") != 0 or entry.get("gid") != 0:
            raise SuccessorProduceError(
                f"{row['guestPath']} is staged as "
                f"{entry.get('uid')}:{entry.get('gid')} and must be root:root"
            )
        if entry.get("mode") != row["mode"]:
            raise SuccessorProduceError(
                f"{row['guestPath']} is staged {entry.get('mode'):04o}, the lock "
                f"stages it {row['mode']:04o}"
            )
        digest = _sha256(_staged_bytes(entry, row["guestPath"]))
        if digest != row["sha256"]:
            raise SuccessorProduceError(
                f"{row['guestPath']} hashes to {digest}, the lock "
                f"stages {row['sha256']}"
            )


def _unit_directives(text: str) -> dict:
    directives: dict = {}
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith(("#", ";", "[")):
            continue
        key, separator, value = stripped.partition("=")
        if separator:
            directives[key.strip()] = value.strip()
    return directives


def assert_launcher_unit(entries: Mapping[str, Any]) -> None:
    """The successor unit, whose output reaches the console the host collects.

    The predecessor unit writes to the journal only.  A guest that refuses to
    start, or starts and then refuses the work, says so into a journal inside an
    image nobody keeps, and the host records a silent boot.  The console is the
    channel the host already captures and hashes, so readiness and refusal have
    to arrive there.
    """

    entry = _entry(entries, LAUNCHER_UNIT_GUEST_PATH, "launcher unit")
    raw = _staged_bytes(entry, LAUNCHER_UNIT_GUEST_PATH)
    directives = _unit_directives(raw.decode("utf-8"))
    for key, expected in LAUNCHER_UNIT_REQUIRED.items():
        actual = directives.get(key)
        if actual != expected:
            raise SuccessorProduceError(
                f"the launcher unit sets {key}={actual!r}; this path requires "
                f"{key}={expected!r}"
            )
    bounding = tuple(directives.get("CapabilityBoundingSet", "").split())
    if bounding != LAUNCHER_BOUNDING_CAPABILITIES:
        raise SuccessorProduceError(
            f"the launcher unit bounds {list(bounding)}; the correction says exactly "
            f"{list(LAUNCHER_BOUNDING_CAPABILITIES)}"
        )
    digest = _sha256(raw)
    if digest != LAUNCHER_UNIT_SHA256:
        raise SuccessorProduceError(
            f"the launcher unit hashes to {digest}, the lock stages "
            f"{LAUNCHER_UNIT_SHA256}"
        )
    assert_launcher_enabled(entries)


def assert_launcher_enabled(entries: Mapping[str, Any]) -> None:
    """The wants link, without which the unit is installed and never started.

    The authority asks for the unit to be present *and enabled*, and those are
    two staged entries rather than one.  Checking `WantedBy=` proves only that
    the unit would like to be enabled; systemd reads this directory.
    """

    link = _staged(entries, LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH, "launcher enablement")
    if link.get("kind") != "symlink":
        raise SuccessorProduceError(
            f"{LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH} is staged as "
            f"{link.get('kind')!r}; enablement is a symlink, and a copy of the "
            f"unit here would start whatever that copy says"
        )
    if link.get("target") != LAUNCHER_UNIT_GUEST_PATH:
        raise SuccessorProduceError(
            f"{LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH} points at "
            f"{link.get('target')!r} and must point at {LAUNCHER_UNIT_GUEST_PATH!r}"
        )
    if link.get("uid") != 0 or link.get("gid") != 0:
        raise SuccessorProduceError(
            f"{LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH} is staged as "
            f"{link.get('uid')}:{link.get('gid')} and must be root:root"
        )
    if link.get("mode") != LAUNCHER_UNIT_ENABLEMENT_MODE:
        raise SuccessorProduceError(
            f"{LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH} is staged "
            f"{link.get('mode'):04o}, the lock stages "
            f"{LAUNCHER_UNIT_ENABLEMENT_MODE:04o}"
        )


def assert_content_manifest(entries: Mapping[str, Any]) -> None:
    """The manifest the launcher replays against, at the sealed digest and size."""

    entry = _entry(entries, CONTENT_MANIFEST_GUEST_PATH, "runtime content manifest")
    raw = _staged_bytes(entry, CONTENT_MANIFEST_GUEST_PATH)
    digest = _sha256(raw)
    if digest != CONTENT_MANIFEST_SHA256:
        raise SuccessorProduceError(
            f"the runtime content manifest hashes to {digest}, the "
            f"replay expectation seals {CONTENT_MANIFEST_SHA256}"
        )
    if len(raw) != CONTENT_MANIFEST_SIZE_BYTES:
        raise SuccessorProduceError(
            f"the runtime content manifest is {len(raw)} bytes, the "
            f"replay expectation seals {CONTENT_MANIFEST_SIZE_BYTES}"
        )
    if entry.get("mode") != CONTENT_MANIFEST_MODE:
        raise SuccessorProduceError(
            f"the runtime content manifest is staged {entry.get('mode'):04o} and "
            f"must be {CONTENT_MANIFEST_MODE:04o}"
        )


def _written(destination: pathlib.Path, guest_path: str, what: str) -> pathlib.Path:
    """The path in the tree that was actually written, present in any form."""

    path = pathlib.Path(destination) / guest_path.lstrip("/")
    if not path.is_symlink() and not path.exists():
        raise SuccessorProduceError(
            f"the written staging tree has no {what}: {guest_path}"
        )
    return path


def _written_file(destination: pathlib.Path, guest_path: str, what: str) -> tuple:
    path = _written(destination, guest_path, what)
    if path.is_symlink() or not path.is_file():
        raise SuccessorProduceError(
            f"{guest_path} was written as something other than a regular file, and "
            f"{what} must be a file"
        )
    return path.read_bytes(), stat.S_IMODE(path.lstat().st_mode)


def gap_evidence(entries: Mapping[str, Any], destination: pathlib.Path) -> dict:
    """The three closed gaps, read back off the tree the writer produced.

    ``assert_account_database``, ``assert_launcher_unit`` and
    ``assert_content_manifest`` read the entry table, which is what the writer was
    asked for.  This reads what it did, off the tree the image would be made from,
    and records what it found so the sealed result carries the evidence rather
    than a claim that the evidence was seen.

    Ownership is deliberately not read from disk.  A preflight that is not root
    cannot reproduce it, so a uid read here would be whoever ran the preflight.
    The owner each entry carries into the image is the one the image writer copies
    from the table, so that is the one recorded.
    """

    accounts = []
    for row in ACCOUNT_DATABASE:
        raw, mode = _written_file(destination, row["guestPath"], "guest account file")
        digest = _sha256(raw)
        if digest != row["sha256"]:
            raise SuccessorProduceError(
                f"the written {row['guestPath']} hashes to {digest}, the lock "
                f"stages {row['sha256']}"
            )
        if mode != row["mode"]:
            raise SuccessorProduceError(
                f"the written {row['guestPath']} is {mode:04o}, the lock stages "
                f"{row['mode']:04o}"
            )
        staged = _entry(entries, row["guestPath"], "guest account file")
        accounts.append(
            {
                "gid": staged["gid"],
                "guestPath": row["guestPath"],
                "mode": f"{mode:04o}",
                "sha256": digest,
                "sizeBytes": len(raw),
                "uid": staged["uid"],
            }
        )

    raw, unit_mode = _written_file(destination, LAUNCHER_UNIT_GUEST_PATH, "launcher unit")
    unit_digest = _sha256(raw)
    if unit_digest != LAUNCHER_UNIT_SHA256:
        raise SuccessorProduceError(
            f"the written launcher unit hashes to {unit_digest}, the lock stages "
            f"{LAUNCHER_UNIT_SHA256}"
        )
    directives = _unit_directives(raw.decode("utf-8"))
    for key, expected in LAUNCHER_UNIT_REQUIRED.items():
        if directives.get(key) != expected:
            raise SuccessorProduceError(
                f"the written launcher unit sets {key}={directives.get(key)!r}; this "
                f"path requires {key}={expected!r}"
            )
    bounding = tuple(directives.get("CapabilityBoundingSet", "").split())
    if bounding != LAUNCHER_BOUNDING_CAPABILITIES:
        raise SuccessorProduceError(
            f"the written launcher unit bounds {list(bounding)}; the correction says "
            f"exactly {list(LAUNCHER_BOUNDING_CAPABILITIES)}"
        )

    link = _written(destination, LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH, "launcher enablement")
    if not link.is_symlink():
        raise SuccessorProduceError(
            f"the written {LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH} is not a symlink, so "
            f"the unit is installed and never started"
        )
    target = os.readlink(str(link))
    if target != LAUNCHER_UNIT_GUEST_PATH:
        raise SuccessorProduceError(
            f"the written enablement link points at {target!r} and must point at "
            f"{LAUNCHER_UNIT_GUEST_PATH!r}"
        )
    # The link's own mode is the host's answer, not the tree's: a symlink is 0777
    # on Linux and whatever the extractor chose elsewhere.  The mode that reaches
    # the image is the staged one, so the staged one is what is recorded.
    staged_link = _staged(
        entries, LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH, "launcher enablement"
    )

    raw, manifest_mode = _written_file(
        destination, CONTENT_MANIFEST_GUEST_PATH, "runtime content manifest"
    )
    manifest_digest = _sha256(raw)
    if manifest_digest != CONTENT_MANIFEST_SHA256:
        raise SuccessorProduceError(
            f"the written runtime content manifest hashes to {manifest_digest}, the "
            f"replay expectation seals {CONTENT_MANIFEST_SHA256}"
        )
    if len(raw) != CONTENT_MANIFEST_SIZE_BYTES:
        raise SuccessorProduceError(
            f"the written runtime content manifest is {len(raw)} bytes, the replay "
            f"expectation seals {CONTENT_MANIFEST_SIZE_BYTES}"
        )
    if manifest_mode != CONTENT_MANIFEST_MODE:
        raise SuccessorProduceError(
            f"the written runtime content manifest is {manifest_mode:04o} and must "
            f"be {CONTENT_MANIFEST_MODE:04o}"
        )

    return {
        "accountDatabase": accounts,
        "launcherUnit": {
            "capabilityBoundingSet": list(LAUNCHER_BOUNDING_CAPABILITIES),
            "directives": {key: directives[key] for key in LAUNCHER_UNIT_REQUIRED},
            "enablement": {
                "gid": staged_link["gid"],
                "guestPath": LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH,
                "kind": "symlink",
                "stagedMode": f"{staged_link['mode']:04o}",
                "target": target,
                "uid": staged_link["uid"],
            },
            "guestPath": LAUNCHER_UNIT_GUEST_PATH,
            "mode": f"{unit_mode:04o}",
            "sha256": unit_digest,
            "source": LAUNCHER_UNIT_SOURCE,
        },
        "runtimeContentManifest": {
            "guestPath": CONTENT_MANIFEST_GUEST_PATH,
            "mode": f"{manifest_mode:04o}",
            "sha256": manifest_digest,
            "sizeBytes": len(raw),
        },
    }


def assert_launcher_binary(raw: bytes) -> None:
    """The rebuilt launcher answers to the seal, by size first and then digest."""

    if len(raw) != LAUNCHER_SIZE_BYTES:
        raise SuccessorProduceError(
            f"the rebuilt launcher is {len(raw)} bytes, the seal says "
            f"{LAUNCHER_SIZE_BYTES}"
        )
    digest = _sha256(raw)
    if digest != LAUNCHER_SHA256:
        raise SuccessorProduceError(
            f"the rebuilt launcher hashes to {digest}, the seal says {LAUNCHER_SHA256}"
        )


def assert_artifact_store(artifact_store: pathlib.Path) -> pathlib.Path:
    """The store is there and is a real directory, before anything reaches it.

    The builder says the same thing further in, in its own words and its own
    exception.  Saying it here is about which run is being described: a store
    that was never populated is a fact about this host, and a reader of the
    refusal should not have to work that out from a message about source locks.
    """

    artifact_store = pathlib.Path(artifact_store)
    if artifact_store.is_symlink() or not artifact_store.is_dir():
        raise SuccessorProduceError(
            f"the artifact store is not an existing real directory: {artifact_store}"
        )
    return artifact_store


def sealed_measurement(*, path: Optional[pathlib.Path] = None) -> dict:
    """The staging measurement this path's totals were frozen from."""

    return _read_sealed(
        path or MEASUREMENT_PATH, MEASUREMENT_SHA256, "sealed staging measurement"
    )


def assert_totals(walked: Mapping[str, Any], complete: Mapping[str, Any]) -> None:
    """The walk reaches the frozen numbers, or the run fails without adopting."""

    try:
        measurement.assert_measurements_agree(dict(EXPECTED_WITHOUT_LAUNCHER), walked)
    except measurement.StagingMeasurementError as exc:
        raise SuccessorProduceError(
            f"the staging tree disagrees with the sealed measurement: {exc}"
        ) from exc
    for key, expected in EXPECTED_WITH_LAUNCHER.items():
        if complete.get(key) != expected:
            raise SuccessorProduceError(
                f"the production-bound projection reaches {key}={complete.get(key)!r}, "
                f"the sealed measurement projects {expected!r}"
            )


def assert_no_conflicts(walked: Mapping[str, Any]) -> None:
    """One collision, one duplicate or one escape is a refusal, not a note."""

    for key in ("pathCollisions", "duplicatePaths", "symlinkEscapes"):
        found = walked.get(key)
        if found:
            raise SuccessorProduceError(
                f"the staging tree has {found} {key}; the sealed measurement has none"
            )


def assert_within_limits(limits: Mapping[str, Any], totals: Mapping[str, Any]) -> None:
    """The sealed recipe's three numbers, applied to the larger projection."""

    try:
        measurement.assert_within_limits(dict(limits), dict(totals))
    except measurement.StagingMeasurementError as exc:
        raise SuccessorProduceError(str(exc)) from exc


def assert_preflight_tool(path: pathlib.Path) -> pathlib.Path:
    """Default deny: the two replay tools, and nothing else, whatever it is."""

    try:
        return measurement.assert_replay_tool(pathlib.Path(path))
    except measurement.StagingMeasurementError as exc:
        raise SuccessorProduceError(str(exc)) from exc


def _local_call_graph(root: str) -> set:
    """Function names in this file reachable from one entry point."""

    tree = ast.parse(_module_source(_this_module()))
    defined = {
        node.name: node
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    }
    seen: set = set()
    pending = [root]
    while pending:
        name = pending.pop()
        if name in seen or name not in defined:
            continue
        seen.add(name)
        for node in ast.walk(defined[name]):
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
                pending.append(node.func.id)
    return seen


def _reached_modules(names: Iterable[str]) -> set:
    """Every module alias the given local functions reach for an attribute on."""

    tree = ast.parse(_module_source(_this_module()))
    defined = {
        node.name: node
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    }
    aliases = set()
    for name in names:
        node = defined.get(name)
        if node is None:
            continue
        for inner in ast.walk(node):
            if isinstance(inner, ast.Attribute) and isinstance(inner.value, ast.Name):
                aliases.add(inner.value.id)
    return aliases


IMAGE_STEP_ALIASES = frozenset(
    {
        "historical",
        "image_verify",
        "initrd",
        "kernel_extract",
        "producer",
        "root_disk",
        "root_disk_execute",
        "writer_tree_module",
    }
)


def assert_preflight_creates_no_outputs() -> None:
    """The no-output run cannot reach an image step, so it cannot make one.

    Not a promise to be careful: from ``preflight`` there is no path through this
    file to the kernel extractor, the initrd builder, the root disk writer, the
    output manifest, or the historical phase's image helpers.  A future edit that
    added one would fail here rather than in a directory that costs an attempt.
    """

    reachable = _local_call_graph("preflight")
    if "produce" in reachable:
        raise SuccessorProduceError("the preflight can reach the production entry point")
    if "write_consumed_marker" in reachable:
        raise SuccessorProduceError(
            "the preflight can reach the consumed-attempt marker, which is the "
            "budget line itself"
        )
    if "consumed_attempt" in reachable:
        raise SuccessorProduceError(
            "the preflight can enter the spent section, whose first act is to "
            "write the marker"
        )
    reached = _reached_modules(reachable) & IMAGE_STEP_ALIASES
    if reached:
        raise SuccessorProduceError(
            f"the preflight can reach image production steps: {sorted(reached)}"
        )


def assert_single_subprocess_gateway() -> None:
    """No process is started from this file at all; execution stays with the gate.

    The replay tools are run by the release gate, which is pinned by digest, and
    the image tools are run by the image steps, which the preflight cannot reach.
    A direct call from here would be a third way to run something, outside both.
    """

    tree = ast.parse(_module_source(_this_module()))
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        function = node.func
        if isinstance(function, ast.Attribute) and isinstance(function.value, ast.Name):
            if function.value.id in {"subprocess", "os"} and function.attr in {
                "run",
                "call",
                "check_call",
                "check_output",
                "Popen",
                "system",
                "execv",
                "spawnv",
                "popen",
            }:
                raise SuccessorProduceError(
                    f"this path starts a process directly: {function.value.id}."
                    f"{function.attr}"
                )


# Read for everyone, written by nobody else.  The phase writes as root inside
# the transient unit and every step that collects what it wrote runs as the
# ordinary runner account.
COLLECTABLE_FILE_MODE = 0o444

CONSUMED_MARKER_NAME = "ATTEMPT-CONSUMED.json"

CONSUMED_MARKER_RULE = (
    "From this file onward the one allowed production attempt is consumed, "
    "whatever happens next.  A failure after this point is not retried; it is "
    "reported to the operator as a hard stop."
)


def consumed_marker(outputs) -> pathlib.Path:
    """The one name that answers the budget question."""

    return pathlib.Path(outputs) / CONSUMED_MARKER_NAME


def write_consumed_marker(outputs) -> pathlib.Path:
    """Say the attempt is spent, on the disk and on the console, just before it is.

    Written atomically, because the whole point of a boundary is that a run
    which dies halfway lands on one side of it.  The document is built in full,
    flushed to a neighbouring name that is not the marker, fsynced, and only
    then renamed into place; a crash anywhere before the rename leaves the
    marker absent and the attempt unspent, which is the honest reading of a run
    that never reached its first image file.

    Echoed to stdout as well, because the disk it is written to belongs to a
    runner that is about to be destroyed and the console is what the host
    already collects.
    """

    outputs = pathlib.Path(outputs)
    marker = consumed_marker(outputs)
    if marker.exists():
        raise SuccessorProduceError(
            f"a consumed-attempt marker is already here and is not replaced: {marker}"
        )

    payload = {
        "attemptId": authority()["attemptId"],
        "authoritySha256": AUTHORITY_SHA256,
        "consumed": True,
        "outputNames": list(historical_phase().output_names()),
        "release": RELEASE,
        "rule": CONSUMED_MARKER_RULE,
        "schema": (
            "boole.native-shadow.mac3-successor-image-production-attempt-consumed.v1"
        ),
        "writtenBefore": "the first image output file",
    }
    raw = (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")

    handle = tempfile.NamedTemporaryFile(
        dir=str(outputs), prefix=".attempt-consumed-partial.", delete=False
    )
    partial = pathlib.Path(handle.name)
    try:
        with handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        # Readable before the rename rather than after it, so the name that
        # answers the budget question is readable from the instant it exists.
        # A temporary file is created at mode 0600 and a rename keeps that, and
        # the account that collects this off the runner is not the root account
        # that wrote it: the first attempt's marker could not be uploaded at
        # all, and the only copy that survived was the console echo below.
        os.chmod(str(partial), COLLECTABLE_FILE_MODE)
        os.replace(str(partial), str(marker))
    except BaseException:
        if partial.exists():
            partial.unlink()
        raise

    # Past the rename the marker exists, so the attempt is spent and the run is
    # committed.  What is left is durability across a power cut and evidence for
    # a host reading the console -- both worth doing, neither worth aborting a
    # run for.  Raising here would spend the one attempt on a failure in the
    # part that only records the spending, so both say so and continue.
    try:
        directory = os.open(str(outputs), os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    except OSError as exc:
        sys.stderr.write(f"{CONSUMED_MARKER_NAME}: directory fsync failed: {exc}\n")

    try:
        sys.stdout.write(f"{CONSUMED_MARKER_NAME}\n{raw.decode('utf-8')}")
        sys.stdout.flush()
    except OSError as exc:
        sys.stderr.write(f"{CONSUMED_MARKER_NAME}: console echo failed: {exc}\n")
    return marker


def attempt_consumed(*, marker_written: bool) -> bool:
    """Where the budget line falls, in one place, and now on a deliberate act.

    It used to be the output directory.  The first production found the case
    that sits between the sealed rule's two sentences: the directory existed,
    because a systemd ``ReadWritePaths`` entry has to exist before the unit
    starts, and no output file was ever written.  The operator settled that case
    as unspent and asked for a boundary with no such gap in it.

    So the answer is a file this phase writes on purpose, immediately before the
    first image file, rather than a directory the isolation needed in order to
    run at all.  Before the marker nothing is spent; after it the attempt is
    spent whatever happens next.
    """

    return bool(marker_written)


UNQUALIFIED_MARKER_NAME = "UNQUALIFIED-DIAGNOSTIC.json"

UNQUALIFIED_MARKER_RULE = (
    "These files were left by a production attempt that did not finish.  They "
    "are kept as diagnostic material and as nothing else: they are not a "
    "qualified image, they are not adopted, they are not booted, and no digest "
    "taken from them is a production digest."
)


def unqualified_marker(outputs) -> pathlib.Path:
    """The one name that says a kept file is not a produced image."""

    return pathlib.Path(outputs) / UNQUALIFIED_MARKER_NAME


def write_unqualified_diagnostic(outputs, failure: BaseException) -> pathlib.Path:
    """Keep what a failed attempt left, under a document that disowns it.

    The first attempt built all three files, passed the content check, raised
    one statement later, and was destroyed with the runner, because the steps
    that keep the outputs ran only when every step before them had passed.  An
    attempt that produces a good image and loses it is the worst outcome
    available to a budget of one.

    Keeping is not adopting.  A run that failed after the marker produced
    something whose qualification was never established, so what is kept says
    so, in the same directory, next to the files it is about.
    """

    outputs = pathlib.Path(outputs)
    marker = unqualified_marker(outputs)
    reserved = {CONSUMED_MARKER_NAME, UNQUALIFIED_MARKER_NAME}
    kept = sorted(
        path.name
        for path in outputs.iterdir()
        if path.is_file() and path.name not in reserved and not path.name.startswith(".")
    )
    payload = {
        "attemptConsumed": True,
        "attemptId": authority()["attemptId"],
        "authoritySha256": AUTHORITY_SHA256,
        "failure": f"{type(failure).__name__}: {failure}",
        "filesKept": kept,
        "mayBeAdopted": False,
        "mayBeBooted": False,
        "qualifiedImage": False,
        "release": RELEASE,
        "rule": UNQUALIFIED_MARKER_RULE,
        "schema": (
            "boole.native-shadow.mac3-successor-image-production-unqualified"
            "-diagnostic.v1"
        ),
        "status": "UNQUALIFIED-DIAGNOSTIC",
    }
    raw = (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")
    handle = tempfile.NamedTemporaryFile(
        dir=str(outputs), prefix=".unqualified-diagnostic-partial.", delete=False
    )
    partial = pathlib.Path(handle.name)
    try:
        with handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(str(partial), COLLECTABLE_FILE_MODE)
        os.replace(str(partial), str(marker))
    except BaseException:
        if partial.exists():
            partial.unlink()
        raise
    try:
        sys.stdout.write(f"{UNQUALIFIED_MARKER_NAME}\n{raw.decode('utf-8')}")
        sys.stdout.flush()
    except OSError as exc:
        sys.stderr.write(f"{UNQUALIFIED_MARKER_NAME}: console echo failed: {exc}\n")
    return marker


def make_outputs_readable(outputs) -> None:
    """Grant read to whoever collects this, and take nothing away.

    The phase runs as root inside the transient unit and the step that uploads
    runs as the ordinary runner account.  Everything here is additive: a mode
    only ever gains the read bits, so this cannot be the thing that made a file
    unusable to the step that wrote it.
    """

    outputs = pathlib.Path(outputs)
    for path in [outputs, *sorted(outputs.rglob("*"))]:
        try:
            current = path.lstat()
        except OSError:
            continue
        if stat.S_ISLNK(current.st_mode):
            continue
        mode = stat.S_IMODE(current.st_mode)
        wanted = mode | (0o055 if stat.S_ISDIR(current.st_mode) else 0o044)
        if wanted == mode:
            continue
        try:
            os.chmod(str(path), wanted)
        except OSError as exc:
            sys.stderr.write(f"{path.name}: could not be made collectable: {exc}\n")


@contextlib.contextmanager
def consumed_attempt(outputs):
    """The section that spends the attempt: marked before it, kept after it.

    Entering writes the marker, so everything inside is on the spent side of the
    budget line by construction rather than by the order somebody remembered to
    write the statements in.  Leaving it badly writes the diagnostic and leaves
    what was produced where it is, readable, disowned, and available to the
    operator who has to decide what happened.

    The diagnostic is written on the way out and never instead of the failure:
    if it cannot be written, the failure that caused it is still what comes out
    of here, and the complaint goes to the console.
    """

    outputs = pathlib.Path(outputs)
    write_consumed_marker(outputs)
    try:
        yield
    except BaseException as failure:
        try:
            write_unqualified_diagnostic(outputs, failure)
        except BaseException as second:
            sys.stderr.write(
                f"{UNQUALIFIED_MARKER_NAME}: could not be written: {second}\n"
            )
        raise
    finally:
        make_outputs_readable(outputs)


def assert_attempt_available(*, runs_performed: int) -> None:
    """One dispatch, and never a second after an output file has existed."""

    if runs_performed != 0:
        raise SuccessorProduceError(
            f"the successor production attempt is already spent "
            f"({runs_performed} performed, 1 allowed); a retry is refused"
        )


def successor_closure(gpgv: pathlib.Path, zstd: pathlib.Path) -> tuple:
    """The successor lock with replay-local tools in it, through its own gate.

    The gate settles the identity question first: a lock whose release string or
    schema is not the successor's is refused before either tool path is opened,
    which is why feeding this the predecessor's lock costs nothing.
    """

    sealed_raw = SOURCE_LOCK_PATH.read_bytes()
    sealed = sealed_source_lock()
    for tool in (gpgv, zstd):
        assert_preflight_tool(tool)
    try:
        runtime, _receipt = release_gate().materialize_runtime_lock(
            sealed, sealed_raw, pathlib.Path(gpgv), pathlib.Path(zstd)
        )
        normalized, normalized_raw, _record = base_projection().normalized_runtime_lock(
            runtime
        )
    except (gate.PortableAuthorityError, base.BootProjectionError) as exc:
        raise SuccessorProduceError(str(exc)) from exc
    return normalized, normalized_raw


def assert_recipe_limits(lock: Mapping[str, Any]) -> dict:
    """The recipe's own three numbers, and this file's copy, agreeing."""

    recipe = lock["buildRecipe"]
    for key, expected in LIMITS.items():
        if recipe.get(key) != expected:
            raise SuccessorProduceError(
                f"the successor lock's recipe sets {key}={recipe.get(key)!r}, this "
                f"path was pre-registered against {expected!r}"
            )
    return recipe


def _assemble(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    nested_tree: Mapping[str, Any],
    content_manifest_sha256: str,
    launcher_binary: Optional[bytes],
) -> tuple:
    """The one assembly both the no-output run and the production go through."""

    if content_manifest_sha256 != CONTENT_MANIFEST_SHA256:
        raise SuccessorProduceError(
            f"the caller expects a runtime content manifest of "
            f"{content_manifest_sha256}, this path builds {CONTENT_MANIFEST_SHA256}"
        )
    assert_shared_assembler()
    latest = builder()
    lock, lock_raw = successor_closure(gpgv, zstd)
    recipe = assert_recipe_limits(lock)
    validated = latest.validate_source_lock(
        lock, lock_raw, repository_root, artifact_store, require_complete=True
    )
    entries = latest.materialize_staging_tree(
        validated,
        repository_root,
        artifact_store,
        launcher_binary=launcher_binary,
        nested_tree=nested_tree,
    )
    assert_account_database(entries)
    assert_launcher_unit(entries)
    assert_content_manifest(entries)
    return lock, lock_raw, recipe, entries


# Every module this path reads code out of.  Two of them are pinned by constant
# above; the rest are recorded so that a result can be traced to the exact text
# that produced it, which is the only form of "which build was this" that
# survives a projection chain.
PROVENANCE_MODULES = (base, gate, historical, measurement, staging)


def provenance(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> dict:
    """Where every input came from, hashed here rather than asserted.

    Each digest is recomputed from the file it names.  The two pinned modules
    were already checked against their constants before anything ran, so this
    records what was read; for the rest, this is the record.
    """

    modules = {
        module.__name__: _sha256(pathlib.Path(module.__file__).read_bytes())
        for module in PROVENANCE_MODULES
    }
    modules[__name__] = _sha256(pathlib.Path(__file__).read_bytes())
    host = os.uname()
    return {
        "artifactStore": str(artifact_store),
        "authoritySha256": AUTHORITY_SHA256,
        "launcherBuildResultSha256": _sha256(LAUNCHER_BUILD_RESULT_PATH.read_bytes()),
        "measurementSha256": MEASUREMENT_SHA256,
        "modules": modules,
        "platform": {
            "machine": host.machine,
            "python": sys.version.split()[0],
            "release": host.release,
            "system": host.sysname,
        },
        "repositoryRoot": str(repository_root),
        "sourceLockSha256": SOURCE_LOCK_SHA256,
        "tools": {"gpgv": str(gpgv), "zstd": str(zstd)},
    }


def preflight(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Any],
    content_manifest_sha256: str,
) -> dict:
    """Everything the production does except the part that costs the attempt."""

    assert_preflight_creates_no_outputs()
    assert_single_subprocess_gateway()
    assert_no_lock_fallback()
    document = authority()
    assert_bound_inputs(document, repository_root)
    assert_attempt_available(runs_performed=document["runsPerformed"])
    assert_artifact_store(artifact_store)
    assert_launcher_binary(launcher_binary)

    _lock, _lock_raw, recipe, entries = _assemble(
        repository_root=repository_root,
        artifact_store=artifact_store,
        gpgv=gpgv,
        zstd=zstd,
        nested_tree=nested_tree,
        content_manifest_sha256=content_manifest_sha256,
        launcher_binary=None,
    )

    computed = measurement.builder_totals(entries)
    destination = pathlib.Path(scratch) / "staging"
    destination.parent.mkdir(parents=True, exist_ok=True)
    measurement.assert_case_sensitive(destination.parent)
    measurement.write_staging_tree(entries, destination, recipe["canonicalMtime"])
    walked = measurement.traverse_staging_tree(destination)
    assert_totals(computed, _with_launcher(computed))
    assert_totals(walked, _with_launcher(walked))
    assert_no_conflicts(walked)
    assert_within_limits(LIMITS, _with_launcher(walked))
    nested_on_disk = measurement.nested_manifest_on_disk(destination)
    gaps = gap_evidence(entries, destination)

    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "authoritySha256": AUTHORITY_SHA256,
        "bootableClaim": BOOTABLE_CLAIM,
        "builderInternal": computed,
        "gapEvidence": gaps,
        "imageProducedClaim": IMAGE_PRODUCED_CLAIM,
        "independentTraversal": walked,
        "launcher": {
            "guestPath": LAUNCHER_GUEST_PATH,
            "includedInTheMeasuredTree": False,
            "rebuiltSha256": _sha256(launcher_binary),
            "sealedSha256": LAUNCHER_SHA256,
            "sealedSizeBytes": LAUNCHER_SIZE_BYTES,
        },
        "limits": dict(LIMITS),
        "nestedContentManifest": nested_on_disk,
        "outputsCreated": False,
        "productionBoundAdditions": [dict(row) for row in PRODUCTION_BOUND_ADDITIONS],
        "provenance": provenance(
            repository_root=repository_root,
            artifact_store=artifact_store,
            gpgv=gpgv,
            zstd=zstd,
        ),
        "release": RELEASE,
        "servingClaim": SERVING_CLAIM,
        "sourceLockSha256": SOURCE_LOCK_SHA256,
        "withSealedLauncher": _with_launcher(walked),
    }


def _with_launcher(totals: Mapping[str, Any]) -> dict:
    """The measured tree plus the one file this host cannot hold, and its parent."""

    return {
        "entries": totals["entries"] + len(PRODUCTION_BOUND_ADDITIONS),
        "largestFileBytes": max(totals["largestFileBytes"], LAUNCHER_SIZE_BYTES),
        "payloadBytes": totals["payloadBytes"] + LAUNCHER_SIZE_BYTES,
    }


def _is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def production_result(
    *,
    manifest_entries: Mapping[str, str],
    output_names: Iterable[str],
    build_receipt: Mapping[str, Any],
    builder_internal: Mapping[str, Any],
    kernel_sha256: str,
    kernel_disposition: Any,
    root_disk: Any,
    root_disk_evidence: Any,
    verify_report: Mapping[str, Any],
) -> dict:
    """What the run produced, said once, in a function a free test can run.

    This assembly is where the first attempt died.  It had never been executed:
    the section it sits in needs root, aarch64, a payload store and the one
    attempt there is, so nothing but the attempt itself could reach it.  The
    field it raised on -- ``outputManifest`` -- appears nowhere else in the
    repository, so no consumer would have noticed the shape either.  What it
    raised on was a type: ``manifest_from_directory`` returns a mapping of
    output name to digest, iterating a mapping yields its keys, and each key was
    handed to ``dict`` as though it were a row.

    So it takes plain values and returns a document, which is a thing a test
    with three files of a few bytes in it can run for nothing.  The manifest is
    checked rather than trusted, because a document that quietly dropped an
    output would read like a smaller production instead of a broken one.
    """

    names = tuple(output_names)
    if not isinstance(manifest_entries, Mapping):
        raise SuccessorProduceError(
            "the output manifest is not a mapping of output name to digest: "
            f"{type(manifest_entries).__name__}"
        )
    missing = [name for name in names if name not in manifest_entries]
    if missing:
        raise SuccessorProduceError(
            "the output manifest is missing an output this phase produces: "
            + ", ".join(missing)
        )
    extra = sorted(set(manifest_entries) - set(names))
    if extra:
        raise SuccessorProduceError(
            "the output manifest carries what this phase does not produce: "
            + ", ".join(extra)
        )
    rows = []
    for name in names:
        digest = manifest_entries[name]
        if not _is_sha256(digest):
            raise SuccessorProduceError(
                f"the output manifest has no sha256 digest for {name}: {digest!r}"
            )
        rows.append({"name": name, "sha256": digest})

    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "authoritySha256": AUTHORITY_SHA256,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": {
            "guestBootVerified": GUEST_BOOT_VERIFIED,
            "guestImageBuilt": True,
            "runtimeCompatibilityVerified": False,
        },
        "buildReceipt": build_receipt,
        "builderInternal": builder_internal,
        "kernel": {"disposition": kernel_disposition, "sha256": kernel_sha256},
        "outputManifest": rows,
        "outputsCreated": True,
        "release": RELEASE,
        "rootDisk": root_disk,
        "rootDiskEvidence": root_disk_evidence,
        "servingClaim": SERVING_CLAIM,
        "sourceLockSha256": SOURCE_LOCK_SHA256,
        "verifyReport": verify_report,
    }


def produce(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher_binary: bytes,
    nested_tree: Mapping[str, Any],
    content_manifest_sha256: str,
) -> dict:
    """The three boot files, from the successor closure, offline, once."""

    if os.geteuid() != 0:
        raise SuccessorProduceError(
            "the produce phase must run as root: the image writer copies the staged "
            "owner into the image, and the frozen plan says root:root throughout"
        )
    phase = historical_phase()
    phase.assert_production_unblocked()
    assert_no_lock_fallback()
    document = authority()
    assert_bound_inputs(document, repository_root)
    assert_attempt_available(runs_performed=document["runsPerformed"])
    assert_artifact_store(artifact_store)
    assert_launcher_binary(launcher_binary)

    oci = pathlib.Path(scratch) / "oci"
    tree = pathlib.Path(scratch) / "tree"
    lock, lock_raw, recipe, entries = _assemble(
        repository_root=repository_root,
        artifact_store=artifact_store,
        gpgv=gpgv,
        zstd=zstd,
        nested_tree=nested_tree,
        content_manifest_sha256=content_manifest_sha256,
        launcher_binary=launcher_binary,
    )
    computed = measurement.builder_totals(entries)
    assert_no_conflicts(computed)
    assert_within_limits(recipe, computed)
    if computed["entries"] != EXPECTED_WITH_LAUNCHER["entries"]:
        raise SuccessorProduceError(
            f"the production-bound tree holds {computed['entries']} entries, the "
            f"sealed measurement projects {EXPECTED_WITH_LAUNCHER['entries']}"
        )

    # The output directory is made here because the three paths are needed, not
    # because the budget turns on it.  The first production proved that a
    # directory is something the isolation requires in order to start at all --
    # a `ReadWritePaths` entry has to exist before the unit does -- so it cannot
    # also be the thing that says an attempt was spent.
    outputs = pathlib.Path(outputs)
    outputs.mkdir(parents=True, exist_ok=True)
    produced = phase.output_paths(outputs)

    build_receipt = shared_namespace()["build_oci_layout"](
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        oci,
        launcher_binary=launcher_binary,
        nested_tree=nested_tree,
    )
    layer = phase.layer_bytes(oci, build_receipt)
    phase._extract_tree(layer, tree)

    # The budget line, and it is an act rather than a side effect.  Everything
    # above it is refusable for free, including the layout build and the tree
    # extraction just above, which write into the scratch and never into the
    # outputs.  Entering the section below writes the marker, so from here on
    # the attempt is consumed whatever happens next -- and leaving it badly
    # leaves what was produced where it is, readable and disowned, rather than
    # discarding it with the runner.
    with consumed_attempt(outputs):
        kernel_result, kernel_disposition = kernel_extract.extract(
            cas_roots=[artifact_store],
            zstd_path=pathlib.Path(zstd),
            out_dir=outputs,
            result_path=pathlib.Path(scratch) / "kernel-extract-result.json",
        )
        initrd_raw = initrd.initrd_bytes(layer)
        produced["initrd"].write_bytes(initrd_raw)

        writer_tree = pathlib.Path(scratch) / "writer"
        try:
            writer_receipt = writer_tree_module.materialize(
                cas_roots=[artifact_store],
                zstd=pathlib.Path(zstd),
                writer_tree=writer_tree,
            )
        except writer_tree_module.WriterTreeError as exc:
            raise SuccessorProduceError(f"the writer set is not usable: {exc}") from exc
        (pathlib.Path(scratch) / "writer-tree-receipt.json").write_bytes(
            root_disk.canonical_json(writer_receipt)
        )

        plan = phase.plan_for(
            layer=layer,
            tree=tree,
            writer_tree=writer_tree,
            image=produced["root-disk"],
            staging=pathlib.Path(scratch) / "staging",
        )
        try:
            disk_result = root_disk_execute.execute(plan, layer, tree, writer_tree)
        except root_disk_execute.RootDiskExecuteError as exc:
            raise SuccessorProduceError(str(exc)) from exc

        report = image_verify.verify_tree(
            tree=image_verify.tree_from_initrd(initrd_raw),
            expectations=image_verify.expectations_from_lock(sealed_source_lock()),
            launcherSha256=_sha256(launcher_binary),
            kernel=produced["kernel"].read_bytes(),
        )
        if not report["passed"]:
            failed = [row["id"] for row in report["checks"] if not row["ok"]]
            raise SuccessorProduceError(
                "the produced image failed: " + ", ".join(failed)
            )

        return production_result(
            manifest_entries=producer.manifest_from_directory(
                outputs, phase.output_names()
            ),
            output_names=phase.output_names(),
            build_receipt=build_receipt,
            builder_internal=computed,
            kernel_sha256=kernel_result["kernel"]["sha256"],
            kernel_disposition=kernel_disposition,
            root_disk=disk_result["image"],
            root_disk_evidence=phase.root_disk_evidence(disk_result),
            verify_report=report,
        )


def _resolved_tool(name: str) -> pathlib.Path:
    """Whichever copy of a replay tool this host has, named as a real file."""

    import shutil

    found = shutil.which(name)
    if found is None:
        raise SuccessorProduceError(f"this host has no {name}")
    return assert_preflight_tool(pathlib.Path(found).resolve())


def _named_or_resolved_tool(name: str, named: Optional[pathlib.Path]) -> pathlib.Path:
    """The tool the caller named, or this host's, and either way a real file."""

    if named is None:
        return _resolved_tool(name)
    return assert_preflight_tool(pathlib.Path(named))


def _write_once(path: pathlib.Path, document: Mapping[str, Any]) -> str:
    """Results are added, never replaced: an existing one stops the run."""

    path = pathlib.Path(path)
    if path.exists():
        raise SuccessorProduceError(
            f"a result is already sealed here and is not overwritten: {path}"
        )
    raw = staging.canonical_json(document)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return _sha256(raw)


def _parser():
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="mode", required=True)

    for mode in ("preflight", "produce"):
        child = sub.add_parser(mode)
        child.add_argument("--repository-root", type=pathlib.Path, default=REPOSITORY_ROOT)
        child.add_argument("--cas", type=pathlib.Path, required=True)
        child.add_argument("--launcher", type=pathlib.Path, required=True)
        child.add_argument("--scratch", type=pathlib.Path, required=True)
        child.add_argument("--result", type=pathlib.Path, required=True)
        # Named rather than looked up when the caller knows: the production
        # runs inside a transient unit with a cleaned environment, where PATH
        # is not the one the surrounding job resolved its tools from.
        child.add_argument("--gpgv", type=pathlib.Path)
        child.add_argument("--zstd", type=pathlib.Path)
        if mode == "produce":
            child.add_argument("--outputs", type=pathlib.Path, required=True)
    return parser



def pin_temporary_directory(scratch) -> pathlib.Path:
    """Name the place a temporary directory is taken from, once, for the whole
    run.

    The production runs inside a transient unit that mounts the filesystem
    hierarchy read-only except the paths it was handed, and `systemd-run` does
    not carry the caller's environment in, so an exported ``TMPDIR`` does not
    reach here.  Python's default list -- /tmp, /var/tmp, /usr/tmp, / -- is
    then entirely unwritable, and two helpers deep in the shared builder ask
    for a temporary directory without naming one: the InRelease signature
    check and the zstd decompressor.  Both are on this path.

    Neither is this module's to edit.  The predecessor image is built from the
    same base module and has to keep reproducing byte for byte, so the fix
    belongs where the writable place is already known: the scratch the caller
    handed in, which is the directory the isolation was already told it may
    write.  Pinning it widens nothing.

    The scratch is not the staging tree -- that is a subdirectory of it, on a
    tmpfs the wrapper mounts -- so nothing taken from here can reach the image.
    """

    pinned = pathlib.Path(scratch) / "tmp"
    pinned.mkdir(parents=True, exist_ok=True)
    tempfile.tempdir = str(pinned)
    return pinned


def main(argv: Optional[list] = None) -> int:
    arguments = _parser().parse_args(argv)
    # Before anything is read, because a run that reads first is a run that can
    # still be stopped by the environment the reading happens in.
    pin_temporary_directory(arguments.scratch)
    gpgv = _named_or_resolved_tool("gpgv", arguments.gpgv)
    zstd = _named_or_resolved_tool("zstd", arguments.zstd)
    try:
        launcher_binary = pathlib.Path(arguments.launcher).read_bytes()
    except OSError as exc:
        raise SuccessorProduceError(
            f"the rebuilt launcher is unreadable: {arguments.launcher}"
        ) from exc

    # Built here rather than defaulted inside the entry points: the argument is
    # required there precisely so that a caller has to have obtained one.
    artifact_store = assert_artifact_store(arguments.cas)
    nested_tree = builder().nested_runtime_tree(
        arguments.repository_root, artifact_store, gpgv, zstd
    )

    common = {
        "repository_root": arguments.repository_root,
        "artifact_store": artifact_store,
        "scratch": arguments.scratch,
        "gpgv": gpgv,
        "zstd": zstd,
        "launcher_binary": launcher_binary,
        "nested_tree": nested_tree,
        "content_manifest_sha256": CONTENT_MANIFEST_SHA256,
    }
    if arguments.mode == "produce":
        result = produce(outputs=arguments.outputs, **common)
    else:
        result = preflight(**common)

    digest = _write_once(arguments.result, result)
    print(f"{arguments.mode}: {arguments.result} sha256={digest}")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
