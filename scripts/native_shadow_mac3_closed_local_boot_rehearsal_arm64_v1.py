"""Walk the whole boot flow without a machine, so the one attempt is not the rehearsal.

The runner can refuse and it can judge, and between those two it stops: the
steps that would build the host, start the machine, wait for it, shut it down
and gather what it left were deliberately never written, because a complete
boot path sitting behind a check is a boot path waiting for someone to delete
the check.  That reasoning still holds, so this does not write one.

What it writes instead is the same sequence with every collaborator handed in.
There is no default host, no default clock and no default way to claim the
mark; a caller who wants this flow to do anything must supply the thing that
does it.  The only supplier that exists is the stand-in below, which writes
files instead of starting anything.  So the sequence can be walked end to end
for free, and the real run is still unwritten rather than merely guarded.

What that proves is the plumbing: that the argument list is the one a host
could act on, that the mark is claimed before the start and not after, that
the host waits to see readiness before it asks for a shutdown and confirms the
machine is gone rather than assuming it, that the console is read by the same
reader and the evidence judged by the same twenty-one rules.  What it cannot
prove is that a guest behaves, because there is no guest.  Every one of those
substitutions is listed in the record it writes.

Two things it must never do, and both are tested rather than promised: it must
not claim the one-use mark, and it must not write the sealed result.  A
rehearsal that spent the attempt would be the exact accident it exists to
prevent, and a rehearsal verdict sitting where a real verdict belongs is a
forged result however it was produced.
"""

import argparse
import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]

if str(REPO / "scripts") not in sys.path:
    sys.path.insert(0, str(REPO / "scripts"))

import native_shadow_mac3_closed_local_boot_arm64_v3 as runner  # noqa: E402
import native_shadow_mac3_guest_evidence_protocol_arm64_v1 as protocol  # noqa: E402

RECORD = "native-shadow-mac3-closed-local-boot-rehearsal-arm64-v1"

# The order is the content. Each name below is a thing that has to happen before
# the next one is safe, and the two that matter most are adjacent: the mark is
# claimed, and only then does anything start.
STEPS = (
    "refuse-what-can-be-refused-for-free",
    "hash-the-archive-before",
    "claim-the-one-use-mark",
    "start-the-machine",
    "wait-for-readiness",
    "ask-for-shutdown",
    "confirm-the-machine-is-gone",
    "hash-the-archive-after",
    "count-the-host",
    "assemble-the-evidence",
    "judge-the-conditions",
    "write-what-a-real-run-would-seal",
)

# Not in STEPS, because it is what happens when the run has already gone wrong.
# A guest that never reached readiness has nothing to receive a polite request,
# so the machine is taken down rather than asked.
STEP_WHEN_THE_GUEST_STALLS = "stop-the-machine-the-hard-way"

WHAT_A_REHEARSAL_PROVES = (
    "that the flow connects end to end: the argument list a host would be given, "
    "the order the mark and the start happen in, the wait for readiness before "
    "the shutdown request, the confirmation that the machine is gone, the reader "
    "that turns a console into evidence, the rules that judge it and the verdict "
    "that comes out. It proves nothing about a guest, because there is no guest."
)


class RehearsalError(RuntimeError):
    """The rehearsal could not be carried out, or tried to become a real run."""


def rehearsal_result_path(directory: pathlib.Path) -> pathlib.Path:
    """Where a rehearsal writes. Never where a sealed result belongs.

    The word is in the file name as well as inside the file, so a reader who
    only ever sees the path does not have to open it to know that what is in it
    did not come from a machine.
    """

    return directory / "REHEARSAL-not-a-sealed-result.json"


def refuse_the_sealed_path(path: pathlib.Path) -> pathlib.Path:
    """Refuse to write where a real verdict belongs. Checked live, not by naming."""

    if path.resolve() == runner.sealed_result_path().resolve():
        raise RehearsalError(
            "a rehearsal may not write the sealed result path; that file means a "
            "machine ran, and none did"
        )
    return path


