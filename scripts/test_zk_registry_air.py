#!/usr/bin/env python3
"""AIR DET generator: IR, evaluator, Lean emission, bus roles, counterexample search, statuses, packages (offline)."""
from __future__ import annotations

import copy
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import air_bus as B       # noqa: E402
from zk_registry import air_det as D       # noqa: E402
from zk_registry import air_ir as IR       # noqa: E402
from zk_registry import air_lean as AL     # noqa: E402
from zk_registry import air_search as AS   # noqa: E402
from zk_registry import package as P       # noqa: E402

KB = IR.FIELDS["KoalaBear"]
FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-air"


def toy_doc(name: str, index: int, loose: bool = False) -> dict:
    """ToyAdd: c = a + b on real rows; receives (a, b), sends (c), range-looks-up a.  ``loose`` drops the c
    constraint (then DET is false)."""
    nodes = [["main", 0, 0], ["main", 0, 1], ["main", 0, 2], ["main", 0, 3], ["const", 1],
             ["sub", 3, 4], ["mul", 3, 5],                         # 6: is_real * (is_real - 1)
             ["add", 0, 1], ["sub", 2, 7], ["mul", 3, 8]]          # 9: is_real * (c - (a + b))
    cons = [6] if loose else [6, 9]
    return {"format": IR.FORMAT, "zkvm": "toy", "release": "v1", "commit": "0" * 40,
            "field": {"name": "KoalaBear", "p": KB},
            "air": {"name": name, "rust_type": f"{name}Chip<F>", "group": "riscv", "index": index},
            "width": 4, "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": nodes,
            "constraints": cons,
            "interactions": [
                {"dir": "receive", "kind": 1, "kind_name": "State", "bus": None, "scope": "local", "values": [0, 1],
                 "mult": 3, "count_weight": None},
                {"dir": "send", "kind": 2, "kind_name": "Result", "bus": None, "scope": "local", "values": [2],
                 "mult": 3, "count_weight": None},
                {"dir": "send", "kind": 5, "kind_name": "Byte", "bus": None, "scope": "local", "values": [0],
                 "mult": 3, "count_weight": None}]}


def window_doc() -> dict:
    """ToyCounter: x increments along the trace (transition constraint), x = 0 on the first row; sends x."""
    nodes = [["main", 0, 0], ["main", 1, 0], ["const", 1], ["trans"], ["first"], ["main", 0, 1],
             ["sub", 1, 0], ["sub", 6, 2], ["mul", 3, 7],           # 8: trans * (next.x - x - 1)
             ["mul", 4, 0]]                                          # 9: first * x
    return {"format": IR.FORMAT, "zkvm": "toy", "release": "v1", "commit": "0" * 40,
            "field": {"name": "KoalaBear", "p": KB},
            "air": {"name": "ToyCounter", "rust_type": "ToyCounter", "group": "system", "index": 2},
            "width": 2, "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": nodes,
            "constraints": [8, 9],
            "interactions": [{"dir": "send", "kind": 2, "kind_name": "Result", "bus": None, "scope": None,
                              "values": [0], "mult": 5, "count_weight": None}]}


class ToyModel(B.BusModel):
    zkvm = "sp1"
    display = "toy"
    tables = {"toyRange": "/-- values below 256 -/\ndef toyRange (v : List F) : Bool :=\n  match v with\n"
                          "  | [x] => x.val < 256\n  | _ => false"}

    def rule(self, air, k, it, negative=False):
        if it.kind == 1:
            return B.Rule("in", "received state")
        if it.kind == 2:
            return B.Rule("out", "result")
        return B.Rule("assume", "byte range", table="toyRange")

    def table_fn(self, table, values):
        return len(values) == 1 and values[0] < 256


def toy_rows(air: IR.Air, n: int = 16) -> dict:
    main = {}
    for i in range(n):
        a, b = (7 * i + 3) % 256, (11 * i + 5) % 1000
        main[str(i)] = [a, b, (a + b) % KB, 1] if i < n - 2 else [0, 0, 0, 0]
    return {"format": "boole-air-rows/v1", "air_index": air.index, "name": air.name, "source": "toy", "height": n,
            "width": 4, "preprocessed_width": 0, "public_values": [], "main": main, "prep": {}}


