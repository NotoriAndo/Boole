#!/usr/bin/env python3
"""Decomposition by instances (waves RT-N2 / RT-G2): offline tests of the monomorphised-program reader, instance keys,
generic-argument recovery and struct naming (Noir), root generation and type-argument resolution (gnark), the forced
type arguments of the gnark planner, the decomposition meaning of ratchet problems and the gnark size policies."""
from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import gnark_callees as GC           # noqa: E402
from zk_registry import gnark_instantiation as I      # noqa: E402
from zk_registry import noir_callees as NC            # noqa: E402
from zk_registry import noir_source as NS             # noqa: E402
from zk_registry import package as P                  # noqa: E402
from zk_registry import ratchet as RT                 # noqa: E402

from test_zk_registry_gnark import mini_catalog, named  # noqa: E402

# the shape of `nargo export --show-monomorphized` (v1.0.0-beta.25) for a wrapper calling `big`
MONO = """global TWO$g0: Field = 2;
fn boole_w$f0(x$l0: [Field; 3], p$l1: ([u8; 2], Field)) -> Field {
    big$f1(x$l0, p$l1)
}
fn big$f1(x$l2: [Field; 3], p$l3: ([u8; 2], Field)) -> Field {
    ((sum$f2(x$l2) + sum$f3([x$l2[0], x$l2[1]])) + (first$f4(p$l3) as Field)) + hint$f5(x$l2[0])
}
fn sum$f2(x$l7: [Field; 3]) -> Field {
    let mut s$l8 = 0;
    for i$l9 in 0 .. 3 {
        s$l8 = (s$l8 + (x$l7[i$l9] * x$l7[i$l9]))
    };
    (s$l8 * TWO$g0)
}
fn sum$f3(x$l10: [Field; 2]) -> Field {
    let mut s$l11 = 0;
    for i$l12 in 0 .. 2 {
        s$l11 = (s$l11 + (x$l10[i$l12] * x$l10[i$l12]))
    };
    (s$l11 * TWO$g0)
}
#[inline_always]
fn first$f4(self$l13: ([u8; 2], Field)) -> u8 {
    self$l13.0[0]
}
unconstrained fn hint$f5(v$l14: Field) -> Field {
    inner$f6(v$l14)
}
unconstrained fn inner$f6(v$l15: Field) -> Field {
    v$l15
}
"""

# the same `sum` instance printed with other ids by a wrapper of its own
MONO_SUM = """global TWO$g4: Field = 2;
fn boole_det_x$f0(x$l0: [Field; 3]) -> Field {
    sum$f7(x$l0)
}
fn sum$f7(x$l1: [Field; 3]) -> Field {
    let mut s$l2 = 0;
    for i$l3 in 0 .. 3 {
        s$l2 = (s$l2 + (x$l1[i$l3] * x$l1[i$l3]))
    };
    (s$l2 * TWO$g4)
}
"""

SRC = """
pub struct Pair<T, let N: u32> { a: [T; N], b: Field }
pub struct Note { value: Field, owner: u8 }
pub struct Tag { x: Field, y: u8 }
pub fn sum<let N: u32>(x: [Field; N]) -> Field { x[0] }
impl<T, let N: u32> Pair<T, N> { pub fn first(self) -> T { self.a[0] } }
pub fn wrap<T>(x: T) -> (T, u32) { (x, 0) }
pub fn scale<let K: u32>(x: Field) -> Field { x * K as Field }
"""


def fn_of(name: str) -> NS.Function:
    fns, _, _ = NS.scan(SRC)
    return next(f for f in fns if f.name == name)


def generics_of(fn: NS.Function) -> list:
    return ([g for g in fn.impl.generics] if fn.impl is not None else []) + list(fn.generics)


class StructTable:
    """Struct expansion over SRC (the role of noir_callees.Structs over a repository index)."""

    def __init__(self):
        self.defs = {sd.name: sd for sd in NS.structs(NS.blank_comments_strings(SRC), SRC)}

    def __call__(self, name):
        sd = self.defs.get(name)
        return ([g.name for g in sd.generics], [ft for _, _, ft in sd.fields]) if sd else NC.STD_STRUCTS.get(name)


