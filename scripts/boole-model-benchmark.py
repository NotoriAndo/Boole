#!/usr/bin/env python3
"""Create Boole model Proof-to-Block benchmark artifact bundles.

This is a non-consensus runner skeleton. It executes command-based benchmark
rows, records pass/skip/fail rows, and writes stable artifacts that later live
Ollama/frontier runners can fill with real proof-attempt data.
"""
from __future__ import annotations

import argparse
import hashlib
import http.client
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]

VERIFIER_HASHES_FIXTURE = ROOT / "fixtures" / "benchmarks" / "verifier-hashes.json"


def load_verifier_hashes(path: Path = VERIFIER_HASHES_FIXTURE) -> dict[str, Any]:
    """Read the version-keyed verifier-hash fixture.

    Shape: ``{"active": str, "versions": {str: str}}``. ``active`` must point
    at a key that exists in ``versions``. The fixture is the single source of
    truth for the benchmark's verifier hash; bumping ``active`` to a new
    version is how Slice S5 hands out a new hash to fresh runs without
    invalidating historical rows (which carry their own ``verifierHashVersion``).
    """
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or "active" not in data or "versions" not in data:
        raise ValueError(f"verifier hashes fixture {path} missing 'active' or 'versions'")
    versions = data["versions"]
    if not isinstance(versions, dict) or not versions:
        raise ValueError(f"verifier hashes fixture {path} 'versions' must be a non-empty map")
    active = data["active"]
    if active not in versions:
        raise ValueError(
            f"verifier hashes fixture {path} 'active' = {active!r} not present in versions {sorted(versions)}"
        )
    return {"active": active, "versions": dict(versions)}


def resolve_verifier_hash(
    *, version: str | None = None, hashes: dict[str, Any] | None = None
) -> tuple[str, str]:
    """Resolve a (version, hash) pair from the fixture.

    With ``version=None`` returns ``(active, versions[active])`` — the new-run
    path. With an explicit ``version`` returns ``(version, versions[version])``
    — the historical-replay path that pins to whatever version the row was
    recorded with, ignoring the current ``active``. Unknown versions raise
    ``KeyError`` with the typed message ``"unknown verifier hash version: <v>"``.
    """
    table = hashes if hashes is not None else load_verifier_hashes()
    versions = table["versions"]
    if version is None:
        version = table["active"]
    if version not in versions:
        raise KeyError(f"unknown verifier hash version: {version}")
    return version, versions[version]


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


def replay_invoked_from_result(parsed: dict[str, Any] | None) -> bool:
    """B4 row-level signal: did this row actually exercise the replay path?

    Distinguishes "replay verified the chain" from "replay was never run
    because no attempt reached the verifier." A row counts as having
    invoked replay iff the parsed JSON output contains a
    `replayMatchesRuntime` key (in any of the conventional locations) —
    presence of the key means the runtime ran the comparison; the value
    (true/false) feeds `replayPass`.
    """
    if parsed is None:
        return False
    status = parsed.get("status") if isinstance(parsed.get("status"), dict) else {}
    safety = parsed.get("safety") if isinstance(parsed.get("safety"), dict) else {}
    return (
        "replayMatchesRuntime" in (status or {})
        or "replayMatchesRuntime" in parsed
        or "replayMatchesRuntime" in (safety or {})
    )


def score_from_result(parsed: dict[str, Any] | None) -> dict[str, Any]:
    return {
        "blocksProduced": blocks_produced_from_result(parsed),
        "replayPass": replay_pass_from_result(parsed),
        "replayInvoked": replay_invoked_from_result(parsed),
    }


def diagnostics_from_result(parsed: dict[str, Any] | None) -> dict[str, Any]:
    return {"verifiedShares": verified_shares_from_result(parsed)}


def zero_score(*, replay_pass: bool = True, replay_invoked: bool = False) -> dict[str, Any]:
    """Default score for rows that did not produce a block.

    `replay_invoked` defaults to False because rejected, skipped,
    setup-required and timed-out rows never reach the verifier — the
    replay comparison was not performed, so we must not report it as
    "passed" in the summary aggregation.
    """
    return {
        "blocksProduced": 0,
        "replayPass": replay_pass,
        "replayInvoked": replay_invoked,
    }


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


ANSI_ESCAPE_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
THINK_BLOCK_RE = re.compile(r"<think\b[^>]*>[\s\S]*?</think>", re.IGNORECASE)


def normalize_model_output(raw_output: str) -> tuple[str, list[str]]:
    normalizations: list[str] = []
    normalized = ANSI_ESCAPE_RE.sub("", raw_output)
    if normalized != raw_output:
        normalizations.append("strip-ansi")
    stripped, count = THINK_BLOCK_RE.subn("", normalized)
    if count > 0:
        normalized = stripped
        normalizations.append("strip-think")
    normalized = normalized.strip()
    return normalized, normalizations


def last_proof_term_line(raw: str) -> str | None:
    for line in reversed(raw.splitlines()):
        candidate = line.strip().strip("`")
        if not candidate:
            continue
        lowered = candidate.lower()
        if lowered.startswith("thinking") or "do not include" in lowered:
            continue
        if re.search(r"\b(sorry|admit)\b", lowered):
            continue
        if "```" in candidate:
            continue
        if re.search(r"^\s*(import|open|namespace|section|def|theorem|lemma|example)\b", candidate):
            continue
        if re.search(r"^\s*by\b", candidate):
            continue
        if re.search(r"[.!?]$", candidate):
            continue
        if not re.search(r"[A-Za-z0-9_\.(){}\[\]:'\"=><,+\-*/\s]+\Z", candidate):
            continue
        return candidate
    return None


