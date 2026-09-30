#!/usr/bin/env python3
"""problem.json schema, validator, identities, wave selection and summary (offline)."""
from __future__ import annotations

import copy
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_det as D        # noqa: E402
from zk_registry import jsonschema_lite as J   # noqa: E402
from zk_registry import lean_emit as E         # noqa: E402
from zk_registry import package as P           # noqa: E402
from zk_registry import r1cs as R              # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-circuit"
H = "ab" * 32
ENV = {"circom": "2.2.3", "circom_binary_sha256": H, "lean": "v4.33.1", "mathlib": "c" * 40,
       "lake_manifest_sha256": H, "packages": {"mathlib": "c" * 40}, "node": "v22", "python": "3.9"}


def make_package(root: str, status: str = "OPEN") -> tuple[str, dict]:
    """A package built from the toy fixture with the same emitters the wave uses."""
    r = R.read_r1cs(str(FIX / "toy.r1cs"))
    syms = R.read_sym(str(FIX / "toy.sym"))
    io = R.main_io_wires(r, syms)
    ns = P.lean_namespace("toy-v1", "toy.Toy")
    meta = {"repo_id": "toy/repo", "instantiation": "Toy()", "generator": "gen v1", "repo_url": "https://example.invalid/t",
            "commit": "0" * 40, "path": "toy.circom", "template": "Toy", "rule": "parameter-free",
            "circom_version": "2.2.3", "circom_flags": D.CIRCOM_FLAGS, "r1cs_sha256": H, "prime_name": "bn128"}
    pkg = os.path.join(root, "toy.Toy")
    model_rel = E.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(pkg, model_rel)))
    Path(pkg, model_rel).write_text(E.emit_model(ns, meta, r, io.outputs, io.inputs, R.wire_names(syms, r.n_wires)),
                                    encoding="utf-8")
    statement = E.emit_statement(ns, meta)
    Path(pkg, "Statement.lean").write_text(statement, encoding="utf-8")
    passed = {"status": "PASS"}
    rec = {
        "schema_version": P.SCHEMA_VERSION, "package_id": "toy-v1/toy.Toy", "property": dict(P.DET_PROPERTY),
        "status": status, "status_reason": "fixture",
        "ids": {"ledger_item_id": "toy/repo:toy.circom#Toy", "repo": "toy/repo", "repo_url": "https://example.invalid/t",
                "release": "v1", "commit": "0" * 40, "path": "toy.circom", "template": "Toy", "template_line": 6,
                "source_sha256": H},
        "instantiation": {"rule": "parameter-free", "args": [], "call": "Toy()", "provenance": ["toy.circom:6 Toy()"],
                          "selection": "only candidate", "candidates": [{"tier": "parameter-free", "call": "Toy()",
                                                                         "compile": "ok", "constraints": 2, "wires": 7}]},
        "spec": dict(P.DET_SPEC),
        "circuit": {"compiler": {"name": "circom", "version": "2.2.3", "flags": D.CIRCOM_FLAGS, "binary_sha256": H},
                    "prime": str(R.BN254_SCALAR), "prime_name": "bn128", "n_constraints": 2, "n_wires": 7,
                    "n_inputs": 3, "n_outputs": 2, "r1cs_sha256": H,
                    "size_policy": {"max_constraints": P.MAX_CONSTRAINTS, "within": True}},
        "statement": {"file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                      "model_module": E.model_module(ns), "model_file": model_rel, "text": statement,
                      "assumptions": list(P.STATEMENT_ASSUMPTIONS), "truth": "unknown"},
        "gates": {"G-ELAB": dict(passed), "G-NONVAC": dict(passed), "G-FID": dict(passed), "G-TRIV": dict(passed),
                  "DET-SEARCH": dict(passed)},
        "checker": {"statement_file": "Statement.lean", "theorem": "det", "theorem_fqn": f"{ns}.det",
                    "lean_opts": list(E.LEAN_OPTIONS),
                    "files": [{"path": "Statement.lean", "role": "statement",
                               "sha256": P.sha256_file(os.path.join(pkg, "Statement.lean"))},
                              {"path": model_rel, "role": "import", "module": E.model_module(ns),
                               "sha256": P.sha256_file(os.path.join(pkg, model_rel))}],
                    "reference_type_sha256": H, "replay_tool_sha256": P.sha256_file(D.G.REPLAY_TOOL),
                    "allowed_axioms": ["Classical.choice", "Quot.sound", "propext"],
                    "forbidden_tokens": D.C.FORBIDDEN_LABELS},
        "env": dict(ENV), "generator": P.generator_info(), "evidence": {},
    }
    P.write_json(os.path.join(pkg, "problem.json"), rec)
    return pkg, rec


