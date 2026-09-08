#!/usr/bin/env python3
"""Disposable signed-canary MCP trace inside the existing qualified Linux gate.

Only synthetic answers and a newly generated disposable development test key
are used. The parent owns all preexisting replay units/materials; this script
adds separate binary names, explicit drop-ins and isolated per-run state, and
removes only those additions. It does not modify the frozen replay grant.
"""
import argparse
import hashlib
import json
import os
import pwd
import shutil
import signal
import subprocess
import tempfile
from pathlib import Path

try:
    from . import native_shadow_crash_restart_gate as crash
    from . import native_shadow_mcp_real_trace_gate as mcp
except ImportError:
    import native_shadow_crash_restart_gate as crash
    import native_shadow_mcp_real_trace_gate as mcp


STATE = Path("/var/lib/boole/native-shadow")
LIBEXEC = Path("/usr/libexec/boole")
SOCKET = Path("/run/boole/native-shadow/launcher.sock")


def require_budget(path: Path, grant: dict, role: str, executions: int, redeliveries: int) -> None:
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    expected = ["open", "candidate"] + ["execute"] * executions + ["redeliver"] * redeliveries
    if [row.get("event") for row in rows] != expected:
        raise ValueError("canary budget event sequence drifted")
    if rows[0].get("journal_id") != grant["journalId"] or rows[0].get("role") != role:
        raise ValueError("canary private journal binding drifted")


def synthetic_answer(fixture: str, label: str) -> str:
    # Both the envelope and extracted source differ from the frozen digest.
    marker = "    // <<< ACFR-PATCH-BEGIN >>>\n"
    if "```rust" not in fixture or marker not in fixture:
        raise ValueError("synthetic canary fixture lacks the public Rust fence")
    return "Synthetic development canary, not a model answer.\n" + fixture.replace(
        marker, marker + "    // fresh-answer-canary-{}\n".format(label), 1
    )


def payload(grant: dict, raw: str) -> dict:
    bindings = grant["bindings"]
    return {
        "schema": "boole.native-shadow.submission.v1",
        "familyVersion": bindings["familyVersion"],
        "templateId": bindings["templateId"],
        "challengeSha256": bindings["challengeSha256"],
        "epoch": grant["epoch"],
        "rawAnswer": raw,
    }