def extract_proof_term_candidate(raw_output: str) -> tuple[str | None, dict[str, Any], str | None]:
    raw, normalizations = normalize_model_output(raw_output)
    extraction: dict[str, Any] = {"mode": "proof-term", "format": "raw", "normalization": "+".join(normalizations) if normalizations else "none"}
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

    if "\n" in raw:
        final_line = last_proof_term_line(raw)
        if final_line is not None and final_line != raw:
            normalization = extraction.get("normalization", "none")
            extraction.update({
                "format": "final-proof-line",
                "normalization": "last-proof-line" if normalization == "none" else f"{normalization}+last-proof-line",
            })
            raw = final_line

    if re.search(r"\b(sorry|admit)\b", raw.lower()):
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


def node_head_url(node_url: str) -> str:
    return node_url.rstrip("/") + "/head"


def http_timeout(timeout_s: int | None) -> int | None:
    return timeout_s if timeout_s and timeout_s > 0 else None


def fetch_node_head(*, node_url: str, timeout_s: int | None) -> dict[str, Any]:
    started = time.time()
    request = urllib.request.Request(node_head_url(node_url), method="GET")
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            response_body = response.read().decode("utf-8")
            result = json.loads(response_body) if response_body.strip() else {}
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, http.client.RemoteDisconnected) as err:
        return {
            "invoked": True,
            "endpoint": node_head_url(node_url),
            "elapsedMs": int((time.time() - started) * 1000),
            "reason": "node_http_head_failed",
            "error": str(err)[-500:],
            "c": None,
        }
    c = result.get("c")
    return {
        "invoked": True,
        "endpoint": node_head_url(node_url),
        "elapsedMs": int((time.time() - started) * 1000),
        "c": c if isinstance(c, str) else None,
        "result": result,
    }


def node_submit_url(node_url: str) -> str:
    return node_url.rstrip("/") + "/submit"


def node_account_balance_url(node_url: str, pk: str) -> str:
    return node_url.rstrip("/") + f"/account/{pk}/balance"


def node_bounty_proof_url(node_url: str, bounty_id: str) -> str:
    return node_url.rstrip("/") + f"/bounties/{bounty_id}/proof"


def derive_bounty_proof_hash(*, candidate: str, run_id: str, attempt_index: int) -> str:
    """Per-attempt proofHash derivation. The bounty registry dedups on
    proofHash, so identical candidate text across attempts (e.g., smoke-mode
    `True.intro`) must still yield distinct hashes — `attempt_index` is the
    salt that disambiguates."""
    digest = hashlib.sha256()
    digest.update(b"benchmark-bounty\x00")
    digest.update(run_id.encode("utf-8"))
    digest.update(b"\x00")
    digest.update(attempt_index.to_bytes(8, "big"))
    digest.update(b"\x00")
    digest.update(candidate.encode("utf-8"))
    return digest.hexdigest()


