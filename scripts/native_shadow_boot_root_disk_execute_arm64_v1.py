#!/usr/bin/env python3
"""Execute the frozen ext4 root disk plan on the arm64 producer, deciding nothing.

The plan module works out what the image must be and deliberately runs nothing,
because the host that plans is not the host that builds.  This module is the
other half: it stages the tree the plan describes, checks the two tools against
the digests the plan froze, and runs the argv the plan wrote with the
environment the plan wrote.  Every value it uses comes from the plan or from the
layer; it contributes none of its own, so a difference between two independent
jobs cannot start here.

Two orderings matter and neither is cosmetic.  Entries are created in the plan's
order because `mke2fs -d` walks the staging tree with readdir and never sorts
it, so on a filesystem that returns creation order the tree it reads is the
order recorded here.  Modes and times are applied afterwards in reverse, deepest
first, because creating a child updates its parent's time and a directory whose
final mode forbids writing cannot be filled after the fact.

Ownership is not negotiable and not simulated.  `mke2fs -d` copies each staged
file's owner into the image, so the produce phase runs as root and the image is
root-owned throughout; a run that is not root stages fine and refuses to build,
rather than writing an image owned by whoever happened to invoke it.

The runner is Ubuntu 24.04 arm64, which is the same distribution the closure was
frozen from, and that similarity is the hazard rather than the convenience: the
runner ships its own copy of every soname the tools need, and the default search
would find those first.  The plan says which copy wins is a run-time fact it
cannot settle, so this module settles it -- the frozen loader runs the tool
against the frozen library directory and nothing else -- and records which
copies were named, with their digests, in the result.

This module reads the local filesystem and runs one pinned aarch64 tool.  It
fetches nothing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from typing import Any, Mapping, Optional, Sequence

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts.native_shadow_boot_root_disk_arm64_v1 import (
    CANONICAL_MTIME,
    E2FSCK_ACCEPTED_EXIT_CODES,
    LIBRARY_DIRECTORY,
    ORIGIN_FROZEN_GUEST,
    ORIGIN_WRITER_SET,
    SHARED_LIBRARIES,
    SUPERSEDED_WRITER_TIME_ENV,
    WALL_CLOCK_LOWER_BOUND,
    WRITER_LIBRARIES,
    WRITER_TIME_ENV,
    canonical_json,
    layer_entries,
    staging_entries,
)
from scripts.native_shadow_boot_root_disk_time_audit_arm64_v1 import TimeAuditError, audit


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_IMAGE_BUILT = False

ABORT_TOOL_DIGEST_MISMATCH = "tool-binary-digest-mismatch"
ABORT_OUTPUT_MISSING = "output-missing-or-empty"
ABORT_WRITER_TIME = "ext4-writer-time-unusable"
ABORT_LOADER_EVIDENCE = "loader-provenance-outside-the-frozen-closure"
ABORT_LIBRARY_VERSION_MIXED = "writer-and-checker-closures-disagree"
ABORT_FSCK = "read-only-filesystem-check-did-not-pass"
ABORT_WALL_CLOCK = "wall-clock-survived-in-the-image"

SCHEMA = "boole.native-shadow.boot-root-disk-execute-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-ROOT-DISK-EXECUTE-ARM64-V1"
STATUS = "ROOT-DISK-IMAGE-WRITTEN-FROM-THE-FROZEN-PLAN-NOT-BOOT-AUTHORITY"

# Permissive enough to fill, restrictive enough not to be the final answer: the
# real mode arrives in the second pass, once the children are in place.
BUILD_DIRECTORY_MODE = 0o755
BUILD_FILE_MODE = 0o600

# The dynamic loader is the one library that is also a program, and it is the
# one that decides where the other seven come from. It is picked out of the
# plan's own list by the prefix every loader carries, so this module keeps no
# second copy of a soname the plan already froze.
LOADER_SONAME_PREFIX = "ld-"
LIBRARY_PATH_OPTION = "--library-path"
LIBRARY_PATH_SEPARATOR = ":"


class RootDiskExecuteError(RuntimeError):
    """The plan, the layer or the tools on disk do not agree."""


def _safe_relative(path: str) -> pathlib.PurePosixPath:
    """A staging path that names something inside the staging root, or nothing."""

    if not path or path.startswith("/"):
        raise RootDiskExecuteError(f"staging path is not relative: {path!r}")
    candidate = pathlib.PurePosixPath(path)
    if candidate.is_absolute() or any(part in ("", ".", "..") for part in candidate.parts):
        raise RootDiskExecuteError(f"staging path leaves the staging root: {path!r}")
    return candidate


def _rows(plan: Mapping[str, Any]) -> list[dict[str, Any]]:
    staging = plan.get("staging")
    if not isinstance(staging, Mapping):
        raise RootDiskExecuteError("plan describes no staging tree")
    rows = staging.get("entries")
    if not isinstance(rows, list) or not rows:
        raise RootDiskExecuteError("plan stages no entries")
    return [dict(row) for row in rows]


def assert_plan_matches_layer(plan: Mapping[str, Any], layer: bytes) -> list[dict[str, Any]]:
    """The plan's metadata and the layer's contents must be the same tree."""

    if plan.get("executed"):
        raise RootDiskExecuteError("plan is already marked executed; it is not a fresh plan")
    rows = _rows(plan)
    for row in rows:
        if row.get("uid") != 0 or row.get("gid") != 0:
            raise RootDiskExecuteError(
                f"staging entry is not root-owned: {row.get('path')!r}"
            )
        if row.get("mtime") != CANONICAL_MTIME:
            raise RootDiskExecuteError(
                f"staging entry does not carry the canonical time: {row.get('path')!r}"
            )
        _safe_relative(str(row.get("path")))
    derived = staging_entries(layer_entries(layer))
    if derived != rows:
        raise RootDiskExecuteError(
            "the layer does not produce the tree this plan stages; "
            f"plan={len(rows)} entries, layer={len(derived)} entries"
        )
    return rows


def stage_tree(plan: Mapping[str, Any], layer: bytes) -> list[str]:
    """Create the staging tree the plan describes, in the plan's order."""

    rows = assert_plan_matches_layer(plan, layer)
    root = pathlib.Path(str(plan["staging"]["path"]))
    if root.exists() and any(root.iterdir()):
        raise RootDiskExecuteError(f"staging root is not empty: {root}")
    root.mkdir(parents=True, exist_ok=True)

    contents = {entry["name"]: entry["data"] for entry in layer_entries(layer)}
    created: list[str] = []
    for row in rows:
        target = root / str(_safe_relative(str(row["path"])))
        kind = row["kind"]
        if kind == "directory":
            target.mkdir(mode=BUILD_DIRECTORY_MODE)
        elif kind == "symlink":
            os.symlink(row["target"], target)
        elif kind == "file":
            target.write_bytes(contents[row["path"]])
            os.chmod(target, BUILD_FILE_MODE)
        else:
            raise RootDiskExecuteError(f"staging entry has an unknown kind: {kind!r}")
        created.append(row["path"])

    staged_as_root = os.geteuid() == 0
    for row in reversed(rows):
        target = root / str(_safe_relative(str(row["path"])))
        if row["kind"] != "symlink":
            os.chmod(target, row["mode"])
        if staged_as_root:
            os.chown(target, 0, 0, follow_symlinks=False)
        os.utime(target, (CANONICAL_MTIME, CANONICAL_MTIME), follow_symlinks=False)
    if staged_as_root:
        os.chown(root, 0, 0)
    os.utime(root, (CANONICAL_MTIME, CANONICAL_MTIME))
    return created


