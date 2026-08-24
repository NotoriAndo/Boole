#!/usr/bin/env python3
"""Answer-free checker for the tracked tuple-struct qualification release.

The task contract supplies the anchor, scaffold, constants and challenge.  This
program derives hidden inputs from that contract, builds a throwaway Rust crate,
and judges only the submitted source.  It deliberately has no solution builder.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import resource
import selectors
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from typing import Any


HERE = pathlib.Path(__file__).resolve().parent
POLICY_PATH = HERE / "policy.json"
RESULT_SCHEMA = "boole.native-shadow.checker-result.v1"
TASK_SCHEMA = "boole.native-shadow.rust-tuple-task.v1"
POLICY_SCHEMA = "boole.native-shadow.rust-checker-policy.v1"
FAMILY = "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1"
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")
ALLOWED_EDITIONS = {"2015", "2018", "2021", "2024"}
INTEGER_TYPES = {
    "i8", "i16", "i32", "i64", "isize",
    "u8", "u16", "u32", "u64", "usize",
}


class AuthorityUnavailable(Exception):
    """The authority or its contained execution environment is unavailable."""

    def __init__(self, reason_code: str):
        super().__init__(reason_code)
        self.reason_code = reason_code


class SubmissionRejected(Exception):
    """The submitted program deterministically violates the task contract."""

    def __init__(self, reason_code: str):
        super().__init__(reason_code)
        self.reason_code = reason_code


@dataclass(frozen=True)
class Contract:
    raw_digest: str
    task_id: str
    edition: str
    anchor_text: str
    type_name: str
    field_types: tuple[str, ...]
    task_seed: str
    challenge: str
    a0: int
    mul: int
    coeffs: tuple[int, ...]
    scaffold: str


def _domain_hash(label: str, *parts: str | bytes) -> str:
    digest = hashlib.sha256()
    digest.update(label.encode("utf-8"))
    for part in parts:
        digest.update(b"\0")
        digest.update(part if isinstance(part, bytes) else part.encode("utf-8"))
    return digest.hexdigest()


def _json_result(verdict: str, reason_code: str, contract: Contract | None = None) -> None:
    result: dict[str, Any] = {
        "schema": RESULT_SCHEMA,
        "verdict": verdict,
        "reasonCode": reason_code,
    }
    if contract is not None:
        result["checkerTaskId"] = contract.task_id
        result["taskDigest"] = contract.raw_digest
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


def _read_json(path: pathlib.Path, unavailable_code: str) -> tuple[dict[str, Any], bytes]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise AuthorityUnavailable(unavailable_code) from exc
    if not isinstance(value, dict):
        raise AuthorityUnavailable(unavailable_code)
    return value, raw


def _load_policy() -> dict[str, Any]:
    policy, _ = _read_json(POLICY_PATH, "policy_unavailable")
    try:
        valid = (
            policy["schema"] == POLICY_SCHEMA
            and policy["activationAllowed"] is False
            and policy["verdicts"]
            == ["accepted", "deterministic_reject", "retryable_unavailable"]
            and policy["cargo"]["testArgs"]
            == ["test", "--locked", "--offline", "--quiet", "-j", "1"]
        )
    except (KeyError, TypeError):
        valid = False
    if not valid:
        raise AuthorityUnavailable("policy_contract_mismatch")
    return policy


def _probe(executable: pathlib.Path, args: list[str], system_path: str) -> str:
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise AuthorityUnavailable("toolchain_unavailable")
    env = {"PATH": str(executable.parent) + ":" + system_path, "LC_ALL": "C"}
    try:
        proc = subprocess.run(
            [str(executable), *args],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
            env=env,
            start_new_session=True,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise AuthorityUnavailable("toolchain_probe_failed") from exc
    if proc.returncode != 0:
        raise AuthorityUnavailable("toolchain_probe_failed")
    return proc.stdout


def _version_fields(output: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in output.splitlines():
        if ": " in line:
            key, value = line.split(": ", 1)
            fields[key.strip()] = value.strip()
    return fields


def _verify_toolchain(policy: dict[str, Any], toolchain_bin: pathlib.Path) -> None:
    toolchain = policy["toolchain"]
    system_path = policy["cargo"]["systemPath"]
    rustc = _version_fields(_probe(toolchain_bin / "rustc", ["-vV"], system_path))
    cargo = _version_fields(_probe(toolchain_bin / "cargo", ["-Vv"], system_path))
    if (
        rustc.get("release") != toolchain["rustcRelease"]
        or rustc.get("commit-hash") != toolchain["rustcCommitHash"]
        or cargo.get("release") != toolchain["cargoRelease"]
        or cargo.get("commit-hash") != toolchain["cargoCommitHash"]
    ):
        raise AuthorityUnavailable("toolchain_identity_mismatch")


def _toolchain_directory(path: pathlib.Path) -> pathlib.Path:
    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise AuthorityUnavailable("toolchain_unavailable") from exc
    if not resolved.is_dir():
        raise AuthorityUnavailable("toolchain_unavailable")
    return resolved


def _bounded_int(value: Any) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise AuthorityUnavailable("task_contract_invalid")
    if value < -(1 << 63) or value >= (1 << 63):
        raise AuthorityUnavailable("task_contract_invalid")
    return value


def _load_contract(task_path: pathlib.Path) -> Contract:
    task, raw = _read_json(task_path, "task_contract_invalid")
    try:
        anchor = task["anchor"]
        constants = task["constants"]
        schema_ok = (
            task["schema"] == TASK_SCHEMA
            and task["familyVersion"] == FAMILY
            and task["nonIssuable"] is True
            and task["edition"] in ALLOWED_EDITIONS
            and isinstance(anchor, dict)
            and isinstance(constants, dict)
        )
        if not schema_ok:
            raise AuthorityUnavailable("task_contract_invalid")

        template_id = task["templateId"]
        task_seed = task["taskSeed"]
        challenge = task["challengeSha256"]
        if not all(isinstance(item, str) and HEX64.fullmatch(item)
                   for item in (template_id, task_seed, challenge)):
            raise AuthorityUnavailable("task_contract_invalid")
        if not isinstance(task["checkerTaskId"], str) or not task["checkerTaskId"]:
            raise AuthorityUnavailable("task_contract_invalid")

        anchor_rel = anchor["path"]
        if not isinstance(anchor_rel, str) or "\\" in anchor_rel:
            raise AuthorityUnavailable("task_contract_invalid")
        pure_rel = pathlib.PurePosixPath(anchor_rel)
        if pure_rel.is_absolute() or not pure_rel.parts or ".." in pure_rel.parts:
            raise AuthorityUnavailable("task_contract_invalid")
        task_root = task_path.resolve(strict=True).parent
        anchor_candidate = task_root / pathlib.Path(*pure_rel.parts)
        if anchor_candidate.is_symlink():
            raise AuthorityUnavailable("anchor_unavailable")
        anchor_path = anchor_candidate.resolve(strict=True)
        anchor_path.relative_to(task_root)
        if not anchor_path.is_file():
            raise AuthorityUnavailable("anchor_unavailable")
        anchor_raw = anchor_path.read_bytes()
        if len(anchor_raw) > 262144:
            raise AuthorityUnavailable("anchor_size_exceeded")
        if hashlib.sha256(anchor_raw).hexdigest() != anchor["sha256"]:
            raise AuthorityUnavailable("anchor_digest_mismatch")
        anchor_text = anchor_raw.decode("utf-8")

        type_name = anchor["typeName"]
        locator = anchor["semanticLocator"]
        field_types = anchor["fieldTypes"]
        if (
            not isinstance(type_name, str)
            or not IDENT.fullmatch(type_name)
            or not isinstance(locator, str)
            or not locator
            or not isinstance(field_types, list)
            or not 1 <= len(field_types) <= 8
            or not all(isinstance(field, str)
                       and field in INTEGER_TYPES | {"bool"} for field in field_types)
        ):
            raise AuthorityUnavailable("task_contract_invalid")

        expected_template = _domain_hash(
            "boole.native-shadow.fixture-template.v1", anchor["sha256"], locator
        )
        expected_challenge = _domain_hash(
            "boole.native-shadow.fixture-challenge.v1", task_seed, template_id
        )
        if template_id != expected_template or challenge != expected_challenge:
            raise AuthorityUnavailable("task_binding_mismatch")

        coeffs = constants["coeffs"]
        if not isinstance(coeffs, list) or len(coeffs) != len(field_types):
            raise AuthorityUnavailable("task_contract_invalid")
        parsed_coeffs = tuple(_bounded_int(value) for value in coeffs)
        scaffold = task["scaffold"]
        if not isinstance(scaffold, str):
            raise AuthorityUnavailable("task_contract_invalid")
    except AuthorityUnavailable:
        raise
    except (KeyError, TypeError, ValueError, OSError, UnicodeDecodeError) as exc:
        raise AuthorityUnavailable("task_contract_invalid") from exc

    return Contract(
        raw_digest=hashlib.sha256(raw).hexdigest(),
        task_id=task["checkerTaskId"],
        edition=task["edition"],
        anchor_text=anchor_text,
        type_name=type_name,
        field_types=tuple(field_types),
        task_seed=task_seed,
        challenge=challenge,
        a0=_bounded_int(constants["a0"]),
        mul=_bounded_int(constants["mul"]),
        coeffs=parsed_coeffs,
        scaffold=scaffold,
    )


def _patch_parts(text: str, markers: list[str]) -> tuple[str, str, str]:
    begin, end = markers
    if text.count(begin) != 1 or text.count(end) != 1:
        raise SubmissionRejected("malformed_patch_region")
    before, remainder = text.split(begin, 1)
    body, after = remainder.split(end, 1)
    return before, body, after


_LINE_COMMENT = re.compile(r"//[^\n]*")
_BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)
_RAW_STRING = re.compile(r'(?:br|rb|r)(?P<hashes>#{0,16})".*?"(?P=hashes)', re.S)
_STRING = re.compile(r'"(?:[^"\\]|\\.)*"', re.S)
_CHAR = re.compile(r"'(?:[^'\\]|\\.)'")


def _code_without_literals(text: str) -> str:
    # Strip literals before comments.  Reversing this order lets `"https://"`
    # hide live code from a line-comment regex even though rustc sees a string.
    text = _RAW_STRING.sub('""', text)
    text = _STRING.sub('""', text)
    text = _CHAR.sub("''", text)
    text = _BLOCK_COMMENT.sub(" ", text)
    text = _LINE_COMMENT.sub(" ", text)
    return text


def _verify_submission(policy: dict[str, Any], contract: Contract,
                       submission_path: pathlib.Path) -> str:
    try:
        submitted = submission_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        raise SubmissionRejected("submission_unreadable") from exc
    markers = policy["patchMarkers"]
    try:
        scaffold_before, _, scaffold_after = _patch_parts(contract.scaffold, markers)
        submitted_before, body, submitted_after = _patch_parts(submitted, markers)
    except (KeyError, TypeError, ValueError) as exc:
        raise AuthorityUnavailable("policy_contract_mismatch") from exc
    if scaffold_before != submitted_before or scaffold_after.rstrip() != submitted_after.rstrip():
        raise SubmissionRejected("outside_patch_modified")

    limits = policy["resourceLimits"]
    if len(body.encode("utf-8")) > limits["patchBytes"]:
        raise SubmissionRejected("patch_size_exceeded")
    if body.count("\n") > limits["patchLines"]:
        raise SubmissionRejected("patch_line_limit_exceeded")
    code = _code_without_literals(body)
    for token in policy["forbiddenTokens"]:
        if token.endswith("!"):
            present = token in code
        else:
            present = re.search(
                r"(?<![A-Za-z0-9_])" + re.escape(token) + r"(?![A-Za-z0-9_])", code
            ) is not None
        if present:
            raise SubmissionRejected("forbidden_construct")
    for pattern in policy["forbiddenPatterns"]:
        if re.search(pattern, code):
            raise SubmissionRejected("forbidden_construct")
    return submitted


class _SplitMix64:
    """The sealed family RNG: splitmix64 seeded by the first 64 seed bits."""

    MASK = (1 << 64) - 1

    def __init__(self, seed_hex: str):
        self.state = int(seed_hex[:16], 16) & self.MASK

    def next(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & self.MASK
        value = self.state
        value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & self.MASK
        value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & self.MASK
        return value ^ (value >> 31)

    def number(self, low: int, high: int) -> int:
        return low + self.next() % (high - low + 1)


def _wrap_i64(value: int) -> int:
    return ((value + (1 << 63)) % (1 << 64)) - (1 << 63)


def _rust_literal(value: int | bool, field_type: str) -> str:
    if field_type == "bool":
        return "true" if value else "false"
    if value < 0:
        return f"({value}{field_type})"
    return f"{value}{field_type}"


def _hidden_harness(contract: Contract, count: int, submitted_bytes: bytes) -> str:
    commitment = _domain_hash("acfr-v1/commitment", contract.task_id, submitted_bytes)
    test_seed = _domain_hash("acfr-v1/test-seed", contract.task_seed, commitment)
    stream = _SplitMix64(test_seed)
    rows: list[str] = []
    expected: list[int] = []
    acc = contract.a0
    for _ in range(count):
        values: list[int | bool] = []
        projection = 0
        for field_type, coefficient in zip(contract.field_types, contract.coeffs):
            if field_type == "bool":
                value: int | bool = bool(stream.number(0, 1))
                numeric = 1 if value else 0
            elif field_type.startswith("u"):
                value = stream.number(0, 60)
                numeric = value
            else:
                value = stream.number(-60, 60)
                numeric = value
            values.append(value)
            projection = _wrap_i64(projection + _wrap_i64(numeric * coefficient))
        acc = _wrap_i64(_wrap_i64(acc * contract.mul) + projection)
        rows.append(
            f"AcfrTy({', '.join(_rust_literal(v, t) for v, t in zip(values, contract.field_types))})"
        )
        expected.append(acc)

    return """#![allow(unused)]
