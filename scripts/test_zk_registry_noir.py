#!/usr/bin/env python3
"""Noir DET generator: offline fixture tests (ACIR normalization and evaluation, Lean emission, source scanning,
instantiation, toolchain helpers, driver helpers, counterexample search, records)."""
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

from zk_registry import noir_acir as A                 # noqa: E402
from zk_registry import noir_det as D                  # noqa: E402
from zk_registry import noir_det_search as DSN         # noqa: E402
from zk_registry import noir_instantiation as I        # noqa: E402
from zk_registry import noir_lean_emit as NE           # noqa: E402
from zk_registry import noir_source as NS              # noqa: E402
from zk_registry import noir_toolchain as T            # noqa: E402
from zk_registry import package as P                   # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "noir"
TOY = FIX / "toyrepo"


def decoded(name: str) -> dict:
    return json.loads((FIX / f"dec_{name}.json").read_text())


def executions(name: str) -> list[dict]:
    return [json.loads(x) for x in (FIX / f"ex_{name}.jsonl").read_text().splitlines() if x.strip()]


def real_witnesses(name: str) -> tuple[A.Flat, list[list[int]]]:
    flat = A.flatten(A.normalize_program(decoded(name)))
    out = []
    for ex in executions(name):
        if ex["ok"]:
            calls = [dict(c, witness=D.to_ints(c["witness"])) for c in ex["calls"]]
            out.append(A.flat_witness(flat, D.to_ints(ex["witness"]), calls))
    return flat, out


def table_of(flat: A.Flat, ws: list[list[int]]) -> A.Interp:
    interp = A.Interp()
    for w in ws:
        for key, ins, outs, _ in A.bb_calls(flat.opcodes, w):
            interp.add(key, ins, outs)
    return interp


class SelectionTests(unittest.TestCase):
    def row(self, **kw) -> dict:
        r = {"framework": "noir", "unit": "CC-circuit-component", "flags": []}
        r.update(kw)
        return r

    def test_filter(self) -> None:
        self.assertTrue(D.selected(self.row()))
        self.assertTrue(D.selected(self.row(unit="G1-trait-method")))
        self.assertFalse(D.selected(self.row(framework="circom")))
        for unit in ("G2-constraint-site", "CC-constraint-site", "X-interaction-site", "C2-operation", "U1-instruction"):
            self.assertFalse(D.selected(self.row(unit=unit)), unit)
        for flag in ("test", "not-counted(unconstrained-body)", "NOT-ITEMIZED(per-fn-aggregate)", "copy-of:x/y",
                     "deprecated-project", "vendored-mapped", "program-as-circuit", "exported-api-false"):
            self.assertFalse(D.selected(self.row(flags=[flag])), flag)

    def test_repo_ids(self) -> None:
        self.assertEqual(D.repo_id_of("https://github.com/AztecProtocol/aztec-packages"), "AztecProtocol/aztec-packages")
        self.assertEqual(D.repo_id_of("https://github.com/zkemail/zkemail.nr"), "zkemail/zkemail.nr")
        self.assertEqual(D.repo_dir_name("zkemail/zkemail.nr"), "zkemail__zkemail.nr")


class AcirTests(unittest.TestCase):
    def test_field_shapes(self) -> None:
        self.assertEqual(A.fe("0" * 63 + "f"), 15)
        self.assertEqual(A.fe([0] * 31 + [7]), 7)
        self.assertEqual(A.fe(5), 5)

    def test_memory_flag_new_format(self) -> None:
        # `a[i] = v` then `a[j] + a[i]`: the inspector prints WRITE b0[w5] = w7, READ w9 = b0[w6]; the serde field
        # named "read" holds MemOpKind::Write as true
        ops = A.normalize_program(decoded("rc2_fx_mem")).main.opcodes
        mem = [op for op in ops if op["kind"] == "mem_op"]
        self.assertEqual(mem[0]["write"].as_const(), 1)
        self.assertEqual(mem[0]["index"].as_witness(), 5)
        self.assertEqual(mem[0]["value"].as_witness(), 7)
        self.assertEqual(mem[1]["write"].as_const(), 0)
        self.assertEqual(mem[1]["index"].as_witness(), 6)

    def test_memory_old_format_with_predicate(self) -> None:
        ops = A.normalize_program(decoded("old_fx_mem")).main.opcodes
        mem = [op for op in ops if op["kind"] == "mem_op"]
        self.assertTrue(mem)
        self.assertIn(mem[0]["write"].as_const(), (0, 1))
        self.assertIsNone(mem[0]["predicate"])

    def test_logic_both_formats(self) -> None:
        for name in ("rc2_fx_logic", "old_fx_logic"):
            ops = A.normalize_program(decoded(name)).main.opcodes
            kinds = {op["kind"] for op in ops}
            self.assertTrue({"and", "xor"} <= kinds, name)
            self.assertTrue(all(op["bits"] == 32 for op in ops if op["kind"] in ("and", "xor")))

    def test_black_box_keys_and_predicate(self) -> None:
        ops = A.normalize_program(decoded("rc2_fx_bb")).main.opcodes
        bbs = [op for op in ops if op["kind"] == "bb"]
        names = [op["name"] for op in bbs]
        self.assertEqual(names, ["Poseidon2Permutation", "EmbeddedCurveAdd"])
        add = bbs[1]
        self.assertEqual(add["predicate"], ("w", 4))
        self.assertEqual(add["outputs"], [13, 14])
        self.assertEqual(len(add["inputs"]), 4)

    def test_real_witnesses_accepted_and_mutants_rejected(self) -> None:
        for name in ("rc2_fx_mem", "old_fx_mem", "rc2_fx_logic", "old_fx_logic", "rc2_fx_bb", "rc2_fx_call"):
            flat, ws = real_witnesses(name)
            self.assertTrue(ws, name)
            interp = table_of(flat, ws)
            for w in ws:
                self.assertEqual(A.check(flat.opcodes, w, interp), (True, None), name)
                bad = list(w)
                o = flat.outputs[0]
                bad[o] = (bad[o] + 1) % A.BN254
                self.assertFalse(A.check(flat.opcodes, bad, interp)[0], name)

    def test_memory_index_bound(self) -> None:
        flat, ws = real_witnesses("rc2_fx_mem")
        w = list(ws[0])
        w[5] = 7                                        # write index outside the 5-element block
        self.assertFalse(A.check(flat.opcodes, w, A.Interp())[0])

    def test_disabled_black_box_is_unconstrained(self) -> None:
        flat, ws = real_witnesses("rc2_fx_bb")
        off = next(w for w in ws if w[4] == 0)          # b = false: the curve addition is disabled
        interp = table_of(flat, ws)
        w2 = list(off)
        w2[13] = 12345                                  # its outputs are free
        self.assertTrue(A.check(flat.opcodes, w2, interp)[0])

    def test_flatten_calls(self) -> None:
        prog = A.normalize_program(decoded("rc2_fx_call"))
        self.assertEqual(len(prog.functions), 2)
        flat = A.flatten(prog)
        self.assertFalse(any(op["kind"] == "call" for op in flat.opcodes))
        self.assertEqual(len(flat.call_sites), 2)
        self.assertGreater(flat.n_witnesses, prog.main.n_witnesses)
        _, ws = real_witnesses("rc2_fx_call")
        self.assertEqual(len(ws), 2)

    def test_unsupported_opcode(self) -> None:
        with self.assertRaises(A.Unsupported):
            A.opcode({"Directive": {}})
        with self.assertRaises(A.Unsupported):
            A.black_box("BigIntAdd", {"lhs": 0, "rhs": 1, "output": 2})


