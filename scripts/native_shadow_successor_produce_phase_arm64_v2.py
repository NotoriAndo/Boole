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

The other is that the expensive step happens once.  A refusal raised before the
output directory exists has cost nothing and may be repeated; a failure raised
after an output file exists has spent the only attempt there is.  Everything that
can be checked is therefore checked on the near side of that line, which is why
``preflight`` exists at all: it does the whole assembly and the whole walk, and
it cannot reach an image tool, because its call graph never touches one.

Neither the launcher source nor its sealed binary is rebuilt or modified here.
The rebuilt bytes arrive as an argument and are matched against the seal.
"""

from __future__ import annotations

import ast
import hashlib
import json
import os
import pathlib
from typing import Any, Iterable, Mapping, Optional

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
AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v2.json"
)
AUTHORITY_SHA256 = "c52e319790e3ca52ba6d635007e541f25e12d6d1497c1abb46ef00b1684b6e58"
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
    import sys

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


def _entry(entries: Mapping[str, Any], guest_path: str, what: str) -> Mapping[str, Any]:
    key = guest_path.lstrip("/")
    entry = entries.get(key)
    if entry is None:
        raise SuccessorProduceError(f"the staging tree has no {what}: {guest_path}")
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


def attempt_consumed(*, outputs_created: bool) -> bool:
    """Where the budget line falls, in one place, as the authority draws it."""

    return bool(outputs_created)


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

    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "authoritySha256": AUTHORITY_SHA256,
        "bootableClaim": BOOTABLE_CLAIM,
        "builderInternal": computed,
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

    # Everything above this line is refusable for free.  The output directory is
    # made here and not one statement earlier, so a refusal above costs nothing
    # and a failure below has spent the attempt.
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
            cas_roots=[artifact_store], zstd=pathlib.Path(zstd), writer_tree=writer_tree
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
        raise SuccessorProduceError("the produced image failed: " + ", ".join(failed))

    manifest_entries = producer.manifest_from_directory(outputs, phase.output_names())
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
        "builderInternal": computed,
        "kernel": {
            "disposition": kernel_disposition,
            "sha256": kernel_result["kernel"]["sha256"],
        },
        "outputManifest": [dict(row) for row in manifest_entries],
        "outputsCreated": True,
        "release": RELEASE,
        "rootDisk": disk_result["image"],
        "rootDiskEvidence": phase.root_disk_evidence(disk_result),
        "servingClaim": SERVING_CLAIM,
        "sourceLockSha256": SOURCE_LOCK_SHA256,
        "verifyReport": report,
    }


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


def main(argv: Optional[list] = None) -> int:
    arguments = _parser().parse_args(argv)
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
