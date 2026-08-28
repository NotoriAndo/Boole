#!/usr/bin/env python3
"""Read the successor's root disk back against the lock it was built from.

The third production attempt built three files that were exactly what the
successor source lock asks for, and was then failed by the stage that reads the
image back, because that stage takes its expectations from the predecessor's
lock.  The image was right.  The baseline it was compared against was the wrong
generation, and the two locks disagree about precisely the files this wave
rewrote.

So the successor gets its own consumer rather than a flag on the predecessor's.
The lock it uses is not a parameter, not an argument, not an environment
variable and not something read out of the image: it is the one the producing
phase already proves, at the digest the production authority bound, checked
before anything is attached to a loop device.  A lock whose bytes have moved is
refused while the image is still an unopened file.

Nothing here falls back.  There is no path from this module to the predecessor's
lock and no path from the predecessor's consumer to this one, which is why the
predecessor's own production keeps working unchanged: it was never wrong.

Reading an image is not booting it.  This stage mounts read-only, with devices,
programs and set-user-id all refused by the mount itself, and claims nothing
about whether the thing it read would start.
"""

from __future__ import annotations

import argparse
import pathlib
import sys
from typing import Any, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as reader
from scripts import native_shadow_successor_produce_phase_arm64_v2 as phase


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_BOOT_VERIFIED = False

SCHEMA = "boole.native-shadow.successor-root-disk-readback.arm64.v2"
RELEASE = "NATIVE-SHADOW-SUCCESSOR-ROOT-DISK-READBACK-ARM64-V2"
STATUS = "SUCCESSOR-ROOT-DISK-CONTENTS-READ-BACK-NOT-BOOT-AUTHORITY"

# Its own name, so a document left by one generation can never be read as the
# other's proof, whatever ends up in an outputs directory.
RESULT_NAME = "SUCCESSOR-ROOT-DISK-READBACK.json"

# The checks are the verification stage's, named once, over there.
REQUIRED_CHECKS = tuple(image_verify.REQUIRED_CHECKS)

# The lock, taken from the phase that built the image rather than restated here.
# Restating it would be a second copy of a frozen value, and a second copy is a
# thing that can drift from the first without anyone noticing -- which is the
# shape of the defect this module exists to correct.
SOURCE_LOCK_PATH = phase.SOURCE_LOCK_PATH
SOURCE_LOCK_SHA256 = phase.SOURCE_LOCK_SHA256

# How the image is opened and read, shared rather than reimplemented: two
# consumers with two copies of a mount option list are two consumers that can
# disagree about what read-only means.
MOUNT_OPTIONS = reader.MOUNT_OPTIONS
FILESYSTEM = reader.FILESYSTEM
mount_argv = reader.mount_argv
umount_argv = reader.umount_argv
output_paths = reader.output_paths
tree_from_directory = reader.tree_from_directory
sealed_launcher_sha256 = reader.sealed_launcher_sha256


class SuccessorReadbackError(RuntimeError):
    """The produced root disk cannot be read, or is not what the seals require."""


def predecessor_names() -> tuple:
    """The names this module must not contain, derived rather than spelled."""

    return phase._predecessor_names()


def assert_no_lock_fallback() -> None:
    """Nothing here can reach the other lock, so nothing can fall back to it."""

    source = pathlib.Path(__file__).read_text(encoding="utf-8")
    for named in (*predecessor_names(), phase.HISTORICAL_LOCK_CONSTANT):
        if named in source:
            raise SuccessorReadbackError(
                f"this consumer names the other source lock ({named}), so the "
                "substitution that spent the third attempt is expressible here"
            )


def sealed_expectations(*, path: Optional[pathlib.Path] = None) -> dict:
    """What the successor lock requires of each tracked path.

    Digest-checked and release-checked by the producing phase before it is
    parsed, so a lock that has been edited is a refusal rather than a baseline.
    The keyword exists for the tests that prove that refusal; the production
    call passes nothing and gets the sealed one.
    """

    return image_verify.expectations_from_lock(phase.sealed_source_lock(path=path))


