#!/usr/bin/env python3
"""ZoKrates DET generator: offline fixture tests (population filter, source scanning, generic
grounding, wrapper rendering, the native and legacy R1CS model builders, the shared .wtns reader)."""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import r1cs as R                        # noqa: E402
from zk_registry import zokrates_det as D                # noqa: E402
from zk_registry import zokrates_instantiation as I       # noqa: E402
from zk_registry import zokrates_legacy as ZL             # noqa: E402
from zk_registry import zokrates_r1cs as ZR               # noqa: E402
from zk_registry import zokrates_source as S              # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "zokrates"


class Population(unittest.TestCase):
    def test_filter(self):
        base = {"framework": "zokrates", "unit": "CC-circuit-component", "flags": []}
        self.assertTrue(D.selected(base))
        self.assertFalse(D.selected(dict(base, framework="circom")))
        self.assertFalse(D.selected(dict(base, unit="CC-constraint-site")))
        self.assertFalse(D.selected(dict(base, unit="C2-operation")))
        self.assertFalse(D.selected(dict(base, flags=["test"])))
        self.assertFalse(D.selected(dict(base, flags=["duplicate-of(x)"])))
        self.assertTrue(D.selected(dict(base, flags=["other"])))


class Wtns(unittest.TestCase):
    def test_round_trip(self):
        r = R.R1cs(prime=R.BN254_SCALAR, field_bytes=32, n_wires=4, n_pub_out=1, n_pub_in=0, n_prv_in=2,
                   n_labels=4, constraints=[([(1, 1)], [(2, 1)], [(3, 1)])])
        w = [1, 6, 2, 3]
        self.assertTrue(R.satisfies(r, [1, 6, 2, 12]))
        self.assertFalse(R.satisfies(r, w))

    def test_parse_wtns_fixture(self):
        prime, w = R.read_wtns(str(FIX / "native.wtns"))
        self.assertEqual(prime, R.BN254_SCALAR)
        self.assertEqual(w, [1, 9, 2, 3, 4])

    def test_bad_magic(self):
        with self.assertRaises(R.R1csFormatError):
            R.parse_wtns(b"xxxx" + b"\0" * 20)


class SourceScanner(unittest.TestCase):
    def test_find_def_brace(self):
        text = 'def rotr32<N>(u32 x) -> u32 {\n    return (x >> N) | (x << (32 - N));\n}\n'
        d = S.find_def(text, "rotr32")
        self.assertEqual(d.generics, ["N"])
        self.assertEqual([p.type for p in d.params], ["u32"])
        self.assertEqual(d.ret, "u32")
        self.assertEqual(d.style, "brace")

    def test_find_def_colon(self):
        text = "def main(private field a) -> (field):\n  return a\n"
        d = S.find_def(text, "main")
        self.assertEqual(d.generics, [])
        self.assertTrue(d.params[0].private)
        self.assertEqual(d.ret, "(field)")
        self.assertEqual(d.style, "colon")

    def test_find_def_overload_by_line(self):
        text = "def cast(bool[8] input) -> u8 {\n    return 0;\n}\n\ndef cast<N, P>(bool[N] input) -> u8[P] {\n    return [0];\n}\n"
        d4 = S.find_def(text, "cast", line=1)
        d10 = S.find_def(text, "cast", line=5)
        self.assertEqual(d4.generics, [])
        self.assertEqual(d10.generics, ["N", "P"])
        self.assertIsNone(S.find_def(text, "cast"))         # ambiguous without a line

    def test_find_calls_and_turbofish(self):
        text = "def main() -> u32 { return rotr32::<16>(5); }"
        calls = S.find_calls(text, "rotr32")
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0].turbofish, ["16"])
        self.assertEqual(calls[0].args, ["5"])

    def test_find_calls_excludes_own_header(self):
        text = "def rotr32<N>(u32 x) -> u32 { return x; }"
        self.assertEqual(S.find_calls(text, "rotr32"), [])

    def test_array_literal_len(self):
        self.assertEqual(S.array_literal_len("[1, 2, 3]"), 3)
        self.assertEqual(S.array_literal_len("[0; 8]"), 8)
        self.assertIsNone(S.array_literal_len("foo(1)"))

    def test_generics_from_call_array_literal(self):
        text = "def f<N>(u32[N] a) -> u32 { return a[0]; } def g() -> u32 { return f([1,2,3,4]); }"
        decl = S.find_def(text, "f")
        call = S.find_calls(text, "f")[0]
        self.assertEqual(S.generics_from_call(decl, call), {"N": 4})

    def test_assert_bounds(self):
        bounds = S.assert_bounds("assert(N > 0 && N <= 6);", ["N"])
        self.assertEqual(bounds["N"], (1, 6))

    def test_imports_both_eras(self):
        text = 'from "ecc/babyjubjubParams" import BabyJubJubParams;\nimport "./verifyEddsa" as verifyEddsa;\n'
        imps = S.find_imports(text)
        self.assertEqual(imps[0].module, "ecc/babyjubjubParams")
        self.assertEqual(imps[0].names, [("BabyJubJubParams", "BabyJubJubParams")])
        self.assertEqual(imps[1].names, [("main", "verifyEddsa")])