def assert_tools(plan: Mapping[str, Any]) -> dict[str, str]:
    """Both pinned tools must be on disk with the bytes the plan froze."""

    tools = plan.get("tools")
    if not isinstance(tools, Mapping) or not tools:
        raise RootDiskExecuteError("plan pins no tools")
    resolved: dict[str, str] = {}
    for name in sorted(tools):
        row = tools[name]
        path = pathlib.Path(str(row["path"]))
        try:
            raw = path.read_bytes()
        except OSError as exc:
            raise RootDiskExecuteError(
                f"{ABORT_TOOL_DIGEST_MISMATCH}: cannot read {name} at {path}"
            ) from exc
        if len(raw) != row["sizeBytes"]:
            raise RootDiskExecuteError(
                f"{ABORT_TOOL_DIGEST_MISMATCH}: {name} is {len(raw)} bytes, "
                f"the plan says {row['sizeBytes']}"
            )
        digest = hashlib.sha256(raw).hexdigest()
        if digest != row["sha256"]:
            raise RootDiskExecuteError(
                f"{ABORT_TOOL_DIGEST_MISMATCH}: {name} hashes to {digest}, "
                f"the plan says {row['sha256']}"
            )
        resolved[name] = digest
    return resolved


def mke2fs_command(plan: Mapping[str, Any]) -> list[str]:
    argv = plan.get("mke2fs", {}).get("argv")
    if not isinstance(argv, list) or not argv:
        raise RootDiskExecuteError("plan carries no mke2fs argv")
    return [str(item) for item in argv]


