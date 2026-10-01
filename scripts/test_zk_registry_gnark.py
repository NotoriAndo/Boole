#!/usr/bin/env python3
"""gnark DET generator: offline fixture tests (population filter, harness-result import and the commitment
model, emulated output relation, counterexample search under it, Lean emission, planner and wrapper
rendering, content keys, toolchain selection, statuses)."""
from __future__ import annotations

import json
import random
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import det_search as DS               # noqa: E402
from zk_registry import gnark_decompose as GD          # noqa: E402
from zk_registry import gnark_det as D                 # noqa: E402
from zk_registry import gnark_instantiation as I       # noqa: E402
from zk_registry import gnark_lean_emit as GE          # noqa: E402
from zk_registry import gnark_r1cs as GR               # noqa: E402
from zk_registry import lean_emit as E                 # noqa: E402
from zk_registry import r1cs as R                      # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "gnark"
BN = R.BN254_SCALAR
FRONTEND = I.FRONTEND
EM = I.EMULATED


def T(c):            # constraint term helper: [(wire, coeff)] -> [[str, str]]
    return [[str(w), str(v % BN)] for w, v in c]


def compiled(cons, n_wires, mode="symbolic", bc=0, bw=0, chw=-1, commits=0, outputs=(), challenge=None):
    return {"mode": mode, "n_constraints": len(cons), "n_wires": n_wires, "n_public": 1, "n_secret": 1,
            "public_names": ["1"], "secret_names": ["x"], "constraints": [[T(a), T(b), T(c)] for a, b, c in cons],
            "outputs": list(outputs), "output_paths": [f"r0[{i}]" for i in range(len(outputs))], "groups": [],
            "hints": [], "commits": commits, "boundary_constraints": bc, "boundary_wires": bw, "challenge_wire": chw,
            "committed": 1 if commits else 0, "range_checks": 0, "range_check_bits": 0, "secs": 0.0,
            **({"challenge": str(challenge)} if challenge is not None else {})}


def commitment_result():
    """x (wire 1) input, y (wire 2) output with y = x * x checked through a 'commitment': the symbolic
    compile proves y - x*x = 0 with a challenge wire X (3): t = X * y (4) and t = X * (x*x) via
    u = x * X (5), v = u * x (6), check v = t; degree 1 in X.  Constant compiles at X = 2, 3."""
    pre = [([(1, 1)], [(1, 1)], [(2, 1)])]                       # y = x*x (the honest relation)
    sym_post = [([(3, 1)], [(2, 1)], [(4, 1)]), ([(1, 1)], [(3, 1)], [(5, 1)]), ([(5, 1)], [(1, 1)], [(6, 1)]),
                ([(6, 1)], [(0, 1)], [(4, 1)])]
    sym = compiled(pre + sym_post, 7, bc=1, bw=3, chw=3, commits=1, outputs=[2])
    consts = []
    for X in (2, 3):
        # t = X*y (wire 3), check t = X * x * x via v = x * x (wire 4), t = X v
        post = [([(0, X)], [(2, 1)], [(3, 1)]), ([(1, 1)], [(1, 1)], [(4, 1)]), ([(4, X)], [(0, 1)], [(3, 1)])]
        consts.append(compiled(pre + post, 5, mode="constant", bc=1, bw=3, commits=1, outputs=[2], challenge=X))
    samples = []
    for x in (3, 5, 7):
        y = x * x
        ws = [[str(v) for v in (1, x, y, X * y, x * x)] for X in (2, 3)]
        samples.append({"profile": "uniform", "solved": True, "test_engine": "accepted", "witnesses": ws})
    return {"id": "t", "curve": "bn254", "field": str(BN), "status": "ok", "symbolic": sym, "degree": 1, "points": 2,
            "constant": consts, "model_size": 1 + 6, "samples": samples}