class IrTests(unittest.TestCase):
    def test_load_and_evaluate(self) -> None:
        air = IR.from_json(toy_doc("ToyAdd", 0))
        lay = IR.Layout.of(air)
        self.assertEqual(lay.n_vars, 4)
        self.assertEqual(air.window_rows(), 1)
        self.assertEqual(air.degree(), 2)
        self.assertTrue(IR.satisfies(air, lay, [3, 4, 7, 1]))
        self.assertEqual(IR.failing_constraints(air, lay, [3, 4, 8, 1]), [1])
        self.assertTrue(IR.satisfies(air, lay, [3, 4, 8, 0]))

    def test_malformed_documents_are_rejected(self) -> None:
        doc = toy_doc("ToyAdd", 0)
        bad = copy.deepcopy(doc)
        bad["nodes"][6] = ["mul", 3, 7]                  # forward reference
        with self.assertRaises(IR.IrError):
            IR.from_json(bad)
        bad = copy.deepcopy(doc)
        bad["nodes"][0] = ["main", 0, 4]                 # column outside the width
        with self.assertRaises(IR.IrError):
            IR.from_json(bad)
        bad = copy.deepcopy(doc)
        bad["nodes"][4] = ["const", KB]                  # non-canonical constant
        with self.assertRaises(IR.IrError):
            IR.from_json(bad)
        bad = copy.deepcopy(doc)
        bad["field"] = {"name": "KoalaBear", "p": 2013265921}
        with self.assertRaises(IR.IrError):
            IR.from_json(bad)

    def test_two_row_window_layout(self) -> None:
        air = IR.from_json(window_doc())
        lay = IR.Layout.of(air)
        self.assertEqual(air.window_rows(), 2)
        self.assertEqual(lay.selectors, ["first", "trans"])
        self.assertEqual(lay.n_vars, 6)                  # 2 local + 2 next + 2 selectors
        self.assertEqual(lay.fixed(), [4, 5])
        rows = [[0, 1], [1, 1], [2, 1]]
        w = lay.window(rows, None, [], 2, 3)             # last row: next wraps to row 0, no transition
        self.assertEqual(w, [2, 1, 0, 1, 0, 0])
        self.assertTrue(IR.satisfies(air, lay, w))
        self.assertTrue(IR.satisfies(air, lay, lay.window(rows, None, [], 0, 3)))
        self.assertFalse(IR.satisfies(air, lay, lay.window([[1, 1], [2, 1], [3, 1]], None, [], 0, 3)))

    def test_selector_homogeneity(self) -> None:
        self.assertTrue(D.selector_homogeneous(IR.from_json(window_doc())))
        doc = window_doc()
        doc["nodes"].append(["add", 3, 0])                # trans + x: not a guard factor
        doc["constraints"].append(10)
        self.assertFalse(D.selector_homogeneous(IR.from_json(doc)))