def mke2fs_environment(plan: Mapping[str, Any]) -> dict[str, str]:
    """Exactly the plan's environment: nothing from the caller leaks in."""

    env = plan.get("mke2fs", {}).get("env")
    if not isinstance(env, Mapping) or not env:
        raise RootDiskExecuteError("plan carries no mke2fs environment")
    return {str(key): str(value) for key, value in env.items()}


def _library_directory() -> pathlib.PurePosixPath:
    """The one directory both closures live in, as the plan derives it."""

    return pathlib.PurePosixPath(LIBRARY_DIRECTORY)


def _library_directory_of(tree: pathlib.Path) -> pathlib.Path:
    return tree / str(_library_directory()).lstrip("/")


def writer_library_path(writer_tree: pathlib.Path, tree: pathlib.Path) -> str:
    """The two directories the writer's loader searches, in that order.

    Order is the mechanism.  Both trees hold a `libext2fs.so.2`, and the one
    that wins is whichever directory is named first; naming them the other way
    round would run the selected writer against the build whose fixed-time flag
    is never armed, which is the sealed failure with a newer binary on top.
    """

    return LIBRARY_PATH_SEPARATOR.join(
        str(_library_directory_of(root)) for root in (writer_tree, tree)
    )


def _loader_logical_path() -> str:
    """The plan's own entry for the dynamic loader."""

    rows = [
        row for row in SHARED_LIBRARIES if row["soname"].startswith(LOADER_SONAME_PREFIX)
    ]
    if len(rows) != 1:
        raise RootDiskExecuteError("the plan does not name exactly one dynamic loader")
    return rows[0]["logicalPath"]


def resolved_libraries(tree: pathlib.Path) -> dict[str, dict[str, Any]]:
    """The copy of each pinned library that the loader will be pointed at.

    The plan lists what is needed and says which copy wins is a run-time fact it
    cannot settle.  This is the settling: the paths are recorded, with their
    digests, so a later reader can see which bytes wrote the image.
    """

    resolved: dict[str, dict[str, Any]] = {}
    for row in SHARED_LIBRARIES:
        path = tree / row["logicalPath"].lstrip("/")
        try:
            raw = path.read_bytes()
        except OSError as exc:
            raise RootDiskExecuteError(
                f"the frozen tree has no {row['soname']} at {path}"
            ) from exc
        resolved[row["soname"]] = {
            "package": row["package"],
            "path": str(path),
            "sha256": hashlib.sha256(raw).hexdigest(),
            "sizeBytes": len(raw),
        }
    return resolved


def assert_writer_tree_is_only_the_sealed_set(writer_tree: pathlib.Path) -> list[str]:
    """The writer's directory holds its own libraries and nothing else.

    Its directory is searched first, so anything sitting in it shadows the
    frozen build of the same soname -- a stray `libc.so.6` there would silently
    become the one the writer runs against, and the digest comparison that
    guards the closure only covers sonames the writer is known to need.  The
    versioned file and the soname symlink Debian ships are the same library, so
    both count as the soname they belong to.
    """

    directory = _library_directory_of(writer_tree)
    sealed = sorted(
        row["soname"] for row in WRITER_LIBRARIES if row["origin"] == ORIGIN_WRITER_SET
    )
    try:
        present = sorted(entry.name for entry in directory.iterdir())
    except OSError as exc:
        raise RootDiskExecuteError(
            f"{ABORT_LOADER_EVIDENCE}: the writer tree has no library directory "
            f"at {directory}"
        ) from exc

    claimed: dict[str, list[str]] = {soname: [] for soname in sealed}
    stray: list[str] = []
    for name in present:
        owner = next(
            (s for s in sealed if name == s or name.startswith(s + ".")), None
        )
        if owner is None:
            stray.append(name)
        else:
            claimed[owner].append(name)
    if stray:
        raise RootDiskExecuteError(
            f"{ABORT_LOADER_EVIDENCE}: the writer tree's library directory holds "
            f"{stray}, which is not part of the set sealed with the writer and "
            f"would be searched before the frozen build of the same name"
        )
    absent = [soname for soname, files in claimed.items() if not files]
    if absent:
        raise RootDiskExecuteError(
            f"{ABORT_LOADER_EVIDENCE}: the writer tree supplies no {absent}"
        )
    return sealed