class Population(unittest.TestCase):
    def test_filter(self):
        base = {"framework": "gnark", "unit": "G1-gadget", "flags": []}
        self.assertTrue(D.selected(base))
        self.assertFalse(D.selected(dict(base, framework="noir")))
        self.assertFalse(D.selected(dict(base, unit="CC-constraint-site")))
        self.assertFalse(D.selected(dict(base, unit="C2-operation")))
        self.assertFalse(D.selected(dict(base, flags=["copy-of(x)"])))
        self.assertFalse(D.selected(dict(base, flags=["NOT-ITEMIZED(per-repo-aggregate)"])))
        self.assertTrue(D.selected(dict(base, flags=["other"])))


class HarnessImport(unittest.TestCase):
    def test_no_commitment_model(self):
        res = json.loads((FIX / "harness_e2mul.json").read_text())
        m = GR.build(res)
        self.assertEqual(m.r.n_constraints, 5)
        self.assertEqual(m.inputs, list(range(1, 5)))
        self.assertEqual(len(m.outputs), 2)
        self.assertEqual(len(m.witnesses), 3)
        self.assertTrue(all(R.satisfies(m.r, w) for w in m.witnesses))
        self.assertEqual(m.commitment, {"commits": 0})
        self.assertEqual(m.solver_errors, {})

    def test_commitment_union_of_constant_compiles(self):
        m = GR.build(commitment_result())
        # pre (1) + 2 x post (3)
        self.assertEqual(m.r.n_constraints, 7)
        self.assertEqual(m.r.n_wires, 3 + 2 * 2)
        self.assertEqual(m.commitment["points"], 2)
        self.assertEqual(m.commitment["challenges"], [2, 3])
        self.assertTrue(all(R.satisfies(m.r, w) for w in m.witnesses))
        # wires after the boundary are renumbered per challenge
        self.assertEqual(m.wire_names[3], "challenge 0 wire 0")
        self.assertEqual(m.wire_names[5], "challenge 1 wire 0")

    def test_commitment_rejects_mismatched_degree_and_pre_section(self):
        res = commitment_result()
        res["points"] = 1
        with self.assertRaises(GR.ModelError):
            GR.build(res)
        res = commitment_result()
        res["constant"][1]["constraints"][0][2] = T([(2, 2)])
        with self.assertRaisesRegex(GR.ModelError, "pre-commitment"):
            GR.build(res)

    def test_degree_analysis(self):
        cons = GR.parse_constraints(commitment_result()["symbolic"]["constraints"], BN)
        self.assertEqual(GR.degree(cons, 1, 3, 3), 1)
        sq = cons + [([(3, 1)], [(3, 1)], [(7, 1)]), ([(7, 1)], [(2, 1)], [(8, 1)]), ([(8, 1)], [(0, 1)], [(4, 1), (2, 1)])]
        self.assertEqual(GR.degree(sq, 1, 3, 3), 2)        # X*X*y against X*y + y
        # a solver-computed wire used as a factor (an inverse) is not a polynomial identity
        bad = cons + [([(7, 1)], [(3, 1)], [(0, 1)])]
        with self.assertRaisesRegex(GR.ModelError, "factor"):
            GR.degree(bad, 1, 3, 3)


