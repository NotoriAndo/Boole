#!/usr/bin/env python3
"""halo2 DET generator: offline fixture tests (population filter and row plan, IR parsing and flattening over the
concrete layout — gates with fixed and compressed-selector substitution, rotations, copies, fixed / range /
advice lookup tables —, the independent evaluator, Lean emission, the counterexample search, harness
instrumentation and injection, lockfile deviations, statuses and record validation)."""
from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import air_search as AS               # noqa: E402
from zk_registry import halo2_det as D                 # noqa: E402
from zk_registry import halo2_harness as HH            # noqa: E402
from zk_registry import halo2_ir as H                  # noqa: E402
from zk_registry import halo2_lean_emit as HE          # noqa: E402
from zk_registry import halo2_targets as T             # noqa: E402
from zk_registry import lean_emit as E                 # noqa: E402
from zk_registry import package as P                   # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "halo2"
PALLAS = 0x40000000000000000000000000000000224698fc094cf91b992d30ed00000001
BN = 0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001


def hx(v: int) -> str:
    return f"{v:064x}"


def doc(p=BN, gates=(), lookups=(), perm=(), cycles=(), fixed=(), advice=(), instance=(), notes=(), k=3,
        usable=None, input_notes=()):
    """A synthetic boole-halo2-ir/v1 document (n = 2^k rows)."""
    n = 1 << k
    d = {"format": H.FORMAT, "halo2_line": "test", "k": k, "n": n, "usable_rows": usable if usable else n - 2,
         "num_fixed": 4, "num_advice": 4, "num_instance": 1,
         "field": {"modulus": hex(p), "one": hx(1), "two": hx(2)},
         "meta": {"target": "t", "input_notes": [list(x) for x in input_notes]},
         "gates": [{"name": f"g{i}", "polys": [{"name": f"p{j}", "e": e} for j, e in enumerate(g)]}
                   for i, g in enumerate(gates)],
         "lookups": [{"input": i, "table": t} for i, t in lookups], "perm_columns": [list(c) for c in perm],
         "cycles": [[list(x) for x in c] for c in cycles],
         "fixed": [[c, r, hx(v)] for c, r, v in fixed], "advice": [[c, r, hx(v)] for c, r, v in advice],
         "instance": [[c, r, hx(v)] for c, r, v in instance], "regions": [],
         "notes": [list(x) for x in notes], "verify": []}
    return H.parse(json.loads(json.dumps(d)))


def A(c, rot=0):
    return ["a", c, rot]


def F(c, rot=0):
    return ["f", c, rot]


def mul(a, b):
    return ["mul", a, b]


def add(a, b):
    return ["add", a, b]


def neg(a):
    return ["neg", a]


def adder_doc(c_value=None, b_free=False):
    """q (fixed 0) * (a + b - c) at row 0; a, b copied from instance rows 0, 1; c copied to the output region."""
    gate = [mul(F(0), add(add(A(0), A(1)), neg(A(2))))]
    perm = [["i", 0], ["a", 0], ["a", 1], ["a", 2], ["a", 3]]
    cycles = [[[0, 0], [1, 0]], [[3, 0], [4, 0]]] + ([] if b_free else [[[0, 1], [2, 0]]])
    a, b = 5, 7
    c = a + b if c_value is None else c_value
    return doc(gates=[gate], perm=perm, cycles=cycles, fixed=[(0, 0, 1)],
               advice=[(0, 0, a), (1, 0, b), (2, 0, c), (3, 0, c)], instance=[(0, 0, a), (0, 1, b)],
               notes=[[1, "boole-outputs", 3, 0, "out0"]])