def post_bounty_proof(*, node_url: str, bounty_id: str, proof_hash: str, prover: str, envelope: Any, timeout_s: int | None) -> dict[str, Any]:
    """POST <node>/bounties/{id}/proof — captures bounty acceptance, family
    id (= bounty.domain), and reward credit. The benchmark tags `bountyAccepted`,
    `bountyFamilyId`, `bountyCreditEarned` on the row from the response."""
    body = {"proofHash": proof_hash, "prover": prover, "envelope": envelope}
    payload = json.dumps(body, separators=(",", ":")).encode("utf-8")
    started = time.time()
    endpoint = node_bounty_proof_url(node_url, bounty_id)
    request = urllib.request.Request(
        endpoint,
        data=payload,
        headers={"content-type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            response_body = response.read().decode("utf-8")
            result = json.loads(response_body) if response_body.strip() else {}
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, http.client.RemoteDisconnected) as err:
        return {
            "invoked": True,
            "endpoint": endpoint,
            "elapsedMs": int((time.time() - started) * 1000),
            "reason": "node_http_bounty_proof_failed",
            "error": str(err)[-500:],
            "accepted": False,
            "result": None,
        }
    bounty = result.get("bounty") if isinstance(result.get("bounty"), dict) else {}
    accepted = bool(result.get("accepted"))
    return {
        "invoked": True,
        "endpoint": endpoint,
        "elapsedMs": int((time.time() - started) * 1000),
        "accepted": accepted,
        "duplicate": bool(result.get("duplicate")),
        "bountyId": bounty_id,
        "familyId": bounty.get("domain") if isinstance(bounty.get("domain"), str) else None,
        "reward": bounty.get("reward") if isinstance(bounty.get("reward"), str) else None,
        "result": result,
    }


def node_block_by_height_url(node_url: str, height: int) -> str:
    return node_url.rstrip("/") + f"/block/{height}"


def fetch_block_by_height(*, node_url: str, height: int, timeout_s: int | None) -> dict[str, Any]:
    """GET <node>/block/<height> — used by --measure-reward to learn who
    proposed the block this attempt landed in. The runner needs `proposerPk`
    to surface `wasProposer` (= proposerPk == prover_pk) and the matching
    chain-rule `proposerBonusEarned` decimal string on the row."""
    started = time.time()
    endpoint = node_block_by_height_url(node_url, height)
    request = urllib.request.Request(endpoint, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            response_body = response.read().decode("utf-8")
            result = json.loads(response_body) if response_body.strip() else {}
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, http.client.RemoteDisconnected) as err:
        return {
            "invoked": True,
            "endpoint": endpoint,
            "elapsedMs": int((time.time() - started) * 1000),
            "reason": "node_http_block_failed",
            "error": str(err)[-500:],
            "block": None,
        }
    block = result.get("block") if isinstance(result.get("block"), dict) else None
    return {
        "invoked": True,
        "endpoint": endpoint,
        "elapsedMs": int((time.time() - started) * 1000),
        "block": block,
        "result": result,
    }


def fetch_account_balance(*, node_url: str, pk: str, timeout_s: int | None) -> dict[str, Any]:
    """GET <node>/account/<pk>/balance — used by --measure-reward to capture
    pre/post share-reward deltas around each /submit. The node returns
    `balance` as a u128 decimal string; this helper preserves that shape so
    arithmetic happens in Python int space (no float rounding)."""
    started = time.time()
    endpoint = node_account_balance_url(node_url, pk)
    request = urllib.request.Request(endpoint, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            response_body = response.read().decode("utf-8")
            result = json.loads(response_body) if response_body.strip() else {}
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, http.client.RemoteDisconnected) as err:
        return {
            "invoked": True,
            "endpoint": endpoint,
            "elapsedMs": int((time.time() - started) * 1000),
            "reason": "node_http_balance_failed",
            "error": str(err)[-500:],
            "balance": None,
        }
    raw = result.get("balance")
    balance = str(raw) if raw is not None else None
    return {
        "invoked": True,
        "endpoint": endpoint,
        "elapsedMs": int((time.time() - started) * 1000),
        "balance": balance,
        "asOfHeight": result.get("asOfHeight"),
        "asOfC": result.get("asOfC"),
    }


def _probe_route(*, node_url: str, route: str, timeout_s: int | None) -> dict[str, Any]:
    """Single GET probe for `--preflight-node`. Records HTTP status, ok-ness,
    and connection errors as a typed dict so the wrapper bash script can
    pretty-print or fail fast based on which route failed."""
    started = time.time()
    endpoint = node_url.rstrip("/") + route
    request = urllib.request.Request(endpoint, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            status = int(response.status)
            elapsed_ms = int((time.time() - started) * 1000)
            return {
                "route": route,
                "method": "GET",
                "endpoint": endpoint,
                "status": status,
                "ok": status == 200,
                "elapsedMs": elapsed_ms,
            }
    except urllib.error.HTTPError as err:
        return {
            "route": route,
            "method": "GET",
            "endpoint": endpoint,
            "status": int(err.code),
            "ok": False,
            "elapsedMs": int((time.time() - started) * 1000),
            "error": str(err)[-500:],
        }
    except (urllib.error.URLError, TimeoutError, http.client.RemoteDisconnected) as err:
        return {
            "route": route,
            "method": "GET",
            "endpoint": endpoint,
            "status": 0,
            "ok": False,
            "elapsedMs": int((time.time() - started) * 1000),
            "error": str(err)[-500:],
        }


def preflight_node_economic_routes(*, node_url: str, prover_pk: str | None, bounty_id: str | None, timeout_s: int | None = 5) -> dict[str, Any]:
    """S24e — health-check the three routes the benchmark depends on for
    economic-signal capture. `/head` is always probed; `/account/<pk>/balance`
    only when prover_pk is set; `/bounties/<id>` only when bounty_id is set.
    Returns `{ok, probes: [...]}` where `ok` is True iff every probe returned
    HTTP 200. The bash wrapper consumes this JSON to short-circuit before
    spawning a long benchmark run against a misconfigured node."""
    routes: list[str] = ["/head"]
    if prover_pk:
        routes.append(f"/account/{prover_pk}/balance")
    if bounty_id:
        routes.append(f"/bounties/{bounty_id}")
    probes = [_probe_route(node_url=node_url, route=route, timeout_s=timeout_s) for route in routes]
    return {
        "ok": all(p["ok"] for p in probes),
        "node": node_url,
        "probes": probes,
    }


def node_ticket_url(node_url: str) -> str:
    return node_url.rstrip("/") + "/ticket"


def post_ticket_to_node(*, node_url: str, submission_body: dict[str, Any], timeout_s: int | None) -> dict[str, Any]:
    """Observe the canonical payload through the local node /ticket endpoint before /submit.

    The node enforces the pof TicketBody contract: exactly {c, pk, n}, with no payload
    wrapper and no submit-shaped extras (j, nonceS, bytes, ...). Sending a wider body
    will return HTTP 400 unexpected_field, which would silently break every live
    --use-node-ticket benchmark run. Project just the three required fields here.
    """
    missing = [field for field in ("c", "pk", "n") if not submission_body.get(field)]
    if missing:
        return {
            "invoked": True,
            "endpoint": node_ticket_url(node_url),
            "elapsedMs": 0,
            "reason": "missing_ticket_field",
            "missing": missing,
            "result": None,
        }
    ticket_body = {
        "c": submission_body["c"],
        "pk": submission_body["pk"],
        "n": submission_body["n"],
    }
    payload = json.dumps(ticket_body, separators=(",", ":")).encode("utf-8")
    request = urllib.request.Request(
        node_ticket_url(node_url),
        data=payload,
        headers={"content-type": "application/json"},
        method="POST",
    )
    started = time.time()
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            response_body = response.read().decode("utf-8")
            result = json.loads(response_body) if response_body.strip() else {}
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, http.client.RemoteDisconnected) as err:
        return {
            "invoked": True,
            "endpoint": node_ticket_url(node_url),
            "elapsedMs": int((time.time() - started) * 1000),
            "reason": "node_http_ticket_failed",
            "error": str(err)[-500:],
            "result": None,
        }
    return {
        "invoked": True,
        "endpoint": node_ticket_url(node_url),
        "elapsedMs": int((time.time() - started) * 1000),
        "accepted": bool(result.get("accepted") or result.get("valid") or result.get("meetsTicketTarget")),
        "result": result,
    }


def post_submission_to_node(*, node_url: str, parsed: dict[str, Any], timeout_s: int | None, use_node_ticket: bool = False) -> dict[str, Any]:
    """Submit the exact submit-lean canonical body to a local node HTTP endpoint."""
    body = parsed.get("submissionBody")
    canon_tag = parsed.get("canonTag")
    if not isinstance(body, dict) or canon_tag is None:
        return {
            "invoked": False,
            "url": node_url,
            "reason": "missing_submission_body",
            "accepted": False,
            "shareAccepted": False,
            "blockProduced": False,
            "result": None,
        }
    ticket_http = post_ticket_to_node(node_url=node_url, submission_body=body, timeout_s=timeout_s) if use_node_ticket else {"invoked": False}
    payload = json.dumps({"body": body, "canonTag": canon_tag}, separators=(",", ":")).encode("utf-8")
    request = urllib.request.Request(
        node_submit_url(node_url),
        data=payload,
        headers={"content-type": "application/json"},
        method="POST",
    )
    started = time.time()
    try:
        with urllib.request.urlopen(request, timeout=http_timeout(timeout_s)) as response:  # noqa: S310 - caller-provided local testnet URL
            response_body = response.read().decode("utf-8")
            result = json.loads(response_body) if response_body.strip() else {}
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, http.client.RemoteDisconnected) as err:
        return {
            "invoked": True,
            "url": node_url,
            "endpoint": node_submit_url(node_url),
            "elapsedMs": int((time.time() - started) * 1000),
            "reason": "node_http_submit_failed",
            "error": str(err)[-500:],
            "accepted": False,
            "shareAccepted": False,
            "blockProduced": False,
            "ticketInvoked": bool(ticket_http.get("invoked")),
            "ticket": ticket_http.get("result"),
            "ticketHttp": ticket_http,
            "result": None,
        }
    return {
        "invoked": True,
        "url": node_url,
        "endpoint": node_submit_url(node_url),
        "elapsedMs": int((time.time() - started) * 1000),
        "accepted": bool(result.get("accepted")),
        "shareAccepted": bool(result.get("shareAccepted") or result.get("shareHash")),
        "blockProduced": bool(result.get("blockProduced") or result.get("block")),
        "replayMatchesRuntime": bool(result.get("replayMatchesRuntime")),
        "invalidAccepted": int(result.get("invalidAccepted") or 0),
        "ticketInvoked": bool(ticket_http.get("invoked")),
        "ticket": ticket_http.get("result"),
        "ticketHttp": ticket_http,
        "result": result,
    }