class EmulatedRelation(unittest.TestCase):
    def model(self):
        # wires: 0 one, 1 x (input), 2 o (output limb), 3 b (free bit); b*b = b; o = x + 5 b
        r = R.R1cs(prime=BN, field_bytes=32, n_wires=4, n_pub_out=0, n_pub_in=0, n_prv_in=1, n_labels=4,
                   constraints=[([(3, 1)], [(3, 1)], [(3, 1)]), ([(2, 1)], [(0, 1)], [(1, 1), (3, 5)])])
        return GR.Model(r=r, inputs=[1], outputs=[2], groups=[{"start": 0, "len": 1, "bits": 64, "modulus": "5",
                                                               "field": "toy", "path": "r0"}],
                        output_paths=["r0.Limbs[0]"], wire_names=["one", "x", "o", "b"], witnesses=[[1, 3, 3, 0]])

    def test_value_comparison_modulo(self):
        m = self.model()
        d = GR.differ(m)
        self.assertEqual(d([1, 3, 3, 0], [1, 3, 8, 1]), [])
        self.assertEqual(d([1, 3, 3, 0], [1, 3, 4, 1]), [2])
        self.assertEqual(m.native_outputs, [])
        self.assertEqual(GR.em_value([1, 2, 3], [1, 2], 4), 2 + 3 * 16)

    def test_search_confirmation_respects_relation(self):
        m = self.model()
        w1, w2 = [1, 3, 3, 0], [1, 3, 8, 1]           # o = x or o = x + 5: both satisfy the constraints
        self.assertEqual(DS.confirm(m.r, w1, w2, m.inputs, m.outputs), [2])          # limb-level DET fails
        self.assertIsNone(DS.confirm(m.r, w1, w2, m.inputs, m.outputs, differ=GR.differ(m)))   # equal mod 5
        ce, _ = DS.search(m.r, [w1], m.inputs, m.outputs, "s", differ=GR.differ(m))
        self.assertIsNone(ce)


class LeanEmission(unittest.TestCase):
    META = {"repo_id": "o/r", "instantiation": "f(x)", "generator": "g", "repo_url": "https://x", "commit": "c" * 40,
            "path": "a.go", "template": "F", "wrapper_id": "w1", "rule": "parameter-free", "call": "f(x)",
            "gnark_version": "v", "go_version": "1.25.7", "curve": "bn254", "commitment": "none", "r1cs_sha256": "0"}

    def test_emulated_statement_and_model(self):
        m = EmulatedRelation().model()
        text = GE.emit_model("ZkDet.T", self.META, m.r, m.inputs, m.native_outputs, m.group_wires(), m.wire_names)
        self.assertIn("def EmulatedOutputs : List (List (Fin nWires) × ℕ × ℕ) := [([2], 64, 5)]", text)
        self.assertIn("def emValue", text)
        st = GE.emit_statement("ZkDet.T", self.META, True)
        self.assertIn("emValue w₁ g.1 g.2.1 % g.2.2 = emValue w₂ g.1 g.2.1 % g.2.2", st)
        self.assertIn("sorry", st)
        plain = GE.emit_statement("ZkDet.T", self.META, False)
        self.assertIn(E.theorem_signature(False), plain)          # the Circom template when nothing is emulated
        bat = GE.emit_battery_forms("ZkDet.T", 2, [("V3", "grind")], 1000, True)
        self.assertIn("EmulatedOutputs", bat)
        fid = GE.emit_fid_runner("ZkDet.T", 2, [("real_000", "/tmp/w")], m.group_wires(), ["real_000"])
        self.assertIn("EMV real_000", fid)
        self.assertIn("EMV-DONE", fid)
        self.assertLess(fid.index("EMV-DONE"), fid.rindex("end ZkDet.T"))


def named(pkg, name, args=None):
    t = {"k": "named", "pkg": pkg, "name": name}
    if args:
        t["args"] = args
    return t


