#!/usr/bin/env python3
"""Create Boole model Proof-to-Block benchmark artifact bundles.

This is a non-consensus runner skeleton. It executes command-based benchmark
rows, records pass/skip/fail rows, and writes stable artifacts that later live
Ollama/frontier runners can fill with real proof-attempt data.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]


def now_ms() -> int:
    return int(time.time() * 1000)


def default_run_id() -> str:
    return time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())


def load_spec(path: Path) -> list[dict[str, Any]]:
    parsed = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(parsed, list):
        raise SystemExit("benchmark spec must be a JSON array")
    for row in parsed:
        if not isinstance(row, dict):
            raise SystemExit("benchmark spec rows must be JSON objects")
    return parsed


def parse_json_output(stdout: str) -> dict[str, Any] | None:
    candidates: list[str] = []
    for line in stdout.splitlines():
        stripped = line.strip()
        if stripped.startswith("{") and stripped.endswith("}"):
            candidates.append(stripped)
    if candidates:
        return json.loads(candidates[-1])
    match = re.search(r"(\{[\s\S]*\})\s*$", stdout.strip())
    if match:
        return json.loads(match.group(1))
    return None


def missing_env(row: dict[str, Any]) -> list[str]:
    return [name for name in row.get("requireEnv", []) if isinstance(name, str) and not os.environ.get(name)]


def blocks_produced_from_result(parsed: dict[str, Any] | None) -> int:
    data = parsed or {}
    summary = data.get("summary") or {}
    status = data.get("status") or {}
    aggregate = data.get("aggregate") or {}
    return int(
        status.get("height")
        or summary.get("blocksMined")
        or summary.get("blocksProduced")
        or aggregate.get("blocksProduced")
        or 0
    )


def verified_shares_from_result(parsed: dict[str, Any] | None) -> int:
    data = parsed or {}
    summary = data.get("summary") or {}
    aggregate = data.get("aggregate") or {}
    return int(
        summary.get("verifyAccepted")
        or aggregate.get("verifyAccepted")
        or summary.get("sharesAccepted")
        or aggregate.get("sharesAccepted")
        or 0
    )


def replay_pass_from_result(parsed: dict[str, Any] | None) -> bool:
    data = parsed or {}
    status = data.get("status") or {}
    safety = data.get("safety") or {}
    return bool(
        status.get("replayMatchesRuntime")
        or data.get("replayMatchesRuntime")
        or safety.get("replayMatchesRuntime")
        or False
    )


def score_from_result(parsed: dict[str, Any] | None) -> dict[str, Any]:
    return {
        "blocksProduced": blocks_produced_from_result(parsed),
        "replayPass": replay_pass_from_result(parsed),
    }


def diagnostics_from_result(parsed: dict[str, Any] | None) -> dict[str, Any]:
    return {"verifiedShares": verified_shares_from_result(parsed)}


def zero_score(*, replay_pass: bool = True) -> dict[str, Any]:
    return {"blocksProduced": 0, "replayPass": replay_pass}


def zero_diagnostics() -> dict[str, Any]:
    return {"verifiedShares": 0}


def mining_path_status(*, target_issued: bool, model_generated: bool, candidate_wrapped: bool, submit_lean_invoked: bool, verifier_accepted: bool, share_accepted: bool, block_produced: bool, replay_passed: bool) -> dict[str, bool]:
    """Expose the controlled local mining path as explicit row evidence.

    The benchmark is not a generation-only score: a public block score must pass
    target issuance, model candidate generation, verifier/admission, share/block
    selection, and replay.  `canonicalPackageSubmitted` is true only when the
    submit-lean verifier path was invoked with a wrapped candidate package.
    """
    return {
        "targetIssued": target_issued,
        "modelGenerated": model_generated,
        "candidateWrapped": candidate_wrapped,
        "submitLeanInvoked": submit_lean_invoked,
        "verifierAccepted": verifier_accepted,
        "canonicalPackageSubmitted": submit_lean_invoked and candidate_wrapped,
        "shareAccepted": share_accepted,
        "blockProduced": block_produced,
        "replayPassed": replay_passed,
    }


def block_production_rate_pct(*, blocks_produced: int, generated_attempts: int) -> float:
    if generated_attempts <= 0:
        return 0.0
    return round((blocks_produced / generated_attempts) * 100.0, 2)


def safety_from_result(parsed: dict[str, Any] | None) -> dict[str, int]:
    safety = (parsed or {}).get("safety") or {}
    summary = (parsed or {}).get("summary") or {}
    return {
        "invalidAccepted": int(safety.get("invalidAccepted") or summary.get("invalidAccepted") or 0),
        "chainDivergence": int(safety.get("chainDivergence") or summary.get("chainDivergence") or 0),
        "replayFailures": int(safety.get("replayFailures") or summary.get("replayFailures") or 0),
    }


def validate_row(row: dict[str, Any]) -> tuple[str, list[str]]:
    name = row.get("name")
    command = row.get("command")
    if not isinstance(name, str) or not name:
        raise SystemExit(f"benchmark row missing name: {row}")
    if not isinstance(command, list) or not command or not all(isinstance(part, str) for part in command):
        raise SystemExit(f"benchmark row {name} command must be a non-empty string array")
    return name, command


def run_row(row: dict[str, Any], timeout_s: int) -> dict[str, Any]:
    name, command = validate_row(row)
    required_missing = missing_env(row)
    base = {
        "name": name,
        "kind": row.get("kind"),
        "metadata": row.get("metadata"),
    }
    if required_missing:
        return {
            **base,
            "ok": True,
            "skipped": True,
            "status": "SKIP",
            "reason": "missing_required_env",
            "missingEnv": required_missing,
            "elapsedMs": 0,
            "score": zero_score(replay_pass=False),
            "diagnostics": zero_diagnostics(),
            "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
        }

    env = os.environ.copy()
    for key, value in (row.get("env") or {}).items():
        if not isinstance(key, str) or not isinstance(value, str):
            raise SystemExit(f"benchmark row {name} env values must be strings")
        env[key] = value

    started = time.time()
    proc = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=int(row.get("timeoutSec", timeout_s)),
        check=False,
    )
    elapsed_ms = int((time.time() - started) * 1000)
    parsed = parse_json_output(proc.stdout) if proc.stdout else None
    row_ok = proc.returncode == 0 and parsed is not None and parsed.get("ok") is True
    skipped = bool((parsed or {}).get("skipped"))
    return {
        **base,
        "ok": row_ok,
        "skipped": skipped,
        "status": "SKIP" if skipped else ("PASS" if row_ok else "FAIL"),
        "exitCode": proc.returncode,
        "elapsedMs": elapsed_ms,
        "score": score_from_result(parsed),
        "diagnostics": diagnostics_from_result(parsed),
        "safety": safety_from_result(parsed),
        "result": parsed,
        "stderrTail": proc.stderr[-1200:],
        "stdoutTail": proc.stdout[-1200:],
    }


def parse_ollama_target(target: str) -> str:
    prefix = "ollama:"
    if not target.startswith(prefix) or target == prefix:
        raise SystemExit(f"unsupported benchmark target: {target}")
    return target[len(prefix) :]


def parse_claude_cli_target(target: str) -> str:
    prefix = "claude-cli:"
    if not target.startswith(prefix) or target == prefix:
        raise SystemExit(f"unsupported benchmark target: {target}")
    return target[len(prefix) :]


def resolve_command(command: str) -> str | None:
    command_path = Path(command).expanduser()
    if command_path.is_absolute() or "/" in command:
        return str(command_path) if command_path.exists() else None
    return shutil.which(command)


def candidate_preview(text: str, limit: int = 160) -> str:
    compact = " ".join(text.strip().split())
    return compact[:limit]


def extract_proof_term_candidate(raw_output: str) -> tuple[str | None, dict[str, Any], str | None]:
    raw = raw_output.strip()
    extraction: dict[str, Any] = {"mode": "proof-term", "format": "raw", "normalization": "none"}
    if not raw:
        return None, extraction, "candidate-empty"

    fence_matches = list(re.finditer(r"```([A-Za-z0-9_-]*)\s*\n([\s\S]*?)\n```", raw))
    if fence_matches:
        if len(fence_matches) != 1 or raw[: fence_matches[0].start()].strip() or raw[fence_matches[0].end() :].strip():
            extraction.update({"format": "ambiguous-fenced", "normalization": "none"})
            return None, extraction, "candidate-ambiguous"
        lang = fence_matches[0].group(1).strip().lower()
        raw = fence_matches[0].group(2).strip()
        extraction.update({"format": "fenced-lean" if lang == "lean" else "fenced-code", "normalization": "strip-fence"})

    parsed_json = parse_json_output(raw)
    if parsed_json is not None:
        for key in ("proof_lean", "proofTerm", "proof", "code"):
            value = parsed_json.get(key)
            if isinstance(value, str) and value.strip():
                raw = value.strip()
                extraction.update({"format": f"json-{key}", "normalization": "json-extract"})
                break
        else:
            return None, extraction | {"format": "json"}, "candidate-missing-proof-field"

    lowered = raw.lower()
    if re.search(r"\b(sorry|admit)\b", lowered):
        return None, extraction, "candidate-forbidden-token"
    if "```" in raw:
        return None, extraction, "candidate-shape-invalid"
    if re.search(r"^\s*(import|open|namespace|section|def|theorem|lemma|example)\b", raw, re.MULTILINE):
        return None, extraction, "candidate-shape-invalid"
    if re.search(r"^\s*by\b", raw):
        return None, extraction, "candidate-shape-invalid"
    return raw, extraction, None


MINING_TARGET_FAMILY = "boole.calibration.pow.v1"
SMOKE_TARGET_FAMILY = "boole.smoke.true.v1"


def target_family_for_mode(benchmark_mode: str) -> str:
    if benchmark_mode == "smoke":
        return SMOKE_TARGET_FAMILY
    if benchmark_mode == "mining":
        return MINING_TARGET_FAMILY
    raise SystemExit(f"unsupported benchmark mode: {benchmark_mode}")


def attempt_context(run_id: str, target: str, attempt_index: int, *, benchmark_mode: str = "mining") -> dict[str, Any]:
    target_family = target_family_for_mode(benchmark_mode)
    seed = f"{run_id}|{target}|{attempt_index}|{benchmark_mode}|{target_family}"
    challenge = hashlib.sha256((seed + "|challenge").encode("utf-8")).hexdigest()
    nonce = hashlib.sha256((seed + "|nonce").encode("utf-8")).hexdigest()[:32]
    return {
        "benchmarkMode": benchmark_mode,
        "targetFamily": target_family,
        "attemptIndex": attempt_index,
        "challenge": challenge,
        "nonce": nonce,
        "theoremName": f"boole_benchmark_pow_target_{attempt_index + 1}",
    }


def row_target_metadata(*, benchmark_mode: str, attempt_context: dict[str, Any] | None = None) -> dict[str, Any]:
    target_family = (attempt_context or {}).get("targetFamily") or target_family_for_mode(benchmark_mode)
    metadata: dict[str, Any] = {
        "benchmarkMode": benchmark_mode,
        "targetFamily": target_family,
    }
    if attempt_context:
        metadata["lotterySample"] = {
            "challenge": attempt_context["challenge"],
            "nonce": attempt_context["nonce"],
            "theoremName": attempt_context["theoremName"],
        }
    return metadata


def wrap_proof_term_candidate(proof_term: str, *, benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None) -> str:
    indented = "\n".join(("  " + line) if line.strip() else line for line in proof_term.splitlines())
    if benchmark_mode == "smoke":
        return f"theorem boole_benchmark_true : True :=\n{indented}\n"
    ctx = attempt_context or globals()["attempt_context"]("manual", "manual", 0, benchmark_mode="mining")
    theorem_name = ctx["theoremName"]
    challenge = ctx["challenge"]
    nonce = ctx["nonce"]
    return (
        f"-- benchmarkMode: mining\n"
        f"-- targetFamily: {MINING_TARGET_FAMILY}\n"
        f"-- lotteryChallenge: {challenge}\n"
        f"-- lotteryNonce: {nonce}\n"
        f"theorem {theorem_name} : \"{challenge}\" = \"{challenge}\" :=\n"
        f"{indented}\n"
    )


def rejected_candidate_shape_row(*, target: str, provider: str, model: str, attempt_index: int, reason: str, elapsed_ms: int, raw_output: str, extraction: dict[str, Any], stderr: str = "", benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None) -> dict[str, Any]:
    return {
        **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=attempt_context),
        "name": f"{target} attempt {attempt_index + 1}",
        "kind": "provider-model",
        "target": target,
        "provider": provider,
        "model": model,
        "attemptIndex": attempt_index,
        "ok": True,
        "skipped": False,
        "status": "REJECTED",
        "reason": reason,
        "generatedAttempt": False,
        "candidateMode": "proof-term",
        "candidateExtraction": extraction,
        "candidatePreview": candidate_preview(raw_output),
        "accepted": False,
        "invalidAccepted": False,
        "elapsedMs": elapsed_ms,
        "latencyMs": elapsed_ms,
        "score": zero_score(),
        "diagnostics": zero_diagnostics(),
        "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
        "verifier": {"invoked": False, "command": "submit-lean"},
        "stderrTail": stderr[-1200:],
        "stdoutTail": raw_output[-1200:],
    }


def model_proof_term_prompt(*, benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None) -> str:
    if benchmark_mode == "smoke":
        return (
            "Boole proof-to-block benchmark SMOKE target contract. Return exactly one Lean 4 proof term for this theorem body, "
            "not a full theorem and not a Markdown code block. Target theorem: `theorem boole_benchmark_true : True := <YOUR_PROOF_TERM>`. "
            "Valid example response: `True.intro`. This mode is smoke-only and is not a public mining score. "
            "Do not include `theorem`, `lemma`, `example`, `import`, explanations, JSON, markdown fences, `by`, `sorry`, or `admit`. "
            "The returned term will be inserted verbatim after `:=` and verified by Boole's submit-lean path."
        )
    ctx = attempt_context or globals()["attempt_context"]("manual", "manual", 0, benchmark_mode="mining")
    return (
        "Boole proof-to-block benchmark MINING target contract. Return exactly one Lean 4 proof term for this theorem body, "
        "not a full theorem and not a Markdown code block. "
        f"Target family: `{ctx['targetFamily']}`. Lottery challenge: `{ctx['challenge']}`. Nonce: `{ctx['nonce']}`. "
        f"Target theorem: `theorem {ctx['theoremName']} : \"{ctx['challenge']}\" = \"{ctx['challenge']}\" := <YOUR_PROOF_TERM>`. "
        "A minimal valid proof term for this equality target is `rfl`. Do not include `theorem`, `lemma`, `example`, `import`, explanations, JSON, markdown fences, `by`, `sorry`, or `admit`. "
        "The returned term will be bound to this per-attempt lottery sample and verified by Boole's submit-lean path."
    )


def classify_ollama_failure(stderr: str, stdout: str, returncode: int) -> str:
    combined = f"{stderr}\n{stdout}".lower()
    if "not found" in combined and "model" in combined:
        return "ollama-model-missing"
    if "pull" in combined and "model" in combined:
        return "ollama-model-missing"
    if "connection refused" in combined or "could not connect" in combined or "daemon" in combined:
        return "ollama-daemon-unavailable"
    if returncode == 127:
        return "ollama-command-not-found"
    return "ollama-generation-failed"


def setup_required_ollama_row(*, target: str, model: str, attempt_index: int, reason: str, elapsed_ms: int = 0, stderr: str = "", stdout: str = "", benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None) -> dict[str, Any]:
    return {
        **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=attempt_context),
        "name": f"{target} attempt {attempt_index + 1}",
        "kind": "provider-model",
        "target": target,
        "provider": "ollama",
        "model": model,
        "attemptIndex": attempt_index,
        "ok": True,
        "skipped": True,
        "status": "SETUP_REQUIRED",
        "reason": reason,
        "generatedAttempt": False,
        "accepted": False,
        "invalidAccepted": False,
        "elapsedMs": elapsed_ms,
        "latencyMs": elapsed_ms,
        "score": zero_score(),
        "diagnostics": zero_diagnostics(),
        "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
        "stderrTail": stderr[-1200:],
        "stdoutTail": stdout[-1200:],
        "recovery": recovery_for_ollama_reason(reason, model),
    }


def recovery_for_ollama_reason(reason: str, model: str) -> list[str]:
    if reason == "ollama-command-not-found":
        return ["Install Ollama, then retry this benchmark target."]
    if reason == "ollama-daemon-unavailable":
        return ["Start Ollama manually with `ollama serve`, then retry."]
    if reason == "ollama-model-missing":
        return [f"Pull the model manually with `ollama pull {model}`, then retry."]
    return ["Inspect Ollama stderr/stdout tail and retry after fixing local setup."]


def setup_required_claude_cli_row(*, target: str, model: str, attempt_index: int, reason: str, elapsed_ms: int = 0, stderr: str = "", stdout: str = "", benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None) -> dict[str, Any]:
    return {
        **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=attempt_context),
        "name": f"{target} attempt {attempt_index + 1}",
        "kind": "provider-model",
        "target": target,
        "provider": "claude-cli",
        "model": model,
        "attemptIndex": attempt_index,
        "ok": True,
        "skipped": True,
        "status": "SETUP_REQUIRED",
        "reason": reason,
        "generatedAttempt": False,
        "accepted": False,
        "invalidAccepted": False,
        "elapsedMs": elapsed_ms,
        "latencyMs": elapsed_ms,
        "score": zero_score(),
        "diagnostics": zero_diagnostics(),
        "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
        "stderrTail": stderr[-1200:],
        "stdoutTail": stdout[-1200:],
        "recovery": ["Install/authenticate Claude CLI, then retry this benchmark target."],
    }


def write_lean_checker_workspace(workspace: Path) -> None:
    if workspace.exists():
        shutil.rmtree(workspace)
    (workspace / "BooleCheck").mkdir(parents=True)
    (workspace / "lean-toolchain").write_text("leanprover/lean4:v4.29.1\n", encoding="utf-8")
    (workspace / "lakefile.lean").write_text(
        """import Lake
