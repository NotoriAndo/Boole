#!/usr/bin/env python3
"""Run the offline half of the arm64 boot image producer, deciding nothing.

Every step this performs already exists and is already frozen.  What did not
exist is the thing that says in which order they run and where each one's
inputs come from, and that is precisely the part with room to differ between two
jobs whose outputs are supposed to be identical.  So this module contributes no
values of its own: the tool paths are read out of the builder authority, the
image size is the root disk plan's own floor for this layer, and the output
names are the producer authority's.  Each of those is derived rather than
restated, because a second copy of a frozen value is a second thing that can
drift.

The phase is offline on purpose.  Everything it needs -- the 191 verified
payloads, the Rust distribution, the rebuilt launcher -- was placed on disk by
the acquire phase, and the sealed producer authority runs this half with the
network taken away.  Nothing here fetches anything, and there is no fallback
that would.

Two host facts are not negotiable.  `mke2fs -d` copies each staged file's owner
into the image, so this runs as root; and the frozen tools are aarch64 ELFs, so
this runs on the arm64 runner.  A run that is neither refuses at the start
rather than producing an image that answers to whoever invoked it.

Producing the three files is not booting them.  Nothing here starts a virtual
machine, and `bootableClaim` stays false in everything it writes.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import pathlib
import sys
import tarfile
import tempfile
from typing import Any, Mapping, Optional, Sequence

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_produce_arm64_v1 as producer
from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_initrd_arm64_v1 as initrd
from scripts import native_shadow_boot_kernel_extract_arm64_v1 as kernel_extract
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk
from scripts import native_shadow_boot_root_disk_execute_arm64_v1 as root_disk_execute
from scripts import native_shadow_boot_writer_tree_arm64_v1 as writer_tree_module
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_builder
from scripts import native_shadow_rootfs_portable_boot_arm64_v1 as portable_boot


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_BOOT_VERIFIED = False

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILDER_AUTHORITY_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
)
BOOT_SOURCE_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
SUCCESSOR_AUTHORITY_PATH = (
    REPOSITORY_ROOT
    / "native/containment/"
    "native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json"
)
PREREGISTRATION_PATH = (
    REPOSITORY_ROOT
    / "native/containment/"
    "native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json"
)
SELECTION_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json"
)

# The one cause the successor authority lists as open.  It is named here rather
# than matched loosely so that a second cause, or this one under a new name, has
# no clearance and refuses -- clearing a cause is not clearing the record.
STAGED_CTIME_BLOCKER = "staged-inode-ctime-is-not-fs-now"

SCHEMA = "boole.native-shadow.boot-produce-phase-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-PRODUCE-PHASE-ARM64-V1"
STATUS = "BOOT-IMAGE-FILES-PRODUCED-OFFLINE-NOT-BOOT-AUTHORITY"

# The only guest path this module names.  `mke2fs` reads `MKE2FS_CONFIG` and
# falls back to the host's own file when it is unset, which would let the
# runner's configuration choose feature flags the closure never froze.  The
# frozen tree ships e2fsprogs, so the config it reads is that tree's.
MKE2FS_CONFIG_GUEST_PATH = "/etc/mke2fs.conf"

# The authority names each tool by the job it does, not by the name of the file.
TOOL_ROLES = {"ext4-image-writer": "mke2fs", "ext4-image-inspector": "debugfs"}

# The three things this phase produces, said once.  The producer authority owns
# what each file is called; these are only enough to tell the three apart, so a
# renamed output is found here rather than written under the old name.
OUTPUT_ROLES = ("kernel", "initrd", "root-disk")

DIGEST_PREFIX = "sha256:"


class ProducePhaseError(RuntimeError):
    """The produce phase cannot run, or what it produced does not answer."""


def builder_authority(
    path: pathlib.Path = BUILDER_AUTHORITY_PATH,
) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ProducePhaseError(f"the builder authority is unreadable: {path}") from exc


def successor_authority(
    path: pathlib.Path = SUCCESSOR_AUTHORITY_PATH,
) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ProducePhaseError(f"the successor authority is unreadable: {path}") from exc


def candidate_preregistration(
    path: pathlib.Path = PREREGISTRATION_PATH,
) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ProducePhaseError(f"the pre-registration is unreadable: {path}") from exc


def selection_record(path: pathlib.Path = SELECTION_PATH) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ProducePhaseError(f"the selection record is unreadable: {path}") from exc


def file_digest(path: pathlib.Path) -> str:
    try:
        return hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError as exc:
        raise ProducePhaseError(f"a bound record is unreadable: {path}") from exc


def assert_staged_ctime_cause_removed(
    record: Optional[Mapping[str, Any]] = None,
) -> bool:
    """Say whether the cause the sealed record named is still in the writer.

    The sealed record cannot answer this and must not be edited to try.  It was
    written when the frozen `mke2fs` was the only `mke2fs`, and against that
    binary the cause is open and stays open: `create_inode` overwrites `i_ctime`
    from the staged file's `st_ctime`, which no caller can set.  What changed is
    which binary writes the image, and that is a fact from after the seal.

    So the answer is derived from the two append-only records that came after it.
    The pre-registration fixed the accept rule while no deb had been fetched, and
    it is the record that grants an unblock at all; the selection applied that
    rule by reading binaries rather than running them.  Both are checked back to
    the sealed record by digest, which means a sealed record quietly rewritten to
    say something friendlier fails this rather than passing it.

    The load-bearing check is the last one.  A record saying FIXED about some
    binary settles nothing unless that binary is the one this plan hands to the
    runner, so the plan's own pinned digest is required to be the control that
    was read.  Nothing here relaxes what the produced images must then satisfy.
    """

    document = record if record is not None else selection_record()

    predecessor = document.get("appendOnly", {}).get("predecessor", {})
    on_disk = file_digest(PREREGISTRATION_PATH)
    if predecessor.get("sha256") != on_disk:
        raise ProducePhaseError(
            "the selection was written against a different pre-registration: it "
            f"binds {predecessor.get('sha256')} and the one on disk is {on_disk}"
        )

    preregistration = candidate_preregistration()
    readiness = preregistration.get("productionReadiness", {})
    if not readiness.get("unblocksOnlyOnAPassingStaticRead"):
        raise ProducePhaseError(
            "the pre-registration does not grant an unblock on a passing read, "
            "so a passing read does not unblock anything"
        )

    bound = {
        row.get("path"): row.get("sha256")
        for row in preregistration.get("bindings", {}).get(
            "recordsThatStayByteUnchanged", []
        )
    }
    relative = SUCCESSOR_AUTHORITY_PATH.relative_to(REPOSITORY_ROOT).as_posix()
    sealed = file_digest(SUCCESSOR_AUTHORITY_PATH)
    if bound.get(relative) != sealed:
        raise ProducePhaseError(
            "the sealed record this clearance answers is not the one the "
            f"pre-registration bound: it binds {bound.get(relative)} and the one "
            f"on disk is {sealed}"
        )

    controls = document.get("controls", {})
    positive = controls.get("positive", {})
    negative = controls.get("negative", {})
    if positive.get("verdict") != "FIXED":
        raise ProducePhaseError(
            "the selected writer did not pass the static read: its verdict is "
            f"{positive.get('verdict')}"
        )
    if negative.get("verdict") != "DEFECT":
        raise ProducePhaseError(
            "the rule failed no control, so it accepted the selected writer "
            "without having rejected anything"
        )

    guest = document.get("guestPackages", {})
    lock = file_digest(BOOT_SOURCE_LOCK_PATH)
    if guest.get("sourceLockSha256") != lock:
        raise ProducePhaseError(
            "the guest package lock moved under the selection: it binds "
            f"{guest.get('sourceLockSha256')} and the one on disk is {lock}"
        )
    if guest.get("replaced") or guest.get("deleted"):
        raise ProducePhaseError(
            "the writer is an addition, not a substitution, and this selection "
            "records the guest packages as replaced or deleted"
        )

    writer_time = document.get("writerTime", {})
    honoured = writer_time.get("variableTheSelectedBuildHonours")
    if root_disk.WRITER_TIME_ENV != honoured:
        raise ProducePhaseError(
            f"the plan sets {root_disk.WRITER_TIME_ENV} and the selected build "
            f"honours {honoured}, which would set the time and leave the flag "
            "the writer branches on clear"
        )
    superseded = writer_time.get("variableThePlanCurrentlySets")
    if root_disk.SUPERSEDED_WRITER_TIME_ENV != superseded:
        raise ProducePhaseError(
            f"the plan knows {root_disk.SUPERSEDED_WRITER_TIME_ENV} as the "
            f"superseded variable and the selection names {superseded}"
        )
    if int(root_disk.EXT4_WRITER_TIME) == 0:
        raise ProducePhaseError(
            "the plan hands the writer the unset sentinel 0, which is what the "
            "sealed run set and why every stamp fell back to the wall clock"
        )

    pinned = positive.get("writer", {}).get("sha256")
    if root_disk.MKE2FS_SHA256 != pinned:
        raise ProducePhaseError(
            f"the plan pins writer {root_disk.MKE2FS_SHA256} and the binary that "
            f"was read and found fixed is {pinned}"
        )
    return True


# Each entry is one named cause and the derivation that says it is gone.  A cause
# with no entry has no clearance, which is the default and the safe one.
BLOCKER_CLEARANCES = {STAGED_CTIME_BLOCKER: assert_staged_ctime_cause_removed}


def assert_production_unblocked(
    record: Optional[Mapping[str, Any]] = None,
) -> bool:
    """Refuse to start while a known cause of the last mismatch is still present.

    The successor record allows one production pair and forbids retrying a pair
    that has produced a result.  So a run dispatched against a cause the record
    itself lists as open would spend the single attempt on an outcome already
    known.  The audit inside this phase would catch such an image, loudly -- this
    is about not spending the attempt, not about trusting the result.

    A cause the record names can be answered by a later append-only record, but
    only by evidence: the clearance has to derive the cause's absence from what
    is on disk, and it refuses if it cannot.  Removing the gate would also let
    the dispatch through, which is why no clearance is allowed to be a statement
    that the cause is gone -- each one is a re-derivation of why.
    """

    readiness = (record if record is not None else successor_authority()).get(
        "productionReadiness", {}
    )
    if not readiness.get("blocked"):
        return True

    why = readiness.get("why", "")
    open_causes = [
        cause
        for cause in readiness.get("blockedBy", [])
        if cause not in BLOCKER_CLEARANCES
    ]
    if open_causes or not readiness.get("blockedBy"):
        causes = ", ".join(open_causes) or "an unnamed cause"
        raise ProducePhaseError(
            f"the successor authority blocks production: {causes}. {why}".strip()
        )

    for cause in readiness["blockedBy"]:
        BLOCKER_CLEARANCES[cause]()
    return True


def tool_paths(
    tree: pathlib.Path,
    writer_tree: pathlib.Path,
    *,
    authority: Optional[Mapping[str, Any]] = None,
) -> dict[str, str]:
    """Where each tool and the config sit, in whichever tree holds it.

    The runner is Ubuntu 24.04 arm64 and ships its own `mke2fs`, its own
    `debugfs` and its own `mke2fs.conf`.  Every one is named relative to a tree
    so that none of the runner's copies can be the one that writes the image.

    The writer is the one tool that no longer comes from the frozen tree.  The
    sealed authority's row for it is left exactly as it was -- it is the record
    of what the first pair used, and that pair's failure is what the row is
    evidence of -- and simply stops being the path that runs.  The inspector,
    the read-only checker and the config stay frozen, so the image is still
    judged by tools that did not write it.
    """

    document = builder_authority() if authority is None else authority
    rows = document.get("toolBinaries")
    if not isinstance(rows, list) or not rows:
        raise ProducePhaseError("the builder authority pins no tool binaries")
    found: dict[str, str] = {}
    for row in rows:
        name = TOOL_ROLES.get(str(row.get("role")))
        if name is None:
            raise ProducePhaseError(f"unknown tool role: {row.get('role')!r}")
        found[name] = str(tree / str(row["memberPath"]).lstrip("./"))
    missing = sorted(set(TOOL_ROLES.values()) - set(found))
    if missing:
        raise ProducePhaseError(
            "the builder authority pins no " + ", ".join(missing)
        )
    found["mke2fs"] = str(writer_tree / writer_tree_module.WRITER_TREE_PATH)
    found["config"] = str(tree / MKE2FS_CONFIG_GUEST_PATH.lstrip("/"))
    # The read-only checker was not pinned when this authority was sealed, and
    # the authority is not edited after the fact to say it was.  It ships in the
    # same e2fsprogs package as the writer, so the binding is that package: the
    # successor record pins the binary's own digest, and `assert_tools` re-hashes
    # the file before it is run.
    packages = {str(row.get("packageSha256")) for row in rows}
    if packages != {root_disk.E2FSPROGS_PACKAGE_SHA256}:
        raise ProducePhaseError(
            "the pinned tools do not all come from the frozen e2fsprogs package: "
            f"{sorted(packages)}"
        )
    found["e2fsck"] = str(tree / root_disk.E2FSCK_MEMBER_PATH.lstrip("./"))
    return found


ROOT_DISK_EVIDENCE_FIELDS = (
    "fsck",
    "loaderEvidence",
    "timeAudit",
    "toolDigests",
    "writerTime",
)


def root_disk_evidence(disk_result: Mapping[str, Any]) -> dict[str, Any]:
    """The four things the executor settles that a plan cannot, kept.

    Which library files the loader really opened, what fixed time the writer was
    handed, whether any timestamp in the produced image is outside the closed
    set, and what the frozen read-only checker said.  All of it was computed here
    and then dropped before, which is why the record of the first failed pair had
    to leave the loader question open.  Missing evidence is an error rather than
    an absent key: a result that quietly lacks a checker verdict would read as
    one that passed.
    """

    missing = [name for name in ROOT_DISK_EVIDENCE_FIELDS if name not in disk_result]
    if missing:
        raise ProducePhaseError(
            "the root disk executor returned no " + ", ".join(missing)
        )
    return {name: disk_result[name] for name in ROOT_DISK_EVIDENCE_FIELDS}


def pinned_size_bytes(layer: bytes) -> int:
    """The image size, derived from the layer rather than chosen for it.

    The plan already computes the smallest size that provably holds this tree --
    content blocks, inode table, journal and a flat metadata allowance.  Taking
    that number is the one option that is not a decision: any headroom above it
    would be a figure this module invented, and two jobs agree only on figures
    neither of them invented.  If the floor turns out not to hold the tree, that
    is the plan's `abort-never-relax` case and is reported, not padded.
    """

    return root_disk.required_bytes(root_disk.layer_entries(layer))


def plan_for(
    *,
    layer: bytes,
    tree: pathlib.Path,
    writer_tree: pathlib.Path,
    image: pathlib.Path,
    staging: pathlib.Path,
) -> dict[str, Any]:
    """The frozen root disk plan for this layer, with nothing left to choose."""

    tools = tool_paths(tree, writer_tree)
    try:
        return root_disk.root_disk_plan(
            layer=layer,
            mke2fs=tools["mke2fs"],
            debugfs=tools["debugfs"],
            e2fsck=tools["e2fsck"],
            config=tools["config"],
            image=str(image),
            staging=str(staging),
            sizeBytes=pinned_size_bytes(layer),
        )
    except root_disk.RootDiskPlanError as exc:
        raise ProducePhaseError(str(exc)) from exc


def layer_bytes(oci: pathlib.Path, receipt: Mapping[str, Any]) -> bytes:
    """The one layer blob the build receipt named, re-hashed before it is used."""

    digest = receipt.get("layerDigest")
    if not isinstance(digest, str) or not digest.startswith(DIGEST_PREFIX):
        raise ProducePhaseError(f"the build receipt names no layer: {digest!r}")
    hexdigest = digest[len(DIGEST_PREFIX) :]
    blob = oci / "blobs" / "sha256" / hexdigest
    try:
        raw = blob.read_bytes()
    except OSError as exc:
        raise ProducePhaseError(f"the verified layer blob is absent: {blob}") from exc
    found = hashlib.sha256(raw).hexdigest()
    if found != hexdigest:
        raise ProducePhaseError(
            f"the layer blob hashes to {found}, the receipt says {hexdigest}"
        )
    return raw


def output_names() -> tuple[str, ...]:
    """The three files the produce phase owes, as the producer authority names them."""

    return producer.output_names(producer.load_authority(REPOSITORY_ROOT))


def output_paths(outputs: pathlib.Path) -> dict[str, pathlib.Path]:
    """One path per role, spelled the way the producer authority spells it."""

    names = output_names()
    found: dict[str, pathlib.Path] = {}
    for role in OUTPUT_ROLES:
        matched = [name for name in names if name.endswith(role)]
        if len(matched) != 1:
            raise ProducePhaseError(
                f"the producer authority names {len(matched)} outputs for {role}"
            )
        found[role] = outputs / matched[0]
    if len(set(found.values())) != len(names):
        raise ProducePhaseError(
            "the producer authority names an output this phase does not produce"
        )
    return found


def _extract_tree(layer: bytes, tree: pathlib.Path) -> None:
    """Unpack the verified layer, keeping the numeric owners it recorded."""

    tree.mkdir(parents=True, exist_ok=True)
    extra: dict[str, Any] = {"numeric_owner": True}
    if hasattr(tarfile, "fully_trusted_filter"):
        # The bytes were just re-hashed against the receipt, so they are the
        # frozen layer; the filter argument only silences a default that would
        # otherwise change under us on a newer interpreter.
        extra["filter"] = "fully_trusted"
    with tarfile.open(fileobj=io.BytesIO(layer), mode="r:") as handle:
        handle.extractall(tree, **extra)


def _runtime_lock(gpgv: pathlib.Path, zstd: pathlib.Path) -> tuple[Any, bytes]:
    """Bind this runner's gpgv and zstd into an ephemeral builder input."""

    try:
        sealed_raw = BOOT_SOURCE_LOCK_PATH.read_bytes()
    except OSError as exc:
        raise ProducePhaseError("the sealed boot source lock is unreadable") from exc
    sealed = json.loads(sealed_raw.decode("utf-8"))
    try:
        runtime, _ = portable_boot.materialize_runtime_lock(
            sealed, sealed_raw, gpgv, zstd
        )
        normalized, normalized_raw, _ = boot_builder.normalized_runtime_lock(runtime)
    except (portable_boot.PortableAuthorityError, boot_builder.BootProjectionError) as exc:
        raise ProducePhaseError(str(exc)) from exc
    return normalized, normalized_raw