def mini_catalog():
    P = "example.org/m/g"
    var = named(FRONTEND, "Variable")
    api = named(FRONTEND, "API")
    el = named(EM, "Element", [{"k": "tparam", "name": "T"}])
    e2 = named(P, "E2")
    data = {
        "named": {
            f"{FRONTEND}.Variable": {"pkg": FRONTEND, "name": "Variable", "underlying": {"k": "iface", "str": "any"}},
            f"{FRONTEND}.API": {"pkg": FRONTEND, "name": "API", "underlying": {"k": "iface", "str": "interface{...}"}},
            f"{EM}.Element": {"pkg": EM, "name": "Element", "tparams": [{"name": "T", "constraint": "FieldParams"}],
                              "underlying": {"k": "struct"}, "fields": [{"name": "Limbs", "type": {"k": "slice", "elem": var}}]},
            f"{P}.E2": {"pkg": P, "name": "E2", "underlying": {"k": "struct"},
                        "fields": [{"name": "A0", "type": var}, {"name": "A1", "type": var}]},
            f"{P}.Ext": {"pkg": P, "name": "Ext", "underlying": {"k": "struct"},
                         "fields": [{"name": "api", "type": api}]},
            f"{P}.Field": {"pkg": P, "name": "Field", "tparams": [{"name": "T", "constraint": "P"}],
                           "underlying": {"k": "struct"}, "fields": [{"name": "api", "type": api}]},
            f"{P}.Hint": {"pkg": P, "name": "Hint", "underlying": {"k": "func", "str": "func()"}},
            f"{P}.Opt": {"pkg": P, "name": "Opt", "underlying": {"k": "func", "str": "func()"}},
        },
        "funcs": [
            {"kind": "func", "pkg": P, "pkg_name": "g", "name": "NewExt", "file": "g/e.go", "line": 1,
             "sig": {"params": [{"name": "api", "type": api}], "results": [{"name": "", "type": {"k": "ptr", "elem": named(P, "Ext")}}]}},
            {"kind": "func", "pkg": P, "pkg_name": "g", "name": "NewField", "file": "g/f.go", "line": 1,
             "sig": {"tparams": [{"name": "T", "constraint": "P"}], "params": [{"name": "api", "type": api}],
                     "results": [{"name": "", "type": {"k": "ptr", "elem": named(P, "Field", [{"k": "tparam", "name": "T"}])}},
                                 {"name": "", "type": named("", "error")}]}},
        ],
        "instances": [{"generic": f"{P}.NewField", "args": [named(P, "ParamsA")], "file": "g/f_test.go", "line": 9, "test": True},
                      {"generic": f"{P}.Field", "args": [named(P, "ParamsB")], "file": "g/other.go", "line": 3, "test": False}],
        "calls": [],
        "targets": [],
    }
    decls = {
        "Ext.Mul": {"kind": "method", "pkg": P, "pkg_name": "g", "name": "Mul", "file": "g/e.go", "line": 5,
                    "recv": named(P, "Ext"), "recv_ptr": False,
                    "sig": {"params": [{"name": "x", "type": {"k": "ptr", "elem": e2}}, {"name": "y", "type": {"k": "ptr", "elem": e2}}],
                            "results": [{"name": "", "type": {"k": "ptr", "elem": e2}}]}},
        "NewExt": data["funcs"][0],
        "Sum": {"kind": "func", "pkg": P, "pkg_name": "g", "name": "Sum", "file": "g/s.go", "line": 2,
                "sig": {"params": [{"name": "api", "type": api}, {"name": "xs", "type": {"k": "slice", "elem": var}}],
                        "results": [{"name": "", "type": var}], "variadic": True}},
        "SumOpt": {"kind": "func", "pkg": P, "pkg_name": "g", "name": "SumOpt", "file": "g/s.go", "line": 9,
                   "sig": {"params": [{"name": "api", "type": api}, {"name": "xs", "type": {"k": "slice", "elem": var}},
                                      {"name": "opts", "type": {"k": "slice", "elem": named(P, "Opt")}}],
                           "results": [{"name": "", "type": var}], "variadic": True}},
        "WithHint": {"kind": "func", "pkg": P, "pkg_name": "g", "name": "WithHint", "file": "g/h.go", "line": 2,
                     "sig": {"params": [{"name": "api", "type": api}, {"name": "h", "type": named(P, "Hint")},
                                        {"name": "x", "type": var}], "results": [{"name": "", "type": var}]}},
        "Field.Mul": {"kind": "method", "pkg": P, "pkg_name": "g", "name": "Mul", "file": "g/f.go", "line": 7,
                      "recv": named(P, "Field", [{"k": "tparam", "name": "T"}]), "recv_ptr": True,
                      "sig": {"params": [{"name": "a", "type": {"k": "ptr", "elem": el}}, {"name": "b", "type": {"k": "ptr", "elem": el}}],
                              "results": [{"name": "", "type": {"k": "ptr", "elem": el}}]}},
    }
    data["targets"] = [{"path": d["file"], "symbol": s, "line": d["line"], "found": True, "decl": d}
                       for s, d in decls.items()]
    return I.Catalog(data, "/nonexistent"), decls