class SchemaValidatorTests(unittest.TestCase):
    def test_subset_keywords(self) -> None:
        schema = {"type": "object", "required": ["a"], "additionalProperties": False,
                  "properties": {"a": {"type": "integer", "minimum": 1}, "b": {"enum": ["x"]}},
                  "if": {"properties": {"a": {"const": 2}}}, "then": {"required": ["b"]}}
        self.assertEqual(J.validate({"a": 1}, schema), [])
        self.assertEqual(J.validate({"a": 2}, schema), ["$: missing required property 'b'"])
        self.assertEqual(len(J.validate({"a": 0, "c": 1}, schema)), 2)
        self.assertEqual(J.validate(True, {"type": "integer"}), ["$: expected integer"])

    def test_unsupported_keywords_are_schema_errors(self) -> None:
        with self.assertRaises(J.SchemaError):
            J.validate({}, {"uniqueItems": True})
        with self.assertRaises(J.SchemaError):
            J.validate({}, {"$ref": "http://x/y"})

    def test_schema_file_uses_only_supported_keywords(self) -> None:
        schema = P.load_schema()
        self.assertEqual(J.validate({}, schema)[:1], ["$: missing required property 'schema_version'"])


class ProblemValidationTests(unittest.TestCase):
    def test_open_package_validates_with_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            self.assertEqual(P.validate_problem(rec, pkg), [])

    def test_open_requires_passing_gates_and_unknown_truth(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, rec = make_package(tmp)
            bad = copy.deepcopy(rec)
            bad["gates"]["G-TRIV"]["status"] = "FAIL"
            self.assertTrue(P.validate_problem(bad))
            bad = copy.deepcopy(rec)
            del bad["gates"]["DET-SEARCH"]
            self.assertTrue(P.validate_problem(bad))
            bad = copy.deepcopy(rec)
            bad["statement"]["truth"] = "closed-by-automation"
            self.assertTrue(P.validate_problem(bad))
            gf = copy.deepcopy(rec)
            gf["status"] = "GATE-FAIL"
            gf["gates"]["G-TRIV"]["status"] = "FAIL"
            gf["statement"]["truth"] = "closed-by-automation"
            self.assertEqual(P.validate_problem(gf), [])

    def test_file_hash_and_statement_text_are_checked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            model = os.path.join(pkg, rec["statement"]["model_file"])
            with open(model, "a", encoding="utf-8") as f:
                f.write("-- edit\n")
            self.assertIn(f"sha256 mismatch for {rec['statement']['model_file']}", P.validate_problem(rec, pkg))
            rec2 = copy.deepcopy(rec)
            rec2["statement"]["text"] += " "
            self.assertIn("statement.text differs from Statement.lean", P.validate_problem(rec2, pkg))

    def test_index_only_statuses(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, rec = make_package(tmp)
            tl = {k: v for k, v in rec.items() if k not in ("statement", "checker", "gates")}
            tl["status"] = "TOO-LARGE"
            self.assertTrue(P.validate_problem(tl))            # within the size policy
            tl["circuit"] = dict(tl["circuit"], n_constraints=5000,
                                 size_policy={"max_constraints": P.MAX_CONSTRAINTS, "within": False})
            self.assertEqual(P.validate_problem(tl), [])
            un = {k: v for k, v in rec.items() if k not in ("circuit", "gates")}
            un["status"] = "UNINSTANTIABLE"
            self.assertIn("UNINSTANTIABLE record must not carry 'statement'", P.validate_problem(un))


class IdentityTests(unittest.TestCase):
    def test_package_names_and_namespaces(self) -> None:
        self.assertEqual(P.package_dir_name("circuits/bitify.circom", "Num2Bits", ("253",)), "bitify.Num2Bits.253")
        self.assertEqual(P.package_dir_name("circuits/smt/smtlevins.circom", "SMTLevIns", ("10",)),
                         "smt.smtlevins.SMTLevIns.10")
        self.assertEqual(P.package_dir_name("circuits/compconstant.circom", "CompConstant", ("-1",)),
                         "compconstant.CompConstant.m1")
        self.assertRegex(P.package_dir_name("circuits/x.circom", "T", ("16+1", "POSEIDON_C(17)")), r"^x\.T\.h[0-9a-f]{10}$")
        self.assertEqual(P.lean_namespace("circomlib-v2.0.5", "bitify.Num2Bits.253"),
                         "ZkDet.circomlib_v2_0_5_bitify_Num2Bits_253")

    def test_generator_hash_covers_the_sources(self) -> None:
        info = P.generator_info()
        self.assertRegex(info["sources_sha256"], r"^[0-9a-f]{64}$")
        here = Path(__file__).resolve().parent / "zk_registry"
        listed = set(P.GENERATOR_SOURCES)
        present = {str(p.relative_to(here)) for p in here.rglob("*") if p.is_file() and p.suffix in (".py", ".js", ".lean", ".json")}
        # the AIR generator (air_det) hashes its own sources; every file belongs to one of the two generators
        from zk_registry import air_det
        self.assertEqual(present - listed - set(air_det.GENERATOR_SOURCES), set())


class WaveHelpersTests(unittest.TestCase):
    def rec(self, tier, call, n, w=10):
        return {"tier": tier, "call": call, "compile_result": {"constraints": n, "wires": w}}

    def test_selection_prefers_the_largest_candidate_within_the_policy(self) -> None:
        recs = [self.rec("repo-test", "T(1)", 2), self.rec("repo-test", "T(253)", 254), self.rec("repo-test", "T(900)", 2500)]
        chosen, fits = D.select(recs, "repo-test", 2000)
        self.assertEqual((chosen["call"], fits), ("T(253)", True))
        chosen, fits = D.select([self.rec("repo-main", "S(512)", 9000), self.rec("repo-main", "S(448)", 8000)], "repo-main", 2000)
        self.assertEqual((chosen["call"], fits), ("S(448)", False))

    def test_summary_counts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, rec = make_package(tmp)
        tl = {k: v for k, v in rec.items() if k not in ("statement", "checker", "gates")}
        tl.update(status="TOO-LARGE", circuit=dict(rec["circuit"], n_constraints=40000))
        gf = copy.deepcopy(rec)
        gf.update(status="GATE-FAIL")
        gf["gates"]["G-TRIV"] = {"status": "FAIL", "closed_by": ["triv_V1_grind"]}
        s = D.summarize([rec, tl, gf])
        self.assertEqual(s["by_status"], {"OPEN": 1, "TOO-LARGE": 1, "UNINSTANTIABLE": 0, "GATE-FAIL": 1,
                                          "DET-FALSE-CANDIDATE": 0})
        self.assertEqual({h["bucket"]: h["count"] for h in s["constraint_histogram"]}["1-10"], 2)
        self.assertEqual({h["bucket"]: h["count"] for h in s["constraint_histogram"]}["10001-100000"], 1)
        self.assertEqual(s["gates"]["G-TRIV"], {"PASS": 1, "FAIL": 1})
        self.assertEqual(s["triv_closures"], [{"package_id": "toy-v1/toy.Toy", "closed_by": ["triv_V1_grind"]}])
        self.assertIn("| OPEN | 1 |", D.render_report(s, [rec, tl, gf], {"repo": "toy"}))

    def test_index_validation_detects_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            index = os.path.join(tmp, "INDEX.jsonl")
            Path(index).write_text(json.dumps(rec) + "\n", encoding="utf-8")
            self.assertEqual(D.validate_all(index, tmp), [])
            rec["status_reason"] = "changed"
            Path(index).write_text(json.dumps(rec) + "\n", encoding="utf-8")
            self.assertEqual(D.validate_all(index, tmp), ["line 1 toy-v1/toy.Toy: problem.json differs from the index record"])

    def test_combined_index_validates_against_the_collections_root(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "toy-v1"))
            _, rec = make_package(os.path.join(tmp, "toy-v1"))
            index = os.path.join(tmp, "INDEX.jsonl")
            Path(index).write_text(json.dumps(rec) + "\n", encoding="utf-8")
            self.assertEqual(D.validate_all(index, tmp, collections_root=True), [])
            self.assertIn("package directory missing", D.validate_all(index, tmp)[0])


class Wave1FixTests(unittest.TestCase):
    """Regression tests for the wave-1 generator fixes (library paths, primes, main blanking,
    dependencies, ledger id prefixes, uncompilable template sources)."""

    def cfg(self, repo_dir: str, **kw) -> D.WaveConfig:
        base = dict(repo_dir=repo_dir, repo_id="toy/repo", repo_url="https://example.invalid/t", release="r",
                    commit="0" * 40, collection="toy-v1", scope_prefix="", exclude=[], circom="circom",
                    circom_version_tag="v2.2.3", lean_env="env.json", node="node", work="w", out="o")
        base.update(kw)
        return D.WaveConfig(**base)

    def shared(self, cfg: D.WaveConfig) -> D.Shared:
        from zk_registry import circom_source as cs
        from zk_registry import lean_runner as L
        env = L.LeanEnv("/tc", [], "v4.33.1", {"mathlib": "c" * 40}, H, "/scratch")
        files = cs.scan_repo(cfg.repo_dir, cs.list_circom_files(cfg.repo_dir))
        return D.Shared(cfg, env, files, set(), None, H, "2.2.3", "v22", {"name": "g", "version": "1",
                                                                          "sources_sha256": H})

    def test_path_rules_select_prime_and_library_paths(self) -> None:
        cfg = self.cfg("/r", include_paths=["node_modules"], path_rules=[
            {"prefix": "circuits.gl/", "prime": "goldilocks", "include_paths": ["circuits.gl"]},
            {"prefix": "circuits.bn128/", "include_paths": ["circuits.bn128", "node_modules/circomlib/circuits"]}])
        self.assertEqual(D.build_options(cfg, "circuits.gl/fft.circom")[:2], ("goldilocks", ["circuits.gl"]))
        self.assertEqual(D.build_options(cfg, "circuits.bn128/fft.circom")[:2],
                         (None, ["circuits.bn128", "node_modules/circomlib/circuits"]))
        self.assertEqual(D.build_options(cfg, "src/x.circom")[:2], (None, ["node_modules"]))
        # recorded flags are repository-relative; the compiler gets absolute library paths
        self.assertEqual(D.option_flags("goldilocks", ["circuits.gl"]), ["--prime", "goldilocks", "-l", "circuits.gl"])
        self.assertEqual(D.option_flags(None, ["../node_modules"], "/a/repo"), ["-l", "/a/node_modules"])
        self.assertEqual(D.option_flags(None, []), [])

    def test_statement_assumptions_follow_the_prime(self) -> None:
        self.assertEqual(P.statement_assumptions("bn128"), [
            "[Fact (Nat.Prime p)]: primality of the circom prime, supplied as an instance hypothesis so that field "
            "lemmas apply; p is the published BN254 scalar field order (prime), so the hypothesis does not "
            "weaken the statement."])                                  # byte-identical to wave 0
        self.assertIn("Goldilocks prime 2^64 - 2^32 + 1", P.statement_assumptions("goldilocks")[0])
        self.assertIn("BLS12-381", P.statement_assumptions("bls12381")[0])

    def test_main_free_copy_blanks_only_the_main_declaration(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            src = 'pragma circom 2.0.0;\ntemplate A(n) { signal input x; signal output y; y <== x; }\n' \
                  '// component main = A(9);\ncomponent main {public [x]} = A(\n  3);\n'
            Path(tmp, "a.circom").write_text(src, encoding="utf-8")
            cfg = self.cfg(tmp)
            sh = self.shared(cfg)
            copy_path = D.main_free_copy(sh, "a.circom")
            self.assertEqual(copy_path, os.path.join(tmp, "a.circom.boole-nomain"))
            text = Path(copy_path).read_text(encoding="utf-8")
            self.assertEqual((len(text), text.count("\n")), (len(src), src.count("\n")))
            self.assertIn("template A(n) { signal input x;", text)
            self.assertNotIn("public", text)
            self.assertIn("// component main = A(9);", text)           # comments are left alone
            from zk_registry import circom_source as cs
            self.assertEqual(cs.find_templates(cs.strip_comments(text), "a")[0].name, "A")
            self.assertFalse(cs._MAIN_RE.search(cs.strip_comments(text)))
            self.assertNotIn("a.circom.boole-nomain", cs.list_circom_files(tmp))   # never scanned as a source
            Path(tmp, "b.circom").write_text("template B() {}\n", encoding="utf-8")
            self.assertIsNone(D.main_free_copy(self.shared(cfg), "b.circom"))

    def test_record_for_a_template_in_a_non_circom_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "v.circom.ejs").write_text("<% x %>\ntemplate Main() {\n}\n", encoding="utf-8")
            deps = [{"name": "circomlib", "version": "2.0.5", "source": "https://registry.npmjs.org/c.tgz",
                     "path": "node_modules/circomlib", "tarball_sha256": H, "resolved_by": "lockfile yarn.lock"}]
            cfg = self.cfg(tmp, ledger_item_prefix="toy:CC/", dependencies=deps)
            rec = D.missing_record(self.shared(cfg), "v.circom.ejs#Main")
            self.assertEqual(rec["status"], "UNINSTANTIABLE")
            self.assertEqual(rec["ids"]["ledger_item_id"], "toy:CC/v.circom.ejs#Main")
            self.assertEqual(rec["ids"]["template_line"], 2)
            self.assertIn("not a .circom source", rec["status_reason"])
            self.assertEqual(P.validate_problem(rec), [])
            bad = copy.deepcopy(rec)
            bad["ids"]["dependencies"][0]["extra"] = 1
            self.assertTrue(P.validate_problem(bad))

    def test_compile_guard_stops_on_the_watch_hook_and_output_size(self) -> None:
        from zk_registry import lean_runner as L
        with tempfile.TemporaryDirectory() as tmp:
            r = L.run_process(["sleep", "30"], dict(os.environ), tmp, 60, watch=lambda: "stop now", poll_s=0.05)
            self.assertEqual(r.killed_by, "stop now")
            self.assertLess(r.secs, 10)
            self.assertEqual(D.output_guard(tmp, 1), "")
            Path(tmp, "main.r1cs").write_bytes(b"\0" * (1024 * 1024 + 1))
            self.assertEqual(D.output_guard(tmp, 1), "compiler output > 1 MB")
            self.assertEqual(D.output_guard(tmp, 2), "")

    def test_a_guard_stopped_candidate_decides_the_tier(self) -> None:
        from zk_registry import instantiation as I
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "a.circom").write_text("template A(n) {\n signal input x;\n}\n", encoding="utf-8")
            cfg = self.cfg(tmp, work=os.path.join(tmp, "w"), out=os.path.join(tmp, "o"), keep_work=False)
            sh = self.shared(cfg)
            t = sh.files["a.circom"].templates[0]
            plan = I.TemplatePlan(t, {"repo-main": [I.Candidate("repo-main", ("900",), ["a:1"])],
                                      "repo-test": [I.Candidate("repo-test", ("2",), ["b:1"])]})

            def fake_compile(sh_, workdir, include_rel, template, args, full, rule_path=None, compiler=None,
                             tag_template=None):
                os.makedirs(workdir, exist_ok=True)
                res = {"rc": 0, "include_context": include_rel, "main_sha256": H, "flags": D.compile_flags("circom2", full)}
                if args == ("900",):
                    return dict(res, rc=-1, guard="stopped by the resource guard (compiler output > 1536 MB)",
                                error="stopped by the resource guard (compiler output > 1536 MB)")
                return dict(res, constraints=10, wires=12, r1cs_sha256=H)

            saved = D.compile_main
            D.compile_main = fake_compile
            try:
                tier, records = D.size_candidates(sh, plan, os.path.join(tmp, "w"))
                self.assertEqual((tier, len(records)), ("repo-main", 1))        # the repo-test tier is not reached
                rec = D.process_template(sh, plan)
            finally:
                D.compile_main = saved
            self.assertEqual(rec["status"], "UNINSTANTIABLE")
            self.assertIn("resource guard", rec["status_reason"])
            self.assertEqual(rec["instantiation"]["candidates"][0]["compile"], "error")
            self.assertEqual(P.validate_problem(rec), [])
            self.assertFalse(os.path.exists(os.path.join(tmp, "w", "items", "a.A")))   # keep_work=False

    def test_scrub_removes_truncated_and_unknown_local_paths(self) -> None:
        from types import SimpleNamespace
        home = os.path.expanduser("~")
        sh = SimpleNamespace(cfg=SimpleNamespace(work="/w/work", repo_dir="/w/repo"),
                             env=SimpleNamespace(scratch="/w/scratch", toolchain="/w/tc"))
        rec = {"evidence": {"generator_errors": {
            "Constraint doesn't match 5 != 53 /private/tmp/sess-x/scratch/repos/r/circ": 1,   # truncated at 120
            "err /w/repo/a/b.circom:3": 2, f"at {home}/projects/x.js:1": 3, "!= 0 /private/tmp": 4,
            'quoted "/tmp/q/r" end': 5}},
            "ids": {"repo_url": "https://github.com/Users/tmp"}}
        out = json.dumps(D.scrub(sh, rec))
        for leak in ("/private/tmp", home, "sess-x"):
            self.assertNotIn(leak, out)
        self.assertIn("err $REPO/a/b.circom:3", out)
        self.assertIn("https://github.com/Users/tmp", out)
        self.assertIn('quoted \\"$LOCAL\\" end', out)                      # JSON escapes survive
        from zk_registry import witness as W
        self.assertEqual(W.error_key("Error: Constraint doesn't match 5 != 53 /x/y/scratch/repos/r/mux.circom:12:4\nmore"),
                         "Error: Constraint doesn't match 5 != 53 mux.circom:12:4")

    def test_schema_accepts_the_main_removal_flag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            rec["instantiation"]["include_main_removed"] = True
            self.assertEqual(P.validate_problem(rec, pkg), [])
            rec["instantiation"]["include_main_removed"] = "yes"
            self.assertTrue(P.validate_problem(rec, pkg))


if __name__ == "__main__":
    unittest.main()