class FilterAndPlan(unittest.TestCase):
    def test_filter(self):
        base = {"framework": "halo2", "unit": "G1-gadget", "flags": []}
        self.assertTrue(D.selected(base))
        self.assertFalse(D.selected(dict(base, framework="plonky3-AIR+halo2")))
        self.assertTrue(D.selected(dict(base, framework="plonky3-AIR+halo2"), "plonky3-AIR+halo2"))
        self.assertFalse(D.selected(dict(base, unit="G2-constraint-site")))
        self.assertFalse(D.selected(dict(base, unit="U1-instruction")))
        self.assertFalse(D.selected(dict(base, flags=["vendored-copy"])))
        self.assertFalse(D.selected(dict(base, flags=["pilot-test"])))

    def row(self, **kw):
        r = {"project": "x/y", "unit": "CC-circuit-component", "path": "src/a.rs", "symbol": "Circuit:Foo",
             "repo": "https://github.com/x/y", "commit": "0" * 40, "census": [{}], "coverage": "none"}
        r.update(kw)
        return r

    def test_plan_rules(self):
        w = T.plan(self.row(project="halo2-gadgets", unit="G1-gadget", symbol="EccChip.add"))
        self.assertIsInstance(w, T.Wrapper)
        self.assertEqual((w.adapter, w.name), ("zcash-gadgets", "EccChip.add"))
        na = T.plan(self.row(project="halo2-gadgets", unit="G1-gadget", symbol="EccChip.witness_point"))
        self.assertEqual(na.status, "NOT-APPLICABLE")
        self.assertIn("todo!", T.plan(self.row(project="halo2-gadgets", unit="G1-gadget",
                                               symbol="EccChip.witness_scalar_var")).reason)
        self.assertEqual(T.plan(self.row(project="halo2-gadgets", unit="G1-gadget",
                                         symbol="LookupRangeCheckConfig.load")).status, "NOT-APPLICABLE")
        self.assertEqual(T.plan(self.row(unit="P2-verifier-entry-point", path="contracts/V.sol")).status,
                         "NOT-APPLICABLE")
        self.assertEqual(T.plan(self.row(unit="P2-verifier-entry-point", symbol="fn:verify")).status,
                         "NOT-APPLICABLE")
        self.assertEqual(T.plan(self.row(unit="P1-component", symbol="recursion:sdk/halo2/aggregation")).status,
                         "NO-INSTANTIATION")
        self.assertEqual(T.plan(self.row(symbol="Circuit:AggregationCircuit")).status, "NO-INSTANTIATION")
        self.assertIn("generic", T.plan(self.row(symbol="Circuit:BaseCircuitBuilder")).reason)
        self.assertEqual(T.plan(self.row(project="darkrenaissance/darkfi", symbol="Chip:ArithChip")).adapter,
                         "darkfi-gadgets")
        self.assertEqual(T.plan(self.row(unit="P3-security-parameter")).status, "NOT-APPLICABLE")


