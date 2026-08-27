"""Drives the one MAC.3 closed-local boot on a development Mac.

The Swift host in `native/mac3/` builds and runs the machine. This module is
what decides *whether* it may run at all: it reads the frozen qualification,
hashes the artifacts it is about to hand over, refuses if that attempt has
already been spent, builds and ad-hoc-signs the host, and afterwards reads the
console transcript into a verdict for each pre-registered condition.

Two things are deliberate. The kernel command line is read out of the frozen
record rather than written here, so there is only one place it can be changed
and that place is gated. And the judging rules take a transcript and some
digests and return verdicts -- no filesystem, no Mac -- so the part that decides
whether the run passed is tested on every push by a runner that cannot boot
anything.

There is more than one attempt on record, and an attempt is selected rather
than assumed. The first was spent and failed; a successor image exists with its
own frozen qualification, its own images and its own receipt path. Selecting is
not the same as resetting: each record carries its own allowance, each seals to
its own file, and the first attempt stays refused because the receipt that
records its failure is still there. The module-level names below remain the
first attempt's, so everything written before the successor existed keeps
meaning what it meant.
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
SEALED_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v1.json"
)
HOST_SOURCE_PATH = REPO / "native/mac3/boole-mac3-closed-local-boot.swift"
ENTITLEMENTS_PATH = REPO / "native/mac3/boole-mac3-closed-local-boot.entitlements"

FIRST_ATTEMPT = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-1"
SUCCESSOR_ATTEMPT = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-2"

# The attempts that exist, by the identity each one is run under. A path is not
# accepted from the command line: an attempt is a record that was frozen and
# merged before it could be run, and letting one be named at the prompt would
# make the freeze a suggestion.
ATTEMPTS = {
    FIRST_ATTEMPT: QUALIFICATION_PATH,
    SUCCESSOR_ATTEMPT: (
        CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json"
    ),
}

# The roles the host may be given files for, and the roles a record may seal.
IMAGE_ROLES = ("kernel", "initrd", "rootDisk")

# What a closed machine looks like before one is built. The same properties are
# read back off the host's receipt afterwards; this is the half that can refuse
# before anything starts.
CLOSED_ISOLATION = {
    "networkDevices": 0,
    "sharedDirectories": 0,
    "writableDisksAttached": 0,
    "rootDiskAttachedReadOnly": True,
    "hostFilesystemExposedToGuest": False,
}

# Ad-hoc. A development Mac running a closed-local boot has no business holding
# a release identity, and `-` is the only identity that cannot be one.
CODESIGN_IDENTITY = "-"
RUNS_ALLOWED = 1
BOOT_TIMEOUT_SECONDS = 180
READ_CHUNK_BYTES = 4 * 1024 * 1024


class RefusedError(RuntimeError):
    """Raised where continuing would spend or misreport the one attempt."""


def qualification_path(attempt: str = FIRST_ATTEMPT) -> pathlib.Path:
    path = ATTEMPTS.get(attempt)
    if path is None:
        raise RefusedError(
            "there is no frozen record for %s; the attempts on record are %s"
            % (attempt, ", ".join(sorted(ATTEMPTS)))
        )
    return path


def qualification(attempt: str = FIRST_ATTEMPT) -> dict:
    return json.loads(qualification_path(attempt).read_text(encoding="utf-8"))


def sealed_result_path(attempt: str = FIRST_ATTEMPT) -> pathlib.Path:
    """Where the attempt's receipt is sealed once it has been spent.

    Read from the record rather than derived, so two attempts cannot be made to
    share one file by anyone editing this module.
    """
    declared = qualification(attempt).get("resultPath")
    if declared is None:
        # The first attempt predates the field; its path is the constant above.
        return SEALED_RESULT_PATH
    return REPO / declared


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(READ_CHUNK_BYTES)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def expected_digests(attempt: str = FIRST_ATTEMPT) -> dict:
    """The two files the host is handed, at the digests its record seals."""
    subject = qualification(attempt)["subject"]
    return {
        "kernel": subject["kernel"]["sha256"],
        "rootDisk": subject["rootDisk"]["sha256"],
    }


def declared_images(record: dict) -> dict:
    """Every image the record seals, including the ones it does not boot from.

    The initrd is not handed to the host -- this image boots without one -- but
    it was produced with the other two and sealed alongside them. Checking it
    is how "these are the files that run converged on" stays a statement about
    the whole set rather than about the subset that happens to be attached.
    """
    subject = record["subject"]
    rows = {}
    for role in IMAGE_ROLES:
        row = subject.get(role)
        if isinstance(row, dict) and row.get("sha256"):
            rows[role] = row
    return rows


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


def assert_attempt_identity(record: dict, attempt: str) -> None:
    """A record may only be run as the attempt it says it is."""
    declared = record.get("attemptId", FIRST_ATTEMPT)
    if declared != attempt:
        raise RefusedError(
            "the record identifies attempt %s and was asked to run as %s"
            % (declared, attempt)
        )


def _canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def assert_conditions_are_not_relaxed(record: dict) -> None:
    """A successor is judged by the bar its predecessor failed, unchanged.

    A record that succeeds another says so, and names the file it copied its
    conditions from at the digest that file has. Both halves are checked: the
    baseline is the one on disk, and the conditions are byte-identical to it.
    Without this, the cheapest way past a failed condition is to reword it.
    """
    claim = record.get("passConditionsUnchanged")
    if claim is None:
        # A record that succeeds nothing is the baseline others are held to.
        return
    baseline_path = REPO / claim["path"]
    found = sha256_file(baseline_path)
    if found != claim["sha256"]:
        raise RefusedError(
            "the record copies its conditions from %s at %s, which reads %s"
            % (claim["path"], claim["sha256"], found)
        )
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))["passConditions"]
    if _canonical(record["passConditions"]) != _canonical(baseline):
        raise RefusedError(
            "the record's %d condition(s) are not the %d it says it carries over"
            % (len(record["passConditions"]), len(baseline))
        )


def assert_isolation_is_closed(record: dict) -> None:
    """Refuse a record that describes a machine able to reach anything."""
    isolation = record.get("isolation", {})
    problems = []
    for field, want in CLOSED_ISOLATION.items():
        found = isolation.get(field)
        if isinstance(want, bool):
            met = found is want
        else:
            met = found == want and not isinstance(found, bool)
        if not met:
            problems.append("%s=%r" % (field, found))
    if problems:
        raise RefusedError(
            "the record does not describe a closed machine: " + ", ".join(problems)
        )


def preflight(attempt: str, images: dict, work: pathlib.Path) -> dict:
    """Every refusal that has to happen before a machine is built.

    Ordered cheapest-first only where that costs nothing: the record is read and
    argued with before two gigabytes are hashed, and the sealed receipt is
    checked before the working directory because a wiped scratch directory must
    not buy a second attempt.
    """
    record = qualification(attempt)
    assert_attempt_identity(record, attempt)
    assert_record_has_an_attempt_left(record)
    assert_conditions_are_not_relaxed(record)
    assert_isolation_is_closed(record)
    assert_no_run_has_been_spent(sealed_result_path(attempt))
    assert_no_run_has_been_spent(work / "RUN-RECEIPT.json")
    assert_no_run_has_been_spent(work / "QUALIFICATION-RESULT.json")

    declared = declared_images(record)
    missing = sorted(set(declared) - set(images))
    if missing:
        raise RefusedError(
            "the record seals %s and no path was given for %s"
            % (", ".join(sorted(declared)), ", ".join(missing))
        )
    read = {}
    for role in sorted(declared):
        row = declared[role]
        path = pathlib.Path(images[role])
        if not path.is_file():
            raise RefusedError("%s is not a file: %s" % (role, path))
        sealed_size = row.get("sizeBytes")
        found_size = path.stat().st_size
        # Size first: it is one stat call, and a file of the wrong length is
        # not the sealed one whatever the rest of it hashes to.
        if sealed_size is not None and found_size != sealed_size:
            raise RefusedError(
                "%s is %d bytes and the record seals %d"
                % (role, found_size, sealed_size)
            )
        read[role] = assert_file_matches(path, row["sha256"], role)
    return {"record": record, "digests": read}


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
    attempt: str = FIRST_ATTEMPT,
) -> list:
    record = qualification(attempt)
    digests = expected_digests(attempt)
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
# pair of digests so that the decision procedure is testable without a Mac. The
# digests arrive as an argument rather than being looked up: a run is judged
# against the record it was authorised by, and a rule that reads the default
# would judge the successor against the attempt it succeeds.

ROOT_MOUNT_MARKERS = (
    "EXT4-fs (vda): mounted filesystem",
    "VFS: Mounted root",
)
PID1_MARKER = "systemd[1]:"
PANIC_MARKER = "Kernel panic"


def _loads_the_converged_image(transcript, before, after, receipt, digests):
    if receipt.get("dryRun"):
        return False, "the receipt is from a dry run; no machine was started"
    kernel = receipt.get("kernel", {}).get("sha256")
    root_disk = receipt.get("rootDisk", {}).get("sha256")
    if kernel != digests["kernel"] or root_disk != digests["rootDisk"]:
        return False, "the host reported kernel %s and root disk %s" % (
            kernel,
            root_disk,
        )
    return True, "the host re-hashed both files and read the sealed digests"


def _closed_local_configuration(transcript, before, after, receipt, digests):
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


def _kernel_reaches_its_root_filesystem(transcript, before, after, receipt, digests):
    for marker in ROOT_MOUNT_MARKERS:
        if marker in transcript:
            return True, "the transcript contains %r" % marker
    return False, "the transcript never reports the root filesystem mounted"


def _guest_systemd_is_pid_1(transcript, before, after, receipt, digests):
    if PANIC_MARKER in transcript:
        return False, "the transcript contains %r" % PANIC_MARKER
    if PID1_MARKER not in transcript:
        return False, "the transcript never shows %r" % PID1_MARKER
    return True, "the transcript shows %r" % PID1_MARKER


def _sealed_image_unchanged_after_the_run(transcript, before, after, receipt, digests):
    if not before or not after:
        return False, "the image was not hashed on both sides of the run"
    if before != after:
        return False, "the image read %s before and %s after" % (before, after)
    return True, "the image hashed to %s before and after" % before


def _console_transcript_captured_and_hashed(transcript, before, after, receipt, digests):
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
    attempt: str = FIRST_ATTEMPT,
) -> list:
    digests = expected_digests(attempt)
    rows = []
    for condition in qualification(attempt)["passConditions"]:
        rule = RULES.get(condition["id"])
        if rule is None:
            raise RefusedError(
                "the frozen record has a condition with no rule to judge it: %s"
                % condition["id"]
            )
        met, evidence = rule(
            transcript,
            root_disk_digest_before,
            root_disk_digest_after,
            receipt,
            digests,
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


def images_from(args: argparse.Namespace) -> dict:
    given = {"kernel": args.kernel, "rootDisk": args.root_disk}
    if args.initrd:
        given["initrd"] = args.initrd
    return given


def command_prepare(args: argparse.Namespace) -> int:
    """Build, sign and dry-run. Costs nothing against the one attempt."""
    work = pathlib.Path(args.work).resolve()
    work.mkdir(parents=True, exist_ok=True)
    record = qualification(args.attempt)
    assert_attempt_identity(record, args.attempt)
    assert_conditions_are_not_relaxed(record)
    assert_isolation_is_closed(record)
    given = images_from(args)
    declared = declared_images(record)
    missing = sorted(set(declared) - set(given))
    if missing:
        raise RefusedError(
            "the record seals %s and no path was given for %s"
            % (", ".join(sorted(declared)), ", ".join(missing))
        )
    for role in sorted(declared):
        assert_file_matches(
            pathlib.Path(given[role]), declared[role]["sha256"], role
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
            attempt=args.attempt,
        )
    )
    print("prepare: ok (%s)" % args.attempt)
    return 0


def command_qualify(args: argparse.Namespace) -> int:
    """The one attempt. Refuses rather than repeats."""
    work = pathlib.Path(args.work).resolve()
    work.mkdir(parents=True, exist_ok=True)
    receipt_path = work / "RUN-RECEIPT.json"
    result_path = work / "QUALIFICATION-RESULT.json"

    checked = preflight(args.attempt, images_from(args), work)
    before = checked["digests"]["rootDisk"]

    kernel = pathlib.Path(args.kernel)
    root_disk = pathlib.Path(args.root_disk)
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
            attempt=args.attempt,
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
        attempt=args.attempt,
    )
    passed = overall_verdict(rows)
    result = {
        "schema": "boole.native-shadow.mac3-closed-local-boot-result.v1",
        "attemptId": args.attempt,
        "qualification": {
            "path": str(qualification_path(args.attempt).relative_to(REPO)),
            "sha256": sha256_file(qualification_path(args.attempt)),
        },
        "images": checked["digests"],
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
    print("qualify: %s (%s)" % (result["verdict"], args.attempt))
    return 0 if passed else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name, handler in (("prepare", command_prepare), ("qualify", command_qualify)):
        child = sub.add_parser(name)
        child.add_argument("--kernel", required=True)
        child.add_argument("--root-disk", required=True)
        # Sealed with the other two and hashed with them, never attached: this
        # image boots without one, and the record says so.
        child.add_argument("--initrd", default=None)
        child.add_argument("--work", required=True)
        child.add_argument("--host", default=None)
        child.add_argument(
            "--attempt",
            choices=sorted(ATTEMPTS),
            default=FIRST_ATTEMPT,
            help="which frozen attempt this run is, and is judged by",
        )
        child.set_defaults(handler=handler)
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except RefusedError as error:
        print("refused: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
