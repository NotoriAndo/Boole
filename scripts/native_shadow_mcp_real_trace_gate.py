#!/usr/bin/env python3
"""Drive one real MCP -> node -> contained-checker closed-local trace.

This gate is intentionally a client only.  The surrounding named Linux gate
owns the real replay-node systemd unit, the qualified launcher, the frozen
checker/rootfs and the durable journal.  Here we prove that an actual
``boole-mcp stdio`` process can traverse that already-audited path, and that a
fresh MCP process manually replaying the same six fields receives the node's
stored terminal result without extending the journal.
"""

import argparse
import hashlib
import json
import os
import select
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Any, Dict, Iterable, List, Tuple


ROOT = Path(__file__).resolve().parents[1]
GRANT_PATH = ROOT / "native/containment/native-shadow-closed-local-replay-grant-v1.json"
FIXTURE_DIR = ROOT / "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history"
JOURNAL_PATH = Path("/var/lib/boole/native-shadow/node-state/replay-v1.ndjson")
NATIVE_URL = "http://127.0.0.1:8082"
NATIVE_HOST = "127.0.0.1"
NATIVE_PORT = 8082
FRAME_LIMIT_BYTES = 65_536
FRAME_TIMEOUT_SECONDS = 140.0

RAW_ANSWER_FILES = {
    "accepted": "replay-accepted.raw.txt",
    "tampered": "replay-tampered.raw.txt",
    "constant": "replay-constant.raw.txt",
    "empty": None,
}

TERMINAL_ROW_KINDS = (
    "grant_attempt_reserved_v1",
    "bootstrap_v2",
    "in_flight_v3",
    "evidence_v2",
    "terminal_consumed_v2",
)