class Flatten(unittest.TestCase):
    def test_byte_order_and_field(self):
        d = json.loads(json.dumps({"format": H.FORMAT, "field": {"modulus": hex(BN), "one": hx(1) [::-1],
                                                                 "two": hx(2)}}))
        with self.assertRaises(H.IrError):
            H.parse(d)
        d["field"]["one"] = hx(1)
        d["field"]["modulus"] = hex(97)
        with self.assertRaises(H.IrError):
            H.parse(d)

    def test_adder_model(self):
        d = adder_doc()
        m = H.flatten(d)
        self.assertEqual(len(m.polys), 1)               # only row 0 has q = 1; other rows fold to 0
        self.assertEqual(len(m.copies), 3)
        self.assertEqual([m.cells[v] for v in m.inputs], [("i", 0, 0), ("i", 0, 1)])
        self.assertEqual([m.cells[v] for v in m.outputs], [("a", 3, 0)])
        w = H.witness(m, d)
        self.assertTrue(H.satisfies(m, w))
        bad = list(w)
        bad[m.outputs[0]] = (bad[m.outputs[0]] + 1) % m.p
        self.assertEqual(H.check(m, bad), ["copy cycle 1"])
        self.assertEqual(H.check(m, H.witness(m, adder_doc(c_value=13))), ["gate 0 'g0' poly 0 'p0' row 0", ])

    def test_rotation_wraps_and_selector_values(self):
        # q * (a[next] - 2 * a[cur]) with q = 3 on the last row: the next row wraps to row 0
        gate = [mul(F(0), add(A(0, 1), neg(["scale", A(0), hx(2)])))]
        d = doc(gates=[gate], fixed=[(0, 7, 3)], advice=[(0, 7, 4), (0, 0, 8)])
        m = H.flatten(d)
        self.assertEqual(len(m.polys), 1)
        self.assertIn(("a", 0, 0), m.cells)
        self.assertTrue(H.satisfies(m, H.witness(m, d)))

    def test_constant_false_item_is_recorded(self):
        d = doc(gates=[[F(1)]], fixed=[(1, 2, 5)])
        m = H.flatten(d)
        self.assertEqual(m.unsat_constants, ["gate 0 'g0' poly 0 'p0' row 2"])
        self.assertFalse(H.satisfies(m, H.witness(m, d)))

    def test_fixed_copy_and_constant_cycle(self):
        d = doc(perm=[["f", 1], ["a", 0]], cycles=[[[0, 3], [1, 2]]], fixed=[(1, 3, 9)], advice=[(0, 2, 9)])
        m = H.flatten(d)
        self.assertEqual(m.copies[0][0], ("c", 9))
        self.assertTrue(H.satisfies(m, H.witness(m, d)))

    def test_range_set_and_advice_tables(self):
        k = 4
        # range table: fixed column 2 = 0..7 on the usable rows; lookup q(f0) * a0 at row 1
        fixed = [(2, r, r) for r in range(8)] + [(0, 1, 1)]
        d = doc(k=k, lookups=[([mul(F(0), A(0))], [F(2)])], fixed=fixed, advice=[(0, 1, 5)])
        m = H.flatten(d)
        self.assertEqual((m.tables[0].kind, m.tables[0].bound), ("range", 8))
        self.assertEqual(len(m.lookups), 1)
        self.assertTrue(H.satisfies(m, H.witness(m, d)))
        w = H.witness(m, d)
        w[m.cells.index(("a", 0, 1))] = 8
        self.assertEqual(len(H.check(m, w)), 1)
        # two-column set table
        fixed2 = [(2, 0, 1), (3, 0, 10), (2, 1, 2), (3, 1, 20), (0, 3, 1)]
        d2 = doc(k=k, lookups=[([mul(F(0), A(0)), mul(F(0), A(1))], [F(2), F(3)])], fixed=fixed2,
                 advice=[(0, 3, 2), (1, 3, 20)])
        m2 = H.flatten(d2)
        self.assertEqual(m2.tables[0].kind, "set")
        self.assertIn((0, 0), m2.tables[0].rows)          # unassigned table rows are zero tuples
        self.assertTrue(H.satisfies(m2, H.witness(m2, d2)))
        # advice-defined table: membership in the tuples of model terms
        d3 = doc(k=3, lookups=[([mul(F(0), A(0))], [A(1)])], fixed=[(0, 0, 1)], advice=[(0, 0, 4), (1, 2, 4)])
        m3 = H.flatten(d3)
        self.assertEqual(m3.tables[0].kind, "advice")
        self.assertTrue(H.satisfies(m3, H.witness(m3, d3)))
        w3 = H.witness(m3, d3)
        w3[m3.cells.index(("a", 1, 2))] = 5
        self.assertFalse(H.satisfies(m3, w3))

    def test_rejections(self):
        with self.assertRaises(H.IrError):
            H.flatten(doc(gates=[[["s", 0]]]))
        with self.assertRaises(H.IrError):
            H.flatten(doc(gates=[[mul(F(0), ["ch", 0])]], fixed=[(0, 0, 1)]))

    def test_input_notes(self):
        d = doc(gates=[[mul(F(0), add(A(0), neg(A(1))))]], fixed=[(0, 0, 1)], advice=[(0, 0, 3), (1, 0, 3)],
                notes=[[0, "swap", 0, 0, "witness b"], [1, "boole-outputs", 1, 0, "out0"]],
                input_notes=[("swap", "witness b")])
        m = H.flatten(d)
        self.assertEqual([m.cells[v] for v in m.inputs], [("a", 0, 0)])

    def test_real_export_fixture(self):
        d0, d1 = H.load(str(FIX / "arith_sample0.json")), H.load(str(FIX / "arith_sample1.json"))
        self.assertEqual(d0["field_name"], "pallas-base")
        self.assertEqual(H.structure_sha256(d0), H.structure_sha256(d1))
        m = H.flatten(d0)
        self.assertEqual((len(m.inputs), len(m.outputs)), (2, 3))
        for d in (d0, d1):
            self.assertTrue(H.satisfies(m, H.witness(m, d)))
        self.assertTrue(m.within_policy())