def write_rehearsal(directory: pathlib.Path, report: dict) -> pathlib.Path:
    path = refuse_the_sealed_path(rehearsal_result_path(directory))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


# --- the stand-in -------------------------------------------------------------


def healthy_console(*, ready: bool = True) -> str:
    """A transcript shaped like the one a healthy guest would leave.

    It is a fixture and the record says so.  Its job is to exercise the reader
    and the rules, not to stand in for evidence: nothing built from it is ever
    written where a real result belongs.
    """

    lines = [
        "[    0.000000] Linux version 6.8.0 (a fixture; no machine ran)",
        "[    0.900000] EXT4-fs (vda): mounted filesystem read-only",
        "[    1.100000] systemd 255 running in system mode",
        "[    1.100001] systemd[1]: Detected virtualization.",
        "[    2.000000] systemd[1]: Started %s." % runner.LAUNCHER_UNIT,
        "[    2.100000] "
        + protocol.format_record(
            "launcher-executable",
            {"path": "/usr/lib/boole/launcher", "sha256": "0" * 64},
        ),
        "[    2.200000] "
        + protocol.format_record(
            "supervisor-privilege", {"uid": 0, "gid": 0, "capabilities": []}
        ),
        "[    2.300000] "
        + protocol.format_record(
            "launcher-prerequisites",
            {"prerequisites": [{"name": "lean-toolchain", "resolved": True}]},
        ),
    ]
    if ready:
        lines.append(
            "[    3.000000] "
            + protocol.format_record("readiness", {"ready": True, "failedUnits": []})
        )
        lines.append(
            "[    3.100000] systemd[1]: Reached target %s." % runner.DEFAULT_TARGET
        )
    else:
        lines.append("[  180.000000] systemd[1]: still starting, no readiness recorded")
    return "\n".join(lines) + "\n"


