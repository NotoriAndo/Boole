#!/usr/bin/env python3
"""Ratchet problems (verified circuit optimization): metric, canonical R1CS order, admissibility, statement
generation, the simulation screen and the checker paths, on the toy fixture ``fixtures/zk-registry/ratchet``.

The toy reference computes ``y = a*b + a*b`` with two products (record: 2 non-linear constraints).  Candidates:
``equivalent`` (one product ``2*a*b``; its committed fixture proof ``Solution.lean`` proves the generated
statement), ``nonequivalent`` (``a*b``, caught by the simulation screen) and ``underconstrained`` (a free hint ``p``
that the witness generator fills with ``a``: it passes the screen, and its `sorry` proof is rejected).  The compiled
fixtures were produced by circom v2.2.3 (``--O0`` for the model, canonical ``--O2`` with public inputs otherwise).

Offline tests need neither circom, node nor Lean.  ``LiveToolchainTests`` re-runs the whole pipeline with the real
tools (pinned compiler, node, Lean + Mathlib) when ``BOOLE_ZK_RATCHET_COMPILERS`` (JSON: tag -> binary) and
``BOOLE_ZK_RATCHET_LEAN_ENV`` (lean_runner environment JSON) are set; it is skipped otherwise."""
from __future__ import annotations

import os
import random
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import check as C              # noqa: E402
from zk_registry import circom_det as CD        # noqa: E402
from zk_registry import gates as G              # noqa: E402
from zk_registry import instantiation as I      # noqa: E402
from zk_registry import lean_emit as E          # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import mech_r1cs as M          # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import r1cs as R               # noqa: E402
from zk_registry import ratchet as RT           # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "ratchet"
CANDS = FIX / "candidates"
COMPILED = FIX / "compiled"
PR = R.BN254_SCALAR
H = "ab" * 32
ENV = {"circom": "2.2.3", "circom_binary_sha256": H, "lean": "v4.33.1", "mathlib": "c" * 40,
       "lake_manifest_sha256": H, "packages": {"mathlib": "c" * 40}, "node": "v22", "python": "3.9"}
REPO_ID, REPO_URL, COMMIT = "fixture/ratchet-toy", "https://example.invalid/ratchet-toy", "0" * 40
LIVE_COMPILERS = os.environ.get("BOOLE_ZK_RATCHET_COMPILERS")
LIVE_LEAN = os.environ.get("BOOLE_ZK_RATCHET_LEAN_ENV")


