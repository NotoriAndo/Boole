#!/usr/bin/env python3
"""Battery P2: flattening variants for linear and copy circuits, and the P2 re-run of packaged statements (offline)."""
from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_det as D           # noqa: E402
from zk_registry import gates as G                # noqa: E402
from zk_registry import lean_emit as E            # noqa: E402
from zk_registry import package as P              # noqa: E402

from test_zk_registry_package import make_package   # noqa: E402

FLAT = ("(try simp only [Inputs, Outputs, List.forall_mem_cons, List.forall_mem_nil, List.not_mem_nil, "
        "IsEmpty.forall_iff, and_true, implies_true, forall_const] at hin ⊢)")


class P2FormTests(unittest.TestCase):
    def test_p2_forms(self) -> None:
        self.assertEqual(E.BATTERY_P2, [("V3", ["simp", "simp_all", "decide", "omega", "grind"]),
                                        ("V4", ["simp_all", "grind"])])
        self.assertEqual(E.battery_prefix("V3", 130), [
            "intro w₁ w₂ h₁ h₂ hin", FLAT, "(try unfold Constraints Block0 Block1 Block2 at h₁ h₂)"])
        self.assertEqual(E.battery_prefix("V4", 2)[-1], "repeat' constructor")
        pre = E.battery_prefix("V3", 2, True, ["BinaryInputs"])
        self.assertEqual(pre[0], "intro w₁ w₂ h₁ h₂ hp₁ hp₂ hin")
        self.assertIn("(try unfold Preconditions at hp₁ hp₂)", pre)
        self.assertIn("BinaryInputs", pre[-1])

    def test_p2_battery_file(self) -> None:
        text = E.emit_battery_forms("ZkDet.toy", 2, [(v, t) for v, ts in E.BATTERY_P2 for t in ts], 200000)
        self.assertEqual(text.count("theorem triv_"), 7)
        self.assertEqual(text.count("set_option maxRecDepth 100000 in"), 7)
        self.assertIn("theorem triv_V4_grind", text)
        self.assertIn("  repeat' constructor\n  all_goals grind\n#print axioms triv_V4_grind", text)
        self.assertIn("  (try unfold Constraints at h₁ h₂)\n  grind\n#print axioms triv_V3_grind", text)
        layout = G._battery_layout(text)
        self.assertEqual(len(layout), 7)

    def test_p1_battery_is_unchanged(self) -> None:
        self.assertEqual(E.BATTERY_VARIANTS, ["V0", "V1", "V2"])
        v1 = E.emit_battery("ZkDet.toy", 2, "V1", ["grind"], 200000)
        self.assertNotIn("maxRecDepth", v1)
        self.assertIn("  (try unfold Constraints at *)", v1)


class RerunTests(unittest.TestCase):
    def test_closure_moves_an_open_package_to_gate_fail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            rec["gates"]["G-TRIV"] = {"status": "PASS", "closed_by": [], "forms": 33}
            p2 = G.Gate("G-TRIV", "FAIL", {"forms": 7, "closed_by": ["triv_V3_grind"], "timeouts": []})
            new = D.apply_p2(rec, p2.to_json())
            self.assertEqual(new["status"], "GATE-FAIL")
            self.assertEqual(new["statement"]["truth"], "closed-by-automation")
            self.assertEqual(new["gates"]["G-TRIV"]["status"], "FAIL")
            self.assertEqual(new["gates"]["G-TRIV"]["closed_by"], ["triv_V3_grind"])
            self.assertEqual(new["gates"]["G-TRIV"]["p2"]["forms"], 7)
            self.assertEqual(new["gates"]["G-TRIV"]["forms"], 40)
            self.assertIn("G-TRIV (battery P2) closed by triv_V3_grind", new["status_reason"])
            self.assertEqual(new["supersedes"], {"status": "OPEN", "status_reason": "fixture",
                                                 "generator_sources_sha256": rec["generator"]["sources_sha256"]})
            self.assertEqual(new["package_id"], rec["package_id"])
            self.assertEqual(P.validate_problem(new, pkg), [])
            self.assertEqual(rec["status"], "OPEN")                      # the input record is not modified

    def test_surviving_package_keeps_open(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            p2 = G.Gate("G-TRIV", "PASS", {"forms": 7, "closed_by": [], "timeouts": []})
            new = D.apply_p2(rec, p2.to_json())
            self.assertEqual(new["status"], "OPEN")
            self.assertEqual(new["gates"]["G-TRIV"]["status"], "PASS")
            self.assertEqual(P.validate_problem(new, pkg), [])


if __name__ == "__main__":
    unittest.main()