def run(command: list, *, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run([str(value) for value in command], check=check, capture_output=True, text=True, timeout=300)


def stop() -> None:
    run(["systemctl", "stop", crash.NODE_SERVICE, crash.LAUNCHER_SERVICE], check=False)
    for unit in (crash.NODE_SERVICE, crash.LAUNCHER_SERVICE):
        crash.wait_for_unit_state(unit, ("inactive", "failed"), 30)
        run(["systemctl", "reset-failed", unit], check=False)


def run_gate(args: argparse.Namespace) -> None:
    if os.geteuid() != 0 or os.uname().sysname != "Linux":
        raise RuntimeError("canary integration gate requires the disposable root-owned Linux test environment")
    node = pwd.getpwnam("boole-node")
    authority = args.authority_directory / "development-canary-v1"
    roots = [STATE / "canary-node", STATE / "canary-launcher"]
    launcher = LIBEXEC / "boole-fresh-answer-canary-launcher"
    node_binary = LIBEXEC / "boole-fresh-answer-canary-node"
    dropins = [
        Path("/run/systemd/system") / (crash.LAUNCHER_SERVICE + ".d") / "20-fresh-answer-canary.conf",
        Path("/run/systemd/system") / (crash.NODE_SERVICE + ".d") / "20-fresh-answer-canary.conf",
    ]
    for target in [authority, *roots, launcher, node_binary, *dropins]:
        if target.exists() or target.is_symlink():
            raise RuntimeError("refusing to replace preexisting canary target: {}".format(target))
    stop()
    created_files = []
    created_dirs = []
    legacy = mcp.LegacyNodeTrap()
    try:
        for directory in [authority, *roots]:
            directory.mkdir(mode=0o755)
            directory.chmod(0o755)
            created_dirs.append(directory)
        authority.chmod(0o555)
        for source, destination in [(args.launcher_binary, launcher), (args.node_binary, node_binary)]:
            shutil.copyfile(source, destination)
            created_files.append(destination)
            destination.chmod(0o755)
        dropins[0].write_text("[Service]\nExecStart=\nExecStart={}\nRestart=no\n".format(launcher))
        created_files.append(dropins[0])
        dropins[1].write_text("[Service]\nExecStart=\nExecStart={}\nReadWritePaths={}\n".format(node_binary, roots[0]))
        created_files.append(dropins[1])
        run(["systemctl", "daemon-reload"])
        with tempfile.TemporaryDirectory(prefix="boole-canary-gate-") as scratch_name:
            scratch = Path(scratch_name)
            key = scratch / "disposable-test-seed"
            key.write_bytes(os.urandom(32))
            key.chmod(0o600)

            def provision(label: str, epoch: int) -> tuple:
                stop()
                output = scratch / label
                output.mkdir(mode=0o700)
                run_id = hashlib.sha256((scratch_name + label).encode()).hexdigest()
                journal_id = hashlib.sha256((run_id + ":private-journal").encode()).hexdigest()
                run([args.operator_binary, "create-grant", key, run_id, journal_id, epoch, output])
                for name in ("grant.json", "grant.sig", "operator-public-key.bin"):
                    destination = authority / name
                    shutil.copyfile(output / name, destination)
                    destination.chmod(0o444)
                    if destination not in created_files:
                        created_files.append(destination)
                for name in ("redelivery.json", "redelivery.sig"):
                    (authority / name).unlink(missing_ok=True)
                for root, uid, gid in [(roots[0], node.pw_uid, node.pw_gid), (roots[1], 0, 0)]:
                    directory = root / journal_id
                    directory.mkdir(mode=0o700)
                    os.chown(directory, uid, gid)
                    created_dirs.append(directory)
                grant = json.loads((output / "grant.json").read_text())
                return grant, output, roots[0] / journal_id, roots[1] / journal_id

            # Signed scope is mandatory even when the explicit development
            # binaries were installed. This negative startup never executes.
            grant, output, _, _ = provision("wrong-signature", 9)
            (authority / "grant.sig").write_bytes(bytes(64))
            run(["systemctl", "start", crash.NODE_SERVICE], check=False)
            crash.wait_for_unit_state(crash.NODE_SERVICE, ("failed",), 120)
            crash.require_listener_refused()
            stop()
            print("fresh-answer-canary:unsigned-startup-refused:PASS", flush=True)

            for label, epoch, filename, verdict in [
                ("accepted", 10, "replay-accepted.raw.txt", "accepted"),
                ("rejected", 11, "replay-constant.raw.txt", "rejected"),
            ]:
                grant, output, node_state, launcher_state = provision(label, epoch)
                raw = synthetic_answer((args.fixture_directory / filename).read_text(), label)
                arguments = payload(grant, raw)
                cursor = crash.freeze_journal_cursor()
                run(["systemctl", "start", crash.NODE_SERVICE])
                crash.wait_for_listener()
                client = mcp.McpStdio(args.mcp_binary, legacy.url, user=node.pw_uid, group=node.pw_gid)
                try:
                    wrong = dict(arguments, epoch=epoch + 100)
                    error, body = client.call("wrong-task", wrong)
                    if not error or body.get("reasonCode") != "canary_task_mismatch":
                        raise ValueError("canary wrong-task admission was not refused")
                    error, first = client.call("fresh-" + label, arguments)
                    if error:
                        raise ValueError("canary fresh answer did not reach contained checker: {}".format(first))
                    mcp._validate_terminal("accepted" if verdict == "accepted" else "constant", first, False)
                    error, body = client.call("unapproved-recovery", arguments)
                    if not error or body.get("reasonCode") != "canary_redelivery_not_authorized":
                        raise ValueError("canary redelivery did not require independent operator approval")
                finally:
                    client.close()
                journal = node_state / "verdict-v1.ndjson"
                before = journal.read_bytes()
                rows = [json.loads(line) for line in before.splitlines()]
                if [row["kind"] for row in rows] != list(crash.TERMINAL_ROW_KINDS):
                    raise ValueError("canary did not durably commit exactly one terminal receipt")
                attempt = rows[0]
                if attempt["candidateDigest"] != hashlib.sha256(raw.encode()).hexdigest():
                    raise ValueError("canary candidate identity drifted")
                require_budget(node_state / "node-budget-v1.jsonl", grant, "node", 1, 0)
                require_budget(launcher_state / "launcher-budget-v1.jsonl", grant, "launcher", 1, 0)
                launcher_before = (launcher_state / "launcher-budget-v1.jsonl").read_bytes()
                # Kill the actual node after terminal durability; the next MCP
                # client and node are fresh processes with the same private state.
                crash.deliver_verified_signal(crash.verified_unit_main_pid(crash.NODE_SERVICE), signal.SIGKILL)
                crash.wait_for_unit_state(crash.NODE_SERVICE, ("failed",), 30)
                stop()
                recovery = scratch / (label + "-recovery")
                recovery.mkdir(mode=0o700)
                run([args.operator_binary, "create-redelivery", key, output, attempt["candidateDigest"], attempt["submissionDigest"], recovery])
                for name in ("redelivery.json", "redelivery.sig"):
                    destination = authority / name
                    shutil.copyfile(recovery / name, destination)
                    destination.chmod(0o444)
                    if destination not in created_files:
                        created_files.append(destination)
                run(["systemctl", "start", crash.NODE_SERVICE])
                crash.wait_for_listener()
                client = mcp.McpStdio(args.mcp_binary, legacy.url, user=node.pw_uid, group=node.pw_gid)
                try:
                    error, recovered = client.call("operator-recovery", arguments)
                    if error:
                        raise ValueError("signed canary recovery failed: {}".format(recovered))
                    mcp.validate_terminal_pair(verdict, first, recovered)
                    for request_id, candidate in [
                        ("recovery-exhausted", arguments),
                        ("changed-candidate", dict(arguments, rawAnswer=raw + "\nchanged")),
                    ]:
                        error, body = client.call(request_id, candidate)
                        if not error or body.get("outcome") != "precheck_reject":
                            raise ValueError("canary exhausted or changed candidate was not refused")
                finally:
                    client.close()
                if before != journal.read_bytes() or launcher_before != (launcher_state / "launcher-budget-v1.jsonl").read_bytes():
                    raise ValueError("canary re-executed or rewrote terminal evidence across restart")
                require_budget(node_state / "node-budget-v1.jsonl", grant, "node", 1, 1)
                if len(crash.peer_marker_pids(cursor)) != 1:
                    raise ValueError("canary sent more than one request to the launcher")
                stop()
                print("fresh-answer-canary:{}-mcp-restart-redelivery:PASS".format(label), flush=True)

            grant, _, node_state, launcher_state = provision("inflight-crash", 12)
            raw = synthetic_answer((args.fixture_directory / "replay-accepted.raw.txt").read_text(), "crash")
            cursor = crash.freeze_journal_cursor()
            run(["systemctl", "start", crash.NODE_SERVICE])
            crash.wait_for_listener()
            launcher_identity = crash.verified_unit_main_pid(crash.LAUNCHER_SERVICE)
            node_identity = crash.verified_unit_main_pid(crash.NODE_SERVICE)
            crash.deliver_verified_signal(launcher_identity, signal.SIGSTOP)
            parked = crash.post_without_reading(payload(grant, raw))
            try:
                crash.wait_for_journal_row(node_state / "verdict-v1.ndjson", "in_flight_v3", 12, 60)
                crash.deliver_verified_signal(node_identity, signal.SIGKILL)
                crash.wait_for_unit_state(crash.NODE_SERVICE, ("failed",), 30)
            finally:
                parked.close()
                stop()
            require_budget(node_state / "node-budget-v1.jsonl", grant, "node", 1, 0)
            before = (node_state / "verdict-v1.ndjson").read_bytes()
            run(["systemctl", "start", crash.NODE_SERVICE], check=False)
            crash.wait_for_unit_state(crash.NODE_SERVICE, ("failed",), crash.startup_recovery_wait_seconds(os.uname().machine))
            crash.require_listener_refused()
            if before != (node_state / "verdict-v1.ndjson").read_bytes() or crash.peer_marker_pids(cursor):
                raise ValueError("ambiguous canary execution was retried after restart")
            stop()
            print("fresh-answer-canary:inflight-crash-fail-closed:PASS", flush=True)
            mcp.require_no_legacy_node_contact(legacy.connections)
            print("fresh-answer-canary:real-mcp-contained-checker:PASS", flush=True)
    finally:
        stop()
        legacy.close()
        # Only exact files inside directories this invocation created are
        # removed. Unexpected entries keep the directory and fail cleanup.
        for directory in reversed(created_dirs):
            for name in ("node-budget-v1.jsonl", "launcher-budget-v1.jsonl", "verdict-v1.ndjson"):
                (directory / name).unlink(missing_ok=True)
        for path in reversed(created_files):
            path.unlink(missing_ok=True)
        for path in reversed(created_dirs):
            path.rmdir()
        run(["systemctl", "daemon-reload"])


def main() -> None:
    def interrupted(_signum, _frame):
        raise SystemExit("canary gate interrupted; cleaning up owned resources")

    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("launcher-binary", "node-binary", "operator-binary", "mcp-binary", "authority-directory", "fixture-directory"):
        parser.add_argument("--" + name, type=Path, required=True)
    run_gate(parser.parse_args())


if __name__ == "__main__":
    main()