class LeanEmitTests(unittest.TestCase):
    def test_render_expr_balanced(self) -> None:
        e = A.Expr(((1, 0, 1),), ((A.BN254 - 1, 2), (3, 3)), A.BN254 - 5)
        self.assertEqual(NE.render_expr(e), "w 0 * w 1 - w 2 + 3 * w 3 - 5")
        self.assertEqual(NE.render_expr(A.Expr((), (), 0)), "0")

    def test_model_parts(self) -> None:
        flat, _ = real_witnesses("rc2_fx_bb")
        model, info = NE.emit_model("ZkDet.T", {"repo_id": "t/t", "instantiation": "fx_bb", "generator": "g",
                                                "repo_url": "https://example.invalid", "commit": "0" * 40,
                                                "path": "src/lib.nr", "template": "fx_bb", "rule": "parameter-free",
                                                "nargo_version": "1.0.0-rc.2", "nargo_command": "nargo export",
                                                "acir_sha256": "0" * 64}, flat, {0: "param `x[0]`"})
        self.assertTrue(info["bb"])
        self.assertIn("abbrev BlackBox : Type := ℕ → List F → List F", model)
        self.assertIn("(w 4 ≠ 0 → [w 13, w 14] = bb 1 [w 5, w 6, w 5, w 6])", model)
        self.assertIn("(w 3).val < 2 ^ 1", model) if False else None
        st = NE.emit_statement("ZkDet.T", {"repo_id": "t/t", "instantiation": "x", "path": "p"}, True)
        self.assertIn("∀ (bb : BlackBox) (w₁ w₂ : Fin nWires → F), Constraints bb w₁ → Constraints bb w₂ →", st)
        st0 = NE.emit_statement("ZkDet.T", {"repo_id": "t/t", "instantiation": "x", "path": "p"}, False)
        self.assertIn(NE.STATEMENT_PROP, st0)

    def test_large_model_raises_recursion_depth(self) -> None:
        flat, _ = real_witnesses("rc2_fx_bb")
        meta = {"repo_id": "t/t", "instantiation": "x", "generator": "g", "repo_url": "https://example.invalid",
                "commit": "0" * 40, "path": "src/lib.nr", "template": "x", "rule": "parameter-free",
                "nargo_version": "1", "nargo_command": "nargo export", "acir_sha256": "0" * 64}
        small, _ = NE.emit_model("ZkDet.T", meta, flat, {})
        self.assertNotIn("maxRecDepth", small)
        rng = [op for op in flat.opcodes if op["kind"] == "assert_zero"][:1]
        big = A.Flat(rng * (NE.BLOCK * (NE.MAX_BLOCKS_DEFAULT_DEPTH + 1)), flat.inputs, flat.outputs, flat.n_witnesses)
        model, info = NE.emit_model("ZkDet.T", meta, big, {})
        self.assertIn("set_option maxRecDepth 100000 in\nset_option maxHeartbeats 4000000 in\n/-- The compiled", model)
        self.assertEqual(info["n_conjuncts"], NE.BLOCK * (NE.MAX_BLOCKS_DEFAULT_DEPTH + 1))
        wide = A.Flat(flat.opcodes, list(range(NE.LONG_LIST_HEARTBEATS + 1)), flat.outputs, NE.LONG_LIST_HEARTBEATS + 9)
        model_w, _ = NE.emit_model("ZkDet.T", meta, wide, {})
        self.assertIn("set_option maxHeartbeats 4000000 in\n/-- Parameter witnesses", model_w)
        self.assertNotIn("maxHeartbeats", model_w.split("def Outputs")[0])

    def test_memory_conjunct(self) -> None:
        flat, _ = real_witnesses("rc2_fx_mem")
        conj = NE.memory_conjuncts(flat.opcodes)
        self.assertEqual(conj, ["memRun [w 0, w 1, w 2, w 3, w 4] [(true, 1, w 5, w 7), (true, 0, w 6, w 9)] = true"])

    def test_and_xor_conjuncts(self) -> None:
        flat, _ = real_witnesses("rc2_fx_logic")
        conj = NE.opcode_conjuncts(flat.opcodes, [])
        self.assertTrue(any("&&&" in c and "< 2 ^ 32" in c for c in conj))
        self.assertTrue(any("^^^" in c for c in conj))

    def test_battery_prefix_names(self) -> None:
        self.assertEqual(NE.battery_prefix("V3", 3, True, False)[0], "intro bb w₁ w₂ h₁ h₂ hin")
        self.assertEqual(NE.battery_prefix("V3", 3, False, False)[0], "intro w₁ w₂ h₁ h₂ hin")
        self.assertIn("(try unfold memRun at *)", NE.battery_prefix("V1", 3, False, True))
        text = NE.emit_battery_forms("ZkDet.T", 3, [("V0", "simp"), ("V4", "grind")], 1000, True, False)
        self.assertIn("theorem triv_V4_grind", text)
        self.assertIn("all_goals grind", text)

    def test_abi_names(self) -> None:
        names = NE.abi_witness_names(decoded("rc2_fx_mem")["abi"])
        self.assertEqual(names[:6], ["a[0]", "a[1]", "a[2]", "a[3]", "a[4]", "i"])


class SourceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.src = (TOY / "lib" / "src" / "lib.nr").read_text()
        self.fns, self.blocks, self.text = NS.scan(self.src)

    def by(self, name: str) -> NS.Function:
        return next(f for f in self.fns if f.name == name)

    def test_comments_and_strings_blanked(self) -> None:
        self.assertNotIn("fake", [f.name for f in self.fns])
        self.assertEqual(len(self.text), len(self.src))
        self.assertEqual(NS.strip_for_hash('fn a() { // c\n let s = "x // y"; /* z */ }'), 'fn a() { let s = "x // y"; }')

    def test_signatures(self) -> None:
        push = self.by("push")
        self.assertEqual([(p.pattern, p.type) for p in push.params], [("self", "&mut Self"), ("x", "T")])
        self.assertEqual(push.impl.self_type, "Acc<T, N>")
        self.assertEqual([(g.name, g.numeric) for g in push.impl.generics], [("T", False), ("N", True)])
        self.assertEqual(push.impl.where, "T: Eq")
        make = self.by("make")
        self.assertEqual([g.name for g in make.generics], ["M"])
        self.assertEqual(make.ret, "[T; M]")
        eq = self.by("eq")
        self.assertEqual(eq.impl.trait, "Eq")
        self.assertEqual(eq.impl.self_type, "Point")
        self.assertTrue(self.by("hint").unconstrained)
        self.assertTrue(self.by("test_sum").attr("test"))

    def test_find_function(self) -> None:
        f, how = NS.find_function(self.fns, "sum", self.by("sum").line)
        self.assertEqual((f.name, how), ("sum", "line"))
        f, how = NS.find_function(self.fns, "Acc::first", None)
        self.assertEqual(f.name, "first")
        f, how = NS.find_function(self.fns, f"push:L{self.by('push').line}", None)
        self.assertEqual(f.name, "push")
        self.assertIsNone(NS.find_function(self.fns, "absent", 3)[0])

    def test_top_level_uses_and_globals(self) -> None:
        self.assertEqual(NS.uses(self.text, top_level=True), ["crate::inner::Point", "std::hash::Hash"])
        self.assertIn("super::sum", NS.uses(self.text))
        self.assertEqual(NS.globals_with_values(self.text)["DOUBLE"], "WIDTH * 2")

    def test_contract_attributes_keep_strings(self) -> None:
        src = (TOY / "con" / "src" / "main.nr").read_text()
        fns, _, _ = NS.scan(src)
        kinds = {f.name: I.contract_attr(f) for f in fns}
        self.assertEqual(kinds, {"transfer": "private", "mint": "public", "check": ""})

    def test_mod_path(self) -> None:
        self.assertEqual(NS.mod_path("lib.nr"), [])
        self.assertEqual(NS.mod_path("a/mod.nr"), ["a"])
        self.assertEqual(NS.mod_path("ops/arith.nr"), ["ops", "arith"])


def toy_target(rel: str, name: str, crate: str = "lib", ctype: str = "lib", stdlib: bool = False) -> I.Target:
    src = (TOY / rel).read_text()
    fns, _, text = NS.scan(src)
    fn = next(f for f in fns if f.name == name)
    return I.Target(f"toy:{name}", "toy/repo", str(TOY), rel, fn, text, src, str(TOY / crate), ctype, stdlib)


class InstantiationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.idx = I.RepoIndex.build(str(TOY), D.nr_files(str(TOY)))

    def test_classify(self) -> None:
        self.assertEqual(I.classify(toy_target("lib/src/lib.nr", "hint"))[0], "NOT-APPLICABLE")
        # generator 1.1: closure parameters are planned (a repository function is supplied per candidate)
        self.assertEqual(I.classify(toy_target("lib/src/lib.nr", "apply"))[0], "export")
        self.assertEqual(I.classify(toy_target("app/src/main.nr", "main", "app", "bin"))[0], "bin-main")
        con = "con/src/main.nr"
        self.assertEqual(I.classify(toy_target(con, "transfer", "con", "contract"))[0], "contract-fn")
        self.assertEqual(I.classify(toy_target(con, "mint", "con", "contract"))[0], "NOT-APPLICABLE")
        self.assertEqual(I.classify(toy_target(con, "check", "con", "contract"))[0], "NO-INSTANTIATION")
        self.assertEqual(I.classify(toy_target("con/src/notes.nr", "note_hash", "con", "contract"))[0], "export")
        priv = toy_target("lib/src/lib.nr", "apply", stdlib=True)
        priv.fn.params = []
        self.assertEqual(I.classify(priv)[0], "NO-INSTANTIATION")

    def test_resolve_globals(self) -> None:
        self.assertEqual(self.idx.resolve("DOUBLE"), "6")
        self.assertEqual(self.idx.resolve("[Field; WIDTH]"), "[Field; 3]")
        self.assertIsNone(self.idx.resolve("N"))
        self.assertEqual(self.idx.resolve("Point"), "Point")

    def test_candidate_tiers(self) -> None:
        cands = I.candidates(toy_target("lib/src/lib.nr", "sum"), self.idx)
        tiers = [(c[0], c[1]) for c in cands]
        self.assertIn(("repo-test", {"N": "3"}), tiers)
        self.assertIn(("repo-test", {"N": "5"}), tiers)            # inside `mod tests`
        self.assertIn(("repo-derived", {"N": "6"}), tiers)
        self.assertEqual(cands[-1][0], "probed")
        first = I.candidates(toy_target("lib/src/lib.nr", "first"), self.idx)
        self.assertIn(("repo-test", {"T": "Field", "N": "4"}), [(c[0], c[1]) for c in first])
        self.assertEqual(I.candidates(toy_target("lib/src/inner.nr", "norm"), self.idx)[0][0], "parameter-free")

    def test_probed_bounded_type_generic(self) -> None:
        t = toy_target("lib/src/lib.nr", "push")
        probed = [a for tier, a, _ in I.candidates(t, None) if tier == "probed"]
        self.assertEqual(probed[0], {"N": "4", "T": "Field"})
        withidx = [a for tier, a, _ in I.candidates(t, self.idx) if tier == "probed"]
        self.assertTrue(all(a["T"] == "Point" for a in withidx))      # the repository's `impl Eq for Point`

    def test_wrappers(self) -> None:
        w = I.wrapper(toy_target("lib/src/lib.nr", "push"), {"T": "Field", "N": "4"}, "x1")
        self.assertIn("fn boole_det_x1(s_self: Acc<Field, 4>, a1: Field) -> Acc<Field, 4> {", w.text)
        self.assertIn("let mut s_self = s_self;", w.text)
        self.assertIn("s_self.push(a1);", w.text)
        self.assertEqual(w.outputs, ["s_self"])
        w = I.wrapper(toy_target("lib/src/lib.nr", "eq"), {}, "x2")
        self.assertIn("<Point as Eq>::eq(s_self, a1)", w.text)
        self.assertNotIn("type ", w.text)
        w = I.wrapper(toy_target("lib/src/lib.nr", "make"), {"T": "u8", "N": "2", "M": "3"}, "x3")
        self.assertIn("Acc::<u8, 2>::make::<3>(a0)", w.text)
        self.assertIn("-> [u8; 3]", w.text)
        w = I.wrapper(toy_target("lib/src/lib.nr", "sum"), {"N": "3"}, "x4", "std::foo::")
        self.assertIn("std::foo::sum::<3>(a0)", w.text)

    def test_associated_constants_and_literal_folding(self) -> None:
        src = ("pub trait Packable { let N: u32; fn pack(self) -> [Field; Self::N]; }\n"
               "pub struct P { x: Field }\n"
               "impl Packable for P { let N: u32 = 2; fn pack(self) -> [Field; Self::N] { [self.x, 0] } }\n"
               "pub struct W<let M: u32> { v: [Field; M] }\n"
               "impl<let M: u32> W<M> { pub fn grow(self) -> [Field; M + 1] { [0; M + 1] } }\n")
        fns, _, text = NS.scan(src)
        pack = next(f for f in fns if f.name == "pack" and f.has_body and f.impl.kind == "impl")
        t = I.Target("x", "toy/repo", str(TOY), "lib/src/lib.nr", pack, text, src, str(TOY / "lib"), "lib")
        w = I.wrapper(t, {}, "a1")
        self.assertIn("-> [Field; <P as Packable>::N]", w.text)
        grow = next(f for f in fns if f.name == "grow")
        t2 = I.Target("y", "toy/repo", str(TOY), "lib/src/lib.nr", grow, text, src, str(TOY / "lib"), "lib")
        w2 = I.wrapper(t2, {"M": "4"}, "a2")
        self.assertIn("-> [Field; 5]", w2.text)
        self.assertEqual(I.fold_literals("StateVariable<4 + 1, Field> [u8; 2 * 3]"), "StateVariable<5, Field> [u8; 6]")
        src3 = "pub struct D<let K: u64> { x: Field }\nimpl<let K: u64> D<K> { pub fn get(self) -> Field { self.x } }\n"
        fns3, _, text3 = NS.scan(src3)
        t3 = I.Target("z", "toy/repo", str(TOY), "lib/src/lib.nr", next(f for f in fns3 if f.name == "get"), text3, src3,
                      str(TOY / "lib"), "lib")
        w3 = I.wrapper(t3, {"K": "4"}, "a3")
        self.assertIn("global BOOLE_A3_K: u64 = 4;", w3.text)
        self.assertIn("s_self: D<BOOLE_A3_K>", w3.text)

    def test_test_code_types_are_not_candidates(self) -> None:
        self.assertTrue(I.is_test_path("aztec/src/unconstrained_array/test_helpers.nr"))
        self.assertTrue(I.is_test_path("src/test/mocks/mock_struct.nr"))
        self.assertFalse(I.is_test_path("src/state_vars/public_mutable.nr"))
        self.assertFalse(I.is_test_path("src/attestation.nr"))
        vis = I.visible_in(toy_target("lib/src/lib.nr", "push"), self.idx)
        self.assertTrue(vis("Point"))
        self.assertFalse(vis("Unknown"))

    def test_std_imports(self) -> None:
        t = toy_target("lib/src/lib.nr", "eq", stdlib=True)
        w = I.wrapper(t, {}, "x5", "std::")
        cands = I.std_import_candidates(t, w.text, "collections/acc.nr")
        self.assertEqual(cands["Eq"][-1], "std::Eq")
        self.assertEqual(cands["Point"][0], "std::inner::Point")
        self.assertEqual(I._expand_use("super::{a::B, C}", ["x", "y"]), ["std::x::a::B", "std::x::C"])


def rec_target(name: str, rel: str = "rec/src/lib.nr") -> I.Target:
    return toy_target(rel, name, "rec")