class Planner(unittest.TestCase):
    def setUp(self):
        self.cat, self.decls = mini_catalog()

    def tgt(self, s):
        return {"found": True, "decl": self.decls[s]}

    def test_gadget_object_receiver(self):
        plan = I.plan_target(self.cat, self.tgt("Ext.Mul"))
        self.assertEqual(plan.kind, "gadget")
        self.assertEqual([c.tier for c in plan.choices], ["parameter-free"])
        self.assertEqual(plan.curve, "BN254")
        w = I.render(self.cat, plan, plan.choices[0], "w000000000000_0")
        self.assertIn("recv := p_g.NewExt(api)", w.source)
        self.assertIn("r0 := recv.Mul(&c.In0, &c.In1)", w.source)
        self.assertIn("return harness.Expose(api, &r0)", w.source)
        self.assertIn('In0 p_g.E2 `gnark:",secret"`', w.source)

    def test_constructor_not_applicable(self):
        with self.assertRaisesRegex(I.NotApplicable, "constructor"):
            I.plan_target(self.cat, self.tgt("NewExt"))

    def test_function_parameter_no_instantiation(self):
        with self.assertRaisesRegex(I.NoInstantiation, "function type"):
            I.plan_target(self.cat, self.tgt("WithHint"))

    def test_slices_are_probed_and_options_omitted(self):
        plan = I.plan_target(self.cat, self.tgt("Sum"))
        self.assertEqual([(c.tier, c.length) for c in plan.choices], [("probed", 2), ("probed", 4)])
        w = I.render(self.cat, plan, plan.choices[1], "w000000000001_1")
        self.assertIn("p_g.Sum(api, c.In1...)", w.source)
        self.assertIn("In1: make([]frontend.Variable, 4)", w.source)
        plan = I.plan_target(self.cat, self.tgt("SumOpt"))
        w = I.render(self.cat, plan, plan.choices[0], "w000000000003_0")
        self.assertIn("p_g.SumOpt(api, c.In1)", w.source)       # the variadic options are omitted

    def test_generic_receiver_from_instantiations(self):
        plan = I.plan_target(self.cat, self.tgt("Field.Mul"))
        tiers = [(c.tier, I.show(c.env["T"])) for c in plan.choices]
        self.assertEqual(tiers, [("repo-test", "g.ParamsA"), ("repo-derived", "g.ParamsB")])
        w = I.render(self.cat, plan, plan.choices[0], "w000000000002_0")
        self.assertIn("recv, errrecv := p_g.NewField[p_g.ParamsA](api)", w.source)
        self.assertIn("p_emulated.Element[p_g.ParamsA]", w.source)


class Keys(unittest.TestCase):
    def rb(self, cat):
        return D.RepoBuild("o/r", "/x", "", "m", "", "1.25", "", "go", "/b", catalog=cat)

    def test_content_key_distinguishes_types(self):
        cat, decls = mini_catalog()
        d1 = dict(decls["Ext.Mul"], sha="a" * 64)
        k1 = D.content_key(self.rb(cat), {"decl": d1})
        self.assertEqual(k1, D.content_key(self.rb(cat), {"decl": dict(d1)}))
        cat.named[f"{d1['pkg']}.E2"]["fields"][0]["type"] = {"k": "basic", "name": "int"}
        self.assertNotEqual(k1, D.content_key(self.rb(cat), {"decl": d1}))