class Instantiation(unittest.TestCase):
    def test_parameter_free(self):
        decl = S.find_def("def main(field a) -> field { return a; }", "main")
        tiers = I.candidates_for(decl, "")
        self.assertEqual(tiers, [("parameter-free", [I.Candidate("parameter-free", {}, decl.name + " has no generic parameters")])])

    def test_repo_call_turbofish(self):
        text = ("def rotr32<N>(u32 x) -> u32 { return x; }\n"
                "def main(u32 x) -> u32 { return rotr32::<12>(x); }")
        decl = S.find_def(text, "rotr32")
        tiers = dict(I.candidates_for(decl, text))
        self.assertEqual(tiers["repo-call"][0].values, {"N": 12})

    def test_linear_relation_probe(self):
        text = ("// Cast a boolean array of size N to an array of 8-bit unsigned integers (u8) of size P\n"
                "def cast<N, P>(bool[N] input) -> u8[P] {\n    assert(N == 8 * P);\n    return [0; P];\n}\n")
        decl = S.find_def(text, "cast")
        tiers = dict(I.candidates_for(decl, text))
        self.assertIn({"P": 1, "N": 8}, [c.values for c in tiers["probed"]])

    def test_wrapper_source_brace(self):
        text = "def rotr32<N>(u32 x) -> u32 { return (x >> N) | (x << (32 - N)); }"
        decl = S.find_def(text, "rotr32")
        cand = I.Candidate("repo-call", {"N": 12}, "test")
        src = I.wrapper_source(decl, cand, "./shaRound", [], "brace")
        self.assertIn('from "./shaRound" import rotr32 as target;', src)
        self.assertIn("def main(u32 a0) -> u32 {", src)
        self.assertIn("return target::<12>(a0);", src)

    def test_wrapper_source_colon(self):
        text = "def main(private field a) -> (field):\n  return a\n"
        decl = S.find_def(text, "main")
        cand = I.Candidate("parameter-free", {}, "test")
        src = I.wrapper_source(decl, cand, "./noopAgreement", [], "colon")
        self.assertIn('import "./noopAgreement" as target;', src)
        self.assertIn("def main(field a0) -> (field):", src)
        self.assertIn("    return target(a0)", src)

    def test_wrapper_imports_struct_type(self):
        text = ('from "./pt" import Pt;\n'
                'def f(Pt p) -> field { return p.x; }\n')
        decl = S.find_def(text, "f")
        cand = I.Candidate("parameter-free", {}, "test")
        imports = S.find_imports(text)
        src = I.wrapper_source(decl, cand, "./m", imports, "brace")
        self.assertIn('from "./pt" import Pt;', src)


class NativeModel(unittest.TestCase):
    def test_build_native_struct(self):
        abi = json.loads((FIX / "native_abi.json").read_text())
        model = ZR.build_native(str(FIX / "native.r1cs"), abi)
        self.assertEqual(model.r.n_constraints, 1)
        self.assertEqual(len(model.inputs), 3)      # struct Pt (x, y) + field k
        self.assertEqual(len(model.outputs), 1)
        prime, w = R.read_wtns(str(FIX / "native.wtns"))
        self.assertTrue(R.satisfies(model.r, w))

    def test_leaf_specs_array_and_struct(self):
        spec = {"type": "array", "components": {"size": 2, "type": "field"}}
        self.assertEqual(ZR.leaf_specs(spec), [("[0]", "field"), ("[1]", "field")])
        spec = {"type": "struct", "components": {"members": [{"name": "x", "type": "field"},
                                                              {"name": "y", "type": "bool"}]}}
        self.assertEqual(ZR.leaf_specs(spec), [("_x", "field"), ("_y", "bool")])


class LegacyModel(unittest.TestCase):
    def test_build_model_and_witness(self):
        ztf = (FIX / "legacy.ztf").read_text()
        abi = json.loads((FIX / "legacy_abi.json").read_text())
        model, renumber = ZL.build_model(ztf, abi)
        self.assertEqual(model.r.n_constraints, 1)
        self.assertEqual(model.inputs, [2])
        self.assertEqual(model.outputs, [1])
        values = ZL.parse_witness_text((FIX / "legacy_witness.txt").read_text())
        w = ZL.witness_vector(values, renumber, model.r.n_wires, model.r.prime)
        self.assertEqual(w, [1, 7, 7])
        self.assertTrue(R.satisfies(model.r, w))

    def test_parse_ztf_header_and_constraint(self):
        header, outputs, cons = ZL.parse_ztf("def main(_0, _1) -> (1):\n\t(1 * _0) * (1 * _1) == 1 * ~out_0\n\t return ~out_0\n")
        self.assertEqual(header, ["_0", "_1"])
        self.assertEqual(outputs, ["~out_0"])
        self.assertEqual(cons, [([("_0", 1)], [("_1", 1)], [("~out_0", 1)])])


class ContentKey(unittest.TestCase):
    def test_deterministic_and_version_sensitive(self):
        a = D.content_key("def f(field a) -> field { return a; }", "f", "0.8.8")
        b = D.content_key("def f(field a) -> field { return a; }", "f", "0.8.8")
        c = D.content_key("def f(field a) -> field { return a; }", "f", "0.6.1")
        self.assertEqual(a, b)
        self.assertNotEqual(a, c)

    def test_comment_same_length_is_insensitive(self):
        a = D.content_key("def f(field a) -> field { return a; } // hi", "f", "0.8.8")
        b = D.content_key("def f(field a) -> field { return a; } // by", "f", "0.8.8")
        self.assertEqual(a, b)


if __name__ == "__main__":
    unittest.main()