class LeanEmitTests(unittest.TestCase):
    def roles(self, air):
        with mock.patch.dict(B.MODELS, {"sp1": ToyModel}):
            return B.model_for("sp1").roles(air)

    def test_model_text(self) -> None:
        air = IR.from_json(toy_doc("ToyAdd", 0))
        roles, iface = self.roles(air)
        self.assertEqual([m["role"] for m in iface["inputs"]], ["in"])
        self.assertEqual(iface["assumptions"][0]["table"], "toyRange")
        meta = {"zkvm_name": "toy", "release": "v1", "generator": "g", "repo_url": "https://example.invalid",
                "extractor": "x", "ir_sha256": "0" * 64}
        text, summary = AL.emit_model("ZkDet.toy_ToyAdd", meta, air, IR.Layout.of(air), roles, ToyModel.tables)
        self.assertIn("  w 3 * (w 3 - 1) = 0 ∧\n  w 3 * (w 2 - (w 0 + w 1)) = 0", text)
        self.assertIn("(w 3 ≠ 0 → toyRange [w 0] = true)", text)
        self.assertIn("    (w 3, [w 0, w 1])  -- #0 receive State", text)
        self.assertIn("def Out (w : Fin nVars → F) : List Msg :=\n  [\n    (w 3, [w 2])  -- #1 send Result", text)
        self.assertIn("def Fixed : List (Fin nVars) := []", text)
        self.assertIn("-/\n\nset_option maxRecDepth 100000\n\nnamespace ZkDet.toy_ToyAdd", text)
        self.assertEqual(summary["tables"], ["toyRange"])
        st = AL.emit_statement("ZkDet.toy_ToyAdd", meta, air)
        self.assertIn("theorem det [Fact (Nat.Prime p)] :\n    ∀ w₁ w₂ : Fin nVars → F, Constraints w₁ → Constraints w₂ → "
                      "Assumptions w₁ → Assumptions w₂ →\n      (∀ i ∈ Fixed, w₁ i = w₂ i) → BusEq (In w₁) (In w₂) → "
                      "BusEq (Out w₁) (Out w₂) := by\n  sorry", st)

    def test_assumption_conjunction_is_not_commented_out(self) -> None:
        doc = toy_doc("ToyAdd", 0)
        doc["interactions"].append({"dir": "send", "kind": 5, "kind_name": "Byte", "bus": None, "scope": "local",
                                    "values": [1], "mult": 3, "count_weight": None})
        air = IR.from_json(doc)
        roles, _ = self.roles(air)
        meta = {"zkvm_name": "toy", "release": "v1", "generator": "g", "repo_url": "https://example.invalid",
                "extractor": "x", "ir_sha256": "0" * 64}
        text, _ = AL.emit_model("ZkDet.toy", meta, air, IR.Layout.of(air), roles, ToyModel.tables)
        asm = text.split("def Assumptions (w : Fin nVars → F) : Prop :=\n", 1)[1].split("\n\n", 1)[0].split("\n")
        self.assertEqual(len(asm), 2)
        self.assertTrue(asm[0].startswith("  (w 3 ≠ 0 → toyRange [w 0] = true) ∧  -- #2 send Byte"))
        self.assertTrue(asm[1].startswith("  (w 3 ≠ 0 → toyRange [w 1] = true)  -- #3 send Byte"))

    def test_long_assumption_lists_are_blocked(self) -> None:
        doc = toy_doc("ToyAdd", 0)
        for _ in range(AL.BLOCK + 5):                    # 70 lookups: more than one block of conjuncts
            doc["interactions"].append({"dir": "send", "kind": 5, "kind_name": "Byte", "bus": None, "scope": "local",
                                        "values": [1], "mult": 3, "count_weight": None})
        air = IR.from_json(doc)
        roles, _ = self.roles(air)
        meta = {"zkvm_name": "toy", "release": "v1", "generator": "g", "repo_url": "https://example.invalid",
                "extractor": "x", "ir_sha256": "0" * 64}
        text, summary = AL.emit_model("ZkDet.toy", meta, air, IR.Layout.of(air), roles, ToyModel.tables)
        self.assertEqual(summary["asm_blocks"], ["AsmBlock0", "AsmBlock1"])
        self.assertIn("def Assumptions (w : Fin nVars → F) : Prop :=\n  AsmBlock0 w ∧ AsmBlock1 w\n", text)
        self.assertEqual(text.count("toyRange [w 1] = true"), AL.BLOCK + 5)
        fid = AL.emit_fid_runner("ZkDet.toy", summary, [("real_000", "/x")])
        self.assertIn("Decidable (AsmBlock1 w) := by unfold AsmBlock1; infer_instance", fid)
        self.assertIn("(try unfold Assumptions AsmBlock0 AsmBlock1 at ha₁ ha₂)", AL.battery_prefix("V3", summary))

    def test_constants_are_balanced_and_shared_terms_hoisted(self) -> None:
        doc = toy_doc("ToyAdd", 0)
        doc["nodes"] += [["const", KB - 5], ["add", 9, 10]]
        big = 11
        for _ in range(8):                               # a shared subterm of size > HOIST_MIN, used twice
            doc["nodes"].append(["mul", big, big])
            big = len(doc["nodes"]) - 1
        doc["constraints"] = [big]
        air = IR.from_json(doc)
        roles, _ = self.roles(air)
        pr = AL.Printer(air, IR.Layout.of(air), AL.roots_of(air, roles))
        self.assertTrue(pr.hoisted)
        text = "\n".join(pr.definitions())
        self.assertIn("(-5)", text)
        self.assertLess(max(len(x) for x in text.split("\n")), 4000)

    def test_battery_forms(self) -> None:
        summary = {"hoisted": ["t12"], "blocks": [], "asm_blocks": [], "tables": ["toyRange"], "n_constraints": 2,
                   "n_vars": 4}
        text = AL.emit_battery_forms("ZkDet.toy", summary, [("V1", "simp"), ("V3", "grind"), ("V4", "simp_all")], 200000)
        self.assertEqual(text.count("theorem triv_"), 3)
        self.assertIn("  (try unfold toyRange at *)", text)
        self.assertIn("  intro w₁ w₂ h₁ h₂ ha₁ ha₂ hfix hin", text)
        self.assertIn("  (try unfold t12 at *)", text)
        self.assertIn("  repeat' constructor\n  all_goals simp_all\n#print axioms triv_V4_simp_all", text)
        self.assertEqual(AL.unfold_order(summary)[:6], ["Constraints", "Assumptions", "toyRange", "Out", "In", "Fixed"])


