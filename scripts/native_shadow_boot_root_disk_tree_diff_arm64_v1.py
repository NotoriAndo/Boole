#!/usr/bin/env python3
"""Compare two produced trees and say, path by path, what differs.

The successor image was built to add five directories and nothing else.  What
the pipeline could show for that claim was arithmetic: the filesystem check
counted five more inodes and five more blocks, and the image grew by five block
sizes.  That is consistent with the claim.  It is not the claim: the same
counts survive a file whose contents changed, a mode that moved, an owner that
was rewritten, a symlink retargeted at something else.

So this opens both images and reads both trees, and compares them entry by
entry -- path, kind, mode, owner, group, content digest, link target, hard link
grouping and extended attributes.  Only when the whole difference is exactly
the five directories the audit record names, each of them empty and carrying
the mode and ownership that record requires, does it pass.

Two containers hold the same tree, so both can be read here.  The root disk is
an ext4 image and needs Linux and a read-only loop mount, reusing the mount the
read-back stage already spells out rather than writing a second one.  The
initrd is a `newc` archive and needs neither, which is also the answer to why
its digest moved: the archive numbers its entries from one, so five entries
inserted in the middle renumber every entry after them.  Whether anything else
moved with them is not something to reason about, and is what this reads.

Reading two images is not booting either of them.  Nothing here starts a guest,
produces an image, or writes to one.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import stat
import sys
import tempfile
from typing import Any, Callable, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as readback
from scripts import native_shadow_boot_rootfs_mount_point_audit_arm64_v1 as mount_points


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_BOOT_VERIFIED = False

SCHEMA = "boole.native-shadow.boot-root-disk-tree-diff.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-ROOT-DISK-TREE-DIFF-ARM64-V1"
STATUS = "TWO-TREES-COMPARED-READ-ONLY-NOT-BOOT-AUTHORITY"

# Compared for every entry.  Anything a tree reader records outside this set is
# still compared, because the comparison walks the union of both sides' fields
# rather than a list written here -- a field that appears on one side only is a
# difference, not something to skip.
CHANGE_SUMMARY_LIMIT = 40


class RootDiskTreeDiffError(RuntimeError):
    """A tree cannot be read, or holds something this comparison cannot describe."""


def _digest_file(path: str) -> str:
    found = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            found.update(block)
    return found.hexdigest()


def read_extended_attributes(path: str) -> dict[str, str]:
    """Every extended attribute on one entry, as hex, without following links."""

    if not hasattr(os, "listxattr"):
        raise RootDiskTreeDiffError(
            "extended attributes cannot be read on this host; the comparison "
            "runs on Linux, where the image can be mounted"
        )
    found: dict[str, str] = {}
    for name in os.listxattr(path, follow_symlinks=False):
        found[name] = os.getxattr(path, name, follow_symlinks=False).hex()
    return found


def _entry(
    path: str,
    info: os.stat_result,
    *,
    logical: str,
    xattrs: Callable[[str], dict[str, str]],
) -> dict[str, Any]:
    bits = stat.S_IFMT(info.st_mode)
    entry: dict[str, Any] = {
        "gid": info.st_gid,
        "kind": "",
        "mode": stat.S_IMODE(info.st_mode),
        "uid": info.st_uid,
        "xattrs": xattrs(path),
    }
    if bits == stat.S_IFDIR:
        # A directory's size and link count are restatements of what it holds,
        # and what it holds is already every other row of this tree.
        entry["kind"] = "directory"
    elif bits == stat.S_IFLNK:
        entry["kind"] = "symlink"
        entry["mtime"] = int(info.st_mtime)
        entry["target"] = os.readlink(path)
    elif bits == stat.S_IFREG:
        entry["kind"] = "file"
        entry["mtime"] = int(info.st_mtime)
        entry["nlink"] = info.st_nlink
        entry["sha256"] = _digest_file(path)
        entry["size"] = info.st_size
    else:
        raise RootDiskTreeDiffError(
            f"{logical}: the tree holds an entry that is not a file, directory or symlink"
        )
    return entry


def read_tree(
    root: pathlib.Path,
    *,
    xattrs: Callable[[str], dict[str, str]] = read_extended_attributes,
) -> dict[str, Any]:
    """Read a mounted tree, root included, without ever walking through a link."""

    entries: dict[str, dict[str, Any]] = {
        "/": _entry(str(root), os.stat(root), logical="/", xattrs=xattrs)
    }
    by_inode: dict[tuple[int, int], list[str]] = {}
    pending: list[tuple[str, str]] = [(str(root), "")]
    while pending:
        directory, prefix = pending.pop()
        with os.scandir(directory) as scan:
            rows = sorted(scan, key=lambda row: row.name)
        for row in rows:
            logical = prefix + "/" + row.name
            info = row.stat(follow_symlinks=False)
            entry = _entry(row.path, info, logical=logical, xattrs=xattrs)
            if entry["kind"] == "directory":
                pending.append((row.path, logical))
            elif entry["kind"] == "file" and info.st_nlink > 1:
                by_inode.setdefault((info.st_dev, info.st_ino), []).append(logical)
            entries[logical] = entry
    groups = sorted(sorted(paths) for paths in by_inode.values() if len(paths) > 1)
    return {"entries": entries, "hardlinkGroups": groups}


def diff_trees(
    before: dict[str, dict[str, Any]], after: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    """Added, removed and changed, over the union of every field on both sides."""

    added = {path: after[path] for path in sorted(set(after) - set(before))}
    removed = {path: before[path] for path in sorted(set(before) - set(after))}
    changed: dict[str, dict[str, Any]] = {}
    for path in sorted(set(before) & set(after)):
        was, now = before[path], after[path]
        fields = {
            field: {"after": now.get(field), "before": was.get(field)}
            for field in sorted(set(was) | set(now))
            if was.get(field) != now.get(field)
        }
        if fields:
            changed[path] = fields
    return {"added": added, "changed": changed, "removed": removed}


def _added_directory_reasons(path: str, entry: dict[str, Any], row: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    if entry.get("kind") != "directory":
        reasons.append(f"{path} was added as a {entry.get('kind')} rather than a directory")
        return reasons
    wanted = int(row["mode"], 8)
    if entry.get("mode") != wanted:
        reasons.append(
            f"{path} was added with mode {entry.get('mode', 0):04o}, not {wanted:04o}"
        )
    if (entry.get("uid"), entry.get("gid")) != (0, 0):
        reasons.append(
            f"{path} was added owned by {entry.get('uid')}:{entry.get('gid')}, not root:root"
        )
    if entry.get("xattrs"):
        reasons.append(f"{path} was added carrying extended attributes")
    return reasons


def verdict(
    *,
    diff: dict[str, Any],
    rows: tuple[dict[str, Any], ...],
    hardlinkGroups: Optional[dict[str, list]] = None,
) -> dict[str, Any]:
    """Pass only when the whole difference is the five directories, each empty."""

    reasons: list[str] = []
    for path in sorted(diff["removed"]):
        reasons.append(f"{path} is present in the earlier tree and absent from the later one")
    for path in sorted(diff["changed"]):
        fields = ", ".join(sorted(diff["changed"][path]))
        reasons.append(f"{path} differs in {fields}")

    wanted = {"/" + row["path"]: row for row in rows}
    for path in sorted(set(diff["added"]) - set(wanted)):
        reasons.append(f"{path} was added and is not one of the required directories")
    for path in sorted(set(wanted) - set(diff["added"])):
        reasons.append(f"{path} is required and was not added")
    for path, row in sorted(wanted.items()):
        entry = diff["added"].get(path)
        if entry is not None:
            reasons.extend(_added_directory_reasons(path, entry, row))

    if hardlinkGroups is not None and hardlinkGroups["before"] != hardlinkGroups["after"]:
        reasons.append("the hard link grouping is not the same in both trees")

    return {"ok": not reasons, "reasons": reasons[:CHANGE_SUMMARY_LIMIT]}


def document(
    *,
    subject: dict[str, Any],
    container: str,
    diff: dict[str, Any],
    verdict: dict[str, Any],
    hardlinkGroups: Optional[dict[str, list]] = None,
) -> dict[str, Any]:
    """What was compared, what differed, and what it is not evidence of."""

    found = {
        "activationAllowed": ACTIVATION_ALLOWED,
        "addedPaths": sorted(diff["added"]),
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": [
            "Both images were opened read-only; neither was produced, written to or booted.",
            "A difference of exactly the required directories is not a boot result.",
        ],
        "changedPaths": {path: sorted(fields) for path, fields in sorted(diff["changed"].items())},
        "container": container,
        "guestBootVerified": GUEST_BOOT_VERIFIED,
        "release": RELEASE,
        "removedPaths": sorted(diff["removed"]),
        "schema": SCHEMA,
        "status": STATUS,
        "subject": subject,
        "verdict": verdict,
    }
    if hardlinkGroups is not None:
        found["hardlinkGroups"] = hardlinkGroups
    return found


def resolve_outputs(outputs: pathlib.Path, key: str) -> pathlib.Path:
    """Which file in a produced set is which, asked of the producer authority."""

    produced = readback.output_paths(outputs)
    if key not in produced:
        raise RootDiskTreeDiffError(f"the producer writes no file called {key!r}")
    return produced[key]


def _subject(before: pathlib.Path, after: pathlib.Path) -> dict[str, Any]:
    return {
        "after": {"name": after.name, "sha256": _digest_file(str(after))},
        "before": {"name": before.name, "sha256": _digest_file(str(before))},
    }


def compare_initrds(*, before: pathlib.Path, after: pathlib.Path) -> dict[str, Any]:
    """Compare two `newc` archives.  No Linux and no root: the archive is bytes."""

    trees = [image_verify.tree_from_initrd(path.read_bytes()) for path in (before, after)]
    difference = diff_trees(trees[0], trees[1])
    return document(
        subject=_subject(before, after),
        container="initrd",
        diff=difference,
        verdict=verdict(diff=difference, rows=mount_points.required_root_directories()),
    )


def compare_root_disks(
    *, before: pathlib.Path, after: pathlib.Path, workdir: Optional[pathlib.Path] = None
) -> dict[str, Any]:
    """Mount both ext4 images read-only, read both trees, and compare them."""

    unmet = readback._unmet_requirements()
    if unmet:
        raise RootDiskTreeDiffError("reading these images requires " + "; ".join(unmet))
    for image in (before, after):
        if not image.is_file() or image.is_symlink():
            raise RootDiskTreeDiffError(f"the image to compare is absent: {image}")

    read: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(dir=workdir) as raw:
        for index, image in enumerate((before, after)):
            where = pathlib.Path(raw) / f"tree-{index}"
            where.mkdir()
            readback._run(readback.mount_argv(image=image, mountpoint=where))
            try:
                read.append(read_tree(where))
            finally:
                readback._run(readback.umount_argv(where))

    difference = diff_trees(read[0]["entries"], read[1]["entries"])
    groups = {"after": read[1]["hardlinkGroups"], "before": read[0]["hardlinkGroups"]}
    return document(
        subject=_subject(before, after),
        container="root-disk",
        diff=difference,
        verdict=verdict(
            diff=difference,
            rows=mount_points.required_root_directories(),
            hardlinkGroups=groups,
        ),
        hardlinkGroups=groups,
    )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name, help_text in (
        ("root-disks", "mount two ext4 images read-only and compare their trees"),
        ("initrds", "compare the trees inside two `newc` archives"),
    ):
        run = sub.add_parser(name, help=help_text)
        # Two produced sets, named as the producer wrote them; which file inside
        # each is the one to open is the producer authority's answer, not a name
        # repeated here and in the workflow that calls this.
        run.add_argument("--before-outputs", type=pathlib.Path, required=True)
        run.add_argument("--after-outputs", type=pathlib.Path, required=True)
        run.add_argument("--result", type=pathlib.Path)
        if name == "root-disks":
            run.add_argument("--workdir", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    key = "root-disk" if options.command == "root-disks" else "initrd"
    try:
        before = resolve_outputs(options.before_outputs, key)
        after = resolve_outputs(options.after_outputs, key)
        if options.command == "root-disks":
            found = compare_root_disks(before=before, after=after, workdir=options.workdir)
        else:
            found = compare_initrds(before=before, after=after)
    except (RootDiskTreeDiffError, image_verify.ImageVerifyError, OSError, ValueError) as exc:
        print(f"native-shadow tree comparison: FAIL: {exc}", file=sys.stderr)
        return 1

    if options.result:
        options.result.parent.mkdir(parents=True, exist_ok=True)
        options.result.write_bytes(image_verify.canonical_json(found))
    print(f"container: {found['container']}")
    print(f"added:   {found['addedPaths']}")
    print(f"removed: {found['removedPaths']}")
    print(f"changed: {sorted(found['changedPaths'])}")
    for reason in found["verdict"]["reasons"]:
        print(f"  {reason}")
    passed = found["verdict"]["ok"]
    print(f"native-shadow tree comparison: {'PASS' if passed else 'FAIL'}")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