open Lake DSL

package boole_check_fixture

lean_exe boole_check where
  root := `BooleCheck.Main
""",
        encoding="utf-8",
    )
    (workspace / "lake-manifest.json").write_text(
        """{\"version\": \"1.1.0\",
 \"packagesDir\": \".lake/packages\",
 \"packages\": [],
 \"name\": \"boole_check_fixture\",
 \"lakeDir\": \".lake\"}
""",
        encoding="utf-8",
    )
    (workspace / "BooleCheck" / "Main.lean").write_text(
        """def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln \"usage: boole_check <proof.lean>\"; return 64
  let output ← IO.Process.output {
    cmd := \"lean\"
    args := #[proofPath]
  }
  if output.stdout.length > 0 then
    IO.print output.stdout
  if output.stderr.length > 0 then
    IO.eprint output.stderr
  if output.exitCode == 0 then
    return 0
  else
    return 1
""",
        encoding="utf-8",
    )


def checker_artifact_hash(workspace: Path) -> str:
    entries: list[tuple[str, bytes]] = []
    for relative in ["lean-toolchain", "lakefile.lean", "lake-manifest.json"]:
        path = workspace / relative
        entries.append((relative, path.read_bytes()))
    checker_root = workspace / "BooleCheck"
    if checker_root.exists():
        for path in checker_root.rglob("*"):
            if path.is_symlink():
                raise RuntimeError(f"symlink not allowed inside checker package: {path}")
            if path.is_file():
                entries.append((path.relative_to(workspace).as_posix(), path.read_bytes()))
    entries.sort(key=lambda item: item[0])
    hasher = hashlib.sha256()
    for relative, data in entries:
        hasher.update(relative.encode())
        hasher.update(b"\0")
        hasher.update(data)
        hasher.update(b"\0")
    return hasher.hexdigest()


def parse_submit_lean_output(proc: subprocess.CompletedProcess[str]) -> dict[str, Any] | None:
    return parse_json_output(proc.stdout) or parse_json_output(proc.stderr)


def submit_candidate_to_verifier(*, candidate: str, target: str, model: str, attempt_index: int, submit_lean_command: str, candidate_root: Path, timeout_s: int | None, benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None) -> dict[str, Any]:
    resolved_submit = resolve_command(submit_lean_command)
    if resolved_submit is None:
        return {
            **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=attempt_context),
            "invoked": False,
            "command": "submit-lean",
            "reason": "submit-lean-command-not-found",
            "exitCode": None,
            "accepted": False,
            "shareAccepted": False,
            "replayMatchesRuntime": True,
            "invalidAccepted": 0,
            "score": zero_score(),
            "diagnostics": zero_diagnostics(),
            "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
        }

    workspace = (candidate_root / f"attempt-{attempt_index + 1}").resolve()
    write_lean_checker_workspace(workspace)
    proof_path = (workspace / "ModelCandidate.lean").resolve()
    proof_path.write_text(candidate + "\n", encoding="utf-8")
    block_store = (workspace / "blockstore.ndjson").resolve()
    required_checker_hash = checker_artifact_hash(workspace)
    verifier_hash = "boole-model-benchmark-ollama-v0"
    started = time.time()
    proc = subprocess.run(
        [
            resolved_submit,
            "submit-lean",
            "--proof",
            str(proof_path),
            "--checker-dir",
            str(workspace),
            "--fixture",
            "fixtures/protocol/admission/v1.json",
            "--block-store",
            str(block_store),
            "--verifier-hash",
            verifier_hash,
            "--require-checker-artifact-hash",
            required_checker_hash,
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout_s,
        check=False,
    )
    elapsed_ms = int((time.time() - started) * 1000)
    parsed = parse_submit_lean_output(proc)
    accepted = proc.returncode == 0 and bool((parsed or {}).get("accepted"))
    share_accepted = bool((parsed or {}).get("shareAccepted"))
    replay_matches = bool((parsed or {}).get("replayMatchesRuntime"))
    invalid_accepted = int((parsed or {}).get("invalidAccepted") or 0)
    blocks = 1 if accepted and share_accepted and (parsed or {}).get("block") else 0
    verified = 1 if accepted and share_accepted else 0
    return {
        **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=attempt_context),
        "invoked": True,
        "command": "submit-lean",
        "exitCode": proc.returncode,
        "elapsedMs": elapsed_ms,
        "accepted": accepted,
        "shareAccepted": share_accepted,
        "replayMatchesRuntime": replay_matches,
        "invalidAccepted": invalid_accepted,
        "miningPath": mining_path_status(
            target_issued=True,
            model_generated=True,
            candidate_wrapped=True,
            submit_lean_invoked=True,
            verifier_accepted=accepted,
            share_accepted=share_accepted,
            block_produced=blocks > 0,
            replay_passed=replay_matches,
        ),
        "verifierHash": verifier_hash,
        "checkerArtifactHash": required_checker_hash,
        "proofSha256": hashlib.sha256(candidate.encode("utf-8")).hexdigest(),
        "result": parsed,
        "score": {"blocksProduced": blocks, "replayPass": replay_matches},
        "diagnostics": {"verifiedShares": verified},
        "safety": {"invalidAccepted": invalid_accepted, "chainDivergence": 0, "replayFailures": 0 if replay_matches else 1},
        "stderrTail": proc.stderr[-1200:],
        "stdoutTail": proc.stdout[-1200:],
        "target": target,
        "model": model,
    }

def run_ollama_attempts(*, target: str, ollama_command: str, attempts: int, timeout_s: int | None, submit_lean_command: str | None = None, candidate_root: Path | None = None, on_row: Any | None = None, benchmark_mode: str = "mining", run_id: str = "manual") -> list[dict[str, Any]]:
    model = parse_ollama_target(target)
    resolved_command = resolve_command(ollama_command)
    if resolved_command is None:
        return [
            setup_required_ollama_row(
                target=target,
                model=model,
                attempt_index=idx,
                reason="ollama-command-not-found",
                benchmark_mode=benchmark_mode,
                attempt_context=attempt_context(run_id=run_id, target=target, attempt_index=idx, benchmark_mode=benchmark_mode),
            )
            for idx in range(attempts)
        ]

    rows: list[dict[str, Any]] = []
    for idx in range(attempts):
        ctx = attempt_context(run_id=run_id, target=target, attempt_index=idx, benchmark_mode=benchmark_mode)
        prompt = model_proof_term_prompt(benchmark_mode=benchmark_mode, attempt_context=ctx)
        started = time.time()
        try:
            proc = subprocess.run(
                [resolved_command, "run", model, prompt],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=timeout_s,
                check=False,
            )
        except subprocess.TimeoutExpired as err:
            elapsed_ms = int((time.time() - started) * 1000)
            rows.append(
                {
                    **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=ctx),
                    "name": f"{target} attempt {idx + 1}",
                    "kind": "provider-model",
                    "target": target,
                    "provider": "ollama",
                    "model": model,
                    "attemptIndex": idx,
                    "ok": True,
                    "skipped": False,
                    "status": "REJECTED",
                    "reason": "ollama-timeout",
                    "generatedAttempt": False,
                    "accepted": False,
                    "invalidAccepted": False,
                    "elapsedMs": elapsed_ms,
                    "latencyMs": elapsed_ms,
                    "score": zero_score(),
                    "diagnostics": zero_diagnostics(),
                    "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
                    "verifier": {"invoked": False, "command": "submit-lean"},
                    "stderrTail": str(err)[-1200:],
                    "stdoutTail": "",
                }
            )
            if on_row:
                on_row(rows[-1], rows)
            continue
        elapsed_ms = int((time.time() - started) * 1000)
        if proc.returncode != 0:
            rows.append(
                setup_required_ollama_row(
                    target=target,
                    model=model,
                    attempt_index=idx,
                    reason=classify_ollama_failure(proc.stderr, proc.stdout, proc.returncode),
                    elapsed_ms=elapsed_ms,
                    stderr=proc.stderr,
                    stdout=proc.stdout,
                    benchmark_mode=benchmark_mode,
                    attempt_context=ctx,
                )
            )
            if on_row:
                on_row(rows[-1], rows)
            continue

        raw_candidate = proc.stdout.strip()
        proof_term, extraction, extraction_reason = extract_proof_term_candidate(raw_candidate)
        if extraction_reason:
            rows.append(
                rejected_candidate_shape_row(
                    target=target,
                    provider="ollama",
                    model=model,
                    attempt_index=idx,
                    reason=extraction_reason,
                    elapsed_ms=elapsed_ms,
                    raw_output=raw_candidate,
                    extraction=extraction,
                    stderr=proc.stderr,
                    benchmark_mode=benchmark_mode,
                    attempt_context=ctx,
                )
            )
            if on_row:
                on_row(rows[-1], rows)
            continue

        candidate = wrap_proof_term_candidate(proof_term or "", benchmark_mode=benchmark_mode, attempt_context=ctx)
        digest = hashlib.sha256(candidate.encode("utf-8")).hexdigest()
        verifier = None
        if submit_lean_command:
            verifier = submit_candidate_to_verifier(
                candidate=candidate,
                target=target,
                model=model,
                attempt_index=idx,
                submit_lean_command=submit_lean_command,
                candidate_root=candidate_root or (ROOT / "artifacts" / "model-benchmarks" / "candidates"),
                timeout_s=timeout_s,
                benchmark_mode=benchmark_mode,
                attempt_context=ctx,
            )
        accepted = bool((verifier or {}).get("accepted"))
        score = (verifier or {}).get("score") or zero_score()
        diagnostics = (verifier or {}).get("diagnostics") or zero_diagnostics()
        safety = (verifier or {}).get("safety") or {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}
        mining_path = (verifier or {}).get("miningPath") or mining_path_status(
            target_issued=True,
            model_generated=True,
            candidate_wrapped=True,
            submit_lean_invoked=False,
            verifier_accepted=False,
            share_accepted=False,
            block_produced=False,
            replay_passed=True,
        )
        rows.append(
            {
                **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=ctx),
                "name": f"{target} attempt {idx + 1}",
                "kind": "provider-model",
                "target": target,
                "provider": "ollama",
                "model": model,
                "attemptIndex": idx,
                "ok": True,
                "skipped": False,
                "status": "ACCEPTED" if accepted else "REJECTED",
                "reason": None if accepted else ((verifier or {}).get("reason") or ("verifier_rejected" if verifier else "verifier-integration-pending")),
                "generatedAttempt": True,
                "candidateMode": "proof-term",
                "candidateExtraction": extraction,
                "candidateSha256": digest,
                "candidateTermSha256": hashlib.sha256((proof_term or "").encode("utf-8")).hexdigest(),
                "candidatePreview": candidate_preview(proof_term or ""),
                "accepted": accepted,
                "invalidAccepted": bool(safety.get("invalidAccepted", 0)),
                "elapsedMs": elapsed_ms,
                "latencyMs": elapsed_ms,
                "score": score,
                "diagnostics": diagnostics,
                "safety": safety,
                "miningPath": mining_path,
                "verifier": verifier or {"invoked": False, "command": "submit-lean"},
                "stderrTail": ((verifier or {}).get("stderrTail") or proc.stderr)[-1200:],
                "stdoutTail": ((verifier or {}).get("stdoutTail") or proc.stdout)[-1200:],
            }
        )
        if on_row:
            on_row(rows[-1], rows)
    return rows


def run_claude_cli_attempts(*, target: str, claude_command: str, attempts: int, timeout_s: int | None, submit_lean_command: str | None = None, candidate_root: Path | None = None, on_row: Any | None = None, benchmark_mode: str = "mining", run_id: str = "manual") -> list[dict[str, Any]]:
    model = parse_claude_cli_target(target)
    resolved_command = resolve_command(claude_command)
    if resolved_command is None:
        return [
            setup_required_claude_cli_row(
                target=target,
                model=model,
                attempt_index=idx,
                reason="claude-cli-command-not-found",
                benchmark_mode=benchmark_mode,
                attempt_context=attempt_context(run_id=run_id, target=target, attempt_index=idx, benchmark_mode=benchmark_mode),
            )
            for idx in range(attempts)
        ]

    rows: list[dict[str, Any]] = []
    for idx in range(attempts):
        ctx = attempt_context(run_id=run_id, target=target, attempt_index=idx, benchmark_mode=benchmark_mode)
        prompt = model_proof_term_prompt(benchmark_mode=benchmark_mode, attempt_context=ctx)
        started = time.time()
        try:
            proc = subprocess.run(
                [resolved_command, "-p", prompt, "--model", model],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=timeout_s,
                check=False,
            )
        except subprocess.TimeoutExpired as err:
            elapsed_ms = int((time.time() - started) * 1000)
            rows.append(
                {
                    **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=ctx),
                    "name": f"{target} attempt {idx + 1}",
                    "kind": "provider-model",
                    "target": target,
                    "provider": "claude-cli",
                    "model": model,
                    "attemptIndex": idx,
                    "ok": True,
                    "skipped": False,
                    "status": "REJECTED",
                    "reason": "claude-cli-timeout",
                    "generatedAttempt": False,
                    "accepted": False,
                    "invalidAccepted": False,
                    "elapsedMs": elapsed_ms,
                    "latencyMs": elapsed_ms,
                    "score": zero_score(),
                    "diagnostics": zero_diagnostics(),
                    "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
                    "verifier": {"invoked": False, "command": "submit-lean"},
                    "stderrTail": str(err)[-1200:],
                    "stdoutTail": "",
                }
            )
            if on_row:
                on_row(rows[-1], rows)
            continue
        elapsed_ms = int((time.time() - started) * 1000)
        if proc.returncode != 0:
            rows.append(
                setup_required_claude_cli_row(
                    target=target,
                    model=model,
                    attempt_index=idx,
                    reason="claude-cli-generation-failed",
                    elapsed_ms=elapsed_ms,
                    stderr=proc.stderr,
                    stdout=proc.stdout,
                    benchmark_mode=benchmark_mode,
                    attempt_context=ctx,
                )
            )
            if on_row:
                on_row(rows[-1], rows)
            continue

        raw_candidate = proc.stdout.strip()
        proof_term, extraction, extraction_reason = extract_proof_term_candidate(raw_candidate)
        if extraction_reason:
            rows.append(
                rejected_candidate_shape_row(
                    target=target,
                    provider="claude-cli",
                    model=model,
                    attempt_index=idx,
                    reason=extraction_reason,
                    elapsed_ms=elapsed_ms,
                    raw_output=raw_candidate,
                    extraction=extraction,
                    stderr=proc.stderr,
                    benchmark_mode=benchmark_mode,
                    attempt_context=ctx,
                )
            )
            if on_row:
                on_row(rows[-1], rows)
            continue

        candidate = wrap_proof_term_candidate(proof_term or "", benchmark_mode=benchmark_mode, attempt_context=ctx)
        digest = hashlib.sha256(candidate.encode("utf-8")).hexdigest()
        verifier = None
        if submit_lean_command:
            verifier = submit_candidate_to_verifier(
                candidate=candidate,
                target=target,
                model=model,
                attempt_index=idx,
                submit_lean_command=submit_lean_command,
                candidate_root=candidate_root or (ROOT / "artifacts" / "model-benchmarks" / "candidates"),
                timeout_s=timeout_s,
                benchmark_mode=benchmark_mode,
                attempt_context=ctx,
            )
        accepted = bool((verifier or {}).get("accepted"))
        score = (verifier or {}).get("score") or zero_score()
        diagnostics = (verifier or {}).get("diagnostics") or zero_diagnostics()
        safety = (verifier or {}).get("safety") or {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}
        mining_path = (verifier or {}).get("miningPath") or mining_path_status(
            target_issued=True,
            model_generated=True,
            candidate_wrapped=True,
            submit_lean_invoked=False,
            verifier_accepted=False,
            share_accepted=False,
            block_produced=False,
            replay_passed=True,
        )
        rows.append(
            {
                **row_target_metadata(benchmark_mode=benchmark_mode, attempt_context=ctx),
                "name": f"{target} attempt {idx + 1}",
                "kind": "provider-model",
                "target": target,
                "provider": "claude-cli",
                "model": model,
                "attemptIndex": idx,
                "ok": True,
                "skipped": False,
                "status": "ACCEPTED" if accepted else "REJECTED",
                "reason": None if accepted else ((verifier or {}).get("reason") or ("verifier_rejected" if verifier else "verifier-integration-pending")),
                "generatedAttempt": True,
                "candidateMode": "proof-term",
                "candidateExtraction": extraction,
                "candidateSha256": digest,
                "candidateTermSha256": hashlib.sha256((proof_term or "").encode("utf-8")).hexdigest(),
                "candidatePreview": candidate_preview(proof_term or ""),
                "accepted": accepted,
                "invalidAccepted": bool(safety.get("invalidAccepted", 0)),
                "elapsedMs": elapsed_ms,
                "latencyMs": elapsed_ms,
                "score": score,
                "diagnostics": diagnostics,
                "safety": safety,
                "miningPath": mining_path,
                "verifier": verifier or {"invoked": False, "command": "submit-lean"},
                "stderrTail": ((verifier or {}).get("stderrTail") or proc.stderr)[-1200:],
                "stdoutTail": ((verifier or {}).get("stdoutTail") or proc.stdout)[-1200:],
            }
        )
        if on_row:
            on_row(rows[-1], rows)
    return rows


def leaderboard_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return sorted(
        rows,
        key=lambda row: (
            0 if row.get("skipped") else 1,
            1 if row.get("ok") else 0,
            int(row.get("score", {}).get("blocksProduced", 0)),
            -int(row.get("elapsedMs", 0)),
        ),
        reverse=True,
    )


def render_leaderboard(summary: dict[str, Any], rows: list[dict[str, Any]]) -> str:
    lines = [
        "# Boole Model Proof-to-Block Benchmark",
        "",
        "Local model-generated proof attempts are evaluated by Boole's verifier path and recorded as accepted/rejected/setup-required benchmark rows. They are not live mining claims.",
        "",
        f"- runId: `{summary['runId']}`",
        f"- ok: `{str(summary['ok']).lower()}`",
        f"- blockProductionRate: `{summary['totals']['blocksProduced']}/{summary['totals']['generatedAttempts']} ({summary['totals']['blockProductionRatePct']:.2f}%)`",
        f"- blocksProduced: `{summary['totals']['blocksProduced']}`",
        f"- generatedAttempts: `{summary['totals']['generatedAttempts']}`",
        f"- replayPassed: `{str(summary['replayPassed']).lower()}`",
        f"- invalidAccepted: `{summary['safety']['invalidAccepted']}`",
        "",
        "## Leaderboard",
        "",
    ]
    for idx, row in enumerate(rows, start=1):
        score = row.get("score", {})
        lines.extend(
            [
                f"### {idx}. {row['name']}",
                f"- status: `{row.get('status')}`",
                f"- provider: `{row.get('provider') or row.get('metadata', {}).get('provider', '')}`",
                f"- model: `{row.get('model') or row.get('metadata', {}).get('model', '')}`",
                f"- generatedAttempt: `{str(row.get('generatedAttempt') is True).lower()}`",
                f"- invalidAccepted: `{str(row.get('invalidAccepted') is True).lower()}`",
                f"- blocksProduced: `{score.get('blocksProduced', 0)}`",
                f"- blockProduced: `{str(int(score.get('blocksProduced', 0)) > 0).lower()}`",
                f"- replayPass: `{str(score.get('replayPass') is True).lower()}`",
                f"- elapsedMs: `{row.get('elapsedMs', 0)}`",
                "",
            ]
        )
    return "\n".join(lines)


def summarize(rows: list[dict[str, Any]], run_id: str, generated_at_ms: int) -> dict[str, Any]:
    safety = {
        "invalidAccepted": sum(int(row.get("safety", {}).get("invalidAccepted", 0)) for row in rows),
        "chainDivergence": sum(int(row.get("safety", {}).get("chainDivergence", 0)) for row in rows),
        "replayFailures": sum(int(row.get("safety", {}).get("replayFailures", 0)) for row in rows),
    }
    active_rows = [row for row in rows if not row.get("skipped")]
    generated_attempts = sum(1 for row in rows if row.get("generatedAttempt") is True)
    verifier_accepted = sum(1 for row in rows if row.get("miningPath", {}).get("verifierAccepted") is True or row.get("accepted") is True)
    verified_shares = sum(int(row.get("diagnostics", {}).get("verifiedShares", 0)) for row in rows)
    blocks_produced = sum(int(row.get("score", {}).get("blocksProduced", 0)) for row in rows)
    replay_passed = bool(active_rows) and all(row.get("score", {}).get("replayPass") is True for row in active_rows)
    ok = all(row.get("ok") is True for row in rows) and safety == {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}
    benchmark_modes = {row.get("benchmarkMode") for row in rows if row.get("benchmarkMode")}
    target_families = {row.get("targetFamily") for row in rows if row.get("targetFamily")}
    benchmark_mode = next(iter(benchmark_modes)) if len(benchmark_modes) == 1 else ("mixed" if benchmark_modes else "unknown")
    target_family = next(iter(target_families)) if len(target_families) == 1 else ("mixed" if target_families else "unknown")
    candidate_hashes = {row.get("candidateSha256") for row in rows if row.get("candidateSha256")}
    share_hashes = {
        (row.get("verifier", {}) or {}).get("result", {}).get("shareHash") or (row.get("verifier", {}) or {}).get("shareHash")
        for row in rows
        if ((row.get("verifier", {}) or {}).get("result", {}).get("shareHash") or (row.get("verifier", {}) or {}).get("shareHash"))
    }
    unique_shares = len(share_hashes)
    return {
        "ok": ok,
        "benchmark": "boole-model-proof-to-block",
        "version": 0,
        "benchmarkMode": benchmark_mode,
        "targetFamily": target_family,
        "runId": run_id,
        "generatedAtUnixMs": generated_at_ms,
        "totals": {
            "rows": len(rows),
            "passed": sum(1 for row in rows if row.get("status") == "PASS"),
            "skipped": sum(1 for row in rows if row.get("status") == "SKIP"),
            "setupRequired": sum(1 for row in rows if row.get("status") == "SETUP_REQUIRED"),
            "failed": sum(1 for row in rows if row.get("status") == "FAIL"),
            "rejected": sum(1 for row in rows if row.get("status") == "REJECTED"),
            "generatedAttempts": generated_attempts,
            "blocksProduced": blocks_produced,
            "blockProductionRatePct": block_production_rate_pct(blocks_produced=blocks_produced, generated_attempts=generated_attempts),
        },
        "publicScore": {
            "primaryMetric": "blockProductionRatePct",
            "formula": "blocksProduced / generatedAttempts * 100",
            "blocksProduced": blocks_produced,
            "generatedAttempts": generated_attempts,
            "blockProductionRatePct": block_production_rate_pct(blocks_produced=blocks_produced, generated_attempts=generated_attempts),
        },
        "attemptHierarchy": {
            "generatedAttempts": generated_attempts,
            "verifierAccepted": verifier_accepted,
            "verifiedShares": verified_shares,
            "blocksProduced": blocks_produced,
        },
        "diagnostics": {
            "accepted": sum(1 for row in rows if row.get("accepted") is True),
            "verifiedShares": verified_shares,
            "uniqueCandidates": len(candidate_hashes),
            "uniqueShares": unique_shares,
            "uniqueShareRatePct": round(unique_shares / generated_attempts * 100, 2) if generated_attempts else 0.0,
        },
        "safety": safety,
        "replayPassed": replay_passed,
        "artifacts": {
            "summary": "benchmark-summary.json",
            "rows": "benchmark-rows.ndjson",
            "leaderboard": "leaderboard.md",
            "replayReport": "replay-report.json",
            "progress": "progress.json",
        },
    }


def write_progress(output_dir: Path, *, run_id: str, generated_at_ms: int, rows: list[dict[str, Any]], total_attempts: int | None = None) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    summary = summarize(rows, run_id, generated_at_ms)
    progress = {
        "runId": run_id,
        "generatedAtUnixMs": generated_at_ms,
        "completedAttempts": len(rows),
        "totalAttempts": total_attempts,
        "okSoFar": summary["ok"],
        "totals": summary["totals"],
        "safety": summary["safety"],
        "replayPassed": summary["replayPassed"],
        "artifacts": {
            "progress": "progress.json",
            "rows": "benchmark-rows.ndjson",
            "summary": "benchmark-summary.json",
            "leaderboard": "leaderboard.md",
            "replayReport": "replay-report.json",
        },
    }
    (output_dir / "progress.json").write_text(json.dumps(progress, indent=2) + "\n", encoding="utf-8")


def append_row_checkpoint(output_dir: Path, row: dict[str, Any], *, run_id: str, generated_at_ms: int, rows: list[dict[str, Any]], total_attempts: int | None = None) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    with (output_dir / "benchmark-rows.ndjson").open("a", encoding="utf-8") as f:
        f.write(json.dumps(row, separators=(",", ":")) + "\n")
    write_progress(output_dir, run_id=run_id, generated_at_ms=generated_at_ms, rows=rows, total_attempts=total_attempts)


def write_artifacts(output_dir: Path, summary: dict[str, Any], rows: list[dict[str, Any]]) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    ordered = leaderboard_rows(rows)
    (output_dir / "benchmark-summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    with (output_dir / "benchmark-rows.ndjson").open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":")) + "\n")
    replay_report = {
        "runId": summary["runId"],
        "replayPassed": summary["replayPassed"],
        "replayFailures": summary["safety"]["replayFailures"],
        "chainDivergence": summary["safety"]["chainDivergence"],
        "rows": [
            {"name": row["name"], "status": row.get("status"), "replayPass": row.get("score", {}).get("replayPass")}
            for row in rows
        ],
    }
    (output_dir / "replay-report.json").write_text(json.dumps(replay_report, indent=2) + "\n", encoding="utf-8")
    (output_dir / "leaderboard.md").write_text(render_leaderboard(summary, ordered), encoding="utf-8")


def run_benchmark(*, spec_path: Path | None = None, output_dir: Path, run_id: str | None = None, timeout_s: int | None = 300, target: str | None = None, attempts: int = 1, ollama_command: str = "ollama", claude_command: str = "claude", submit_lean_command: str | None = None, benchmark_mode: str = "mining") -> dict[str, Any]:
    run_id = run_id or default_run_id()
    generated_at_ms = now_ms()
    if target:
        if not (target.startswith("ollama:") or target.startswith("claude-cli:")):
            raise SystemExit(f"unsupported benchmark target: {target}")
        output_dir.mkdir(parents=True, exist_ok=True)
        (output_dir / "benchmark-rows.ndjson").write_text("", encoding="utf-8")
        write_progress(output_dir, run_id=run_id, generated_at_ms=generated_at_ms, rows=[], total_attempts=attempts)

        def checkpoint(row: dict[str, Any], current_rows: list[dict[str, Any]]) -> None:
            append_row_checkpoint(
                output_dir,
                row,
                run_id=run_id,
                generated_at_ms=generated_at_ms,
                rows=current_rows,
                total_attempts=attempts,
            )

        if target.startswith("ollama:"):
            rows = run_ollama_attempts(
                target=target,
                ollama_command=ollama_command,
                attempts=attempts,
                timeout_s=timeout_s,
                benchmark_mode=benchmark_mode,
                submit_lean_command=submit_lean_command,
                candidate_root=output_dir / "candidates",
                on_row=checkpoint,
                run_id=run_id,
            )
        else:
            rows = run_claude_cli_attempts(
                target=target,
                claude_command=claude_command,
                attempts=attempts,
                timeout_s=timeout_s,
                benchmark_mode=benchmark_mode,
                submit_lean_command=submit_lean_command,
                candidate_root=output_dir / "candidates",
                on_row=checkpoint,
                run_id=run_id,
            )
    else:
        if spec_path is None:
            raise SystemExit("--spec is required unless --target is provided")
        rows = [run_row(row, timeout_s=timeout_s) for row in load_spec(spec_path)]
    summary = summarize(rows, run_id, generated_at_ms)
    write_progress(output_dir, run_id=run_id, generated_at_ms=generated_at_ms, rows=rows, total_attempts=attempts if target else len(rows))
    write_artifacts(output_dir, summary, rows)
    return {"ok": summary["ok"], "runId": run_id, "artifactDir": str(output_dir), "summary": summary}


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description="Run Boole model Proof-to-Block benchmark rows and write artifacts.")
    parser.add_argument("--spec", help="Benchmark row spec JSON array path.")
    parser.add_argument("--target", help="Single model target, supports ollama:<model> and claude-cli:<model>.")
    parser.add_argument("--attempts", type=int, default=1, help="Attempts for single --target runs.")
    parser.add_argument("--ollama-command", default=os.environ.get("BOOLE_OLLAMA_COMMAND", "ollama"), help="Ollama command path/name. Defaults to BOOLE_OLLAMA_COMMAND or ollama.")
    parser.add_argument("--claude-command", default=os.environ.get("BOOLE_CLAUDE_COMMAND", "claude"), help="Claude CLI command path/name. Defaults to BOOLE_CLAUDE_COMMAND or claude.")
    parser.add_argument("--submit-lean-command", default=os.environ.get("BOOLE_SUBMIT_LEAN_COMMAND"), help="Optional submit-lean command path/name for verifier-backed generated attempts.")
    parser.add_argument("--output-dir", help="Artifact output directory. Defaults to artifacts/model-benchmarks/<run-id>.")
    parser.add_argument("--run-id", help="Stable run id for reproducible tests/evidence.")
    parser.add_argument("--timeout-sec", type=int, default=300, help="Per-attempt timeout seconds. Use 0 to disable subprocess timeouts.")
    parser.add_argument("--benchmark-mode", choices=["mining", "smoke"], default="mining", help="Benchmark target mode. Default mining uses per-attempt calibrated target family; smoke is True.intro pipeline-only.")
    args = parser.parse_args(argv)

    if bool(args.spec) == bool(args.target):
        raise SystemExit("provide exactly one of --spec or --target")
    if args.attempts < 1:
        raise SystemExit("--attempts must be >= 1")
    if args.timeout_sec < 0:
        raise SystemExit("--timeout-sec must be >= 0")

    run_id = args.run_id or default_run_id()
    output_dir = Path(args.output_dir) if args.output_dir else ROOT / "artifacts" / "model-benchmarks" / run_id
    result = run_benchmark(
        spec_path=Path(args.spec) if args.spec else None,
        output_dir=output_dir,
        run_id=run_id,
        timeout_s=None if args.timeout_sec == 0 else args.timeout_sec,
        target=args.target,
        attempts=args.attempts,
        ollama_command=args.ollama_command,
        claude_command=args.claude_command,
        submit_lean_command=args.submit_lean_command,
        benchmark_mode=args.benchmark_mode,
    )
    print(json.dumps(result, separators=(",", ":")))
    if not result["ok"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
