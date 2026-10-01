#!/usr/bin/env python3
"""Recovery R1 (Circom): Lean limits, size policies, probe-min instantiation, DET-MOD mapping and emission (offline)."""
from __future__ import annotations

import copy
import os
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_det as D            # noqa: E402
from zk_registry import circom_detmod as DM        # noqa: E402
from zk_registry import circom_source as cs        # noqa: E402
from zk_registry import instantiation as I         # noqa: E402
from zk_registry import lean_runner as L           # noqa: E402
from zk_registry import package as P               # noqa: E402
from zk_registry import r1cs as R                  # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "detmod"
H = "ab" * 32


def write(root: str, rel: str, text: str) -> None:
    p = Path(root, rel)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


class LeanLimitTests(unittest.TestCase):
    def test_slots_bound_concurrent_lean_processes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tc = Path(tmp, "tc")
            (tc / "bin").mkdir(parents=True)
            log = Path(tmp, "log")
            lean = tc / "bin" / "lean"
            lean.write_text(f"#!/bin/sh\necho start >> {log}\nsleep 0.4\necho end >> {log}\n", encoding="utf-8")
            lean.chmod(0o755)
            env = L.LeanEnv(str(tc), [], "v4.33.1", {}, H, tmp)
            L.set_lean_limits(1, 4096)
            try:
                ths = [threading.Thread(target=L.run_lean, args=(env, [], tmp, 30)) for _ in range(3)]
                for t in ths:
                    t.start()
                for t in ths:
                    t.join()
            finally:
                L.set_lean_limits(None, None)
            self.assertEqual(log.read_text().split(), ["start", "end"] * 3)     # never two at once
            self.assertIsNone(L._LIMITS["slots"])


class SizePolicyTests(unittest.TestCase):
    def test_packaged_record_must_name_a_known_size_policy(self) -> None:
        import test_zk_registry_package as TP
        with tempfile.TemporaryDirectory() as tmp:
            _, rec = TP.make_package(tmp)
            self.assertEqual(P.validate_problem(rec), [])
            big = copy.deepcopy(rec)
            big["circuit"]["size_policy"]["max_constraints"] = 3001
            self.assertIn("unknown size policy of 3001 constraints", P.validate_problem(big))
            saved = P.SIZE_POLICIES
            P.SIZE_POLICIES = saved + (3001,)
            try:
                self.assertEqual(P.validate_problem(big), [])
                big["circuit"]["n_constraints"] = 3002
                self.assertIn("packaged status outside the size policy", P.validate_problem(big))
            finally:
                P.SIZE_POLICIES = saved