def submit_candidate_to_verifier(*, candidate: str, target: str, model: str, attempt_index: int, submit_lean_command: str, candidate_root: Path, timeout_s: int | None, benchmark_mode: str = "mining", attempt_context: dict[str, Any] | None = None, node_url: str | None = None, use_node_ticket: bool = False, measure_reward: bool = False, prover_pk: str | None = None, bounty_id: str | None = None, bounty_envelope: Any = None, run_id: str = "manual") -> dict[str, Any]:
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
    verifier_hash_version, verifier_hash = resolve_verifier_hash()
    started = time.time()
    node_head = fetch_node_head(node_url=node_url, timeout_s=timeout_s) if node_url else {"invoked": False}
    submit_args = [
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
    ]
    if node_head.get("c"):
        submit_args.extend(["--head-c", str(node_head["c"])])
    proc = subprocess.run(
        submit_args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout_s,
        check=False,
    )
    elapsed_ms = int((time.time() - started) * 1000)
    parsed = parse_submit_lean_output(proc) or {}
    balance_before: dict[str, Any] | None = None
    balance_after: dict[str, Any] | None = None
    if measure_reward and node_url and prover_pk:
        balance_before = fetch_account_balance(node_url=node_url, pk=prover_pk, timeout_s=timeout_s)
    node_http = post_submission_to_node(node_url=node_url, parsed=parsed, timeout_s=timeout_s, use_node_ticket=use_node_ticket) if node_url else None
    if measure_reward and node_url and prover_pk:
        balance_after = fetch_account_balance(node_url=node_url, pk=prover_pk, timeout_s=timeout_s)
    effective = ((node_http or {}).get("result") if node_url else parsed) or {}
    accepted = proc.returncode == 0 and bool(effective.get("accepted"))
    share_accepted = bool(effective.get("shareAccepted") or effective.get("shareHash"))
    replay_matches = bool(effective.get("replayMatchesRuntime"))
    invalid_accepted = int(effective.get("invalidAccepted") or 0)
    blocks = 1 if accepted and share_accepted and (effective.get("blockProduced") or effective.get("block")) else 0
    verified = 1 if accepted and share_accepted else 0
    before_str: str | None = None
    after_str: str | None = None
    delta_str: str | None = None
    if measure_reward:
        before_str = (balance_before or {}).get("balance") if balance_before else None
        after_str = (balance_after or {}).get("balance") if balance_after else None
        try:
            if before_str is not None and after_str is not None:
                delta_int = int(after_str) - int(before_str)
                if delta_int < 0:
                    delta_int = 0
                delta_str = str(delta_int)
        except (TypeError, ValueError):
            delta_str = None
    # S24b: when a block was produced, GET /block/{height} so the row knows
    # whether prover_pk proposed (and thus earned the +1 chain-rule proposer
    # bonus on top of any share credit). Skipped silently when the response
    # carries no `block.height` or when --measure-reward is off.
    block_info: dict[str, Any] | None = None
    proposer_pk: str | None = None
    selected_share_pks: list[str] | None = None
    was_proposer: bool | None = None
    proposer_bonus_earned: str | None = None
    block_obj = effective.get("block") if isinstance(effective.get("block"), dict) else None
    block_height: int | None = None
    if block_obj is not None:
        raw_height = block_obj.get("height")
        if isinstance(raw_height, int):
            block_height = raw_height
        elif isinstance(raw_height, str) and raw_height.isdigit():
            block_height = int(raw_height)
    if measure_reward and node_url and prover_pk and blocks > 0 and block_height is not None:
        block_info = fetch_block_by_height(node_url=node_url, height=block_height, timeout_s=timeout_s)
        chain_block = (block_info or {}).get("block") or {}
        proposer_pk = chain_block.get("proposerPk") if isinstance(chain_block.get("proposerPk"), str) else None
        raw_share_pks = chain_block.get("selectedSharePks")
        if isinstance(raw_share_pks, list):
            selected_share_pks = [s for s in raw_share_pks if isinstance(s, str)]
        was_proposer = proposer_pk == prover_pk if proposer_pk else False
        proposer_bonus_earned = "1" if was_proposer else "0"
    # S24c: bounty mode — POST the candidate's proof to /bounties/{id}/proof
    # so the row records the second economic stream (bounty credit, scored
    # per-family). Skipped silently when bounty_id is unset.
    bounty_http: dict[str, Any] | None = None
    bounty_accepted: bool | None = None
    bounty_family_id: str | None = None
    bounty_credit_earned: str | None = None
    bounty_proof_hash: str | None = None
    if bounty_id and node_url and prover_pk:
        bounty_proof_hash = derive_bounty_proof_hash(candidate=candidate, run_id=run_id, attempt_index=attempt_index)
        bounty_http = post_bounty_proof(
            node_url=node_url,
            bounty_id=bounty_id,
            proof_hash=bounty_proof_hash,
            prover=prover_pk,
            envelope=bounty_envelope,
            timeout_s=timeout_s,
        )
        bounty_accepted = bool(bounty_http.get("accepted"))
        bounty_family_id = bounty_http.get("familyId")
        reward_str = bounty_http.get("reward")
        if bounty_accepted:
            bounty_credit_earned = reward_str if isinstance(reward_str, str) else "0"
        else:
            bounty_credit_earned = "0"
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
        "verifierHashVersion": verifier_hash_version,
        "checkerArtifactHash": required_checker_hash,
        "proofSha256": hashlib.sha256(candidate.encode("utf-8")).hexdigest(),
        "result": parsed,
        "nodeHead": node_head,
        "nodeHttp": node_http or {"invoked": False, "url": node_url},
        "accountBalanceBefore": before_str,
        "accountBalanceAfter": after_str,
        "attemptShareReward": delta_str,
        "balancePollBefore": balance_before or {"invoked": False},
        "balancePollAfter": balance_after or {"invoked": False},
        "blockHeight": block_height,
        "proposerPk": proposer_pk,
        "selectedSharePks": selected_share_pks,
        "wasProposer": was_proposer,
        "proposerBonusEarned": proposer_bonus_earned,
        "blockLookup": block_info or {"invoked": False},
        "bountyId": bounty_id,
        "bountyProofHash": bounty_proof_hash,
        "bountyAccepted": bounty_accepted,
        "bountyFamilyId": bounty_family_id,
        "bountyCreditEarned": bounty_credit_earned,
        "bountyHttp": bounty_http or {"invoked": False},
        # The verifier was just invoked above, so this row exercised the
        # replay path — `replayInvoked: True` regardless of whether the
        # comparison passed. Summary aggregation uses this to distinguish
        # "replay verified" from "replay never ran" (the legacy vacuous-
        # pass bug captured by B4 in the parity plan).
        "score": {"blocksProduced": blocks, "replayPass": replay_matches, "replayInvoked": True},
        "diagnostics": {"verifiedShares": verified},
        "safety": {"invalidAccepted": invalid_accepted, "chainDivergence": 0, "replayFailures": 0 if replay_matches else 1},
        "stderrTail": proc.stderr[-1200:],
        "stdoutTail": proc.stdout[-1200:],
        "target": target,
        "model": model,
    }

