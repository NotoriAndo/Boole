#!/usr/bin/env python3
"""Closed-local installed Mac native-shadow end-to-end harness.

This developer-only module composes the already signed install, loopback node
and frozen four-case replay surfaces.  It never grants production, mining,
reward, consensus, P2P or activation authority.
"""

from __future__ import annotations

import argparse
import hashlib
import http.server
import http.client
import json
import os
import shutil
import signal
import socket
import subprocess
import threading
import time
import sys
from contextlib import contextmanager
from functools import partial
from pathlib import Path
from typing import Any, Callable


CASE_FILES = {
    "accepted": "replay-accepted.raw.txt",
    "tampered": "replay-tampered.raw.txt",
    "constant": "replay-constant.raw.txt",
    "empty": "replay-empty.raw.txt",
}

EXPECTED_CASE_RESULTS = {
    "accepted": (200, "accepted", "accepted"),
    "tampered": (200, "deterministic_reject", "checker_rejected"),
    "constant": (200, "deterministic_reject", "checker_rejected"),
    "empty": (400, "precheck_reject", "intake_rejected"),
}

PRODUCT_ARTIFACT_ROLES = {
    "host-cli",
    "host-node",
    "host-wallet-agent",
    "host-controller",
}

GUEST_ARTIFACT_ROLES = {
    "guest-kernel",
    "guest-root-disk",
    "rootfs-content-manifest",
    "registry",
    "execution-policy",
    "toolchain-identity",
    "checker-release-manifest",
    "registry-overlay",
    "closed-local-replay-grant",
    "local-execution-authority",
    "closed-local-replay-execution-authority",
}

CONTROLLER_RUNTIME_LEASE_BASENAME = ".controller-runtime.lock"


def _strict_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("installed Mac E2E JSON contains a duplicate key")
        result[key] = value
    return result


def _read_object(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=_strict_object)
    if not isinstance(value, dict):
        raise ValueError(f"{path} is not one JSON object")
    return value


def _raw_answer(fixture_dir: Path, case: dict[str, Any]) -> str:
    case_id = case.get("caseId")
    filename = CASE_FILES.get(case_id)
    if filename is None:
        raise ValueError(f"installed Mac E2E has no fixture for case {case_id!r}")
    raw_answer = (fixture_dir / filename).read_text(encoding="utf-8")
    if hashlib.sha256(raw_answer.encode("utf-8")).hexdigest() != case.get(
        "rawAnswerSha256"
    ):
        raise ValueError(f"installed Mac E2E fixture differs for {case_id}")
    return raw_answer


def _copy_regular_new(source: Path, destination: Path) -> None:
    metadata = source.lstat()
    if source.is_symlink() or not source.is_file():
        raise ValueError(f"installed Mac E2E source is not a regular file: {source}")
    with source.open("rb") as reader, destination.open("xb") as writer:
        shutil.copyfileobj(reader, writer, 4 * 1024 * 1024)
        writer.flush()
        os.fsync(writer.fileno())
    if destination.stat().st_size != metadata.st_size:
        raise ValueError(f"installed Mac E2E source changed while copying: {source}")