def bind_temporary_directory(scratch: pathlib.Path) -> pathlib.Path:
    """Put every temporary file in the one tree the sealed unit can write.

    `systemd-run` starts a transient unit with a clean environment, so the
    `TMPDIR` the driver exports never arrives here; and the unit is sealed with
    the filesystem read-only apart from the paths it was given, so /tmp,
    /var/tmp, /usr/tmp and / are all refused.  Python finds nothing usable and
    dies before the phase has written anything.

    Binding it here rather than carrying it in leaves no environment variable to
    forget.  The environment is set as well as `tempfile`, because `zstd`,
    `mke2fs` and `debugfs` read the variable rather than Python's idea of it.
    """

    temporary = scratch / "tmp"
    temporary.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(temporary)
    tempfile.tempdir = str(temporary)
    return temporary


def produce(
    *,
    scratch: pathlib.Path,
    outputs: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    cas: Optional[pathlib.Path] = None,
    repository_root: pathlib.Path = REPOSITORY_ROOT,
) -> dict[str, Any]:
    """Build the three boot files from the frozen closure, offline."""

    if os.geteuid() != 0:
        raise ProducePhaseError(
            "the produce phase must run as root: mke2fs -d copies the staged owner "
            "into the image, and the frozen plan says root:root throughout"
        )
    assert_production_unblocked()
    store = scratch / "cas" if cas is None else cas
    oci = scratch / "oci"
    tree = scratch / "tree"
    outputs.mkdir(parents=True, exist_ok=True)
    produced = output_paths(outputs)

    lock, lock_raw = _runtime_lock(gpgv, zstd)
    try:
        launcher_binary = launcher.read_bytes()
    except OSError as exc:
        raise ProducePhaseError(f"the rebuilt launcher is unreadable: {launcher}") from exc
    build_receipt = boot_builder.build_oci_layout(
        lock,
        lock_raw,
        repository_root,
        store,
        oci,
        launcher_binary=launcher_binary,
    )
    layer = layer_bytes(oci, build_receipt)
    _extract_tree(layer, tree)

    kernel_result, kernel_disposition = kernel_extract.extract(
        cas_roots=[store],
        zstd_path=zstd,
        out_dir=outputs,
        result_path=scratch / "kernel-extract-result.json",
    )
    if not produced["kernel"].is_file():
        raise ProducePhaseError(
            "the kernel extractor named its file something other than the producer "
            f"authority's {produced['kernel'].name}"
        )
    initrd_raw = initrd.initrd_bytes(layer)
    produced["initrd"].write_bytes(initrd_raw)

    # The writer set is unpacked into a tree of its own, beside the frozen one
    # rather than inside it: the guest tree is what gets written into the image
    # and not one byte of it moves because the tool that writes it changed.
    writer_tree = scratch / "writer"
    try:
        writer_receipt = writer_tree_module.materialize(
            cas_roots=[store], zstd=zstd, writer_tree=writer_tree
        )
    except writer_tree_module.WriterTreeError as exc:
        raise ProducePhaseError(f"the writer set is not usable: {exc}") from exc
    (scratch / "writer-tree-receipt.json").write_bytes(
        root_disk.canonical_json(writer_receipt)
    )

    image = produced["root-disk"]
    plan = plan_for(
        layer=layer,
        tree=tree,
        writer_tree=writer_tree,
        image=image,
        staging=scratch / "staging",
    )
    (scratch / "root-disk-plan.json").write_bytes(root_disk.canonical_json(plan))
    try:
        disk_result = root_disk_execute.execute(plan, layer, tree, writer_tree)
    except root_disk_execute.RootDiskExecuteError as exc:
        raise ProducePhaseError(str(exc)) from exc

    report = image_verify.verify_tree(
        tree=image_verify.tree_from_initrd(initrd_raw),
        expectations=image_verify.expectations_from_lock(
            json.loads(BOOT_SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        ),
        launcherSha256=hashlib.sha256(launcher_binary).hexdigest(),
        kernel=produced["kernel"].read_bytes(),
    )
    if not report["passed"]:
        failed = [row["id"] for row in report["checks"] if not row["ok"]]
        raise ProducePhaseError("the produced image failed: " + ", ".join(failed))

    entries = producer.manifest_from_directory(outputs, output_names())
    manifest = producer.manifest_text(entries)
    (scratch / "OUTPUT-MANIFEST.txt").write_text(manifest, encoding="utf-8")
    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": {
            "guestBootVerified": GUEST_BOOT_VERIFIED,
            "guestImageBuilt": True,
            "runtimeCompatibilityVerified": False,
        },
        "buildReceipt": build_receipt,
        "kernel": {
            "disposition": kernel_disposition,
            "sha256": kernel_result["kernel"]["sha256"],
        },
        "manifest": manifest,
        "release": RELEASE,
        # The executor settles four things a plan cannot: which library files the
        # loader really opened, what fixed time the writer was handed, whether any
        # timestamp in the produced image is outside the closed set, and what the
        # frozen read-only checker said about the filesystem.  All four used to be
        # computed here and then dropped, which is why the record of the first
        # failed pair had to leave the loader question open.
        "rootDisk": disk_result["image"],
        "rootDiskEvidence": root_disk_evidence(disk_result),
        "schema": SCHEMA,
        "status": STATUS,
        "verification": report,
    }