class FakeHost:
    """Writes files instead of starting anything, and remembers what it was asked.

    Every method records its own name, because half of what a rehearsal checks
    is ordering: a mark claimed after the start, or a shutdown requested before
    readiness was ever seen, is a real defect that no single step reveals.
    """

    def __init__(
        self,
        directory: pathlib.Path,
        *,
        free_refusals_pass: bool = True,
        reaches_readiness: bool = True,
        obeys_shutdown: bool = True,
    ) -> None:
        self.directory = pathlib.Path(directory)
        self.free_refusals_pass = free_refusals_pass
        self.reaches_readiness = reaches_readiness
        self.obeys_shutdown = obeys_shutdown
        self.order = []
        self.argv = []
        self.started = False
        self.stopped = False

    def _note(self, name: str) -> None:
        self.order.append(name)

    def free_refusals(self) -> dict:
        self._note("free_refusals")
        return {
            "ready": self.free_refusals_pass,
            "blockers": []
            if self.free_refusals_pass
            else ["the stand-in was told to report the run as not ready"],
            "hardStop": list(runner.hard_stop_conditions()),
        }

    def hash_archive(self) -> list:
        """The sealed digests read back as matching, without touching the archive.

        A rehearsal that re-hashed multi-gigabyte images would be neither free
        nor safe to run in continuous integration, and hashing is the one part
        of the flow already covered by its own tests.
        """

        self._note("hash_archive")
        return [dict(row, ok=True) for row in runner.archive_targets()]

    def claim_mark(self) -> dict:
        self._note("claim_mark")
        return {
            "receiptExistedBeforeTheRun": False,
            "claimedByThisRun": True,
            "priorClaims": 0,
            "claimedWhere": "nowhere: the stand-in claimed no mark on disk",
        }

    def start(self, argv: list) -> None:
        self._note("start")
        flags = {
            item: argv[index + 1]
            for index, item in enumerate(argv)
            if item.startswith("--") and index + 1 < len(argv)
        }
        for required in ("--console", "--receipt", "--kernel", "--root-disk"):
            if required not in flags:
                raise RehearsalError(
                    "the runner did not pass %s, so a real host would have had "
                    "nothing to act on" % required
                )
        self.argv = list(argv)
        self.started = True
        self.console_path = pathlib.Path(flags["--console"])
        self.receipt_path = pathlib.Path(flags["--receipt"])
        self.console_path.parent.mkdir(parents=True, exist_ok=True)
        self.console_path.write_text(
            healthy_console(ready=self.reaches_readiness), encoding="utf-8"
        )
        sealed = {
            row["name"]: row["sha256"]
            for row in runner.qualification()["subject"]["images"]
        }
        self.receipt_path.write_text(
            json.dumps(
                {
                    "writtenBy": "the rehearsal stand-in, not a host",
                    "noMachineWasStarted": True,
                    "kernel": {"sha256": sealed["guest-kernel"]},
                    "rootDisk": {
                        "sha256": sealed["guest-root-disk"],
                        "attachedReadOnly": True,
                    },
                    "machine": {
                        "networkDevices": 0,
                        "sharedDirectories": 0,
                        "socketDevices": 0,
                        "storageDevices": 1,
                    },
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    def console(self) -> str:
        self._note("console")
        return self.console_path.read_text(encoding="utf-8")

    def receipt(self) -> dict:
        self._note("receipt")
        return json.loads(self.receipt_path.read_text(encoding="utf-8"))

    def request_shutdown(self) -> None:
        self._note("request_shutdown")
        if self.obeys_shutdown:
            self.stopped = True

    def force_stop(self) -> None:
        self._note("force_stop")
        self.stopped = True

    def has_stopped(self) -> bool:
        self._note("has_stopped")
        return self.stopped

    def count_the_host(self) -> dict:
        self._note("count_the_host")
        left = 0 if self.stopped else 1
        return {
            "virtualMachines": left,
            "hostProcesses": left,
            "temporaryMounts": 0,
            "socketPathPresent": False,
            "workingDirectoryPresent": False,
        }

    def seal(self, directory: pathlib.Path, report: dict) -> pathlib.Path:
        self._note("seal")
        return write_rehearsal(directory, report)


# --- the flow -----------------------------------------------------------------


def drive(directory: pathlib.Path, *, host) -> dict:
    """Walk the sequence once against whatever was handed in.

    `host` has no default on purpose.  The only way to make this function do
    anything is to supply the thing that does it, and the only supplier in the
    repository is the stand-in above.
    """

    directory = pathlib.Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    steps = []

    steps.append("refuse-what-can-be-refused-for-free")
    refusals = host.free_refusals()
    if not refusals.get("ready"):
        return {
            "steps": steps,
            "report": _report(
                steps=steps,
                refusals=refusals,
                conditions=[],
                read=None,
                stopped=False,
                stalled=False,
                sealed_to=None,
            ),
        }

    steps.append("hash-the-archive-before")
    before = host.hash_archive()

    steps.append("claim-the-one-use-mark")
    ledger = host.claim_mark()

    steps.append("start-the-machine")
    console_path = directory / "console.log"
    receipt_path = directory / "receipt.json"
    argv = runner.host_argv(
        host=directory / "no-such-host-binary",
        kernel=directory / "no-such-kernel",
        root_disk=directory / "no-such-root-disk",
        console=console_path,
        receipt=receipt_path,
        dry_run=False,
    )
    host.start(argv)

    steps.append("wait-for-readiness")
    transcript = host.console()
    read = protocol.read_transcript(transcript)
    # A pair, not a boolean. Truth-testing the pair reads every stall as ready,
    # which is how a rehearsal earns its keep.
    ready, why_ready = protocol.readiness_seen(read)

    if ready:
        steps.append("ask-for-shutdown")
        host.request_shutdown()
    else:
        steps.append(STEP_WHEN_THE_GUEST_STALLS)
        host.force_stop()

    steps.append("confirm-the-machine-is-gone")
    stopped = bool(host.has_stopped())

    steps.append("hash-the-archive-after")
    after = host.hash_archive()

    steps.append("count-the-host")
    host_after = host.count_the_host()

    steps.append("assemble-the-evidence")
    evidence = runner.evidence_from(
        transcript=transcript,
        receipt=host.receipt(),
        archive_before=before,
        archive_after=after,
        host_after=host_after,
        ledger=ledger,
        sealed=runner.resolve_sealed_evidence(),
        readiness_seen=ready,
    )

    steps.append("judge-the-conditions")
    conditions = runner.judge_pass_conditions(evidence)

    steps.append("write-what-a-real-run-would-seal")
    report = _report(
        steps=steps,
        refusals=refusals,
        conditions=conditions,
        read=read,
        stopped=stopped,
        stalled=not ready,
        sealed_to=None,
    )
    report["readiness"] = {"seen": ready, "why": why_ready}
    written = host.seal(directory, report)
    report["writtenTo"] = str(written)
    return {"steps": steps, "report": report}


def _report(*, steps, refusals, conditions, read, stopped, stalled, sealed_to) -> dict:
    return {
        "record": RECORD,
        "schema": 1,
        "isARehearsal": True,
        "machinesStarted": 0,
        "oneUseMarksCreated": 0,
        "machineStopped": stopped,
        "guestStalled": stalled,
        "whatARehearsalProves": WHAT_A_REHEARSAL_PROVES,
        "whatWasFaked": [
            "the host: nothing was started, and the stand-in wrote the console and "
            "the receipt a host would have written",
            "the guest: there was none, so every console line is a fixture rather "
            "than an observation",
            "the archive read-back: the sealed digests were reported as matching "
            "without re-reading the images, which their own tests already cover",
            "the free refusals: the stand-in answered them instead of the runner "
            "walking the real archive and ledger",
        ],
        "steps": list(steps),
        "freeRefusals": refusals,
        "guestRecordsRead": None
        if read is None
        else {
            "found": sorted(read["records"]),
            "missing": sorted(read["missing"]),
            "conflicting": sorted(read["conflicting"]),
            "malformed": list(read["malformed"]),
            "unknownRecordIds": sorted(read["unknownRecordIds"]),
        },
        "conditions": conditions,
        "metCount": len([row for row in conditions if row["verdict"] == "MET"]),
        "notMetCount": len([row for row in conditions if row["verdict"] == "NOT MET"]),
        "stillStopped": list(runner.hard_stop_conditions()),
        "overallVerdict": "REHEARSAL ONLY -- NOT A RESULT",
        "sealedResultWritten": False,
        "writtenTo": sealed_to,
        "bootAuthorisation": {"grantedByThisRecord": False, "runsPerformed": 0},
    }


# --- command ------------------------------------------------------------------


def command_rehearse(args: argparse.Namespace) -> int:
    directory = pathlib.Path(args.out).expanduser()
    host = FakeHost(
        directory,
        reaches_readiness=not args.stall,
        obeys_shutdown=not args.ignore_shutdown,
    )
    driven = drive(directory, host=host)
    report = driven["report"]
    print("steps walked: %s" % " -> ".join(driven["steps"]))
    print(
        "rehearsal: %d of %d conditions would read MET; %d machines started, "
        "%d marks claimed"
        % (
            report["metCount"],
            len(report["conditions"]),
            report["machinesStarted"],
            report["oneUseMarksCreated"],
        )
    )
    for stopped in report["stillStopped"]:
        print("still unobservable with this image: %s" % stopped)
    print("written to %s" % report.get("writtenTo"))
    print("this is not a result and authorises nothing")
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    rehearse = commands.add_parser("rehearse")
    rehearse.add_argument("--out", required=True)
    rehearse.add_argument("--stall", action="store_true")
    rehearse.add_argument("--ignore-shutdown", action="store_true")
    rehearse.set_defaults(handler=command_rehearse)
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except (RehearsalError, runner.RefusedError) as error:
        print("refused: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
