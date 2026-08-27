"""Unpack the sealed ext4 writer set into a tree of its own.

The image writer was replaced because the frozen one cannot be made
deterministic: it copies each staged file's own `st_ctime` into the inode and
no environment variable reaches that branch.  The replacement is added as a
production tool and nothing else moves -- the 191 packages the guest is built
from are neither replaced nor deleted, and the inspector and the read-only
checker stay on the frozen build so the image is judged by tools that did not
write it.

That boundary is a claim about two directories, so this builds the second one.
Exactly three files come out of the two sealed packages: the writer, and the
two libraries its own `Pre-Depends` pins to the writer's version.  The five
remaining libraries it loads are the frozen guest's and are not copied here;
the loader finds them by being pointed at the frozen tree second.

Each library is written under its soname rather than as the versioned name
plus a link, because a link is one more thing that can point elsewhere and the
tree is supposed to hold nothing but the set.  The bytes are the ones the
symlink in the package resolves to, and every one of the three is re-hashed
against the digest the selection record measured before it is written.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import pathlib
import subprocess
import sys
import tarfile
from typing import Any, Mapping, Optional, Sequence

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
SELECTION_RECORD_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json"
)
DATA_MEMBER_SUFFIX = ".zst"
DATA_MEMBER_PREFIX = "data.tar"
WRITER_TREE_PATH = "usr/sbin/mke2fs"


class WriterTreeError(RuntimeError):
    """Raised when the tree cannot be built from exactly the sealed bytes."""


def selection_record() -> dict[str, Any]:
    """The record that measured these binaries, read rather than paraphrased."""

    try:
        return json.loads(SELECTION_RECORD_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise WriterTreeError("the e2fsprogs selection record is unreadable") from exc


def sealed_pins() -> dict[str, Any]:
    """What to take, from which package, and what each has to hash to.

    The writer's member path comes from the selection record because that is
    the member the digest was measured from; the digests themselves come from
    the plan, which is what the executor re-checks the files against.  A pin
    the two disagreed about would be caught here rather than at production
    time.
    """

    measured = selection_record()["controls"]["positive"]["writer"]
    if measured["sha256"] != root_disk.MKE2FS_SHA256:
        raise WriterTreeError(
            "the plan and the selection record pin different writers: "
            f"{root_disk.MKE2FS_SHA256} against {measured['sha256']}"
        )
    return {
        "libraries": [
            {
                "logicalPath": row["logicalPath"],
                "packageSha256": root_disk.WRITER_LIBRARY_PACKAGE_SHA256,
                "sha256": row["sha256"],
                "sizeBytes": row["sizeBytes"],
                "soname": row["soname"],
            }
            for row in root_disk.WRITER_LIBRARIES
            if row["origin"] == root_disk.ORIGIN_WRITER_SET
        ],
        "writer": {
            "memberPath": "." + str(measured["memberPath"]),
            "packageSha256": root_disk.WRITER_PACKAGE_SHA256,
            "sha256": root_disk.MKE2FS_SHA256,
            "sizeBytes": root_disk.MKE2FS_SIZE_BYTES,
            "treePath": WRITER_TREE_PATH,
        },
    }


def _ar_members(archive: bytes) -> dict[str, bytes]:
    """Read a .deb's `ar` members without shelling out to whatever is on PATH."""

    if not archive.startswith(b"!<arch>\n"):
        raise WriterTreeError("not an ar archive: the global header is missing")
    members: dict[str, bytes] = {}
    offset = 8
    while offset + 60 <= len(archive):
        header = archive[offset : offset + 60]
        if header[58:60] != b"`\n":
            raise WriterTreeError(f"ar header at offset {offset} is malformed")
        name = header[0:16].decode("ascii", "replace").strip().rstrip("/")
        try:
            size = int(header[48:58].decode("ascii").strip())
        except ValueError as exc:
            raise WriterTreeError(
                f"ar header at offset {offset} has an unreadable size"
            ) from exc
        start = offset + 60
        members[name] = archive[start : start + size]
        offset = start + size + (size % 2)
    return members


def _data_tar(package: bytes, zstd: pathlib.Path) -> bytes:
    members = _ar_members(package)
    name = next(
        (
            key
            for key in members
            if key.startswith(DATA_MEMBER_PREFIX) and key.endswith(DATA_MEMBER_SUFFIX)
        ),
        None,
    )
    if name is None:
        raise WriterTreeError(
            "the package carries no zstd data member: " + ", ".join(sorted(members))
        )
    completed = subprocess.run(
        [str(zstd), "-d", "-c"], input=members[name], capture_output=True, check=False
    )
    if completed.returncode != 0:
        raise WriterTreeError(
            f"zstd refused the data member: "
            f"{completed.stderr.decode('utf-8', 'replace').strip()}"
        )
    return completed.stdout


def _package_bytes(cas_roots: Sequence[pathlib.Path], sha256: str) -> bytes:
    for root in cas_roots:
        candidate = pathlib.Path(root) / "sha256" / sha256
        if candidate.is_file():
            raw = candidate.read_bytes()
            found = hashlib.sha256(raw).hexdigest()
            if found != sha256:
                raise WriterTreeError(
                    f"the stored package hashes to {found}, the pin says {sha256}"
                )
            return raw
    raise WriterTreeError(f"the content-addressed store holds no object {sha256}")