class MonomorphisedProgram(unittest.TestCase):
    def setUp(self):
        self.prog = NC.parse_programs(MONO)[0]

    def test_parse(self):
        p = self.prog
        self.assertEqual(p.instances[p.root].name, "boole_w")
        self.assertEqual(p.instances[1].params, [("x", "[Field; 3]"), ("p", "([u8; 2], Field)")])
        self.assertEqual(p.instances[4].attrs, ["#[inline_always]"])
        self.assertTrue(p.instances[5].unconstrained)
        self.assertEqual(p.instances[1].refs, [2, 3, 4, 5])
        self.assertEqual(p.instances[2].grefs, [0])
        self.assertIn(0, p.globals)

    def test_reachable_stops_at_unconstrained_calls(self):
        reach = NC.reachable(self.prog)
        self.assertEqual(sorted(reach), [0, 1, 2, 3, 4])          # hint (Brillig) and its callee are not circuit code
        self.assertEqual(reach[2], 2)
        self.assertEqual(NC.wrapper_instance(self.prog, "big"), 1)

    def test_instance_key_is_id_free(self):
        other = NC.parse_programs(MONO_SUM)[0]
        self.assertEqual(NC.instance_key(self.prog, 2), NC.instance_key(other, NC.wrapper_instance(other, "sum")))
        self.assertNotEqual(NC.instance_key(self.prog, 2), NC.instance_key(self.prog, 3))   # N = 3 vs N = 2
        self.assertIn("inner$F", NC.canonical(self.prog, 5))     # an unconstrained callee is part of the identity

    def test_two_programs(self):
        progs = NC.parse_programs(MONO + MONO_SUM)
        self.assertEqual([p.instances[p.root].name for p in progs], ["boole_w", "boole_det_x"])

    def test_trivially_free_and_literals(self):
        self.assertFalse(NC.trivially_free(NC.canonical(self.prog, 2)))
        self.assertTrue(NC.trivially_free("fn get$F0(self$L0: &mut (Field,)) -> Field {\n    self$L0.0\n}"))
        self.assertFalse(NC.trivially_free("fn get$F0(a$L0: [Field; 4], i$L1: u32) -> Field {\n    a$L0[i$L1]\n}"))
        self.assertEqual(NC.literals(NC.canonical(self.prog, 3)), ["2", "0"])      # loop bounds / lengths first
        self.assertEqual(NC.literals("for i in 0 .. 8192 {\n x[3] + 3 + 3\n}"), ["8192", "3", "0"])


class GenericArguments(unittest.TestCase):
    def setUp(self):
        self.prog = NC.parse_programs(MONO)[0]
        self.st = StructTable()

    def assign(self, name, inst, named=lambda g, t: [], guesses=None):
        fn = fn_of(name)
        return NC.assignments(fn, inst, generics_of(fn), self.st, lambda n: None, named, (), guesses)

    def test_numeric_from_array_length(self):
        self.assertEqual(self.assign("sum", self.prog.instances[2]), ([{"N": "3"}], ""))
        self.assertEqual(self.assign("sum", self.prog.instances[3]), ([{"N": "2"}], ""))

    def test_impl_generics_through_struct_expansion(self):
        self.assertEqual(self.assign("first", self.prog.instances[4]), ([{"T": "u8", "N": "2"}], ""))

    def test_mismatch_and_undetermined(self):
        inst = NC.Instance(9, "sum", [("x", "Field")], "Field", False, [], "")
        got, why = self.assign("sum", inst)
        self.assertEqual(got, [])
        self.assertIn("do not match", why)
        inst = NC.Instance(9, "scale", [("x", "Field")], "Field", False, [], "fn scale$F0() {\n  (x * 8)\n}")
        self.assertIn("not determined", self.assign("scale", inst)[1])
        got, _ = self.assign("scale", inst, guesses=["8", "1"])
        self.assertEqual(got, [{"K": "8"}, {"K": "1"}])            # guesses, each confirmed by a compile

    def test_struct_named_before_tuple(self):
        inst = NC.Instance(9, "wrap", [("x", "(Field, u8)")], "((Field, u8), u32)", False, [], "")
        got, _ = self.assign("wrap", inst, named=lambda g, t: ["Note"] if NC.norm(t) == "(Field,u8)" else [])
        self.assertEqual(got, [{"T": "Note"}, {"T": "(Field, u8)"}])

    def test_name_tuple_over_index(self):
        class Idx:
            struct_defs = {sd.name: [("src/lib.nr", sd, False)] for sd in StructTable().defs.values()}

            def struct_def(self, name, scope=None):
                d = self.struct_defs.get(name)
                return (d[0][0], d[0][1]) if d else None
        st = NC.Structs(Idx(), ["src"], lambda n: True)
        self.assertEqual(st.name_tuple(NC.parse_type("(Field, u8)")), ["Note", "Tag", "(Field, u8)"])
        # ranked by the field names the instance binds (`let y$l4 = ...` of a struct literal)
        self.assertEqual(st.name_tuple(NC.parse_type("(Field, u8)"), 0, "let y$l4 = 1;")[0], "Tag")
        self.assertEqual(st.name_tuple(NC.parse_type("([u8; 2], Field)"))[0], "Pair<u8, 2>")

    def test_provenance_fn_args(self):
        fn = NS.scan("fn f(g: fn(Field) -> Field, n: u32) -> Field { g(n as Field) }")[0][0]
        prov = ["function argument `g` = `double` from a.nr:3",
                "parameter `n` fixed to the compile-time constant `16` from a.nr:9"]
        self.assertEqual(NC.provenance_fn_args(fn, prov), {0: "double", 1: "16"})
        self.assertEqual(NC.provenance_fn_args(fn, prov, consts=False), {0: "double"})