class LeanText(unittest.TestCase):
    def test_model_statement_and_battery(self):
        m = H.flatten(H.load(str(FIX / "is_equal_sample0.json")))
        meta = {"repo_id": "r/x", "target": "Chip:IsEqualChip", "generator": "g", "repo_url": "u", "commit": "c",
                "path": "p", "symbol": "s", "wrapper": "w", "rule": "repo-test", "call": "c",
                "halo2_line": "zcash-0.3", "ir_sha256": "0", "model_sha256": m.sha256()}
        text, summary = HE.emit_model("ZkDet.T", meta, m)
        self.assertIn("abbrev nWires : ℕ :=", text)
        self.assertIn("def Inputs : List (Fin nWires)", text)
        self.assertIn("def Constraints (w : Fin nWires → F) : Prop :=", text)
        self.assertNotIn("Fin nVars", text)
        st = HE.emit_statement("ZkDet.T", meta)
        self.assertIn(E.theorem_signature(), st)
        bat = HE.emit_battery_forms("ZkDet.T", summary, [("V3", "grind")], 1000)
        self.assertIn("(try simp only [Inputs, Outputs", bat)
        fid = HE.emit_fid_runner("ZkDet.T", summary, [("real_000", "/tmp/w.txt")])
        self.assertIn("loadWitness", fid)

    def test_lookup_items(self):
        fixed = [(2, r, r) for r in range(8)] + [(0, 1, 1)] + [(3, 0, 1), (1, 0, 10), (0, 2, 1)]
        d = doc(k=4, lookups=[([mul(F(0), A(0))], [F(2)]), ([mul(F(0), A(0)), mul(F(0), A(1))], [F(3), F(1)])],
                fixed=fixed, advice=[(0, 1, 5), (0, 2, 1), (1, 2, 10)])
        m = H.flatten(d)
        text, summary = HE.emit_model("ZkDet.T", {k: "x" for k in ("repo_id", "target", "generator", "repo_url",
                                                                   "commit", "path", "symbol", "wrapper", "rule",
                                                                   "call", "halo2_line", "ir_sha256",
                                                                   "model_sha256")}, m)
        self.assertIn(".val < 8", text)
        self.assertIn("∈ Table1", text)
        self.assertRegex(text, rf"set_option maxHeartbeats {HE.TABLE_HEARTBEATS} in\n/--[^\n]*-/\ndef Table1")
        self.assertEqual(summary["tables"], ["Table1"])


class Search(unittest.TestCase):
    def test_free_output_found_and_determined_not(self):
        d = adder_doc(b_free=True)          # b is not tied to its input: the output is not determined
        m = H.flatten(d)
        ctx, ok = D.search_context(m)
        self.assertTrue(ok)
        ce, _ = AS.search(ctx, [H.witness(m, d)], "t", 5.0)
        self.assertIsNotNone(ce)
        self.assertTrue(H.satisfies(m, ce.other))
        self.assertEqual([ce.base[v] for v in m.inputs], [ce.other[v] for v in m.inputs])
        self.assertNotEqual([ce.base[v] for v in m.outputs], [ce.other[v] for v in m.outputs])
        d2 = adder_doc()
        m2 = H.flatten(d2)
        ce2, _ = AS.search(D.search_context(m2)[0], [H.witness(m2, d2)], "t", 5.0)
        self.assertIsNone(ce2)

    def test_advice_tables_not_searched(self):
        d3 = doc(k=3, lookups=[([mul(F(0), A(0))], [A(1)])], fixed=[(0, 0, 1)], advice=[(0, 0, 4), (1, 2, 4)])
        self.assertFalse(D.search_context(H.flatten(d3))[1])


