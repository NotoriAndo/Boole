#!/usr/bin/env python3
"""Generalized checker tests: text rules, forbidden constructs, verdict mapping and the early
(pre-compile) paths of check(); Lean itself is not needed."""
from __future__ import annotations

import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from test_zk_registry_package import ENV, make_package  # noqa: E402
from zk_registry import check as C                        # noqa: E402
from zk_registry import gates as G                        # noqa: E402
from zk_registry import lean_runner as L                  # noqa: E402

H = "ab" * 32


def fake_env(root: str, **pins) -> L.LeanEnv:
    packages = {"mathlib": pins.get("mathlib", ENV["mathlib"])}
    return L.LeanEnv(os.path.join(root, "no-toolchain"), [], pins.get("lean", ENV["lean"]), packages,
                     pins.get("manifest", ENV["lake_manifest_sha256"]), os.path.join(root, "scratch"))


def submission(statement: str, proof: str, helper: str = "") -> str:
    head, tail = statement.split("/-- Output determinism", 1)
    tail = "/-- Output determinism" + tail
    return head + helper + tail.replace("  sorry\n", proof, 1)


class TextRuleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pkg, self.rec = make_package(self.tmp)
        self.stmt = Path(self.pkg, "Statement.lean").read_text(encoding="utf-8")

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_statement_structure(self) -> None:
        st = C.parse_statement(self.stmt, "det")
        self.assertTrue(st["decl"].startswith("/-- Output determinism"))
        self.assertTrue(st["decl"].endswith("w₁ o = w₂ o :="))      # the proof term may replace `by sorry`
        self.assertEqual(st["body"], " by\n  sorry")
        self.assertTrue(st["post"].strip().startswith("end ZkDet."))

    def test_helpers_above_the_theorem_and_a_proof_are_accepted(self) -> None:
        sub = submission(self.stmt, "  exact helper\n", helper="theorem helper : True := trivial\n\n")
        probs, aux, body = C.text_check(self.stmt, sub, "det")
        self.assertEqual(probs, [])
        self.assertIn("theorem helper", aux)
        self.assertEqual(body.strip(), "by\n  exact helper")

    def test_changed_statement_imports_or_tail_are_invalid(self) -> None:
        changed = submission(self.stmt, "  trivial\n").replace("∀ o ∈ Outputs", "∀ o ∈ Inputs")
        self.assertTrue(C.text_check(self.stmt, changed, "det")[0])
        self.assertTrue(C.text_check(self.stmt, "import Mathlib\n" + submission(self.stmt, "  x\n"), "det")[0])
        self.assertTrue(C.text_check(self.stmt, submission(self.stmt, "  x\n") + "theorem extra : True := trivial\n", "det")[0])

    def test_forbidden_constructs_ignore_comments_and_strings(self) -> None:
        hits = C.forbidden_scan([("proof", '  -- sorry\n  /- native_decide -/\n  exact "admit"\n'),
                                 ("helper declarations", "axiom bad : False\nset_option debug.skipKernelTC true\n")])
        self.assertEqual(sorted(h.split(" in ")[0] for h in hits), ["axiom", "debug.skipKernelTC"])
        self.assertEqual(C.forbidden_scan([("proof", "  native_decide")]), ["native_decide in proof (line 1 of that part)"])


