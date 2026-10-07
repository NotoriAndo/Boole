#!/usr/bin/env python3
"""gnark ratchet problems: the non-linear metric over the registry model, candidate admissibility (Go source rules)
and the call renaming in the registry wrapper, statement generation (native and emulated outputs), the simulation
screen and the checker paths, on the toy fixture ``fixtures/zk-registry/ratchet-gnark``.

The toy gadget ``booletoy.Toy(api, a, b)`` computes ``2 * a^2 * b`` as ``a*a*b + a*(a*b)`` (record: 4 non-linear
constraints).  Candidates: ``equivalent`` (``(a*a) * (b+b)``, 2; its committed fixture proof ``Solution.lean``
proves the generated statement), ``nonequivalent`` (``a*a*b``, caught by the screen) and ``underconstrained`` (the
square is an unconstrained hint: it passes the screen and its `sorry` proof is rejected).  The compiled fixtures
(``compiled/<name>.json``: ``gnarkx run`` results; ``compiled/<name>.sim.jsonl``: ``gnarkx simulate`` outputs, seed
``fixture-seed``, 24 random vectors) were produced by the harness with Go 1.25.7 against the gnark v0.16.3 module
snapshot with ``booletoy/toy.go`` added under ``std/``.

Offline tests need neither Go nor Lean.  ``LiveToolchainTests`` re-runs the pipeline with the real tools when
``BOOLE_ZK_RATCHET_GNARK_TOOLS`` (a directory with ``go1.25.7/``, the official Go release),
``BOOLE_ZK_RATCHET_GNARK_SNAPSHOT`` (a gnark module snapshot made by ``ratchet_gnark.make_snapshot`` at commit cfc7b2f9)
and ``BOOLE_ZK_RATCHET_LEAN_ENV`` (lean_runner environment JSON) are set; it is skipped otherwise."""
from __future__ import annotations

import copy
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import check as C              # noqa: E402
from zk_registry import gates as G              # noqa: E402
from zk_registry import gnark_det as GD         # noqa: E402
from zk_registry import gnark_lean_emit as GE   # noqa: E402
from zk_registry import gnark_r1cs as GR        # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import mech_r1cs as M          # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import r1cs as R               # noqa: E402
from zk_registry import ratchet as RT           # noqa: E402
from zk_registry import ratchet_gnark as RG     # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "ratchet-gnark"
CANDS = FIX / "candidates"
COMPILED = FIX / "compiled"
H = "ab" * 32
WID = "w0b0e1e70a700_0"
TOY_PATH = "std/booletoy/toy.go"
REPO_ID, REPO_URL, COMMIT = "fixture/ratchet-gnark-toy", "https://example.invalid/ratchet-gnark-toy", "0" * 40
GO_PINS = {"go": "1.25.7", "go_sha256": H, "go_compile": "pkg/tool/darwin_arm64/compile", "go_compile_sha256": H}
ENV = {"lean": "v4.33.1", "mathlib": "c" * 40, "lake_manifest_sha256": H, "packages": {"mathlib": "c" * 40},
       "python": "3.9", "gnark": "pinned checkout", "go": "1.25.7 (fixture)"}
SIM_SEED = "fixture-seed"
NAMES = ("reference", "equivalent", "nonequivalent", "underconstrained")
LIVE_TOOLS = os.environ.get("BOOLE_ZK_RATCHET_GNARK_TOOLS")
LIVE_SNAPSHOT = os.environ.get("BOOLE_ZK_RATCHET_GNARK_SNAPSHOT")
LIVE_LEAN = os.environ.get("BOOLE_ZK_RATCHET_LEAN_ENV")


def wrapper_text() -> str:
    return (FIX / "wrapper.go").read_text(encoding="utf-8")


def harness(name: str) -> dict:
    return json.loads((COMPILED / f"{name}.json").read_text(encoding="utf-8"))


def model_of(name: str) -> GR.Model:
    return GR.build(harness(name))


def sim(name: str) -> tuple[dict, list[dict]]:
    return RG.read_sim(str(COMPILED / f"{name}.sim.jsonl"))


def fixture_snapshot(base: str, dest: str) -> str:
    """A gnark module snapshot with the toy package added (``std/booletoy``); returns its manifest digest."""
    shutil.copytree(base, dest)
    RT.put(os.path.join(dest, "repo", TOY_PATH), (FIX / "booletoy" / "toy.go").read_text(encoding="utf-8"))
    return RT.write_manifest(dest)


def fake_snapshot(root: str) -> tuple[str, str]:
    """An offline stand-in for the module snapshot: go.mod, the toy package and the package directories the fixture
    candidates import (no Go code is compiled offline)."""
    sid = "fixture__ratchet-gnark-toy@000000000000"
    dst = os.path.join(root, "snapshots", sid)
    RT.put(os.path.join(dst, "repo", "go.mod"), "module github.com/consensys/gnark\n\ngo 1.25.7\n")
    RT.put(os.path.join(dst, "repo", TOY_PATH), (FIX / "booletoy" / "toy.go").read_text(encoding="utf-8"))
    for d in ("frontend", "constraint/solver", "vendor/github.com/consensys/gnark-crypto/ecc"):
        RT.put(os.path.join(dst, "repo", d, "doc.go"), f"package {d.rsplit('/', 1)[-1]}\n")
    RT.write_manifest(dst)
    return dst, sid


