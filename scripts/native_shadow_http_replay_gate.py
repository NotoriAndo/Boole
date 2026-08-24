#!/usr/bin/env python3
"""Drive the exact four-case closed-local native-shadow HTTP replay matrix."""

import argparse
import hashlib
import http.client
import json
import socket
import time
from pathlib import Path
from typing import Any, Dict, Iterable, Tuple


ROOT = Path(__file__).resolve().parents[1]
GRANT_PATH = ROOT / "native/containment/native-shadow-closed-local-replay-grant-v1.json"
FIXTURE_DIR = ROOT / "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history"
HOST = "127.0.0.1"
PORT = 8082
ROUTE = "/native-shadow/submissions"
JOURNAL_PATH = Path("/var/lib/boole/native-shadow/node-state/replay-v1.ndjson")
STARTUP_WAIT_SECONDS = 120.0
RESPONSE_LIMIT_BYTES = 65_536

RAW_ANSWER_FILES = {
    "accepted": "replay-accepted.raw.txt",
    "tampered": "replay-tampered.raw.txt",
    "constant": "replay-constant.raw.txt",
    "empty": None,
}


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
            raise ValueError("duplicate JSON key in HTTP replay response: {}".format(key))
        result[key] = value
    return result


def _require_exact_keys(value: Any, keys: Iterable[str], label: str) -> Dict[str, Any]:
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ValueError("HTTP replay matrix {} has unexpected fields".format(label))
    return value


def validate_case_response(case_id: str, status: int, body: Dict[str, Any]) -> None:
    if case_id == "empty":
        expected = {
            "schema": "boole.native-shadow.adjudication-error.v1",
            "outcome": "precheck_reject",
            "reasonCode": "intake_rejected",
        }
        if status != 400 or body != expected:
            raise ValueError("HTTP replay matrix empty case was not the exact intake rejection")
        return

    if case_id == "accepted":
        expected_outcome = "accepted"
        expected_reason = "accepted"
        expected_verdict = "accepted"
        expected_reject_reason = None
    elif case_id in ("tampered", "constant"):
        expected_outcome = "deterministic_reject"
        expected_reason = "compile_or_hidden_test_failed"
        expected_verdict = "rejected"
        expected_reject_reason = "compile-or-hidden-test-failed"
    else:
        raise ValueError("HTTP replay matrix contains an unknown case")

    expected_top_keys = {
        "schema",
        "outcome",
        "reasonCode",
        "redelivered",
        "evidenceDigest",
        "receipt",
    }
    _require_exact_keys(body, expected_top_keys, case_id)
    receipt = _require_exact_keys(
        body["receipt"],
        {
            "taskId",
            "submissionId",
            "artifactRoot",
            "checkerHash",
            "verdict",
            "rejectReason",
        },
        "{} receipt".format(case_id),
    )
    if (
        status != 200
        or body["schema"] != "boole.native-shadow.adjudication.v1"
        or body["outcome"] != expected_outcome
        or body["reasonCode"] != expected_reason
        or body["redelivered"] is not False
        or not _is_lower_sha256(body["evidenceDigest"])
        or receipt["verdict"] != expected_verdict
        or receipt["rejectReason"] != expected_reject_reason
        or not all(
            _is_lower_sha256(receipt[field])
            for field in ("taskId", "submissionId", "artifactRoot", "checkerHash")
        )
    ):
        raise ValueError("HTTP replay matrix {} response drifted".format(case_id))


def validate_journal_events(events: list) -> None:
    expected = []
    terminal_verdicts = {
        0: ("accepted", "accepted"),
        1: ("deterministic_reject", "compile_or_hidden_test_failed"),
        2: ("deterministic_reject", "compile_or_hidden_test_failed"),
    }
    for epoch in range(3):
        expected.extend(
            [
                ("grant_attempt_reserved_v1", epoch),
                ("bootstrap_v2", epoch),
                ("in_flight_v3", epoch),
                ("evidence_v2", epoch),
                ("terminal_consumed_v2", epoch),
            ]
        )
    expected.append(("grant_attempt_reserved_v1", 3))
    observed = [
        (event.get("kind"), event.get("epoch"))
        for event in events
        if isinstance(event, dict)
    ]
    if len(events) != len(expected) or observed != expected:
        raise ValueError("HTTP replay journal event order or count drifted")
    for event in events:
        kind = event["kind"]
        epoch = event["epoch"]
        if kind == "grant_attempt_reserved_v1":
            expected_kind = "pre_intake" if epoch == 3 else "checker"
            if event.get("attemptKind") != expected_kind:
                raise ValueError("HTTP replay journal attempt kind drifted")
        elif kind == "evidence_v2":
            evidence = json.loads(event.get("evidenceJson", ""), object_pairs_hook=_strict_object)
            verdict, reason = terminal_verdicts[epoch]
            if evidence.get("verdict") != verdict or evidence.get("reasonCode") != reason:
                raise ValueError("HTTP replay journal evidence verdict drifted")
        elif kind == "terminal_consumed_v2" and event.get("exhausted") is not True:
            raise ValueError("HTTP replay journal terminal row is not exhausted")