class Harness(unittest.TestCase):
    DEV = ("use std::fmt;\n\npub mod metadata;\nmod util;\n\nimpl<F: Field> Assignment<F> for MockProver<F> {\n"
           "    fn assign_advice<V, VR, A, AR>(\n        &mut self,\n        _: A,\n        column: Column<Advice>,\n"
           "        row: usize,\n        to: V,\n    ) -> Result<(), Error>\n    {\n"
           "        if !self.usable_rows.contains(&row) {\n"
           "            return Err(Error::not_enough_rows_available(self.k));\n        }\n\n"
           "        if let Some(region) = self.current_region.as_mut() {\n"
           "            region.update_extent(column.into(), row);\n"
           "            region.cells.push((column.into(), row));\n        }\n        Ok(())\n    }\n}\n")

    def test_instrument_zcash_shape(self):
        with tempfile.TemporaryDirectory() as t:
            os.makedirs(os.path.join(t, "src", "dev"))
            with open(os.path.join(t, "src", "dev.rs"), "w") as f:
                f.write(self.DEV)
            info = HH.instrument_proofs(t, "zcash-0.3")
            text = Path(os.path.join(t, "src", "dev.rs")).read_text()
            self.assertIn("pub mod boole_export;", text)
            self.assertIn("boole_annotation: A,", text)
            self.assertLess(text.index("boole_export::note_advice("), text.index("if let Some(region)"))
            self.assertTrue(os.path.exists(os.path.join(t, "src", "dev", "boole_export.rs")))
            HH.instrument_proofs(t, "zcash-0.3")                       # idempotent
            self.assertEqual(Path(os.path.join(t, "src", "dev.rs")).read_text().count("note_advice("), 1)
            self.assertEqual(len(info["exporter_sha256"]), 64)
        with tempfile.TemporaryDirectory() as t:
            os.makedirs(os.path.join(t, "src", "dev"))
            with open(os.path.join(t, "src", "dev.rs"), "w") as f:
                f.write("pub mod metadata;\n")
            with self.assertRaises(HH.HarnessError):
                HH.instrument_proofs(t, "zcash-0.3")

    def test_inject_inside_test_module(self):
        with tempfile.TemporaryDirectory() as t:
            os.makedirs(os.path.join(t, "src"))
            with open(os.path.join(t, "src", "circuit.rs"), "w") as f:
                f.write("fn a() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n")
            inj = HH.Injection("zcash_gadgets/boole_h1.rs", "src/circuit/tests/boole_h1.rs", "src/circuit.rs",
                               "mod boole_h1;", "\nmod tests {")
            HH.inject_wrappers(t, [inj], driver_field="pub type Fp = u64;")
            text = Path(os.path.join(t, "src", "circuit.rs")).read_text()
            self.assertIn("mod tests {\n    mod boole_h1;\n", text)
            drv = Path(os.path.join(t, "src", "circuit", "tests", "boole_h1.rs")).read_text()
            self.assertIn("pub type Fp = u64;", drv)
            self.assertNotIn("pasta_curves", drv)

    def test_lock_diff(self):
        before = ('version = 3\n\n[[package]]\nname = "a"\nversion = "1.0.0"\nsource = "registry+x"\n\n'
                  '[[package]]\nname = "b"\nversion = "2.0.0"\nsource = "registry+x"\n')
        after = ('version = 3\n\n[[package]]\nname = "a"\nversion = "1.0.0"\n\n'
                 '[[package]]\nname = "c"\nversion = "0.1.0"\nsource = "registry+x"\n')
        diff = HH._lock_diff(before, after)
        self.assertEqual(diff[0], {"package": "a 1.0.0", "before": "registry+x", "after": ""})
        self.assertEqual(diff[1]["package"], "c 0.1.0")
        self.assertEqual(diff[-1], {"unused_entries_dropped": 1})