def materialize_transport_layout(plan_path: Path, output_dir: Path) -> dict[str, int]:
    """Expose exactly the signed host root and fixed guest transport prefix."""

    plan = _read_object(plan_path)
    product = plan.get("productArtifacts")
    guest = plan.get("guestArtifacts")
    if not isinstance(product, dict) or set(product) != PRODUCT_ARTIFACT_ROLES:
        raise ValueError("installed Mac E2E needs the exact four host artifacts")
    if not isinstance(guest, dict) or set(guest) != GUEST_ARTIFACT_ROLES:
        raise ValueError("installed Mac E2E needs the exact eleven direct-boot guest artifacts")
    declared_output = plan.get("outputDir")
    if not isinstance(declared_output, str) or Path(declared_output).resolve() != output_dir.resolve():
        raise ValueError("installed Mac E2E plan outputDir differs from the metadata directory")
    if output_dir.is_symlink() or not output_dir.is_dir():
        raise ValueError("installed Mac E2E metadata directory is not a real directory")

    copied: list[Path] = []
    guest_dir = output_dir / "guest"
    try:
        guest_dir.mkdir()
        for role in sorted(PRODUCT_ARTIFACT_ROLES):
            destination = output_dir / role
            _copy_regular_new(Path(product[role]), destination)
            copied.append(destination)
        for role in sorted(GUEST_ARTIFACT_ROLES):
            destination = guest_dir / role
            _copy_regular_new(Path(guest[role]), destination)
            copied.append(destination)
        os.sync()
    except Exception:
        for path in reversed(copied):
            path.unlink(missing_ok=True)
        try:
            guest_dir.rmdir()
        except OSError:
            pass
        raise
    return {
        "hostArtifacts": len(PRODUCT_ARTIFACT_ROLES),
        "guestArtifacts": len(GUEST_ARTIFACT_ROLES),
    }


class _QuietBundleHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, _format: str, *_args: object) -> None:
        pass


@contextmanager
def _loopback_bundle_server(bundle_dir: Path):
    handler = partial(_QuietBundleHandler, directory=str(bundle_dir))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        port = server.server_address[1]
        yield f"http://127.0.0.1:{port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def install_direct_boot_bundle(
    cli: Path,
    bundle_dir: Path,
    install_root: Path,
    staging: Path,
    trust_roots: dict[str, Any],
    *,
    timeout_seconds: int,
) -> dict[str, Any]:
    """Run the real product installer against a transient loopback origin."""

    expected_root_fields = {
        "productKeyId",
        "productPublicKeyHex",
        "guestKeyId",
        "guestPublicKeyHex",
    }
    if set(trust_roots) != expected_root_fields:
        raise ValueError("installed Mac E2E trust roots have unexpected fields")
    if not cli.is_file() or cli.is_symlink():
        raise ValueError("installed Mac E2E CLI is not a regular file")
    if install_root.exists() or staging.exists():
        raise ValueError("installed Mac E2E install and staging paths must start absent")

    with _loopback_bundle_server(bundle_dir) as base_url:
        command = [
            str(cli),
            "product",
            "install-direct-boot",
            "--base-url",
            base_url,
            "--install-root",
            str(install_root),
            "--download-staging",
            str(staging),
            "--product-trust-root-key-id",
            str(trust_roots["productKeyId"]),
            "--product-trust-root-public-key",
            str(trust_roots["productPublicKeyHex"]),
            "--guest-trust-root-key-id",
            str(trust_roots["guestKeyId"]),
            "--guest-trust-root-public-key",
            str(trust_roots["guestPublicKeyHex"]),
            "--first-product-minimum",
            "1",
            "--first-guest-minimum",
            "1",
            "--timeout-seconds",
            str(timeout_seconds),
        ]
        completed = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
            timeout=max(30, timeout_seconds * 32),
        )
    if completed.returncode != 0:
        raise ValueError(
            "installed Mac E2E product install failed: " + completed.stderr.strip()
        )
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise ValueError("installed Mac E2E product install emitted unexpected stdout")
    envelope = json.loads(lines[0], object_pairs_hook=_strict_object)
    if (
        not isinstance(envelope, dict)
        or envelope.get("ok") is not True
        or envelope.get("version") != "v1"
        or envelope.get("command") != "product.install-direct-boot"
        or not isinstance(envelope.get("result"), dict)
    ):
        raise ValueError("installed Mac E2E product install envelope drifted")
    if staging.exists():
        raise ValueError("installed Mac E2E product installer left staging behind")
    return envelope


