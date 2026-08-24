#!/usr/bin/env python3
"""Drive the closed-local native-shadow crash/restart adjudication gate.

Runs as root inside the named Linux gate only, after the HTTP replay matrix
has proven the normal path.  It kills the real production replay-node and
launcher processes at durable journal points and proves, with externally
countable evidence, that one answer is graded at most once and that stored
terminal results are redelivered exactly across a full process restart.

Kill safety: every signal flows through one verified-identity call site that
re-checks the systemd MainPID, its /proc start time, and its membership in
the unit's own cgroup immediately before delivery.  No name-based process
matching is used anywhere.

The module is deliberately standalone (no sibling imports) so the audited
file can be executed from any location without a package context.
"""

import argparse
import hashlib
import http.client
import json
import os
import re
import signal
import socket
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple

ROOT = Path(__file__).resolve().parents[1]
GRANT_PATH = ROOT / "native/containment/native-shadow-closed-local-replay-grant-v1.json"
FIXTURE_DIR = ROOT / "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history"
JOURNAL_PATH = Path("/var/lib/boole/native-shadow/node-state/replay-v1.ndjson")
SOCKET_PATH = Path("/run/boole/native-shadow/launcher.sock")
RUNTIME_ROOT = Path("/run/boole/native-shadow")

HOST = "127.0.0.1"
PORT = 8082
ROUTE = "/native-shadow/submissions"
RESPONSE_LIMIT_BYTES = 65_536

NODE_SERVICE = "boole-native-shadow-replay-node.service"
LAUNCHER_SERVICE = "boole-native-shadow-launcher.service"

LISTENER_WAIT_SECONDS = 120.0
IN_FLIGHT_WAIT_SECONDS = 60.0
TERMINAL_WAIT_SECONDS = 240.0
UNIT_STATE_WAIT_SECONDS = 30.0
POLL_INTERVAL_SECONDS = 0.05

UNRESOLVED_IN_FLIGHT_MESSAGE = (
    "native-shadow replay route "
    "remains closed while durable InFlight rows are unresolved"
)

PEER_MARKER_PATTERN = re.compile(
    r"^native-shadow-active-execution-peer:pid=([1-9][0-9]*)$", re.MULTILINE
)

TERMINAL_ROW_KINDS = (
    "grant_attempt_reserved_v1",
    "bootstrap_v2",
    "in_flight_v3",
    "evidence_v2",
    "terminal_consumed_v2",
)