def run_ollama_attempts(*, target: str, ollama_command: str, attempts: int, timeout_s: int | None, submit_lean_command: str | None = None, candidate_root: Path | None = None, on_row: Any | None = None, benchmark_mode: str = "mining", run_id: str = "manual", node_url: str | None = None, use_node_ticket: bool = False, deadline_monotonic: float | None = None, measure_reward: bool = False, prover_pk: str | None = None, bounty_id: str | None = None, bounty_envelope: Any = None) -> list[dict[str, Any]]:
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
        # B6: cooperative wall-clock cap — stop launching new
        # attempts past the deadline; in-flight attempts are not
        # killed.
        if deadline_monotonic is not None and time.monotonic() > deadline_monotonic:
            break
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
                node_url=node_url,
                use_node_ticket=use_node_ticket,
                measure_reward=measure_reward,
                prover_pk=prover_pk,
                bounty_id=bounty_id,
                bounty_envelope=bounty_envelope,
                run_id=run_id,
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


def run_claude_cli_attempts(*, target: str, claude_command: str, attempts: int, timeout_s: int | None, submit_lean_command: str | None = None, candidate_root: Path | None = None, on_row: Any | None = None, benchmark_mode: str = "mining", run_id: str = "manual", node_url: str | None = None, use_node_ticket: bool = False, deadline_monotonic: float | None = None, measure_reward: bool = False, prover_pk: str | None = None, bounty_id: str | None = None, bounty_envelope: Any = None) -> list[dict[str, Any]]:
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
        # B6: cooperative wall-clock cap — see run_ollama_attempts.
        if deadline_monotonic is not None and time.monotonic() > deadline_monotonic:
            break
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
                node_url=node_url,
                use_node_ticket=use_node_ticket,
                measure_reward=measure_reward,
                prover_pk=prover_pk,
                bounty_id=bounty_id,
                bounty_envelope=bounty_envelope,
                run_id=run_id,
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


def _latency_quantiles(samples: list[int]) -> dict[str, Any]:
    """Closed-form linear-interpolation quantiles (numpy "linear" /
    type-7). For an empty sample set, all p* fields are `None`. We
    avoid pulling in `statistics.quantiles` because it returns N-1
    cut-points rather than direct p50/p90/p99 lookups.

    Used by the B6 timeout-ergonomics surface so timeout-induced
    rejection patterns surface visibly instead of hiding behind a
    single `elapsedMs` per row.
    """
    n = len(samples)
    if n == 0:
        return {"p50Ms": None, "p90Ms": None, "p99Ms": None, "sampleCount": 0}
    xs = sorted(samples)

    def q(p: float) -> int:
        idx = p * (n - 1)
        lo = int(idx)
        hi = min(lo + 1, n - 1)
        frac = idx - lo
        return int(round(xs[lo] + (xs[hi] - xs[lo]) * frac))

    return {
        "p50Ms": q(0.50),
        "p90Ms": q(0.90),
        "p99Ms": q(0.99),
        "sampleCount": n,
    }