def result_document(*, report: dict, image: pathlib.Path, entries: int) -> dict:
    """What was read, what it was compared against, and what that is not."""

    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": [
            "Reading the image back is not booting it; no virtual machine was "
            "started.",
            "This stage runs outside the producer authority's transient unit, "
            "which seals private devices and so cannot hold a loop mount.",
            "The expectations are the successor source lock's, at the digest "
            "the production authority bound; no other lock is reachable here.",
        ],
        "entryCount": entries,
        "guestBootVerified": GUEST_BOOT_VERIFIED,
        "image": {"name": image.name, "sha256": reader._digest(str(image))},
        "release": RELEASE,
        "schema": SCHEMA,
        "sourceLock": {
            "path": str(SOURCE_LOCK_PATH.relative_to(phase.REPOSITORY_ROOT)),
            "sha256": SOURCE_LOCK_SHA256,
        },
        "status": STATUS,
        "verification": report,
    }


def settle(
    *, outputs: pathlib.Path, document: dict, result: Optional[pathlib.Path]
) -> None:
    """Write the finding down, and disown the files if the finding is a refusal.

    The document is written before the refusal is raised, because a run that
    fails here has already spent the attempt and the reason is the only thing it
    still has to give.  A refusal then also marks the directory: the third
    attempt left three files and a failing report under names that said nothing
    about whether they were a production, and the operator had to establish that
    by hand.
    """

    if result is not None:
        result.parent.mkdir(parents=True, exist_ok=True)
        result.write_bytes(image_verify.canonical_json(document))
    try:
        image_verify.assert_passed(document["verification"])
    except image_verify.ImageVerifyError as refusal:
        try:
            phase.write_unqualified_diagnostic(outputs, refusal)
        except BaseException as second:
            sys.stderr.write(
                f"{phase.UNQUALIFIED_MARKER_NAME}: could not be written: {second}\n"
            )
        raise


def verify(
    *,
    outputs: pathlib.Path,
    mountpoint: Optional[pathlib.Path] = None,
    result: Optional[pathlib.Path] = None,
) -> dict[str, Any]:
    """Mount the produced image, read it, and check it against its own lock."""

    assert_no_lock_fallback()

    # Before the image is opened: a lock that has moved, or that belongs to
    # another generation, is refused while nothing is attached to a device.
    expectations = sealed_expectations()

    unmet = reader._unmet_requirements()
    if unmet:
        raise SuccessorReadbackError(
            "reading the image back requires " + "; ".join(unmet)
        )

    outputs = pathlib.Path(outputs)
    produced = output_paths(outputs)
    image = produced["root-disk"]
    if not image.is_file() or image.is_symlink():
        raise SuccessorReadbackError(f"the produced root disk is absent: {image}")

    where = outputs.parent / "successor-root-disk-readback" if mountpoint is None else mountpoint
    where.mkdir(parents=True, exist_ok=True)

    reader._run(mount_argv(image=image, mountpoint=where))
    try:
        tree = tree_from_directory(where)
    finally:
        reader._run(umount_argv(where))

    report = image_verify.verify_tree(
        tree=tree,
        expectations=expectations,
        launcherSha256=sealed_launcher_sha256(),
        kernel=produced["kernel"].read_bytes(),
    )
    document = result_document(report=report, image=image, entries=len(tree))
    settle(outputs=outputs, document=document, result=result)
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("verify", help="mount the produced root disk and check it")
    run.add_argument("--outputs", type=pathlib.Path, required=True)
    run.add_argument("--mountpoint", type=pathlib.Path)
    run.add_argument("--result", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    try:
        document = verify(
            outputs=options.outputs,
            mountpoint=options.mountpoint,
            result=options.result,
        )
    except (
        SuccessorReadbackError,
        phase.SuccessorProduceError,
        reader.RootDiskReadbackError,
        image_verify.ImageVerifyError,
    ) as exc:
        print(f"native-shadow successor root disk read-back: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "native-shadow successor root disk read-back: PASS "
        f"({document['entryCount']} entries)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