def resolve_writer_libraries(
    writer_tree: pathlib.Path, tree: pathlib.Path
) -> dict[str, dict[str, Any]]:
    """The copy of each of the writer's libraries the loader will find first.

    The search is done the way the loader will do it -- writer tree, then frozen
    tree -- rather than by assuming each row is where its origin says.  A writer
    tree that is short a library still resolves, against the frozen build whose
    fixed-time flag is never armed, so the first hit is recorded with the tree it
    came out of and the mismatch is caught rather than papered over.
    """

    resolved: dict[str, dict[str, Any]] = {}
    for row in WRITER_LIBRARIES:
        member = row["logicalPath"].lstrip("/")
        for root, origin in ((writer_tree, ORIGIN_WRITER_SET), (tree, ORIGIN_FROZEN_GUEST)):
            path = root / member
            if not path.is_file():
                continue
            raw = path.read_bytes()
            resolved[row["soname"]] = {
                "origin": origin,
                "package": row["package"],
                "path": str(path),
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            }
            break
        else:
            raise RootDiskExecuteError(
                f"{ABORT_LOADER_EVIDENCE}: neither the writer tree nor the frozen "
                f"tree has {row['soname']} at {row['logicalPath']}"
            )
        if resolved[row["soname"]]["origin"] != row["origin"]:
            raise RootDiskExecuteError(
                f"{ABORT_LOADER_EVIDENCE}: {row['soname']} resolved to the "
                f"{resolved[row['soname']]['origin']} copy at "
                f"{resolved[row['soname']]['path']}, and the plan says it comes "
                f"from the {row['origin']}"
            )
    return resolved


def _closure(
    libraries: Mapping[str, Mapping[str, Any]],
    *,
    library_path: str,
    tree: pathlib.Path,
    loader_from: Mapping[str, Mapping[str, Any]],
) -> dict[str, Any]:
    soname = pathlib.PurePosixPath(_loader_logical_path()).name
    return {
        "libraries": dict(libraries),
        "libraryPath": library_path,
        "loader": dict(loader_from[soname], soname=soname),
        "tree": str(tree),
    }


def loader_evidence(tree: pathlib.Path, writer_tree: pathlib.Path) -> dict[str, Any]:
    """Which files on disk each loader was pointed at, named as such.

    There are two closures now and they are recorded apart, because what makes
    the arrangement checkable is that they differ in exactly two libraries.  A
    single merged list would still be true and would no longer show that.  Both
    are run by the frozen loader -- the writer set supplies no interpreter -- so
    the loader itself comes out of the frozen tree in both.
    """

    assert_writer_tree_is_only_the_sealed_set(writer_tree)
    checker = resolved_libraries(tree)
    writer = resolve_writer_libraries(writer_tree, tree)
    return {
        "checker": _closure(
            checker,
            library_path=str(_library_directory_of(tree)),
            tree=tree,
            loader_from=checker,
        ),
        "writer": _closure(
            writer,
            library_path=writer_library_path(writer_tree, tree),
            tree=writer_tree,
            loader_from=checker,
        ),
    }


