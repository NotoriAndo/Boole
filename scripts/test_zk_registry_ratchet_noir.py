#!/usr/bin/env python3
"""Noir ratchet problems: the ACIR cost vector and its component-wise order, candidate admissibility, the call
substitution in the reference's wrapper, statement generation (black boxes shared), the simulation screen and the
checker paths, on the toy fixture ``fixtures/zk-registry/ratchet-noir``.

The toy reference ``toy(a, b)`` computes ``2 * a^2 * b`` as two separate products (record: 4 multiplication terms).
Candidates: ``equivalent`` (one product chain, 2 terms; its committed fixture proof ``Solution.lean`` proves the
generated statement), ``nonequivalent`` (``a^2 * b``, caught by the screen) and ``underconstrained`` (the square is
an unconstrained Brillig hint: it passes the screen and its `sorry` proof is rejected).  The compiled fixtures
(``compiled/*.acir.json``: decoded ACIR; ``compiled/*.program.json``: executor inputs) were produced by the
harness with nargo v1.0.0-beta.25 and its boole-acir-tool.

Offline tests need neither nargo nor Lean.  ``LiveToolchainTests`` re-runs the pipeline with the real tools when
``BOOLE_ZK_RATCHET_NOIR_TOOLS`` (a directory with ``v1.0.0-beta.25/nargo`` and ``boole-acir-tool``) and
``BOOLE_ZK_RATCHET_LEAN_ENV`` (lean_runner environment JSON) are set; it is skipped otherwise."""
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

from zk_registry import check as C              # noqa: E402
from zk_registry import gates as G              # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import mech_acir as M          # noqa: E402
from zk_registry import noir_acir as A          # noqa: E402
from zk_registry import noir_det as ND           # noqa: E402
from zk_registry import noir_lean_emit as NE    # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import ratchet as RT           # noqa: E402
from zk_registry import ratchet_noir as RN      # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "ratchet-noir"
CANDS = FIX / "candidates"
COMPILED = FIX / "compiled"
PR = A.BN254
H = "ab" * 32
NARGO_SHA = "4e6e7cebf0c96a59b1993f6ff63e4f473be79a311f85009b2408854fd3e02a35"     # nargo v1.0.0-beta.25 release
ENV = {"lean": "v4.33.1", "mathlib": "c" * 40, "lake_manifest_sha256": H, "packages": {"mathlib": "c" * 40},
       "python": "3.9", "nargo": "1.0.0-beta.25 (v1.0.0-beta.25)", "nargo_binary_sha256": NARGO_SHA,
       "acir_tool_sha256": H}
REPO_ID, REPO_URL, COMMIT = "fixture/ratchet-noir-toy", "https://example.invalid/ratchet-noir-toy", "0" * 40
WRAPPER = "#[export]\nfn boole_det_toy0000000(a0: Field, a1: Field) -> Field {\n    let r = toy(a0, a1);\n    r\n}\n"
LAYOUT = {"crate": "toy", "crate_type": "lib", "target_file": "src/lib.nr"}
LIVE_TOOLS = os.environ.get("BOOLE_ZK_RATCHET_NOIR_TOOLS")
LIVE_LEAN = os.environ.get("BOOLE_ZK_RATCHET_LEAN_ENV")


def decoded(name: str) -> dict:
    return json.loads((COMPILED / f"{name}.acir.json").read_text(encoding="utf-8"))


def flat_of(name: str) -> A.Flat:
    return A.flatten(A.normalize_program(decoded(name)))


