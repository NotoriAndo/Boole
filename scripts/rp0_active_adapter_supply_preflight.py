#!/usr/bin/env python3
"""Fail-closed RP0-MD preflight for the currently active Rust adapter.

This tool never promotes candidate eligibility to reward-ready stock.  It only
proves the narrower statement that the frozen candidate upper bound cannot meet
the RP0-MD stock threshold and that the frozen annual-flow evidence is still
unmeasured.
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys
from typing import Any


INPUT_SCHEMA = "boole.rp0.active-adapter-supply-preflight-input.v1"
OUTPUT_SCHEMA = "boole.rp0.active-adapter-supply-preflight-result.v1"
EVIDENCE_KINDS = {"ACTUAL-FROZEN-ARTIFACTS", "SYNTHETIC-TEST"}
ACTUAL_MANIFEST_SHA256 = "77f1731ed28b6244d29bc12ddfdc738c953bcdf750196c146955223c1d3a6af2"
ACTIVE_FAMILY = "RUST-TUPLE-STRUCT-PROJECT-V1"
CANDIDATE_CLASSIFICATION = "CANDIDATE-ELIGIBILITY-NOT-REWARD-READY"
ACTIVE_ADAPTER_CANDIDATE_UPPER_BOUND = 197
STOCK_THRESHOLD = 10_950
ANNUAL_LOWER_THRESHOLD = 3_650
REQUIRED_LEDGER_MARKERS = (
    "LLM-MINEABLE-ELIGIBLE-V5 = 14,160",
    "197 is a candidate-eligibility count, not a solve count.",
    "does not expand `LLM-MINEABLE-ELIGIBLE-V5`",
    "mineable_now = 0",
)
ACTUAL_ARTIFACT_IDENTITIES = {
    "accounting_policy": {
        "root": "tracked",
        "path": "fixtures/supply/authority/rp0-accounting-policy-projection-v1.py",
        "sha256": "a6fe267095cb0fdcdc909165af04f678930c8154e3310fecffe8a5e5f4ef2c41",
        "source": {
            "root": "local",
            "path": "local-docs/replenishment-p0-2026-07-22/md_accounting.py",
            "sha256": "4455cc764b7f49b1e5f66f6bf914fef757da007a596a866f872ce2483b5f213e",
        },
    },
    "dedup_result": {
        "root": "tracked",
        "path": "fixtures/supply/authority/rp0-active-adapter-candidate-projection-v1.json",
        "sha256": "1eab05e37af062eb368280e732c78a85c218a3c197f4ad79bac51d6e073f5e70",
        "source": {
            "root": "local",
            "path": "local-docs/rust-tuple-struct-project-v1-2026-08-20/STEP7-DEDUP-RESULT.json",
            "sha256": "3f519d169cc113b3eef3b3ee0282738a20756b43dc32b58b2f6a532c2c7f6c2c",
        },
    },
    "flow_result": {
        "root": "tracked",
        "path": "fixtures/supply/authority/rp0-stock-flow-projection-v1.json",
        "sha256": "e047ee17fdf4d5e118a206ddc1de9e9a4de5d2604d5e5a4a530d46797dc08caf",
        "source": {
            "root": "local",
            "path": "local-docs/rp-a2-strict-p0-2026-07-26/result-summary-v4.json",
            "sha256": "1a39d63704e28d84140b6f0beee5c6c2eddb89b3aeaf0f12ee3918ab03f624a1",
        },
    },
    "reward_status": {
        "root": "tracked",
        "path": "fixtures/supply/authority/rp0-reward-status-projection-v1.json",
        "sha256": "c909d2ba7cf5a4b7e61ef30bd8a94832195517b2a349c154a3be337a71852415",
        "source": {
            "root": "local",
            "path": "local-docs/replenishment-p0-2026-07-22/rp-a2-authenticated-result-freeze-v4.json",
            "sha256": "80282c4b3b4cb943ffb4d6e4c107abd788c602d693aa91319cd5a0a1a8c20101",
        },
    },
    "tracked_ledger": {
        "root": "tracked",
        "path": "fixtures/supply/authority/rp0-active-adapter-ledger-projection-v1.txt",
        "sha256": "7b11fc471b2a5326bfe5d0b4e2881b0852dee9b5f74b2aa3ee5442b6f609b27a",
        "source": {
            "root": "tracked",
            "path": "docs/llm-mineable-eligibility-census-p1.md",
            "sha256": "b1f2f650c9cc0b191f84a2df7169cffde2a2c155757ab33c62f4e4ebca8ba8e7",
        },
    },
}


class PreflightStop(ValueError):
    """Frozen evidence cannot support this bounded HOLD preflight."""


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise PreflightStop(f"duplicate_json_key:{key}")
        result[key] = value
    return result


def _load_json(raw: bytes, label: str) -> Any:
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=_reject_duplicate_keys)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise PreflightStop(f"invalid_json:{label}") from exc


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _require_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise PreflightStop(f"{label}_keys")


def _read_frozen(root: Path, spec: dict[str, Any], label: str) -> bytes:
    raw_path = spec.get("path")
    expected = spec.get("sha256")
    if not isinstance(raw_path, str) or not isinstance(expected, str):
        raise PreflightStop(f"artifact_spec_invalid:{label}")
    pure = PurePosixPath(raw_path)
    if pure.is_absolute() or any(part in ("", ".", "..") for part in pure.parts):
        raise PreflightStop(f"artifact_path_not_normalized:{label}")
    resolved_root = root.resolve(strict=True)
    try:
        path = (resolved_root / Path(*pure.parts)).resolve(strict=True)
        path.relative_to(resolved_root)
    except (OSError, ValueError) as exc:
        raise PreflightStop(f"artifact_outside_root:{label}") from exc
    if not path.is_file():
        raise PreflightStop(f"artifact_not_regular_file:{label}")
    raw = path.read_bytes()
    actual = _sha256(raw)
    if actual != expected:
        raise PreflightStop(f"artifact_sha256_mismatch:{label}")
    return raw


def _policy_constants(raw: bytes) -> tuple[int, int]:
    try:
        tree = ast.parse(raw.decode("utf-8"), filename="md_accounting.py")
    except (UnicodeError, SyntaxError) as exc:
        raise PreflightStop("accounting_policy_unparseable") from exc
    values: dict[str, int] = {}
    wanted = {"PASS_STOCK_THRESHOLD", "PASS_ANNUAL_LOWER_THRESHOLD"}
    for node in tree.body:
        if not isinstance(node, ast.Assign) or len(node.targets) != 1:
            continue
        target = node.targets[0]
        if not isinstance(target, ast.Name) or target.id not in wanted:
            continue
        try:
            value = ast.literal_eval(node.value)
        except (ValueError, TypeError) as exc:
            raise PreflightStop("accounting_threshold_not_literal") from exc
        if not isinstance(value, int):
            raise PreflightStop("accounting_threshold_not_int")
        values[target.id] = value
    if set(values) != wanted:
        raise PreflightStop("accounting_threshold_missing")
    return values["PASS_STOCK_THRESHOLD"], values["PASS_ANNUAL_LOWER_THRESHOLD"]


def evaluate(manifest_path: Path, tracked_root: Path, local_root: Path) -> dict[str, Any]:
    manifest_raw = manifest_path.read_bytes()
    manifest = _load_json(manifest_raw, "manifest")
    if not isinstance(manifest, dict) or manifest.get("schema") != INPUT_SCHEMA:
        raise PreflightStop("manifest_schema")
    _require_keys(
        manifest,
        {
            "schema",
            "evidence_kind",
            "active_adapter",
            "artifacts",
            "ledger_markers",
            "policy",
        },
        "manifest",
    )
    evidence_kind = manifest.get("evidence_kind")
    if evidence_kind not in EVIDENCE_KINDS:
        raise PreflightStop("evidence_kind")
    if (
        evidence_kind == "ACTUAL-FROZEN-ARTIFACTS"
        and _sha256(manifest_raw) != ACTUAL_MANIFEST_SHA256
    ):
        raise PreflightStop("actual_manifest_sha256")

    active = manifest.get("active_adapter")
    if not isinstance(active, dict):
        raise PreflightStop("active_adapter_missing")
    _require_keys(
        active,
        {"family", "classification", "candidate_upper_bound"},
        "active_adapter",
    )
    if active.get("family") != ACTIVE_FAMILY:
        raise PreflightStop("active_adapter_family")
    if active.get("classification") != CANDIDATE_CLASSIFICATION:
        raise PreflightStop("candidate_must_not_be_reward_ready")
    candidate_upper_bound = active.get("candidate_upper_bound")
    if not isinstance(candidate_upper_bound, int) or candidate_upper_bound < 0:
        raise PreflightStop("candidate_upper_bound")
    if candidate_upper_bound != ACTIVE_ADAPTER_CANDIDATE_UPPER_BOUND:
        raise PreflightStop("active_adapter_candidate_bound_drift")

    policy = manifest.get("policy")
    if policy != {
        "stock_threshold": STOCK_THRESHOLD,
        "annual_lower_threshold": ANNUAL_LOWER_THRESHOLD,
    }:
        raise PreflightStop("threshold_drift")

    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, dict):
        raise PreflightStop("artifacts_missing")
    _require_keys(
        artifacts,
        {
            "tracked_ledger",
            "dedup_result",
            "flow_result",
            "accounting_policy",
            "reward_status",
        },
        "artifacts",
    )
    if evidence_kind == "ACTUAL-FROZEN-ARTIFACTS" and artifacts != ACTUAL_ARTIFACT_IDENTITIES:
        raise PreflightStop("actual_artifact_identities")
    roots = {"tracked": tracked_root, "local": local_root}
    raw_by_label: dict[str, bytes] = {}
    for label in (
        "tracked_ledger",
        "dedup_result",
        "flow_result",
        "accounting_policy",
        "reward_status",
    ):
        spec = artifacts.get(label)
        if not isinstance(spec, dict) or spec.get("root") not in roots:
            raise PreflightStop(f"artifact_root_invalid:{label}")
        raw_by_label[label] = _read_frozen(roots[spec["root"]], spec, label)

    stock_policy, inflow_policy = _policy_constants(raw_by_label["accounting_policy"])
    if (stock_policy, inflow_policy) != (STOCK_THRESHOLD, ANNUAL_LOWER_THRESHOLD):
        raise PreflightStop("accounting_policy_threshold_drift")

    markers = manifest.get("ledger_markers")
    if markers != list(REQUIRED_LEDGER_MARKERS):
        raise PreflightStop("required_ledger_markers_mismatch")
    ledger = raw_by_label["tracked_ledger"].decode("utf-8", errors="strict")
    if any(marker not in ledger for marker in markers):
        raise PreflightStop("tracked_ledger_marker_missing")

    dedup = _load_json(raw_by_label["dedup_result"], "dedup_result")
    if not isinstance(dedup, dict):
        raise PreflightStop("dedup_result_not_object")
    conservation = dedup.get("conservation_check")
    if (
        dedup.get("wave") != ACTIVE_FAMILY
        or dedup.get("model_calls_this_step") != 0
        or dedup.get("unique_new_issuable") != candidate_upper_bound
        or not isinstance(conservation, dict)
        or conservation.get("holds") is not True
        or conservation.get("unique_new_issuable") != candidate_upper_bound
        or "candidate-eligibility" not in str(dedup.get("overinterpretation_warning", ""))
    ):
        raise PreflightStop("candidate_evidence_not_upper_bound_only")

    if candidate_upper_bound >= STOCK_THRESHOLD:
        raise PreflightStop("candidate_upper_bound_no_longer_proves_insufficiency")

    flow = _load_json(raw_by_label["flow_result"], "flow_result")
    if not isinstance(flow, dict):
        raise PreflightStop("flow_result_not_object")
    flow_inputs = flow.get("inputs")
    capacities = flow.get("capacities")
    checks = flow.get("checks")
    if (
        not isinstance(flow_inputs, dict)
        or flow_inputs.get("annual_net_replenishment") != "NOT_MEASURED"
        or flow_inputs.get("effective_stock_actual") != 0
        or not isinstance(capacities, dict)
        or capacities.get("stock_only_3y_per_day") != "0"
        or flow.get("not_measured") != ["annual_net_replenishment"]
        or not isinstance(checks, dict)
        or checks.get("estimation_used") is not False
        or checks.get("stock_flow_overlap") != 0
        or flow.get("public_claim") is not False
    ):
        raise PreflightStop("flow_evidence_not_unmeasured_and_clean")

    reward = _load_json(raw_by_label["reward_status"], "reward_status")
    if not isinstance(reward, dict) or reward != {
        "bf7": "HOLD",
        "reward_ready": 0,
        "state": "RP_A2_FULL_TASK_MANIFEST_GO_STRICT_REWARD_HOLD",
        "strict_ready": 1,
    }:
        raise PreflightStop("reward_ready_status_not_zero_and_frozen")

    evidence = {
        label: {
            "path": artifacts[label]["path"],
            "root": artifacts[label]["root"],
            "sha256": artifacts[label]["sha256"],
            **(
                {"source": artifacts[label]["source"]}
                if "source" in artifacts[label]
                else {}
            ),
        }
        for label in sorted(raw_by_label)
    }
    return {
        "schema": OUTPUT_SCHEMA,
        "evidence_kind": evidence_kind,
        "input_manifest_sha256": _sha256(manifest_raw),
        "evidence": evidence,
        "stock_branch": {
            "label": "STOCK-BRANCH-MATHEMATICALLY-INSUFFICIENT",
            "active_adapter_family": ACTIVE_FAMILY,
            "reward_ready_stock": 0,
            "effective_reward_stock": 0,
            "pass_threshold": STOCK_THRESHOLD,
            "actual_stock_shortfall": STOCK_THRESHOLD,
        },
        "candidate_counterfactual": {
            "classification": CANDIDATE_CLASSIFICATION,
            "disposition": "NOT-COUNTED",
            "candidate_upper_bound_if_all_promoted": candidate_upper_bound,
            "candidate_shortfall_if_all_promoted": STOCK_THRESHOLD
            - candidate_upper_bound,
        },
        "flow_branch": {
            "label": "FLOW-BRANCH-NOT-MEASURED",
            "annual_reward_net_replenishment": "NOT_MEASURED",
            "annual_reward_net_replenishment_95pct_lower": "NOT_MEASURED",
            "pass_threshold": ANNUAL_LOWER_THRESHOLD,
        },
        "decision": "HOLD-REPLENISHMENT-P0-MD",
        "bf7_label": "HOLD-BF7-SUPPLY",
        "verdict": "HOLD",
        "pass_claimed": False,
        "not_a_public_or_paid_action": True,
    }


def crosscheck_original_sources(
    manifest_path: Path,
    tracked_root: Path,
    local_root: Path,
) -> dict[str, Any]:
    """Verify each tracked projection against its whole frozen original."""
    manifest_raw = manifest_path.read_bytes()
    manifest = _load_json(manifest_raw, "manifest")
    if (
        not isinstance(manifest, dict)
        or manifest.get("evidence_kind") != "ACTUAL-FROZEN-ARTIFACTS"
        or _sha256(manifest_raw) != ACTUAL_MANIFEST_SHA256
        or manifest.get("artifacts") != ACTUAL_ARTIFACT_IDENTITIES
    ):
        raise PreflightStop("crosscheck_requires_canonical_actual_manifest")

    artifacts = manifest["artifacts"]
    source_roots = {"tracked": tracked_root, "local": local_root}
    projection_raw = {
        label: _read_frozen(tracked_root, spec, f"projection:{label}")
        for label, spec in artifacts.items()
    }
    source_raw: dict[str, bytes] = {}
    for label, spec in artifacts.items():
        source = spec.get("source")
        if not isinstance(source, dict) or source.get("root") not in source_roots:
            raise PreflightStop(f"source_provenance_invalid:{label}")
        source_raw[label] = _read_frozen(
            source_roots[source["root"]],
            source,
            f"source:{label}",
        )

    projected_ledger = projection_raw["tracked_ledger"].decode("utf-8")
    original_ledger = source_raw["tracked_ledger"].decode("utf-8")
    if any(marker not in projected_ledger for marker in REQUIRED_LEDGER_MARKERS):
        raise PreflightStop("ledger_projection_facts")
    if any(marker not in original_ledger for marker in REQUIRED_LEDGER_MARKERS):
        raise PreflightStop("ledger_source_facts")

    projected_candidate = _load_json(projection_raw["dedup_result"], "candidate_projection")
    original_candidate = _load_json(source_raw["dedup_result"], "candidate_source")
    expected_candidate = {
        "wave": original_candidate.get("wave"),
        "model_calls_this_step": original_candidate.get("model_calls_this_step"),
        "unique_new_issuable": original_candidate.get("unique_new_issuable"),
        "conservation_check": {
            "holds": original_candidate.get("conservation_check", {}).get("holds"),
            "unique_new_issuable": original_candidate.get("conservation_check", {}).get(
                "unique_new_issuable"
            ),
        },
        "overinterpretation_warning": original_candidate.get(
            "overinterpretation_warning"
        ),
    }
    if projected_candidate != expected_candidate:
        raise PreflightStop("candidate_projection_differs_from_source")

    projected_flow = _load_json(projection_raw["flow_result"], "flow_projection")
    original_flow = _load_json(source_raw["flow_result"], "flow_source")
    expected_flow = {
        "inputs": {
            "annual_net_replenishment": original_flow.get("inputs", {}).get(
                "annual_net_replenishment"
            ),
            "effective_stock_actual": original_flow.get("inputs", {}).get(
                "effective_stock_actual"
            ),
        },
        "capacities": {
            "stock_only_3y_per_day": original_flow.get("capacities", {}).get(
                "stock_only_3y_per_day"
            )
        },
        "not_measured": original_flow.get("not_measured"),
        "checks": {
            "estimation_used": original_flow.get("checks", {}).get("estimation_used"),
            "stock_flow_overlap": original_flow.get("checks", {}).get(
                "stock_flow_overlap"
            ),
        },
        "public_claim": original_flow.get("public_claim"),
    }
    if projected_flow != expected_flow:
        raise PreflightStop("flow_projection_differs_from_source")

    projected_reward = _load_json(
        projection_raw["reward_status"], "reward_projection"
    )
    original_reward = _load_json(source_raw["reward_status"], "reward_source")
    expected_reward = {
        key: original_reward.get(key)
        for key in ("bf7", "reward_ready", "state", "strict_ready")
    }
    if projected_reward != expected_reward:
        raise PreflightStop("reward_projection_differs_from_source")

    if _policy_constants(projection_raw["accounting_policy"]) != _policy_constants(
        source_raw["accounting_policy"]
    ):
        raise PreflightStop("policy_projection_differs_from_source")

    return {
        "schema": "boole.rp0.active-adapter-supply-source-crosscheck.v1",
        "input_manifest_sha256": ACTUAL_MANIFEST_SHA256,
        "sources_verified": 5,
        "projection_facts_verified": True,
    }


def canonical_json(document: Any) -> str:
    return json.dumps(
        document,
        ensure_ascii=False,
        indent=2,
        sort_keys=True,
        allow_nan=False,
    ) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--tracked-root", type=Path, required=True)
    parser.add_argument("--local-root", type=Path, required=True)
    parser.add_argument("--crosscheck-originals", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = evaluate(args.manifest, args.tracked_root, args.local_root)
        if args.crosscheck_originals:
            crosscheck_original_sources(args.manifest, args.tracked_root, args.local_root)
    except (OSError, UnicodeError, PreflightStop) as exc:
        print(f"STOP:{exc}", file=sys.stderr)
        return 2
    sys.stdout.write(canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