class VerdictMappingTests(unittest.TestCase):
    def verdict(self, r: dict) -> tuple[list, list, list]:
        invalid, fail, error = [], [], []
        C.apply_compile_result(r, {"reference_type_sha256": H}, invalid, fail, error)
        return invalid, fail, error

    def ok_post(self, **kw) -> dict:
        post = {"replay": "ok", "target": {"kind": "theorem", "levels": "[]"}, "type_sha256": H,
                "axioms": ["propext"], "consts": [{"name": "x.det", "kind": "theorem", "unsafe": False}]}
        post.update(kw)
        return post

    def ok_compile(self, **kw) -> dict:
        r = {"compile_rc": 0, "compile_timeout": False, "n_errors": 0, "errors": [], "sorry_warnings": 0,
             "printed_axioms": ["propext"], "post": self.ok_post()}
        r.update(kw)
        return r

    def test_pass(self) -> None:
        self.assertEqual(self.verdict(self.ok_compile()), ([], [], []))

    def test_failures_and_invalids(self) -> None:
        self.assertTrue(self.verdict(self.ok_compile(compile_rc=1, n_errors=2, errors=["1:1: x"], post=None))[1])
        self.assertTrue(self.verdict(self.ok_compile(sorry_warnings=1))[1])
        self.assertTrue(self.verdict(self.ok_compile(printed_axioms=["propext", "Lean.ofReduceBool"]))[0])
        self.assertTrue(self.verdict(self.ok_compile(post=self.ok_post(type_sha256="0" * 64)))[0])
        self.assertTrue(self.verdict(self.ok_compile(post=self.ok_post(replay="FAIL: deep recursion")))[0])
        self.assertTrue(self.verdict(self.ok_compile(post=self.ok_post(target={"kind": "def", "levels": "[]"})))[0])
        consts = [{"name": "x.ax", "kind": "axiom", "unsafe": False}]
        self.assertTrue(self.verdict(self.ok_compile(post=self.ok_post(consts=consts)))[0])
        self.assertTrue(self.verdict(self.ok_compile(post=None))[2])     # compiled but no post-check

    def test_replay_record_parser(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "replay.out")
            Path(out).write_text("CONST\tA.det\ttheorem\tfalse\tfalse\nREPLAY\tok\nTARGET\ttheorem\t[]\n"
                                 "AXIOM\tpropext\nTYPE\tforall x, x\n", encoding="utf-8")
            post = G.parse_replay(out)
        self.assertEqual(post["replay"], "ok")
        self.assertEqual(post["axioms"], ["propext"])
        self.assertEqual(post["type_sha256"], G.sha256_text("forall x, x"))

    def test_battery_classification(self) -> None:
        text = "\n".join(["set_option maxHeartbeats 1 in", "theorem triv_V0_simp : True := by", "  simp",
                          "#print axioms triv_V0_simp", "set_option maxHeartbeats 1 in",
                          "theorem triv_V0_omega : True := by", "  omega", "#print axioms triv_V0_omega",
                          "theorem triv_V0_grind : True := by", "  grind", "#print axioms triv_V0_grind"])
        msgs = [{"severity": "information", "pos": {"line": 4}, "data": "'triv_V0_simp' does not depend on any axioms"},
                {"severity": "error", "pos": {"line": 7}, "data": "omega could not prove the goal"},
                {"severity": "information", "pos": {"line": 8}, "data": "'triv_V0_omega' depends on axioms: [sorryAx]"}]
        cls = G.classify_battery(text, msgs, timed_out=True)
        self.assertEqual({k: v["status"] for k, v in cls.items()},
                         {"triv_V0_simp": "closed", "triv_V0_omega": "failed", "triv_V0_grind": "timeout"})


class CheckEarlyPathTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp()
        self.pkg, self.rec = make_package(self.tmp)
        self.stmt = Path(self.pkg, "Statement.lean").read_text(encoding="utf-8")
        self.sub_dir = os.path.join(self.tmp, "solver")
        os.makedirs(self.sub_dir)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def run_check(self, text: str, env=None, workdir=None) -> dict:
        path = os.path.join(self.sub_dir, "Statement.lean")
        Path(path).write_text(text, encoding="utf-8")
        return C.check(self.pkg, path, env or fake_env(self.tmp), workdir=workdir)

    def test_unchanged_statement_is_invalid_without_compiling(self) -> None:
        rep = self.run_check(self.stmt)
        self.assertEqual(rep["verdict"], "INVALID")
        self.assertEqual(rep["compile"], "skipped")
        self.assertIn("sorry in proof (line 2 of that part)", rep["invalid"])

    def test_environment_pin_mismatch_is_an_error(self) -> None:
        rep = self.run_check(submission(self.stmt, "  trivial\n"), env=fake_env(self.tmp, mathlib="d" * 40))
        self.assertEqual(rep["verdict"], "ERROR")
        self.assertTrue(any("mathlib" in e for e in rep["error"]))

    def test_modified_import_copy_in_the_workdir_is_invalid(self) -> None:
        model_rel = self.rec["statement"]["model_file"]
        copy = os.path.join(self.sub_dir, model_rel)
        os.makedirs(os.path.dirname(copy))
        shutil.copyfile(os.path.join(self.pkg, model_rel), copy)
        with open(copy, "a", encoding="utf-8") as f:
            f.write("-- tampered\n")
        rep = self.run_check(submission(self.stmt, "  native_decide\n"), workdir=self.sub_dir)
        self.assertEqual(rep["verdict"], "INVALID")
        self.assertTrue(any("was modified" in x for x in rep["invalid"]))
        self.assertTrue(any(x.startswith("native_decide") for x in rep["invalid"]))

    def test_tampered_package_is_an_error(self) -> None:
        with open(os.path.join(self.pkg, "Statement.lean"), "a", encoding="utf-8") as f:
            f.write("\n")
        rep = self.run_check(self.stmt)
        self.assertEqual(rep["verdict"], "ERROR")


if __name__ == "__main__":
    unittest.main()
