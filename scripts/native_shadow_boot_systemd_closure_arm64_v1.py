#!/usr/bin/env python3
"""Audit the systemd guest closure the frozen source lock declares.

The boot-artifact plan scaffold left three input slots null.  Two are answered:
the image builder authority pinned the tools, and the kernel extraction produced
the image.  This answers the third, `systemdGuestClosure`, and it answers it
entirely from files -- nothing here starts a machine or runs a guest.

The question is narrow and mechanical.  If this rootfs were assembled, would
PID 1 be real systemd rather than a shim, and would systemd start the launcher?
Both halves are chains of file facts: a package ships `/usr/sbin/init` as a
symlink, the symlink resolves to a binary another package ships, that binary is
an AArch64 ELF; separately a unit file names an executable, an enablement
symlink sits in the directory the unit's own `WantedBy` asks for, and the
sysusers and tmpfiles fragments that unit orders itself after are present.

Two tiers of evidence are reported separately, and that separation is the point.
Everything read from the tracked source lock is re-provable by CI: the guest
files are in the repository, so the whole chain source file -> digest -> lock
entry can be checked on a clean runner.  Everything read out of the package
content store cannot be re-proved there -- those bytes are gitignored and the
runner has never seen them.  Averaging the two into a single boolean would let
the weaker half borrow the stronger half's credibility, so the result keeps them
apart and says which is which.

An audited closure is not a running system.  No boundary here becomes true
except the one this step actually establishes.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import io
import json
import lzma
import pathlib
import posixpath
import subprocess
import sys
import tarfile
from typing import Any, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload
from scripts.native_shadow_boot_rootfs_payload_acquire_arm64_v1 import canonical_json
from scripts.native_shadow_boot_kernel_extract_arm64_v1 import ar_member


REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native" / "containment"
SOURCE_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
RESULT_PATH = CONTAINMENT / "native-shadow-boot-systemd-closure-result-arm64-v1.json"
UNIT_SOURCE_PATH = REPO / "native" / "systemd" / "boole-native-shadow-launcher.service"

RELEASE = "NATIVE-SHADOW-BOOT-SYSTEMD-CLOSURE-ARM64-V1"
RESULT_SCHEMA = "boole.native-shadow.boot-systemd-closure-result.arm64.v1"
RESULT_STATUS = "SYSTEMD-GUEST-CLOSURE-AUDITED-NOT-BOOT-AUTHORITY"

# The scaffold named this string for its systemdGuestClosure slot. Answering a
# slot means using the name the slot was written with.
CLOSURE_FORMAT = "systemd-rootfs-closure-authority-v1"

# The lock is pinned by digest, not by path, so rewriting that document cannot
# quietly change what this audit is auditing.
SOURCE_LOCK_SHA256 = "9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf"

LAUNCHER_GUEST_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"
LAUNCHER_UNIT_PATH = "/usr/lib/systemd/system/boole-native-shadow-launcher.service"
SYSUSERS_PATH = "/usr/lib/sysusers.d/boole-native-shadow.conf"
TMPFILES_PATH = "/usr/lib/tmpfiles.d/boole-native-shadow.conf"
MACHINE_ID_PATH = "/etc/machine-id"
ENABLEMENT_LINK_PATH = (
    "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service"
)

# Ubuntu 24.04 is usr-merged, so the init symlink lives under /usr/sbin and its
# target is relative. Both facts were read off the package rather than assumed.
INIT_LINK_PATH = "/usr/sbin/init"
PID1_PATH = "/usr/lib/systemd/systemd"

# systemd alone does not make PID 1 systemd: systemd-sysv is the package that
# installs the init symlink. Requiring both is what turns "systemd is present"
# into "systemd is what starts".
REQUIRED_PACKAGES = ("init-system-helpers", "systemd", "systemd-sysv", "udev")

# The 191 frozen packages are not uniform: 188 carry .zst members, two carry .xz,
# and linux-modules carries uncompressed .tar. A reader that knows only some of
# those variants does not fail loudly -- it silently skips a package, which is
# worse than failing. All four forms are listed for that reason.
CONTROL_MEMBER_NAMES = (
    "control.tar",
    "control.tar.gz",
    "control.tar.xz",
    "control.tar.zst",
)
DATA_MEMBER_NAMES = ("data.tar", "data.tar.gz", "data.tar.xz", "data.tar.zst")

# e_machine sits at a defined offset in the ELF header. Read there, not searched
# for, the same discipline the kernel magic check uses.
ELF_MACHINE_OFFSET = 18
ELF_MACHINE_AARCH64 = 183

REPLAY_NODE_MARKER = "replay-node"


class SystemdClosureError(RuntimeError):
    pass


def load_source_lock(path: pathlib.Path = SOURCE_LOCK_PATH) -> dict[str, Any]:
    """Read the boot source lock, refusing bytes that are not the pinned ones."""
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != SOURCE_LOCK_SHA256:
        raise SystemdClosureError(
            f"boot source lock digest is {digest}, expected {SOURCE_LOCK_SHA256}"
        )
    return json.loads(raw.decode("utf-8"))


def tracked_file(lock: dict[str, Any], logical_path: str) -> dict[str, Any]:
    """Return the tracked-file entry for a guest path.

    Absence is an error rather than a None, because every caller here is asking
    a question whose only safe answer is the entry itself.
    """
    for entry in lock.get("trackedFiles", []):
        if entry.get("logicalPath") == logical_path:
            return entry
    raise SystemdClosureError(f"source lock declares no tracked file at {logical_path}")


def derived_symlink(lock: dict[str, Any], logical_path: str) -> dict[str, Any]:
    for entry in lock.get("derivedEntries", []):
        if entry.get("logicalPath") == logical_path and entry.get("kind") == "symlink":
            return entry
    raise SystemdClosureError(f"source lock declares no symlink at {logical_path}")


def systemd_logical_roots(lock: dict[str, Any]) -> list[str]:
    roots: set[str] = set()
    for group in lock.get("closureRoots", []):
        roots.update(group.get("logicalRoots", []))
    return sorted(roots)


def replay_node_references(lock: dict[str, Any]) -> list[str]:
    """Report every declared guest path that mentions a replay-node service.

    The image is supposed to contain the launcher and nothing that replays
    on its own. A source path is checked as well as a logical one: a unit named
    innocuously that is copied from a replay-node file is still a replay node.
    """
    found: set[str] = set()
    for entry in lock.get("trackedFiles", []):
        haystack = f"{entry.get('logicalPath', '')} {entry.get('sourcePath', '')}"
        if REPLAY_NODE_MARKER in haystack:
            found.add(entry.get("logicalPath", ""))
    for entry in lock.get("derivedEntries", []):
        haystack = f"{entry.get('logicalPath', '')} {entry.get('target', '')}"
        if REPLAY_NODE_MARKER in haystack:
            found.add(entry.get("logicalPath", ""))
    return sorted(found)


def unit_field(text: str, key: str) -> str:
    """Return the single value of a systemd unit directive.

    A repeated directive is refused rather than resolved. systemd's own rules for
    a repeated key are per-directive -- some append, some reset, some are an
    error -- so guessing one here would make this audit disagree with the thing
    it claims to be auditing.
    """
    values = [
        line.split("=", 1)[1].strip()
        for line in text.splitlines()
        if line.strip().startswith(f"{key}=")
    ]
    if not values:
        raise SystemdClosureError(f"unit declares no {key}")
    if len(values) > 1:
        raise SystemdClosureError(
            f"unit declares {key} {len(values)} times; refusing to guess which one wins"
        )
    return values[0]


def resolve_link(link_path: str, target: str) -> str:
    """Resolve a symlink target against the directory the link itself lives in.

    Relative targets are the normal case here, and joining one onto the wrong
    base lands somewhere else entirely. A target that would climb above the root
    is refused instead of being clamped: clamping turns an escape into a
    plausible-looking path.
    """
    if not link_path.startswith("/"):
        raise SystemdClosureError(f"link path {link_path!r} is not absolute")
    if target.startswith("/"):
        stack: list[str] = []
    else:
        stack = [part for part in posixpath.dirname(link_path).split("/") if part]
    for part in target.split("/"):
        if part in ("", "."):
            continue
        if part == "..":
            if not stack:
                raise SystemdClosureError(
                    f"symlink target {target!r} escapes the root from {link_path!r}"
                )
            stack.pop()
            continue
        stack.append(part)
    return "/" + "/".join(stack)


def elf_machine(data: bytes) -> int:
    if not data.startswith(b"\x7fELF"):
        raise SystemdClosureError("pid 1 candidate is not an ELF binary")
    end = ELF_MACHINE_OFFSET + 2
    if len(data) < end:
        raise SystemdClosureError("ELF header is too short to hold e_machine")
    return int.from_bytes(data[ELF_MACHINE_OFFSET:end], "little")


def _decompress(name: str, raw: bytes, zstd_path: Optional[pathlib.Path]) -> bytes:
    if name.endswith(".zst"):
        if zstd_path is None:
            raise SystemdClosureError(f"{name} needs a zstd binary and none was given")
        return subprocess.run(
            [str(zstd_path), "-d", "-c"],
            input=raw,
            stdout=subprocess.PIPE,
            check=True,
        ).stdout
    if name.endswith(".xz"):
        return lzma.decompress(raw)
    if name.endswith(".gz"):
        return gzip.decompress(raw)
    return raw


def _deb_tar(
    blob: bytes, names: tuple[str, ...], zstd_path: Optional[pathlib.Path]
) -> tarfile.TarFile:
    for name in names:
        try:
            raw = ar_member(blob, name)
        except Exception:
            continue
        return tarfile.open(fileobj=io.BytesIO(_decompress(name, raw, zstd_path)))
    raise SystemdClosureError(
        "package has none of the expected members: " + ", ".join(names)
    )


def package_identity(
    blob: bytes, zstd_path: Optional[pathlib.Path] = None
) -> dict[str, str]:
    """Return name, version and architecture from a .deb control file."""
    with _deb_tar(blob, CONTROL_MEMBER_NAMES, zstd_path) as tar:
        handle = tar.extractfile("./control")
        if handle is None:
            raise SystemdClosureError("package control member is not a regular file")
        text = handle.read().decode("utf-8", "replace")
    fields = {}
    for line in text.splitlines():
        if line.startswith(("Package:", "Version:", "Architecture:")):
            key, value = line.split(":", 1)
            fields[key.lower()] = value.strip()
    missing = {"package", "version", "architecture"} - set(fields)
    if missing:
        raise SystemdClosureError(
            "package control file is missing " + ", ".join(sorted(missing))
        )
    return {
        "architecture": fields["architecture"],
        "name": fields["package"],
        "version": fields["version"],
    }


def package_member(
    blob: bytes, member: str, zstd_path: Optional[pathlib.Path] = None
) -> tarfile.TarInfo:
    with _deb_tar(blob, DATA_MEMBER_NAMES, zstd_path) as tar:
        try:
            return tar.getmember(member)
        except KeyError as error:
            raise SystemdClosureError(f"package ships no {member}") from error


def package_member_bytes(
    blob: bytes, member: str, zstd_path: Optional[pathlib.Path] = None
) -> bytes:
    with _deb_tar(blob, DATA_MEMBER_NAMES, zstd_path) as tar:
        info = tar.getmember(member)
        if not info.isreg():
            raise SystemdClosureError(f"{member} is not a regular file")
        handle = tar.extractfile(info)
        if handle is None:
            raise SystemdClosureError(f"{member} could not be read")
        return handle.read()


@dataclasses.dataclass(frozen=True)
class LockAudit:
    """What the tracked source lock says. CI can re-prove all of it."""

    launcherUnitSha256: str
    sysusersSha256: str
    tmpfilesSha256: str
    machineIdEmpty: bool
    enablementTarget: str
    execStart: str
    wantedBy: str
    replayNodeReferences: list[str]


@dataclasses.dataclass(frozen=True)
class PackageAudit:
    """What the frozen package bytes say. CI has never seen those bytes."""

    packages: list[dict[str, str]]
    pid1Path: str
    pid1ProvidedBy: str
    pid1Sha256: str
    pid1Machine: str
    initLinkPath: str
    initLinkTarget: str
    initLinkResolvesTo: str
    initLinkProvidedBy: str


def build_result(
    *, lock_audit: LockAudit, package_audit: PackageAudit
) -> dict[str, Any]:
    if lock_audit.replayNodeReferences:
        raise SystemdClosureError(
            "the closure declares replay-node services: "
            + ", ".join(lock_audit.replayNodeReferences)
        )
    if not lock_audit.machineIdEmpty:
        raise SystemdClosureError(
            "/etc/machine-id is not empty; every image would boot with one identity"
        )
    if lock_audit.execStart != LAUNCHER_GUEST_PATH:
        raise SystemdClosureError(
            f"unit ExecStart is {lock_audit.execStart}, expected {LAUNCHER_GUEST_PATH}"
        )
    if lock_audit.enablementTarget != LAUNCHER_UNIT_PATH:
        raise SystemdClosureError(
            f"enablement symlink points at {lock_audit.enablementTarget}, "
            f"expected {LAUNCHER_UNIT_PATH}"
        )

    lock_row = dataclasses.asdict(lock_audit)
    lock_row["reproducibleInCi"] = True
    package_row = dataclasses.asdict(package_audit)
    # Said out loud in the document rather than left to a reader to work out:
    # the package bytes are gitignored, so a clean runner cannot check this half.
    package_row["reproducibleInCi"] = False

    return {
        "activationAllowed": False,
        "bootableClaim": False,
        "boundaries": {
            "bootAuthority": False,
            "guestBootVerified": False,
            "guestImageBuilt": False,
            "initrdBuilt": False,
            "launcherDeployedIntoGuest": False,
            "rootDiskBuilt": False,
            "runtimeCompatibilityVerified": False,
            "systemdGuestClosureAudited": True,
        },
        "closureFormat": CLOSURE_FORMAT,
        "lockAudit": lock_row,
        "packageAudit": package_row,
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "sourceLockSha256": SOURCE_LOCK_SHA256,
        "status": RESULT_STATUS,
    }


def seal_or_reprove(
    result: dict[str, Any], *, result_path: pathlib.Path = RESULT_PATH
) -> str:
    raw = canonical_json(result)
    if result_path.exists():
        if result_path.read_bytes() != raw:
            raise SystemdClosureError(
                "this audit disagrees with the sealed systemd closure result; "
                "report the difference, never overwrite the seal"
            )
        return "re-proved"
    payload._write_result_once(result_path, raw)
    return "sealed"


def _cas_blob(cas_roots: list[pathlib.Path], sha256: str) -> bytes:
    for root in cas_roots:
        candidate = root / sha256
        if candidate.is_file():
            return candidate.read_bytes()
    raise SystemdClosureError(f"no content store holds {sha256}")


def audit_lock(lock: dict[str, Any]) -> LockAudit:
    unit_text = UNIT_SOURCE_PATH.read_text()
    unit_entry = tracked_file(lock, LAUNCHER_UNIT_PATH)
    # The lock records a digest; the repository holds the file. Checking the two
    # against each other is what makes this half re-provable rather than asserted.
    actual = hashlib.sha256(unit_text.encode("utf-8")).hexdigest()
    if actual != unit_entry["sha256"]:
        raise SystemdClosureError(
            f"unit source hashes to {actual}, lock declares {unit_entry['sha256']}"
        )

    wanted_by = unit_field(unit_text, "WantedBy")
    expected_link = (
        f"/etc/systemd/system/{wanted_by}.wants/boole-native-shadow-launcher.service"
    )
    if expected_link != ENABLEMENT_LINK_PATH:
        raise SystemdClosureError(
            f"unit asks to be wanted by {wanted_by}, but the enablement symlink "
            f"is at {ENABLEMENT_LINK_PATH}; a unit enabled into the wrong target "
            "never starts"
        )
    link = derived_symlink(lock, ENABLEMENT_LINK_PATH)
    machine_id = tracked_file(lock, MACHINE_ID_PATH)

    return LockAudit(
        launcherUnitSha256=unit_entry["sha256"],
        sysusersSha256=tracked_file(lock, SYSUSERS_PATH)["sha256"],
        tmpfilesSha256=tracked_file(lock, TMPFILES_PATH)["sha256"],
        machineIdEmpty=machine_id["sha256"] == hashlib.sha256(b"").hexdigest(),
        enablementTarget=link["target"],
        execStart=unit_field(unit_text, "ExecStart"),
        wantedBy=wanted_by,
        replayNodeReferences=replay_node_references(lock),
    )


def audit_packages(
    lock: dict[str, Any],
    *,
    cas_roots: list[pathlib.Path],
    zstd_path: pathlib.Path,
) -> PackageAudit:
    identities: dict[str, dict[str, str]] = {}
    blobs: dict[str, bytes] = {}
    for artifact in lock["artifacts"]:
        if artifact["kind"] != "deb":
            continue
        blob = _cas_blob(cas_roots, artifact["sha256"])
        identity = package_identity(blob, zstd_path)
        identity["sha256"] = artifact["sha256"]
        identities[identity["name"]] = identity
        blobs[identity["name"]] = blob

    missing = [name for name in REQUIRED_PACKAGES if name not in identities]
    if missing:
        raise SystemdClosureError(
            "closure is missing required packages: " + ", ".join(missing)
        )

    init_info = package_member(blobs["systemd-sysv"], "." + INIT_LINK_PATH, zstd_path)
    if not init_info.issym():
        raise SystemdClosureError(f"{INIT_LINK_PATH} is not a symlink")
    resolved = resolve_link(INIT_LINK_PATH, init_info.linkname)
    if resolved != PID1_PATH:
        raise SystemdClosureError(
            f"{INIT_LINK_PATH} resolves to {resolved}, expected {PID1_PATH}"
        )

    pid1 = package_member_bytes(blobs["systemd"], "." + PID1_PATH, zstd_path)
    machine = elf_machine(pid1)
    if machine != ELF_MACHINE_AARCH64:
        raise SystemdClosureError(
            f"pid 1 e_machine is {machine}, expected {ELF_MACHINE_AARCH64} (AArch64)"
        )

    return PackageAudit(
        packages=[identities[name] for name in REQUIRED_PACKAGES],
        pid1Path=PID1_PATH,
        pid1ProvidedBy="systemd",
        pid1Sha256=hashlib.sha256(pid1).hexdigest(),
        pid1Machine="aarch64",
        initLinkPath=INIT_LINK_PATH,
        initLinkTarget=init_info.linkname,
        initLinkResolvesTo=resolved,
        initLinkProvidedBy="systemd-sysv",
    )


def audit(
    *,
    cas_roots: list[pathlib.Path],
    zstd_path: pathlib.Path,
    result_path: pathlib.Path = RESULT_PATH,
) -> tuple[dict[str, Any], str]:
    lock = load_source_lock()
    result = build_result(
        lock_audit=audit_lock(lock),
        package_audit=audit_packages(lock, cas_roots=cas_roots, zstd_path=zstd_path),
    )
    return result, seal_or_reprove(result, result_path=result_path)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check", help="report the lock-tier audit without reading packages")
    run = sub.add_parser("audit", help="audit both tiers and seal the result")
    run.add_argument("--cas", type=pathlib.Path, action="append", required=True)
    run.add_argument("--zstd", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    if args.command == "check":
        lock_audit = audit_lock(load_source_lock())
        print(
            "systemd closure lock tier: "
            f"execStart={lock_audit.execStart} "
            f"wantedBy={lock_audit.wantedBy} "
            f"machineIdEmpty={'yes' if lock_audit.machineIdEmpty else 'no'} "
            f"replayNode={len(lock_audit.replayNodeReferences)}"
        )
        return 0
    result, disposition = audit(cas_roots=args.cas, zstd_path=args.zstd)
    print(
        f"systemd closure audit: {disposition} "
        f"pid1={result['packageAudit']['pid1Sha256'][:12]} "
        f"machine={result['packageAudit']['pid1Machine']} "
        f"packages={len(result['packageAudit']['packages'])}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