def _resolved_member(archive: tarfile.TarFile, path: str) -> tarfile.TarInfo:
    """The regular file a member names, following at most one soname link.

    A runtime package ships the soname as a link to the versioned build, so the
    link has to be followed to reach any bytes at all.  It is followed exactly
    one hop and only within its own directory: a target that climbs out of it
    is how a package would reach a file the pins never described.
    """

    try:
        info = archive.getmember(path)
    except KeyError as exc:
        raise WriterTreeError(f"the package carries no member {path}") from exc
    if info.isfile():
        return info
    if not info.issym():
        raise WriterTreeError(f"{path} is neither a regular file nor a link")
    target = info.linkname
    if target.startswith("/") or "/" in target:
        raise WriterTreeError(
            f"{path} links to {target}, which is outside its own directory"
        )
    # Joined as text: a .deb names its members `./usr/...` and `PurePosixPath`
    # would drop the leading `./`, leaving a name the archive does not hold.
    resolved = path.rsplit("/", 1)[0] + "/" + target
    try:
        followed = archive.getmember(resolved)
    except KeyError as exc:
        raise WriterTreeError(
            f"{path} links to {target}, which the package does not carry"
        ) from exc
    if not followed.isfile():
        raise WriterTreeError(f"{path} links to {target}, which is not a regular file")
    return followed


def _member_bytes(archive: tarfile.TarFile, path: str, pin: Mapping[str, Any]) -> bytes:
    info = _resolved_member(archive, path)
    extracted = archive.extractfile(info)
    if extracted is None:
        raise WriterTreeError(f"{path} yielded no content")
    raw = extracted.read()
    if len(raw) != pin["sizeBytes"]:
        raise WriterTreeError(
            f"{path} is {len(raw)} bytes, the pin says {pin['sizeBytes']}"
        )
    found = hashlib.sha256(raw).hexdigest()
    if found != pin["sha256"]:
        raise WriterTreeError(f"{path} hashes to {found}, the pin says {pin['sha256']}")
    return raw


def _write(path: pathlib.Path, raw: bytes, *, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    path.chmod(mode)


def _assert_tree_is_empty(writer_tree: pathlib.Path) -> None:
    """Nothing may be inherited: the tree is this run's, or it is not the set."""

    if not writer_tree.exists():
        return
    stray = sorted(
        str(path.relative_to(writer_tree))
        for path in writer_tree.rglob("*")
        if path.is_file() or path.is_symlink()
    )
    if stray:
        raise WriterTreeError(
            f"the writer tree already holds {', '.join(stray)}; it is built per run "
            "and an inherited file is not part of the sealed set"
        )


def materialize(
    *,
    cas_roots: Sequence[pathlib.Path],
    zstd: pathlib.Path,
    writer_tree: pathlib.Path,
    pins: Optional[Mapping[str, Any]] = None,
) -> dict[str, Any]:
    """Build the writer tree and return what was written, with digests."""

    settled = sealed_pins() if pins is None else pins
    _assert_tree_is_empty(writer_tree)
    writer_pin = settled["writer"]
    package = _package_bytes(cas_roots, writer_pin["packageSha256"])
    with tarfile.open(fileobj=io.BytesIO(_data_tar(package, zstd))) as archive:
        raw = _member_bytes(archive, writer_pin["memberPath"], writer_pin)
    written = writer_tree / writer_pin["treePath"]
    _write(written, raw, mode=0o755)

    libraries: list[dict[str, Any]] = []
    grouped: dict[str, list[Mapping[str, Any]]] = {}
    for row in settled["libraries"]:
        grouped.setdefault(row["packageSha256"], []).append(row)
    for package_sha256, rows in sorted(grouped.items()):
        blob = _package_bytes(cas_roots, package_sha256)
        with tarfile.open(fileobj=io.BytesIO(_data_tar(blob, zstd))) as archive:
            for row in rows:
                member = "." + row["logicalPath"]
                raw = _member_bytes(archive, member, row)
                path = writer_tree / row["logicalPath"].lstrip("/")
                _write(path, raw, mode=0o644)
                libraries.append(
                    {
                        "packageSha256": package_sha256,
                        "path": str(path),
                        "sha256": row["sha256"],
                        "sizeBytes": row["sizeBytes"],
                        "soname": row["soname"],
                    }
                )
    return {
        "libraries": sorted(libraries, key=lambda row: row["soname"]),
        "tree": str(writer_tree),
        "writer": {
            "packageSha256": writer_pin["packageSha256"],
            "path": str(written),
            "sha256": writer_pin["sha256"],
            "sizeBytes": writer_pin["sizeBytes"],
        },
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cas", type=pathlib.Path, action="append", required=True)
    parser.add_argument("--zstd", type=pathlib.Path, required=True)
    parser.add_argument("--writer-tree", type=pathlib.Path, required=True)
    parser.add_argument("--receipt", type=pathlib.Path)
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        receipt = materialize(
            cas_roots=args.cas, zstd=args.zstd, writer_tree=args.writer_tree
        )
    except WriterTreeError as exc:
        print(f"writer tree: {exc}", file=sys.stderr)
        return 1
    raw = root_disk.canonical_json(receipt)
    if args.receipt is not None:
        args.receipt.write_bytes(raw)
    sys.stdout.write(raw.decode("utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