def _assert_closure(
    closure: Mapping[str, Any], *, sonames: set, roots: list, where: str, what: str
) -> None:
    loader = closure.get("loader")
    if not isinstance(loader, Mapping):
        raise RootDiskExecuteError(
            f"{ABORT_LOADER_EVIDENCE}: no dynamic loader recorded for the {what}"
        )
    libraries = closure.get("libraries")
    if not isinstance(libraries, Mapping):
        raise RootDiskExecuteError(
            f"{ABORT_LOADER_EVIDENCE}: no library paths recorded for the {what}"
        )
    missing = sonames - set(libraries)
    if missing:
        raise RootDiskExecuteError(
            f"{ABORT_LOADER_EVIDENCE}: the {what} records no path for {sorted(missing)}"
        )
    allowed = [str(root) for root in roots]
    for name, row in sorted(list(libraries.items()) + [("the dynamic loader", loader)]):
        path = row.get("path") if isinstance(row, Mapping) else None
        if not path:
            raise RootDiskExecuteError(
                f"{ABORT_LOADER_EVIDENCE}: {name} in the {what} has no path"
            )
        digest = str(row.get("sha256", ""))
        if len(digest) != 64 or set(digest) - set("0123456789abcdef"):
            raise RootDiskExecuteError(
                f"{ABORT_LOADER_EVIDENCE}: {name} at {path} has no usable digest"
            )
        if not any(str(path).startswith(root + "/") for root in allowed):
            raise RootDiskExecuteError(
                f"{ABORT_LOADER_EVIDENCE}: {name} resolved to {path}, which is "
                f"outside {where} ({', '.join(allowed)})"
            )


def assert_loader_evidence(
    evidence: Mapping[str, Any], *, tree: pathlib.Path, writer_tree: pathlib.Path
) -> bool:
    """Every file either tool loaded is a pinned file, and is written down.

    The runner is Ubuntu 24.04 arm64 and ships its own copy of every one of
    these sonames.  A path outside the two pinned trees means the image was
    written, or read back, partly by the runner -- and no digest comparison
    downstream would show it, because the image would simply be different, or
    worse, the same by luck.
    """

    for what, roots, where, sonames in (
        (
            "checker closure",
            [tree],
            "the frozen tree",
            {row["soname"] for row in SHARED_LIBRARIES},
        ),
        (
            "writer closure",
            [writer_tree, tree],
            "the writer set and the frozen tree",
            {row["soname"] for row in WRITER_LIBRARIES},
        ),
    ):
        closure = evidence.get(what.split()[0])
        if not isinstance(closure, Mapping):
            raise RootDiskExecuteError(
                f"{ABORT_LOADER_EVIDENCE}: the evidence records no {what}"
            )
        _assert_closure(
            closure, sonames=sonames, roots=roots, where=where, what=what
        )
    return True


def assert_writer_time(env: Mapping[str, str]) -> int:
    """The writer is handed a fixed time, by the variable that arms the clamp.

    Two variables reach this writer and only one of them is enough.  The
    superseded one sets the fixed time and leaves the flag clear, which puts the
    writer back on the branch that copies each staged file's own `st_ctime` --
    the sealed failure, with a newer binary and nothing in the output to say so.
    So an environment carrying only that one is refused here rather than run.
    """

    raw = env.get(WRITER_TIME_ENV)
    if raw is None:
        superseded = env.get(SUPERSEDED_WRITER_TIME_ENV)
        detail = (
            f"it pins {SUPERSEDED_WRITER_TIME_ENV}={superseded!r}, which this writer "
            f"reads without arming the clamp"
            if superseded is not None
            else "every time field would come from the wall clock"
        )
        raise RootDiskExecuteError(
            f"{ABORT_WRITER_TIME}: the environment pins no {WRITER_TIME_ENV}; {detail}"
        )
    try:
        value = int(str(raw))
    except ValueError as exc:
        raise RootDiskExecuteError(
            f"{ABORT_WRITER_TIME}: {WRITER_TIME_ENV}={raw!r}"
        ) from exc
    if value == 0:
        raise RootDiskExecuteError(
            f"{ABORT_WRITER_TIME}: {WRITER_TIME_ENV}=0 is the library's unset sentinel, "
            f"not a time; it asks for the wall clock"
        )
    if value >= WALL_CLOCK_LOWER_BOUND:
        raise RootDiskExecuteError(
            f"{ABORT_WRITER_TIME}: {WRITER_TIME_ENV}={value} is in wall-clock range"
        )
    return value