def registry_meta(ns_res: dict, model: GR.Model) -> dict:
    return {"repo_id": REPO_ID, "instantiation": "booletoy.Toy(api, c.In1, c.In2)",
            "generator": f"{GD.GENERATOR_NAME} v{GD.GENERATOR_VERSION}", "repo_url": REPO_URL, "commit": COMMIT,
            "path": TOY_PATH, "template": "Toy", "wrapper_id": WID, "rule": "parameter-free",
            "call": "booletoy.Toy(api, c.In1, c.In2)", "gnark_version": "pinned checkout", "go_version": "1.25.7",
            "curve": ns_res["curve"], "commitment": "no commitment",
            "r1cs_sha256": RT.sha(R.encode_r1cs(model.r))}


def registry_package(root: str, env: dict | None = None, type_sha: str = H, res: dict | None = None) -> str:
    """A registry gnark DET package of the toy reference, built with the wave's emitters from a harness result (the
    committed one by default)."""
    res = res or harness("reference")
    model = GR.build(res)
    ns = P.lean_namespace("ratchet-gnark-fixture", "booletoy.Toy")
    meta = registry_meta(res, model)
    text = GE.emit_model(ns, meta, model.r, model.inputs, model.native_outputs, model.group_wires(), model.wire_names)
    statement = GE.emit_statement(ns, meta, False)
    pkg = os.path.join(root, "registry", "booletoy.Toy")
    model_rel = GE.model_relpath(ns)
    RT.put(os.path.join(pkg, model_rel), text)
    RT.put(os.path.join(pkg, "Statement.lean"), statement)
    RT.put(os.path.join(pkg, "evidence", "wrapper.go"), f"// wrapper compiled against {REPO_ID} at {COMMIT}\n"
           + wrapper_text())
    P.write_json(os.path.join(pkg, "evidence", "harness_result.json"), {k: v for k, v in res.items() if k != "samples"})
    sym = res["symbolic"]
    rec = {
        "schema_version": P.SCHEMA_VERSION, "package_id": "ratchet-gnark-fixture/booletoy.Toy",
        "property": dict(P.DET_PROPERTY), "status": "OPEN", "status_reason": "fixture",
        "ids": {"ledger_item_id": f"{REPO_ID}:{TOY_PATH}#Toy", "repo": REPO_ID, "repo_url": REPO_URL, "release": "v1",
                "commit": COMMIT, "path": TOY_PATH, "template": "Toy", "template_line": 7, "source_sha256": H},
        "instantiation": {"rule": "parameter-free", "args": [], "call": meta["call"], "params": [],
                          "provenance": [], "selection": "only candidate",
                          "candidates": [{"tier": "parameter-free", "call": meta["call"], "compile": "ok"}]},
        "spec": dict(GD.GNARK_DET_SPEC),
        "circuit": {"compiler": {"name": "gnark", "version": "pinned checkout", "binary_sha256": H,
                                 "flags": ["frontend.Compile", "r1cs.NewBuilder (wrapped: model builder)",
                                           "field bn254"],
                                 "source": "fixture"},
                    "prime": res["field"], "prime_name": res["curve"], "n_constraints": model.r.n_constraints,
                    "n_wires": model.r.n_wires, "n_inputs": len(model.inputs), "n_outputs": len(model.outputs),
                    "r1cs_sha256": meta["r1cs_sha256"], "size_policy": {"max_constraints": P.MAX_CONSTRAINTS,
                                                                       "within": True},
                    "gnark": {"production_constraints": (res.get("production") or {}).get("n_constraints"),
                              "commitments": sym["commits"], "wrapper_id": WID, "curve": res["curve"]}},
        "statement": {"file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                      "model_module": GE.model_module(ns), "model_file": model_rel, "text": statement,
                      "assumptions": GD.statement_assumptions(res["curve"], model.commitment, model.hints, False),
                      "truth": "unknown"},
        "gates": {g: {"status": "PASS"} for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV", "DET-SEARCH")},
        "checker": {"statement_file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                    "lean_opts": list(GE.LEAN_OPTIONS),
                    "files": [{"path": "Statement.lean", "role": "statement", "sha256": RT.sha(statement.encode())},
                              {"path": model_rel, "role": "import", "module": GE.model_module(ns),
                               "sha256": RT.sha(text.encode())}],
                    "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                    "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS},
        "env": dict(env or ENV), "generator": {"name": GD.GENERATOR_NAME, "version": GD.GENERATOR_VERSION,
                                                 "sources_sha256": H},
        "evidence": {"hints": model.hints, "commitment": model.commitment},
    }
    P.write_json(os.path.join(pkg, "problem.json"), rec)
    return pkg


def fixture_det(reg: str, verdict: dict | None = None) -> dict:
    """DET evidence of the toy reference: the battery P3 mechanical proof."""
    rep = M.solve(reg, os.path.join(os.path.dirname(reg), "det", "Solution.lean"))
    assert rep["propagation"] == "DETERMINED", rep
    return RT.det_record("mech-p3", RG.reference_spec(reg), os.path.join(os.path.dirname(reg), "det", "Solution.lean"),
                         verdict or {"verdict": "PASS"}, "fixture")


def offline_problem(root: str) -> tuple[str, dict]:
    """The toy ratchet problem assembled from the committed fixtures (``build_problem`` does the same after a rebuild
    of the reference; the live tests compare the two)."""
    reg = registry_package(root)
    spec = RG.reference_spec(reg)
    meas = RG.measure_reference(reg, spec)
    snap, sid = fake_snapshot(root)
    layout = RG.package_layout(snap, "", TOY_PATH)
    wrapper = {"file": "reference/wrapper.go", "sha256": RT.sha(spec["wrapper"].encode()), "id": WID, "note": "fixture"}
    snapshot = {"id": sid, "manifest_sha256": P.sha256_file(os.path.join(snap, "MANIFEST.sha256")),
                "n_files": len(RT.manifest(snap)), "rule": "fixture"}
    prob = RG.problem_record(spec, layout, meas, fixture_det(reg), snapshot, wrapper, GO_PINS)
    prob["simulate"]["seed"] = SIM_SEED
    pdir = os.path.join(root, "problems", "toy")
    RT.put(os.path.join(pdir, spec["model"]["file"]), Path(reg, spec["model"]["file"]).read_text(encoding="utf-8"))
    RT.put(os.path.join(pdir, "reference", "wrapper.go"), spec["wrapper"])
    P.write_json(os.path.join(pdir, "problem.json"), prob)
    return pdir, prob


def compiled(name: str) -> dict:
    """A compile result of a committed fixture (the shape ``compile_program`` returns, without the binary)."""
    res = harness(name)
    model = GR.build(res)
    return {"result": res, "model": model, "counts": RG.counts_of(model.r), "facts": RG.harness_facts(res, model),
            "io": RG.io_signature(res), "r1cs_sha256": RT.sha(R.encode_r1cs(model.r))}


def candidate_record(prob: dict, name: str) -> tuple[dict, dict]:
    res = compiled(name)
    text = (CANDS / f"{name}.go").read_text(encoding="utf-8")
    cand = {"source_sha256": RT.sha(text.encode()), "local_files": ["Candidate.go"], "r1cs_sha256": res["r1cs_sha256"],
            "n_wires": res["model"].r.n_wires, **RG.score(prob["record"], res["counts"]),
            **{k: res["facts"][k] for k in ("production_constraints", "commitments", "challenge_points",
                                            "range_check_bits", "hint_wires")}}
    return cand, res


def candidate_package(pdir: str, prob: dict, name: str, out: str) -> tuple[str, str]:
    cand, res = candidate_record(prob, name)
    pkg = os.path.join(out, "pkg")
    stmt, files = RG.write_candidate_package(prob, pdir, cand, res, pkg)
    P.write_json(os.path.join(pkg, "problem.json"), RG.candidate_problem(prob, cand, files, stmt, H))
    return pkg, stmt


def screen(name: str) -> dict:
    ra, a = sim("reference")
    rb, b = sim(name)
    return RG.compare_runs(ra, a, rb, b, model_of("reference"), model_of(name))


def fake_env(root: str) -> L.LeanEnv:
    return L.LeanEnv(os.path.join(root, "no-toolchain"), [], ENV["lean"], {"mathlib": ENV["mathlib"]},
                     ENV["lake_manifest_sha256"], os.path.join(root, "scratch"))


def go_source(body: str, package: str = "booletoy", imports: str = 'import "github.com/consensys/gnark/frontend"\n') \
        -> str:
    return f"package {package}\n\n{imports}\n{body}"


TOY_FN = ("func BooleRatchetCandidate(api frontend.API, a, b frontend.Variable) frontend.Variable {\n"
          "\treturn api.Mul(a, b)\n}\n")


class MetricTests(unittest.TestCase):
    def test_toy_counts(self) -> None:
        got = {n: RG.counts_of(model_of(n).r) for n in NAMES}
        self.assertEqual({n: c["nonlinear"] for n, c in got.items()},
                         {"reference": 4, "equivalent": 2, "nonequivalent": 2, "underconstrained": 1})
        self.assertEqual({n: c["linear"] for n, c in got.items()},
                         {"reference": 1, "equivalent": 1, "nonequivalent": 1, "underconstrained": 1})
        rec = got["reference"]
        self.assertEqual(RG.score(rec, got["equivalent"])["smaller"], True)
        self.assertEqual(RG.score(rec, got["equivalent"])["reduction_pct"], 50.0)
        self.assertFalse(RG.score(rec, rec)["smaller"])

    def test_linear_and_constant_factors_are_free(self) -> None:
        p = R.BN254_SCALAR
        r = R.R1cs(p, 32, 5, 0, 2, 0, 5, [([(1, 1)], [(0, 1)], [(3, 1)]),          # a * 1 = c: linear
                                           ([(1, 2), (2, 1)], [(0, 5)], [(4, 1)]),   # (2a + b) * 5 = d: linear
                                           ([(1, 1)], [(2, 1), (0, 3)], [(4, 1)]),   # a * (b + 3) = d: non-linear
                                           ([], [(1, 1)], [(3, 1)])])                # empty factor: linear
        self.assertEqual(RG.counts_of(r), {"nonlinear": 1, "linear": 3, "total": 4})

    def test_facts_report_production_count_hints_and_commitment(self) -> None:
        f = RG.harness_facts(harness("underconstrained"), model_of("underconstrained"))
        self.assertEqual((f["commitments"], f["challenge_points"], f["range_check_bits"]), (0, 0, 0))
        self.assertEqual(f["hint_wires"], 2)                         # the square hint and the exposure hint
        self.assertEqual(f["production_constraints"], harness("underconstrained")["production"]["n_constraints"])


class AdmissibilityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)
        self.snap = os.path.join(self.tmp, "snapshots", self.prob["snapshot"]["id"])

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def admit(self, text: str) -> str:
        path = os.path.join(self.tmp, "c", "Candidate.go")
        RT.put(path, text)
        return RG.admit_candidate(path, self.prob, self.snap)

    def test_fixture_candidates_are_admitted(self) -> None:
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            self.assertTrue(self.admit((CANDS / f"{name}.go").read_text(encoding="utf-8")), name)
        ref = self.prob["reference"]
        self.assertEqual((ref["kind"], ref["name"], ref["receiver_type"], ref["package_name"], ref["package_dir"]),
                         ("function", "Toy", None, "booletoy", "std/booletoy"))

    def test_rejections(self) -> None:
        bad = {
            "wrong package": go_source(TOY_FN, package="frontend"),
            "missing": go_source("func Other() {}\n"),
            "twice": go_source(TOY_FN + TOY_FN),
            "method": go_source("type T struct{}\n\nfunc (t T) BooleRatchetCandidate(api frontend.API, a, b "
                                "frontend.Variable) frontend.Variable {\n\treturn a\n}\n"),
            "os": go_source(TOY_FN, imports='import (\n\t"os"\n\t"github.com/consensys/gnark/frontend"\n)\n'),
            "unsafe": go_source(TOY_FN, imports='import (\n\t_ "unsafe"\n\t"github.com/consensys/gnark/frontend"\n)\n'),
            "reflect": go_source(TOY_FN, imports='import (\n\tr "reflect"\n'
                                                 '\t"github.com/consensys/gnark/frontend"\n)\n'),
            "cgo": go_source(TOY_FN, imports='import "C"\nimport "github.com/consensys/gnark/frontend"\n'),
            "harness": go_source(TOY_FN, imports='import (\n\t"github.com/consensys/gnark/boolegnarkx/harness"\n'
                                                 '\t"github.com/consensys/gnark/frontend"\n)\n'),
            "outside": go_source(TOY_FN, imports='import (\n\t"example.invalid/x"\n'
                                                 '\t"github.com/consensys/gnark/frontend"\n)\n'),
            "linkname": go_source("//go:linkname x runtime.x\n" + TOY_FN),
            "build tag": "//go:build ignore\n\n" + go_source(TOY_FN),
            "plus build": "// +build ignore\n\n" + go_source(TOY_FN),
        }
        for what, text in bad.items():
            with self.assertRaises(RG.Reject, msg=what):
                self.admit(text)

    def test_comments_and_strings_do_not_count(self) -> None:
        text = go_source("// func BooleRatchetCandidate() {}\nvar s = `func BooleRatchetCandidate(`\n"
                         "var t = \"func BooleRatchetCandidate(\"\n\n/* func BooleRatchetCandidate */\n" + TOY_FN)
        self.assertTrue(self.admit(text))
        lit = go_source("var f = func() {}\n\nvar g =\n\tfunc() {}\n\n" + TOY_FN)
        self.assertTrue(self.admit(lit))
        multi = go_source(TOY_FN, imports='import (\n\t"math/big"\n\tfr "github.com/consensys/gnark/frontend"\n)\n')
        self.assertTrue(self.admit(multi.replace("frontend.", "fr.").replace("var _ = big.NewInt", "")))

    def test_call_renaming(self) -> None:
        w = wrapper_text()
        out = RG.candidate_wrapper(w, "Toy")
        self.assertIn("r0 := p_booletoy.BooleRatchetCandidate(api, c.In1, c.In2)", out)
        self.assertEqual(out.replace("BooleRatchetCandidate", "Toy"), w)
        meth = w.replace("r0 := p_booletoy.Toy(api, c.In1, c.In2)",
                         "recv := p_booletoy.New(api)\n\tr0 := recv.Add(c.In1)")
        self.assertEqual(RG.call_site(meth, "Add")[0], "method")
        self.assertIn("r0 := recv.BooleRatchetCandidate(c.In1)", RG.candidate_wrapper(meth, "Add"))
        gen = w.replace("p_booletoy.Toy(api,", "p_booletoy.Toy[p_x.T](api,")
        self.assertIn("p_booletoy.BooleRatchetCandidate[p_x.T](api,", RG.candidate_wrapper(gen, "Toy"))
        circ = w.replace("r0 := p_booletoy.Toy(api, c.In1, c.In2)\n\treturn harness.Expose(api, &r0)",
                         "if err := c.In0.Define(api); err != nil {\n\t\treturn err\n\t}\n\treturn harness.Expose(api)")
        self.assertEqual(RG.call_site(circ, "Define")[0], "circuit")
        self.assertIn("c.In0.BooleRatchetCandidate(api)", RG.candidate_wrapper(circ, "Define"))
        twice = w.replace("return harness.Expose(api, &r0)", "_ = p_booletoy.Toy(api, c.In1, c.In1)\n\treturn "
                                                             "harness.Expose(api, &r0)")
        with self.assertRaises(ValueError):
            RG.candidate_wrapper(twice, "Toy")

    def test_build_errors_name_the_candidate_file(self) -> None:
        r = L.RunResult(1, "# github.com/consensys/gnark/std/booletoy\n"
                           "../../../work/x/compile/Candidate.go:4:40: undefined: frontend\n"
                           "/abs/work/x/tool/wrappers/w0b0e1e70a700_0.go:3:2: too many errors\n", 1.0, False)
        msg = RG.build_error(r, "/abs/work/x", "/abs/snap")
        self.assertEqual(msg, "Candidate.go:4:40: undefined: frontend | wrapper w0b0e1e70a700_0.go:3:2: too many errors")

    def test_io_must_match(self) -> None:
        io = self.prob["reference"]["io"]
        self.assertIsNone(RG.io_mismatch(io, RG.io_signature(harness("equivalent"))))
        other = copy.deepcopy(harness("equivalent"))
        other["symbolic"]["output_paths"] = ["r0", "r1"]
        self.assertIn("output paths", RG.io_mismatch(io, RG.io_signature(other)))
        other = copy.deepcopy(harness("equivalent"))
        other["symbolic"]["secret_names"] = ["In1"]
        self.assertIn("secret names", RG.io_mismatch(io, RG.io_signature(other)))


class ReferenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_spec_and_measure(self) -> None:
        reg = registry_package(self.tmp)
        spec = RG.reference_spec(reg)
        self.assertIsNone(RG.static_exclusion(spec))
        meas = RG.measure_reference(reg, spec)
        self.assertEqual(meas["counts"], {"nonlinear": 4, "linear": 1, "total": 5})
        self.assertEqual((meas["io"]["n_inputs"], meas["io"]["n_outputs"], meas["io"]["groups"]), (2, 1, []))
        self.assertEqual(RG.target_name(spec), "Toy")

    def test_model_provenance_is_checked(self) -> None:
        reg = registry_package(self.tmp)
        prob = json.loads(Path(reg, "problem.json").read_text(encoding="utf-8"))
        prob["circuit"]["r1cs_sha256"] = H
        P.write_json(os.path.join(reg, "problem.json"), prob)
        with self.assertRaises(RG.NotEligible) as cm:
            RG.measure_reference(reg)
        self.assertEqual(cm.exception.code, "provenance")
        reg2 = registry_package(os.path.join(self.tmp, "b"))
        spec = RG.reference_spec(reg2)
        spec["generator"] = {"name": "other", "version": "0"}       # the model text no longer re-renders
        with self.assertRaises(RG.NotEligible):
            RG.measure_reference(reg2, spec)

    def test_static_exclusions(self) -> None:
        reg = registry_package(self.tmp)
        spec = RG.reference_spec(reg)
        self.assertEqual(RG.static_exclusion(dict(spec, property="DET-MOD"))[0], "property")
        self.assertEqual(RG.static_exclusion(dict(spec, wrapper=None))[0], "provenance")
        self.assertEqual(RG.static_exclusion(dict(spec, symbol="Other"))[0], "provenance")

    def test_battery_det_solution_follows_the_text_rules(self) -> None:
        reg = registry_package(self.tmp)
        stmt = Path(reg, "Statement.lean").read_text(encoding="utf-8")
        sol = RG.battery_det_solution(stmt, 5, False, "triv_V4_grind")
        probs, aux, body = C.text_check(stmt, sol, "det")
        self.assertEqual(probs, [])
        self.assertEqual(C.forbidden_scan([("helper declarations", aux), ("proof", body)]), [])
        self.assertIn("all_goals grind", body)
        em = RG.battery_det_solution(stmt, 5, True, "triv_V3_grind")
        self.assertIn("EmulatedOutputs", em)
        with self.assertRaises(ValueError):
            RG.battery_det_solution(stmt, 5, False, "triv_V2_bv_decide")

    def test_model_key(self) -> None:
        reg = registry_package(self.tmp)
        meas = RG.measure_reference(reg)
        self.assertEqual(RG.model_key(meas), RG.model_key(dict(meas, res=None)))
        other = dict(meas, io=dict(meas["io"], output_paths=["r9"]))
        self.assertNotEqual(RG.model_key(meas), RG.model_key(other))


class StatementTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_open_problem_validates_and_has_no_statement(self) -> None:
        self.assertEqual(P.validate_problem(self.prob, self.pdir), [])
        self.assertFalse(P.has_statement(self.prob))
        self.assertEqual((self.prob["record"]["nonlinear"], self.prob["record"]["total"]), (4, 5))
        bad = copy.deepcopy(self.prob)
        bad["record"]["nonlinear"] = 3
        self.assertTrue(P.validate_problem(bad, self.pdir))
        bad = copy.deepcopy(self.prob)
        bad["reference"]["receiver_type"] = "T"
        self.assertTrue(P.validate_problem(bad))
        RT.put(os.path.join(self.pdir, "reference", "wrapper.go"), "changed")
        self.assertIn("sha256 mismatch for reference/wrapper.go", P.validate_problem(self.prob, self.pdir))

    def test_statement_states_both_directions(self) -> None:
        pkg, stmt = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        cprob = json.loads(Path(pkg, "problem.json").read_text(encoding="utf-8"))
        self.assertEqual(P.validate_problem(cprob, pkg), [])
        self.assertTrue(P.has_statement(cprob))
        ref_ns = self.prob["reference"]["model"]["namespace"]
        self.assertIn(f"theorem equiv [Fact (Nat.Prime {ref_ns}.p)] :\n    ∀ x y : List {ref_ns}.F,", stmt)
        left, right = stmt.split(" ↔\n")
        self.assertIn(f"{ref_ns}.Constraints w ∧ {ref_ns}.Inputs.map w = x ∧ {ref_ns}.Outputs.map w = y)", left)
        self.assertIn("Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by\n  sorry", right)
        self.assertNotIn("EmulatedOutputs", stmt)
        cmodel = Path(pkg, "ZkRatchet", ref_ns.split(".", 1)[1], "Cand", "Model.lean").read_text(encoding="utf-8")
        self.assertIn("# Ratchet candidate model: ", cmodel)
        self.assertEqual(M.parse_model(cmodel, "theorem det :\n").n_wires, model_of("equivalent").r.n_wires)

    def test_emulated_outputs_are_compared_by_value(self) -> None:
        prob = copy.deepcopy(self.prob)
        prob["reference"]["io"]["groups"] = [{"start": 0, "len": 1, "bits": 64, "modulus": "97", "field": "F97",
                                               "path": "r0"}]
        cand, _ = candidate_record(prob, "equivalent")
        stmt = RG.statement_text(prob, cand)
        ref_ns = prob["reference"]["model"]["namespace"]
        self.assertIn(f"∀ (x y : List {ref_ns}.F) (z : List ℕ),", stmt)
        self.assertIn(f"{ref_ns}.EmulatedOutputs.map (fun g => {ref_ns}.emValue w g.1 g.2.1 % g.2.2) = z) ↔", stmt)
        self.assertIn("Cand.EmulatedOutputs.map (fun g => Cand.emValue w g.1 g.2.1 % g.2.2) = z) := by", stmt)
        self.assertTrue(any("modulo the emulated modulus" in a for a in RG.statement_assumptions(prob, cand)))

    def test_open_problem_is_not_checkable(self) -> None:
        sol = os.path.join(self.tmp, "s", "Solution.lean")
        RT.put(sol, "theorem x : True := trivial\n")
        rep = C.check(self.pdir, sol, fake_env(self.tmp))
        self.assertEqual(rep["verdict"], "ERROR")


class ScenarioTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_simulation_fixtures_share_their_vectors(self) -> None:
        hdrs = {n: sim(n)[0] for n in NAMES}
        self.assertEqual({h["seed"] for h in hdrs.values()}, {SIM_SEED})
        labels = [x["label"] for x in sim("reference")[1]]
        self.assertEqual(labels[:5], [f"edge: all {p}" for p in ("zero", "one", "two", "max", "half")])
        self.assertIn("edge: In1 max, others zero", labels)
        self.assertEqual(hdrs["reference"]["vectors"], len(labels))

    def test_equivalent_candidate_passes_and_its_committed_proof_follows_the_rules(self) -> None:
        sm = screen("equivalent")
        self.assertEqual((sm["verdict"], sm["mismatches"], sm["reference_recheck_failures"]), ("PASS", 0, 0))
        self.assertGreater(sm["witnesses_rechecked"], 0)
        pkg, stmt = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        proof = (FIX / "Solution.lean").read_text(encoding="utf-8")
        probs, aux, body = C.text_check(stmt, proof, "equiv")
        self.assertEqual(probs, [])
        self.assertEqual(C.forbidden_scan([("helper declarations", aux), ("proof", body)]), [])
        self.assertEqual(RG.det_screen(RG.candidate_model(self.prob, compiled("equivalent")))["propagation"],
                         "DETERMINED")

    def test_nonequivalent_smaller_candidate_is_caught(self) -> None:
        cand, _ = candidate_record(self.prob, "nonequivalent")
        self.assertTrue(cand["smaller"])
        sm = screen("nonequivalent")
        self.assertEqual(sm["verdict"], "MISMATCH")
        self.assertGreater(sm["mismatch_kinds"]["different outputs"], 0)

    def test_underconstrained_candidate_passes_the_screen_but_its_sorry_proof_is_rejected(self) -> None:
        sm = screen("underconstrained")
        self.assertEqual((sm["verdict"], sm["mismatches"]), ("PASS", 0))
        self.assertEqual(RG.det_screen(RG.candidate_model(self.prob, compiled("underconstrained")))["propagation"],
                         "STUCK")
        pkg, stmt = candidate_package(self.pdir, self.prob, "underconstrained", os.path.join(self.tmp, "uc"))
        sol = os.path.join(self.tmp, "uc-solver", "Solution.lean")
        RT.put(sol, stmt)
        rep = C.check(pkg, sol, fake_env(self.tmp))
        self.assertEqual((rep["verdict"], rep["compile"]), ("INVALID", "skipped"))
        self.assertTrue(any(x.startswith("sorry in proof") for x in rep["invalid"]))

    def test_witnesses_and_acceptance_are_compared(self) -> None:
        ra, a = sim("reference")
        rb, b = sim("equivalent")
        bad = [dict(x, witnesses=[[w[0], w[1], w[2], "5"] + w[4:] for w in x["witnesses"]]) if x.get("witnesses")
               else x for x in b]
        sm = RG.compare_runs(ra, a, rb, bad, model_of("reference"), model_of("equivalent"))
        self.assertEqual(sm["mismatch_kinds"], {"candidate witness violates the candidate model":
                                                sum(1 for x in b if x.get("witnesses"))})
        fails = [dict(x, ok=False, error="constraint #1 is not satisfied", outputs=None, witnesses=None) for x in b]
        sm = RG.compare_runs(ra, a, rb, fails, model_of("reference"), model_of("equivalent"))
        self.assertEqual(sm["mismatch_kinds"], {"reference accepts, candidate rejects": len(b)})
        with self.assertRaises(RuntimeError):
            RG.compare_runs(ra, a[:3], rb, b[:2], model_of("reference"), model_of("equivalent"))

    def test_output_fixed_only_by_a_free_wire_is_flagged(self) -> None:
        def model(cons):
            r = R.R1cs(R.BN254_SCALAR, 32, 5, 0, 0, 2, 5, cons)
            return GR.Model(r=r, inputs=[1, 2], outputs=[4], groups=[], output_paths=["r0"], wire_names=[None] * 5,
                            witnesses=[])
        free = model([([(3, 1)], [(0, 1)], [(4, 1)])])                              # out = h, h free
        pinned = model([([(1, 1)], [(2, 1)], [(3, 1)]), ([(3, 1)], [(0, 1)], [(4, 1)])])   # h = a * b, out = h
        direct = model([([(1, 1), (2, 1)], [(0, 1)], [(4, 1)])])                    # out = a + b
        self.assertEqual((RG.free_outputs(free), RG.free_outputs(pinned), RG.free_outputs(direct)), ([4], [], []))
        self.assertEqual(RG.free_outputs(model_of("equivalent")), [])
        self.assertEqual(RG.free_outputs(model_of("underconstrained")), [])   # pinned by a product: only the proof

    def test_emulated_values_and_commitment_witness_merge(self) -> None:
        hdr = {"groups": [{"start": 1, "len": 2, "bits": 4, "modulus": "7"}], "commits": 1, "boundary_wires": 2}
        self.assertEqual(RG.output_view(hdr, ["9", "3", "1"]), ([9], [(3 + 16) % 7]))
        self.assertEqual(RG.output_view(hdr, ["9", "5", "0"]), RG.output_view(hdr, ["9", "12", "0"]))
        self.assertEqual(RG.merge_witness(hdr, [["1", "4", "8"], ["1", "4", "9", "10"]]), [1, 4, 8, 9, 10])
        with self.assertRaises(ValueError):
            RG.merge_witness(hdr, [["1", "4", "8"], ["1", "5", "9"]])


