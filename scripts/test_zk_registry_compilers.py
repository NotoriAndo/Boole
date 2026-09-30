#!/usr/bin/env python3
"""circom compiler-version selection by `pragma circom`, circom 1 support and digest pins (offline)."""
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
from zk_registry import circom_source as cs       # noqa: E402
from zk_registry import instantiation as I        # noqa: E402

ALL = ["v0.5.46", "v2.0.9", "v2.1.9", "v2.2.3"]


def write(root: str, rel: str, text: str) -> None:
    p = Path(root, rel)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


class PragmaTests(unittest.TestCase):
    def test_closure_pragmas_follow_includes_and_ignore_comments(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "lib.circom", "pragma circom 2.0.3;\ntemplate L() {}\n")
            write(tmp, "mid.circom", '// pragma circom 2.2.0;\npragma circom 2.1.4;\ninclude "lib.circom";\n'
                                     "template M() {}\n")
            write(tmp, "old.circom", "template O() { signal private input a; }\n")
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            self.assertEqual(sorted(D.closure_pragmas(files, "mid.circom")), [(2, 0, 3), (2, 1, 4)])
            self.assertEqual(D.closure_pragmas(files, "old.circom"), [])

    def test_compiler_order_honours_the_newest_pragma_line(self) -> None:
        self.assertEqual(D.compiler_order([(2, 0, 3)], ALL), ["v2.0.9", "v2.1.9", "v2.2.3"])
        self.assertEqual(D.compiler_order([(2, 0, 0), (2, 1, 4)], ALL), ["v2.1.9", "v2.2.3"])
        self.assertEqual(D.compiler_order([(2, 2, 0)], ALL), ["v2.2.3"])
        self.assertEqual(D.compiler_order([(2, 3, 0)], ALL), ["v2.2.3"])          # newer than every pin
        # no pragma: circom 2's documented default (latest), then circom 1 for pre-2.0 sources
        self.assertEqual(D.compiler_order([], ALL), ["v2.2.3", "v0.5.46"])
        # only the configured compilers are used (a wave configured with circom 2.2.3 alone behaves as before)
        self.assertEqual(D.compiler_order([(2, 0, 3)], ["v2.2.3"]), ["v2.2.3"])
        self.assertEqual(D.compiler_order([], ["v2.2.3"]), ["v2.2.3"])

    def test_every_pinned_compiler_has_a_line_and_a_digest(self) -> None:
        for tag, rel in D.CIRCOM_RELEASES.items():
            self.assertIn("line", rel, tag)
            self.assertTrue(rel["sha256"], tag)
            self.assertTrue(rel["digest_source"], tag)
        self.assertEqual(sorted(D.CIRCOM_RELEASES), ["v0.5.46", "v2.0.9", "v2.1.9", "v2.2.3"])


class Circom1Tests(unittest.TestCase):
    def test_circom1_main_has_no_pragma(self) -> None:
        self.assertEqual(I.main_source("src/a.circom", "T", ("3",), pragma=None),
                         'include "src/a.circom";\ncomponent main = T(3);\n')

    def test_circom1_flags(self) -> None:
        self.assertEqual(D.compile_flags("circom1", full=False), ["-f", "-r", "main.r1cs"])
        self.assertEqual(D.compile_flags("circom1", full=True),
                         ["-f", "-r", "main.r1cs", "-s", "main.sym", "-w", "main.wasm"])
        self.assertEqual(D.compile_flags("circom2", full=False), ["--r1cs", "--O0"])
        self.assertEqual(D.compile_flags("circom2", full=True), D.CIRCOM_FLAGS)

    def test_private_inputs_are_inputs(self) -> None:
        decls = cs.signal_declarations("signal private input a[2]; signal input b; signal output c;")
        self.assertEqual([(d.direction, d.name, d.dims) for d in decls],
                         [("input", "a", ["2"]), ("input", "b", []), ("output", "c", [])])


class AttemptTests(unittest.TestCase):
    def test_every_attempt_keeps_its_own_error(self) -> None:
        from zk_registry import lean_runner as L
        H = "ab" * 32
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "a.circom", "pragma circom 2.0.0;\ntemplate A(n) { signal input x; }\n")
            cfg = D.WaveConfig(repo_dir=tmp, repo_id="t/r", repo_url="https://x", release="r", commit="0" * 40,
                               collection="c", scope_prefix="", exclude=[], circom="c", circom_version_tag="v2.2.3",
                               lean_env="e", node="n", work=os.path.join(tmp, "w"), out=os.path.join(tmp, "o"))
            env = L.LeanEnv("/tc", [], "v4.33.1", {}, H, "/s")
            comps = {t: D.Compiler(t, "circom2", t, t[1:], H) for t in ("v2.0.9", "v2.1.9", "v2.2.3")}
            sh = D.Shared(cfg, env, cs.scan_repo(tmp, cs.list_circom_files(tmp)), set(), None, H, "2.2.3", "v22",
                          {"name": "g", "version": "1", "sources_sha256": H}, comps)
            errors = {"v2.0.9": "error[T3001]: False assert reached | previous errors were found",
                      "v2.1.9": "error[T3001]: False assert reached", "v2.2.3": "error[T2046]: Bus or signal not defined"}

            def fake(sh_, workdir, include_rel, template, args, full, rule_path=None, compiler=None, tag_template=None):
                return {"rc": 1, "include_context": include_rel, "main_sha256": H, "flags": [],
                        "compiler": compiler.tag, "error": errors[compiler.tag]}
            saved = D.compile_main
            D.compile_main = fake
            try:
                t = sh.files["a.circom"].templates[0]
                _, recs = D.size_candidates(sh, I.TemplatePlan(t, {"repo-main": [I.Candidate("repo-main", ("2",), [])]}),
                                            os.path.join(tmp, "w"))
            finally:
                D.compile_main = saved
            attempts = recs[0]["compile_result"]["attempts"]
            self.assertEqual([(a["compiler"], a["result"]) for a in attempts], [
                ("v2.0.9", "error: error[T3001]: False assert reached"),
                ("v2.1.9", "error: error[T3001]: False assert reached"),
                ("v2.2.3", "error: error[T2046]: Bus or signal not defined")])
            row = D.candidate_summary(recs)[0]
            self.assertTrue(row["error"].startswith("v2.0.9: error[T3001]"))       # the pragma-line compiler first


class DigestTests(unittest.TestCase):
    def test_compiler_digests_are_checked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp, "circom")
            fake.write_bytes(b"not circom")
            with self.assertRaisesRegex(ValueError, "pinned"):
                D.verify_compiler("v2.1.9", str(fake))
            os.makedirs(Path(tmp, "c1", "node_modules", "circom"))
            Path(tmp, "c1", "package-lock.json").write_text("{}", encoding="utf-8")
            Path(tmp, "c1", "node_modules", "circom", "package.json").write_text(json.dumps({"version": "0.5.46"}),
                                                                               encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "pinned"):
                D.verify_compiler("v0.5.46", str(Path(tmp, "c1")))
            with self.assertRaisesRegex(ValueError, "unknown"):
                D.verify_compiler("v9.9.9", str(fake))


if __name__ == "__main__":
    unittest.main()
