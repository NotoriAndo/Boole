#!/usr/bin/env python3
"""Installed Mac crash/restart E2E for the closed-local native-shadow route."""

from __future__ import annotations

import argparse
import hashlib
import http.client
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from scripts import native_shadow_installed_mac_e2e_v1 as installed


CONTROLLER_EXECUTION_MARKER = "boole-mac4-controller-command:execution"
UNRESOLVED_IN_FLIGHT_MESSAGE = (
    "native-shadow replay route remains closed while durable InFlight rows are unresolved"
)
HTTP_PORT = 8082
JOURNAL_WAIT_SECONDS = 15
PROCESS_WAIT_SECONDS = 35


def _write_json_atomic(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def _payloads(grant_path: Path, fixture_dir: Path) -> dict[str, dict[str, object]]:
    grant = installed._read_object(grant_path)
    task = grant.get("task")
    cases = grant.get("cases")
    if not isinstance(task, dict) or not isinstance(cases, list):
        raise ValueError("installed Mac crash E2E grant shape drifted")
    result: dict[str, dict[str, object]] = {}
    for case in cases:
        if not isinstance(case, dict):
            raise ValueError("installed Mac crash E2E case is not an object")
        case_id = case.get("caseId")
        if case_id not in {"accepted", "tampered"}:
            continue
        raw_answer = installed._raw_answer(fixture_dir, case)
        result[str(case_id)] = {
            "schema": "boole.native-shadow.submission.v1",
            "familyVersion": task["familyVersion"],
            "templateId": task["templateId"],
            "challengeSha256": task["challengeSha256"],
            "epoch": case["epoch"],
            "rawAnswer": raw_answer,
        }
    if set(result) != {"accepted", "tampered"}:
        raise ValueError("installed Mac crash E2E grant lacks its two cases")
    return result


def _encode_submission(payload: dict[str, object]) -> bytes:
    return json.dumps(payload, separators=(",", ":")).encode("utf-8")


def post_and_read(payload: dict[str, object]) -> tuple[int, bytes, dict[str, Any]]:
    encoded = _encode_submission(payload)
    connection = http.client.HTTPConnection("127.0.0.1", HTTP_PORT, timeout=120)
    try:
        connection.request(
            "POST",
            "/native-shadow/submissions",
            body=encoded,
            headers={"Content-Type": "application/json", "Content-Length": str(len(encoded))},
        )
        response = connection.getresponse()
        raw = response.read(65_537)
        if len(raw) > 65_536:
            raise ValueError("installed Mac crash response exceeds its cap")
        value = json.loads(raw, object_pairs_hook=installed._strict_object)
        if not isinstance(value, dict):
            raise ValueError("installed Mac crash response is not one object")
        return response.status, raw, value
    finally:
        connection.close()


def post_without_reading(payload: dict[str, object]) -> socket.socket:
    encoded = _encode_submission(payload)
    request = (
        b"POST /native-shadow/submissions HTTP/1.1\r\n"
        b"Host: 127.0.0.1\r\n"
        b"Content-Type: application/json\r\n"
        + f"Content-Length: {len(encoded)}\r\n".encode("ascii")
        + b"Connection: close\r\n\r\n"
        + encoded
    )
    client = socket.create_connection(("127.0.0.1", HTTP_PORT), timeout=5)
    client.sendall(request)
    return client


def validate_response(
    case_id: str,
    status: int,
    value: dict[str, Any],
    *,
    redelivered: bool,
) -> None:
    expected = {
        "accepted": (200, "accepted", "accepted"),
        "tampered": (200, "deterministic_reject", "checker_rejected"),
    }[case_id]
    if (
        status != expected[0]
        or value.get("outcome") != expected[1]
        or value.get("reasonCode") != expected[2]
        or value.get("redelivered") is not redelivered
    ):
        raise ValueError(f"installed Mac crash response drifted for {case_id}: {value}")


def validate_flag_only_redelivery(first: bytes, second: bytes) -> None:
    if first.count(b'"redelivered":false') != 1:
        raise ValueError("first delivery lacks one false redelivery flag")
    if second.count(b'"redelivered":true') != 1:
        raise ValueError("redelivery lacks one true redelivery flag")
    if first.replace(b'"redelivered":false', b'"redelivered":true') != second:
        raise ValueError("terminal redelivery changed bytes beyond its flag")


def read_journal(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        value = json.loads(line, object_pairs_hook=installed._strict_object)
        if not isinstance(value, dict) or not isinstance(value.get("kind"), str):
            raise ValueError("installed Mac crash journal row drifted")
        rows.append(value)
    return rows


def wait_for_journal(path: Path, kind: str, epoch: int) -> list[dict[str, Any]]:
    deadline = time.monotonic() + JOURNAL_WAIT_SECONDS
    while time.monotonic() < deadline:
        if path.is_file():
            rows = read_journal(path)
            if any(row.get("kind") == kind and row.get("epoch") == epoch for row in rows):
                return rows
        time.sleep(0.02)
    raise ValueError(f"installed Mac crash journal did not reach {kind} epoch {epoch}")


def require_terminal_journal(rows: list[dict[str, Any]]) -> None:
    expected = [
        ("grant_attempt_reserved_v1", 0),
        ("bootstrap_v2", 0),
        ("in_flight_v3", 0),
        ("evidence_v2", 0),
        ("terminal_consumed_v2", 0),
        ("grant_attempt_reserved_v1", 1),
        ("bootstrap_v2", 1),
        ("in_flight_v3", 1),
        ("evidence_v2", 1),
        ("terminal_consumed_v2", 1),
    ]
    observed = [(row["kind"], row.get("epoch")) for row in rows]
    if observed != expected:
        raise ValueError(f"installed Mac terminal journal drifted: {observed}")


def require_unresolved_journal(rows: list[dict[str, Any]]) -> None:
    expected = [
        ("grant_attempt_reserved_v1", 0),
        ("bootstrap_v2", 0),
        ("in_flight_v3", 0),
    ]
    observed = [(row["kind"], row.get("epoch")) for row in rows]
    if observed != expected:
        raise ValueError(f"installed Mac unresolved journal drifted: {observed}")


def _node_command(
    node: Path,
    install_root: Path,
    runtime_root: Path,
    journal_path: Path,
    trust_roots: dict[str, Any],
) -> list[str]:
    return [
        str(node),
        "--install-root",
        str(install_root),
        "--runtime-root",
        str(runtime_root),
        "--journal-path",
        str(journal_path),
        "--product-trust-root-key-id",
        str(trust_roots["productKeyId"]),
        "--product-trust-root-public-key",
        str(trust_roots["productPublicKeyHex"]),
        "--guest-trust-root-key-id",
        str(trust_roots["guestKeyId"]),
        "--guest-trust-root-public-key",
        str(trust_roots["guestPublicKeyHex"]),
    ]


def start_node(
    install_root: Path,
    runtime_root: Path,
    journal_path: Path,
    log_root: Path,
    trust_roots: dict[str, Any],
) -> subprocess.Popen[bytes]:
    if not runtime_root.exists():
        installed._prepare_private_directory(runtime_root)
    if not journal_path.parent.exists():
        installed._prepare_private_directory(journal_path.parent)
    installed._prepare_private_directory(log_root)
    stdout = (log_root / "node.stdout").open("xb")
    stderr = (log_root / "node.stderr").open("xb")
    try:
        process = subprocess.Popen(
            _node_command(
                installed._installed_host_node(install_root),
                install_root,
                runtime_root,
                journal_path,
                trust_roots,
            ),
            stdout=stdout,
            stderr=stderr,
            start_new_session=True,
        )
    finally:
        stdout.close()
        stderr.close()
    return process


def wait_for_listener(process: subprocess.Popen[bytes], timeout: int = 120) -> None:
    installed._wait_for_node(process, timeout)


def process_table() -> list[tuple[int, int, int, str]]:
    completed = subprocess.run(
        ["ps", "-axo", "pid=,ppid=,pgid=,command="],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
        timeout=10,
    )
    if completed.returncode != 0:
        raise ValueError("installed Mac crash process table is unavailable")
    rows: list[tuple[int, int, int, str]] = []
    for line in completed.stdout.splitlines():
        fields = line.strip().split(None, 3)
        if len(fields) == 4 and all(value.isdigit() for value in fields[:3]):
            rows.append((int(fields[0]), int(fields[1]), int(fields[2]), fields[3]))
    return rows


def controller_child(node_pid: int, runtime_root: Path) -> int:
    expected = str(runtime_root / "active-controller" / "host-controller")
    deadline = time.monotonic() + PROCESS_WAIT_SECONDS
    while time.monotonic() < deadline:
        matches = [
            row
            for row in process_table()
            if row[1] == node_pid and row[2] == node_pid and row[3].split()[0] == expected
        ]
        if len(matches) == 1:
            return matches[0][0]
        if len(matches) > 1:
            raise ValueError("installed Mac crash found multiple controller children")
        time.sleep(0.05)
    raise ValueError("installed Mac crash controller child did not appear")


def kill_process_group(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        raise ValueError("installed Mac crash target exited before the kill point")
    os.killpg(process.pid, signal.SIGKILL)
    process.wait(timeout=10)
    deadline = time.monotonic() + PROCESS_WAIT_SECONDS
    while time.monotonic() < deadline:
        if not any(row[2] == process.pid for row in process_table()):
            return
        time.sleep(0.05)
    raise ValueError("installed Mac crash process group survived SIGKILL")


def stop_cleanly(process: subprocess.Popen[bytes]) -> None:
    process.send_signal(signal.SIGTERM)
    if process.wait(timeout=40) != 0:
        raise ValueError("installed Mac crash restarted node did not stop cleanly")


def cleanup_process_group(process: subprocess.Popen[bytes] | None) -> None:
    """Leave no node-owned controller or VM process behind after a failed E2E."""
    if process is None:
        return
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=10)
    deadline = time.monotonic() + PROCESS_WAIT_SECONDS
    while time.monotonic() < deadline:
        if not any(row[2] == process.pid for row in process_table()):
            return
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        time.sleep(0.05)
    raise ValueError("installed Mac crash cleanup left a node-owned process group")


def execution_markers(*log_roots: Path) -> int:
    return sum(
        (root / "node.stderr").read_text(encoding="utf-8", errors="replace").count(
            CONTROLLER_EXECUTION_MARKER
        )
        for root in log_roots
    )


def terminal_redelivery_scenario(
    install_root: Path,
    scenario_root: Path,
    trust_roots: dict[str, Any],
    payloads: dict[str, dict[str, object]],
) -> dict[str, Any]:
    runtime = scenario_root / "runtime"
    journal = scenario_root / "state" / "replay.ndjson"
    first_logs = scenario_root / "node-before"
    second_logs = scenario_root / "node-after"
    first: subprocess.Popen[bytes] | None = None
    second: subprocess.Popen[bytes] | None = None
    silent: socket.socket | None = None
    try:
        first = start_node(install_root, runtime, journal, first_logs, trust_roots)
        wait_for_listener(first)
        first_pid = first.pid

        silent = post_without_reading(payloads["accepted"])
        wait_for_journal(journal, "terminal_consumed_v2", 0)
        silent.close()
        silent = None
        status, tampered_raw, tampered = post_and_read(payloads["tampered"])
        validate_response("tampered", status, tampered, redelivered=False)
        status, accepted_before_raw, accepted_before = post_and_read(payloads["accepted"])
        validate_response("accepted", status, accepted_before, redelivered=True)
        require_terminal_journal(read_journal(journal))
        journal_before = journal.read_bytes()
        if execution_markers(first_logs) != 2:
            raise ValueError("installed Mac crash expected two checker executions before kill")

        kill_process_group(first)
        second = start_node(install_root, runtime, journal, second_logs, trust_roots)
        wait_for_listener(second)
        if second.pid == first_pid:
            raise ValueError("installed Mac crash node PID did not change")
        status, accepted_after_raw, accepted_after = post_and_read(payloads["accepted"])
        validate_response("accepted", status, accepted_after, redelivered=True)
        if accepted_after_raw != accepted_before_raw:
            raise ValueError("accepted terminal redelivery changed across restart")
        status, tampered_after_raw, tampered_after = post_and_read(payloads["tampered"])
        validate_response("tampered", status, tampered_after, redelivered=True)
        validate_flag_only_redelivery(tampered_raw, tampered_after_raw)
        if journal.read_bytes() != journal_before:
            raise ValueError("installed Mac crash redelivery appended journal rows")
        if execution_markers(first_logs, second_logs) != 2:
            raise ValueError("installed Mac crash relaunched the checker after restart")
        stop_cleanly(second)
        installed.require_controller_runtime_clean(runtime)
        return {
            "scenario": "terminal-redelivery-across-node-controller-guest-loss",
            "nodePidChanged": True,
            "checkerExecutionsBeforeCrash": 2,
            "checkerExecutionsAfterRestart": 0,
            "journalRows": 10,
            "journalBytesUnchangedAfterRestart": True,
            "acceptedRedeliveryByteIdentical": True,
            "tamperedRedeliveryFlagOnlyDelta": True,
            "runtimeClean": True,
        }
    finally:
        if silent is not None:
            silent.close()
        cleanup_process_group(second)
        cleanup_process_group(first)


def unresolved_inflight_scenario(
    install_root: Path,
    scenario_root: Path,
    trust_roots: dict[str, Any],
    payloads: dict[str, dict[str, object]],
) -> dict[str, Any]:
    runtime = scenario_root / "runtime"
    journal = scenario_root / "state" / "replay.ndjson"
    first_logs = scenario_root / "node-before"
    second_logs = scenario_root / "node-after"
    first: subprocess.Popen[bytes] | None = None
    second: subprocess.Popen[bytes] | None = None
    parked: socket.socket | None = None
    try:
        first = start_node(install_root, runtime, journal, first_logs, trust_roots)
        wait_for_listener(first)
        controller_pid = controller_child(first.pid, runtime)
        os.kill(controller_pid, signal.SIGSTOP)
        parked = post_without_reading(payloads["accepted"])
        rows = wait_for_journal(journal, "in_flight_v3", 0)
        require_unresolved_journal(rows)
        kill_process_group(first)
        parked.close()
        parked = None

        second = start_node(install_root, runtime, journal, second_logs, trust_roots)
        if second.wait(timeout=140) == 0:
            raise ValueError("installed Mac crash unresolved InFlight restart unexpectedly served")
        try:
            refused = socket.create_connection(("127.0.0.1", HTTP_PORT), timeout=0.2)
        except OSError:
            pass
        else:
            refused.close()
            raise ValueError("installed Mac crash unresolved restart opened its listener")
        stderr = (second_logs / "node.stderr").read_text(encoding="utf-8", errors="replace")
        if UNRESOLVED_IN_FLIGHT_MESSAGE not in stderr:
            raise ValueError("installed Mac crash unresolved restart lacked fail-closed evidence")
        require_unresolved_journal(read_journal(journal))
        if execution_markers(first_logs, second_logs) != 0:
            raise ValueError("installed Mac crash ran a checker in the unresolved scenario")
        installed.require_controller_runtime_clean(runtime)
        return {
            "scenario": "unresolved-inflight-fail-closed-after-node-controller-guest-loss",
            "checkerExecutions": 0,
            "journalRows": 3,
            "restartRefused": True,
            "listenerRefused": True,
            "failClosedMessageObserved": True,
            "runtimeClean": True,
        }
    finally:
        if parked is not None:
            parked.close()
        cleanup_process_group(second)
        cleanup_process_group(first)


def execute(args: argparse.Namespace) -> dict[str, Any]:
    work_root = Path(args.work).resolve()
    if work_root.exists():
        raise ValueError("installed Mac crash E2E work root must start absent")
    installed._prepare_private_directory(work_root)
    plan_path = Path(args.kat_plan).resolve()
    plan = installed._read_object(plan_path)
    output = work_root / "http-root"
    if Path(str(plan.get("outputDir", ""))).resolve() != output:
        raise ValueError("installed Mac crash KAT plan targets another output root")
    cli = Path(args.cli).resolve()
    product = plan.get("productArtifacts")
    if not isinstance(product, dict) or Path(str(product.get("host-cli", ""))).resolve() != cli:
        raise ValueError("installed Mac crash CLI differs from the signed host role")

    try:
        kat = subprocess.run(
            [str(Path(args.kat_binary).resolve()), str(plan_path)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
            timeout=120,
        )
        if kat.returncode != 0:
            raise ValueError("installed Mac crash KAT metadata failed: " + kat.stderr.strip())
        roots = installed._read_object(output / "TRUST-ROOTS.json")
        installed.materialize_transport_layout(plan_path, output)
        install = installed.install_direct_boot_bundle(
            cli,
            output,
            work_root / "install-root",
            work_root / "download-staging",
            roots,
            timeout_seconds=args.install_timeout_seconds,
        )
        payloads = _payloads(Path(args.grant).resolve(), Path(args.fixtures).resolve())
        terminal = terminal_redelivery_scenario(
            work_root / "install-root", work_root / "terminal", roots, payloads
        )
        unresolved = unresolved_inflight_scenario(
            work_root / "install-root", work_root / "unresolved", roots, payloads
        )
        result = {
            "schema": "boole.native-shadow.installed-mac-crash-restart-e2e.v1",
            "status": "INSTALLED-MAC-CRASH-RESTART-EXACTLY-ONCE-PASS",
            "sourceRevision": plan["sourceRevision"],
            "install": install,
            "scenarios": [terminal, unresolved],
            "boundary": {
                "production": False,
                "testnet": False,
                "mining": False,
                "reward": False,
                "consensus": False,
                "p2p": False,
                "activationAllowed": False,
            },
        }
        _write_json_atomic(Path(args.result).resolve(), result)
        return result
    finally:
        for name in ("http-root", "download-staging", "install-root"):
            path = work_root / name
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path)
            elif path.exists() or path.is_symlink():
                path.unlink()
        for scenario in ("terminal", "unresolved"):
            runtime = work_root / scenario / "runtime"
            if runtime.is_dir() and not runtime.is_symlink():
                shutil.rmtree(runtime)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kat-plan", required=True)
    parser.add_argument("--kat-binary", required=True)
    parser.add_argument("--cli", required=True)
    parser.add_argument("--work", required=True)
    parser.add_argument("--result", required=True)
    parser.add_argument("--grant", required=True)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--install-timeout-seconds", type=int, default=5)
    return parser.parse_args()


def main() -> int:
    try:
        result = execute(parse_args())
    except Exception as error:
        print(f"installed Mac crash E2E refused: {error}", file=os.sys.stderr)
        return 2
    print("installed Mac crash E2E: " + str(result["status"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
