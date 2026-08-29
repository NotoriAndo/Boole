"""Runs, or refuses to run, the third MAC.3 closed-local boot.

The criteria for this attempt were sealed first and say nothing about the
program that carries them out.  This is that program, and it is written around
four things the previous runner could not do.

It knows this attempt.  The earlier runner offered a choice of the first two
and refused anything else, so the third attempt could not have started.

It has a rule for every condition.  The earlier runner discovered an unjudgeable
condition *after* the machine had run, which is the shape that spends the one
attempt and writes no verdict.  Here the absence is found while nothing has been
started, and the same check runs on every push.

It marks the attempt as used before the machine starts.  A receipt written
afterwards is no record at all if the Mac dies in the middle: the boot happened
and nothing says so.  The mark is created with an exclusive open, outside the
working directory, so a wiped scratch directory cannot buy a second attempt.

And it re-reads the whole archive on both sides of the run -- three images and
both copies of the preservation manifest -- rather than the root disk alone.

None of that makes the run possible today.  Five of the twenty-one conditions
ask for evidence this image cannot produce: the launcher prints nothing on its
success path, and four conditions need it to speak.  Those are a hard stop, not
a rounding error, and the refusal is the point -- the attempt stays unspent.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
)
CONTRACT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-execution-contract-arm64-v3.json"
)
ATTEMPT = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3"

# Outside the repository and outside any scratch directory. The one thing this
# file has to survive is the run it is recording, including the run ending
# badly: a mark kept beside the work would vanish with the work.
LEDGER_DIR = pathlib.Path.home() / "boole-artifacts/native-shadow/mac3-boot-ledger"

MANIFEST_AT_THE_ARCHIVE = "preservation-manifest-at-the-archive"
MANIFEST_IN_THE_REPOSITORY = "preservation-record-in-the-repository"

BOOT_TIMEOUT_SECONDS = 180
READ_CHUNK_BYTES = 4 * 1024 * 1024

SYSTEMD_VERSION = "systemd 255"
PID1_MARKER = "systemd[1]:"
PANIC_MARKER = "Kernel panic"
ROOT_MOUNT_MARKERS = ("EXT4-fs (vda): mounted filesystem", "VFS: Mounted root")
LAUNCHER_UNIT = "boole-native-shadow-launcher-v2.service"

# The guest's own default, not the target the launcher unit is wanted by. The
# second attempt's transcript settled on graphical.target after passing through
# multi-user.target, so stopping at multi-user is stopping early.
DEFAULT_TARGET = "graphical.target"

FAILURE_MARKERS = ("[FAILED]", "Failed to start", "Dependency failed", "Timed out")
EMERGENCY_MARKERS = ("emergency", "rescue")

# Attempts, not probes. A kernel that carries a network driver announces it at
# boot whether or not a device exists, and reading that as reaching for a
# network would fail a run for the drivers it was built with.
TRANSPORT_MARKERS = (
    "connecting to peer",
    "vsock connect",
    "boole-node",
    "DHCPDISCOVER",
)


class RefusedError(RuntimeError):
    """Raised where continuing would spend or misreport the one attempt."""


class HardStop(RefusedError):
    """Raised where the image cannot produce evidence a condition asks for."""


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def qualification() -> dict:
    return read_json(QUALIFICATION_PATH)


def contract() -> dict:
    return read_json(CONTRACT_PATH)


def pass_conditions() -> list:
    return qualification()["passConditions"]


def condition_ids() -> set:
    return {row["id"] for row in pass_conditions()}


def sealed_result_path() -> pathlib.Path:
    """Where the receipt is sealed. Read from the record, never derived."""

    return REPO / qualification()["resultPath"]


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(READ_CHUNK_BYTES)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


# --- refusals that happen before anything is built ---------------------------


def assert_attempt_identity() -> None:
    declared = qualification()["attemptId"]
    if declared != ATTEMPT:
        raise RefusedError(
            "the sealed record identifies attempt %s and this runner is %s"
            % (declared, ATTEMPT)
        )


def assert_no_run_has_been_sealed(receipt_path: pathlib.Path) -> None:
    if receipt_path.exists():
        raise RefusedError(
            "a receipt already exists at %s; the one attempt is spent" % receipt_path
        )


def assert_every_condition_has_a_rule(rules=None) -> None:
    """Found before a machine is built, rather than after one has run."""

    known = set(RULES if rules is None else rules)
    missing = sorted(condition_ids() - known)
    if missing:
        raise RefusedError(
            "the sealed record has %d condition(s) with no rule to judge them: %s"
            % (len(missing), ", ".join(missing))
        )


def hard_stop_conditions() -> list:
    return sorted(row["id"] for row in contract()["hardStop"]["conditions"])


def assert_every_condition_is_observable() -> None:
    stopped = hard_stop_conditions()
    if stopped:
        raise HardStop(
            "this image cannot produce evidence for %d condition(s), so the boot "
            "would be spent without a verdict: %s" % (len(stopped), ", ".join(stopped))
        )


# --- the archive, re-read on both sides --------------------------------------


def archive_root() -> pathlib.Path:
    return pathlib.Path(qualification()["subject"]["archiveRoot"])


def archive_targets() -> list:
    """The three images and both copies of the preservation manifest.

    The manifests are in here for the reason the images are: the record that
    says what the images should hash to is itself a file on a disk whose owner
    can change it, so trusting it without re-reading it is trusting the last
    time anyone looked.
    """

    record = qualification()
    preservation = record["subject"]["preservationRecord"]
    repository_copy = REPO / preservation["path"]
    archive_copy_name = read_json(repository_copy)["archive"]["selfDescribingCopy"]
    targets = [
        {
            "name": row["name"],
            "relativePath": row["archivePath"],
            "sha256": row["sha256"],
            "bytes": row["bytes"],
        }
        for row in record["subject"]["images"]
    ]
    targets.append(
        {
            "name": MANIFEST_AT_THE_ARCHIVE,
            "relativePath": archive_copy_name,
            "sha256": preservation["sha256"],
            "bytes": repository_copy.stat().st_size,
        }
    )
    targets.append(
        {
            "name": MANIFEST_IN_THE_REPOSITORY,
            "absolutePath": str(repository_copy),
            "sha256": preservation["sha256"],
            "bytes": repository_copy.stat().st_size,
        }
    )
    targets.sort(key=lambda row: row["name"])
    return targets


def recompute(targets: list, root: pathlib.Path) -> list:
    """Read every target now. Nothing here is carried over from an earlier check."""

    rows = []
    for target in targets:
        if "absolutePath" in target:
            path = pathlib.Path(target["absolutePath"])
        else:
            path = root / target["relativePath"]
        row = {"name": target["name"], "path": str(path), "expected": target["sha256"]}
        if not path.is_file():
            rows.append(dict(row, ok=False, why="not a file"))
            continue
        found_bytes = path.stat().st_size
        if target.get("bytes") is not None and found_bytes != target["bytes"]:
            rows.append(
                dict(
                    row,
                    ok=False,
                    bytes=found_bytes,
                    why="%d bytes, expected %d" % (found_bytes, target["bytes"]),
                )
            )
            continue
        found = sha256_file(path)
        rows.append(
            dict(
                row,
                ok=found == target["sha256"],
                bytes=found_bytes,
                sha256=found,
                why="" if found == target["sha256"] else "read %s" % found,
            )
        )
    return rows


def assert_archive_matches(rows: list) -> None:
    failed = [row for row in rows if not row["ok"]]
    if failed:
        raise RefusedError(
            "the archive does not read as sealed: "
            + "; ".join("%s (%s)" % (row["name"], row["why"]) for row in failed)
        )


# --- the one-use mark, claimed before the machine starts ---------------------


def ledger_path() -> pathlib.Path:
    return LEDGER_DIR / ("%s.used.json" % ATTEMPT)


def assert_one_use_is_unclaimed() -> None:
    path = ledger_path()
    if path.exists():
        raise RefusedError(
            "this attempt is already marked as used at %s; a boot that happened "
            "is a boot that counts, whatever it produced" % path
        )


def claim_one_use(approval: dict) -> pathlib.Path:
    """Create the mark, or refuse. Exclusive, so two runs cannot both win.

    Called immediately before the machine is started rather than after it
    stops.  A run that dies with the Mac still spends the attempt, and this is
    the only way that fact survives the run.
    """

    path = ledger_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "attemptId": ATTEMPT,
        "approval": approval,
        "qualification": {
            "path": QUALIFICATION_PATH.relative_to(REPO).as_posix(),
            "sha256": sha256_file(QUALIFICATION_PATH),
        },
        "whatThisMeans": (
            "the machine for this attempt was about to be started when this file "
            "was written.  It is not a verdict and it is not a receipt: it is the "
            "record that the one boot was used."
        ),
    }
    try:
        handle = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o444)
    except FileExistsError as error:
        raise RefusedError(
            "this attempt is already marked as used at %s" % path
        ) from error
    with os.fdopen(handle, "w", encoding="utf-8") as stream:
        stream.write(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def prior_claims() -> int:
    return 1 if ledger_path().exists() else 0


# --- judging ------------------------------------------------------------------
#
# Every rule takes one evidence bundle and returns (met, why). No filesystem and
# no Mac, so the part that decides whether the run passed is exercised on every
# push by a runner that cannot boot anything.


def evidence_from(
    *,
    transcript: str = "",
    receipt: dict = None,
    archive_before: list = None,
    archive_after: list = None,
    host_after: dict = None,
    ledger: dict = None,
    sealed: dict = None,
    readiness_seen: bool = False,
    timeout_seconds: int = BOOT_TIMEOUT_SECONDS,
) -> dict:
    return {
        "transcript": transcript,
        "receipt": receipt or {},
        "archiveBefore": archive_before or [],
        "archiveAfter": archive_after or [],
        "hostAfter": host_after or {},
        "ledger": ledger or {},
        "sealed": sealed or {},
        "readinessSeen": readiness_seen,
        "timeoutSeconds": timeout_seconds,
    }


def empty_evidence() -> dict:
    """What a run that produced nothing looks like. It passes no condition."""

    return evidence_from()


def _rows_by_name(rows: list) -> dict:
    return {row["name"]: row for row in rows}


def _sealed_check(evidence: dict, identifier: str):
    row = evidence["sealed"].get(identifier)
    if not row:
        return False, "the sealed record this condition rests on was not read"
    if not row.get("ok"):
        return False, "the sealed record no longer reads as recorded: %s" % row.get(
            "why", ""
        )
    return True, "%s reports %s" % (row["path"], row["check"])


def _root_disk_unchanged(evidence: dict):
    before = _rows_by_name(evidence["archiveBefore"]).get("guest-root-disk")
    after = _rows_by_name(evidence["archiveAfter"]).get("guest-root-disk")
    if not before or not after:
        return False, "the root disk was not hashed on both sides of the run"
    if before.get("sha256") != after.get("sha256"):
        return False, "the root disk read %s before and %s after" % (
            before.get("sha256"),
            after.get("sha256"),
        )
    return True, "the root disk hashed to %s before and after" % before.get("sha256")


def _archive_digests_recomputed(evidence):
    rows = _rows_by_name(evidence["archiveBefore"])
    wanted = {row["name"] for row in archive_targets()}
    missing = sorted(wanted - set(rows))
    if missing:
        return False, "not recomputed immediately before the boot: " + ", ".join(missing)
    bad = sorted(name for name, row in rows.items() if not row.get("ok"))
    if bad:
        return False, "read differently from the seal: " + ", ".join(bad)
    return True, "%d targets re-read at the archive and all matched" % len(rows)


def _closed_local_configuration(evidence):
    machine = evidence["receipt"].get("machine", {})
    problems = []
    for field in ("networkDevices", "sharedDirectories", "socketDevices"):
        if machine.get(field, -1) != 0:
            problems.append("%s=%s" % (field, machine.get(field)))
    if machine.get("storageDevices") != 1:
        problems.append("storageDevices=%s" % machine.get("storageDevices"))
    if not evidence["receipt"].get("rootDisk", {}).get("attachedReadOnly"):
        problems.append("attachedReadOnly=false")
    if problems:
        return False, "the built machine reported " + ", ".join(problems)
    return True, "no network device, no shared directory, one read-only disk"


def _console_transcript_captured_and_hashed(evidence):
    transcript = evidence["transcript"]
    if not transcript.strip():
        return False, "the console transcript is empty"
    return True, "%d bytes, sha256 %s" % (
        len(transcript.encode("utf-8")),
        hashlib.sha256(transcript.encode("utf-8")).hexdigest(),
    )


def _every_write_lands_on_tmpfs(evidence):
    met, why = _sealed_check(evidence, "every-write-lands-on-tmpfs")
    if not met:
        return False, why
    unchanged, unchanged_why = _root_disk_unchanged(evidence)
    if not unchanged:
        return False, unchanged_why
    return True, "%s, and %s" % (why, unchanged_why)


def _exactly_one_boot_of_this_image(evidence):
    ledger = evidence["ledger"]
    if ledger.get("receiptExistedBeforeTheRun"):
        return False, "a receipt for this attempt already existed before the run"
    if not ledger.get("claimedByThisRun"):
        return False, "this run did not claim the one-use mark before starting"
    prior = ledger.get("priorClaims")
    if prior != 0:
        return False, "the one-use mark had already been claimed %s time(s)" % prior
    return True, "the mark was unclaimed and this run claimed it before starting"


def _guest_systemd_is_pid_1(evidence):
    transcript = evidence["transcript"]
    if PANIC_MARKER in transcript:
        return False, "the transcript contains %r" % PANIC_MARKER
    if PID1_MARKER not in transcript:
        return False, "the transcript never shows %r" % PID1_MARKER
    if SYSTEMD_VERSION not in transcript:
        return False, "the transcript never reports %r" % SYSTEMD_VERSION
    return True, "the transcript shows %r and %r" % (PID1_MARKER, SYSTEMD_VERSION)


def _kernel_reaches_its_root_filesystem(evidence):
    for marker in ROOT_MOUNT_MARKERS:
        if marker in evidence["transcript"]:
            return True, "the transcript contains %r" % marker
    return False, "the transcript never reports the root filesystem mounted"


def _launcher_service_starts_without_being_asked(evidence):
    if LAUNCHER_UNIT not in evidence["transcript"]:
        return False, "the transcript never names %s" % LAUNCHER_UNIT
    return True, "systemd started %s without a console command asking for it" % (
        LAUNCHER_UNIT
    )


def _loads_the_converged_image(evidence):
    receipt = evidence["receipt"]
    if receipt.get("dryRun"):
        return False, "the receipt is from a dry run; no machine was started"
    sealed = {row["name"]: row["sha256"] for row in qualification()["subject"]["images"]}
    kernel = receipt.get("kernel", {}).get("sha256")
    root_disk = receipt.get("rootDisk", {}).get("sha256")
    if kernel != sealed["guest-kernel"] or root_disk != sealed["guest-root-disk"]:
        return False, "the host reported kernel %s and root disk %s" % (kernel, root_disk)
    return True, "the host re-hashed both files and read the sealed digests"


def _no_failed_unit_and_no_freeze(evidence):
    transcript = evidence["transcript"]
    for marker in FAILURE_MARKERS:
        if marker in transcript:
            return False, "the transcript contains %r" % marker
    if not evidence["readinessSeen"]:
        return False, (
            "the transcript ends without the readiness point being reached within the "
            "run's stated budget of %d seconds, which counts as a stall rather than as "
            "an absence of evidence" % evidence["timeoutSeconds"]
        )
    return True, "no failed unit, no timeout, and the run reached its readiness point"


def _no_network_device_and_no_shared_directory(evidence):
    machine = evidence["receipt"].get("machine", {})
    problems = [
        "%s=%s" % (field, machine.get(field))
        for field in ("networkDevices", "sharedDirectories", "socketDevices")
        if machine.get(field, -1) != 0
    ]
    if problems:
        return False, "the host configuration counted " + ", ".join(problems)
    return True, "zero network devices, zero shared directories, zero socket devices"


def _nothing_beyond_the_closed_local_boot(evidence):
    machine = evidence["receipt"].get("machine", {})
    problems = [
        "%s=%s" % (field, machine.get(field))
        for field in ("networkDevices", "sharedDirectories", "socketDevices")
        if machine.get(field, -1) != 0
    ]
    if evidence["receipt"].get("nodeProcessStarted"):
        problems.append("a node process was started")
    granted = evidence["receipt"].get("authorityGrantedToTheGuest")
    if granted:
        problems.append("authority granted to the guest: %s" % granted)
    for marker in TRANSPORT_MARKERS:
        if marker in evidence["transcript"]:
            problems.append("the transcript contains %r" % marker)
    if problems:
        return False, "; ".join(problems)
    return True, (
        "no transport to run over, no node process, and no reward, block, "
        "consensus or P2P authority granted"
    )


def _nothing_of_the_guest_survives_the_shutdown(evidence):
    after = evidence["hostAfter"]
    if not after:
        return False, "the host was not counted after the run"
    problems = [
        "%s=%s" % (field, after.get(field))
        for field in ("virtualMachines", "hostProcesses", "temporaryMounts")
        if after.get(field, -1) != 0
    ]
    # The sealed condition extends the count to the socket path and the working
    # directory. Absent keys read as absent files, which is what passing means.
    for field in ("socketPathPresent", "workingDirectoryPresent"):
        if after.get(field, False):
            problems.append("%s=true" % field)
    if problems:
        return False, "the host still reports " + ", ".join(problems)
    return True, (
        "no virtual machine, no host process, no temporary mount, no socket path "
        "and no working directory left"
    )


def _root_disk_stays_read_only(evidence):
    if not evidence["receipt"].get("rootDisk", {}).get("attachedReadOnly"):
        return False, "the host did not report the disk attached read-only"
    return _root_disk_unchanged(evidence)


def _sealed_image_unchanged_after_the_run(evidence):
    before = _rows_by_name(evidence["archiveBefore"])
    after = _rows_by_name(evidence["archiveAfter"])
    if not before or not after:
        return False, "the archive was not hashed on both sides of the run"
    missing = sorted(set(before) - set(after))
    if missing:
        return False, "not re-read after the run: " + ", ".join(missing)
    changed = sorted(
        name for name in before if before[name].get("sha256") != after[name].get("sha256")
    )
    if changed:
        return False, "changed across the run: " + ", ".join(changed)
    return True, "all %d targets hashed identically before and after" % len(before)


def _systemd_does_not_enter_emergency(evidence):
    transcript = evidence["transcript"]
    lowered = transcript.lower()
    for marker in EMERGENCY_MARKERS:
        if marker in lowered:
            return False, "the transcript contains %r" % marker
    reached = "Reached target %s" % DEFAULT_TARGET
    if reached not in transcript:
        return False, (
            "the transcript never reports %r, which is the guest's own default "
            "rather than the target the launcher unit is wanted by" % reached
        )
    return True, "no emergency or rescue unit appears and %r is reached" % reached


def _not_observable(identifier: str, missing: str):
    """A rule for a condition this image cannot answer.

    It exists so the runner is complete, not so the run can be judged.  It
    never returns met: silence is not evidence, and a rule that guessed here
    would leave the hard stop as the only thing between an empty transcript and
    a pass.
    """

    def rule(evidence):
        return False, "%s cannot be judged from this image: %s" % (identifier, missing)

    return rule


RULES = {
    "archive-digests-recomputed-immediately-before-boot": _archive_digests_recomputed,
    "closed-local-configuration": _closed_local_configuration,
    "console-transcript-captured-and-hashed": _console_transcript_captured_and_hashed,
    "every-write-lands-on-tmpfs": _every_write_lands_on_tmpfs,
    "exactly-one-boot-of-this-image": _exactly_one_boot_of_this_image,
    "guest-systemd-is-pid-1": _guest_systemd_is_pid_1,
    "kernel-reaches-its-root-filesystem": _kernel_reaches_its_root_filesystem,
    "launcher-executable-matches-the-sealed-digest": _not_observable(
        "launcher-executable-matches-the-sealed-digest",
        "the guest has to report the digest of the file it executed, and nothing "
        "carries a word out of the guest until MAC.4 exists",
    ),
    "launcher-prerequisites-verify-inside-the-guest": _not_observable(
        "launcher-prerequisites-verify-inside-the-guest",
        "the launcher prints nothing on its success path, so a transcript without "
        "an error is silence rather than a pass",
    ),
    "launcher-service-starts-without-being-asked": (
        _launcher_service_starts_without_being_asked
    ),
    "launcher-supervises-as-root-and-submissions-run-unprivileged": _not_observable(
        "launcher-supervises-as-root-and-submissions-run-unprivileged",
        "the launcher half is read from the guest's own kernel status at startup, "
        "which nothing writes to the console",
    ),
    "loads-the-converged-image": _loads_the_converged_image,
    "no-failed-unit-and-no-freeze-in-the-transcript": _no_failed_unit_and_no_freeze,
    "no-host-wallet-model-key-or-node-secret-in-the-guest": _not_observable(
        "no-host-wallet-model-key-or-node-secret-in-the-guest",
        "the sealed read-back compares the paths the lock tracks and cannot see an "
        "extra file, and no sealed check searches the image for secret-bearing names",
    ),
    "no-network-device-and-no-shared-directory": (
        _no_network_device_and_no_shared_directory
    ),
    "nothing-beyond-the-closed-local-boot-is-attempted": (
        _nothing_beyond_the_closed_local_boot
    ),
    "nothing-of-the-guest-survives-the-shutdown": (
        _nothing_of_the_guest_survives_the_shutdown
    ),
    "readiness-and-clean-shutdown-are-observed": _not_observable(
        "readiness-and-clean-shutdown-are-observed",
        "the launcher's readiness point is not visible to the host, which the "
        "condition itself says and this image does not change",
    ),
    "root-disk-stays-read-only": _root_disk_stays_read_only,
    "sealed-image-unchanged-after-the-run": _sealed_image_unchanged_after_the_run,
    "systemd-does-not-enter-emergency-and-reaches-its-target": (
        _systemd_does_not_enter_emergency
    ),
}


def judge_pass_conditions(evidence: dict) -> list:
    assert_every_condition_has_a_rule()
    rows = []
    for condition in pass_conditions():
        met, why = RULES[condition["id"]](evidence)
        rows.append(
            {
                "id": condition["id"],
                "condition": condition["condition"],
                "judgedBy": condition["judgedBy"],
                "evidence": why,
                "verdict": "MET" if met else "NOT MET",
            }
        )
    return rows


def overall_verdict(rows: list) -> bool:
    return bool(rows) and all(row["verdict"] == "MET" for row in rows)


def resolve_sealed_evidence() -> dict:
    """Re-read every sealed record the evidence plan rests on."""

    resolved = {}
    for row in contract()["evidencePlan"]:
        if row["evidenceSource"] != "sealed-record":
            continue
        sealed = row["sealedEvidence"]
        path = REPO / sealed["path"]
        if not path.is_file():
            resolved[row["id"]] = dict(sealed, ok=False, why="missing")
            continue
        found = sha256_file(path)
        resolved[row["id"]] = dict(
            sealed,
            ok=found == sealed["sha256"],
            why="" if found == sealed["sha256"] else "reads %s" % found,
        )
    return resolved


# --- commands -----------------------------------------------------------------


def _run(argv: list) -> subprocess.CompletedProcess:
    print("+ " + " ".join(argv))
    return subprocess.run(argv, check=True)


def host_argv(
    host: pathlib.Path,
    kernel: pathlib.Path,
    root_disk: pathlib.Path,
    console: pathlib.Path,
    receipt: pathlib.Path,
    dry_run: bool = False,
) -> list:
    record = qualification()
    sealed = {row["name"]: row["sha256"] for row in record["subject"]["images"]}
    argv = [
        str(host),
        "--kernel",
        str(kernel),
        "--kernel-sha256",
        sealed["guest-kernel"],
        "--root-disk",
        str(root_disk),
        "--root-disk-sha256",
        sealed["guest-root-disk"],
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


def start_the_machine(
    host: pathlib.Path,
    kernel: pathlib.Path,
    root_disk: pathlib.Path,
    console: pathlib.Path,
    receipt: pathlib.Path,
    approval: dict,
    runner=None,
):
    """Mark the attempt as used, then start. In that order, always.

    Between these two statements is the whole reason this function exists: if
    the machine starts first and the Mac dies, the attempt was spent and
    nothing on disk says so.
    """

    claim_one_use(approval)
    argv = host_argv(
        host=host,
        kernel=kernel,
        root_disk=root_disk,
        console=console,
        receipt=receipt,
        dry_run=False,
    )
    return (runner or _run)(argv)


def preflight(runner=None) -> dict:
    """Everything that can be established without starting anything.

    It gathers every refusal rather than stopping at the first, because the
    point of a free check is to show the whole distance still to go.  It builds
    no machine and claims no mark, and reports both counts as zero so that is
    something the operator reads rather than something they trust.
    """

    del runner  # accepted so callers can prove it is never used
    blockers = []
    record = qualification()
    if record["attemptId"] != ATTEMPT:
        blockers.append("the sealed record is for %s" % record["attemptId"])

    receipt_path = sealed_result_path()
    if receipt_path.exists():
        blockers.append("a receipt already exists at %s" % receipt_path)

    missing_rules = sorted(condition_ids() - set(RULES))
    if missing_rules:
        blockers.append("no rule for: " + ", ".join(missing_rules))

    stopped = hard_stop_conditions()
    if stopped:
        blockers.append(
            "this image cannot show evidence for: " + ", ".join(stopped)
        )

    claimed = ledger_path().exists()
    if claimed:
        blockers.append("the one-use mark is already claimed at %s" % ledger_path())

    root = archive_root()
    if root.is_dir():
        archive = recompute(archive_targets(), root)
        failed = [row["name"] for row in archive if not row["ok"]]
        if failed:
            blockers.append("the archive does not read as sealed: " + ", ".join(failed))
    else:
        archive = []
        blockers.append("the archive is not present on this machine: %s" % root)

    sealed = resolve_sealed_evidence()
    stale = sorted(name for name, row in sealed.items() if not row["ok"])
    if stale:
        blockers.append("sealed evidence no longer reads as recorded: " + ", ".join(stale))

    return {
        "attemptId": ATTEMPT,
        "machinesStarted": 0,
        "oneUseMarksCreated": 0,
        "oneUseMarkClaimed": claimed,
        "priorClaims": prior_claims(),
        "archive": archive,
        "sealedEvidence": sealed,
        "hardStop": stopped,
        "missingRules": missing_rules,
        "blockers": blockers,
        "ready": not blockers,
    }


def command_preflight(args: argparse.Namespace) -> int:
    del args
    report = preflight()
    print(json.dumps(report, indent=2, sort_keys=True))
    for blocker in report["blockers"]:
        print("blocked: %s" % blocker, file=sys.stderr)
    print(
        "preflight: %s (machines started %d, one-use marks created %d)"
        % (
            "READY" if report["ready"] else "NOT READY",
            report["machinesStarted"],
            report["oneUseMarksCreated"],
        ),
        file=sys.stderr,
    )
    return 0 if report["ready"] else 1


def command_qualify(args: argparse.Namespace) -> int:
    """The one attempt. Refuses rather than guesses.

    The steps past the refusals -- building the host, starting the machine,
    gathering the evidence -- are deliberately not written yet.  A boot path
    sitting here complete but unreachable is a boot path waiting for someone to
    delete the check above it.  Closing the hard stops is what opens writing it,
    and closing them means changing the image, not the conditions.
    """

    del args
    assert_attempt_identity()
    assert_every_condition_has_a_rule()
    assert_no_run_has_been_sealed(sealed_result_path())
    assert_one_use_is_unclaimed()
    assert_every_condition_is_observable()
    raise RefusedError(
        "every refusal passed, but the boot path is not written while any condition "
        "is unobservable; the attempt stays unspent"
    )


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name, handler in (
        ("preflight", command_preflight),
        ("qualify", command_qualify),
    ):
        child = sub.add_parser(name)
        child.set_defaults(handler=handler)
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except RefusedError as error:
        print("refused: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