def _wait_for_node(process: subprocess.Popen[bytes], timeout_seconds: int) -> None:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise ValueError("installed Mac E2E node exited before opening its route")
        try:
            with socket.create_connection(("127.0.0.1", 8082), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise ValueError("installed Mac E2E node did not open its route before the deadline")


def _require_product_health(cli: Path) -> dict[str, Any]:
    completed = subprocess.run(
        [str(cli), "product", "status-direct-boot", "--timeout-seconds", "2"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
        timeout=5,
    )
    if completed.returncode != 0:
        raise ValueError(
            "installed Mac E2E product health failed: " + completed.stderr.strip()
        )
    try:
        envelope = json.loads(completed.stdout, object_pairs_hook=_strict_object)
    except json.JSONDecodeError as error:
        raise ValueError("installed Mac E2E product health is not JSON") from error
    if (
        not isinstance(envelope, dict)
        or envelope.get("ok") is not True
        or envelope.get("version") != "v1"
        or envelope.get("command") != "product.status-direct-boot"
        or not isinstance(envelope.get("result"), dict)
        or envelope.get("result", {}).get("live", {}).get("live") is not True
        or envelope.get("result", {}).get("ready", {}).get("ready") is not True
    ):
        raise ValueError("installed Mac E2E product health envelope drifted")
    return envelope


def _post_submission(payload: dict[str, object]) -> tuple[int, bytes]:
    encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    connection = http.client.HTTPConnection("127.0.0.1", 8082, timeout=120)
    try:
        connection.request(
            "POST",
            "/native-shadow/submissions",
            body=encoded,
            headers={"Content-Type": "application/json", "Content-Length": str(len(encoded))},
        )
        response = connection.getresponse()
        body = response.read(65_537)
        if len(body) > 65_536:
            raise ValueError("installed Mac E2E node response exceeds its cap")
        return response.status, body
    finally:
        connection.close()


def _prepare_private_directory(path: Path) -> None:
    path.mkdir(parents=True, mode=0o700)
    os.chown(path, os.geteuid(), os.getegid())
    path.chmod(0o700)
    metadata = path.stat()
    if (
        path.is_symlink()
        or not path.is_dir()
        or metadata.st_uid != os.geteuid()
        or metadata.st_gid != os.getegid()
        or metadata.st_mode & 0o777 != 0o700
    ):
        raise ValueError(f"installed Mac E2E directory is not private 0700: {path}")


def require_controller_runtime_clean(runtime_root: Path) -> None:
    """Permit only the durable recovery lease after the VM owner exits."""

    entries = list(runtime_root.iterdir())
    if not entries:
        return
    lease = runtime_root / CONTROLLER_RUNTIME_LEASE_BASENAME
    if entries != [lease] or lease.is_symlink() or not lease.is_file():
        raise ValueError("installed Mac E2E runtime root retained controller residue")
    metadata = lease.stat()
    if metadata.st_uid != os.geteuid() or metadata.st_gid != os.getegid():
        raise ValueError("installed Mac E2E runtime lease owner drifted")
    if metadata.st_mode & 0o7777 != 0o600:
        raise ValueError("installed Mac E2E runtime lease mode drifted")


def run_installed_node_matrix(
    cli: Path,
    install_root: Path,
    state_root: Path,
    work_root: Path,
    trust_roots: dict[str, Any],
    grant_path: Path,
    fixture_dir: Path,
    *,
    startup_timeout_seconds: int,
) -> tuple[list[dict[str, object]], dict[str, Any]]:
    """Run the installed product command, health probe, matrix and clean stop."""

    if state_root.exists() or work_root.exists():
        raise ValueError("installed Mac E2E mutable roots must start absent")
    _prepare_private_directory(work_root)
    stdout_path = work_root / "node.stdout"
    stderr_path = work_root / "node.stderr"
    command = [
        str(cli),
        "product",
        "run-direct-boot",
        "--install-root",
        str(install_root),
        "--state-root",
        str(state_root),
        "--product-trust-root-key-id",
        str(trust_roots["productKeyId"]),
        "--product-trust-root-public-key",
        str(trust_roots["productPublicKeyHex"]),
        "--guest-trust-root-key-id",
        str(trust_roots["guestKeyId"]),
        "--guest-trust-root-public-key",
        str(trust_roots["guestPublicKeyHex"]),
    ]
    process: subprocess.Popen[bytes] | None = None
    matrix: list[dict[str, object]] | None = None
    health: dict[str, Any] | None = None
    with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
        try:
            process = subprocess.Popen(
                command,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
            )
            _wait_for_node(process, startup_timeout_seconds)
            health = _require_product_health(cli)
            matrix = run_case_matrix(grant_path, fixture_dir, _post_submission)
            process.send_signal(signal.SIGTERM)
            try:
                return_code = process.wait(timeout=30)
            except subprocess.TimeoutExpired as error:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
                raise ValueError("installed Mac E2E node did not stop cleanly") from error
            if return_code != 0:
                raise ValueError(
                    f"installed Mac E2E node did not exit cleanly: status {return_code}"
                )
        finally:
            if process is not None and process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
    if matrix is None or health is None:
        raise ValueError("installed Mac E2E matrix did not run")
    require_controller_runtime_clean(state_root / "controller")
    materialized_node = state_root / "host" / "boole-mac-native-shadow-replay-node"
    metadata = materialized_node.lstat()
    if (
        materialized_node.is_symlink()
        or not materialized_node.is_file()
        or metadata.st_uid != os.geteuid()
        or metadata.st_gid != os.getegid()
        or metadata.st_nlink != 1
        or metadata.st_mode & 0o7777 != 0o500
    ):
        raise ValueError("installed Mac E2E materialized host-node metadata drifted")
    return matrix, health


def _write_json_atomic(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def _remove_transient_children(work_root: Path) -> None:
    for name in ("http-root", "install-root", "download-staging", "runtime", "state"):
        path = work_root / name
        if path.is_symlink():
            path.unlink()
        elif path.is_dir():
            shutil.rmtree(path)
        elif path.exists():
            path.unlink()


def execute(args: argparse.Namespace) -> dict[str, Any]:
    work_root = Path(args.work).resolve()
    if work_root.exists():
        raise ValueError("installed Mac E2E work root must start absent")
    _prepare_private_directory(work_root)
    plan_path = Path(args.kat_plan).resolve()
    plan = _read_object(plan_path)
    expected_output = work_root / "http-root"
    if Path(str(plan.get("outputDir", ""))).resolve() != expected_output:
        raise ValueError("installed Mac E2E KAT plan must target WORK/http-root")
    product = plan.get("productArtifacts")
    cli = Path(args.cli).resolve()
    if not isinstance(product, dict) or Path(str(product.get("host-cli", ""))).resolve() != cli:
        raise ValueError("installed Mac E2E CLI differs from the signed host-cli input")

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
            raise ValueError("installed Mac E2E KAT metadata failed: " + kat.stderr.strip())
        kat_lines = [line for line in kat.stdout.splitlines() if line.strip()]
        if len(kat_lines) != 1:
            raise ValueError("installed Mac E2E KAT metadata emitted unexpected stdout")
        emitted_roots = json.loads(kat_lines[0], object_pairs_hook=_strict_object)
        roots = _read_object(expected_output / "TRUST-ROOTS.json")
        if emitted_roots != roots:
            raise ValueError("installed Mac E2E KAT public roots differ across outputs")
        layout = materialize_transport_layout(plan_path, expected_output)
        install = install_direct_boot_bundle(
            cli,
            expected_output,
            work_root / "install-root",
            work_root / "download-staging",
            roots,
            timeout_seconds=args.install_timeout_seconds,
        )
        cases, health = run_installed_node_matrix(
            cli,
            work_root / "install-root",
            work_root / "state",
            work_root / "node-logs",
            roots,
            Path(args.grant).resolve(),
            Path(args.fixtures).resolve(),
            startup_timeout_seconds=args.startup_timeout_seconds,
        )
        result = {
            "schema": "boole.native-shadow.installed-mac-e2e.v1",
            "status": "INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS",
            "sourceRevision": plan["sourceRevision"],
            "transportLayout": layout,
            "install": {
                "command": install["command"],
                "releaseSequence": install.get("result", {}).get("releaseSequence"),
                "guestReleaseSequence": install.get("result", {}).get("guestReleaseSequence"),
            },
            "health": health["result"],
            "cases": cases,
            "loopbackOnly": True,
            "production": False,
            "testnet": False,
            "mining": False,
            "reward": False,
            "consensus": False,
            "p2p": False,
            "activationAllowed": False,
        }
        _write_json_atomic(Path(args.result).resolve(), result)
        return result
    finally:
        _remove_transient_children(work_root)


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(
        description="Run the signed installed Mac bundle through closed-local adjudication"
    )
    value.add_argument("--kat-plan", required=True)
    value.add_argument("--kat-binary", required=True)
    value.add_argument("--cli", required=True)
    value.add_argument("--work", required=True)
    value.add_argument("--result", required=True)
    value.add_argument("--grant", required=True)
    value.add_argument("--fixtures", required=True)
    value.add_argument("--install-timeout-seconds", type=int, default=30)
    value.add_argument("--startup-timeout-seconds", type=int, default=180)
    return value


def main(argv: list[str] | None = None) -> int:
    try:
        result = execute(parser().parse_args(argv))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"installed Mac E2E refused: {error}", file=sys.stderr)
        return 2
    print(f"installed Mac E2E: {result['status']}")
    return 0


def run_case_matrix(
    grant_path: Path,
    fixture_dir: Path,
    post: Callable[[dict[str, object]], tuple[int, bytes]],
) -> list[dict[str, object]]:
    """Submit the frozen four cases and refuse any verdict drift."""

    grant = _read_object(grant_path)
    task = grant.get("task")
    cases = grant.get("cases")
    if not isinstance(task, dict) or not isinstance(cases, list):
        raise ValueError("installed Mac E2E grant lacks task or cases")
    ordered = {case.get("caseId"): case for case in cases if isinstance(case, dict)}
    if set(ordered) != set(CASE_FILES) or len(cases) != len(CASE_FILES):
        raise ValueError("installed Mac E2E grant must contain the exact four cases")

    results: list[dict[str, object]] = []
    for case_id in CASE_FILES:
        case = ordered[case_id]
        payload: dict[str, object] = {
            "schema": "boole.native-shadow.submission.v1",
            "familyVersion": task["familyVersion"],
            "templateId": task["templateId"],
            "challengeSha256": task["challengeSha256"],
            "epoch": case["epoch"],
            "rawAnswer": _raw_answer(fixture_dir, case),
        }
        status, raw_body = post(payload)
        body = json.loads(raw_body, object_pairs_hook=_strict_object)
        if not isinstance(body, dict):
            raise ValueError(f"installed Mac E2E {case_id} response is not an object")
        expected_status, expected_outcome, expected_reason = EXPECTED_CASE_RESULTS[case_id]
        passed = (
            status == expected_status
            and body.get("outcome") == expected_outcome
            and body.get("reasonCode") == expected_reason
        )
        if not passed:
            raise ValueError(
                "installed Mac E2E {} verdict drifted: status={} body={}".format(
                    case_id,
                    status,
                    json.dumps(body, sort_keys=True, separators=(",", ":")),
                )
            )
        results.append(
            {
                "caseId": case_id,
                "status": status,
                "outcome": expected_outcome,
                "reasonCode": expected_reason,
                "passed": True,
            }
        )
    return results


if __name__ == "__main__":
    raise SystemExit(main())
