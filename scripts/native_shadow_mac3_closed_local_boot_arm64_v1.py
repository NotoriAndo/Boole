"""Drives the one MAC.3 closed-local boot on a development Mac.

The Swift host in `native/mac3/` builds and runs the machine. This module is
what decides *whether* it may run at all: it reads the frozen qualification,
hashes the two artifacts it is about to hand over, refuses if an attempt has
already been spent, builds and ad-hoc-signs the host, and afterwards reads the
console transcript into a verdict for each pre-registered condition.

Two things are deliberate. The kernel command line is read out of the frozen
record rather than written here, so there is only one place it can be changed
and that place is gated. And the judging rules take a transcript and some
digests and return verdicts -- no filesystem, no Mac -- so the part that decides
whether the run passed is tested on every push by a runner that cannot boot
anything.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json"
)
HOST_SOURCE_PATH = REPO / "native/mac3/boole-mac3-closed-local-boot.swift"
ENTITLEMENTS_PATH = REPO / "native/mac3/boole-mac3-closed-local-boot.entitlements"

# Ad-hoc. A development Mac running a closed-local boot has no business holding
# a release identity, and `-` is the only identity that cannot be one.
CODESIGN_IDENTITY = "-"
RUNS_ALLOWED = 1
BOOT_TIMEOUT_SECONDS = 180
READ_CHUNK_BYTES = 4 * 1024 * 1024


class RefusedError(RuntimeError):
    """Raised where continuing would spend or misreport the one attempt."""


def qualification() -> dict:
    return json.loads(QUALIFICATION_PATH.read_text(encoding="utf-8"))


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(READ_CHUNK_BYTES)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def expected_digests() -> dict:
    subject = qualification()["subject"]
    return {
        "kernel": subject["kernel"]["sha256"],
        "rootDisk": subject["rootDisk"]["sha256"],
    }


def assert_file_matches(path: pathlib.Path, expected: str, role: str) -> str:
    if not path.is_file():
        raise RefusedError("%s is not a file: %s" % (role, path))
    found = sha256_file(path)
    if found != expected:
        raise RefusedError(
            "%s digest mismatch: expected %s, read %s" % (role, expected, found)
        )
    return found


def assert_record_has_an_attempt_left(record: dict) -> None:
    performed = record["runsPerformed"]
    if performed >= record["runsAllowed"] or performed >= RUNS_ALLOWED:
        raise RefusedError(
            "the qualification records %d run(s) already performed of %d allowed"
            % (performed, record["runsAllowed"])
        )


def assert_no_run_has_been_spent(receipt_path: pathlib.Path) -> None:
    if receipt_path.exists():
        raise RefusedError(
            "a run receipt already exists at %s; the one attempt is spent"
            % receipt_path
        )


def assert_attachment_is_read_only(root_disk_row: dict) -> None:
    if not root_disk_row.get("attachedReadOnly"):
        raise RefusedError("the sealed image was not attached read-only")


def swiftc_argv(source: pathlib.Path, binary: pathlib.Path) -> list:
    return [
        "swiftc",
        "-O",
        "-framework",
        "Virtualization",
        str(source),
        "-o",
        str(binary),
    ]


def codesign_argv(binary: pathlib.Path) -> list:
    return [
        "codesign",
        "--force",
        "-s",
        CODESIGN_IDENTITY,
        "--entitlements",
        str(ENTITLEMENTS_PATH),
        str(binary),
    ]


def host_argv(
    host: pathlib.Path,
    kernel: pathlib.Path,
    root_disk: pathlib.Path,
    console: pathlib.Path,
    receipt: pathlib.Path,
    dry_run: bool = False,
) -> list:
    record = qualification()
    digests = expected_digests()
    argv = [
        str(host),
        "--kernel",
        str(kernel),
        "--kernel-sha256",
        digests["kernel"],
        "--root-disk",
        str(root_disk),
        "--root-disk-sha256",
        digests["rootDisk"],
        "--cmdline",
        record["boot"]["kernelCommandLine"],
        "--console",
        str(console),
        "--receipt",
        str(receipt),
        "--timeout",
        str(BOOT_TIMEOUT_SECONDS),
    ]
    if dry_run:
        argv.append("--dry-run")
    return argv


# --- judging -----------------------------------------------------------------
#
# Each rule returns (met, evidence). They are written against a transcript and a
# pair of digests so that the decision procedure is testable without a Mac.

ROOT_MOUNT_MARKERS = (
    "EXT4-fs (vda): mounted filesystem",
    "VFS: Mounted root",
)
PID1_MARKER = "systemd[1]:"
PANIC_MARKER = "Kernel panic"


def _loads_the_converged_image(transcript, before, after, receipt):
    if receipt.get("dryRun"):
        return False, "the receipt is from a dry run; no machine was started"
    digests = expected_digests()
    kernel = receipt.get("kernel", {}).get("sha256")
    root_disk = receipt.get("rootDisk", {}).get("sha256")
    if kernel != digests["kernel"] or root_disk != digests["rootDisk"]:
        return False, "the host reported kernel %s and root disk %s" % (
            kernel,
            root_disk,
        )
    return True, "the host re-hashed both files and read the sealed digests"


def _closed_local_configuration(transcript, before, after, receipt):
    machine = receipt.get("machine", {})
    problems = []
    for field in ("networkDevices", "sharedDirectories", "socketDevices"):
        if machine.get(field, -1) != 0:
            problems.append("%s=%s" % (field, machine.get(field)))
    if machine.get("storageDevices") != 1:
        problems.append("storageDevices=%s" % machine.get("storageDevices"))
    if not receipt.get("rootDisk", {}).get("attachedReadOnly"):
        problems.append("attachedReadOnly=false")
    if problems:
        return False, "the built machine reported " + ", ".join(problems)
    return True, "no network device, no shared directory, one read-only disk"


def _kernel_reaches_its_root_filesystem(transcript, before, after, receipt):
    for marker in ROOT_MOUNT_MARKERS:
        if marker in transcript:
            return True, "the transcript contains %r" % marker
    return False, "the transcript never reports the root filesystem mounted"


def _guest_systemd_is_pid_1(transcript, before, after, receipt):
    if PANIC_MARKER in transcript:
        return False, "the transcript contains %r" % PANIC_MARKER
    if PID1_MARKER not in transcript:
        return False, "the transcript never shows %r" % PID1_MARKER
    return True, "the transcript shows %r" % PID1_MARKER


def _sealed_image_unchanged_after_the_run(transcript, before, after, receipt):
    if not before or not after:
        return False, "the image was not hashed on both sides of the run"
    if before != after:
        return False, "the image read %s before and %s after" % (before, after)
    return True, "the image hashed to %s before and after" % before


def _console_transcript_captured_and_hashed(transcript, before, after, receipt):
    if not transcript.strip():
        return False, "the console transcript is empty"
    return True, "%d bytes, sha256 %s" % (
        len(transcript.encode("utf-8")),
        hashlib.sha256(transcript.encode("utf-8")).hexdigest(),
    )


RULES = {
    "loads-the-converged-image": _loads_the_converged_image,
    "closed-local-configuration": _closed_local_configuration,
    "kernel-reaches-its-root-filesystem": _kernel_reaches_its_root_filesystem,
    "guest-systemd-is-pid-1": _guest_systemd_is_pid_1,
    "sealed-image-unchanged-after-the-run": _sealed_image_unchanged_after_the_run,
    "console-transcript-captured-and-hashed": _console_transcript_captured_and_hashed,
}


def judge_pass_conditions(
    transcript: str,
    root_disk_digest_before: str,
    root_disk_digest_after: str,
    receipt: dict,
) -> list:
    rows = []
    for condition in qualification()["passConditions"]:
        rule = RULES.get(condition["id"])
        if rule is None:
            raise RefusedError(
                "the frozen record has a condition with no rule to judge it: %s"
                % condition["id"]
            )
        met, evidence = rule(
            transcript, root_disk_digest_before, root_disk_digest_after, receipt
        )
        rows.append(
            {
                "id": condition["id"],
                "condition": condition["condition"],
                "judgedBy": condition["judgedBy"],
                "evidence": evidence,
                "verdict": "MET" if met else "NOT MET",
            }
        )
    return rows


def overall_verdict(rows: list) -> bool:
    return bool(rows) and all(row["verdict"] == "MET" for row in rows)


# --- commands ----------------------------------------------------------------


def _run(argv: list) -> subprocess.CompletedProcess:
    print("+ " + " ".join(argv))
    return subprocess.run(argv, check=True)


def build_and_sign(work: pathlib.Path) -> pathlib.Path:
    binary = work / "boole-mac3-closed-local-boot"
    _run(swiftc_argv(HOST_SOURCE_PATH, binary))
    _run(codesign_argv(binary))
    return binary


def command_prepare(args: argparse.Namespace) -> int:
    """Build, sign and dry-run. Costs nothing against the one attempt."""
    work = pathlib.Path(args.work).resolve()
    work.mkdir(parents=True, exist_ok=True)
    digests = expected_digests()
    assert_file_matches(pathlib.Path(args.kernel), digests["kernel"], "guest-kernel")
    assert_file_matches(
        pathlib.Path(args.root_disk), digests["rootDisk"], "guest-root-disk"
    )
    binary = build_and_sign(work)
    _run(
        host_argv(
            host=binary,
            kernel=pathlib.Path(args.kernel),
            root_disk=pathlib.Path(args.root_disk),
            console=work / "DRY-RUN-CONSOLE.log",
            receipt=work / "DRY-RUN-RECEIPT.json",
            dry_run=True,
        )
    )
    print("prepare: ok")
    return 0


def command_qualify(args: argparse.Namespace) -> int:
    """The one attempt. Refuses rather than repeats."""
    work = pathlib.Path(args.work).resolve()
    work.mkdir(parents=True, exist_ok=True)
    receipt_path = work / "RUN-RECEIPT.json"
    result_path = work / "QUALIFICATION-RESULT.json"

    record = qualification()
    assert_record_has_an_attempt_left(record)
    assert_no_run_has_been_spent(receipt_path)
    assert_no_run_has_been_spent(result_path)

    kernel = pathlib.Path(args.kernel)
    root_disk = pathlib.Path(args.root_disk)
    digests = expected_digests()
    assert_file_matches(kernel, digests["kernel"], "guest-kernel")
    before = assert_file_matches(root_disk, digests["rootDisk"], "guest-root-disk")

    binary = pathlib.Path(args.host) if args.host else build_and_sign(work)
    console = work / "CONSOLE.log"
    _run(
        host_argv(
            host=binary,
            kernel=kernel,
            root_disk=root_disk,
            console=console,
            receipt=receipt_path,
            dry_run=False,
        )
    )

    after = sha256_file(root_disk)
    transcript = (
        console.read_text(encoding="utf-8", errors="replace")
        if console.exists()
        else ""
    )
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    assert_attachment_is_read_only(receipt.get("rootDisk", {}))

    rows = judge_pass_conditions(
        transcript=transcript,
        root_disk_digest_before=before,
        root_disk_digest_after=after,
        receipt=receipt,
    )
    passed = overall_verdict(rows)
    result = {
        "schema": "boole.native-shadow.mac3-closed-local-boot-result.v1",
        "runsPerformed": 1,
        "verdict": "PASS" if passed else "FAIL",
        "passConditions": rows,
        "rootDisk": {"sha256Before": before, "sha256After": after},
        "console": {
            "path": console.name,
            "sizeBytes": len(transcript.encode("utf-8")),
            "sha256": hashlib.sha256(transcript.encode("utf-8")).hexdigest(),
        },
        "hostReceipt": receipt,
        "bootableClaim": passed,
        "activationAllowed": False,
    }
    result_path.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    for row in rows:
        print("%-40s %s  (%s)" % (row["id"], row["verdict"], row["evidence"]))
    print("qualify: %s" % result["verdict"])
    return 0 if passed else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name, handler in (("prepare", command_prepare), ("qualify", command_qualify)):
        child = sub.add_parser(name)
        child.add_argument("--kernel", required=True)
        child.add_argument("--root-disk", required=True)
        child.add_argument("--work", required=True)
        child.add_argument("--host", default=None)
        child.set_defaults(handler=handler)
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except RefusedError as error:
        print("refused: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