def assert_no_version_mixing(
    writer: Mapping[str, Mapping[str, Any]],
    checker: Mapping[str, Mapping[str, Any]],
) -> None:
    """The two closures differ in exactly the libraries sealed with the writer.

    Changing the writer put a second build of `libext2fs.so.2` on the machine,
    and the failure that buys is silent: the new `mke2fs` resolving the frozen
    library finds the fixed-time flag unarmed and writes staged times again,
    producing an image that looks like every other failed one.  There is no
    output to inspect for it, so it is checked here instead.

    Both directions matter.  A writer-set library that is not the pinned build
    means the writer ran against the wrong `libext2fs`; a shared library whose
    bytes differ between the closures means the writer's tree was assembled from
    something other than the frozen guest, which is the same hazard arriving
    from the other side.
    """

    for row in WRITER_LIBRARIES:
        soname = row["soname"]
        resolved = writer.get(soname)
        if resolved is None:
            raise RootDiskExecuteError(
                f"{ABORT_LIBRARY_VERSION_MIXED}: the writer closure resolved no "
                f"{soname}"
            )
        if row["origin"] == ORIGIN_WRITER_SET:
            if resolved.get("sha256") != row["sha256"]:
                raise RootDiskExecuteError(
                    f"{ABORT_LIBRARY_VERSION_MIXED}: the writer resolved {soname} to "
                    f"{resolved.get('sha256')}, and the build sealed with it is "
                    f"{row['sha256']}"
                )
            continue
        counterpart = checker.get(soname)
        if counterpart is None:
            raise RootDiskExecuteError(
                f"{ABORT_LIBRARY_VERSION_MIXED}: the checker closure resolved no "
                f"{soname} to compare the writer's copy against"
            )
        if resolved.get("sha256") != counterpart.get("sha256"):
            raise RootDiskExecuteError(
                f"{ABORT_LIBRARY_VERSION_MIXED}: {soname} is {resolved.get('sha256')} "
                f"for the writer and {counterpart.get('sha256')} for the checkers; "
                f"outside the sealed set the two closures must be the same bytes"
            )


def fsck_passed(exit_code: int) -> bool:
    """Only zero.  One and two mean the checker changed something, which `-n` forbids."""

    return exit_code in E2FSCK_ACCEPTED_EXIT_CODES


def assert_fsck_ran(result: Mapping[str, Any]) -> bool:
    """A result with no check in it is a failure, not a pass by omission."""

    report = result.get("fsck")
    if not isinstance(report, Mapping) or "exitCode" not in report:
        raise RootDiskExecuteError(f"{ABORT_FSCK}: the checker did not run")
    if not fsck_passed(int(report["exitCode"])):
        raise RootDiskExecuteError(f"{ABORT_FSCK}: e2fsck exited {report['exitCode']}")
    return True