@unittest.skipUnless(LIVE_TOOLS and LIVE_SNAPSHOT and LIVE_LEAN,
                     "set BOOLE_ZK_RATCHET_GNARK_TOOLS, BOOLE_ZK_RATCHET_GNARK_SNAPSHOT and BOOLE_ZK_RATCHET_LEAN_ENV")
class LiveToolchainTests(unittest.TestCase):
    """The pipeline with the real tools: the toy reference compiled from a snapshot copy (its harness result must equal
    the committed one), its DET proof, build (rebuild compared with the registry R1CS), count, simulate (gnark's
    solver), statement elaboration and the final check."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.tmp = tempfile.mkdtemp()
        L.set_lean_limits(slots=1, rss_mb=RT.DET_PROOF_LIMITS["rss_mb"])
        cls.env = L.load_env(LIVE_LEAN)
        cls.gocache = os.path.join(cls.tmp, "gocache")
        sid = "fixture__ratchet-gnark-toy@cfc7b2f907cc"
        cls.snap = os.path.join(cls.tmp, "snapshots", sid)
        fixture_snapshot(LIVE_SNAPSHOT, cls.snap)
        pins = RG.go_pins(os.path.join(LIVE_TOOLS, "go1.25.7"))
        layout = RG.package_layout(cls.snap, "", TOY_PATH)
        stage = os.path.join(cls.tmp, "stage")
        RT.put(os.path.join(stage, "reference", "wrapper.go"), wrapper_text())
        pre = {"reference": {**layout, "name": "Toy", "wrapper": {"file": "reference/wrapper.go", "id": WID,
                                                                    "sha256": RT.sha(wrapper_text().encode())}},
               "env": pins, "_dir": stage}
        cls.ref_res = RG.compile_program(pre, cls.snap, LIVE_TOOLS, os.path.join(cls.tmp, "refc"), gocache=cls.gocache)
        lean_pins = cls.env.pins()
        env = dict(ENV, lean=lean_pins["lean"], mathlib=lean_pins["mathlib"],
                   lake_manifest_sha256=lean_pins["lake_manifest_sha256"], packages=lean_pins["packages"])
        reg = registry_package(cls.tmp, env)
        gate = G.g_elab(cls.env, reg, os.path.join(cls.tmp, "elab", "build"),
                        RG.reference_spec(reg)["model"]["namespace"], os.path.join(cls.tmp, "elab"))
        assert gate.status == "PASS", gate.detail
        shutil.rmtree(os.path.join(cls.tmp, "registry"))
        cls.reg = registry_package(cls.tmp, env, gate.detail["reference_type_sha256"])
        det = fixture_det(cls.reg)
        chk = C.check(cls.reg, os.path.join(cls.tmp, "registry", "det", "Solution.lean"), cls.env, None, 1800, False,
                      os.path.join(cls.tmp, "detcheck"))
        cls.det_verdict = chk["verdict"]
        cls.tools = os.path.join(cls.tmp, "tools")
        os.makedirs(cls.tools)
        os.symlink(os.path.join(LIVE_TOOLS, "go1.25.7"), os.path.join(cls.tools, "go1.25.7"))
        cls.pdir = os.path.join(cls.tmp, "problems", "toy")
        cls.prob = RG.build_problem(cls.reg, cls.snap, cls.pdir, cls.tools, det, "", sid,
                                    os.path.join(cls.tmp, "build"), gocache=cls.gocache)
        for root, _, names in os.walk(cls.snap):              # workspaces hand out read-only snapshots
            for n in names:
                os.chmod(os.path.join(root, n), 0o444)

    @classmethod
    def tearDownClass(cls) -> None:
        for root, _, names in os.walk(cls.snap):
            for n in names:
                os.chmod(os.path.join(root, n), 0o644)
        shutil.rmtree(cls.tmp)

    def out(self, name: str) -> str:
        return os.path.join(self.tmp, "runs", name)

    def test_build_matches_the_committed_fixtures(self) -> None:
        self.assertEqual(self.det_verdict, "PASS")
        self.assertEqual(self.ref_res["result"]["symbolic"]["constraints"],
                         harness("reference")["symbolic"]["constraints"])
        self.assertEqual((self.prob["record"]["nonlinear"], self.prob["record"]["total"]), (4, 5))
        self.assertEqual(P.validate_problem(self.prob, self.pdir), [])
        self.assertEqual(self.prob["reference"]["model"]["r1cs_sha256"], compiled("reference")["r1cs_sha256"])

    def test_counts_and_simulation_with_gnarks_solver(self) -> None:
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            rep, _ = RG.compile_candidate(self.prob | {"_dir": self.pdir}, str(CANDS / f"{name}.go"),
                                          self.out("count-" + name), self.snap, self.tools, self.gocache)
            self.assertEqual(rep["candidate"]["r1cs_sha256"], compiled(name)["r1cs_sha256"], name)
        verdicts = {}
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            rep = RG.simulate(self.pdir, str(CANDS / f"{name}.go"), self.out("sim-" + name), 100, self.tools,
                              self.snap, self.gocache)
            verdicts[name] = rep["simulate"]["verdict"]
        self.assertEqual(verdicts, {"equivalent": "PASS", "nonequivalent": "MISMATCH", "underconstrained": "PASS"})

    def test_final_checks(self) -> None:
        verdict, rep = RG.run_check(self.pdir, str(CANDS / "equivalent.go"), str(FIX / "Solution.lean"),
                                    self.out("check-eq"), self.tools, self.env, self.snap, gocache=self.gocache)
        self.assertEqual(verdict, "PASS", rep.get("checker"))
        cr = RG.run_candidate(self.pdir, str(CANDS / "underconstrained.go"), self.out("stmt-uc"), self.tools,
                              self.snap, env=self.env, gocache=self.gocache)
        sorry = os.path.join(self.tmp, "uc-solver", "Solution.lean")
        RT.put(sorry, Path(cr["package"], "Statement.lean").read_text(encoding="utf-8"))
        verdict, _ = RG.run_check(self.pdir, str(CANDS / "underconstrained.go"), sorry, self.out("check-uc"),
                                  self.tools, self.env, self.snap, gocache=self.gocache)
        self.assertEqual(verdict, "INVALID")
        same = os.path.join(self.tmp, "same", "Candidate.go")
        RT.put(same, go_source("func BooleRatchetCandidate(api frontend.API, a, b frontend.Variable) "
                               "frontend.Variable {\n\treturn Toy(api, a, b)\n}\n"))
        verdict, rep = RG.run_check(self.pdir, same, str(FIX / "Solution.lean"), self.out("check-same"), self.tools,
                                    self.env, self.snap, gocache=self.gocache)
        self.assertEqual(verdict, "REJECTED")                   # the reference itself: not smaller


if __name__ == "__main__":
    unittest.main()