def validate_journal_file(path: Path = JOURNAL_PATH) -> None:
    raw_lines = path.read_text(encoding="utf-8").splitlines()
    if not raw_lines or any(not line for line in raw_lines):
        raise ValueError("HTTP replay journal is empty or contains an empty line")
    events = [json.loads(line, object_pairs_hook=_strict_object) for line in raw_lines]
    validate_journal_events(events)


def _wait_for_listener() -> None:
    deadline = time.monotonic() + STARTUP_WAIT_SECONDS
    while True:
        try:
            with socket.create_connection((HOST, PORT), timeout=1.0):
                return
        except (ConnectionRefusedError, TimeoutError, OSError):
            if time.monotonic() >= deadline:
                raise RuntimeError("closed-local replay HTTP listener did not become ready")
            time.sleep(0.05)


def _post(payload: Dict[str, Any]) -> Tuple[int, Dict[str, Any]]:
    encoded = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    connection = http.client.HTTPConnection(HOST, PORT, timeout=120.0)
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
            raise ValueError("HTTP replay matrix response exceeded the fixed cap")
        body = json.loads(raw.decode("utf-8"), object_pairs_hook=_strict_object)
        if not isinstance(body, dict):
            raise ValueError("HTTP replay matrix response is not one JSON object")
        return response.status, body
    finally:
        connection.close()


def _load_matrix(grant_path: Path = GRANT_PATH) -> Tuple[Dict[str, Any], list]:
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
        raise ValueError("closed-local replay grant scope drifted")
    cases = grant.get("cases")
    if not isinstance(cases, list) or [case.get("caseId") for case in cases] != [
        "accepted",
        "tampered",
        "constant",
        "empty",
    ]:
        raise ValueError("closed-local replay grant case order drifted")
    return grant, cases


def parse_args(argv: Any = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="drive the closed-local native-shadow HTTP replay matrix"
    )
    parser.add_argument("--grant-path", type=Path, default=GRANT_PATH)
    parser.add_argument("--fixture-directory", type=Path, default=FIXTURE_DIR)
    parser.add_argument("--journal-path", type=Path, default=JOURNAL_PATH)
    return parser.parse_args(argv)


def main(
    grant_path: Path = GRANT_PATH,
    fixture_directory: Path = FIXTURE_DIR,
    journal_path: Path = JOURNAL_PATH,
) -> int:
    grant, cases = _load_matrix(grant_path)
    task = grant["task"]
    _wait_for_listener()
    for case in cases:
        case_id = case["caseId"]
        filename = RAW_ANSWER_FILES[case_id]
        raw_answer = (
            ""
            if filename is None
            else (fixture_directory / filename).read_text(encoding="utf-8")
        )
        if hashlib.sha256(raw_answer.encode("utf-8")).hexdigest() != case["rawAnswerSha256"]:
            raise ValueError("{} raw answer differs from the replay grant".format(case_id))
        payload = {
            "schema": "boole.native-shadow.submission.v1",
            "familyVersion": task["familyVersion"],
            "templateId": task["templateId"],
            "challengeSha256": task["challengeSha256"],
            "epoch": case["epoch"],
            "rawAnswer": raw_answer,
        }
        status, body = _post(payload)
        validate_case_response(case_id, status, body)
        print("native-shadow-http-replay-case:{}:PASS".format(case_id), flush=True)
    validate_journal_file(journal_path)
    print("native-shadow-http-replay-journal:PASS", flush=True)
    print("native-shadow-http-replay-matrix:PASS", flush=True)
    return 0


if __name__ == "__main__":
    args = parse_args()
    raise SystemExit(
        main(args.grant_path, args.fixture_directory, args.journal_path)
    )