def run_fsck(plan: Mapping[str, Any], tree: pathlib.Path) -> dict[str, Any]:
    """Read the produced filesystem back with the frozen checker, repairing nothing."""

    contract = plan.get("e2fsck")
    if not isinstance(contract, Mapping) or not contract.get("argv"):
        raise RootDiskExecuteError(f"{ABORT_FSCK}: the plan carries no e2fsck argv")
    argv = [str(item) for item in contract["argv"]]
    forbidden = sorted(set(argv) & set(contract.get("forbiddenOptions", ())))
    if forbidden:
        raise RootDiskExecuteError(f"{ABORT_FSCK}: the argv carries {forbidden}")
    loader = tree / _loader_logical_path().lstrip("/")
    library_path = _library_directory_of(tree)
    finished = subprocess.run(  # noqa: S603 - pinned argv from the frozen plan
        [str(loader), LIBRARY_PATH_OPTION, str(library_path), *argv],
        env={str(k): str(v) for k, v in contract.get("env", {}).items()},
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    return {
        "argv": argv,
        "exitCode": finished.returncode,
        "passed": fsck_passed(finished.returncode),
        "repairOptionsUsed": False,
        "stderr": finished.stderr.decode("utf-8", "replace").strip()[:2048],
        "stdout": finished.stdout.decode("utf-8", "replace").strip()[:2048],
    }


def frozen_invocation(
    plan: Mapping[str, Any], tree: pathlib.Path, writer_tree: pathlib.Path
) -> list[str]:
    """The plan's argv, run by the frozen loader against only pinned libraries.

    The runner is Ubuntu 24.04 arm64 and ships its own copies of every one of
    these sonames, so leaving the search to the default path would let the
    runner's libraries write an image the closure never pinned.  The interpreter
    is the frozen one in both closures: the writer set supplies two libraries
    and no `ld-`, and running the selected writer under the runner's interpreter
    would put the whole search back in the runner's hands.
    """

    resolve_writer_libraries(writer_tree, tree)
    loader = tree / _loader_logical_path().lstrip("/")
    if not loader.is_file():
        raise RootDiskExecuteError(f"the frozen tree has no dynamic loader at {loader}")
    return [
        str(loader),
        LIBRARY_PATH_OPTION,
        writer_library_path(writer_tree, tree),
        *mke2fs_command(plan),
    ]


def execute(
    plan: Mapping[str, Any],
    layer: bytes,
    tree: pathlib.Path,
    writer_tree: pathlib.Path,
) -> dict[str, Any]:
    """Stage, check the tools, write the image, and report what was written."""

    if os.geteuid() != 0:
        raise RootDiskExecuteError(
            "the produce phase must run as root: mke2fs -d copies the staged owner "
            "into the image, and the frozen plan says root:root throughout"
        )
    digests = assert_tools(plan)
    evidence = loader_evidence(tree, writer_tree)
    assert_loader_evidence(evidence, tree=tree, writer_tree=writer_tree)
    assert_no_version_mixing(
        evidence["writer"]["libraries"], evidence["checker"]["libraries"]
    )
    environment = mke2fs_environment(plan)
    writer_time = assert_writer_time(environment)
    created = stage_tree(plan, layer)
    argv = frozen_invocation(plan, tree, writer_tree)
    finished = subprocess.run(  # noqa: S603 - pinned argv from the frozen plan
        argv,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    if finished.returncode != 0:
        raise RootDiskExecuteError(
            f"mke2fs exited {finished.returncode}: "
            f"{finished.stderr.decode('utf-8', 'replace').strip()}"
        )
    image = pathlib.Path(str(plan["mke2fs"]["argv"][-2]))
    if not image.is_file():
        raise RootDiskExecuteError(f"{ABORT_OUTPUT_MISSING}: {image} was not written")
    raw = image.read_bytes()
    if not raw:
        raise RootDiskExecuteError(f"{ABORT_OUTPUT_MISSING}: {image} is zero bytes")
    try:
        times = audit(raw)
    except TimeAuditError as exc:
        raise RootDiskExecuteError(f"{ABORT_WALL_CLOCK}: {exc}") from exc
    if not times["passed"]:
        first = times["violations"][0]
        raise RootDiskExecuteError(
            f"{ABORT_WALL_CLOCK}: {times['violationCount']} timestamps are outside "
            f"{times['allowedTimestamps']}; {first['field']} in {first['where']} "
            f"is {first['value']}"
        )
    fsck = run_fsck(plan, tree)
    result = {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": {
            "guestBootVerified": False,
            "guestImageBuilt": GUEST_IMAGE_BUILT,
            "rootDiskBuilt": True,
            "runtimeCompatibilityVerified": False,
        },
        "executed": True,
        "fsck": fsck,
        "image": {
            "name": image.name,
            "sha256": hashlib.sha256(raw).hexdigest(),
            "sizeBytes": len(raw),
        },
        "loaderEvidence": evidence,
        "release": RELEASE,
        # Kept under the name the earlier results used, and now the writer's
        # closure: these are the libraries that wrote the image.  The checker's
        # are alongside them in the evidence rather than merged in, because the
        # two agreeing everywhere except the sealed set is the property.
        "resolvedLibraries": evidence["writer"]["libraries"],
        "schema": SCHEMA,
        "stagedEntryCount": len(created),
        "status": STATUS,
        "timeAudit": times,
        "toolDigests": digests,
        "writerTime": writer_time,
    }
    assert_fsck_ran(result)
    return result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run", help="execute a frozen root disk plan")
    run.add_argument("--plan", type=pathlib.Path, required=True)
    run.add_argument("--layer", type=pathlib.Path, required=True)
    run.add_argument("--tree", type=pathlib.Path, required=True)
    run.add_argument("--writer-tree", type=pathlib.Path, required=True)
    run.add_argument("--output", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        plan = json.loads(args.plan.read_text(encoding="utf-8"))
        result = execute(plan, args.layer.read_bytes(), args.tree, args.writer_tree)
    except (RootDiskExecuteError, OSError, ValueError) as exc:
        print(f"root-disk-execute: {exc}", file=sys.stderr)
        return 1
    args.output.write_bytes(canonical_json(result))
    print(
        f"root-disk {result['image']['name']} bytes={result['image']['sizeBytes']} "
        f"sha256={result['image']['sha256']}"
    )
    print(f"bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
