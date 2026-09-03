"""Shared session-bound submission helpers for closed-local testnet-2 smokes."""

import json
import os
import pathlib
import subprocess
import time


NETWORK_ID = "boole-testnet-2"
OWNER_ID = "testnet2-smoke-owner-v1"


def _run_json(command, *, env, run):
    completed = run(
        command,
        env=env,
        text=True,
        capture_output=True,
        check=True,
    )
    return json.loads(completed.stdout)


def build_registration_envelope(
    cli_bin,
    workdir,
    node_label,
    fixture,
    *,
    now_secs=None,
    run=subprocess.run,
):
    """Create a fresh owner-signed registration for a pinned fixture session."""
    workdir = pathlib.Path(workdir)
    auth_dir = workdir / f"session-auth-{node_label}"
    keys_dir = auth_dir / "keys"
    auth_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["BOOLE_KEYS_DIR"] = str(keys_dir)

    created = _run_json(
        [cli_bin, "keys", "new", "--id", OWNER_ID, "--dev"],
        env=env,
        run=run,
    )
    expected_owner = fixture["sessionState"]["ownerPk"]
    if created.get("key", {}).get("pk") != expected_owner:
        raise RuntimeError("fixture ownerPk does not match the deterministic smoke owner key")

    now_secs = int(time.time()) if now_secs is None else int(now_secs)
    payload = {
        "schema": "boole.sessions.register.v1",
        "session": fixture["sessionState"],
        "currentHeight": 0,
        "validBefore": now_secs + 300,
        "nonce": f"testnet2-register-{node_label}-{now_secs}",
    }
    payload_path = auth_dir / "register-payload.json"
    payload_path.write_text(json.dumps(payload, separators=(",", ":")))
    signed = _run_json(
        [
            cli_bin,
            "keys",
            "sign",
            "--id",
            OWNER_ID,
            "--payload",
            str(payload_path),
            "--network-id",
            NETWORK_ID,
            "--json",
        ],
        env=env,
        run=run,
    )["result"]["envelope"]
    if signed.get("network_id") != NETWORK_ID:
        raise RuntimeError("registration signature is not bound to boole-testnet-2")
    return signed


def authorized_submit(fixture, ts_ms):
    session = fixture.get("submissionSession")
    if not isinstance(session, dict):
        raise RuntimeError("fixture is missing submissionSession")
    signed_work = session.get("signedWork", {})
    if signed_work.get("network_id") != NETWORK_ID:
        raise RuntimeError("fixture signedWork is not bound to boole-testnet-2")
    return {
        "body": fixture["body"],
        "canonTag": 0,
        "ts": int(ts_ms),
        "session": session,
    }


def assert_session_receipt(response, fixture):
    receipt = response.get("receipt", {})
    expected_session = fixture["submissionSession"]
    expected_hash = expected_session["signedWork"]["payload"]["requestHash"]
    if (
        receipt.get("sessionPk") != expected_session["submittedBy"]
        or receipt.get("requestHash") != expected_hash
    ):
        raise RuntimeError("accepted response did not carry the expected session-bound receipt")