def registry_package(root: str, env: dict | None = None, type_sha: str = H) -> str:
    """A registry DET package of the toy reference, built with the wave's emitters (``env`` / ``type_sha``: the real
    Lean pins and elaborated type in the live tests)."""
    r = R.read_r1cs(str(COMPILED / "toy.O0.r1cs"))
    syms = R.read_sym(str(COMPILED / "toy.O0.sym"))
    io = R.main_io_wires(r, syms)
    ns = P.lean_namespace("ratchet-fixture", "toy.Toy")
    flags = ["--r1cs", "--sym", "--wasm", "--O0"]
    r1cs_sha = P.sha256_file(str(COMPILED / "toy.O0.r1cs"))
    meta = {"repo_id": REPO_ID, "instantiation": "Toy()", "generator": "fixture", "repo_url": REPO_URL,
            "commit": COMMIT, "path": "toy.circom", "template": "Toy", "rule": "parameter-free",
            "circom_version": "2.2.3", "circom_flags": flags, "r1cs_sha256": r1cs_sha, "prime_name": "bn128"}
    pkg = os.path.join(root, "registry", "toy.Toy")
    model_rel = E.model_relpath(ns)
    model = E.emit_model(ns, meta, r, io.outputs, io.inputs, R.wire_names(syms, r.n_wires))
    RT.put(os.path.join(pkg, model_rel), model)
    statement = E.emit_statement(ns, meta)
    RT.put(os.path.join(pkg, "Statement.lean"), statement)
    passed = {"status": "PASS"}
    bin_sha = CD.CIRCOM_RELEASES["v2.2.3"]["sha256"]["macos-amd64"]
    rec = {
        "schema_version": P.SCHEMA_VERSION, "package_id": "ratchet-fixture/toy.Toy", "property": dict(P.DET_PROPERTY),
        "status": "OPEN", "status_reason": "fixture",
        "ids": {"ledger_item_id": f"{REPO_ID}:toy.circom#Toy", "repo": REPO_ID, "repo_url": REPO_URL, "release": "v1",
                "commit": COMMIT, "path": "toy.circom", "template": "Toy", "template_line": 5, "source_sha256": H},
        "instantiation": {"rule": "parameter-free", "args": [], "call": "Toy()", "include_context": "toy.circom",
                          "main_sha256": RT.sha(I.main_source("toy.circom", "Toy", (), pragma="2.0.0").encode()),
                          "provenance": ["toy.circom:5 Toy()"], "selection": "only candidate",
                          "candidates": [{"tier": "parameter-free", "call": "Toy()", "compile": "ok", "constraints": 3,
                                          "wires": 6}]},
        "spec": dict(P.DET_SPEC),
        "circuit": {"compiler": {"name": "circom", "version": "2.2.3", "flags": flags, "binary_sha256": bin_sha},
                    "prime": str(PR), "prime_name": "bn128", "n_constraints": 3, "n_wires": 6, "n_inputs": 2,
                    "n_outputs": 1, "r1cs_sha256": r1cs_sha,
                    "size_policy": {"max_constraints": P.MAX_CONSTRAINTS, "within": True}},
        "statement": {"file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                      "model_module": E.model_module(ns), "model_file": model_rel, "text": statement,
                      "assumptions": list(P.STATEMENT_ASSUMPTIONS), "truth": "unknown"},
        "gates": {g: dict(passed) for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV", "DET-SEARCH")},
        "checker": {"statement_file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                    "lean_opts": list(E.LEAN_OPTIONS),
                    "files": [{"path": "Statement.lean", "role": "statement", "sha256": RT.sha(statement.encode())},
                              {"path": model_rel, "role": "import", "module": E.model_module(ns),
                               "sha256": RT.sha(model.encode())}],
                    "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                    "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS},
        "env": dict(env or ENV), "generator": P.generator_info(), "evidence": {},
    }
    P.write_json(os.path.join(pkg, "problem.json"), rec)
    return pkg


def fixture_det(reg: str, verdict: dict | None = None) -> dict:
    """DET evidence of the toy reference: the battery P3 mechanical proof (the offline tests record a fixture
    verdict; the live tests run the production checker on it)."""
    sol = os.path.join(os.path.dirname(reg), "det", "Solution.lean")
    rec = M.solve(reg, sol)
    assert rec["propagation"] == "DETERMINED", rec
    return RT.det_record("mech-p3", RT.reference_spec(reg), sol, verdict or {"verdict": "PASS"}, "fixture")


def snapshot_into(root: str) -> str:
    dst = os.path.join(root, "snapshots", RT.snapshot_id(REPO_ID, COMMIT, []))
    shutil.copytree(str(FIX / "snapshot"), dst)
    return dst


def offline_problem(root: str) -> tuple[str, dict]:
    """The toy ratchet problem assembled from the compiled fixtures (``build_problem`` does the same from real
    compiles; the live tests compare the two)."""
    reg = registry_package(root)
    spec = RT.reference_spec(reg)
    snap = snapshot_into(root)
    o0 = {"r1cs": str(COMPILED / "toy.O0.r1cs"), "sym": str(COMPILED / "toy.O0.sym")}
    sig = RT.signature(o0)
    rec_path = str(COMPILED / "record.r1cs")
    record_main = RT.reference_main(spec, False, False, ["a", "b"])
    measured = {
        "nomain": False, "custom_templates": False, "io": {k: sig[k] for k in ("prime", "prime_name", "output_names",
                                                                              "input_names")},
        "public_inputs": RT.input_bases(sig["input_names"]),
        "record": {"flags": RT.record_flags_text(spec), "main_sha256": RT.sha(record_main.encode()),
                   "r1cs_sha256": P.sha256_file(rec_path), "r1cs_raw_sha256": H,
                   "n_wires": R.read_r1cs(rec_path).n_wires, **RT.counts(rec_path)},
        "model_counts": RT.counts(o0["r1cs"]),
        "simulate": {"seed": None, "max_bits": {"a": 254, "b": 254}, "ladder": list(RT.LADDER),
                     "reference_generator": {"flags": RT.model_flags_text(spec) + RT.FLAGS_WASM, "wasm_sha256": H,
                                             "witness_calculator_sha256": H, "r1cs_sha256": spec["r1cs_sha256"]}}}
    snapshot = {"id": os.path.basename(snap), "manifest_sha256": P.sha256_file(os.path.join(snap, "MANIFEST.sha256")),
                "n_files": 1, "rule": "fixture"}
    prob = RT.problem_record(spec, measured, fixture_det(reg), snapshot)
    pdir = os.path.join(root, "problems", "toy")
    RT.put(os.path.join(pdir, spec["model"]["file"]),
           Path(reg, spec["model"]["file"]).read_text(encoding="utf-8"))
    P.write_json(os.path.join(pdir, "problem.json"), prob)
    return pdir, prob


def compiled_candidate(prob: dict, name: str) -> tuple[dict, dict]:
    """(candidate record, compile result) of a fixture candidate from its committed canonical --O2 compile."""
    res = {"r1cs": str(COMPILED / f"{name}.r1cs"), "sym": str(COMPILED / f"{name}.sym"),
           "r1cs_sha256": P.sha256_file(str(COMPILED / f"{name}.r1cs"))}
    text = (CANDS / f"{name}.circom").read_text(encoding="utf-8")
    cand = {"source_sha256": RT.sha(text.encode()), "local_files": ["Candidate.circom"], "snapshot_includes": [],
            "pragma": "2.0.0", "r1cs_sha256": res["r1cs_sha256"],
            "sym_sha256": P.sha256_file(res["sym"]), "n_wires": R.read_r1cs(res["r1cs"]).n_wires,
            **RT.score(prob["record"], RT.counts(res["r1cs"]))}
    return cand, res


def candidate_package(pdir: str, prob: dict, name: str, out: str) -> tuple[str, str]:
    """The checker package of a fixture candidate (status CANDIDATE) with a placeholder reference type digest."""
    cand, res = compiled_candidate(prob, name)
    pkg = os.path.join(out, "pkg")
    stmt, files = RT.write_candidate_package(prob, pdir, cand, res, pkg)
    P.write_json(os.path.join(pkg, "problem.json"), RT.candidate_problem(prob, cand, files, stmt, H))
    return pkg, stmt


def generator(kind: str):
    """Python stand-ins for the circuits' wasm witness generators: the witness each circuit's generator computes,
    re-checked against that circuit's R1CS like ``ratchet_sim.js`` does."""
    path = {"ref": COMPILED / "toy.O0.r1cs", "equivalent": COMPILED / "equivalent.r1cs",
            "nonequivalent": COMPILED / "nonequivalent.r1cs",
            "underconstrained": COMPILED / "underconstrained.r1cs"}[kind]
    r = R.read_r1cs(str(path))

    def witness(a: int, b: int) -> list[int]:
        if kind == "ref":                                   # [1, y, a, b, s, t]
            return [1, 2 * a * b % PR, a, b, a * b % PR, a * b % PR]
        if kind == "equivalent":                            # [1, y, a, b]
            return [1, 2 * a * b % PR, a, b]
        if kind == "nonequivalent":
            return [1, a * b % PR, a, b]
        return [1, 2 * a * b % PR, a, b, a]                 # underconstrained: [1, y, a, b, p] with p <-- a

    def run(objs: list[dict]) -> list[dict]:
        out = []
        for o in objs:
            a, b = int(o["a"][0]), int(o["b"][0])
            if a >= PR or b >= PR:
                out.append({"s": "R", "err": "Error: input out of field"})
                continue
            w = witness(a, b)
            bad = R.violated(r, w, 1)
            out.append({"s": "V", "c": bad[0], "out": [str(w[1])]} if bad else {"s": "A", "out": [str(w[1])]})
        return out
    return run, r


def screen(prob: dict, name: str, n: int = 300) -> dict:
    vectors = RT.sim_vectors(prob, n)
    objs = [v for _, v in vectors]
    ref_run, _ = generator("ref")
    cand_run, cand_r = generator(name)
    sig = RT.signature({"r1cs": str(COMPILED / f"{name}.r1cs"), "sym": str(COMPILED / f"{name}.sym")})
    return RT.compare_runs(vectors, ref_run(objs), cand_run(objs), cand_r, sig["outputs"], sig["output_names"])


def fake_env(root: str, pins: dict | None = None) -> L.LeanEnv:
    pins = pins or ENV
    return L.LeanEnv(os.path.join(root, "no-toolchain"), [], pins["lean"], {"mathlib": pins["mathlib"]},
                     pins["lake_manifest_sha256"], os.path.join(root, "scratch"))


class MetricTests(unittest.TestCase):
    def test_classification(self) -> None:
        cases = [
            (([], [], [(1, 1), (2, PR - 1)]), False),            # 0 * 0 = w1 - w2: linear
            (([(0, 5)], [(1, 1)], [(2, 1)]), False),             # 5 * w1 = w2: A constant
            (([(1, 1)], [(0, 7)], [(2, 1)]), False),             # w1 * 7 = w2: B constant
            (([(1, 0)], [(2, 1)], [(3, 1)]), False),             # A has a zero coefficient only
            (([(1, 1)], [(2, 1)], [(3, 1)]), True),              # w1 * w2 = w3
            (([(0, 3), (1, 1)], [(0, 1), (2, 1)], []), True),    # (3 + w1)(1 + w2) = 0
            (([(1, 1)], [(1, 1), (0, PR - 1)], []), True),       # w1 (w1 - 1) = 0 (boolean check)
        ]
        for c, want in cases:
            self.assertEqual(RT.nonlinear(c), want, c)

    def test_record_and_candidate_counts(self) -> None:
        self.assertEqual(RT.counts(str(COMPILED / "toy.O0.r1cs")), {"nonlinear": 2, "linear": 1, "total": 3})
        rec = RT.counts(str(COMPILED / "record.r1cs"))
        self.assertEqual(rec, {"nonlinear": 2, "linear": 0, "total": 2})
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            s = RT.score(rec, RT.counts(str(COMPILED / f"{name}.r1cs")))
            self.assertEqual((s["nonlinear"], s["smaller"], s["reduction_pct"]), (1, True, 50.0), name)

    def test_p1_target4_regression_linear_compression_does_not_count(self) -> None:
        # ratchet pilot P1, target 4 (BytesToBitsArray(93)): record 744 non-linear + 93 linear; the P1 candidate had
        # 806 constraints, all quadratic: fewer in total, more non-linear.  It must not count; P1 targets 2 and 5
        # (all non-linear) still count.
        rec4 = {"nonlinear": 744, "linear": 93, "total": 837}
        s = RT.score(rec4, {"nonlinear": 806, "linear": 0, "total": 806})
        self.assertFalse(s["smaller"])
        self.assertGreater(s["total_reduction_pct"], 0)
        self.assertTrue(RT.score({"nonlinear": 413, "linear": 0, "total": 413},
                                 {"nonlinear": 402, "linear": 0, "total": 402})["smaller"])
        self.assertTrue(RT.score({"nonlinear": 245, "linear": 0, "total": 245},
                                 {"nonlinear": 122, "linear": 0, "total": 122})["smaller"])
        # the same effect on concrete constraint systems: two products plus two linear copies vs. three products
        x = [(1, 1)]
        record = R.R1cs(PR, 32, 7, 2, 2, 0, 7, [([(3, 1)], [(4, 1)], [(5, 1)]), ([(3, 1)], [(3, 1)], [(6, 1)]),
                                                ([], [], [(1, 1), (5, PR - 1)]), ([], [], [(2, 1), (6, PR - 1)])])
        squashed = R.R1cs(PR, 32, 5, 2, 2, 0, 5, [([(3, 1)], [(4, 1)], x), ([(3, 1)], [(3, 1)], [(2, 1)]),
                                                  ([(1, 1)], [(0, 1), (2, 1)], [(1, 1), (3, 1)])])
        rc, cc = RT.counts_of(record), RT.counts_of(squashed)
        self.assertEqual((rc["nonlinear"], rc["total"], cc["nonlinear"], cc["total"]), (2, 4, 3, 3))
        self.assertFalse(RT.score(rc, cc)["smaller"])
        self.assertTrue(RT.score(rc, {"nonlinear": 1, "linear": 9, "total": 10})["smaller"])


class CanonicalOrderTests(unittest.TestCase):
    def test_shuffled_constraints_and_terms_canonicalize_identically(self) -> None:
        base = R.read_r1cs(str(COMPILED / "toy.O0.r1cs"))
        want = R.encode_r1cs(RT.canonicalize(base))
        rng = random.Random(7)
        for _ in range(20):
            cons = [tuple(rng.sample(list(lc), len(lc)) for lc in c) for c in base.constraints]
            rng.shuffle(cons)
            shuffled = R.R1cs(base.prime, base.field_bytes, base.n_wires, base.n_pub_out, base.n_pub_in,
                              base.n_prv_in, base.n_labels, cons, list(base.wire_to_label), list(base.section_order))
            self.assertEqual(R.encode_r1cs(RT.canonicalize(shuffled)), want)
        canon = RT.canonicalize(base)
        self.assertEqual(R.encode_r1cs(RT.canonicalize(canon)), want)                 # idempotent
        self.assertEqual((canon.n_wires, canon.n_pub_out, canon.n_prv_in, canon.wire_to_label),
                         (base.n_wires, base.n_pub_out, base.n_prv_in, base.wire_to_label))
        self.assertEqual(RT.counts_of(canon), RT.counts_of(base))

    def test_committed_o2_fixtures_are_canonical(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            for name in ("record", "equivalent", "nonequivalent", "underconstrained"):
                p = os.path.join(tmp, f"{name}.r1cs")
                shutil.copyfile(str(COMPILED / f"{name}.r1cs"), p)
                self.assertEqual(P.sha256_file(RT.canonical_r1cs(p)), P.sha256_file(p), name)


class AdmissibilityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)
        self.man = RT.verify_snapshot(RT.resolve_snapshot(self.pdir, self.prob, None),
                                      self.prob["snapshot"]["manifest_sha256"])

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def cand(self, text: str, extra: dict | None = None) -> str:
        d = tempfile.mkdtemp(dir=self.tmp)
        for name, t in (extra or {}).items():
            RT.put(os.path.join(d, name), t)
        RT.put(os.path.join(d, "Candidate.circom"), text)
        return os.path.join(d, "Candidate.circom")

    def admit(self, text: str, extra: dict | None = None):
        return RT.admit_sources(self.cand(text, extra), self.prob, self.man)

    def rejected(self, text: str, extra: dict | None = None) -> str:
        with self.assertRaises(RT.Reject) as cm:
            self.admit(text, extra)
        return str(cm.exception)

    def test_fixture_candidates_are_admissible(self) -> None:
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            srcs, snap, pragma = RT.admit_sources(str(CANDS / f"{name}.circom"), self.prob, self.man)
            self.assertEqual((sorted(srcs), snap, pragma), (["Candidate.circom"], [], "2.0.0"))
        srcs, snap, _ = self.admit('pragma circom 2.1.0;\ninclude "helper.circom";\ninclude "repo/toy.circom";\n'
                                   "template Toy() { signal input a; signal input b; signal output y; }\n",
                                   {"helper.circom": "template H() {}\n"})
        self.assertEqual((sorted(srcs), snap), (["Candidate.circom", "helper.circom"], ["repo/toy.circom"]))

    def test_rejections(self) -> None:
        toy = "template Toy() { signal input a; signal input b; signal output y; y <== a * b; }\n"
        self.assertIn("component main", self.rejected(toy + "component main = Toy();\n"))
        self.assertIn("custom_templates", self.rejected("pragma custom_templates;\n" + toy))
        self.assertIn("leaves the allowed", self.rejected('include "../toy.circom";\n' + toy))
        self.assertIn("leaves the allowed", self.rejected('include "/etc/x.circom";\n' + toy))
        self.assertIn("pinned repository snapshot", self.rejected('include "repo/other.circom";\n' + toy))
        self.assertIn("pinned repository snapshot", self.rejected('include "circomlib/poseidon.circom";\n' + toy))
        self.assertIn("pinned repository snapshot", self.rejected('include "helper.circom";\n' + toy))
        self.assertIn("component main", self.rejected('include "helper.circom";\n' + toy,
                                                      {"helper.circom": "component main = Toy();\n"}))
        self.assertIn("newer than the pinned compiler", self.rejected("pragma circom 2.3.0;\n" + toy))
        self.assertIn("declares no `template Toy`", self.rejected(toy.replace("Toy", "Toy2")))
        self.assertIn("declares no `template Toy`", self.rejected('include "repo/toy.circom";\n'))
        self.assertIn("1 parameter", self.rejected(toy.replace("Toy()", "Toy(n)")))

    def test_main_io_shape(self) -> None:
        io = self.prob["reference"]["io"]
        good = RT.signature({"r1cs": str(COMPILED / "equivalent.r1cs"), "sym": str(COMPILED / "equivalent.sym")})
        self.assertIsNone(RT.io_mismatch(io, good))
        for k, v in (("output_names", ["main.z"]), ("input_names", ["main.b", "main.a"]), ("prime", "7"),
                     ("n_pub_out", 2)):
            self.assertIn(k, RT.io_mismatch(io, dict(good, **{k: v})))
        self.assertIn("public", RT.io_mismatch(io, dict(good, n_inputs=3)))
        o0 = RT.signature({"r1cs": str(COMPILED / "toy.O0.r1cs"), "sym": str(COMPILED / "toy.O0.sym")})
        self.assertIn("n_pub_in", RT.io_mismatch(io, o0))           # private inputs (no public clause)

    def test_reference_eligibility(self) -> None:
        spec = RT.reference_spec(os.path.join(self.tmp, "registry", "toy.Toy"))
        self.assertIsNone(RT.static_exclusion(spec))
        for tag in ("v2.0.9", "v0.5.46"):
            self.assertEqual(RT.static_exclusion(dict(spec, compiler=dict(spec["compiler"], tag=tag)))[0], "compiler")
        self.assertEqual(RT.static_exclusion(dict(spec, tag_wrapper=True))[0], "tag-wrapper")
        self.assertEqual(RT.static_exclusion(dict(spec, property="DET-MOD"))[0], "property")
        self.assertEqual(RT.static_exclusion(dict(spec, n_constraints=4001))[0], "size")
        det, msha = self.prob["det"], spec["model"]["sha256"]
        self.assertIsNone(RT.det_exclusion(det, msha))
        self.assertEqual(RT.det_exclusion(None, msha)[0], "det")
        self.assertEqual(RT.det_exclusion(dict(det, checker=dict(det["checker"], verdict="FAIL")), msha)[0], "det")
        self.assertEqual(RT.det_exclusion(det, "0" * 64)[0], "det")
        self.assertEqual(RT.det_exclusion(dict(det, checker=dict(det["checker"], peak_rss_mb=20001)), msha)[0],
                         "det-limits")
        with self.assertRaises(RT.NotEligible) as cm:
            RT.compiler_binary({}, "v2.0.9")
        self.assertEqual(cm.exception.code, "compiler")


class StatementTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_open_problem_validates_and_has_no_statement(self) -> None:
        self.assertEqual(P.validate_problem(self.prob, self.pdir), [])
        self.assertFalse(P.has_statement(self.prob))
        self.assertTrue(P.has_statement({"status": "OPEN"}))                     # DET packages unchanged
        self.assertFalse(P.has_statement({"status": "TOO-LARGE"}))
        cand, _ = compiled_candidate(self.prob, "equivalent")
        self.assertIn("OPEN ratchet problem must not carry 'candidate'",
                      P.validate_problem(dict(self.prob, candidate=cand), self.pdir))
        bad = dict(self.prob, det=dict(self.prob["det"], model_sha256="0" * 64))
        self.assertIn("DET evidence is not about the reference model file", P.validate_problem(bad))
        Path(self.pdir, self.prob["reference"]["model"]["file"]).write_text("tampered\n", encoding="utf-8")
        self.assertTrue(any("sha256 mismatch" in e for e in P.validate_problem(self.prob, self.pdir)))

    def test_statement_states_both_directions_of_the_io_relation(self) -> None:
        pkg, stmt = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        ref_ns = self.prob["reference"]["model"]["namespace"]
        base = "ZkRatchet." + ref_ns.split(".", 1)[1]
        self.assertTrue(stmt.startswith(f"import {ref_ns}.Model\nimport {base}.Cand.Model\n"))
        st = C.parse_statement(stmt, "equiv")
        self.assertEqual(st["body"], " by\n  sorry")
        sig = st["sig"]
        left = (f"(∃ w : Fin {ref_ns}.nWires → {ref_ns}.F, {ref_ns}.Constraints w ∧ {ref_ns}.Inputs.map w = x ∧ "
                f"{ref_ns}.Outputs.map w = y)")
        right = "(∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y)"
        self.assertIn(f"{left} ↔\n      {right}", sig)
        self.assertIn(f"∀ x y : List {ref_ns}.F", sig)
        self.assertIn(f"[Fact (Nat.Prime {ref_ns}.p)]", sig)
        cand, _ = compiled_candidate(self.prob, "equivalent")
        for digest in (cand["source_sha256"], cand["r1cs_sha256"], self.prob["reference"]["model"]["r1cs_sha256"]):
            self.assertIn(digest, stmt)
        cprob = RT.read_json(os.path.join(pkg, "problem.json"))
        self.assertEqual(P.validate_problem(cprob, pkg), [])
        self.assertTrue(P.has_statement(cprob))
        self.assertEqual([f["role"] for f in cprob["checker"]["files"]], ["statement", "import", "import"])
        cmodel = Path(pkg, E.model_relpath(base + ".Cand")).read_text(encoding="utf-8")
        self.assertIn("# Ratchet candidate model:", cmodel)
        self.assertIn("(canonical constraint order)", cmodel)
        # the statement is a function of the candidate: same inputs, same bytes
        _, again = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq2"))
        self.assertEqual(again, stmt)

    def test_candidate_package_semantics(self) -> None:
        pkg, _ = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        cprob = RT.read_json(os.path.join(pkg, "problem.json"))
        wrong = dict(cprob, candidate=dict(cprob["candidate"], smaller=False))
        self.assertIn("candidate.smaller disagrees with the record", P.validate_problem(wrong))
        files = [dict(f, sha256="0" * 64) if f["path"] == self.prob["reference"]["model"]["file"] else f
                 for f in cprob["checker"]["files"]]
        wrong = dict(cprob, checker=dict(cprob["checker"], files=files))
        self.assertTrue(any("reference model file" in e for e in P.validate_problem(wrong)))
        with open(os.path.join(pkg, "Statement.lean"), "a", encoding="utf-8") as f:
            f.write("\n")
        self.assertTrue(P.validate_problem(cprob, pkg))

    def test_open_problem_is_not_checkable(self) -> None:
        sol = os.path.join(self.tmp, "Solution.lean")
        shutil.copyfile(str(FIX / "Solution.lean"), sol)
        rep = C.check(self.pdir, sol, fake_env(self.tmp))
        self.assertEqual(rep["verdict"], "ERROR")
        self.assertIn("has no Lean statement", rep["error"][0])

    def test_battery_det_solution_follows_the_text_rules(self) -> None:
        stmt = Path(self.tmp, "registry", "toy.Toy", "Statement.lean").read_text(encoding="utf-8")
        for form in ("triv_V0_grind", "triv_V1_simp_all", "triv_V3_grind", "triv_V4_simp_all"):
            sol = RT.battery_det_solution(stmt, 3, form)
            probs, aux, body = C.text_check(stmt, sol, "det")
            self.assertEqual((probs, aux), ([], ""), form)
            self.assertEqual(C.forbidden_scan([("proof", body)]), [], form)
        self.assertIn("all_goals simp_all", RT.battery_det_solution(stmt, 3, "triv_V4_simp_all"))
        for form in ("triv_V0_bv_decide", "triv_V1_exactQ", "nonsense"):
            with self.assertRaises(ValueError):
                RT.battery_det_solution(stmt, 3, form)


class ScenarioTests(unittest.TestCase):
    """The three fixture candidates through the screen and the checker paths that need no Lean."""

    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pdir, self.prob = offline_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_simulation_vectors_are_deterministic_and_prefix_stable(self) -> None:
        a, b = RT.sim_vectors(self.prob, 50), RT.sim_vectors(self.prob, 80)
        self.assertEqual(a, b[:len(a)])
        self.assertEqual(sum(1 for lb, _ in a if lb.startswith("edge")), len(a) - 50)
        other = dict(self.prob, simulate=dict(self.prob["simulate"], seed="another seed"))
        self.assertNotEqual(RT.sim_vectors(other, 50), a)

    def test_equivalent_candidate_passes_screen_and_its_committed_proof_follows_the_rules(self) -> None:
        sim = screen(self.prob, "equivalent")
        self.assertEqual((sim["verdict"], sim["mismatches"]), ("PASS", 0))
        pkg, stmt = candidate_package(self.pdir, self.prob, "equivalent", os.path.join(self.tmp, "eq"))
        proof = (FIX / "Solution.lean").read_text(encoding="utf-8")
        probs, aux, body = C.text_check(stmt, proof, "equiv")
        self.assertEqual(probs, [])
        self.assertEqual(C.forbidden_scan([("helper declarations", aux), ("proof", body)]), [])
        self.assertEqual(RT.det_screen(self.prob, Path(pkg, E.model_relpath(RT.namespaces(self.prob)[2]))
                                       .read_text(encoding="utf-8"))["propagation"], "DETERMINED")

    def test_nonequivalent_smaller_candidate_is_caught_by_the_screen(self) -> None:
        cand, _ = compiled_candidate(self.prob, "nonequivalent")
        self.assertTrue(cand["smaller"])
        sim = screen(self.prob, "nonequivalent")
        self.assertEqual(sim["verdict"], "MISMATCH")
        self.assertGreater(sim["mismatch_kinds"]["different outputs"], 0)
        self.assertEqual(sim["examples"][0]["kind"], "different outputs")

    def test_underconstrained_candidate_passes_the_screen_but_its_sorry_proof_is_rejected(self) -> None:
        sim = screen(self.prob, "underconstrained")
        self.assertEqual((sim["verdict"], sim["mismatches"]), ("PASS", 0))
        pkg, stmt = candidate_package(self.pdir, self.prob, "underconstrained", os.path.join(self.tmp, "uc"))
        model = Path(pkg, E.model_relpath(RT.namespaces(self.prob)[2])).read_text(encoding="utf-8")
        self.assertEqual(RT.det_screen(self.prob, model)["propagation"], "STUCK")      # the hint is free
        sol = os.path.join(self.tmp, "uc-solver", "Solution.lean")
        RT.put(sol, stmt)                                                                 # proof: `sorry`
        rep = C.check(pkg, sol, fake_env(self.tmp))
        self.assertEqual(rep["verdict"], "INVALID")
        self.assertEqual(rep["compile"], "skipped")
        self.assertTrue(any(x.startswith("sorry in proof") for x in rep["invalid"]))

    def test_structural_screen_flags_an_output_in_no_constraint(self) -> None:
        _, r = generator("equivalent")
        free = R.R1cs(r.prime, r.field_bytes, r.n_wires, r.n_pub_out, r.n_pub_in, r.n_prv_in, r.n_labels,
                      [([(2, 1)], [(3, 1)], [(0, 1)])], list(r.wire_to_label))
        sim = RT.compare_runs([], [], [], free, [1], ["main.y"])
        self.assertEqual((sim["verdict"], sim["unconstrained_outputs"]), ("MISMATCH", ["main.y"]))


@unittest.skipUnless(LIVE_COMPILERS and LIVE_LEAN, "set BOOLE_ZK_RATCHET_COMPILERS and BOOLE_ZK_RATCHET_LEAN_ENV")
class LiveToolchainTests(unittest.TestCase):
    """The whole pipeline with the real tools: build, count, simulate (wasm + node), statement elaboration, the
    reference's DET proof and the final check by the production checker."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.tmp = tempfile.mkdtemp()
        cls.compilers = RT.read_json(LIVE_COMPILERS)
        L.set_lean_limits(slots=1, rss_mb=RT.DET_PROOF_LIMITS["rss_mb"])
        cls.env = L.load_env(LIVE_LEAN)
        pins = cls.env.pins()
        env = dict(ENV, lean=pins["lean"], mathlib=pins["mathlib"], lake_manifest_sha256=pins["lake_manifest_sha256"],
                   packages=pins["packages"])
        reg = registry_package(cls.tmp, env)
        spec = RT.reference_spec(reg)
        gate = G.g_elab(cls.env, reg, os.path.join(cls.tmp, "elab", "build"), spec["model"]["namespace"],
                        os.path.join(cls.tmp, "elab"))
        assert gate.status == "PASS", gate.detail
        cls.reg = registry_package(cls.tmp, env, gate.detail["reference_type_sha256"])
        sol = os.path.join(cls.tmp, "registry", "det", "Solution.lean")
        M.solve(cls.reg, sol)
        chk = C.check(cls.reg, sol, cls.env, None, 1800, False, os.path.join(cls.tmp, "detcheck"))
        cls.det_verdict = chk["verdict"]
        det = RT.det_record("mech-p3", RT.reference_spec(cls.reg), sol, {"verdict": chk["verdict"]}, "live test")
        cls.snap = snapshot_into(cls.tmp)
        cls.pdir = os.path.join(cls.tmp, "problems", "toy")
        cls.prob = RT.build_problem(cls.reg, cls.snap, cls.pdir, cls.compilers, det,
                                    work=os.path.join(cls.tmp, "build"))

    @classmethod
    def tearDownClass(cls) -> None:
        shutil.rmtree(cls.tmp)

    def out(self, name: str) -> str:
        return os.path.join(self.tmp, "runs", name)

    def test_build_matches_the_committed_fixtures(self) -> None:
        self.assertEqual(self.det_verdict, "PASS")
        rec = self.prob["record"]
        self.assertEqual((rec["nonlinear"], rec["linear"], rec["total"]), (2, 0, 2))
        self.assertEqual(rec["r1cs_sha256"], P.sha256_file(str(COMPILED / "record.r1cs")))
        self.assertEqual(self.prob["reference"]["io"]["public_inputs"], ["a", "b"])
        self.assertEqual(self.prob["simulate"]["max_bits"], {"a": 254, "b": 254})
        self.assertEqual(P.validate_problem(self.prob, self.pdir), [])

    def test_canonical_o2_is_stable_across_compiles(self) -> None:
        digests = set()
        for k in range(3):
            rep, _ = RT.compile_candidate(self.prob, str(CANDS / "equivalent.circom"), self.out(f"canon{k}"),
                                          self.snap, self.compilers)
            digests.add(rep["candidate"]["r1cs_sha256"])
        self.assertEqual(digests, {P.sha256_file(str(COMPILED / "equivalent.r1cs"))})

    def test_simulation_screen_with_the_real_generators(self) -> None:
        verdicts = {}
        for name in ("equivalent", "nonequivalent", "underconstrained"):
            rep = RT.simulate(self.pdir, str(CANDS / f"{name}.circom"), self.out("sim-" + name), 200, self.compilers)
            verdicts[name] = rep["simulate"]["verdict"]
        self.assertEqual(verdicts, {"equivalent": "PASS", "nonequivalent": "MISMATCH", "underconstrained": "PASS"})

    def test_final_checks(self) -> None:
        verdict, rep = RT.run_check(self.pdir, str(CANDS / "equivalent.circom"), str(FIX / "Solution.lean"),
                                    self.out("check-eq"), self.compilers, self.env)
        self.assertEqual(verdict, "PASS", rep.get("checker"))
        cr = RT.run_candidate(self.pdir, str(CANDS / "underconstrained.circom"), self.out("stmt-uc"), self.compilers,
                              env=self.env)
        sorry = os.path.join(self.tmp, "uc-solver", "Solution.lean")
        RT.put(sorry, Path(cr["package"], "Statement.lean").read_text(encoding="utf-8"))
        verdict, rep = RT.run_check(self.pdir, str(CANDS / "underconstrained.circom"), sorry, self.out("check-uc"),
                                    self.compilers, self.env)
        self.assertEqual(verdict, "INVALID")
        verdict, rep = RT.run_check(self.pdir, str(FIX / "snapshot" / "repo" / "toy.circom"),
                                    str(FIX / "Solution.lean"), self.out("check-ref"), self.compilers, self.env)
        self.assertEqual(verdict, "REJECTED")                   # the reference itself: not smaller (2 = 2)


if __name__ == "__main__":
    unittest.main()