class ProbeMinTests(unittest.TestCase):
    SRC = ("pragma circom 2.0.0;\n"
           "template T(n, k) { assert(k >= 2); signal input x[n]; signal output y; y <== x[0] * k; }\n"
           "template U(m) { signal input a; signal output b; b <== a; }\n"
           "template Caller(z) { component t = T(z, 8); }\n")

    def plans(self, tmp: str, probe_min: bool) -> dict:
        write(tmp, "c.circom", self.SRC)
        files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
        return {p.template.name: p for p in I.plan_templates(files, ["c.circom"], "t/r", probe_min=probe_min)}

    def test_probe_min_is_off_by_default(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            self.assertFalse(self.plans(tmp, False)["T"].candidates)

    def test_ascending_candidates_with_grounded_positions_first(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pl = self.plans(tmp, True)
            cands = pl["T"].candidates["probed"]
            self.assertTrue(all(c.first_fit for c in cands))
            # k: the repository passes 8 (grounded first), then the assert's lower bound 2, 3, ...; n from 1
            self.assertEqual([c.args for c in cands[:4]], [("1", "8"), ("2", "8"), ("3", "8"), ("4", "8")])
            self.assertIn("k from repository call-site literals [8]", cands[0].provenance[0])
            self.assertTrue(cands[0].provenance[0].startswith(I.PROBE_MIN_NOTE))
            self.assertEqual([c.args for c in pl["U"].candidates["probed"][:3]], [("1",), ("2",), ("3",)])
            self.assertLessEqual(len(cands), I.PROBE_MIN_CANDIDATES)

    def test_driver_stops_at_the_first_compiling_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pl = self.plans(tmp, True)["U"]
            cfg = D.WaveConfig(repo_dir=tmp, repo_id="t/r", repo_url="https://x", release="r", commit="0" * 40,
                               collection="c", scope_prefix="", exclude=[], circom="c", circom_version_tag="v2.2.3",
                               lean_env="e", node="n", work=os.path.join(tmp, "w"), out=os.path.join(tmp, "o"))
            env = L.LeanEnv("/tc", [], "v4.33.1", {}, H, "/s")
            comps = {"v2.2.3": D.Compiler("v2.2.3", "circom2", "c", "2.2.3", H)}
            sh = D.Shared(cfg, env, cs.scan_repo(tmp, cs.list_circom_files(tmp)), set(), None, H, "2.2.3", "v22",
                          {"name": "g", "version": "1", "sources_sha256": H}, comps)
            seen = []

            def fake(sh_, workdir, include_rel, template, args, full, rule_path=None, compiler=None, tag_template=None):
                seen.append(args)
                if args == ("1",):
                    return {"rc": 1, "include_context": include_rel, "main_sha256": H, "flags": [],
                            "compiler": compiler.tag, "error": "error[T3001]: False assert reached"}
                return {"rc": 0, "include_context": include_rel, "main_sha256": H, "flags": [], "compiler": compiler.tag,
                        "constraints": 5, "wires": 7, "r1cs_sha256": H}
            saved = D.compile_main
            D.compile_main = fake
            try:
                tier, recs = D.size_candidates(sh, pl, os.path.join(tmp, "w"))
            finally:
                D.compile_main = saved
            self.assertEqual(tier, "probed")
            self.assertEqual(seen, [("1",), ("2",)])                 # (3,) and later are never compiled
            chosen, fits = D.select(recs, tier, 2000)
            self.assertEqual((chosen["args"], fits), (["2"], True))


class DetModMappingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.r = R.read_r1cs(str(FIX / "parent.r1cs"))
        self.syms = R.read_sym(str(FIX / "parent.sym"))
        self.tmp = tempfile.TemporaryDirectory()
        write(self.tmp.name, "parent.circom", (FIX / "parent.circom").read_text(encoding="utf-8"))
        self.files = cs.scan_repo(self.tmp.name, cs.list_circom_files(self.tmp.name))
        self.parent = next(t for t in self.files["parent.circom"].templates if t.name == "Parent")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_component_variables_map_to_templates(self) -> None:
        self.assertEqual(DM.component_templates(self.files, self.parent), {"i": "Inner", "z": "Sq"})

    def test_split_keeps_the_parents_own_constraints(self) -> None:
        sp = DM.split(self.r, self.syms, self.files, self.parent)
        names = R.wire_names(self.syms, self.r.n_wires)
        kept_wires = [sorted({names[i] for lc in self.r.constraints[k] for i, _ in lc} - {"one"}) for k in sp.kept]
        self.assertEqual(sorted(kept_wires), sorted([
            ["main.i[0].a", "main.x"], ["main.i[1].a", "main.y"], ["main.z.in"],
            ["main.i[0].b", "main.i[1].b", "main.o"], ["main.q", "main.x", "main.z.out"]]))
        self.assertEqual(sp.stats["dropped_constraints"], self.r.n_constraints - 5)
        # both Inner instances have identical sub-circuits (one kind); the constant-fed Sq is its own kind
        self.assertEqual([(c.instance, c.template, c.kind) for c in sp.calls],
                         [("i[0]", "Inner", 0), ("i[1]", "Inner", 0), ("z", "Sq", 1)])
        self.assertEqual([names[w] for w in sp.calls[0].inputs + sp.calls[0].outputs], ["main.i[0].a", "main.i[0].b"])
        self.assertEqual(sp.instances, 3)
        self.assertEqual(sp.hidden_wires, 7)            # s.in/out/t of both Inner, z.t

    def test_reduce_renumbers_and_keeps_main_io(self) -> None:
        sp = DM.split(self.r, self.syms, self.files, self.parent)
        red = DM.reduce(self.r, sp)
        self.assertEqual(red.wires[:5], [0, 1, 2, 3, 4])
        self.assertEqual(red.r.n_constraints, 5)
        self.assertEqual(red.r.n_wires, 1 + 4 + 2 + 2 + 2)     # one, main io, i[k].a/b, z.in/out

    def test_python_call_semantics(self) -> None:
        sp = DM.split(self.r, self.syms, self.files, self.parent)
        red = DM.reduce(self.r, sp)
        ix = {n: j for j, n in enumerate(R.wire_names(self.syms, self.r.n_wires)[w] for w in red.wires)}
        w = [0] * red.r.n_wires
        w[ix["main.i[0].a"]], w[ix["main.i[0].b"]] = 5, 7
        w[ix["main.i[1].a"]], w[ix["main.i[1].b"]] = 5, 7
        self.assertTrue(DM.calls_respect_own(red.calls, w))
        w2 = list(w)
        w2[ix["main.i[1].b"]] = 8                      # same kind, same input, different output
        self.assertFalse(DM.calls_respect_own(red.calls, w2))
        w3 = list(w)
        w3[ix["main.i[0].b"]] = w3[ix["main.i[1].b"]] = 9
        self.assertTrue(DM.calls_respect_own(red.calls, w3))
        self.assertFalse(DM.pair_consistent(red.calls, w, w3))      # input 5 -> 7 in one, 9 in the other

    def test_not_eligible_parents(self) -> None:
        bad = copy.copy(self.parent)
        bad.body = self.parent.body + "\n    signal leak <== z.t;\n"
        with self.assertRaisesRegex(DM.NotEligible, "not an input/output of Sq"):
            DM.split(self.r, self.syms, self.files, bad)
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "a.circom", "pragma circom 2.1.0;\ntemplate S() { signal input a; signal output b; b <== a; }\n"
                                   "template P() { signal input x; signal output y; y <== S()(x); }\n")
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            p = next(t for t in files["a.circom"].templates if t.name == "P")
            with self.assertRaisesRegex(DM.NotEligible, "anonymous"):
                DM.component_templates(files, p)
            write(tmp, "b.circom", "pragma circom 2.0.0;\ntemplate S() { signal input a; signal output b; b <== a; }\n"
                                   "template S2() { signal input a; signal output b; b <== a; }\n"
                                   "template P() { component c[2]; c[0] = S(); c[1] = S2(); }\n")
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            p = next(t for t in files["b.circom"].templates if t.name == "P")
            with self.assertRaisesRegex(DM.NotEligible, "different templates"):
                DM.component_templates(files, p)
        syms = [R.SymEntry(e.label, e.wire, e.component, e.name.replace("main.z.", "main.w.")) for e in self.syms]
        with self.assertRaisesRegex(DM.NotEligible, "not mapped"):
            DM.split(self.r, syms, self.files, self.parent)


class DetModEmitTests(unittest.TestCase):
    def setUp(self) -> None:
        r = R.read_r1cs(str(FIX / "parent.r1cs"))
        syms = R.read_sym(str(FIX / "parent.sym"))
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "parent.circom", (FIX / "parent.circom").read_text(encoding="utf-8"))
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            parent = next(t for t in files["parent.circom"].templates if t.name == "Parent")
            self.red = DM.reduce(r, DM.split(r, syms, files, parent))
        full = R.wire_names(syms, r.n_wires)
        self.names = [full[w] for w in self.red.wires]
        self.meta = {"repo_id": "t/r", "instantiation": "Parent()", "generator": "g v1", "repo_url": "https://x",
                     "commit": "0" * 40, "path": "parent.circom", "template": "Parent", "rule": "parameter-free",
                     "circom_version": "2.2.3", "circom_flags": ["--O0"], "r1cs_sha256": H, "prime_name": "bn128"}

    def test_model_and_statement(self) -> None:
        text = DM.emit_model("ZkDet.T", self.meta, self.red, [1, 2], [3, 4], self.names)
        self.assertIn("def Calls : List (ℕ × List (Fin nWires) × List (Fin nWires)) := "
                      "[(0, [6], [5]), (0, [8], [7]), (1, [10], [9])]", text)
        self.assertIn("∀ c ∈ Calls, c.2.2.map w = f c.1 (c.2.1.map w)", text)
        self.assertIn("DET-MOD model", text)
        self.assertTrue(text.endswith("end ZkDet.T\n"))
        st = DM.emit_statement("ZkDet.T", self.meta)
        self.assertIn("theorem det_mod [Fact (Nat.Prime p)] :\n    ∀ f : ℕ → List F → List F, ∀ w₁ w₂", st)
        self.assertIn("CallsRespect f w₁ → CallsRespect f w₂ →", st)
        self.assertEqual(st.count("sorry"), 1)

    def test_runner_and_battery(self) -> None:
        run = DM.emit_runner("ZkDet.T", 5, [("real_000", "/x/a.txt")], pair=("/x/a.txt", "/x/b.txt"))
        self.assertIn('IO.println s!"CALLS real_000 {if decide (CallsRespect (tabOf w) w) then "ACCEPT" else '
                      '"REJECT"}"', run)
        self.assertLess(run.index("PAIR"), run.index('IO.println "FID-DONE"'))
        self.assertIn("def tabOf2", run)
        bat = DM.emit_battery_forms("ZkDet.T", 5, [("V3", "grind"), ("V0", "simp")], 200000)
        self.assertIn("intro f w₁ w₂ h₁ h₂ hc₁ hc₂ hin", bat)
        self.assertIn("(try unfold CallsRespect Calls at hc₁ hc₂)", bat)
        self.assertEqual(bat.count("theorem triv_"), 2)
        self.assertEqual(bat.count(DM.SIGNATURE), 2)


class LongModelTests(unittest.TestCase):
    def test_long_block_conjunction_raises_the_recursion_depth(self) -> None:
        from zk_registry import lean_emit as E
        meta = {"repo_id": "t/r", "instantiation": "T()", "generator": "g v1", "repo_url": "https://x",
                "commit": "0" * 40, "path": "t.circom", "template": "T", "rule": "parameter-free",
                "circom_version": "2.2.3", "circom_flags": ["--O0"], "r1cs_sha256": H, "prime_name": "bn128"}

        def model(n: int) -> str:
            cons = [([(1, 1)], [(1, 1)], [(2, 1)])] * n
            r = R.R1cs(R.BN254_SCALAR, 32, 3, 1, 1, 0, 3, cons)
            return E.emit_model("ZkDet.T", meta, r, [1], [2], ["one", "main.o", "main.i"])
        flag = "set_option maxRecDepth 100000 in\n/-- The compiled constraint system"
        self.assertNotIn(flag, model(E.BLOCK * E.LONG_CONJUNCTION))          # 8,192 constraints: unchanged
        self.assertIn(flag, model(E.BLOCK * E.LONG_CONJUNCTION + 1))


if __name__ == "__main__":
    unittest.main()