class Status(unittest.TestCase):
    def gates(self, triv="PASS", det="PASS", truth="unknown"):
        g = {k: {"status": "PASS"} for k in ("G-ELAB", "G-NONVAC", "G-FID")}
        g["G-TRIV"] = {"status": triv, "closed_by": ["triv_V3_grind"] if triv == "FAIL" else []}
        g["DET-SEARCH"] = {"status": det, "truth": truth, "method": "linear-kernel"}
        return g

    def test_statuses(self):
        rec = {"statement": {"truth": "unknown"}}
        D.set_status(rec, self.gates(), True)
        self.assertEqual(rec["status"], "OPEN")
        D.set_status(rec, self.gates(triv="FAIL"), True)
        self.assertEqual(rec["status"], "GATE-FAIL")
        self.assertEqual(rec["statement"]["truth"], "closed-by-automation")
        rec = {"statement": {"truth": "unknown"}}
        D.set_status(rec, self.gates(triv="SKIPPED", det="SKIPPED"), False)
        self.assertEqual(rec["status"], "GATE-FAIL")
        self.assertIn("no result cells", rec["status_reason"])
        D.set_status(rec, self.gates(det="FAIL", truth="false-counterexample-found"), True)
        self.assertEqual(rec["status"], "DET-FALSE-CANDIDATE")

    def test_static_record_validates(self):
        class Env:
            lean_version, packages, manifest_sha256 = "v4", {"mathlib": "a" * 40}, "0" * 64
        sh = D.Shared.__new__(D.Shared)
        sh.env, sh.ledger_sha, sh.generator = Env(), "1" * 64, {"name": "g", "version": "1",
                                                                "sources_sha256": "2" * 64}
        row = {"item_id": "x/y:CC/a.rs#S", "project": "x/y", "repo": "https://github.com/x/y", "commit": "a" * 40,
               "path": "src/a.rs", "symbol": "S", "unit": "CC-circuit-component", "census": [{"line": 3}],
               "coverage": "partial"}
        rec = D.base_record(sh, row, "3" * 64, True)
        rec["status"], rec["status_reason"] = "NOT-APPLICABLE", "reason"
        self.assertEqual(P.validate_problem(rec), [])
        self.assertEqual(rec["evidence"]["coverage"]["class"], "partial")
        rec2 = json.loads(json.dumps(rec))
        rec2["circuit"] = {"compiler": {"name": "halo2", "version": "v", "flags": [], "binary_sha256": "4" * 64},
                           "prime": str(PALLAS), "prime_name": "pallas-base", "n_constraints": 5000, "n_wires": 9,
                           "halo2": {}, "size_policy": {"max_constraints": D.MAX_CONSTRAINTS, "within": False}}
        rec2["status"] = "TOO-LARGE"
        self.assertEqual(P.validate_problem(rec2), [])

    def test_region_decomposition(self):
        def rec(pid, status, repo, commit, regions):
            return {"package_id": pid, "status": status, "ids": {"repo": repo, "commit": commit},
                    "evidence": {"layout_regions": regions}}
        recs = [rec("o/Circuit", "TOO-LARGE", "zcash/orchard", "a" * 40,
                    {"complete point addition": 3, "load private": 9, "custom": 1}),
                rec("z/EccChip.add", "OPEN", "zcash/halo2", "b" * 40, {"complete point addition": 1}),
                rec("q/Other", "GATE-FAIL", "qed-it/halo2", "c" * 40, {"complete point addition": 1})]
        with tempfile.TemporaryDirectory() as t:
            idx = os.path.join(t, "INDEX.jsonl")
            with open(idx, "w") as f:
                f.write("\n".join(json.dumps(r) for r in recs) + "\n")
            s = D.decompose(idx, os.path.join(t, "dec"))
            self.assertEqual((s["parents"], s["region_edges"], s["mapped_to_wave_records"], s["unmapped"]),
                             (1, 2, 1, 1))
            edges = [json.loads(x) for x in Path(t, "dec", "EDGES.jsonl").read_text().splitlines()]
            kids = {c["package_id"]: c["same_source"] for e in edges for c in e["children"]}
            self.assertEqual(kids, {"z/EccChip.add": True, "q/Other": False})

    def test_dir_names(self):
        self.assertEqual(D.dir_name_for({"path": "src/zk/gadget/is_equal.rs", "symbol": "Chip:IsEqualChip"}),
                         "src.zk.gadget.is_equal.Chip_IsEqualChip")


if __name__ == "__main__":
    unittest.main()