class Decomposition(unittest.TestCase):
    def test_split_key(self):
        self.assertEqual(GD.split_key("github.com/consensys/gnark/std/math/emulated.Field.Mul"),
                         ("github.com/consensys/gnark/std/math/emulated", "Field.Mul"))
        self.assertEqual(GD.split_key("github.com/consensys/gnark/std/math/bits.ToBinary"),
                         ("github.com/consensys/gnark/std/math/bits", "ToBinary"))
        self.assertEqual(GD.split_key("proof-tool.Prove"), ("proof-tool", "Prove"))


class Toolchain(unittest.TestCase):
    def test_gnark_version_deviation_is_semantic(self):
        devs = [{"module": "golang.org/x/sys", "repository": "v0.1.0", "resolved": "v0.2.0"},
                {"module": "github.com/consensys/gnark", "repository": "v0.14.0", "resolved": "v0.16.3"}]
        self.assertEqual([d["module"] for d in D.semantic_deviations(devs)], ["github.com/consensys/gnark"])
        self.assertEqual(D.semantic_deviations(devs[:1]), [])

    def test_wrapper_error_lines(self):
        err = ("wrappers/w0123456789ab_0.go:12:3: undefined: x\n"
               "boolegnarkx/wrappers/w0123456789ac_1.go:4:1: cannot use y\n")
        self.assertEqual(D._ERR_RE.findall(err), [("w0123456789ab_0", "undefined: x"),
                                                 ("w0123456789ac_1", "cannot use y")])

    def test_go_requirement_and_pick(self):
        self.assertEqual(D.go_requirement("module m\n\ngo 1.26.0\n\ntoolchain go1.27.0\n"), ("1.26.0", "go1.27.0"))
        cfg = D.WaveConfig("c", "l", {}, {"a": {"goroot": "/a", "version": "1.25.7", "installed": True, "source": "i"},
                                           "b": {"goroot": "/b", "version": "1.26.8", "installed": False, "source": "d"}},
                           "/s", "/e", "/o", "/w", "/b")
        self.assertEqual(D.pick_go(cfg, "1.22"), "a")
        self.assertEqual(D.pick_go(cfg, "1.25.13"), "b")
        with self.assertRaises(RuntimeError):
            D.pick_go(cfg, "1.28")


class Statuses(unittest.TestCase):
    def rec(self, rule="parameter-free"):
        return {"instantiation": {"rule": rule}, "statement": {"truth": "unknown"}}

    def gates(self, **kw):
        g = {k: {"status": "PASS"} for k in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV")}
        g["DET-SEARCH"] = {"status": "PASS", "truth": "unknown"}
        g.update(kw)
        return g

    def test_open_vacuous_and_probed_counterexample(self):
        r = self.rec()
        D.set_status(r, self.gates(), True)
        self.assertEqual(r["status"], "OPEN")
        r = self.rec()
        D.set_status(r, self.gates(**{"G-TRIV": {"status": "SKIPPED"}}), False)
        self.assertEqual(r["status"], "GATE-FAIL")
        self.assertIn("vacuous", r["status_reason"])
        r = self.rec("probed")
        D.set_status(r, self.gates(**{"DET-SEARCH": {"status": "FAIL", "truth": "false-counterexample-found",
                                                     "method": "linear-kernel"}}), True)
        self.assertEqual(r["status"], "DET-FALSE-CANDIDATE")
        self.assertIn("not-a-finding", r["status_reason"])

    def test_skipped_search_is_not_open(self):
        r = self.rec()
        D.set_status(r, self.gates(**{"DET-SEARCH": {"status": "SKIPPED", "truth": "unknown"}}), True)
        self.assertEqual(r["status"], "GATE-FAIL")

    def test_scrub(self):
        self.assertNotIn(str(Path.home()), D.scrub_text(None, f"{Path.home()}/x/y.go:3"))


if __name__ == "__main__":
    unittest.main()