def render_leaderboard(summary: dict[str, Any], rows: list[dict[str, Any]]) -> str:
    # B4: when no row invoked replay, summary['replayPassed'] is None.
    # Render the em-dash so "no evidence" reads as "no evidence" instead
    # of being tonally indistinguishable from `false`/`true`.
    replay_passed_value = summary.get("replayPassed")
    replay_passed_display = "—" if replay_passed_value is None else str(replay_passed_value).lower()
    # B6: surface attempt-latency p50/p90/p99 in the top-level summary
    # block so timeout-induced rejection patterns are visible. Render
    # `—` when the sample set is empty (e.g. all-skipped runs) to match
    # the replayPassed null-rendering convention above.
    latency = summary.get("latencyDistribution") or {"p50Ms": None, "p90Ms": None, "p99Ms": None, "sampleCount": 0}

    def _ms(v: Any) -> str:
        return "—" if v is None else str(v)

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
        f"- replayPassed: `{replay_passed_display}`",
        f"- invalidAccepted: `{summary['safety']['invalidAccepted']}`",
        f"- latency.p50Ms: `{_ms(latency['p50Ms'])}` (sampleCount: `{latency['sampleCount']}`)",
        f"- latency.p90Ms: `{_ms(latency['p90Ms'])}`",
        f"- latency.p99Ms: `{_ms(latency['p99Ms'])}`",
    ]
    if summary.get("runTerminationReason"):
        lines.append(f"- runTerminationReason: `{summary['runTerminationReason']}`")
    # S24d: cross-target economic-spread surfaced at the top so a glance
    # tells you whether one model dominated the share-reward stream. Only
    # rendered when at least 2 targets had measured share-reward.
    reward_distribution = summary.get("rewardDistribution") or {}
    spread = reward_distribution.get("economicSpread")
    cumulative_share = reward_distribution.get("cumulativeShareReward")
    proposer_bonus_cum = reward_distribution.get("proposerBonusCumulative")
    if cumulative_share is not None:
        lines.append(f"- cumulativeShareReward: `{cumulative_share}`")
    if proposer_bonus_cum is not None:
        lines.append(f"- proposerBonusCumulative: `{proposer_bonus_cum}` (blocks proposed: `{reward_distribution.get('proposerBlockCount', 0)}`)")
    family_credits = reward_distribution.get("bountyFamilyCreditsByFamily") or {}
    if family_credits:
        rendered_families = ", ".join(f"{family}={amount}" for family, amount in sorted(family_credits.items()))
        lines.append(f"- bountyFamilyCreditsByFamily: `{rendered_families}`")
    if spread:
        lines.append(
            "- economicSpread: "
            f"`range={spread['rangeShareReward']}` "
            f"(max={spread['maxTarget']}={spread['maxShareReward']}, "
            f"min={spread['minTarget']}={spread['minShareReward']}), "
            f"spreadPct=`{spread['spreadPct']:.2f}%` "
            f"across `{spread['targetCount']}` targets"
        )
    lines.extend([
        "",
        "## Leaderboard",
        "",
    ])
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
    # B4: distinguish "replay verified the chain" from "replay was never
    # invoked." Only rows that actually exercised the replay path
    # (verifier invocation surfaced `replayMatchesRuntime`) are counted in
    # the aggregate. When the set is empty, the summary reports `null`
    # rather than the legacy vacuous `True`.
    invoked_active_rows = [row for row in active_rows if row.get("score", {}).get("replayInvoked") is True]
    replay_invoked_count = len(invoked_active_rows)
    if replay_invoked_count == 0:
        replay_passed: bool | None = None
    else:
        replay_passed = all(row.get("score", {}).get("replayPass") is True for row in invoked_active_rows)
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
    # B6: latency distribution over `skipped=False` rows with
    # `elapsedMs > 0`. Excluding skipped rows (env-missing /
    # setup-required) keeps the distribution about real model
    # work; excluding zero-elapsed rows skips early-exit
    # placeholders that never invoked a model.
    latency_samples = [
        int(row["elapsedMs"])
        for row in rows
        if not row.get("skipped") and int(row.get("elapsedMs", 0) or 0) > 0
    ]
    latency_distribution = _latency_quantiles(latency_samples)
    # S24a: cumulative share-reward, computed from per-row
    # `verifier.attemptShareReward` decimal strings. Rewards are u128 on
    # chain, so arithmetic stays in Python int space and surfaces back as
    # a string. Rows that did not exercise --measure-reward contribute
    # nothing (their attemptShareReward is None).
    per_target_reward: dict[str, int] = {}
    per_target_bonus: dict[str, int] = {}
    per_target_was_proposer: dict[str, int] = {}
    cumulative_total = 0
    cumulative_bonus = 0
    proposer_block_count = 0
    measured_rows = 0
    # S24c: per-family bounty credit, plus per-target attempt/accept counts.
    # `bountyFamilyCreditsByFamily` is the cross-row global rollup; the per-
    # target slice carries its own family map so leaderboard consumers can
    # read economic spread without re-walking the rows.
    bounty_credit_by_family: dict[str, int] = {}
    bounty_attempts_by_target: dict[str, int] = {}
    bounty_accepted_by_target: dict[str, int] = {}
    per_target_bounty_family: dict[str, dict[str, int]] = {}
    for row in rows:
        verifier = row.get("verifier") or {}
        target_key = row.get("target") or row.get("name") or "unknown"
        raw = verifier.get("attemptShareReward")
        if raw is not None:
            try:
                value = int(raw)
            except (TypeError, ValueError):
                value = None
            if value is not None:
                per_target_reward[target_key] = per_target_reward.get(target_key, 0) + value
                cumulative_total += value
                measured_rows += 1
        # S24b: per-row proposer credit, surfaced as decimal string by the
        # verifier block. Missing/non-numeric values fall through silently —
        # they shouldn't be possible when measure_reward + a block landed,
        # but the aggregator must not bail on a malformed row.
        bonus_raw = verifier.get("proposerBonusEarned")
        if bonus_raw is not None:
            try:
                bonus_int = int(bonus_raw)
            except (TypeError, ValueError):
                bonus_int = 0
            per_target_bonus[target_key] = per_target_bonus.get(target_key, 0) + bonus_int
            cumulative_bonus += bonus_int
        if verifier.get("wasProposer") is True:
            per_target_was_proposer[target_key] = per_target_was_proposer.get(target_key, 0) + 1
            proposer_block_count += 1
        # S24c: bounty acceptance + per-family credit.
        if verifier.get("bountyId"):
            bounty_attempts_by_target[target_key] = bounty_attempts_by_target.get(target_key, 0) + 1
            if verifier.get("bountyAccepted"):
                bounty_accepted_by_target[target_key] = bounty_accepted_by_target.get(target_key, 0) + 1
            family = verifier.get("bountyFamilyId")
            credit_raw = verifier.get("bountyCreditEarned")
            if isinstance(family, str) and credit_raw is not None:
                try:
                    credit_int = int(credit_raw)
                except (TypeError, ValueError):
                    credit_int = 0
                bounty_credit_by_family[family] = bounty_credit_by_family.get(family, 0) + credit_int
                target_map = per_target_bounty_family.setdefault(target_key, {})
                target_map[family] = target_map.get(family, 0) + credit_int
    all_target_keys = (
        set(per_target_reward)
        | set(per_target_bonus)
        | set(per_target_was_proposer)
        | set(bounty_attempts_by_target)
        | set(per_target_bounty_family)
    )
    # S24d: cross-target economic spread. Only meaningful when 2+ targets
    # carry measured share-reward — for a single-target run there is no
    # peer to compare against, so we emit `null` (not 0%) so consumers
    # don't read the absence of spread as "all models tied".
    economic_spread: dict[str, Any] | None = None
    if len(per_target_reward) >= 2:
        max_target, max_value = max(per_target_reward.items(), key=lambda kv: (kv[1], kv[0]))
        min_target, min_value = min(per_target_reward.items(), key=lambda kv: (kv[1], kv[0]))
        range_value = max_value - min_value
        spread_pct = round((range_value / max_value) * 100, 2) if max_value > 0 else 0.0
        economic_spread = {
            "targetCount": len(per_target_reward),
            "minShareReward": str(min_value),
            "maxShareReward": str(max_value),
            "rangeShareReward": str(range_value),
            "spreadPct": spread_pct,
            "minTarget": min_target,
            "maxTarget": max_target,
        }
    reward_distribution: dict[str, Any] = {
        "measuredRows": measured_rows,
        "cumulativeShareReward": str(cumulative_total) if measured_rows else None,
        "proposerBonusCumulative": str(cumulative_bonus) if measured_rows else None,
        "proposerBlockCount": proposer_block_count,
        "bountyFamilyCreditsByFamily": {
            family: str(amount) for family, amount in bounty_credit_by_family.items()
        },
        "economicSpread": economic_spread,
        "perTarget": {
            target: {
                "cumulativeShareReward": str(per_target_reward.get(target, 0)),
                "proposerBonusCumulative": str(per_target_bonus.get(target, 0)),
                "proposerBlockCount": per_target_was_proposer.get(target, 0),
                "bountyAttempts": bounty_attempts_by_target.get(target, 0),
                "bountyAccepted": bounty_accepted_by_target.get(target, 0),
                "bountyFamilyCreditsByFamily": {
                    family: str(amount)
                    for family, amount in per_target_bounty_family.get(target, {}).items()
                },
            }
            for target in all_target_keys
        },
    }
    return {
        "ok": ok,
        "benchmark": "boole-model-proof-to-block",
        "version": 0,
        "benchmarkMode": benchmark_mode,
        "targetFamily": target_family,
        "runId": run_id,
        "generatedAtUnixMs": generated_at_ms,
        "latencyDistribution": latency_distribution,
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
            "replayInvoked": replay_invoked_count,
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
        "rewardDistribution": reward_distribution,
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
            {
                "name": row["name"],
                "status": row.get("status"),
                "replayPass": row.get("score", {}).get("replayPass"),
                "replayInvoked": row.get("score", {}).get("replayInvoked"),
            }
            for row in rows
        ],
    }
    (output_dir / "replay-report.json").write_text(json.dumps(replay_report, indent=2) + "\n", encoding="utf-8")
    (output_dir / "leaderboard.md").write_text(render_leaderboard(summary, ordered), encoding="utf-8")