class SearchTests(unittest.TestCase):
    def ctx(self, doc):
        air = IR.from_json(doc)
        with mock.patch.dict(B.MODELS, {"sp1": ToyModel}):
            m = B.model_for("sp1")
            roles, _ = m.roles(air)
        return AS.Context(air, IR.Layout.of(air), roles, m.table_fn)

    def test_determined_air_has_no_counterexample(self) -> None:
        ctx = self.ctx(toy_doc("ToyAdd", 0))
        ce, log = AS.search(ctx, [[3, 4, 7, 1], [200, 9, 209, 1]], "t")
        self.assertIsNone(ce)
        self.assertEqual(log["active_bases"], 2)

    def test_loose_air_counterexample_is_confirmed(self) -> None:
        ctx = self.ctx(toy_doc("ToyLoose", 1, loose=True))
        ce, _ = AS.search(ctx, [[3, 4, 7, 1]], "t")
        self.assertIsNotNone(ce)
        self.assertEqual(AS.confirm(ctx, ce.base, ce.other), [0])
        self.assertEqual(ce.base[:2], ce.other[:2])

    def test_confirm_requires_assumptions_and_inputs(self) -> None:
        ctx = self.ctx(toy_doc("ToyLoose", 1, loose=True))
        self.assertIsNone(AS.confirm(ctx, [300, 4, 7, 1], [300, 4, 8, 1]))   # range assumption fails
        self.assertIsNone(AS.confirm(ctx, [3, 4, 7, 1], [3, 5, 8, 1]))       # inputs differ
        self.assertIsNone(AS.confirm(ctx, [3, 4, 7, 0], [3, 4, 8, 0]))       # inactive: equal contributions
        self.assertEqual(AS.confirm(ctx, [3, 4, 7, 1], [3, 4, 8, 1]), [0])

    def test_linear_kernel_moves_through_a_free_column(self) -> None:
        doc = toy_doc("ToyLoose", 1, loose=True)
        doc["nodes"] += [["main", 0, 2], ["sub", 10, 10]]            # a constraint that is identically zero
        doc["constraints"].append(11)
        ctx = self.ctx(doc)
        ce, status = AS.linear_kernel(ctx, [3, 4, 7, 1], __import__("random").Random(1), set(), 5.0)
        self.assertIsNotNone(ce)
        self.assertEqual(status, "found")

    def test_window_counterexample(self) -> None:
        ctx = self.ctx(window_doc())
        lay = ctx.layout
        rows = [[i, 1] for i in range(4)]
        ws = [lay.window(rows, None, [], i, 4) for i in range(4)]
        ce, _ = AS.search(ctx, ws, "t")
        self.assertIsNotNone(ce)                          # x of an interior row is not fixed by its window