def registry_package(root: str, env: dict | None = None, type_sha: str = H) -> str:
    """A registry Noir DET package of the toy reference, built with the wave's emitters from the committed ACIR."""
    dec = decoded("reference")
    flat = flat_of("reference")
    ns = P.lean_namespace("ratchet-noir-fixture", "toy.toy")
    meta = {"repo_id": REPO_ID, "instantiation": "toy(a0, a1)", "generator": "fixture", "repo_url": REPO_URL,
            "commit": COMMIT, "path": "toy/src/lib.nr", "template": "toy", "rule": "parameter-free",
            "nargo_version": "1.0.0-beta.25", "nargo_command": "nargo export --silence-warnings",
            "acir_sha256": A.normalize_program(dec).raw_sha256}
    names = {0: "param `a0`", 1: "param `a1`", 2: "return value [0]"}
    model, info = NE.emit_model(ns, meta, flat, names)
    statement = NE.emit_statement(ns, meta, info["bb"])
    pkg = os.path.join(root, "registry", "toy.toy")
    model_rel = NE.model_relpath(ns)
    RT.put(os.path.join(pkg, model_rel), model)
    RT.put(os.path.join(pkg, "Statement.lean"), statement)
    RT.put(os.path.join(pkg, "evidence", "wrapper.nr"), f"// appended to toy/src/lib.nr (pinned commit {COMMIT})\n"
           + WRAPPER)
    P.write_json(os.path.join(pkg, "evidence", "acir.json"), dec)
    rec = {
        "schema_version": P.SCHEMA_VERSION, "package_id": "ratchet-noir-fixture/toy.toy",
        "property": dict(P.DET_PROPERTY), "status": "OPEN", "status_reason": "fixture",
        "ids": {"ledger_item_id": f"{REPO_ID}:toy/src/lib.nr#toy", "repo": REPO_ID, "repo_url": REPO_URL,
                "release": "v1", "commit": COMMIT, "path": "toy/src/lib.nr", "template": "toy", "template_line": 2,
                "source_sha256": H, "dependencies": []},
        "instantiation": {"rule": "parameter-free", "args": [], "call": "toy(a0, a1)", "params": [],
                          "provenance": ["toy/src/lib.nr:2 toy (no generic parameters)"], "selection": "only candidate",
                          "candidates": [{"tier": "parameter-free", "call": "toy(a0, a1)", "compile": "ok"}]},
        "spec": dict(ND.NOIR_DET_SPEC),
        "circuit": {"compiler": {"name": "nargo", "version": "1.0.0-beta.25", "binary_sha256": NARGO_SHA,
                                 "flags": ["export", "--silence-warnings"]},
                    "n_constraints": len(flat.opcodes), "n_wires": flat.n_witnesses, "prime": str(PR),
                    "prime_name": "bn254", "size_policy": {"max_constraints": P.MAX_CONSTRAINTS, "within": True}},
        "statement": {"file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                      "model_module": NE.model_module(ns), "model_file": model_rel, "text": statement,
                      "assumptions": ND.statement_assumptions([], False, 0), "truth": "unknown"},
        "gates": {g: {"status": "PASS"} for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV", "DET-SEARCH")},
        "checker": {"statement_file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                    "lean_opts": list(NE.LEAN_OPTIONS),
                    "files": [{"path": "Statement.lean", "role": "statement", "sha256": RT.sha(statement.encode())},
                              {"path": model_rel, "role": "import", "module": NE.model_module(ns),
                               "sha256": RT.sha(model.encode())}],
                    "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                    "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS},
        "env": dict(env or ENV), "generator": ND.generator_info(),
        "evidence": {"plan": "export", "compiler_selection": {"attempt_order": ["v1.0.0-beta.25"], "pin": "fixture"}},
    }
    P.write_json(os.path.join(pkg, "problem.json"), rec)
    return pkg


def fixture_det(reg: str, verdict: dict | None = None) -> dict:
    """DET evidence of the toy reference: the battery P3 mechanical proof."""
    pkg = M.load(reg)
    res = M.solve(pkg)
    assert res.status == "DETERMINED", res.status
    sol = os.path.join(os.path.dirname(reg), "det", "Solution.lean")
    RT.put(sol, M.emit_solution(pkg, res))
    return RT.det_record("mech-p3", RN.reference_spec(reg), sol, verdict or {"verdict": "PASS"}, "fixture")


def snapshot_into(root: str) -> tuple[str, str]:
    sid = RT.snapshot_id(REPO_ID, COMMIT, [])
    dst = os.path.join(root, "snapshots", sid)
    shutil.copytree(str(FIX / "snapshot"), dst)
    return dst, sid


def offline_problem(root: str, reg: str | None = None) -> tuple[str, dict]:
    """The toy ratchet problem assembled from the committed fixtures (``build_problem`` does the same with a rebuild
    of the reference; the live tests compare the two)."""
    reg = reg or registry_package(root)
    spec = RN.reference_spec(reg)
    meas = RN.measure_reference(reg)
    snap, sid = snapshot_into(root)
    text = RN.append_text("export", spec["wrapper"], spec["imports"])
    append = {"file": "reference/append.nr", "sha256": RT.sha(text.encode()), "how": "fixture"}
    program = {"file": "reference/program.json", "sha256": P.sha256_file(str(COMPILED / "reference.program.json")),
               "note": "fixture"}
    snapshot = {"id": sid, "manifest_sha256": P.sha256_file(os.path.join(snap, "MANIFEST.sha256")), "n_files": 2,
                "rule": "fixture"}
    prob = RN.problem_record(spec, LAYOUT, meas, fixture_det(reg), snapshot, program, append, H)
    pdir = os.path.join(root, "problems", "toy")
    RT.put(os.path.join(pdir, spec["model"]["file"]), Path(reg, spec["model"]["file"]).read_text(encoding="utf-8"))
    RT.put(os.path.join(pdir, "reference", "append.nr"), text)
    shutil.copyfile(str(COMPILED / "reference.program.json"), os.path.join(pdir, "reference", "program.json"))
    P.write_json(os.path.join(pdir, "problem.json"), prob)
    return pdir, prob


def compiled(name: str) -> dict:
    """A compile result of a committed fixture program (the shape ``compile_program`` returns)."""
    dec = decoded(name)
    prog = A.normalize_program(dec)
    return {"decoded": dec, "prog": prog, "flat": A.flatten(prog), "acir_sha256": prog.raw_sha256,
            "program": str(COMPILED / f"{name}.program.json"),
            "program_sha256": P.sha256_file(str(COMPILED / f"{name}.program.json"))}


def candidate_record(prob: dict, name: str) -> tuple[dict, dict]:
    res = compiled(name)
    text = (CANDS / f"{name}.nr").read_text(encoding="utf-8")
    cand = {"source_sha256": RT.sha(text.encode()), "local_files": ["Candidate.nr"], "acir_sha256": res["acir_sha256"],
            "program_sha256": res["program_sha256"], "n_wires": res["flat"].n_witnesses,
            **RN.score(prob["record"], RN.cost_of(res["flat"].opcodes))}
    return cand, res


def candidate_package(pdir: str, prob: dict, name: str, out: str) -> tuple[str, str]:
    cand, res = candidate_record(prob, name)
    pkg = os.path.join(out, "pkg")
    stmt, files, cand_bb = RN.write_candidate_package(prob, pdir, cand, res, pkg)
    P.write_json(os.path.join(pkg, "problem.json"), RN.candidate_problem(prob, cand, files, stmt, H, cand_bb))
    return pkg, stmt


def executor(name: str):
    """Python stand-in for the pinned executor on the toy programs: the witness each program's solver computes."""
    def witness(a: int, b: int) -> dict[int, int]:
        y = 2 * a * a * b % PR
        if name == "reference":                      # [a, b, y, a^2, a*b]
            return {0: a, 1: b, 2: y, 3: a * a % PR, 4: a * b % PR}
        if name == "equivalent":                     # [a, b, y, 2 a^2]
            return {0: a, 1: b, 2: y, 3: 2 * a * a % PR}
        if name == "nonequivalent":                  # [a, b, a^2 b, a^2]
            return {0: a, 1: b, 2: a * a * b % PR, 3: a * a % PR}
        return {0: a, 1: b, 2: y, 3: a * a % PR}     # underconstrained: hint w3 = a^2 (Brillig)

    def run(objs: list[dict]) -> list[dict]:
        out = []
        for o in objs:
            w = witness(int(o["a0"]), int(o["a1"]))
            out.append({"ok": True, "witness": {str(k): format(v, "064x") for k, v in w.items()}, "calls": [],
                        "oracles": []})
        return out
    return run


def screen(name: str, n: int = 200) -> dict:
    abi = decoded("reference")["abi"]
    vectors = RN.sim_vectors(abi, "fixture-seed", n)
    objs = [v for _, v in vectors]
    return RN.compare_runs(vectors, executor("reference")(objs), executor(name)(objs), flat_of("reference"),
                           flat_of(name))


def fake_env(root: str) -> L.LeanEnv:
    return L.LeanEnv(os.path.join(root, "no-toolchain"), [], ENV["lean"], {"mathlib": ENV["mathlib"]},
                     ENV["lake_manifest_sha256"], os.path.join(root, "scratch"))


def az(mul=(), lin=(), const=0) -> dict:
    return {"kind": "assert_zero", "expr": A.Expr(tuple(mul), tuple(lin), const)}


def bb_call(name: str, out: int, predicate=None) -> dict:
    """A black-box opcode on witness 0 (its key is its name: no static parameters)."""
    return {"kind": "bb", "name": name, "key": name, "inputs": [("w", 0)], "outputs": [out], "predicate": predicate}


class MetricTests(unittest.TestCase):
    def test_toy_costs(self) -> None:
        rec = RN.cost_of(flat_of("reference").opcodes)
        self.assertEqual((rec["nonlinear"], rec["linear"], rec["priced"], rec["brillig"]), (4, 0, 4, 0))
        for name, nl, br in (("equivalent", 2, 0), ("nonequivalent", 2, 0), ("underconstrained", 1, 1)):
            c = RN.cost_of(flat_of(name).opcodes)
            self.assertEqual((c["nonlinear"], c["brillig"], c["priced"]), (nl, br, nl), name)
            self.assertTrue(RN.score(rec, c)["smaller"], name)

    def test_components(self) -> None:
        ops = [az([(1, 0, 1), (5, 2, 2), (0, 1, 1)], [(1, 3)]),           # two degree-2 terms (a zero one is free)
               az([], [(1, 0), (PR - 1, 1)]),                            # linear: free
               {"kind": "range", "input": ("w", 0), "bits": 8},
               {"kind": "and", "lhs": ("w", 0), "rhs": ("w", 1), "bits": 32, "output": 4},
               bb_call("Poseidon2Permutation", 5),
               bb_call("Blake2s", 6, ("c", 0)),                          # constant-zero predicate: disabled
               {"kind": "mem_init", "block": 0, "init": [0, 1, 2]}, {"kind": "mem_op", "block": 0},
               {"kind": "brillig", "id": 0, "outputs": [7], "predicate": None}]
        c = RN.cost_of(ops)
        self.assertEqual({k: c[k] for k in ("nonlinear", "linear", "range_bits", "logic_bits", "memory", "brillig")},
                         {"nonlinear": 2, "linear": 1, "range_bits": 8, "logic_bits": 32, "memory": 4, "brillig": 1})
        self.assertEqual(c["black_box"], {"Poseidon2Permutation": 1})
        self.assertEqual(c["priced"], 2 + 8 + 32 + 4 + 1)
        # the wrapper's ABI typing of its parameters (witness 0: u8) is free; other range checks are priced
        abi = {"parameters": [{"name": "a0", "type": {"kind": "integer", "sign": "unsigned", "width": 8}},
                              {"name": "a1", "type": {"kind": "field"}}]}
        rng = RN.abi_ranges(abi, [0, 1])
        self.assertEqual(rng, {0: 8})
        c = RN.cost_of(ops + [{"kind": "range", "input": ("w", 1), "bits": 8}], rng)
        self.assertEqual((c["abi_range_bits"], c["range_bits"]), (8, 8))
        self.assertEqual(RN.abi_ranges(abi, [0, 2]), {})                  # inputs not 0..n-1: nothing is free
        nested = {"parameters": [{"name": "s", "type": {"kind": "struct", "fields": [
            {"name": "b", "type": {"kind": "boolean"}},
            {"name": "t", "type": {"kind": "array", "length": 2, "type": {"kind": "string", "length": 2}}}]}}]}
        self.assertEqual(RN.abi_ranges(nested, [0, 1, 2, 3, 4]), {0: 1, 1: 8, 2: 8, 3: 8, 4: 8})

    def test_componentwise_order(self) -> None:
        base = {"nonlinear": 10, "range_bits": 16, "logic_bits": 0, "black_box": {"Sha256": 1}, "memory": 0}
        rec = dict(base, priced=P.noir_priced(base))

        def cand(**kw):
            c = dict(base, **kw)
            return dict(c, priced=P.noir_priced(c))
        self.assertTrue(P.noir_cost_smaller(rec, cand(nonlinear=9)))
        self.assertTrue(P.noir_cost_smaller(rec, cand(range_bits=8, black_box={})))
        self.assertFalse(P.noir_cost_smaller(rec, cand()))                                   # equal: not smaller
        # trading multiplications for range bits does not count, whatever the sum does
        self.assertFalse(P.noir_cost_smaller(rec, cand(nonlinear=2, range_bits=17)))
        self.assertEqual(P.noir_larger_components(rec, cand(nonlinear=2, range_bits=17)), ["range_bits"])
        # a black box the record does not call is a larger component
        self.assertFalse(P.noir_cost_smaller(rec, cand(nonlinear=1, black_box={"Sha256": 1, "Keccakf1600": 1})))
        self.assertEqual(P.noir_larger_components(rec, cand(black_box={"Sha256": 2})), ["black_box[Sha256]"])
        # linear, ABI range and Brillig counts are free
        s = RN.score(rec, dict(cand(nonlinear=9), linear=500, abi_range_bits=64, brillig=7))
        self.assertTrue(s["smaller"])
        self.assertEqual((s["nonlinear_reduction_pct"], s["larger_components"]), (10.0, []))


class AdmissibilityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def cand(self, text: str) -> str:
        p = os.path.join(self.tmp, "Candidate.nr")
        RT.put(p, text)
        return p

    def rejected(self, text: str) -> str:
        with self.assertRaises(RT.Reject) as cm:
            RN.admit_candidate(self.cand(text))
        return str(cm.exception)

    def test_fixture_candidates_and_allowed_forms(self) -> None:
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            self.assertIn(RN.CANDIDATE_FN, RN.admit_candidate(str(CANDS / f"{name}.nr")))
        ok = ("use std::hash::poseidon2_permutation;\n// #[oracle(get)] in a comment is fine\n#[inline_always]\n"
              "fn helper(x: Field) -> Field { x * x }\nunconstrained fn hint(x: Field) -> Field { x }\n"
              "global K: Field = 3;\n#[no_predicates]\npub fn boole_ratchet_candidate(a: Field) -> Field {\n"
              "    let s = \"#[export]\";\n    helper(a) + K\n}\n")
        RN.admit_candidate(self.cand(ok))

    def test_rejections(self) -> None:
        good = "pub fn boole_ratchet_candidate(a: Field, b: Field) -> Field { a * b }\n"
        self.assertIn("exactly one", self.rejected("fn other(a: Field) -> Field { a }\n"))
        self.assertIn("exactly one", self.rejected(good + good))
        self.assertIn("constrained", self.rejected(good.replace("pub fn", "pub unconstrained fn")))
        self.assertIn("constrained", self.rejected(good.replace("pub fn", "pub comptime fn")))
        self.assertIn("top-level", self.rejected("struct S {}\nimpl S {\n    " + good + "}\n"))
        for attr in ("oracle(get_note)", "foreign(sha256)", "builtin(to_le_bits)", "export", "test"):
            self.assertIn("is not allowed", self.rejected(f"#[{attr}]\nunconstrained fn x() -> Field {{ 0 }}\n" + good))
        self.assertIn("`fn main`", self.rejected(good + "fn main(x: Field) { assert(x == 0); }\n"))
        self.assertIn("`mod` items", self.rejected("mod helpers;\n" + good))
        self.assertIn("`contract` blocks", self.rejected("contract Foo {}\n" + good))
        self.assertIn("boole_det_", self.rejected(good + "fn boole_det_1(a: Field) -> Field { a }\n"))

    def test_candidate_call_substitution(self) -> None:
        cases = [("toy(a0, a1)", "", "boole_ratchet_candidate(a0, a1)"),
                 ("<Foo<4> as Bar>::f::<2, 3>(&mut a0, (a1 + 1), g(a2))", "",
                  "boole_ratchet_candidate(&mut a0, (a1 + 1), g(a2))"),
                 ("std::hash::poseidon2::Poseidon2::hash(a0, 3)", "", "boole_ratchet_candidate(a0, 3)"),
                 ("s_self.push::<4>(a1)", "    let mut s_self = s_self;\n", "boole_ratchet_candidate(&mut s_self, a1)"),
                 ("s_self.len()", "", "boole_ratchet_candidate(s_self)"),
                 ("s_self.get(a1, a2)", "    let s_self = Foo { x: a0 };\n", "boole_ratchet_candidate(s_self, a1, a2)")]
        for call, wrapper, want in cases:
            self.assertEqual(RN.candidate_call(call, wrapper), want, call)
        with self.assertRaises(ValueError):
            RN.candidate_call("toy", "")


class ReferenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.reg = registry_package(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_spec_and_measure(self) -> None:
        spec = RN.reference_spec(self.reg)
        self.assertIsNone(RN.static_exclusion(spec))
        self.assertEqual((spec["plan"], spec["wrapper"], spec["imports"], RN.compiler_tag(spec)),
                         ("export", WRAPPER, [], "v1.0.0-beta.25"))
        meas = RN.measure_reference(self.reg)
        self.assertEqual((meas["inputs"], meas["outputs"], meas["cost"]["nonlinear"]), ([0, 1], [2], 4))
        self.assertEqual(RN.append_text("export", WRAPPER, ["crate::x::Y"]),
                         RN.WRAPPER_HEADER + "use crate::x::Y;\n" + WRAPPER)
        self.assertEqual(RN.append_text("std-export", WRAPPER, ["use std::a::B;"]), "use std::a::B;\n\n" + WRAPPER)

    def test_static_exclusions(self) -> None:
        spec = RN.reference_spec(self.reg)
        self.assertEqual(RN.static_exclusion(dict(spec, plan="bin-main"))[0], "plan")
        self.assertEqual(RN.static_exclusion(dict(spec, property="DET-MOD"))[0], "property")
        self.assertEqual(RN.static_exclusion(dict(spec, call="nope(a0)"))[0], "provenance")
        self.assertEqual(RN.static_exclusion(dict(spec, nargo_version="1.0.0-beta.0",
                                                  compiler_attempts=["aztec-v0.67.0"]))[0], "compiler")
        self.assertEqual(RN.static_exclusion(dict(spec, compiler_attempts=[]))[0], "compiler")

    def test_model_provenance_is_checked(self) -> None:
        p = RN.reference_spec(self.reg)["model"]["file"]
        path = Path(self.reg, p)
        path.write_text(path.read_text(encoding="utf-8").replace("w 0 * w 1 - w 4 = 0", "w 0 * w 1 - w 3 = 0"),
                        encoding="utf-8")
        with self.assertRaises(RT.NotEligible) as cm:
            RN.measure_reference(self.reg)
        self.assertEqual(cm.exception.code, "provenance")

    def test_battery_det_solution_follows_the_text_rules(self) -> None:
        stmt = Path(self.reg, "Statement.lean").read_text(encoding="utf-8")
        for form in ("triv_V1_grind", "triv_V2_simp_all", "triv_V3_grind", "triv_V4_aesop"):
            sol = RN.battery_det_solution(stmt, 3, False, False, form)
            probs, aux, body = C.text_check(stmt, sol, "det")
            self.assertEqual((probs, aux), ([], ""), form)
            self.assertEqual(C.forbidden_scan([("proof", body)]), [], form)
        self.assertIn("(try unfold Constraints at *)", RN.battery_det_solution(stmt, 3, False, False, "triv_V1_grind"))
        for form in ("triv_V1_bv_decide", "nonsense"):
            with self.assertRaises(ValueError):
                RN.battery_det_solution(stmt, 3, False, False, form)

    def test_model_key_ignores_names(self) -> None:
        self.assertEqual(RN.model_key(flat_of("equivalent")), RN.model_key(flat_of("equivalent")))
        self.assertNotEqual(RN.model_key(flat_of("equivalent")), RN.model_key(flat_of("nonequivalent")))


class StatementTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_open_problem_validates_and_has_no_statement(self) -> None:
        self.assertEqual(P.validate_problem(self.prob, self.pdir), [])
        self.assertFalse(P.has_statement(self.prob))
        cand, _ = candidate_record(self.prob, "equivalent")
        self.assertIn("OPEN ratchet problem must not carry 'candidate'",
                      P.validate_problem(dict(self.prob, candidate=cand), self.pdir))
        bad = dict(self.prob, record=dict(self.prob["record"], priced=5))
        self.assertIn("record priced cost is not the sum of its components", P.validate_problem(bad))
        bad = dict(self.prob, det=dict(self.prob["det"], model_sha256="0" * 64))
        self.assertIn("DET evidence is not about the reference model file", P.validate_problem(bad))
        ref = self.prob["reference"]
        bad = dict(self.prob, reference=dict(ref, crate=None))
        self.assertTrue(any("crate" in e for e in P.validate_problem(bad)))
        Path(self.pdir, "reference", "append.nr").write_text("tampered\n", encoding="utf-8")
        self.assertTrue(any("append.nr" in e for e in P.validate_problem(self.prob, self.pdir)))

    def test_statement_states_both_directions(self) -> None:
        pkg, stmt = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        ref_ns = self.prob["reference"]["model"]["namespace"]
        base = "ZkRatchet." + ref_ns.split(".", 1)[1]
        self.assertTrue(stmt.startswith(f"import {ref_ns}.Model\nimport {base}.Cand.Model\n"))
        st = C.parse_statement(stmt, "equiv")
        self.assertEqual(st["body"], " by\n  sorry")
        left = (f"(∃ w : Fin {ref_ns}.nWires → {ref_ns}.F, {ref_ns}.Constraints w ∧ {ref_ns}.Inputs.map w = x ∧ "
                f"{ref_ns}.Outputs.map w = y)")
        right = "(∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y)"
        self.assertIn(f"{left} ↔\n      {right}", st["sig"])
        self.assertIn(f"∀ (x y : List {ref_ns}.F)", st["sig"])
        cand, _ = candidate_record(self.prob, "equivalent")
        for digest in (cand["source_sha256"], cand["acir_sha256"], self.prob["reference"]["model"]["acir_sha256"]):
            self.assertIn(digest, stmt)
        cprob = RT.read_json(os.path.join(pkg, "problem.json"))
        self.assertEqual(P.validate_problem(cprob, pkg), [])
        self.assertTrue(P.has_statement(cprob))
        _, again = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq2"))
        self.assertEqual(again, stmt)
        wrong = dict(cprob, candidate=dict(cprob["candidate"], smaller=False))
        self.assertIn("candidate.smaller disagrees with the record", P.validate_problem(wrong))

    def test_black_boxes_are_shared_and_numbered_like_the_reference(self) -> None:
        ref = self.prob["reference"]
        prob = dict(self.prob, reference=dict(ref, model=dict(ref["model"], black_boxes=["Sha256", "Blake2s"])))
        cflat = A.Flat([bb_call(n, 2) for n in ("Keccakf1600", "Blake2s")], [0, 1], [2], 3)
        self.assertEqual(RN.candidate_keys(prob, cflat), ["Sha256", "Blake2s", "Keccakf1600"])
        self.assertIsNone(RN.candidate_keys(prob, flat_of("equivalent")))
        cmodel, info = NE.emit_model("NS", {"repo_id": "r", "instantiation": "c", "generator": "g", "repo_url": "u",
                                            "commit": "-", "path": "p", "template": "t", "rule": "r",
                                            "nargo_version": "v", "nargo_command": "c", "acir_sha256": H},
                                     cflat, {}, RN.candidate_keys(prob, cflat))
        self.assertIn("[w 2] = bb 2 [w 0] ∧\n  [w 2] = bb 1 [w 0]", cmodel)
        cand, _ = candidate_record(self.prob, "equivalent")
        stmt = RN.statement_text(prob, cand, True)
        ref_ns = ref["model"]["namespace"]
        self.assertIn(f"∀ (bb : ℕ → List {ref_ns}.F → List {ref_ns}.F) (x y : List {ref_ns}.F)", stmt)
        self.assertIn(f"{ref_ns}.Constraints bb w ∧", stmt)
        self.assertIn("Cand.Constraints bb w ∧", stmt)
        self.assertIn("Cand.Constraints w ∧", RN.statement_text(prob, cand, False))
        self.assertIn("0: Sha256, 1: Blake2s", stmt)

    def test_open_problem_is_not_checkable(self) -> None:
        sol = os.path.join(self.tmp, "Solution.lean")
        shutil.copyfile(str(FIX / "Solution.lean"), sol)
        rep = C.check(self.pdir, sol, fake_env(self.tmp))
        self.assertEqual(rep["verdict"], "ERROR")
        self.assertIn("has no Lean statement", rep["error"][0])


class ScenarioTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_simulation_vectors_are_deterministic_and_prefix_stable(self) -> None:
        abi = decoded("reference")["abi"]
        a, b = RN.sim_vectors(abi, "s", 30), RN.sim_vectors(abi, "s", 60)
        self.assertEqual(a, b[:len(a)])
        self.assertEqual(sum(1 for lb, _ in a if lb.startswith("edge")), len(a) - 30)
        self.assertNotEqual(RN.sim_vectors(abi, "other", 30), a)
        self.assertIn({"a0": str(PR - 1), "a1": "0"}, [v for _, v in a])        # one parameter at its maximum

    def test_equivalent_candidate_passes_and_its_committed_proof_follows_the_rules(self) -> None:
        sim = screen("equivalent")
        self.assertEqual((sim["verdict"], sim["mismatches"], sim["reference_recheck_failures"]), ("PASS", 0, 0))
        pkg, stmt = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        proof = (FIX / "Solution.lean").read_text(encoding="utf-8")
        probs, aux, body = C.text_check(stmt, proof, "equiv")
        self.assertEqual(probs, [])
        self.assertEqual(C.forbidden_scan([("helper declarations", aux), ("proof", body)]), [])
        self.assertEqual(RN.det_screen(flat_of("equivalent"))["propagation"], "DETERMINED")

    def test_nonequivalent_smaller_candidate_is_caught(self) -> None:
        cand, _ = candidate_record(self.prob, "nonequivalent")
        self.assertTrue(cand["smaller"])
        sim = screen("nonequivalent")
        self.assertEqual(sim["verdict"], "MISMATCH")
        self.assertGreater(sim["mismatch_kinds"]["different outputs"], 0)

    def test_underconstrained_candidate_passes_the_screen_but_its_sorry_proof_is_rejected(self) -> None:
        sim = screen("underconstrained")
        self.assertEqual((sim["verdict"], sim["mismatches"]), ("PASS", 0))
        self.assertEqual(RN.det_screen(flat_of("underconstrained"))["propagation"], "STUCK")
        pkg, stmt = candidate_package(self.pdir, self.prob, "underconstrained", os.path.join(self.tmp, "uc"))
        sol = os.path.join(self.tmp, "uc-solver", "Solution.lean")
        RT.put(sol, stmt)
        rep = C.check(pkg, sol, fake_env(self.tmp))
        self.assertEqual((rep["verdict"], rep["compile"]), ("INVALID", "skipped"))
        self.assertTrue(any(x.startswith("sorry in proof") for x in rep["invalid"]))

    def test_executor_witnesses_are_rechecked(self) -> None:
        abi = decoded("reference")["abi"]
        vectors = RN.sim_vectors(abi, "s", 5)
        objs = [v for _, v in vectors]
        good = executor("equivalent")(objs)
        bad = [dict(x, witness=dict(x["witness"], **{"3": format(5, "064x")})) for x in good]
        sim = RN.compare_runs(vectors, executor("reference")(objs), bad, flat_of("reference"), flat_of("equivalent"))
        self.assertEqual(sim["mismatch_kinds"], {"candidate witness violates the candidate ACIR": len(vectors)})
        fails = [{"ok": False, "error": "Failed to solve program: assertion"} for _ in objs]
        sim = RN.compare_runs(vectors, executor("reference")(objs), fails, flat_of("reference"), flat_of("equivalent"))
        self.assertEqual(sim["mismatch_kinds"], {"reference succeeds, candidate fails": len(vectors)})
        free = A.Flat([az([(1, 0, 1)], [(PR - 1, 3)])], [0, 1], [2], 4)
        sim = RN.compare_runs([], [], [], flat_of("reference"), free)
        self.assertEqual((sim["verdict"], sim["unconstrained_outputs"]), ("MISMATCH", [0]))


@unittest.skipUnless(LIVE_TOOLS and LIVE_LEAN, "set BOOLE_ZK_RATCHET_NOIR_TOOLS and BOOLE_ZK_RATCHET_LEAN_ENV")
class LiveToolchainTests(unittest.TestCase):
    """The pipeline with the real tools: build (reference rebuilt and compared with the registry ACIR), count,
    simulate (the pinned executor), statement elaboration, the reference's DET proof and the final check."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.tmp = tempfile.mkdtemp()
        L.set_lean_limits(slots=1, rss_mb=RT.DET_PROOF_LIMITS["rss_mb"])
        cls.env = L.load_env(LIVE_LEAN)
        pins = cls.env.pins()
        env = dict(ENV, lean=pins["lean"], mathlib=pins["mathlib"], lake_manifest_sha256=pins["lake_manifest_sha256"],
                   packages=pins["packages"])
        reg = registry_package(cls.tmp, env)
        gate = G.g_elab(cls.env, reg, os.path.join(cls.tmp, "elab", "build"),
                        RN.reference_spec(reg)["model"]["namespace"], os.path.join(cls.tmp, "elab"))
        assert gate.status == "PASS", gate.detail
        shutil.rmtree(os.path.join(cls.tmp, "registry"))
        cls.reg = registry_package(cls.tmp, env, gate.detail["reference_type_sha256"])
        det = fixture_det(cls.reg)
        chk = C.check(cls.reg, os.path.join(cls.tmp, "registry", "det", "Solution.lean"), cls.env, None, 1800, False,
                      os.path.join(cls.tmp, "detcheck"))
        cls.det_verdict = chk["verdict"]
        cls.snap, sid = snapshot_into(cls.tmp)
        for root, _, names in os.walk(cls.snap):              # workspaces hand out read-only snapshots
            for n in names:
                os.chmod(os.path.join(root, n), 0o444)
        cls.pdir = os.path.join(cls.tmp, "problems", "toy")
        cls.prob = RN.build_problem(cls.reg, cls.snap, cls.pdir, LIVE_TOOLS, det, LAYOUT, sid,
                                    os.path.join(cls.tmp, "build"))

    @classmethod
    def tearDownClass(cls) -> None:
        shutil.rmtree(cls.tmp)

    def out(self, name: str) -> str:
        return os.path.join(self.tmp, "runs", name)

    def test_build_matches_the_committed_fixtures(self) -> None:
        self.assertEqual(self.det_verdict, "PASS")
        self.assertEqual((self.prob["record"]["nonlinear"], self.prob["record"]["priced"]), (4, 4))
        self.assertEqual(P.validate_problem(self.prob, self.pdir), [])
        self.assertEqual(self.prob["reference"]["model"]["acir_sha256"], compiled("reference")["acir_sha256"])

    def test_counts_and_simulation_with_the_real_executor(self) -> None:
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            rep, _ = RN.compile_candidate(self.prob, self.pdir, str(CANDS / f"{name}.nr"), self.out("count-" + name),
                                          self.snap, LIVE_TOOLS)
            self.assertEqual(rep["candidate"]["acir_sha256"], compiled(name)["acir_sha256"], name)
        verdicts = {}
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            rep = RN.simulate(self.pdir, str(CANDS / f"{name}.nr"), self.out("sim-" + name), 100, LIVE_TOOLS)
            verdicts[name] = rep["simulate"]["verdict"]
        self.assertEqual(verdicts, {"equivalent": "PASS", "nonequivalent": "MISMATCH", "underconstrained": "PASS"})

    def test_final_checks(self) -> None:
        verdict, rep = RN.run_check(self.pdir, str(CANDS / "equivalent.nr"), str(FIX / "Solution.lean"),
                                    self.out("check-eq"), LIVE_TOOLS, self.env)
        self.assertEqual(verdict, "PASS", rep.get("checker"))
        cr = RN.run_candidate(self.pdir, str(CANDS / "underconstrained.nr"), self.out("stmt-uc"), LIVE_TOOLS,
                              env=self.env)
        sorry = os.path.join(self.tmp, "uc-solver", "Solution.lean")
        RT.put(sorry, Path(cr["package"], "Statement.lean").read_text(encoding="utf-8"))
        verdict, _ = RN.run_check(self.pdir, str(CANDS / "underconstrained.nr"), sorry, self.out("check-uc"),
                                  LIVE_TOOLS, self.env)
        self.assertEqual(verdict, "INVALID")
        same = os.path.join(self.tmp, "same", "Candidate.nr")
        RT.put(same, "pub fn boole_ratchet_candidate(a: Field, b: Field) -> Field {\n    toy(a, b)\n}\n")
        verdict, rep = RN.run_check(self.pdir, same, str(FIX / "Solution.lean"), self.out("check-same"), LIVE_TOOLS,
                                    self.env)
        self.assertEqual(verdict, "REJECTED")                   # the reference itself: not smaller


if __name__ == "__main__":
    unittest.main()