use crate::%s as AcfrTy;
use crate::boole_submission::acfr_solve;

#[test]
fn boole_native_hidden_cases() {
    let items: [AcfrTy; %d] = [
        %s
    ];
    let expected: [i64; %d] = [%s];
    for end in 0..items.len() {
        assert_eq!(acfr_solve(&items[..=end]), expected[end], "case {}", end);
    }
}
""" % (
        contract.type_name,
        count,
        ",\n        ".join(rows),
        count,
        ", ".join(str(value) for value in expected),
    )


def _cargo_manifest(edition: str) -> str:
    return """[package]
name = "boole_native_shadow_task"
version = "0.0.0"
edition = "%s"
publish = false

[lib]
path = "src/lib.rs"
doctest = false

[profile.dev]
debug = false
incremental = false
""" % edition


CARGO_LOCK = """# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "boole_native_shadow_task"
version = "0.0.0"
"""


def _set_limits(limits: dict[str, int]) -> None:
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_CPU, (limits["cpuSeconds"], limits["cpuSeconds"]))
    resource.setrlimit(resource.RLIMIT_FSIZE, (limits["fileBytes"], limits["fileBytes"]))
    resource.setrlimit(resource.RLIMIT_NOFILE, (limits["openFiles"], limits["openFiles"]))
    # RLIMIT_NPROC is deliberately not used.  Linux and Darwin both account it
    # across the real UID (including threads), not this subprocess tree.  A
    # shared CI or node user can therefore make an otherwise-valid compiler
    # fail.  Production process-count containment requires a dedicated cgroup
    # or PID namespace and is outside this non-activatable qualification release.
    # Darwin exposes RLIMIT_AS but rejects practical finite values.  All other
    # supported platforms must accept the frozen ceiling or the run is
    # unavailable; this is not a best-effort limit.
    if sys.platform != "darwin" and hasattr(resource, "RLIMIT_AS"):
        resource.setrlimit(
            resource.RLIMIT_AS, (limits["memoryBytes"], limits["memoryBytes"])
        )


def _kill_group(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def _run_contained(command: list[str], cwd: pathlib.Path, env: dict[str, str],
                   limits: dict[str, int]) -> tuple[int, bytes]:
    try:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            preexec_fn=lambda: _set_limits(limits),
        )
    except (OSError, subprocess.SubprocessError) as exc:
        raise AuthorityUnavailable("contained_process_unavailable") from exc
    assert process.stdout is not None
    os.set_blocking(process.stdout.fileno(), False)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    output = bytearray()
    deadline = time.monotonic() + limits["wallSeconds"]
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                _kill_group(process)
                process.wait()
                raise AuthorityUnavailable("resource_wall_limit")
            events = selector.select(min(0.1, remaining))
            for key, _ in events:
                chunk = os.read(key.fd, 65536)
                if chunk:
                    output.extend(chunk)
                    if len(output) > limits["outputBytes"]:
                        _kill_group(process)
                        process.wait()
                        raise AuthorityUnavailable("resource_output_limit")
                else:
                    selector.unregister(key.fileobj)
            code = process.poll()
            if code is not None and not selector.get_map():
                _kill_group(process)
                return code, bytes(output)
    finally:
        selector.close()
        process.stdout.close()
        if process.poll() is None:
            _kill_group(process)
            process.wait()


def _infrastructure_failure_reason(code: int, output: bytes) -> str | None:
    if code == 0:
        return None
    if code < 0:
        return "resource_process_terminated"
    lines = tuple(b" ".join(line.strip().lower().split()) for line in output.splitlines())
    system_error = (
        b"= note: terminate called after throwing an instance of 'std::system_error'"
        in lines
        and b"what(): resource temporarily unavailable" in lines
    )
    spawn_error = any(
        (
            line.startswith(b"error: could not execute process")
            or line.startswith(b"error: failed to spawn")
        )
        and line.endswith(b"(os error 11)")
        for line in lines
    )
    process_failure = system_error or spawn_error
    if process_failure:
        return "resource_process_limit"
    memory_failure = any(
        re.fullmatch(rb"memory allocation of [0-9]+ bytes failed", line)
        or re.fullmatch(rb"(?:[^ |:]+: )?fatal error: cannot allocate memory", line)
        or (
            line.startswith(b"error:")
            and line.endswith(b"cannot allocate memory (os error 12)")
        )
        for line in lines
    )
    if memory_failure:
        return "resource_memory_limit"
    return None


def _judge(policy: dict[str, Any], contract: Contract, submission: str,
           toolchain_bin: pathlib.Path, scratch_root: pathlib.Path) -> None:
    if not scratch_root.is_dir() or scratch_root.is_symlink():
        raise AuthorityUnavailable("scratch_root_unavailable")
    limits = policy["resourceLimits"]
    try:
        with tempfile.TemporaryDirectory(prefix="boole-native-check-", dir=scratch_root) as tmp:
            root = pathlib.Path(tmp)
            src = root / "src"
            src.mkdir()
            home = root / "home"
            cargo_home = root / "cargo-home"
            target = root / "target"
            temporary = root / "tmp"
            for directory in (home, cargo_home, target, temporary):
                directory.mkdir()
            (root / "Cargo.toml").write_text(_cargo_manifest(contract.edition), encoding="utf-8")
            (root / "Cargo.lock").write_text(CARGO_LOCK, encoding="utf-8")
            (src / "submission.rs").write_text(submission, encoding="utf-8")
            (src / "hidden.rs").write_text(
                _hidden_harness(contract, limits["hiddenCases"], submission.encode("utf-8")),
                encoding="utf-8",
            )
            anchor = contract.anchor_text
            if not anchor.endswith("\n"):
                anchor += "\n"
            (src / "lib.rs").write_text(
                anchor
                + "\n#[path = \"submission.rs\"]\nmod boole_submission;\n"
                + "#[cfg(test)]\n#[path = \"hidden.rs\"]\nmod boole_hidden_tests;\n",
                encoding="utf-8",
            )

            cargo = toolchain_bin / "cargo"
            rustc = toolchain_bin / "rustc"
            env = {
                "PATH": str(toolchain_bin) + ":" + policy["cargo"]["systemPath"],
                "HOME": str(home),
                "CARGO_HOME": str(cargo_home),
                "CARGO_TARGET_DIR": str(target),
                "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER": "/usr/bin/x86_64-linux-gnu-gcc-13",
                "CARGO_NET_OFFLINE": "true",
                "CARGO_TERM_COLOR": "never",
                "RUSTC": str(rustc),
                "RUSTFLAGS": policy["cargo"]["rustflags"],
                "LC_ALL": "C",
                "TERM": "dumb",
                "TMPDIR": str(temporary),
            }
            code, output = _run_contained(
                [str(cargo), *policy["cargo"]["testArgs"]], root, env, limits
            )
            infrastructure_failure = _infrastructure_failure_reason(code, output)
            if infrastructure_failure is not None:
                raise AuthorityUnavailable(infrastructure_failure)
            if code != 0:
                raise SubmissionRejected("compile_or_hidden_test_failed")
    except AuthorityUnavailable:
        raise
    except SubmissionRejected:
        raise
    except OSError as exc:
        raise AuthorityUnavailable("scratch_workspace_unavailable") from exc


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="tracked Rust tuple-struct checker")
    parser.add_argument("--task", required=True, type=pathlib.Path)
    parser.add_argument("--submission", required=True, type=pathlib.Path)
    parser.add_argument("--toolchain-bin", required=True, type=pathlib.Path)
    parser.add_argument("--scratch-root", type=pathlib.Path)
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    contract: Contract | None = None
    try:
        policy = _load_policy()
        toolchain_bin = _toolchain_directory(args.toolchain_bin)
        _verify_toolchain(policy, toolchain_bin)
        contract = _load_contract(args.task)
        submission = _verify_submission(policy, contract, args.submission)
        if args.scratch_root is None:
            raise AuthorityUnavailable("scratch_root_required")
        _judge(policy, contract, submission, toolchain_bin, args.scratch_root)
    except SubmissionRejected as exc:
        _json_result("deterministic_reject", exc.reason_code, contract)
    except AuthorityUnavailable as exc:
        _json_result("retryable_unavailable", exc.reason_code, contract)
    except Exception:
        _json_result("retryable_unavailable", "checker_internal_error", contract)
    else:
        _json_result("accepted", "accepted", contract)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
