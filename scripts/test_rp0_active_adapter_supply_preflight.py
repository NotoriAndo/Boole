#!/usr/bin/env python3
"""Behavior tests for the RP0 active-adapter supply preflight."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts import rp0_active_adapter_supply_preflight as preflight


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


class ActiveAdapterSupplyPreflightTests(unittest.TestCase):
    def _evidence(self, root: Path) -> tuple[Path, Path, Path]:
        tracked = root / "tracked"
        local = root / "local"
        tracked.mkdir()
        local.mkdir()

        ledger_raw = (
            "LLM-MINEABLE-ELIGIBLE-V5 = 14,160\n"
            "197 is a candidate-eligibility count, not a solve count.\n"
            "Entry 27 does not expand `LLM-MINEABLE-ELIGIBLE-V5`.\n"
            "mineable_now = 0\n"
        ).encode()
        (tracked / "ledger.md").write_bytes(ledger_raw)

        dedup = {
            "wave": "RUST-TUPLE-STRUCT-PROJECT-V1",
            "model_calls_this_step": 0,
            "unique_new_issuable": 197,
            "conservation_check": {
                "holds": True,
                "unique_new_issuable": 197,
            },
            "overinterpretation_warning": (
                "197 is a candidate-eligibility count after internal dedup, "
                "not a solve rate."
            ),
        }
        dedup_raw = (json.dumps(dedup, sort_keys=True) + "\n").encode()
        (local / "dedup.json").write_bytes(dedup_raw)

        flow = {
            "inputs": {
                "annual_net_replenishment": "NOT_MEASURED",
                "effective_stock_actual": 0,
            },
            "capacities": {"stock_only_3y_per_day": "0"},
            "not_measured": ["annual_net_replenishment"],
            "checks": {"estimation_used": False, "stock_flow_overlap": 0},
            "public_claim": False,
        }
        flow_raw = (json.dumps(flow, sort_keys=True) + "\n").encode()
        (local / "flow.json").write_bytes(flow_raw)

        accounting_raw = (
            b"PASS_STOCK_THRESHOLD = 10_950\n"
            b"PASS_ANNUAL_LOWER_THRESHOLD = 3_650\n"
        )
        (local / "md_accounting.py").write_bytes(accounting_raw)

        reward_status = {
            "bf7": "HOLD",
            "reward_ready": 0,
            "strict_ready": 1,
            "state": "RP_A2_FULL_TASK_MANIFEST_GO_STRICT_REWARD_HOLD",
        }
        reward_raw = (json.dumps(reward_status, sort_keys=True) + "\n").encode()
        (local / "reward.json").write_bytes(reward_raw)

        manifest = {
            "schema": "boole.rp0.active-adapter-supply-preflight-input.v1",
            "evidence_kind": "SYNTHETIC-TEST",
            "active_adapter": {
                "family": "RUST-TUPLE-STRUCT-PROJECT-V1",
                "classification": "CANDIDATE-ELIGIBILITY-NOT-REWARD-READY",
                "candidate_upper_bound": 197,
            },
            "artifacts": {
                "tracked_ledger": {
                    "root": "tracked",
                    "path": "ledger.md",
                    "sha256": _sha256(ledger_raw),
                },
                "dedup_result": {
                    "root": "local",
                    "path": "dedup.json",
                    "sha256": _sha256(dedup_raw),
                },
                "flow_result": {
                    "root": "local",
                    "path": "flow.json",
                    "sha256": _sha256(flow_raw),
                },
                "accounting_policy": {
                    "root": "local",
                    "path": "md_accounting.py",
                    "sha256": _sha256(accounting_raw),
                },
                "reward_status": {
                    "root": "local",
                    "path": "reward.json",
                    "sha256": _sha256(reward_raw),
                },
            },
            "ledger_markers": [
                "LLM-MINEABLE-ELIGIBLE-V5 = 14,160",
                "197 is a candidate-eligibility count, not a solve count.",
                "does not expand `LLM-MINEABLE-ELIGIBLE-V5`",
                "mineable_now = 0",
            ],
            "policy": {
                "stock_threshold": 10_950,
                "annual_lower_threshold": 3_650,
            },
        }
        manifest_path = root / "manifest.json"
        manifest_path.write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        return manifest_path, tracked, local

    def test_candidate_upper_bound_below_threshold_yields_truthful_hold(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest, tracked, local = self._evidence(Path(tmp))
            result = preflight.evaluate(manifest, tracked, local)

        self.assertEqual(result["stock_branch"]["label"], "STOCK-BRANCH-MATHEMATICALLY-INSUFFICIENT")
        self.assertEqual(result["stock_branch"]["reward_ready_stock"], 0)
        self.assertEqual(result["stock_branch"]["effective_reward_stock"], 0)
        self.assertEqual(result["stock_branch"]["actual_stock_shortfall"], 10_950)
        self.assertNotIn("candidate_upper_bound", result["stock_branch"])
        self.assertEqual(
            result["candidate_counterfactual"],
            {
                "candidate_shortfall_if_all_promoted": 10_753,
                "candidate_upper_bound_if_all_promoted": 197,
                "classification": "CANDIDATE-ELIGIBILITY-NOT-REWARD-READY",
                "disposition": "NOT-COUNTED",
            },
        )
        self.assertEqual(result["flow_branch"]["label"], "FLOW-BRANCH-NOT-MEASURED")
        self.assertEqual(result["verdict"], "HOLD")
        self.assertFalse(result["pass_claimed"])
        self.assertEqual(result["evidence_kind"], "SYNTHETIC-TEST")

    def test_any_frozen_artifact_drift_stops_without_a_verdict(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest, tracked, local = self._evidence(Path(tmp))
            (local / "dedup.json").write_text("{}\n", encoding="utf-8")
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "artifact_sha256_mismatch:dedup_result",
            ):
                preflight.evaluate(manifest, tracked, local)

    def test_manifest_cannot_remove_the_candidate_not_reward_ready_warning(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["ledger_markers"] = ["mineable_now = 0"]
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "required_ledger_markers_mismatch",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_manifest_cannot_replace_the_active_adapter_bound_with_v5_or_domain_supply(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            dedup_path = local / "dedup.json"
            dedup = json.loads(dedup_path.read_text(encoding="utf-8"))
            dedup["unique_new_issuable"] = 905
            dedup["conservation_check"]["unique_new_issuable"] = 905
            dedup_raw = (json.dumps(dedup, sort_keys=True) + "\n").encode()
            dedup_path.write_bytes(dedup_raw)

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["active_adapter"]["candidate_upper_bound"] = 905
            manifest["artifacts"]["dedup_result"]["sha256"] = _sha256(dedup_raw)
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "active_adapter_candidate_bound_drift",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_public_provenance_freeze_contains_only_candidate_upper_bound_evidence(self) -> None:
        manifest_path = (
            Path(__file__).resolve().parents[1]
            / "fixtures"
            / "supply"
            / "rp0-active-adapter-supply-preflight-v1.json"
        )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

        self.assertEqual(
            manifest["active_adapter"],
            {
                "candidate_upper_bound": 197,
                "classification": "CANDIDATE-ELIGIBILITY-NOT-REWARD-READY",
                "family": "RUST-TUPLE-STRUCT-PROJECT-V1",
            },
        )
        serialized = json.dumps(manifest, sort_keys=True)
        self.assertNotIn("reward_ready_stock", serialized)
        self.assertNotIn("annual_reward_net_replenishment_95pct_lower", serialized)

    def test_tracked_canonical_result_is_the_actual_frozen_artifact_hold(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        fixture_root = repo_root / "fixtures" / "supply"
        manifest_path = fixture_root / "rp0-active-adapter-supply-preflight-v1.json"
        manifest_raw = manifest_path.read_bytes()
        result_raw = (
            fixture_root / "rp0-active-adapter-supply-preflight-result-v1.json"
        ).read_bytes()
        result = json.loads(result_raw)
        reproduced = preflight.evaluate(manifest_path, repo_root, repo_root)

        self.assertEqual(result_raw, preflight.canonical_json(reproduced).encode("utf-8"))
        self.assertEqual(result_raw, preflight.canonical_json(result).encode("utf-8"))
        self.assertEqual(result["evidence_kind"], "ACTUAL-FROZEN-ARTIFACTS")
        self.assertEqual(result["input_manifest_sha256"], _sha256(manifest_raw))
        self.assertEqual(result["stock_branch"]["actual_stock_shortfall"], 10_950)
        self.assertEqual(result["stock_branch"]["reward_ready_stock"], 0)
        self.assertEqual(result["stock_branch"]["effective_reward_stock"], 0)
        self.assertEqual(
            result["candidate_counterfactual"]["candidate_upper_bound_if_all_promoted"],
            197,
        )
        self.assertEqual(result["candidate_counterfactual"]["disposition"], "NOT-COUNTED")
        self.assertEqual(result["flow_branch"]["label"], "FLOW-BRANCH-NOT-MEASURED")
        self.assertEqual(result["verdict"], "HOLD")
        self.assertFalse(result["pass_claimed"])

    def test_manifest_cannot_smuggle_a_reward_ready_stock_claim(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["active_adapter"]["reward_ready_stock"] = 14_160
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "active_adapter_keys",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_manifest_cannot_smuggle_a_top_level_reward_ready_claim(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["reward_ready_stock"] = 14_160
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(preflight.PreflightStop, "manifest_keys"):
                preflight.evaluate(manifest_path, tracked, local)

    def test_candidate_calibration_cannot_be_relabelled_reward_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["active_adapter"]["classification"] = "REWARD-READY"
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "candidate_must_not_be_reward_ready",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_measured_or_estimated_flow_requires_the_full_md_path_instead(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            flow_path = local / "flow.json"
            flow = json.loads(flow_path.read_text(encoding="utf-8"))
            flow["inputs"]["annual_net_replenishment"] = 5000
            flow["not_measured"] = []
            flow["checks"]["estimation_used"] = True
            flow_raw = (json.dumps(flow, sort_keys=True) + "\n").encode()
            flow_path.write_bytes(flow_raw)
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["artifacts"]["flow_result"]["sha256"] = _sha256(flow_raw)
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "flow_evidence_not_unmeasured_and_clean",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_threshold_weakening_is_rejected_even_when_manifest_and_policy_agree(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            policy_path = local / "md_accounting.py"
            policy_raw = (
                b"PASS_STOCK_THRESHOLD = 197\n"
                b"PASS_ANNUAL_LOWER_THRESHOLD = 1\n"
            )
            policy_path.write_bytes(policy_raw)
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["policy"] = {
                "stock_threshold": 197,
                "annual_lower_threshold": 1,
            }
            manifest["artifacts"]["accounting_policy"]["sha256"] = _sha256(policy_raw)
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(preflight.PreflightStop, "threshold_drift"):
                preflight.evaluate(manifest_path, tracked, local)

    def test_cli_output_is_byte_deterministic_and_contains_no_host_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest, tracked, local = self._evidence(Path(tmp))
            command = [
                sys.executable,
                str(Path(preflight.__file__).resolve()),
                "--manifest",
                str(manifest),
                "--tracked-root",
                str(tracked),
                "--local-root",
                str(local),
            ]
            first = subprocess.run(command, check=True, capture_output=True)
            second = subprocess.run(command, check=True, capture_output=True)

        self.assertEqual(first.stdout, second.stdout)
        self.assertEqual(first.stderr, b"")
        self.assertNotIn(str(tracked).encode(), first.stdout)
        self.assertNotIn(str(local).encode(), first.stdout)
        parsed = json.loads(first.stdout)
        self.assertEqual(parsed["verdict"], "HOLD")

    def test_candidate_at_or_above_threshold_cannot_reuse_the_hold_proof(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            dedup_path = local / "dedup.json"
            dedup = json.loads(dedup_path.read_text(encoding="utf-8"))
            dedup["unique_new_issuable"] = 10_950
            dedup["conservation_check"]["unique_new_issuable"] = 10_950
            dedup_raw = (json.dumps(dedup, sort_keys=True) + "\n").encode()
            dedup_path.write_bytes(dedup_raw)
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["active_adapter"]["candidate_upper_bound"] = 10_950
            manifest["artifacts"]["dedup_result"]["sha256"] = _sha256(dedup_raw)
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "active_adapter_candidate_bound_drift",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_stock_flow_overlap_is_a_stop_not_a_hold_result(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            flow_path = local / "flow.json"
            flow = json.loads(flow_path.read_text(encoding="utf-8"))
            flow["checks"]["stock_flow_overlap"] = 1
            flow_raw = (json.dumps(flow, sort_keys=True) + "\n").encode()
            flow_path.write_bytes(flow_raw)
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["artifacts"]["flow_result"]["sha256"] = _sha256(flow_raw)
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "flow_evidence_not_unmeasured_and_clean",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_artifact_path_traversal_stops_before_reading_outside_the_root(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["artifacts"]["dedup_result"]["path"] = "../dedup.json"
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "artifact_path_not_normalized:dedup_result",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_duplicate_json_key_stops_before_any_evidence_is_counted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            raw = manifest_path.read_text(encoding="utf-8")
            duplicate = raw[:-2] + ',\n  "schema": "forged"\n}\n'
            manifest_path.write_text(duplicate, encoding="utf-8")
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "duplicate_json_key:schema",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_synthetic_manifest_cannot_be_relabelled_as_actual_frozen_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest_path, tracked, local = self._evidence(Path(tmp))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["evidence_kind"] = "ACTUAL-FROZEN-ARTIFACTS"
            manifest_path.write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                preflight.PreflightStop,
                "actual_manifest_sha256",
            ):
                preflight.evaluate(manifest_path, tracked, local)

    def test_actual_mode_requires_exact_projection_artifact_identities(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        canonical = (
            repo_root
            / "fixtures"
            / "supply"
            / "rp0-active-adapter-supply-preflight-v1.json"
        )
        with tempfile.TemporaryDirectory() as tmp:
            forged_path = Path(tmp) / "manifest.json"
            forged = json.loads(canonical.read_text(encoding="utf-8"))
            forged["artifacts"]["dedup_result"]["path"] = forged["artifacts"][
                "flow_result"
            ]["path"]
            forged_raw = (json.dumps(forged, indent=2, sort_keys=True) + "\n").encode()
            forged_path.write_bytes(forged_raw)
            with mock.patch.object(
                preflight,
                "ACTUAL_MANIFEST_SHA256",
                _sha256(forged_raw),
            ):
                with self.assertRaisesRegex(
                    preflight.PreflightStop,
                    "actual_artifact_identities",
                ):
                    preflight.evaluate(forged_path, repo_root, repo_root)


if __name__ == "__main__":
    unittest.main()