def assert_replica_evidence(result: Mapping[str, Any]) -> dict[str, Any]:
    """Read a replica's own result back and require it to have proved itself.

    The executor already stops a run that fails any of this, so a replica that
    got here should pass.  That is the reason to check it again from outside:
    the failure this guards against is not a bad image but a result that never
    ran the check and reads as though it did.  Absence is a failure here.
    """

    evidence = result.get("rootDiskEvidence")
    if not isinstance(evidence, Mapping):
        raise ProducePhaseError("the result carries no root disk evidence")
    missing = [name for name in ROOT_DISK_EVIDENCE_FIELDS if name not in evidence]
    if missing:
        raise ProducePhaseError("the evidence carries no " + ", ".join(missing))
    fsck = evidence["fsck"]
    if not isinstance(fsck, Mapping) or fsck.get("exitCode") != 0:
        raise ProducePhaseError(f"the read-only check did not pass: {fsck}")
    times = evidence["timeAudit"]
    if not isinstance(times, Mapping) or not times.get("passed"):
        raise ProducePhaseError(
            f"the image carries {times.get('violationCount')} timestamps outside "
            f"{times.get('allowedTimestamps')}"
        )
    writer = evidence["writerTime"]
    if not isinstance(writer, int) or writer <= 0:
        raise ProducePhaseError(f"the writer was handed no fixed time: {writer!r}")
    loader = evidence["loaderEvidence"]
    checker = loader.get("checker") if isinstance(loader, Mapping) else None
    writer_closure = loader.get("writer") if isinstance(loader, Mapping) else None
    if not isinstance(checker, Mapping) or not checker.get("tree"):
        raise ProducePhaseError("the evidence names no frozen tree")
    if not isinstance(writer_closure, Mapping) or not writer_closure.get("tree"):
        raise ProducePhaseError("the evidence names no writer set tree")
    root_disk_execute.assert_loader_evidence(
        loader,
        tree=pathlib.Path(checker["tree"]),
        writer_tree=pathlib.Path(writer_closure["tree"]),
    )
    return {
        "fsckExitCode": fsck["exitCode"],
        "librariesRecorded": len(checker["libraries"]) + len(writer_closure["libraries"]),
        "rootDiskSha256": result.get("rootDisk", {}).get("sha256"),
        "timestampsOutsideTheClosedSet": 0,
        "writerTime": writer,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("produce", help="build the boot files offline, as root")
    run.add_argument("--scratch", type=pathlib.Path, required=True)
    run.add_argument("--outputs", type=pathlib.Path, required=True)
    run.add_argument("--gpgv", type=pathlib.Path, required=True)
    run.add_argument("--zstd", type=pathlib.Path, required=True)
    run.add_argument("--launcher", type=pathlib.Path, required=True)
    run.add_argument("--cas", type=pathlib.Path)
    run.add_argument("--result", type=pathlib.Path)
    check = sub.add_parser(
        "evidence", help="require a replica's own result to have proved itself"
    )
    check.add_argument("--result", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _parser().parse_args(argv)
    if args.command == "evidence":
        try:
            summary = assert_replica_evidence(
                json.loads(args.result.read_text(encoding="utf-8"))
            )
        except (ProducePhaseError, root_disk_execute.RootDiskExecuteError, OSError, ValueError) as exc:
            print(f"produce-phase: {exc}", file=sys.stderr)
            return 1
        json.dump(summary, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
        return 0
    # Here rather than inside produce(): the binding is process-global, and the
    # process boundary is the one place where changing process-global state is
    # not a surprise to whoever called.
    try:
        bind_temporary_directory(args.scratch)
        result = produce(
            scratch=args.scratch,
            outputs=args.outputs,
            gpgv=args.gpgv,
            zstd=args.zstd,
            launcher=args.launcher,
            cas=args.cas,
        )
    except (ProducePhaseError, OSError, ValueError) as exc:
        print(f"produce-phase: {exc}", file=sys.stderr)
        return 1
    if args.result:
        args.result.write_bytes(root_disk.canonical_json(result))
    sys.stdout.write(result["manifest"])
    print(f"bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