class StatusTests(unittest.TestCase):
    def rec(self):
        return {"statement": {"truth": "unknown"}}

    def gates(self, **over):
        g = {k: {"status": "PASS"} for k in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV")}
        g["DET-SEARCH"] = {"status": "PASS", "truth": "unknown"}
        g.update(over)
        return g

    def test_open(self) -> None:
        rec = self.rec()
        D.set_status(rec, self.gates(), True)
        self.assertEqual(rec["status"], "OPEN")

    def test_window_counterexample_is_not_a_finding(self) -> None:
        rec = self.rec()
        D.set_status(rec, self.gates(**{"DET-SEARCH": {"status": "FAIL", "truth": "window-false-counterexample-found",
                                                        "method": "linear-kernel"},
                                        "G-TRIV": {"status": "SKIPPED"}}), True)
        self.assertEqual(rec["status"], "GATE-FAIL")
        self.assertIn("not-a-finding", rec["status_reason"])
        self.assertNotIn("G-TRIV", rec["status_reason"])
        self.assertEqual(rec["statement"]["truth"], "window-false-counterexample-found")

    def test_row_counterexample_is_a_candidate(self) -> None:
        rec = self.rec()
        D.set_status(rec, self.gates(**{"DET-SEARCH": {"status": "FAIL", "truth": "false-counterexample-found",
                                                        "method": "single-variable"}}), True)
        self.assertEqual(rec["status"], "DET-FALSE-CANDIDATE")

    def test_no_outputs_and_closure(self) -> None:
        rec = self.rec()
        D.set_status(rec, self.gates(**{"G-TRIV": {"status": "FAIL", "closed_by": ["triv_V0_simp"]}}), False)
        self.assertEqual(rec["status"], "GATE-FAIL")
        self.assertIn("no output messages", rec["status_reason"])
        self.assertEqual(rec["statement"]["truth"], "closed-by-automation")


class NegativeMultiplicityTests(unittest.TestCase):
    def recursion_doc(self) -> dict:
        """A recursion-style AIR: reads sent with a multiplicity column holding -1, writes with a count column."""
        nodes = [["main", 0, 0], ["main", 0, 1], ["prep", 0, 0], ["prep", 0, 1]]
        return {"format": IR.FORMAT, "zkvm": "pico", "release": "v1", "commit": "0" * 40,
                "field": {"name": "KoalaBear", "p": KB},
                "air": {"name": "RecursionToy", "rust_type": "RecursionToy", "group": "recursion", "index": 0},
                "width": 2, "preprocessed_width": 2, "num_public_values": 0, "meta": {}, "nodes": nodes,
                "constraints": [],
                "interactions": [
                    {"dir": "send", "kind": 1, "kind_name": "Memory", "bus": None, "scope": None, "values": [0],
                     "mult": 2, "count_weight": None},
                    {"dir": "send", "kind": 1, "kind_name": "Memory", "bus": None, "scope": None, "values": [1],
                     "mult": 3, "count_weight": None}]}

    def test_negated_column_reads_are_inputs(self) -> None:
        air = IR.from_json(self.recursion_doc())
        lay = IR.Layout.of(air)
        real = [(0, [5, 6, KB - 1, 2])]                   # prep[0] = -1 (read), prep[1] = 2 reads of the write
        neg = D.negative_multiplicities(air, lay, real)
        self.assertEqual(neg, frozenset({0}))
        roles, iface = B.model_for("pico").roles(air, neg)
        self.assertEqual([m["interaction"] for m in iface["inputs"]], [0])
        self.assertEqual([m["interaction"] for m in iface["outputs"]], [1])
        roles, iface = B.model_for("pico").roles(air)    # without row evidence both look like writes
        self.assertEqual(len(iface["outputs"]), 2)


def mini_air(name: str, interactions: list, group: str = "app-vm", nodes=None, index: int = 0) -> IR.Air:
    nodes = nodes or [["main", 0, 0], ["main", 0, 1], ["const", 0], ["const", 1]]
    return IR.from_json({"format": IR.FORMAT, "zkvm": "toy", "release": "v1", "commit": "0" * 40,
                         "field": {"name": "BabyBear", "p": IR.FIELDS["BabyBear"]},
                         "air": {"name": name, "rust_type": name, "group": group, "index": index},
                         "width": 2, "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": nodes,
                         "constraints": [], "interactions": interactions})


def it(direction: str, bus=None, kind=None, kind_name="", values=(0,), mult=1) -> dict:
    return {"dir": direction, "kind": kind, "kind_name": kind_name, "bus": bus, "scope": None, "values": list(values),
            "mult": mult, "count_weight": None}


class BusModelTests(unittest.TestCase):
    def test_tables(self) -> None:
        sp1, pico, ovm = B.model_for("sp1"), B.model_for("pico"), B.model_for("openvm")
        self.assertTrue(sp1.table_fn("sp1Byte", [0, 3 & 5, 3, 5]))
        self.assertTrue(sp1.table_fn("sp1Byte", [5, 1, 200, 0]))
        self.assertFalse(sp1.table_fn("sp1Byte", [5, 1, 200, 1]))         # MSB rows have c = 0
        self.assertTrue(sp1.table_fn("sp1Byte", [6, 65535, 16, 0]))
        self.assertFalse(sp1.table_fn("sp1Byte", [6, 2, 1, 0]))
        self.assertTrue(pico.table_fn("picoByte", [4, 200 >> 3, 200 & 7, 200, 3]))
        self.assertFalse(pico.table_fn("picoByte", [9, 8, 0, 3, 0]))
        self.assertTrue(pico.table_fn("picoByte", [9, 7, 0, 3, 0]))       # bits in the b position
        self.assertTrue(ovm.table_fn("ovmVarRange", [2 ** 17 - 1, 17]))
        self.assertFalse(ovm.table_fn("ovmVarRange", [1, 18]))
        self.assertTrue(ovm.table_fn("ovmBitwise", [6, 3, 5, 1]))
        self.assertFalse(ovm.table_fn("ovmBitwise", [6, 3, 5, 0]))
        self.assertTrue(ovm.table_fn("ovmRangeTuple", [255, 8191]))

    def test_memory_direction_conventions(self) -> None:
        # SP1 / Pico memory accesses send the previous state and receive the current one
        sp1 = B.model_for("sp1")
        air = mini_air("Add", [it("send", kind=1, kind_name="Memory"), it("receive", kind=1, kind_name="Memory"),
                               it("receive", kind=7, kind_name="State"), it("send", kind=7, kind_name="State")],
                       group="riscv")
        _, iface = sp1.roles(air)
        self.assertEqual([m["interaction"] for m in iface["inputs"]], [0, 2])
        self.assertEqual([m["interaction"] for m in iface["outputs"]], [1, 3])
        # OpenVM memory accesses receive the previous state (a negated count) and send the new one
        ovm = B.model_for("openvm")
        air = mini_air("VmAirWrapper<Rv32BaseAluAdapterAir, BaseAluCoreAir<4, 8>>",
                       [it("receive", bus=1), it("send", bus=1), it("send", bus=6, values=(0, 0, 0, 0))])
        ovm.observe([air, mini_air("BitwiseOperationLookupAir<8>", [it("receive", bus=6, values=(0, 0, 0, 0))])])
        _, iface = ovm.roles(air)
        self.assertEqual([m["interaction"] for m in iface["inputs"]], [0])
        self.assertEqual([m["interaction"] for m in iface["outputs"]], [1])
        self.assertEqual(iface["assumptions"][0]["table"], "ovmBitwise")

    def test_call_buses_are_split(self) -> None:
        pico = B.model_for("pico")
        alu = list(range(13))
        nodes = [["main", 0, 0], ["main", 0, 1]] + [["const", k] for k in range(13)]
        air = mini_air("Add", [it("receive", kind=4, kind_name="Alu", values=[2 + k for k in alu])], group="riscv",
                       nodes=nodes)
        _, iface = pico.roles(air)
        self.assertEqual(iface["inputs"][0]["fields"], [0, 5, 6, 7, 8, 9, 10, 11, 12])
        self.assertEqual(iface["outputs"][0]["fields"], [1, 2, 3, 4])

    def test_global_messages_follow_their_flags(self) -> None:
        sp1 = B.model_for("sp1")
        nodes = [["main", 0, 0], ["main", 0, 1], ["const", 0], ["const", 1]]
        send_flag = [0] * 8 + [3, 2, 0]                  # is_send = 1, is_receive = 0
        recv_flag = [0] * 8 + [2, 3, 0]
        air = mini_air("MemoryLocal", [it("send", kind=9, kind_name="Global", values=send_flag),
                                       it("send", kind=9, kind_name="Global", values=recv_flag)],
                       group="riscv", nodes=nodes)
        _, iface = sp1.roles(air)
        self.assertEqual([m["interaction"] for m in iface["outputs"]], [0])
        self.assertEqual([m["interaction"] for m in iface["inputs"]], [1])

    def test_openvm_bus_names_and_labels(self) -> None:
        ovm = B.model_for("openvm")
        airs = [mini_air("ProgramAir", [it("receive", bus=2)]),
                mini_air("VmConnectorAir", [it("send", bus=0), it("send", bus=2)]),
                mini_air("MemoryMerkleAir<8>", [it("send", bus=4), it("send", bus=5)]),
                mini_air("Poseidon2PeripheryAir<BabyBearParameters, 1>", [it("receive", bus=5)]),
                mini_air("PersistentBoundaryAir<8>", [it("send", bus=1), it("send", bus=4), it("send", bus=5)])]
        ovm.observe(airs)
        self.assertEqual(ovm.bus_names, {2: "Program", 0: "Execution", 5: "Poseidon2", 4: "Merkle", 1: "Memory"})
        self.assertEqual(ovm.chip_type("VmAirWrapper<Rv32BaseAluAdapterAir, BaseAluCoreAir<4, 8>>"), "Rv32BaseAluAir")
        self.assertEqual(ovm.chip_type("VmAirWrapper<Rv32VecHeapAdapterAir<2, 8, 8, 4, 4>, 2, 8, 8, 4, 32, 32>, "
                                       "ShiftCoreAir<32, 8>>"), "Rv32Shift256Air")
        self.assertEqual(ovm.chip_type_at("VmAirWrapper<Rv32VecHeapAdapterAir<2, 8, 8, 4, 4>, FieldExpressionCoreAir>",
                                          38), "ModularAir")
        self.assertEqual(ovm.label(38), "bn254 Fp mul/div")
        self.assertEqual(ovm.label(19), "bls12-381 Fp is-equal")

    def test_openvm_bus_names_are_per_circuit(self) -> None:
        ovm = B.model_for("openvm")
        table = mini_air("BitwiseOperationLookupAir<8>", [it("receive", bus=6, values=(0, 0, 0, 0))])
        user = mini_air("VmAirWrapper<X>", [it("send", bus=6, values=(0, 0, 0, 0))])
        leaf = mini_air("TranscriptAir", [it("send", bus=6, values=(0, 0, 0, 0))], group="recursion-leaf")
        ovm.observe([table, user, leaf])
        self.assertEqual(ovm.roles(user)[1]["assumptions"][0]["table"], "ovmBitwise")
        _, iface = ovm.roles(leaf)
        self.assertEqual(iface["assumptions"], [])
        self.assertEqual(iface["outputs"][0]["bus"], "bus 6")

    def test_chip_types(self) -> None:
        self.assertEqual(B.model_for("sp1").chip_type("Secp256r1DoubleAssignUser"), "WeierstrassDoubleAssignChip")
        self.assertEqual(B.model_for("sp1").chip_type("RecursionWrapExtFeltConvert"), "ConvertChip")
        self.assertEqual(B.model_for("pico").chip_type("LessThan"), "LtChip")
        self.assertEqual(B.model_for("pico").chip_type("Bls381Fp2Mul"), "Fp2MulChip")


class CoverageTests(unittest.TestCase):
    def test_sp1_supervisor_only(self) -> None:
        self.assertEqual(B.coverage("sp1", "Add", "AddChip<SupervisorMode>")["status"], "partial")
        self.assertEqual(B.coverage("sp1", "AddUser", "AddChip<UserMode>")["status"], "none")
        self.assertEqual(B.coverage("sp1", "Keccak", "KeccakPermuteChip")["status"], "none")

    def test_pico_changed_chips_do_not_count(self) -> None:
        mul = B.coverage("pico", "Mul", "MulChip<F>")
        self.assertEqual(mul["status"], "partial")
        self.assertTrue(mul["sources"][0]["same_code"])
        add = B.coverage("pico", "Add", "AddChip<F>")
        self.assertEqual(add["status"], "none")
        self.assertFalse(add["sources"][0]["same_code"])

    def test_openvm(self) -> None:
        self.assertEqual(B.coverage("openvm", "Rv32BaseAluAir", "VmAirWrapper<..>")["status"], "partial")
        self.assertEqual(B.coverage("openvm", "Rv32HintStoreAir", "VmAirWrapper<..>")["status"], "none")


class PackageSchemaTests(unittest.TestCase):
    def record(self, status: str) -> dict:
        air = IR.from_json(toy_doc("ToyAdd", 0))
        rec = {"schema_version": P.AIR_SCHEMA_VERSION, "package_id": "toy-v1/000.ToyAdd",
               "property": dict(D.DET_PROPERTY), "status": status, "status_reason": "fixture",
               "ids": {"ledger_item_id": "sp1:ToyAddChip", "zkvm": "sp1", "repo": "toy/toy",
                       "repo_url": "https://example.invalid/toy", "release": "v1", "commit": "0" * 40,
                       "air_name": "ToyAdd", "rust_type": "ToyAddChip", "air_index": 0, "group": "riscv"},
               "spec": dict(D.DET_SPEC), "coverage": {"status": "none", "sources": []},
               "env": {"lean": "v4.33.1", "mathlib": "c" * 40, "lake_manifest_sha256": "ab" * 32, "python": "3.9",
                       "rust": "rustc 1.98.0"},
               "generator": {"name": D.GENERATOR_NAME, "version": D.GENERATOR_VERSION, "sources_sha256": "ab" * 32}}
        if status == "TOO-LARGE":
            rec["air"] = {"field": "KoalaBear", "p": KB, "width": 4, "preprocessed_width": 0, "num_public_values": 0,
                          "window_rows": 1, "n_constraints": 2, "n_nodes": len(air.nodes), "n_interactions": 3,
                          "ir_sha256": "ab" * 32, "content_sha256": air.content_sha256(),
                          "extractor": {"harness_sha256": "ab" * 32, "builder": "b", "rust_toolchain": "r"},
                          "size_policy": {"max_constraints": 2000, "max_nodes": 40000, "within": False}}
        return rec

    def test_air_records_use_the_air_schema(self) -> None:
        self.assertEqual(P.validate_problem(self.record("EXTRACTION-FAILED")), [])
        self.assertEqual(P.validate_problem(self.record("TOO-LARGE")), [])
        bad = self.record("TOO-LARGE")
        bad["air"]["size_policy"]["within"] = True
        self.assertTrue(any("within" in e for e in P.validate_problem(bad)))
        bad = self.record("EXTRACTION-FAILED")
        bad["status"] = "UNINSTANTIABLE"                  # a circom status is not an AIR status
        self.assertTrue(P.validate_problem(bad))
        bad = self.record("OPEN")                         # packaged status without statement / gates / checker
        self.assertTrue(P.validate_problem(bad))


class HarnessPrepareTests(unittest.TestCase):
    def test_member_is_added_once(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "Cargo.toml").write_text('[workspace]\nresolver = "2"\nmembers = [\n  "crates/a",\n]\n', encoding="utf-8")
            with mock.patch.object(D, "HARNESS_DIR", tmp + "/h"):
                os.makedirs(tmp + "/h/common")
                os.makedirs(tmp + "/h/toy")
                Path(tmp, "h/common/boole_air_ir.rs").write_text("// ir\n", encoding="utf-8")
                Path(tmp, "h/toy/Cargo.toml.in").write_text("[package]\nname = \"boole-air-extract\"\n", encoding="utf-8")
                Path(tmp, "h/toy/main.rs").write_text("fn main() {}\n", encoding="utf-8")
                D.prepare_harness("toy", tmp)
                D.prepare_harness("toy", tmp)
            text = Path(tmp, "Cargo.toml").read_text(encoding="utf-8")
            self.assertEqual(text.count('"boole-air-extract"'), 1)
            self.assertTrue(Path(tmp, "boole-air-extract/src/main.rs").exists())
            self.assertTrue(Path(tmp, "boole-air-extract/src/boole_air_ir.rs").exists())
            self.assertTrue(Path(tmp, "boole-air-extract/Cargo.toml").exists())


class GeneratorInfoTests(unittest.TestCase):
    def test_generator_hash_covers_the_air_sources(self) -> None:
        here = Path(D.HERE)
        listed = set(D.GENERATOR_SOURCES)
        for rel in listed:
            self.assertTrue((here / rel).is_file(), rel)
        air_files = {str(p.relative_to(here)) for p in here.glob("air_*.py")}
        self.assertEqual(air_files - listed, set())
        self.assertRegex(D.generator_info()["sources_sha256"], r"^[0-9a-f]{64}$")


class HarnessModuleTests(unittest.TestCase):
    def test_module_install_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(tmp + "/ws/vm/src")
            Path(tmp, "ws/vm/src/lib.rs").write_text("pub mod chips;\n", encoding="utf-8")
            os.makedirs(tmp + "/h/common")
            os.makedirs(tmp + "/h/toy")
            Path(tmp, "h/common/boole_air_ir.rs").write_text("// ir\n", encoding="utf-8")
            Path(tmp, "h/toy/main.rs").write_text("// extractor\n", encoding="utf-8")
            Path(tmp, "h/toy/install.json").write_text(json.dumps({"mode": "module", "dir": "vm/src",
                                                                   "lib": "vm/src/lib.rs", "module": "boole_air_extract"}),
                                                       encoding="utf-8")
            with mock.patch.object(D, "HARNESS_DIR", tmp + "/h"):
                D.prepare_harness("toy", tmp + "/ws")
                D.prepare_harness("toy", tmp + "/ws")
            lib = Path(tmp, "ws/vm/src/lib.rs").read_text(encoding="utf-8")
            self.assertEqual(lib.count("pub mod boole_air_extract;"), 1)
            self.assertEqual(lib.count("#[cfg(test)]\npub mod boole_air_ir;"), 1)
            self.assertTrue(Path(tmp, "ws/vm/src/boole_air_extract.rs").exists())
            self.assertFalse(Path(tmp, "ws/boole-air-extract").exists())


class OrderTests(unittest.TestCase):
    def test_large_and_small_alternate(self) -> None:
        items = [{"index": i} for i in range(5)]
        order = D.interleave_by_size(items, lambda m: [5, 1, 9, 3, 7][m["index"]])
        self.assertEqual([m["index"] for m in order], [2, 1, 4, 3, 0])


class RowsTests(unittest.TestCase):
    def test_real_windows_from_sparse_rows(self) -> None:
        air = IR.from_json(window_doc())
        lay = IR.Layout.of(air)
        rows = {"height": 8, "main": {"0": [0, 1], "1": [1, 1], "5": [5, 1], "7": [7, 1]}, "prep": {},
                "public_values": []}
        got = D.real_windows(air, lay, rows)
        self.assertEqual([i for i, _ in got], [0, 7])     # row 1's successor and row 5's are missing
        self.assertEqual(got[1][1], [7, 1, 0, 1, 0, 0])


if __name__ == "__main__":
    unittest.main()