def _strict_object(pairs: Iterable[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key in real MCP trace")
        result[key] = value
    return result


def _is_lower_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def _validate_terminal(case_id: str, body: Dict[str, Any], redelivered: bool) -> None:
    accepted = case_id == "accepted"
    if case_id not in ("accepted", "tampered", "constant"):
        raise ValueError("real MCP trace has no terminal case: {}".format(case_id))
    expected = {
        "schema",
        "outcome",
        "reasonCode",
        "redelivered",
        "evidenceDigest",
        "receipt",
    }
    if not isinstance(body, dict) or set(body) != expected:
        raise ValueError("real MCP trace terminal response fields drifted")
    receipt = body["receipt"]
    if not isinstance(receipt, dict) or set(receipt) != {
        "taskId",
        "submissionId",
        "artifactRoot",
        "checkerHash",
        "verdict",
        "rejectReason",
    }:
        raise ValueError("real MCP trace receipt fields drifted")
    if (
        body["schema"] != "boole.native-shadow.adjudication.v1"
        or body["outcome"] != ("accepted" if accepted else "deterministic_reject")
        or body["reasonCode"] != ("accepted" if accepted else "checker_rejected")
        or body["redelivered"] is not redelivered
        or not _is_lower_sha256(body["evidenceDigest"])
        or receipt["verdict"] != ("accepted" if accepted else "rejected")
        or receipt["rejectReason"]
        != (None if accepted else "compile-or-hidden-test-failed")
        or not all(
            _is_lower_sha256(receipt[field])
            for field in ("taskId", "submissionId", "artifactRoot", "checkerHash")
        )
    ):
        raise ValueError("real MCP trace terminal verdict drifted")


def validate_terminal_pair(verdict: str, first: Dict[str, Any], replay: Dict[str, Any]) -> None:
    case_id = "accepted" if verdict == "accepted" else "tampered"
    _validate_terminal(case_id, first, False)
    _validate_terminal(case_id, replay, True)
    first_without_delivery = dict(first)
    replay_without_delivery = dict(replay)
    first_without_delivery.pop("redelivered")
    replay_without_delivery.pop("redelivered")
    if first_without_delivery != replay_without_delivery:
        raise ValueError("terminal result drifted across MCP restart")


def validate_journal_snapshot(text: str) -> None:
    lines = text.splitlines()
    if not lines or any(not line for line in lines):
        raise ValueError("real MCP trace journal is empty or malformed")
    events = [json.loads(line, object_pairs_hook=_strict_object) for line in lines]
    expected = [
        (kind, epoch) for epoch in range(3) for kind in TERMINAL_ROW_KINDS
    ] + [("grant_attempt_reserved_v1", 3)]
    observed = [(event.get("kind"), event.get("epoch")) for event in events]
    if observed != expected:
        raise ValueError("real MCP trace journal order or count drifted")
    terminal = {
        0: ("accepted", "accepted"),
        1: ("deterministic_reject", "checker_rejected"),
        2: ("deterministic_reject", "checker_rejected"),
    }
    for event in events:
        kind = event["kind"]
        epoch = event["epoch"]
        if kind == "grant_attempt_reserved_v1":
            wanted = "pre_intake" if epoch == 3 else "checker"
            if event.get("attemptKind") != wanted:
                raise ValueError("real MCP trace attempt kind drifted")
        elif kind == "evidence_v2":
            evidence = json.loads(event.get("evidenceJson", ""), object_pairs_hook=_strict_object)
            verdict, reason = terminal[epoch]
            if evidence.get("verdict") != verdict or evidence.get("reasonCode") != reason:
                raise ValueError("real MCP trace evidence drifted")
        elif kind == "terminal_consumed_v2" and event.get("exhausted") is not True:
            raise ValueError("real MCP trace terminal was not exhausted")


def require_journal_unchanged(before: str, after: str) -> None:
    if before != after:
        raise ValueError("journal changed during MCP manual replay")


def require_no_legacy_node_contact(connections: int) -> None:
    if connections != 0:
        raise ValueError("legacy node received a real MCP native request")


def wait_for_native_listener() -> None:
    deadline = time.monotonic() + 120.0
    while True:
        try:
            with socket.create_connection((NATIVE_HOST, NATIVE_PORT), timeout=1.0):
                return
        except OSError:
            if time.monotonic() >= deadline:
                raise RuntimeError("real MCP trace native listener did not become ready")
            time.sleep(0.05)


class LegacyNodeTrap:
    def __init__(self) -> None:
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._listener.bind(("127.0.0.1", 0))
        self._listener.listen()
        self._listener.settimeout(0.1)
        self._connections = 0
        self._closed = False
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()

    @property
    def url(self) -> str:
        host, port = self._listener.getsockname()
        return "http://{}:{}".format(host, port)

    @property
    def connections(self) -> int:
        return self._connections

    def _serve(self) -> None:
        while not self._closed:
            try:
                connection, _ = self._listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            self._connections += 1
            connection.close()

    def close(self) -> None:
        self._closed = True
        self._listener.close()
        self._thread.join(timeout=1.0)


class McpStdio:
    def __init__(self, binary: Path, node_url: str) -> None:
        self._process = subprocess.Popen(
            [
                str(binary),
                "stdio",
                "--node-url",
                node_url,
                "--native-shadow-url",
                NATIVE_URL,
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        if self._process.stdin is None or self._process.stdout is None:
            raise RuntimeError("real MCP trace did not receive stdio pipes")
        self._stdin = self._process.stdin
        self._stdout = self._process.stdout
        try:
            self._initialize()
        except BaseException:
            self._process.terminate()
            self._process.wait(timeout=5.0)
            self._close_pipes()
            raise

    def _write(self, value: Dict[str, Any]) -> None:
        body = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
        frame = "Content-Length: {}\r\n\r\n".format(len(body)).encode("ascii") + body
        self._stdin.write(frame)
        self._stdin.flush()

    def _read_bytes(self, count: int, deadline: float) -> bytes:
        chunks: List[bytes] = []
        remaining = count
        while remaining:
            wait = deadline - time.monotonic()
            if wait <= 0 or not select.select([self._stdout], [], [], wait)[0]:
                raise TimeoutError("real MCP trace timed out reading a response frame")
            chunk = os.read(self._stdout.fileno(), remaining)
            if not chunk:
                raise EOFError("real MCP trace MCP process closed stdout")
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    def _read(self) -> Dict[str, Any]:
        deadline = time.monotonic() + FRAME_TIMEOUT_SECONDS
        header = bytearray()
        while not header.endswith(b"\r\n\r\n"):
            header.extend(self._read_bytes(1, deadline))
            if len(header) > 4096:
                raise ValueError("real MCP trace response header is oversized")
        content_length = None
        for line in bytes(header[:-4]).decode("ascii").split("\r\n"):
            if line.lower().startswith("content-length:"):
                content_length = int(line.split(":", 1)[1].strip())
        if content_length is None or content_length > FRAME_LIMIT_BYTES:
            raise ValueError("real MCP trace response length is missing or oversized")
        raw = self._read_bytes(content_length, deadline)
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_strict_object)
        if not isinstance(value, dict):
            raise ValueError("real MCP trace response is not one JSON object")
        return value

    def _initialize(self) -> None:
        self._write(
            {
                "jsonrpc": "2.0",
                "id": "initialize",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "native-shadow-real-trace", "version": "1"},
                },
            }
        )
        response = self._read()
        if response.get("id") != "initialize" or response.get("result", {}).get(
            "protocolVersion"
        ) != "2024-11-05":
            raise ValueError("real MCP trace initialization drifted")
        self._write({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call(self, request_id: str, arguments: Dict[str, Any]) -> Tuple[bool, Dict[str, Any]]:
        self._write(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "tools/call",
                "params": {"name": "boole.verify_native", "arguments": arguments},
            }
        )
        response = self._read()
        if response.get("jsonrpc") != "2.0" or response.get("id") != request_id:
            raise ValueError("real MCP trace response correlation drifted")
        result = response.get("result")
        if not isinstance(result, dict) or set(result) != {"content", "isError"}:
            raise ValueError("real MCP trace result envelope drifted")
        if not isinstance(result["isError"], bool):
            raise ValueError("real MCP trace isError is not boolean")
        content = result["content"]
        if (
            not isinstance(content, list)
            or len(content) != 1
            or not isinstance(content[0], dict)
            or content[0].get("type") != "text"
            or not isinstance(content[0].get("text"), str)
        ):
            raise ValueError("real MCP trace content envelope drifted")
        body = json.loads(content[0]["text"], object_pairs_hook=_strict_object)
        if not isinstance(body, dict):
            raise ValueError("real MCP trace native body is not one JSON object")
        return bool(result["isError"]), body

    def close(self) -> None:
        self._stdin.close()
        try:
            status = self._process.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            self._process.terminate()
            status = self._process.wait(timeout=5.0)
        stderr = b""
        if self._process.stderr is not None:
            stderr = self._process.stderr.read()
        self._close_pipes()
        if status != 0:
            raise RuntimeError(
                "real MCP trace MCP process failed: {} {}".format(
                    status, stderr.decode("utf-8", errors="replace")[-2048:]
                )
            )

    def _close_pipes(self) -> None:
        for pipe in (self._process.stdin, self._process.stdout, self._process.stderr):
            if pipe is not None and not pipe.closed:
                pipe.close()


def _load_inputs(
    grant_path: Path, fixture_directory: Path
) -> List[Tuple[str, Dict[str, Any]]]:
    grant = json.loads(
        grant_path.read_text(encoding="utf-8"), object_pairs_hook=_strict_object
    )
    scope = grant.get("scope")
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
    if scope != required_scope:
        raise ValueError("real MCP trace grant scope drifted")
    cases = grant.get("cases")
    if not isinstance(cases, list) or [case.get("caseId") for case in cases] != [
        "accepted",
        "tampered",
        "constant",
        "empty",
    ]:
        raise ValueError("real MCP trace cases drifted")
    task = grant["task"]
    result = []
    for case in cases:
        case_id = case["caseId"]
        filename = RAW_ANSWER_FILES[case_id]
        raw_answer = (
            ""
            if filename is None
            else (fixture_directory / filename).read_text(encoding="utf-8")
        )
        if hashlib.sha256(raw_answer.encode("utf-8")).hexdigest() != case["rawAnswerSha256"]:
            raise ValueError("real MCP trace {} answer drifted".format(case_id))
        result.append(
            (
                case_id,
                {
                    "schema": "boole.native-shadow.submission.v1",
                    "familyVersion": task["familyVersion"],
                    "templateId": task["templateId"],
                    "challengeSha256": task["challengeSha256"],
                    "epoch": case["epoch"],
                    "rawAnswer": raw_answer,
                },
            )
        )
    return result


def run_trace(binary: Path, grant_path: Path, fixture_directory: Path, journal_path: Path) -> None:
    if not binary.is_file() or binary.is_symlink() or not os.access(binary, os.X_OK):
        raise ValueError("real MCP trace binary is not one executable regular file")
    cases = _load_inputs(grant_path, fixture_directory)
    legacy = LegacyNodeTrap()
    first: Dict[str, Dict[str, Any]] = {}
    try:
        wait_for_native_listener()
        mcp = McpStdio(binary, legacy.url)
        try:
            for case_id, arguments in cases:
                is_error, body = mcp.call("first-{}".format(case_id), arguments)
                if case_id == "empty":
                    expected = {
                        "schema": "boole.native-shadow.adjudication-error.v1",
                        "outcome": "precheck_reject",
                        "reasonCode": "intake_rejected",
                    }
                    if not is_error or body != expected:
                        raise ValueError("real MCP trace empty-answer precheck drifted")
                else:
                    if is_error:
                        raise ValueError("real MCP trace terminal verdict became an MCP error")
                    _validate_terminal(case_id, body, False)
                    first[case_id] = body
                    print("native-shadow-real-mcp-case:{}:PASS".format(case_id), flush=True)
        finally:
            mcp.close()

        before = journal_path.read_text(encoding="utf-8")
        validate_journal_snapshot(before)

        restarted = McpStdio(binary, legacy.url)
        try:
            for case_id, arguments in cases[:2]:
                is_error, body = restarted.call("replay-{}".format(case_id), arguments)
                if is_error:
                    raise ValueError("real MCP trace replay became an MCP error")
                validate_terminal_pair(
                    "accepted" if case_id == "accepted" else "rejected",
                    first[case_id],
                    body,
                )
                print("native-shadow-real-mcp-redelivery:{}:PASS".format(case_id), flush=True)
        finally:
            restarted.close()

        after = journal_path.read_text(encoding="utf-8")
        require_journal_unchanged(before, after)
        require_no_legacy_node_contact(legacy.connections)
        print("native-shadow-real-mcp-journal-unchanged:PASS", flush=True)
        print("native-shadow-real-mcp-legacy-node-connections:0", flush=True)
        print("native-shadow-real-mcp-trace:PASS", flush=True)
    finally:
        legacy.close()


def parse_args(argv: Any = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="drive the real native MCP closed-local trace")
    parser.add_argument("--mcp-binary", type=Path, required=True)
    parser.add_argument("--grant-path", type=Path, default=GRANT_PATH)
    parser.add_argument("--fixture-directory", type=Path, default=FIXTURE_DIR)
    parser.add_argument("--journal-path", type=Path, default=JOURNAL_PATH)
    return parser.parse_args(argv)


def main(argv: Any = None) -> int:
    args = parse_args(argv)
    run_trace(args.mcp_binary, args.grant_path, args.fixture_directory, args.journal_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