def run_benchmark(*, spec_path: Path | None = None, output_dir: Path, run_id: str | None = None, timeout_s: int | None = 600, target: str | None = None, attempts: int = 1, ollama_command: str = "ollama", claude_command: str = "claude", submit_lean_command: str | None = None, benchmark_mode: str = "mining", node_url: str | None = None, use_node_ticket: bool = False, max_run_seconds: int | None = None, measure_reward: bool = False, prover_pk: str | None = None, bounty_id: str | None = None, bounty_envelope: Any = None) -> dict[str, Any]:
    run_id = run_id or default_run_id()
    generated_at_ms = now_ms()
    # B6: cooperative wall-clock cap. We never SIGTERM in-flight
    # subprocesses; we only stop launching new attempts past the
    # deadline. `None` means "no cap".
    deadline_monotonic = (
        time.monotonic() + max_run_seconds
        if max_run_seconds is not None and max_run_seconds > 0
        else None
    )
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
                node_url=node_url,
                use_node_ticket=use_node_ticket,
                deadline_monotonic=deadline_monotonic,
                measure_reward=measure_reward,
                prover_pk=prover_pk,
                bounty_id=bounty_id,
                bounty_envelope=bounty_envelope,
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
                node_url=node_url,
                use_node_ticket=use_node_ticket,
                deadline_monotonic=deadline_monotonic,
                measure_reward=measure_reward,
                prover_pk=prover_pk,
                bounty_id=bounty_id,
                bounty_envelope=bounty_envelope,
            )
        expected_count = attempts
    else:
        if spec_path is None:
            raise SystemExit("--spec is required unless --target is provided")
        spec_rows = load_spec(spec_path)
        expected_count = len(spec_rows)
        rows = []
        for spec_row in spec_rows:
            if deadline_monotonic is not None and time.monotonic() > deadline_monotonic:
                break
            rows.append(run_row(spec_row, timeout_s=timeout_s))
    summary = summarize(rows, run_id, generated_at_ms)
    if deadline_monotonic is not None and len(rows) < expected_count:
        # B6: record *why* the run finalized with fewer rows than
        # planned. CI consumers that gate on `summary["ok"]` are
        # unaffected; this field is the diagnostic.
        summary["runTerminationReason"] = "max-run-seconds"
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
    parser.add_argument("--node-url", default=os.environ.get("BOOLE_NODE_URL"), help="Optional local Boole node base URL. When set with --submit-lean-command, accepted canonical submissions are POSTed to <url>/submit and scoring uses the node HTTP result.")
    parser.add_argument("--use-node-ticket", action="store_true", default=os.environ.get("BOOLE_USE_NODE_TICKET", "").lower() in {"1", "true", "yes"}, help="When used with --node-url, POST the canonical payload to <url>/ticket before /submit and record node ticket evidence.")
    parser.add_argument("--output-dir", help="Artifact output directory. Defaults to artifacts/model-benchmarks/<run-id>.")
    parser.add_argument("--run-id", help="Stable run id for reproducible tests/evidence.")
    parser.add_argument("--timeout-sec", type=int, default=600, help="Per-attempt timeout seconds (default: 600). Use 0 to disable subprocess timeouts; frontier-model cold-start latency rarely exceeds 600s, so the bumped default avoids silent rejections without removing the safety bound.")
    parser.add_argument("--max-run-seconds", type=int, default=0, help="Wall-clock cap for the entire run, in seconds. 0 (default) means no cap. When the cap fires, in-flight attempts complete normally but no new attempts launch; the summary records `runTerminationReason=\"max-run-seconds\"` and the process exits 0.")
    parser.add_argument("--benchmark-mode", choices=["mining", "smoke"], default="mining", help="Benchmark target mode. mining=active v1-lenbound proof-to-block target family; smoke=True.intro pipeline-only and not a public mining score.")
    parser.add_argument("--measure-reward", action="store_true", help="Poll <node>/account/<prover-pk>/balance before and after each /submit so each row carries the per-attempt share-reward delta. Requires --node-url and --prover-pk; the summary aggregates `cumulativeShareReward` over the run.")
    parser.add_argument("--prover-pk", default=os.environ.get("BOOLE_PROVER_PK"), help="32-byte hex public key the local node credits when this run wins shares. Used by --measure-reward to construct /account/<pk>/balance reads.")
    parser.add_argument("--bounty-id", default=os.environ.get("BOOLE_BOUNTY_ID"), help="When set, every attempt POSTs its candidate to <node>/bounties/<id>/proof after the share submission. Each row gains bountyAccepted/bountyFamilyId/bountyCreditEarned and the summary aggregates per-family credit. Requires --node-url and --prover-pk.")
    parser.add_argument("--bounty-envelope-json", default=None, help="Optional path to a JSON file used as the verifier-specific `envelope` field on each /bounties/<id>/proof body. Defaults to null when omitted.")
    parser.add_argument("--preflight-node", action="store_true", help="Health-check the node's economic routes (/head, /account/<pk>/balance, /bounties/<id>) and exit. Prints `{ok, probes:[...]}` JSON to stdout; exit code is 0 iff every probe returned 200. Requires --node-url; --prover-pk and --bounty-id activate the matching probes.")
    args = parser.parse_args(argv)

    if args.preflight_node:
        if not args.node_url:
            raise SystemExit("--preflight-node requires --node-url")
        result = preflight_node_economic_routes(
            node_url=args.node_url,
            prover_pk=args.prover_pk,
            bounty_id=args.bounty_id,
            timeout_s=None if args.timeout_sec == 0 else args.timeout_sec,
        )
        print(json.dumps(result, separators=(",", ":")))
        if not result["ok"]:
            failing = ", ".join(p["route"] for p in result["probes"] if not p["ok"])
            print(f"preflight failed: {failing}", file=sys.stderr)
            raise SystemExit(1)
        return

    if bool(args.spec) == bool(args.target):
        raise SystemExit("provide exactly one of --spec or --target")
    if args.attempts < 1:
        raise SystemExit("--attempts must be >= 1")
    if args.timeout_sec < 0:
        raise SystemExit("--timeout-sec must be >= 0")
    if args.max_run_seconds < 0:
        raise SystemExit("--max-run-seconds must be >= 0")
    if args.measure_reward and not args.node_url:
        raise SystemExit("--measure-reward requires --node-url so the runner can read /account/<pk>/balance.")
    if args.measure_reward and not args.prover_pk:
        raise SystemExit("--measure-reward requires --prover-pk to address the credited account.")
    if args.bounty_id and not args.node_url:
        raise SystemExit("--bounty-id requires --node-url so the runner can POST /bounties/<id>/proof.")
    if args.bounty_id and not args.prover_pk:
        raise SystemExit("--bounty-id requires --prover-pk to identify the proof's prover.")
    bounty_envelope: Any = None
    if args.bounty_envelope_json:
        envelope_path = Path(args.bounty_envelope_json)
        if not envelope_path.is_file():
            raise SystemExit(f"--bounty-envelope-json: file not found: {envelope_path}")
        try:
            bounty_envelope = json.loads(envelope_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as err:
            raise SystemExit(f"--bounty-envelope-json: invalid JSON in {envelope_path}: {err}") from err

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
        node_url=args.node_url,
        use_node_ticket=args.use_node_ticket,
        max_run_seconds=args.max_run_seconds if args.max_run_seconds > 0 else None,
        measure_reward=args.measure_reward,
        prover_pk=args.prover_pk,
        bounty_id=args.bounty_id,
        bounty_envelope=bounty_envelope,
    )
    print(json.dumps(result, separators=(",", ":")))
    if not result["ok"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