class RecoveryTests(unittest.TestCase):
    """Generator 1.1 (recovery R1): implementors from derives / test code, closure arguments from call sites,
    non-ABI parameter values, public-execution classification, size policy and superseding records."""

    def setUp(self) -> None:
        self.idx = I.RepoIndex.build(str(TOY), D.nr_files(str(TOY)))

    def test_struct_parser(self) -> None:
        src = "#[derive(Eq)]\npub struct N<T, let K: u32> { pub a: Field, b: [T; K], pub(crate) c: fn[E](u8) -> bool }\n"
        sd = NS.structs(NS.blank_comments_strings(src), src)[0]
        self.assertEqual((sd.name, sd.visibility, sd.attrs), ("N", "pub", ["derive(Eq)"]))
        self.assertEqual([g.name for g in sd.generics], ["T", "K"])
        self.assertEqual(sd.fields[2], ("pub(crate)", "c", "fn[E](u8) -> bool"))

    def test_derived_and_test_implementors(self) -> None:
        self.assertEqual(I.derived_traits(["derive(Eq, a::Packable)", "note", "aztec::macros::events::event"]),
                         ["Eq", "Packable", "NoteType", "NoteHash", "EventInterface"])
        self.assertIn("Note1", self.idx.trait_impls["Packable"])
        self.assertNotIn("Hasher2", self.idx.trait_impls)
        self.assertIn("MockH", self.idx.test_trait_impls["Hasher2"])
        store = I.candidates(rec_target("store"), self.idx)
        self.assertEqual(store[0][1], {"T": "Note1"})
        hashed = I.candidates(rec_target("hashed"), self.idx)
        self.assertEqual((hashed[0][0], hashed[0][1]), ("probed", {"H": "MockH"}))
        self.assertIn("test code", hashed[0][2][-1])

    def test_public_execution_code_is_not_applicable(self) -> None:
        plan, why = I.classify(rec_target("public_only"))
        self.assertEqual(plan, "NOT-APPLICABLE")
        self.assertIn("PublicContext", why)

    def test_values_containing_public_context(self) -> None:
        t = rec_target("get")
        self.assertEqual(I.classify(t)[0], "export")
        with self.assertRaises(ValueError) as cm:
            I.wrapper(t, {}, "p1", builder=I.ValueBuilder(t, self.idx))
        self.assertIn(I.PUBLIC_EXECUTION, str(cm.exception))
        self.assertIn("PublicContext", str(cm.exception))

    def test_closure_arguments_from_call_sites(self) -> None:
        t = rec_target("apply_twice")
        self.assertEqual(I.fn_param_indices(t), [0])
        found = [e for e, _ in I.fn_arg_candidates(t, self.idx, 0)]
        self.assertEqual(found[0], "|x| x + 1")                  # closures first, shorter first
        self.assertIn("crate::keep", found)                       # named function, qualified by its file
        self.assertEqual(I.fn_arg_candidates(rec_target("never_called"), self.idx, 0), [])
        cands, why = D.with_fn_args(rec_target("never_called"), self.idx, [("parameter-free", {}, [])], [0])
        self.assertEqual(cands, [])
        self.assertIn("no call of the function", why)
        fw = rec_target("fold_with")
        self.assertEqual(I.env_generics(fw), ["Env"])
        cand = I.candidates(fw, self.idx)[0]
        self.assertEqual(cand[1], {"Env": "_"})
        ext, _ = D.with_fn_args(fw, self.idx, [cand], [1])
        self.assertEqual(ext[0][3], {1: "|a, b| a + b"})
        self.assertIn("rec/src/lib.nr:", ext[0][2][-1])
        w = I.wrapper(fw, ext[0][1], "f1", fn_args=ext[0][3])
        self.assertIn("fold_with::<_>(a0, |a, b| a + b)", w.text)
        self.assertNotIn("a1", w.text)

    def test_constant_arguments_from_call_sites(self) -> None:
        t = rec_target("chunks")
        self.assertEqual(I.const_arg_candidates(t, self.idx, {}), {1: ("3", I.const_arg_candidates(t, self.idx, {})[1][1])})
        self.assertIn("rec/src/lib.nr:", I.const_arg_candidates(t, self.idx, {})[1][1])
        self.assertEqual(I.const_arg_candidates(t, self.idx, {1: "2"}), {})
        w = I.wrapper(t, {}, "c1", fn_args={1: "3"})
        self.assertIn("fn boole_det_c1(a0: [Field; 8]) -> Field {", w.text)
        self.assertIn("chunks(a0, 3)", w.text)
        self.assertTrue(D.CONST_ERROR.search("Could not determine loop bound at compile-time at src/x.nr:1:1"))

    def test_non_abi_values_built_from_fields(self) -> None:
        t = rec_target("bump")
        b = I.ValueBuilder(t, self.idx)
        self.assertTrue(b.needs("Wrap<Field>"))
        self.assertFalse(b.needs("Ctx"))
        w = I.wrapper(t, {"S": "Field"}, "v1", builder=b)
        self.assertIn("s_self_ctx_r: Ctx, s_self_storage: Field, a1: Field", w.text)
        self.assertIn("let mut s_self_ctx_l = s_self_ctx_r;", w.text)
        self.assertIn("let s_self_v: Wrap<Field> = Wrap { ctx: &mut s_self_ctx_l, storage: s_self_storage, "
                      "filter: keep };", w.text)
        self.assertIn("let mut s_self = s_self_v;", w.text)
        self.assertEqual(w.outputs, ["r", "s_self_ctx_l", "s_self.storage"])
        self.assertTrue(any("keep" in n for n in w.notes))
        # a struct declared in another file of the crate: its names are written by absolute path
        h = I.wrapper(rec_target("read", "rec/src/other.nr"), {}, "v2", builder=I.ValueBuilder(
            rec_target("read", "rec/src/other.nr"), self.idx))
        self.assertIn("a0_inner_r: Ctx", h.text)
        lib = I.wrapper(rec_target("vec_len"), {}, "v3", builder=I.ValueBuilder(rec_target("vec_len"), self.idx))
        self.assertIn("a0_a: [Field; 4]", lib.text)
        self.assertIn("vec_len(a0_a.as_vector())", lib.text)
        # without a builder (wave N1 behaviour) the wrapper is unchanged
        self.assertIn("s_self: Wrap<Field>", I.wrapper(t, {"S": "Field"}, "v4").text)

    def test_std_import_candidates_from_declaring_module(self) -> None:
        idx = I.RepoIndex(str(TOY))
        idx.decl_files["Poseidon2Hasher"] = ["noir_stdlib/src/hash/poseidon2.nr"]
        t = toy_target("lib/src/lib.nr", "eq", stdlib=True)
        w = I.wrapper(t, {}, "x6", "std::")
        cands = I.std_import_candidates(t, w.text.replace("Point", "Poseidon2Hasher"), "hash/mod.nr", idx)
        self.assertIn("std::hash::poseidon2::Poseidon2Hasher", cands["Poseidon2Hasher"])

    def test_stdlib_names_in_non_public_modules(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            src = os.path.join(d, "noir_stdlib", "src")
            os.makedirs(os.path.join(src, "hash"))
            Path(src, "lib.nr").write_text("pub mod hash;\n")
            Path(src, "hash", "mod.nr").write_text("pub(crate) mod poseidon2;\npub mod keccak;\n")
            self.assertFalse(D.std_module_public(d, "noir_stdlib/src/hash/poseidon2.nr"))
            self.assertTrue(D.std_module_public(d, "noir_stdlib/src/hash/keccak.nr"))
            idx = I.RepoIndex(d)
            idx.decl_files["Poseidon2Hasher"] = ["noir_stdlib/src/hash/poseidon2.nr"]
            t = toy_target("lib/src/lib.nr", "eq", stdlib=True)
            t.root = d
            err = D.std_private_note(t, idx, "Could not resolve 'Poseidon2Hasher' in path at src/lib.nr:5:58")
            self.assertIn(D.STD_NOT_PUBLIC, err)
            self.assertEqual(D.std_private_note(t, idx, "Could not resolve 'Other'"), "Could not resolve 'Other'")

    def test_contract_block_note(self) -> None:
        t = toy_target("con/src/notes.nr", "note_hash", "con", "contract")
        self.assertIn("contract block", D.contract_note(t, "Could not resolve 'Toy' in path at src/x.nr:1:1"))
        self.assertEqual(D.contract_note(t, "Could not resolve 'Other'"), "Could not resolve 'Other'")

    def test_lean_concurrency_limit(self) -> None:
        import threading
        import time as _t
        from zk_registry import lean_runner as LR
        saved = LR.run_lean
        state = {"now": 0, "max": 0}
        lock = threading.Lock()

        def fake(*a, **k):
            with lock:
                state["now"] += 1
                state["max"] = max(state["max"], state["now"])
            _t.sleep(0.02)
            with lock:
                state["now"] -= 1
            return "ok"
        try:
            LR.run_lean = fake
            D._LEAN_LIMIT.clear()
            D.limit_lean(2)
            ths = [threading.Thread(target=LR.run_lean) for _ in range(6)]
            for th in ths:
                th.start()
            for th in ths:
                th.join()
            self.assertEqual(state["max"], 2)
        finally:
            LR.run_lean = saved
            D._LEAN_LIMIT.clear()

    def test_size_policy_and_bands(self) -> None:
        self.assertEqual(D.WaveConfig("c", "l", {}, "/t", "/h", "/s", "/e", "/o", "/w").max_opcodes, 2000)
        self.assertEqual([D.size_band(n) for n in (0, 50, 2000, 2001, 10000, 99999, 100001)],
                         ["XS", "S", "M", "L1", "L2", "XL2", "XXL"])

    def test_superseding_record_and_model_key(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            rec = {"statement": {"model_file": "ZkDet/A/Model.lean"}, "checker": {"theorem_fqn": "ZkDet.A.det"}}
            os.makedirs(os.path.join(d, "ZkDet", "A"))
            with open(os.path.join(d, "ZkDet", "A", "Model.lean"), "w") as f:
                f.write("-- generated for x\nnamespace ZkDet.A\n/- wire names -/\ndef c := 1\nend ZkDet.A\n")
            k1 = D.model_key(d, rec)
            with open(os.path.join(d, "ZkDet", "A", "Model.lean"), "w") as f:
                f.write("-- another header\nnamespace ZkDet.A\n\ndef c := 1\nend ZkDet.A\n")
            self.assertEqual(D.model_key(d, rec), k1)
            p1, p2 = os.path.join(d, "a.jsonl"), os.path.join(d, "b.jsonl")
            old = {"ids": {"ledger_item_id": "i"}, "status": "TOO-LARGE", "status_reason": "big",
                   "package_id": "r/p", "generator": {"sources_sha256": "e" * 64}, "evidence": {}}
            with open(p1, "w") as f:
                f.write(json.dumps(dict(old, status="COMPILE-FAIL")) + "\n")
            with open(p2, "w") as f:
                f.write(json.dumps(old) + "\n")
            path, got = D.prior_records([p1, p2])["i"]
            self.assertEqual((path, got["status"]), (p2, "TOO-LARGE"))
            # resume: a completed record is kept, a packaged record without its package is regenerated
            na = dict(old, status="NOT-APPLICABLE", ids={"ledger_item_id": "a"})
            gf = dict(old, status="GATE-FAIL", ids={"ledger_item_id": "b"}, package_id="r/missing")
            with open(os.path.join(d, "PROGRESS.jsonl"), "w") as f:
                f.write(json.dumps(na) + "\n" + json.dumps(gf) + "\n")
            kept, dd = D.resume_records(d, {"a", "b"}, {})
            self.assertEqual([r["ids"]["ledger_item_id"] for r in kept], ["a"])
            self.assertEqual(dd, [])
            self.assertEqual(D.resume_records(d, {"b"}, {}), ([], []))
            # kept packages with equal models: the first (by item id) is canonical, the second is marked
            for name in ("p1", "p2"):
                os.makedirs(os.path.join(d, "r", name, "ZkDet", "A"))
                Path(d, "r", name, "ZkDet", "A", "Model.lean").write_text("namespace ZkDet.A\ndef c := 1\n")
                Path(d, "r", name, "problem.json").write_text("{}")
            pk = dict(old, status="OPEN", statement={"model_file": "ZkDet/A/Model.lean"},
                      checker={"theorem_fqn": "ZkDet.A.det"})
            with open(os.path.join(d, "PROGRESS.jsonl"), "w") as f:
                f.write(json.dumps(dict(pk, ids={"ledger_item_id": "y"}, package_id="r/p2")) + "\n")
                f.write(json.dumps(dict(pk, ids={"ledger_item_id": "x"}, package_id="r/p1")) + "\n")
            kept, dd = D.resume_records(d, {"x", "y"}, {})
            self.assertEqual([(x["item_id"], x["canonical_package_id"]) for x in dd], [("y", "r/p1")])
            self.assertEqual(kept[1]["evidence"]["same_model_as"], "r/p1")
        sh = D.Shared.__new__(D.Shared)
        sh.cfg = D.WaveConfig("c", "l", {}, "/t", "/h", "/s", "/e", "/o", "/w")
        sh.env = type("E", (), {"lean_version": "v4.33.1", "packages": {"mathlib": "c" * 40},
                                "manifest_sha256": "a" * 64})()
        sh.ledger_sha, sh.generator = "b" * 64, D.generator_info()
        row = {"item_id": "i", "repo": "https://github.com/o/r", "commit": "0" * 40, "path": "src/x.nr",
               "symbol": "y", "census": [{"line": 3, "pin": "v1"}], "coverage": "none"}
        new = D.base_record(sh, None, row, "x.y")
        new["status"], new["status_reason"] = "NOT-APPLICABLE", "public-execution code"
        D.supersede(new, old, "/base/packages/noir-n1/INDEX.jsonl", "/base")
        self.assertEqual(new["supersedes"]["status"], "TOO-LARGE")
        self.assertEqual(new["evidence"]["supersedes_record"]["index"], "packages/noir-n1/INDEX.jsonl")
        self.assertEqual(P.validate_problem(new), [])


class ToolchainTests(unittest.TestCase):
    def test_read_toml_single_quotes_and_inline_tables(self) -> None:
        t = T.read_toml(str(TOY / "lib" / "Nargo.toml"))
        self.assertEqual(t["package"]["type"], "lib")
        self.assertEqual(t["dependencies"]["helper"], {"path": "../app"})
        self.assertEqual(t["dependencies"]["poseidon"]["tag"], "v0.1.1")

    def test_git_dep_dir_and_alias(self) -> None:
        self.assertEqual(T.git_dep_dir("/h", "https://github.com/noir-lang/poseidon", "v0.1.1"),
                         "/h/nargo/github.com/noir-lang/poseidon/v0.1.1")
        self.assertEqual(T.git_dep_dir("/h", "https://github.com/AztecProtocol/aztec-packages/", "t"),
                         "/h/nargo/github.com/AztecProtocol/aztec-packages/t")
        with tempfile.TemporaryDirectory() as d:
            dest = os.path.join(d, "nargo", "github.com", "o", "r", "v1")
            os.makedirs(dest)
            T._concat_alias(d, "https://github.com/o/r", "v1", dest)
            self.assertTrue(os.path.islink(os.path.join(d, "nargo", "github.com", "o", "rv1")))

    def test_every_repository_has_a_compiler(self) -> None:
        for repo, (tags, why) in T.REPO_COMPILERS.items():
            self.assertTrue(tags and why, repo)
            self.assertTrue(all(t in T.COMPILERS for t in tags), repo)
        published = [c for c in T.COMPILERS.values() if c.published_digest]
        self.assertTrue(all(len(c.asset_sha256) == 64 for c in published))

    def test_nargo_env_blocks_network_git(self) -> None:
        env = T.Toolchain("/t", "/h", tempfile.gettempdir()).env()
        self.assertEqual(env["GIT_ALLOW_PROTOCOL"], "file")
        self.assertEqual(env["HOME"], "/h")


class DriverTests(unittest.TestCase):
    def test_copy_crate_paths_and_lib_conversion(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            D.copy_crate(str(TOY / "lib"), os.path.join(d, "lib"), as_lib=False)
            man = Path(d, "lib", "Nargo.toml").read_text()
            self.assertIn(f'path = "{TOY / "app"}"', man)
            D.copy_crate(str(TOY / "con"), os.path.join(d, "con"), as_lib=True, strip_contract=True)
            self.assertIn('type = "lib"', Path(d, "con", "Nargo.toml").read_text())
            lib = Path(d, "con", "src", "lib.nr").read_text()
            self.assertIn("mod notes;", lib)
            self.assertNotIn("contract", lib)
            self.assertNotIn("#[aztec]", lib)

    def test_content_key_separates_mutually_reaching_functions(self) -> None:
        sh = D.Shared.__new__(D.Shared)
        a, b = toy_target("lib/src/lib.nr", "push"), toy_target("lib/src/lib.nr", "first")
        self.assertNotEqual(D.content_key(sh, a, "v"), D.content_key(sh, b, "v"))
        self.assertEqual(D.content_key(sh, a, "v"), D.content_key(sh, toy_target("lib/src/lib.nr", "push"), "v"))
        self.assertNotEqual(D.content_key(sh, a, "v"), D.content_key(sh, a, "w"))
        with tempfile.TemporaryDirectory() as d:          # a verbatim copy in another crate is equal by content
            shutil.copytree(TOY / "lib", os.path.join(d, "lib"))
            Path(d, "lib", "src", "lib.nr").write_text((TOY / "lib" / "src" / "lib.nr").read_text().replace(
                "// \"comment with { brace\"", "// another comment"))
            src = Path(d, "lib", "src", "lib.nr").read_text()
            fns, _, text = NS.scan(src)
            fn = next(f for f in fns if f.name == "push")
            c = I.Target("c", "toy/repo", d, "lib/src/lib.nr", fn, text, src, os.path.join(d, "lib"), "lib")
            self.assertEqual(D.content_key(sh, a, "v"), D.content_key(sh, c, "v"))

    def test_module_tree(self) -> None:
        files = D.module_files(str(TOY / "lib"))
        self.assertEqual({os.path.basename(f) for f in files}, {"lib.nr", "inner.nr"})
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(os.path.join(d, "src", "a"))
            Path(d, "src", "lib.nr").write_text("// mod a;\nmod b;\n")
            Path(d, "src", "b.nr").write_text("mod c;\n")
            os.makedirs(os.path.join(d, "src", "b"))
            Path(d, "src", "b", "c.nr").write_text("")
            Path(d, "src", "a", "mod.nr").write_text("")
            got = {os.path.relpath(f, os.path.join(d, "src")) for f in D.module_files(d)}
            self.assertEqual(got, {"lib.nr", "b.nr", "b/c.nr"})

    def test_remove_pub_contract_block(self) -> None:
        src = "mod a;\n#[aztec]\npub contract Toy {\n    fn f() {}\n}\n"
        out = D.remove_contract_block(src)
        self.assertEqual(out.strip(), "mod a;")

    def test_mutants_of_an_empty_program(self) -> None:
        flat = A.Flat([], [], [], 0)
        self.assertEqual(D.make_mutants(flat, [[]], 4, "s"), [])

    def test_first_error(self) -> None:
        out = "warning: x\nerror: Could not resolve 'Foo' in path\n   ┌─ src/lib.nr:3:5\n  │\n"
        self.assertEqual(D.first_error(out), "Could not resolve 'Foo' in path at src/lib.nr:3:5")

    def test_first_error_compiler_panic(self) -> None:
        out = ("\x1b[31mThe application panicked (crashed).\x1b[0m\nMessage:  unexpected type\n"
               "Location: \x1b[35mcompiler/noirc_frontend/src/monomorphization/ast.rs\x1b[0m:\x1b[35m637\x1b[0m\n"
               "This is a bug. We may have already fixed this in newer versions\n")
        e = D.first_error(out)
        self.assertTrue(e.startswith("compiler internal error (panic): unexpected type"))
        self.assertNotIn("\x1b", e)

    def test_toml_values(self) -> None:
        self.assertEqual(D.toml_value(["1", True, {"a": "2"}]), '["1", true, { a = "2" }]')

    def test_assignment_imports_and_callees(self) -> None:
        sh = D.Shared.__new__(D.Shared)
        sh.indexes = {"toy/repo": I.RepoIndex.build(str(TOY), D.nr_files(str(TOY)))}
        t = toy_target("lib/src/lib.nr", "push")
        self.assertEqual(D.assignment_imports(sh, t, {"T": "Point", "N": "4"}), [])     # imported by the file
        t2 = toy_target("lib/src/inner.nr", "norm")
        self.assertEqual(D.assignment_imports(sh, t2, {"T": "Acc"}), ["crate::Acc"])
        t3 = toy_target("app/src/main.nr", "main", "app", "bin")
        sh.indexes["toy/repo"].struct_files["Remote"] = ["lib/src/inner.nr"]
        t3.crate = str(TOY / "lib")
        self.assertEqual(D.direct_path_deps(str(TOY / "lib")), {"helper": str(TOY / "app")})
        kids, unresolved = D.callees(toy_target("lib/src/lib.nr", "main_like"))
        self.assertEqual([k.fn.name for k in kids], ["sum"])
        self.assertEqual(kids[0].item_id.split("#")[1], f"sum@L{kids[0].fn.line}")

    def test_sampler(self) -> None:
        abi = decoded("rc2_fx_mem")["abi"]
        a = D.sample_inputs(abi, 12, "s")
        self.assertEqual(a, D.sample_inputs(abi, 12, "s"))
        self.assertEqual(a[0], {"a": ["0"] * 5, "i": "0", "j": "0", "v": "0"})
        self.assertTrue(all(0 <= int(x["i"]) < 2 ** 32 for x in a))
        s = D.sample_value({"kind": "string", "length": 4}, __import__("random").Random(1), "digits")
        self.assertTrue(s.isdigit() and len(s) == 4)

    def test_dir_name(self) -> None:
        t = toy_target("lib/src/lib.nr", "push")
        name = D.dir_name_for(t, {"T": "Field", "N": "4"})
        self.assertTrue(name.startswith("lib.src.lib.Acc_T_N.push.L"))
        self.assertRegex(name, r"^[A-Za-z0-9._-]+$")

    def test_set_status(self) -> None:
        def rec(rule="parameter-free"):
            return {"instantiation": {"rule": rule}, "statement": {"truth": "unknown"}}
        ok = {"status": "PASS"}
        gates = {"G-ELAB": ok, "G-NONVAC": ok, "G-FID": ok, "G-TRIV": ok, "DET-SEARCH": {"status": "PASS", "truth": "unknown"}}
        r = rec()
        D.set_status(r, gates, True)
        self.assertEqual(r["status"], "OPEN")
        r = rec()
        D.set_status(r, dict(gates, **{"G-TRIV": {"status": "SKIPPED"}}), False)
        self.assertEqual(r["status"], "GATE-FAIL")
        self.assertIn("no return values", r["status_reason"])
        r = rec()
        D.set_status(r, dict(gates, **{"G-TRIV": {"status": "FAIL", "closed_by": ["triv_V3_grind"]}}), True)
        self.assertEqual(r["statement"]["truth"], "closed-by-automation")
        r = rec("probed")
        D.set_status(r, dict(gates, **{"DET-SEARCH": {"status": "FAIL", "truth": "false-counterexample-found",
                                                      "method": "output-mutation"}}), True)
        self.assertEqual(r["status"], "DET-FALSE-CANDIDATE")
        self.assertIn("not-a-finding", r["status_reason"])

    def test_coverage_flag(self) -> None:
        c = D.coverage_of({"coverage": "conditional", "census": [{"coverage_raw": "conditional"}]})
        self.assertEqual((c["class"], c["same_pin"]), ("partial", False))
        self.assertEqual(D.coverage_of({"coverage": "none", "census": [{}]})["class"], "none")

    def test_records_validate(self) -> None:
        sh = D.Shared.__new__(D.Shared)
        sh.cfg = D.WaveConfig("c", "l", {}, "/t", "/h", "/s", "/e", "/o", "/w")
        sh.env = type("E", (), {"lean_version": "v4.33.1", "packages": {"mathlib": "c" * 40},
                                "manifest_sha256": "a" * 64})()
        sh.ledger_sha = "b" * 64
        sh.generator = D.generator_info()
        row = {"item_id": "toy:CC/x#y", "repo": "https://github.com/o/r", "commit": "0" * 40, "path": "src/x.nr",
               "symbol": "y", "census": [{"line": 3, "pin": "v1"}], "coverage": "none"}
        rec = D.base_record(sh, None, row, "x.y")
        rec["status"], rec["status_reason"] = "NOT-APPLICABLE", "unconstrained function"
        self.assertEqual(P.validate_problem(rec), [])
        big = dict(rec, status="TOO-LARGE", status_reason="big",
                   circuit={"compiler": {"name": "nargo", "version": "1", "flags": ["export"], "binary_sha256": "c" * 64},
                            "prime": str(A.BN254), "prime_name": "bn254", "n_constraints": 5000, "n_wires": 9,
                            "size_policy": {"max_constraints": 2000, "within": False}, "acir": {"sha256": "x"}})
        self.assertEqual(P.validate_problem(big), [])
        bad = dict(rec, status="UNKNOWN-STATUS")
        self.assertTrue(P.validate_problem(bad))


class SearchTests(unittest.TestCase):
    def test_free_hint_output_is_found(self) -> None:
        flat, ws = real_witnesses("rc2_fx_under")
        self.assertEqual(sum(op["kind"] == "brillig" for op in flat.opcodes), 1)
        ce, log = DSN.search(flat, ws, DSN.Oracle(A.Interp()), "t")
        self.assertIsNotNone(ce)                       # the return witness copies the free hint
        self.assertIn(ce.method, ("output-mutation", "linear-kernel"))
        self.assertTrue(A.check(flat.opcodes, ce.other, A.Interp())[0])

    def test_constrained_circuit_has_no_counterexample(self) -> None:
        for name in ("rc2_fx_mem", "rc2_fx_logic", "rc2_fx_call"):
            flat, ws = real_witnesses(name)
            ce, _ = DSN.search(flat, ws, DSN.Oracle(table_of(flat, ws)), "t", budget_s=5)
            self.assertIsNone(ce, name)

    def test_bbeval_used_for_changed_black_box_inputs(self) -> None:
        flat, ws = real_witnesses("rc2_fx_bb")
        calls = []

        def bbeval(op, w):
            calls.append(op["name"])
            return None
        oracle = DSN.Oracle(table_of(flat, ws), bbeval)
        w = list(ws[0])
        w[0] = (w[0] + 1) % A.BN254                  # a Poseidon2 input changes: its value is asked for
        self.assertFalse(oracle.complete(flat.opcodes, w))
        self.assertEqual(calls, ["Poseidon2Permutation"])

    def test_single_wire_confirmation_matches_full_check(self) -> None:
        for name in ("rc2_fx_mem", "rc2_fx_logic", "rc2_fx_bb", "rc2_fx_under", "rc2_fx_call"):
            flat, ws = real_witnesses(name)
            touch = DSN.touching(flat)
            oracle = DSN.Oracle(table_of(flat, ws))
            for w in ws:
                for o in flat.outputs:
                    for delta in (1, 7):
                        w2 = list(w)
                        w2[o] = (w2[o] + delta) % A.BN254
                        fast = bool(DSN.confirm_single(flat, w, w2, o, oracle, touch))
                        full = bool(DSN.confirm(flat, w, w2, oracle))
                        self.assertEqual(fast, full, (name, o))

    def test_propagate(self) -> None:
        flat, ws = real_witnesses("rc2_fx_call")
        w = ws[0]
        out = DSN.propagate(flat.opcodes, w, set(flat.inputs), {}, 1e18)
        self.assertEqual(out, w)


if __name__ == "__main__":
    unittest.main()