def _is_lower_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def _strict_object(pairs: Iterable[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key in crash gate response: {}".format(key))
        result[key] = value
    return result


def _require_exact_keys(value: Any, keys: Iterable[str], label: str) -> Dict[str, Any]:
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ValueError("crash gate {} has unexpected fields".format(label))
    return value


def validate_adjudication_response(
    case_id: str, status: int, body: Dict[str, Any], expect_redelivered: bool
) -> None:
    if case_id == "accepted":
        expected_outcome = "accepted"
        expected_reason = "accepted"
        expected_verdict = "accepted"
        expected_reject_reason = None
    elif case_id == "tampered":
        expected_outcome = "deterministic_reject"
        expected_reason = "checker_rejected"
        expected_verdict = "rejected"
        expected_reject_reason = "compile-or-hidden-test-failed"
    else:
        raise ValueError("crash gate has no case: {}".format(case_id))

    _require_exact_keys(
        body,
        {"schema", "outcome", "reasonCode", "redelivered", "evidenceDigest", "receipt"},
        case_id,
    )
    receipt = _require_exact_keys(
        body["receipt"],
        {"taskId", "submissionId", "artifactRoot", "checkerHash", "verdict", "rejectReason"},
        "{} receipt".format(case_id),
    )
    if (
        status != 200
        or body["schema"] != "boole.native-shadow.adjudication.v1"
        or body["outcome"] != expected_outcome
        or body["reasonCode"] != expected_reason
        or body["redelivered"] is not expect_redelivered
        or not _is_lower_sha256(body["evidenceDigest"])
        or receipt["verdict"] != expected_verdict
        or receipt["rejectReason"] != expected_reject_reason
        or not all(
            _is_lower_sha256(receipt[field])
            for field in ("taskId", "submissionId", "artifactRoot", "checkerHash")
        )
    ):
        raise ValueError(
            "crash gate {} response drifted: status={} body={}".format(
                case_id, status, json.dumps(body, sort_keys=True, separators=(",", ":"))
            )
        )


def validate_redelivered_byte_parity(first_raw: bytes, redelivered_raw: bytes) -> None:
    """The redelivered body must differ from the first delivery only in the flag."""
    if first_raw.count(b'"redelivered":false') != 1:
        raise ValueError("first delivery does not carry exactly one redelivered:false flag")
    if redelivered_raw.count(b'"redelivered":true') != 1:
        raise ValueError("redelivery does not carry exactly one redelivered:true flag")
    if first_raw.replace(b'"redelivered":false', b'"redelivered":true') != redelivered_raw:
        raise ValueError("redelivered terminal bytes drifted from the first delivery")


def validate_identical_redelivery(first_raw: bytes, second_raw: bytes) -> None:
    if first_raw != second_raw:
        raise ValueError("two redeliveries of one terminal result returned different bytes")


def journal_kinds(text: str) -> List[Tuple[str, Any]]:
    kinds: List[Tuple[str, Any]] = []
    for line in text.splitlines():
        if not line:
            raise ValueError("crash gate journal contains an empty line")
        event = json.loads(line, object_pairs_hook=_strict_object)
        kinds.append((event.get("kind"), event.get("epoch")))
    return kinds


def require_two_case_terminal_journal(kinds: List[Tuple[str, Any]]) -> None:
    expected = [(kind, epoch) for epoch in (0, 1) for kind in TERMINAL_ROW_KINDS]
    if kinds != expected:
        raise ValueError("crash gate two-case terminal journal drifted: {}".format(kinds))


def require_unresolved_in_flight_journal(kinds: List[Tuple[str, Any]]) -> None:
    expected = [("grant_attempt_reserved_v1", 0), ("bootstrap_v2", 0), ("in_flight_v3", 0)]
    if kinds != expected:
        raise ValueError(
            "crash gate unresolved InFlight journal drifted: {}".format(kinds)
        )


@dataclass(frozen=True)
class ProcessIdentity:
    unit: str
    pid: int
    start_time: int


def _run(command: List[str], check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(command, check=check, capture_output=True, text=True)


def _systemctl_value(unit: str, prop: str) -> str:
    return _run(["systemctl", "show", unit, "--property={}".format(prop), "--value"]).stdout.strip()


def _proc_start_time(pid: int) -> int:
    stat = Path("/proc/{}/stat".format(pid)).read_text(encoding="ascii")
    # Field 22 (starttime) counted after the parenthesised comm field, which
    # may itself contain spaces; splitting after the final ')' is exact.
    fields = stat.rsplit(") ", 1)[1].split()
    return int(fields[19])


def verified_unit_main_pid(unit: str) -> ProcessIdentity:
    pid_text = _systemctl_value(unit, "MainPID")
    if not re.fullmatch(r"[1-9][0-9]*", pid_text):
        raise RuntimeError("{} has no live MainPID: {!r}".format(unit, pid_text))
    pid = int(pid_text)
    # The launcher unit delegates its cgroup subtree and moves itself into a
    # child manager cgroup, so membership is proven from the process's own
    # unified-hierarchy path: it must sit in the unit cgroup or below it.
    cgroup_text = Path("/proc/{}/cgroup".format(pid)).read_text(encoding="ascii")
    match = re.search(r"^0::(.+)$", cgroup_text, re.MULTILINE)
    unit_cgroup = "/system.slice/{}".format(unit)
    if match is None or not (
        match.group(1) == unit_cgroup or match.group(1).startswith(unit_cgroup + "/")
    ):
        raise RuntimeError(
            "{} MainPID {} is outside its own unit cgroup: {!r}".format(
                unit, pid, cgroup_text
            )
        )
    return ProcessIdentity(unit=unit, pid=pid, start_time=_proc_start_time(pid))


def deliver_verified_signal(identity: ProcessIdentity, signum: int) -> None:
    fresh = verified_unit_main_pid(identity.unit)
    if fresh != identity:
        raise RuntimeError(
            "refusing to signal {}: recorded identity {} drifted to {}".format(
                identity.unit, identity, fresh
            )
        )
    os.kill(identity.pid, signum)


def unit_invocation(unit: str) -> str:
    invocation = _systemctl_value(unit, "InvocationID")
    if not re.fullmatch(r"[0-9a-f]{32}", invocation):
        raise RuntimeError("{} has an invalid InvocationID: {!r}".format(unit, invocation))
    return invocation


def wait_for_unit_state(unit: str, wanted: Iterable[str], deadline_seconds: float) -> str:
    wanted_set = set(wanted)
    deadline = time.monotonic() + deadline_seconds
    state = ""
    while time.monotonic() < deadline:
        state = _systemctl_value(unit, "ActiveState")
        if state in wanted_set:
            return state
        time.sleep(POLL_INTERVAL_SECONDS)
    raise RuntimeError(
        "{} did not reach {} in time (last state: {})".format(unit, sorted(wanted_set), state)
    )


def require_unit_down(unit: str) -> None:
    state = _systemctl_value(unit, "ActiveState")
    if state == "failed":
        _run(["systemctl", "reset-failed", unit], check=False)
        state = _systemctl_value(unit, "ActiveState")
    if state != "inactive":
        raise RuntimeError("{} must be down before a crash scenario, saw {}".format(unit, state))


def freeze_journal_cursor() -> str:
    _run(["journalctl", "--sync"])
    output = _run(["journalctl", "--no-pager", "--show-cursor", "-n", "0"]).stdout
    match = re.search(r"^-- cursor: (\S+)", output, re.MULTILINE)
    if match is None:
        raise RuntimeError("could not freeze a systemd journal cursor")
    return match.group(1)


def peer_marker_pids(cursor: str) -> List[str]:
    _run(["journalctl", "--sync"])
    output = _run(
        [
            "journalctl",
            "--no-pager",
            "-o",
            "cat",
            "-u",
            LAUNCHER_SERVICE,
            "--after-cursor",
            cursor,
        ]
    ).stdout
    return PEER_MARKER_PATTERN.findall(output)


def node_unit_log(cursor: str) -> str:
    _run(["journalctl", "--sync"])
    return _run(
        [
            "journalctl",
            "--no-pager",
            "-o",
            "cat",
            "-u",
            NODE_SERVICE,
            "--after-cursor",
            cursor,
        ]
    ).stdout


def wait_for_listener(deadline_seconds: float = LISTENER_WAIT_SECONDS) -> None:
    deadline = time.monotonic() + deadline_seconds
    while True:
        try:
            with socket.create_connection((HOST, PORT), timeout=1.0):
                return
        except OSError:
            if time.monotonic() >= deadline:
                raise RuntimeError("crash gate HTTP listener did not become ready")
            time.sleep(POLL_INTERVAL_SECONDS)


def require_listener_refused() -> None:
    try:
        with socket.create_connection((HOST, PORT), timeout=1.0):
            pass
    except ConnectionRefusedError:
        return
    raise RuntimeError("crash gate listener accepted a connection while it must stay closed")


def _encode_payload(payload: Dict[str, Any]) -> bytes:
    return json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def post_and_read(payload: Dict[str, Any]) -> Tuple[int, bytes, Dict[str, Any]]:
    encoded = _encode_payload(payload)
    connection = http.client.HTTPConnection(HOST, PORT, timeout=130.0)
    try:
        connection.request(
            "POST",
            ROUTE,
            body=encoded,
            headers={"Content-Type": "application/json", "Content-Length": str(len(encoded))},
        )
        response = connection.getresponse()
        raw = response.read(RESPONSE_LIMIT_BYTES + 1)
        if len(raw) > RESPONSE_LIMIT_BYTES:
            raise ValueError("crash gate response exceeded the fixed cap")
        body = json.loads(raw.decode("utf-8"), object_pairs_hook=_strict_object)
        if not isinstance(body, dict):
            raise ValueError("crash gate response is not one JSON object")
        return response.status, raw, body
    finally:
        connection.close()


def post_without_reading(payload: Dict[str, Any]) -> socket.socket:
    """Send one full request and return the open socket without reading a byte.

    The caller closes the socket at the scenario's chosen durable point,
    modelling a client that disappears before the terminal response exists.
    """
    encoded = _encode_payload(payload)
    head = (
        "POST {route} HTTP/1.1\r\n"
        "Host: {host}:{port}\r\n"
        "Content-Type: application/json\r\n"
        "Content-Length: {length}\r\n"
        "Connection: close\r\n\r\n"
    ).format(route=ROUTE, host=HOST, port=PORT, length=len(encoded)).encode("ascii")
    sock = socket.create_connection((HOST, PORT), timeout=10.0)
    sock.sendall(head + encoded)
    return sock


def read_journal_text(journal_path: Path) -> str:
    if not journal_path.exists():
        return ""
    return journal_path.read_text(encoding="utf-8")


def wait_for_journal_row(
    journal_path: Path, kind: str, epoch: int, deadline_seconds: float
) -> None:
    deadline = time.monotonic() + deadline_seconds
    while True:
        text = read_journal_text(journal_path)
        try:
            if (kind, epoch) in journal_kinds(text):
                return
        except ValueError:
            pass  # A row may be mid-append; the durable state settles below.
        if time.monotonic() >= deadline:
            raise RuntimeError(
                "journal row ({}, {}) did not appear in time; journal: {!r}".format(
                    kind, epoch, read_journal_text(journal_path)[-2048:]
                )
            )
        time.sleep(POLL_INTERVAL_SECONDS)


def reap_inert_socket_inode(socket_path: Path) -> bool:
    """Remove a leftover launcher socket inode after proving it is dead.

    The launcher only unlinks its fixed socket on its own graceful exits, so
    a launcher stopped over SIGTERM leaves the bound inode behind (the
    manager gate's exit trap reaps it the same way).  A live listener must
    never be reaped: a connection probe has to be refused first.
    """
    if not os.path.lexists(socket_path):
        return False
    probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        probe.settimeout(1.0)
        try:
            probe.connect(str(socket_path))
        except ConnectionRefusedError:
            pass
        except FileNotFoundError:
            return False
        else:
            raise RuntimeError(
                "fixed launcher socket still accepts connections after stop"
            )
    finally:
        probe.close()
    try:
        os.unlink(socket_path)
    except FileNotFoundError:
        return False
    return True


def stop_and_verify_clean(socket_path: Path) -> Dict[str, Any]:
    _run(["systemctl", "stop", NODE_SERVICE], check=False)
    _run(["systemctl", "stop", LAUNCHER_SERVICE], check=False)
    for unit in (NODE_SERVICE, LAUNCHER_SERVICE):
        wait_for_unit_state(unit, ("inactive", "failed"), UNIT_STATE_WAIT_SECONDS)
        _run(["systemctl", "reset-failed", unit], check=False)
        if _systemctl_value(unit, "MainPID") != "0":
            raise RuntimeError("{} left a main process behind after stop".format(unit))
        cgroup_dir = Path("/sys/fs/cgroup/system.slice/{}".format(unit))
        deadline = time.monotonic() + UNIT_STATE_WAIT_SECONDS
        while cgroup_dir.exists():
            if time.monotonic() >= deadline:
                raise RuntimeError("{} left its cgroup behind after stop".format(unit))
            time.sleep(POLL_INTERVAL_SECONDS)
    reaped = reap_inert_socket_inode(socket_path)
    if os.path.lexists(socket_path):
        raise RuntimeError("crash gate left the fixed launcher socket behind")
    if RUNTIME_ROOT.exists():
        for entry in RUNTIME_ROOT.iterdir():
            if entry.name.startswith("rootfs-"):
                raise RuntimeError("crash gate left a derived runtime root behind")
    return {"verified": True, "inertSocketInodeReaped": reaped}


def load_grant(grant_path: Path) -> Tuple[Dict[str, Any], Dict[str, Dict[str, Any]]]:
    grant = json.loads(grant_path.read_text(encoding="utf-8"), object_pairs_hook=_strict_object)
    required_scope = {
        "namedLinuxVerificationReplayOnly": True,
        "maxMatrixRequestsTotal": 4,
        "maxCheckerExecutionsTotal": 3,
        "loopbackOnly": True,
        "p2pAllowed": False,
        "consensusAllowed": False,
        "rewardAllowed": False,
        "mineableNow": False,
        "activationAllowed": False,
        "nonIssuable": True,
    }
    if grant.get("scope") != required_scope:
        raise ValueError("closed-local replay grant scope drifted")
    cases = {case["caseId"]: case for case in grant["cases"]}
    return grant, cases


def load_raw_answer(fixture_directory: Path, case: Dict[str, Any], filename: str) -> str:
    raw_answer = (fixture_directory / filename).read_text(encoding="utf-8")
    if hashlib.sha256(raw_answer.encode("utf-8")).hexdigest() != case["rawAnswerSha256"]:
        raise ValueError("{} raw answer differs from the replay grant".format(case["caseId"]))
    return raw_answer


def submission_payload(task: Dict[str, Any], case: Dict[str, Any], raw_answer: str) -> Dict[str, Any]:
    return {
        "schema": "boole.native-shadow.submission.v1",
        "familyVersion": task["familyVersion"],
        "templateId": task["templateId"],
        "challengeSha256": task["challengeSha256"],
        "epoch": case["epoch"],
        "rawAnswer": raw_answer,
    }


def identity_record(identity: ProcessIdentity, invocation: str) -> Dict[str, Any]:
    return {"pid": identity.pid, "startTime": identity.start_time, "invocationId": invocation}


def scenario_terminal_redelivery_across_node_kill(
    task: Dict[str, Any],
    accepted_payload: Dict[str, Any],
    tampered_payload: Dict[str, Any],
    journal_path: Path,
    socket_path: Path,
) -> Dict[str, Any]:
    require_unit_down(NODE_SERVICE)
    require_unit_down(LAUNCHER_SERVICE)
    journal_path.unlink(missing_ok=True)
    cursor = freeze_journal_cursor()
    _run(["systemctl", "start", NODE_SERVICE])
    wait_for_listener()
    node_before = verified_unit_main_pid(NODE_SERVICE)
    launcher_before = verified_unit_main_pid(LAUNCHER_SERVICE)
    node_invocation_before = unit_invocation(NODE_SERVICE)
    launcher_invocation_before = unit_invocation(LAUNCHER_SERVICE)

    # Crash point C: the accepted client vanishes before any response bytes
    # exist, so the terminal verdict is durably saved but never delivered.
    silent_client = post_without_reading(accepted_payload)
    wait_for_journal_row(journal_path, "in_flight_v3", 0, IN_FLIGHT_WAIT_SECONDS)
    silent_client.close()
    wait_for_journal_row(journal_path, "terminal_consumed_v2", 0, TERMINAL_WAIT_SECONDS)

    status, tampered_first_raw, tampered_first = post_and_read(tampered_payload)
    validate_adjudication_response("tampered", status, tampered_first, expect_redelivered=False)

    # Crash point D: the disconnect above cancelled nothing; the same node
    # instance redelivers the saved accepted terminal without a second run.
    status, accepted_first_raw, accepted_first = post_and_read(accepted_payload)
    validate_adjudication_response("accepted", status, accepted_first, expect_redelivered=True)

    require_two_case_terminal_journal(journal_kinds(read_journal_text(journal_path)))
    markers_before_kill = peer_marker_pids(cursor)
    if markers_before_kill != [str(node_before.pid)] * 2:
        raise RuntimeError(
            "expected exactly two checker executions from the fixed node process, "
            "saw: {}".format(markers_before_kill)
        )

    deliver_verified_signal(node_before, signal.SIGKILL)
    wait_for_unit_state(NODE_SERVICE, ("failed",), UNIT_STATE_WAIT_SECONDS)
    require_listener_refused()
    # The half-consumed launcher receives an orderly report-free stop; the
    # spec treats that as crash-equivalent for its execution ledger.
    _run(["systemctl", "stop", LAUNCHER_SERVICE], check=False)
    wait_for_unit_state(LAUNCHER_SERVICE, ("inactive", "failed"), UNIT_STATE_WAIT_SECONDS)
    _run(["systemctl", "reset-failed", NODE_SERVICE], check=False)
    _run(["systemctl", "reset-failed", LAUNCHER_SERVICE], check=False)

    _run(["systemctl", "start", NODE_SERVICE])
    wait_for_listener()
    node_after = verified_unit_main_pid(NODE_SERVICE)
    launcher_after = verified_unit_main_pid(LAUNCHER_SERVICE)
    node_invocation_after = unit_invocation(NODE_SERVICE)
    launcher_invocation_after = unit_invocation(LAUNCHER_SERVICE)
    if (node_after.pid, node_after.start_time) == (node_before.pid, node_before.start_time):
        raise RuntimeError("the replay node did not restart as a new process")
    if launcher_invocation_after == launcher_invocation_before:
        raise RuntimeError("the launcher did not restart as a new instance")
    if node_invocation_after == node_invocation_before:
        raise RuntimeError("the replay node did not restart as a new invocation")

    status, accepted_again_raw, accepted_again = post_and_read(accepted_payload)
    validate_adjudication_response("accepted", status, accepted_again, expect_redelivered=True)
    validate_identical_redelivery(accepted_first_raw, accepted_again_raw)

    status, tampered_again_raw, tampered_again = post_and_read(tampered_payload)
    validate_adjudication_response("tampered", status, tampered_again, expect_redelivered=True)
    validate_redelivered_byte_parity(tampered_first_raw, tampered_again_raw)

    require_two_case_terminal_journal(journal_kinds(read_journal_text(journal_path)))
    markers_total = peer_marker_pids(cursor)
    if markers_total != markers_before_kill:
        raise RuntimeError(
            "checker executions changed across restart: {} -> {}".format(
                markers_before_kill, markers_total
            )
        )

    cleanup = stop_and_verify_clean(socket_path)
    return {
        "scenario": "terminal-redelivery-across-node-kill",
        "challengeSha256": task["challengeSha256"],
        "cases": [
            {"caseId": "accepted", "epoch": 0},
            {"caseId": "tampered", "epoch": 1},
        ],
        "nodeBefore": identity_record(node_before, node_invocation_before),
        "nodeAfter": identity_record(node_after, node_invocation_after),
        "launcherBefore": identity_record(launcher_before, launcher_invocation_before),
        "launcherAfter": identity_record(launcher_after, launcher_invocation_after),
        "checkerStartsBeforeKill": len(markers_before_kill),
        "checkerStartsTotal": len(markers_total),
        "checkerStartsAfterRestart": len(markers_total) - len(markers_before_kill),
        "redeliveredResponses": 3,
        "journalRows": 10,
        "acceptedRedeliveryByteIdentical": True,
        "tamperedRedeliveryFlagOnlyDelta": True,
        "cleanup": cleanup,
    }


def scenario_unresolved_inflight_fail_closed(
    task: Dict[str, Any],
    accepted_payload: Dict[str, Any],
    journal_path: Path,
    socket_path: Path,
) -> Dict[str, Any]:
    require_unit_down(NODE_SERVICE)
    require_unit_down(LAUNCHER_SERVICE)
    journal_path.unlink(missing_ok=True)
    cursor = freeze_journal_cursor()
    _run(["systemctl", "start", NODE_SERVICE])
    wait_for_listener()
    node_identity = verified_unit_main_pid(NODE_SERVICE)
    launcher_identity = verified_unit_main_pid(LAUNCHER_SERVICE)
    node_invocation_id = unit_invocation(NODE_SERVICE)
    launcher_invocation_id = unit_invocation(LAUNCHER_SERVICE)

    # Crash points A/B: freeze the qualified launcher so the request parks at
    # the durable InFlight row, then kill the node before any verdict exists.
    deliver_verified_signal(launcher_identity, signal.SIGSTOP)
    parked_client = post_without_reading(accepted_payload)
    wait_for_journal_row(journal_path, "in_flight_v3", 0, IN_FLIGHT_WAIT_SECONDS)
    require_unresolved_in_flight_journal(journal_kinds(read_journal_text(journal_path)))

    deliver_verified_signal(node_identity, signal.SIGKILL)
    wait_for_unit_state(NODE_SERVICE, ("failed",), UNIT_STATE_WAIT_SECONDS)
    # The launcher dies while still frozen, so it can never service the
    # parked connection and no checker ever starts for this attempt.
    deliver_verified_signal(launcher_identity, signal.SIGKILL)
    parked_client.close()
    _run(["systemctl", "stop", LAUNCHER_SERVICE], check=False)
    wait_for_unit_state(LAUNCHER_SERVICE, ("inactive", "failed"), UNIT_STATE_WAIT_SECONDS)
    _run(["systemctl", "reset-failed", NODE_SERVICE], check=False)
    _run(["systemctl", "reset-failed", LAUNCHER_SERVICE], check=False)

    restart_cursor = freeze_journal_cursor()
    _run(["systemctl", "start", NODE_SERVICE], check=False)
    wait_for_unit_state(NODE_SERVICE, ("failed",), IN_FLIGHT_WAIT_SECONDS)
    require_listener_refused()

    message_deadline = time.monotonic() + UNIT_STATE_WAIT_SECONDS
    while UNRESOLVED_IN_FLIGHT_MESSAGE not in node_unit_log(restart_cursor):
        if time.monotonic() >= message_deadline:
            raise RuntimeError(
                "the restarted node did not report the unresolved InFlight refusal"
            )
        time.sleep(POLL_INTERVAL_SECONDS)

    require_unresolved_in_flight_journal(journal_kinds(read_journal_text(journal_path)))
    markers = peer_marker_pids(cursor)
    if markers:
        raise RuntimeError(
            "a checker execution happened during the ambiguous scenario: {}".format(markers)
        )

    cleanup = stop_and_verify_clean(socket_path)
    return {
        "scenario": "unresolved-inflight-fail-closed",
        "challengeSha256": task["challengeSha256"],
        "case": {"caseId": "accepted", "epoch": 0},
        "node": identity_record(node_identity, node_invocation_id),
        "launcher": identity_record(launcher_identity, launcher_invocation_id),
        "checkerStarts": 0,
        "journalRows": 3,
        "restartRefused": True,
        "failClosedMessageObserved": True,
        "listenerRefusedAfterRestartAttempt": True,
        "cleanup": cleanup,
    }


def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="drive the closed-local native-shadow crash/restart gate"
    )
    parser.add_argument("--grant-path", type=Path, default=GRANT_PATH)
    parser.add_argument("--fixture-directory", type=Path, default=FIXTURE_DIR)
    parser.add_argument("--journal-path", type=Path, default=JOURNAL_PATH)
    parser.add_argument("--socket-path", type=Path, default=SOCKET_PATH)
    return parser.parse_args(argv)


def main(
    grant_path: Path,
    fixture_directory: Path,
    journal_path: Path,
    socket_path: Path,
) -> int:
    grant, cases = load_grant(grant_path)
    task = grant["task"]
    accepted_raw = load_raw_answer(fixture_directory, cases["accepted"], "replay-accepted.raw.txt")
    tampered_raw = load_raw_answer(fixture_directory, cases["tampered"], "replay-tampered.raw.txt")
    accepted_payload = submission_payload(task, cases["accepted"], accepted_raw)
    tampered_payload = submission_payload(task, cases["tampered"], tampered_raw)

    evidence = scenario_terminal_redelivery_across_node_kill(
        task, accepted_payload, tampered_payload, journal_path, socket_path
    )
    print(
        "native-shadow-crash-restart-evidence:{}".format(
            json.dumps(evidence, sort_keys=True, separators=(",", ":"))
        ),
        flush=True,
    )
    print(
        "native-shadow-crash-restart-case:terminal-redelivery-across-node-kill:PASS",
        flush=True,
    )

    evidence = scenario_unresolved_inflight_fail_closed(
        task, accepted_payload, journal_path, socket_path
    )
    print(
        "native-shadow-crash-restart-evidence:{}".format(
            json.dumps(evidence, sort_keys=True, separators=(",", ":"))
        ),
        flush=True,
    )
    print(
        "native-shadow-crash-restart-case:unresolved-inflight-fail-closed:PASS",
        flush=True,
    )

    print("native-shadow-crash-restart-gate:PASS", flush=True)
    return 0


if __name__ == "__main__":
    args = parse_args()
    raise SystemExit(
        main(args.grant_path, args.fixture_directory, args.journal_path, args.socket_path)
    )