class GnarkRoots(unittest.TestCase):
    def test_exported_and_split(self):
        self.assertTrue(GC.exported("github.com/consensys/gnark/std/math/emulated.Field.Mul"))
        self.assertFalse(GC.exported("github.com/consensys/gnark/std/math/emulated.Field.mulMod"))
        self.assertFalse(GC.exported("github.com/consensys/gnark/std/math/emulated.staticFieldParams.NbLimbs"))
        self.assertEqual(GC.split_key("example.org/m/g.Field.Mul"), ("example.org/m/g", "Field.Mul"))

    def test_roots_file(self):
        cat, decls = mini_catalog()
        env = {"T": named("example.org/m/g", "ParamsB")}
        src = GC.roots_file(decls["Field.Mul"], env)
        self.assertIn('g "example.org/m/g"', src)
        self.assertIn("var BooleRoot0 = (*g.Field[g.ParamsB]).Mul", src)
        self.assertIn("var BooleRoot0 = (*g.Ext).Mul", GC.roots_file(decls["Ext.Mul"], {}))
        self.assertIn("var BooleRoot0 = g.Sum\n", GC.roots_file(decls["Sum"], {}))

    def test_env_of_record(self):
        cat, decls = mini_catalog()
        cat.data["named"]["example.org/m/g.ParamsB"] = {"pkg": "example.org/m/g", "name": "ParamsB"}
        tgt = {"found": True, "decl": decls["Field.Mul"]}
        self.assertEqual(GC.env_of_record(cat, tgt, ["T=g.ParamsB", "slice length 2"]),
                         {"T": named("example.org/m/g", "ParamsB")})
        self.assertIsNone(GC.env_of_record(cat, tgt, ["T=g.Unknown"]))
        self.assertEqual(GC.label_of({"T": named("example.org/m/g", "ParamsB")}), ["T=g.ParamsB"])


class ForcedTypeArguments(unittest.TestCase):
    def test_caller_instance_replaces_repository_instantiations(self):
        cat, decls = mini_catalog()
        tgt = {"found": True, "decl": decls["Field.Mul"]}
        plan = I.plan_target(cat, tgt, {"T": named("example.org/m/g", "ParamsC")})
        self.assertEqual([(c.tier, I.show(c.env["T"])) for c in plan.choices], [("decomposition", "g.ParamsC")])
        self.assertTrue(plan.choices[0].provenance[0].startswith(RT.CALLER_INSTANCE))
        w = I.render(cat, plan, plan.choices[0], "w000000000009_0")
        self.assertIn("p_g.NewField[p_g.ParamsC](api)", w.source)
        with self.assertRaises(I.NoInstantiation):                  # the caller's arguments must name every parameter
            I.plan_target(cat, tgt, {})
        plan = I.plan_target(cat, {"found": True, "decl": decls["Sum"]}, {})
        self.assertEqual([c.tier for c in plan.choices], ["probed", "probed"])     # probed lengths keep the label


class RatchetMeaning(unittest.TestCase):
    def test_decomposition_reference(self):
        prob = {"instantiation": {"rule": "decomposition", "provenance": [RT.CALLER_INSTANCE + "instance x"],
                                  "decomposition": {"parents": [{"package_id": "o__r/a.f.L1", "call": "g"}],
                                                    "n_parents": 1, "depth": 2, "instance_key": "k",
                                                    "content_sha256": "0" * 64, "variants": 1}}}
        ref = RT.decomposition_reference(prob)
        self.assertEqual(ref["parents"], ["o__r/a.f.L1"])
        self.assertIn("preserves", ref["meaning"].replace("without changing", "preserves"))
        old = copy.deepcopy(prob)
        old["instantiation"]["provenance"] = ["probed: N=4"]        # wave-N1/G1 decomposition children keep no meaning
        self.assertIsNone(RT.decomposition_reference(old))


class GnarkSizePolicy(unittest.TestCase):
    def test_reference_rebuild_policy(self):
        from zk_registry import ratchet_gnark as RG
        self.assertEqual(RG.reference_policy(2000), 2000)
        self.assertEqual(RG.reference_policy(4000), 4000)        # an RT-G2 decomposition record (4,000 policy)
        self.assertEqual(RG.reference_policy(None), 2000)
        self.assertEqual(RG.reference_policy(9000), RG.CANDIDATE_SIZE_POLICY)

    def test_gnark_records_name_a_known_policy(self):
        from test_zk_registry_package import make_package
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            _, rec = make_package(tmp)
            big = copy.deepcopy(rec)
            big["circuit"].update(n_constraints=3500, size_policy={"max_constraints": 4000, "within": True})
            big["circuit"]["compiler"] = dict(big["circuit"]["compiler"], name="gnark")
            self.assertNotIn("packaged status outside the size policy", P.validate_problem(big))
            big["circuit"]["n_constraints"] = 4001
            self.assertIn("packaged status outside the size policy", P.validate_problem(big))
            big["circuit"].update(n_constraints=2500, size_policy={"max_constraints": 2500, "within": True})
            self.assertIn("unknown size policy of 2500 constraints", P.validate_problem(big))


if __name__ == "__main__":
    unittest.main()
