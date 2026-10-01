"""Instantiation planner and wrapper emission for Noir functions.

Every ledger function becomes one compiled ACIR program:

* **in-crate wrapper** (library crates; binary crates and contract-crate modules through a library copy of
  the crate): an ``#[export]`` function appended to the function's own source file, so private items and
  the file's imports resolve exactly as in the original module; ``nargo export`` compiles it like a
  ``main``;
* **stdlib wrapper**: the standard library is embedded in ``nargo``, so a wrapper crate calls the public
  item by its ``std::`` path (private stdlib items have no instantiation);
* **repository main**: ``fn main`` of a binary crate is compiled as the repository builds it;
* **contract entrypoint**: private Aztec contract functions are taken from the compiled contract artifact.

Wrapper shape: parameters are the function's parameters with concrete types (``self`` and ``&mut``
parameters by value), the body calls the function (inherent methods as ``Self::f``, trait methods as
``<Self as Trait>::f``, generic arguments always by turbofish) and returns the return value followed by
the final values of every ``&mut`` parameter, so state changes are outputs of the DET statement.

Generic parameters (of the function and its ``impl``) are assigned by tiers, first tier with a compiling
candidate wins: ``parameter-free`` (none) -> ``repo-main`` / ``repo-test`` / ``repo-derived`` (turbofish
arguments of calls to the function and concrete instantiations of the ``impl``'s self type written in the
repository, classified by the enclosing function: a ``main``, a ``#[test]`` / test module, or other code;
global constants with literal values are substituted) -> ``probed`` (numeric generics 4, 2, 1; type
generics Field, u32, u8, bool — recorded as probed; a counterexample there is labelled not-a-finding).
Within a tier the largest compiled instantiation within the size policy is chosen.

Generator 1.1 (recovery R1) adds: implementors from ``#[derive(..)]`` and the Aztec ``#[note]`` /
``#[custom_note]`` / ``#[event]`` macros, implementors declared in test modules of the module tree as a
fallback (probed), closure / function parameters supplied by a concrete function taken from a repository
call site (:func:`fn_arg_candidates`, recorded in the provenance), closure environment generics left to
inference (``_``), and values of non-ABI parameter types (references, function-typed fields, vectors, unit)
built inside the wrapper from ABI-typed inputs (:class:`ValueBuilder`; every ABI field stays an input).
Signatures that take Aztec ``PublicContext`` / ``UtilityContext`` are public-execution code
(NOT-APPLICABLE).
"""
from __future__ import annotations

import os
import re
from dataclasses import dataclass, field

from . import noir_source as NS

PRIMITIVES = {"Field", "bool", "u1", "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "str"}
PROBE_NUMERIC = ["4", "2", "1"]
PROBE_TYPES = ["Field", "u32", "u8", "bool"]
MAX_CANDIDATES_PER_TIER = 4
TIER_ORDER = ["parameter-free", "repo-main", "repo-test", "repo-derived", "probed"]
ENTRY_PRIVATE = ("private",)
ENTRY_PUBLIC = ("public",)
ENTRY_UNCONSTRAINED = ("utility", "view_unconstrained")
# Aztec execution contexts that exist only in public (AVM) or utility (unconstrained) execution
NON_CIRCUIT_TYPES = ("PublicContext", "UtilityContext")
# attribute macros that implement traits: derive(..) lists them; the Aztec note / event macros at the pins
# (aztec-nr macros/notes.nr, macros/events.nr) implement these
MACRO_TRAITS = {"note": ["NoteType", "NoteHash"], "custom_note": ["NoteType"], "event": ["EventInterface"]}
FN_TYPE = re.compile(r"(?<![A-Za-z0-9_])(fn\s*[\[(]|impl\s+Fn)")
MAX_FN_ARG_CANDIDATES = 3


@dataclass
class Target:
    item_id: str
    repo: str                      # owner/name
    root: str                      # checkout root
    path: str                      # repo-relative source path
    fn: NS.Function
    text: str                      # blanked source of the file
    src: str                       # original source of the file
    crate: str | None              # crate directory (absolute)
    crate_type: str
    stdlib: bool = False

    @property
    def rel_in_crate(self) -> str:
        return os.path.relpath(os.path.join(self.root, self.path), os.path.join(self.crate, "src"))

    @property
    def generics(self) -> list[tuple[str, NS.Generic]]:
        imp = self.fn.impl
        out = [("impl", g) for g in (imp.generics if imp is not None else [])]
        return out + [("fn", g) for g in self.fn.generics]


def contract_attr(fn: NS.Function) -> str:
    """Aztec entrypoint kind from attributes: private | public | utility | '' (helper)."""
    for a in fn.attrs:
        a = a.strip()
        m = re.match(r"external\s*\(\s*\"(\w+)\"\s*\)", a)
        if m:
            return m.group(1)
        if re.match(r"(aztec::macros::functions::)?(private|public|utility)\b", a):
            return re.match(r"(?:aztec::macros::functions::)?(\w+)", a).group(1)
    return ""


def classify(t: Target) -> tuple[str, str]:
    """(plan, reason).  plan: export | std-export | bin-main | contract-fn | NOT-APPLICABLE | NO-INSTANTIATION."""
    fn = t.fn
    if fn.unconstrained:
        return "NOT-APPLICABLE", "unconstrained function: compiled to Brillig bytecode only, no ACIR"
    if fn.comptime:
        return "NOT-APPLICABLE", "comptime function: evaluated at compile time, no ACIR"
    if fn.attr("oracle") or fn.attr("builtin"):
        return "NOT-APPLICABLE", "oracle / builtin declaration: no ACIR of its own"
    sig = [p.type for p in fn.params] + ([fn.impl.self_type] if fn.impl is not None else [])
    for ty in sig:
        hit = next((n for n in NON_CIRCUIT_TYPES if re.search(r"(?<![A-Za-z0-9_])" + n + r"(?![A-Za-z0-9_])", ty)), None)
        if hit:
            return "NOT-APPLICABLE", (f"public-execution code: the signature takes `{hit}`, which exists only in Aztec "
                                      "public / utility execution (Brillig, transpiled to AVM bytecode), never in an "
                                      "ACIR circuit")
    for prm in fn.params:
        if re.search(r"(?<![A-Za-z0-9_])impl\s+Fn", prm.type):
            return "NO-INSTANTIATION", (f"parameter `{prm.pattern}: {prm.type}` has an `impl Fn` type, which is not an "
                                        "ABI type, so no circuit takes it as an input")
    in_contract = any(b.kind == "contract" for b in fn.blocks)
    if in_contract:
        kind = contract_attr(fn)
        if kind in ENTRY_PRIVATE:
            return "contract-fn", "private contract entrypoint, compiled as part of the contract artifact"
        if kind in ENTRY_PUBLIC:
            return "NOT-APPLICABLE", ("Aztec public function: compiled to Brillig and transpiled to AVM bytecode, "
                                      "no ACIR")
        if kind in ENTRY_UNCONSTRAINED:
            return "NOT-APPLICABLE", "Aztec utility function: unconstrained, no ACIR"
        return "NO-INSTANTIATION", ("helper inside a contract block: contract crates have no export mechanism and "
                                    "the helper is compiled only inlined into contract entrypoints")
    if t.stdlib:
        imp = fn.impl
        if imp is None and not fn.visibility.startswith("pub"):
            return "NO-INSTANTIATION", ("private standard-library item: the stdlib is embedded in nargo, so no wrapper "
                                        "crate can call it")
        if imp is not None and not imp.trait and imp.kind == "impl" and not fn.visibility.startswith("pub"):
            return "NO-INSTANTIATION", "private inherent method of the standard library: not callable from a wrapper crate"
        if fn.visibility.startswith("pub(crate)"):
            return "NO-INSTANTIATION", "pub(crate) standard-library item: not callable from a wrapper crate"
        return "std-export", ""
    if t.crate_type == "bin" and fn.name == "main" and not fn.blocks:
        return "bin-main", "the binary crate's own main"
    return "export", ""


# ------------------------------------------------------------------------------------------ repository index

def is_test_path(rel: str) -> bool:
    """A file of test code: a path component naming tests or mocks (``tests/``, ``test_helpers.nr``, ``mocks/``)."""
    return any(re.search(r"(^|_)(tests?|mocks?)(_|$)|^test_|_test$", re.sub(r"\.nr$", "", c))
               for c in rel.split("/"))


@dataclass
class Occurrence:
    tier: str
    args: list[str]
    where: str


@dataclass
class RepoIndex:
    root: str
    files: dict = field(default_factory=dict)          # rel path -> (fns, blocks, text)
    globals: dict = field(default_factory=dict)        # name -> literal text
    structs: dict = field(default_factory=dict)        # name -> has generics
    trait_impls: dict = field(default_factory=dict)    # trait name -> [self types]
    struct_files: dict = field(default_factory=dict)   # struct name -> [rel paths declaring it]
    struct_defs: dict = field(default_factory=dict)    # struct name -> [(rel, NS.StructDef, in test code)]
    test_struct_files: dict = field(default_factory=dict)   # struct name -> [rel] (declared in test code)
    test_trait_impls: dict = field(default_factory=dict)    # trait name -> [self types] (in test code)
    decl_files: dict = field(default_factory=dict)     # struct / trait / type name -> [rel] (outside test code)

    @classmethod
    def build(cls, root: str, rels: list[str]) -> "RepoIndex":
        idx = cls(root)
        for rel in rels:
            p = os.path.join(root, rel)
            try:
                with open(p, encoding="utf-8", errors="replace") as f:
                    src = f.read()
            except OSError:
                continue
            fns, blocks, text = NS.scan(src)
            idx.files[rel] = (fns, blocks, text)
            idx.globals.update(NS.globals_with_values(text))
            test_ranges = [(b.start, b.end) for b in blocks if b.kind == "mod" and re.search(r"test|mock", b.self_type)]
            for m in re.finditer(r"(?<![A-Za-z0-9_])struct\s+(" + NS.IDENT + r")\s*(<)?", text):
                if is_test_path(rel) or any(a < m.start() < b for a, b in test_ranges):
                    idx.test_struct_files.setdefault(m.group(1), []).append(rel)
                    continue                      # test-only types are candidates only as a fallback
                idx.structs[m.group(1)] = bool(m.group(2))
                idx.struct_files.setdefault(m.group(1), []).append(rel)
            for m in re.finditer(r"(?<![A-Za-z0-9_])(?:struct|trait|type)\s+(" + NS.IDENT + r")", text):
                if not (is_test_path(rel) or any(a < m.start() < b for a, b in test_ranges)):
                    idx.decl_files.setdefault(m.group(1), []).append(rel)
            for sd in NS.structs(text, src):
                in_test = is_test_path(rel) or any(a < sd.start < b for a, b in test_ranges)
                idx.struct_defs.setdefault(sd.name, []).append((rel, sd, in_test))
                if not sd.generics:
                    for tr in derived_traits(sd.attrs):
                        (idx.test_trait_impls if in_test else idx.trait_impls).setdefault(tr, []).append(sd.name)
            test_file = is_test_path(rel)
            test_mods = [b for b in blocks if b.kind == "mod" and re.search(r"test", b.self_type)]
            for b in blocks:
                in_test = test_file or any(m.start < b.start < m.end for m in test_mods)
                if b.kind == "impl" and b.trait and not b.generics:
                    tm = re.match(r"\s*([A-Za-z_][A-Za-z0-9_:]*)", b.trait)
                    if tm:
                        # implementors in test code are candidates only as a fallback
                        (idx.test_trait_impls if in_test else idx.trait_impls).setdefault(
                            tm.group(1).split("::")[-1], []).append(b.self_type)
        return idx

    def struct_def(self, name: str, scope: list[str] | None = None) -> tuple[str, "NS.StructDef"] | None:
        """The declaration of struct ``name`` (inside ``scope`` src directories when given; non-test first)."""
        defs = self.struct_defs.get(name, [])
        if scope is not None:
            defs = [d for d in defs if any(d[0] == s or d[0].startswith(s + "/") for s in scope)]
        defs = sorted(defs, key=lambda d: (d[2], d[0]))
        return (defs[0][0], defs[0][1]) if defs else None

    def context_tier(self, rel: str, pos: int, crate_type: str) -> str:
        fns, blocks, _ = self.files[rel]
        enclosing = [f for f in fns if f.start <= pos < f.end]
        f = min(enclosing, key=lambda x: x.end - x.start) if enclosing else None
        test_path = is_test_path(rel)
        in_test_mod = any(b.kind == "mod" and re.search(r"test", b.self_type) and b.start < pos < b.end for b in blocks)
        if f is not None and f.attr("test"):
            return "repo-test"
        if test_path or in_test_mod:
            return "repo-test"
        if f is not None and f.name == "main" and not f.blocks:
            return "repo-main"
        return "repo-derived"

    def resolve(self, arg: str, depth: int = 0) -> str | None:
        """A concrete generic argument (globals substituted), or None."""
        a = arg.strip()
        if depth > 6 or not a:
            return None
        if re.fullmatch(r"\d+(_\d+)*", a):
            return a.replace("_", "")
        if a in PRIMITIVES:
            return a
        m = re.fullmatch(r"str\s*<\s*(.+)\s*>", a)
        if m:
            n = self.resolve(m.group(1), depth + 1)
            return f"str<{n}>" if n is not None else None
        m = re.fullmatch(r"\[\s*(.+)\s*;\s*(.+)\s*\]", a)
        if m:
            inner, n = self.resolve(m.group(1), depth + 1), self.resolve(m.group(2), depth + 1)
            return f"[{inner}; {n}]" if inner is not None and n is not None else None
        if a.startswith("(") and a.endswith(")"):
            parts = NS.split_top(a[1:-1])
            rs = [self.resolve(p, depth + 1) for p in parts]
            return "(" + ", ".join(rs) + ("," if len(rs) == 1 else "") + ")" if all(r is not None for r in rs) else None
        if re.fullmatch(NS.IDENT, a):
            if a in self.globals:
                return self.resolve(self.globals[a], depth + 1)
            if self.structs.get(a) is False:
                return a
            return None
        # integer arithmetic of literals and globals
        if re.fullmatch(r"[A-Za-z0-9_+\-*/ ()]+", a):
            expr = re.sub(NS.IDENT, lambda m: self.resolve(m.group(0), depth + 1) or "X", a)
            if "X" not in expr and re.fullmatch(r"[0-9+\-*/ ()]+", expr):
                try:
                    v = eval(expr.replace("/", "//"), {"__builtins__": {}}, {})       # noqa: S307 (digits/operators only)
                    return str(int(v)) if v >= 0 else None
                except (SyntaxError, ZeroDivisionError, ValueError):
                    return None
        return None


def derived_traits(attrs: list[str]) -> list[str]:
    """Traits an attribute list implements: ``derive(A, b::B)`` -> [A, B]; the Aztec note / event macros
    (:data:`MACRO_TRAITS`)."""
    out = []
    for a in attrs:
        a = a.strip()
        m = re.match(r"derive\s*\((.*)\)\s*$", a, re.S)
        if m:
            out += [x.strip().split("::")[-1] for x in NS.split_top(m.group(1)) if x.strip()]
            continue
        name = re.match(r"([A-Za-z_][A-Za-z0-9_:]*)", a)
        if name:
            out += MACRO_TRAITS.get(name.group(1).split("::")[-1], [])
    return out


def _self_type_parts(self_type: str) -> tuple[str, list[str]]:
    m = re.match(r"\s*([A-Za-z_][A-Za-z0-9_:]*)\s*(<.*>)?\s*$", self_type)
    if not m:
        return "", []
    base = m.group(1).split("::")[-1]
    args = NS.split_top(m.group(2)[1:-1]) if m.group(2) else []
    return base, args


def repo_occurrences(t: Target, idx: RepoIndex) -> list[tuple[str, dict, str]]:
    """(tier, partial assignment, provenance) from turbofish calls and self-type instantiations."""
    out = []
    fn, imp = t.fn, t.fn.impl
    fn_gen = [g.name for g in fn.generics]
    impl_gen = [g.name for g in imp.generics] if imp is not None else []
    base, sargs = _self_type_parts(imp.self_type) if imp is not None and imp.kind == "impl" else ("", [])
    for rel, (fns, blocks, text) in idx.files.items():
        if fn_gen:
            for m in re.finditer(r"(?<![A-Za-z0-9_])" + re.escape(fn.name) + r"\s*::\s*<", text):
                k = NS.matching_angle(text, m.end() - 1)
                args = NS.split_top(text[m.end():k - 1])
                if len(args) != len(fn_gen):
                    continue
                vals = [idx.resolve(a) for a in args]
                if any(v is None for v in vals):
                    continue
                tier = idx.context_tier(rel, m.start(), t.crate_type)
                out.append((tier, dict(zip(fn_gen, vals)), f"{rel}:{NS.line_of(text, m.start())} {fn.name}::<{', '.join(args)}>"))
        if impl_gen and base and sargs:
            for m in re.finditer(r"(?<![A-Za-z0-9_])" + re.escape(base) + r"\s*(::\s*)?<", text):
                k = NS.matching_angle(text, m.end() - 1)
                args = NS.split_top(text[m.end():k - 1])
                if len(args) != len(sargs):
                    continue
                assign = {}
                ok = True
                for formal, actual in zip(sargs, args):
                    formal = formal.strip()
                    if formal in impl_gen:
                        v = idx.resolve(actual)
                        if v is None:
                            ok = False
                            break
                        assign[formal] = v
                if not ok or not assign:
                    continue
                tier = idx.context_tier(rel, m.start(), t.crate_type)
                out.append((tier, assign, f"{rel}:{NS.line_of(text, m.start())} {base}<{', '.join(args)}>"))
    return out


def env_generics(t: Target) -> list[str]:
    """Generic parameters that are closure environments (``fn[Env](..)``): left to inference (``_``)."""
    out = []
    for p in t.fn.params:
        for e in re.findall(r"fn\s*\[\s*([A-Za-z_][A-Za-z0-9_]*)\s*\]", p.type):
            if e not in out:
                out.append(e)
    return [g.name for _, g in t.generics if g.name in out]


def candidates(t: Target, idx: RepoIndex | None) -> list[tuple[str, dict, list[str]]]:
    """Candidate assignments (tier, {generic: value}, provenance) in tier order, at most
    :data:`MAX_CANDIDATES_PER_TIER` per tier.  Closure environment generics are assigned ``_``."""
    env = env_generics(t)
    if not env:
        return _candidates(t, idx, [])
    return [(tier, dict(a, **{e: "_" for e in env}), prov) for tier, a, prov in _candidates(t, idx, env)]


def _candidates(t: Target, idx: RepoIndex | None, env: list[str]) -> list[tuple[str, dict, list[str]]]:
    gens = [(s, g) for s, g in t.generics if g.name not in env]
    if not gens:
        return [("parameter-free", {}, ["no generic parameters" if not env else
                                        "no generic parameters besides closure environments"])]
    names = [g.name for _, g in gens]
    out: list[tuple[str, dict, list[str]]] = []
    if idx is not None:
        occ = repo_occurrences(t, idx)
        by_tier: dict[str, list] = {}
        vis = visible_in(t, idx)
        for tier, assign, prov in occ:
            tnames = {n for v in assign.values() for n in re.findall(r"(?<![A-Za-z0-9_:])([A-Z][A-Za-z0-9_]*)", v)}
            if any(n not in PRIMITIVES and not vis(n) for n in tnames):
                continue                          # a type the target's crate cannot name
            by_tier.setdefault(tier, []).append((assign, prov))
        for tier in ("repo-main", "repo-test", "repo-derived"):
            parts = by_tier.get(tier, [])
            seen = []
            # complete assignments directly; combine a self-type part with a turbofish part of the same tier
            complete = [(a, [p]) for a, p in parts if set(a) == set(names)]
            fn_parts = [(a, p) for a, p in parts if set(a) and set(a) <= {g.name for g in t.fn.generics}]
            impl_parts = [(a, p) for a, p in parts if set(a) and set(a) <= {g.name for s, g in gens if s == "impl"}]
            for ia, ip in impl_parts:
                for fa, fp in fn_parts or [({}, None)]:
                    merged = {**ia, **fa}
                    if set(merged) == set(names):
                        complete.append((merged, [ip] + ([fp] if fp else [])))
            for a, prov in complete:
                key = tuple(sorted(a.items()))
                if key in seen:
                    continue
                seen.append(key)
                out.append((tier, a, prov))
                if len(seen) >= MAX_CANDIDATES_PER_TIER:
                    break
    numeric = [g.name for _, g in gens if g.numeric]
    types = [g for _, g in gens if not g.numeric]
    bounds = where_bounds(t)
    type_values = []
    test_types: set = set()
    for g in types:
        bs = [b for b in g.bounds + bounds.get(g.name, []) if b]
        vals = implementors(idx, bs, visible_in(t, idx)) if bs and idx is not None else list(PROBE_TYPES)
        if not vals and bs and idx is not None:
            # fallback: implementors declared in test modules of the repository (named from the target's crate)
            vals = implementors(idx, bs, visible_in(t, idx, test=True), test=True)
            test_types |= set(vals)
        type_values.append(vals)
    probes = []
    for k in range(max([len(v) for v in type_values] + [1])):
        for nv in (PROBE_NUMERIC if numeric else [None]):
            a = {n: nv for n in numeric}
            for g, vals in zip(types, type_values):
                if vals:
                    a[g.name] = vals[min(k, len(vals) - 1)]
            if len(a) == len(numeric) + len(types) and a not in probes:
                probes.append(a)
    # numeric first (largest), Field first
    for a in probes[:MAX_CANDIDATES_PER_TIER]:
        prov = ["probed: " + ", ".join(f"{k}={v}" for k, v in a.items())]
        tt = sorted(v for v in a.values() if v in test_types)
        if tt:
            prov.append("implementor(s) declared in repository test code (no other implementor of the bounds): "
                        + ", ".join(tt))
        out.append(("probed", a, prov))
    return out


def crate_scope(t: Target) -> list[str]:
    """``src`` directories of the target's crate and of its path dependencies (repository-relative)."""
    from .noir_toolchain import read_toml
    out, todo, seen = [], [t.crate] if t.crate else [], set()
    while todo:
        d = todo.pop()
        if d in seen:
            continue
        seen.add(d)
        out.append(os.path.relpath(os.path.join(d, "src"), t.root))
        m = os.path.join(d, "Nargo.toml")
        if os.path.exists(m):
            for spec in read_toml(m).get("dependencies", {}).values():
                if isinstance(spec, dict) and "path" in spec:
                    todo.append(os.path.normpath(os.path.join(d, spec["path"])))
    return out


def visible_in(t: Target, idx: RepoIndex, test: bool = False):
    """Predicate: a struct name the target's crate can name (declared outside test code in the crate or a
    path dependency; with ``test`` also in its test code)."""
    scope = crate_scope(t)

    def ok(name: str) -> bool:
        files = idx.struct_files.get(name, []) + (idx.test_struct_files.get(name, []) if test else [])
        return any(f == s or f.startswith(s + "/") for f in files for s in scope)
    return ok


def where_bounds(t: Target) -> dict[str, list[str]]:
    """``where T: A + B, U: C`` of the function and its impl -> {T: [A, B], U: [C]}."""
    out: dict[str, list[str]] = {}
    texts = [t.fn.where] + ([t.fn.impl.where] if t.fn.impl is not None else [])
    for w in texts:
        for part in NS.split_top(w or ""):
            m = re.match(r"\s*(" + NS.IDENT + r")\s*:\s*(.+)$", part)
            if m:
                out.setdefault(m.group(1), []).extend(b.strip() for b in NS.split_top(m.group(2), "+"))
    return out


def implementors(idx: RepoIndex, bounds: list[str], visible=None, test: bool = False) -> list[str]:
    """Non-generic types the repository implements every bound trait for (primitives first); ``visible``
    filters out types the target's crate cannot name (declared in other crates or in test code); with
    ``test`` the implementations and derives of test code count as well.  Aztec public / utility contexts
    are never candidates."""
    sets = []
    for b in bounds:
        name = re.match(r"\s*([A-Za-z_][A-Za-z0-9_:]*)", b)
        if not name:
            continue
        tname = name.group(1).split("::")[-1]
        impls = idx.trait_impls.get(tname, []) + (idx.test_trait_impls.get(tname, []) if test else [])
        sets.append({x.strip() for x in impls if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", x.strip())})
    if not sets:
        return list(PROBE_TYPES)
    common = set.intersection(*sets) - set(NON_CIRCUIT_TYPES)
    if visible is not None:
        common = {x for x in common if x in PRIMITIVES or visible(x)}
    return sorted(common, key=lambda x: (x not in PRIMITIVES, PROBE_TYPES.index(x) if x in PROBE_TYPES else 99, x))[:4]


# ------------------------------------------------------------------------------------------ function arguments

def fn_param_indices(t: Target) -> list[int]:
    """Indices of parameters with a function (closure) type."""
    return [i for i, p in enumerate(t.fn.params) if p.pattern != "self" and FN_TYPE.search(p.type)
            and not re.search(r"impl\s+Fn", p.type)]


def file_scope_paths(idx: RepoIndex, rel: str, crate_src: str) -> dict[str, str]:
    """name -> absolute path for the items a file declares (``crate::<module>::name``) and its top-level
    ``use`` declarations (``crate::`` / ``super::`` / ``self::`` made absolute; dependency paths kept)."""
    fns, _, text = idx.files[rel]
    src_rel = os.path.relpath(os.path.join(idx.root, rel), crate_src)
    mod = NS.mod_path(src_rel)
    here = "crate::" + "".join(p + "::" for p in mod)
    out: dict[str, str] = {}
    for f in fns:
        if not f.blocks:
            out.setdefault(f.name, here + f.name)
    for m in re.finditer(r"(?<![A-Za-z0-9_])(?:struct|trait|global|type)\s+(" + NS.IDENT + r")", text):
        out.setdefault(m.group(1), here + m.group(1))
    for u in NS.uses(text, top_level=True):
        for path in _expand_use_crate(u, mod):
            name = path.split("::")[-1]
            alias = re.search(r"\s+as\s+(" + NS.IDENT + r")$", path)
            if alias:
                name, path = alias.group(1), path[:alias.start()]
            out[name] = path
    return out


def _expand_use_crate(u: str, mod: list[str]) -> list[str]:
    u = u.strip()
    m = re.match(r"(.*?)\{(.*)\}\s*$", u, re.S)
    if m:
        out = []
        for part in NS.split_top(m.group(2)):
            if part.strip() and part.strip() != "self":
                out += _expand_use_crate(m.group(1) + part.strip(), mod)
        return out
    if u.startswith("super::"):
        parent, rest = mod[:-1], u[len("super::"):]
        while rest.startswith("super::"):
            parent, rest = parent[:-1], rest[len("super::"):]
        return ["crate::" + "".join(p + "::" for p in parent) + rest]
    if u.startswith("self::"):
        return ["crate::" + "".join(p + "::" for p in mod) + u[len("self::"):]]
    return [u]


def crate_src_of(root: str, rel: str) -> str | None:
    """The ``src`` directory of the Nargo package containing ``rel``."""
    d = os.path.dirname(os.path.join(root, rel))
    while len(d) >= len(root):
        if os.path.exists(os.path.join(d, "Nargo.toml")):
            return os.path.join(d, "src")
        d = os.path.dirname(d)
    return None


def _usable_path(path: str, site_crate_src: str | None, t: Target) -> str | None:
    """A path written at a call site, usable inside the target's file: ``std::`` paths, and ``crate::`` paths
    when the call site is in the target's crate."""
    if path.startswith("std::"):
        return path
    if path.startswith("crate::") and t.crate is not None and site_crate_src == os.path.join(t.crate, "src"):
        return path
    return None


def split_call_args(s: str) -> list[str]:
    """Top-level call arguments; a closure's parameter list (``|a, b|``) is not split."""
    parts, out = NS.split_top(s), []
    while parts:
        p = parts.pop(0)
        while p.lstrip().startswith("|") and p.count("|") < 2 and parts:
            p = p + ", " + parts.pop(0)
        out.append(p)
    return out


def fn_arg_candidates(t: Target, idx: RepoIndex | None, k: int) -> list[tuple[str, str]]:
    """Concrete functions for the function-typed parameter ``k`` taken from calls of the target written in the
    repository (tests included): closure literals and named functions (qualified through the call site's
    scope).  (expression, provenance) pairs, at most :data:`MAX_FN_ARG_CANDIDATES`; call sites in the
    target's crate first, closures that always fail last, closures before named functions, shorter first."""
    if idx is None:
        return []
    fn = t.fn
    has_self = bool(fn.params) and fn.params[0].pattern == "self"
    n_params = len(fn.params)
    found: dict[str, tuple] = {}
    pat = re.compile(r"(\.\s*|::\s*|(?<![A-Za-z0-9_]))" + re.escape(fn.name) + r"\s*(::\s*<)?")
    target_src = os.path.join(t.crate, "src") if t.crate else None
    for rel, (fns, _, text) in idx.files.items():
        if fn.name not in text:
            continue
        site_src = crate_src_of(idx.root, rel)
        scope = None
        for m in pat.finditer(text):
            if re.search(r"fn\s+$", text[max(0, m.start() - 8):m.start() + len(m.group(1))]):
                continue                                          # the declaration
            i = m.end()
            if m.group(2):
                i = NS.matching_angle(text, i - 1)
            while i < len(text) and text[i] in " \t\r\n":
                i += 1
            if i >= len(text) or text[i] != "(":
                continue
            close = NS.matching(text, i, "(", ")")
            args = split_call_args(text[i + 1:close - 1])
            method = m.group(1).startswith(".")
            if method:
                if not has_self or len(args) != n_params - 1:
                    continue
                pos = k - 1
            else:
                if len(args) != n_params:
                    continue
                pos = k
            if not 0 <= pos < len(args):
                continue
            arg = re.sub(r"\s+", " ", args[pos]).strip()
            expr = None
            if arg.startswith("|"):
                expr = arg
            elif re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*(::<[^()]*>)?", arg):
                head = arg.split("::")[0]
                if scope is None and site_src is not None:
                    scope = file_scope_paths(idx, rel, site_src)
                full = arg if "::" in arg and head in ("std", "crate") else None
                if full is None and scope is not None and head in scope:
                    full = scope[head] + arg[len(head):]
                expr = _usable_path(full, site_src, t) if full else None
            if expr is None or expr in found:
                continue
            same = site_src == target_src
            # closures that always fail (`assert(false)`) are tried last: no execution would satisfy them
            found[expr] = (not same, bool(re.search(r"assert\s*\(\s*false", expr)), not expr.startswith("|"), len(expr),
                           f"{rel}:{NS.line_of(text, m.start())} argument `{arg[:120]}`")
    ranked = sorted(found.items(), key=lambda kv: kv[1][:4])
    return [(e, v[4]) for e, v in ranked[:MAX_FN_ARG_CANDIDATES]]


INT_TYPES = ("u1", "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "Field")


def const_arg_candidates(t: Target, idx: RepoIndex | None, taken: dict) -> dict[int, tuple[str, str]]:
    """Integer parameters fixed to a literal written at a repository call site (globals resolved), for a
    function the compiler needs compile-time arguments for: {index: (literal, provenance)}.  Only parameters
    of integer / Field type that are not already supplied in ``taken``; the first call site with a literal
    (in the target's crate first) wins."""
    if idx is None:
        return {}
    out: dict[int, tuple[str, str]] = {}
    for k, p in enumerate(t.fn.params):
        if p.pattern == "self" or k in taken or p.type.strip() not in INT_TYPES:
            continue
        for arg, prov in call_site_args(t, idx, k):
            v = idx.resolve(re.sub(r"(u|i)(8|16|32|64|128)$", "", arg.strip()))
            if v is not None and re.fullmatch(r"\d+", v):
                out[k] = (v, prov)
                break
    return out


def call_site_args(t: Target, idx: RepoIndex, k: int) -> list[tuple[str, str]]:
    """Argument texts at parameter position ``k`` of the calls of the target written in the repository
    (method calls map ``self`` away; the target's crate first)."""
    fn = t.fn
    has_self = bool(fn.params) and fn.params[0].pattern == "self"
    n_params = len(fn.params)
    pat = re.compile(r"(\.\s*|::\s*|(?<![A-Za-z0-9_]))" + re.escape(fn.name) + r"\s*(::\s*<)?")
    target_src = os.path.join(t.crate, "src") if t.crate else None
    out = []
    for rel, (fns, _, text) in idx.files.items():
        if fn.name not in text:
            continue
        same = crate_src_of(idx.root, rel) == target_src
        for m in pat.finditer(text):
            if re.search(r"fn\s+$", text[max(0, m.start() - 8):m.start() + len(m.group(1))]):
                continue
            i = m.end()
            if m.group(2):
                i = NS.matching_angle(text, i - 1)
            while i < len(text) and text[i] in " \t\r\n":
                i += 1
            if i >= len(text) or text[i] != "(":
                continue
            args = split_call_args(text[i + 1:NS.matching(text, i, "(", ")") - 1])
            method = m.group(1).startswith(".")
            if method and (not has_self or len(args) != n_params - 1):
                continue
            if not method and len(args) != n_params:
                continue
            pos = k - 1 if method else k
            if 0 <= pos < len(args):
                out.append((not same, args[pos], f"{rel}:{NS.line_of(text, m.start())} argument `{args[pos][:80]}`"))
    return [(a, pv) for _, a, pv in sorted(out, key=lambda x: x[0])]


# ------------------------------------------------------------------------------------------ non-ABI values

@dataclass
class Built:
    params: list[str]
    pre: list[str]
    expr: str
    outs: list[tuple[str, str]]
    notes: list[str]
    abi_fields: list[tuple[str, str]] = field(default_factory=list)     # struct builds: ABI-typed fields


class ValueBuilder:
    """Builds a value of a non-ABI type inside the wrapper from ABI-typed inputs: a struct from its fields
    (every ABI field is a wrapper parameter, so it stays a DET input), ``&mut T`` / ``&T`` through a local
    whose final value is an output, function-typed fields from the function the repository stores in that
    field (a struct literal in the crate), ``[T]`` from an array of 4 elements and ``()`` as itself."""

    def __init__(self, t: Target, idx: RepoIndex | None, vector_method: str = "as_vector"):
        self.t, self.idx, self.vector_method = t, idx, vector_method
        self.scope = crate_scope(t) if idx is not None and t.crate else None

    def struct_of(self, ty: str):
        if self.t.stdlib:
            return None                       # stdlib wrappers live in another crate: only [T] and () are built
        m = re.fullmatch(r"\s*((?:[A-Za-z_][A-Za-z0-9_]*::)*)([A-Za-z_][A-Za-z0-9_]*)\s*(<.*>)?\s*", ty)
        if not m or self.idx is None or m.group(2) in PRIMITIVES:
            return None
        found = self.idx.struct_def(m.group(2), self.scope)
        if found is None:
            return None
        rel, sd = found
        args = NS.split_top(m.group(3)[1:-1]) if m.group(3) else []
        if len(args) != len(sd.generics):
            return None
        mapping = {g.name: a for g, a in zip(sd.generics, args)}
        return rel, sd, [(v, n, fold_literals(subst(ft, mapping, None))) for v, n, ft in sd.fields]

    def needs(self, ty: str, depth: int = 0) -> bool:
        ty = ty.strip()
        if depth > 6 or not ty:
            return False
        if ty.startswith("&") or FN_TYPE.match(ty) or ty == "()":
            return True
        m = re.fullmatch(r"\[(.*)\]", ty, re.S)
        if m:
            parts = NS.split_top(m.group(1), ";")
            return len(parts) == 1 or self.needs(parts[0], depth + 1)
        if ty.startswith("(") and ty.endswith(")"):
            return any(self.needs(p, depth + 1) for p in NS.split_top(ty[1:-1]))
        st = self.struct_of(ty)
        return st is not None and any(self.needs(ft, depth + 1) for _, _, ft in st[2])

    def build(self, ty: str, name: str, depth: int = 0) -> Built:
        ty = ty.strip()
        if depth > 6:
            raise ValueError(f"type nesting too deep: {ty}")
        if not self.needs(ty):
            return Built([f"{name}: {ty}"], [], name, [], [])
        m = re.match(r"&\s*(mut\s+)?(.+)$", ty, re.S)
        if m:
            inner = self.build(m.group(2), f"{name}_r", depth + 1)
            loc = f"{name}_l"
            pre = inner.pre + [f"let {'mut ' if m.group(1) else ''}{loc} = {inner.expr};"]
            outs = inner.outs + ([(loc, m.group(2).strip())] if m.group(1) and not self.needs(m.group(2)) else [])
            return Built(inner.params, pre, f"&{'mut ' if m.group(1) else ''}{loc}", outs,
                         inner.notes + [f"`{ty}` through the local `{loc}`"])
        if ty == "()":
            return Built([], [], "()", [], ["unit value"])
        if FN_TYPE.match(ty):
            raise ValueError(f"function type `{ty}` without a repository function")
        m = re.fullmatch(r"\[(.*)\]", ty, re.S)
        if m:
            parts = NS.split_top(m.group(1), ";")
            if len(parts) == 1:
                inner = self.build(f"[{parts[0]}; 4]", f"{name}_a", depth + 1)
                return Built(inner.params, inner.pre, f"{inner.expr}.{self.vector_method}()", inner.outs,
                             inner.notes + [f"`{ty}` from an array of 4 elements (`{self.vector_method}`)"])
            raise ValueError(f"array of non-ABI elements `{ty}`")
        if ty.startswith("(") and ty.endswith(")"):
            parts = [self.build(p, f"{name}_{i}", depth + 1) for i, p in enumerate(NS.split_top(ty[1:-1]))]
            return Built(sum((b.params for b in parts), []), sum((b.pre for b in parts), []),
                         "(" + ", ".join(b.expr for b in parts) + ("," if len(parts) == 1 else "") + ")",
                         sum((b.outs for b in parts), []), sum((b.notes for b in parts), []))
        st = self.struct_of(ty)
        if st is None:
            raise ValueError(f"no declaration for `{ty}`")
        rel, sd, fields = st
        q = (lambda x: x) if rel == self.t.path else (lambda x: self.qualify(x, rel))
        params, pre, outs, notes, inits, abi = [], [], [], [], [], []
        for _, fname, ft in fields:
            ft = q(ft)
            if FN_TYPE.match(ft):
                vals = self.field_fn_values(rel, sd.name, fname)
                if not vals:
                    raise ValueError(f"no repository function for the field `{sd.name}.{fname}: {ft}`")
                inits.append(f"{fname}: {vals[0][0]}")
                notes.append(f"`{sd.name}.{fname}` = `{vals[0][0]}` ({vals[0][1]})")
                continue
            b = self.build(ft, f"{name}_{fname}", depth + 1)
            if not self.needs(ft):
                abi.append((fname, ft))
            params += b.params
            pre += b.pre
            outs += b.outs
            notes += b.notes
            inits.append(f"{fname}: {b.expr}")
        var = f"{name}_v"
        pre.append(f"let {var}: {ty} = {q(sd.name)} {{ {', '.join(inits)} }};")
        return Built(params, pre, var, outs, notes + [f"`{ty}` built from its fields"], abi)

    def qualify(self, text: str, rel: str) -> str:
        """Names of a declaration in another file of the target's crate, written as absolute paths from that
        file's scope (its own items and ``use`` declarations)."""
        src = crate_src_of(self.idx.root, rel)
        if src is None or self.t.crate is None or src != os.path.join(self.t.crate, "src"):
            return text
        scope = file_scope_paths(self.idx, rel, src)

        def rep(m):
            w = m.group(0)
            return scope.get(w, w) if w not in PRIMITIVES else w
        return re.sub(r"(?<![A-Za-z0-9_:])[A-Za-z_][A-Za-z0-9_]*(?![A-Za-z0-9_])", rep, text)

    def field_fn_values(self, rel: str, struct: str, fname: str) -> list[tuple[str, str]]:
        """Functions the crate stores in ``struct.fname``: values of that field in struct literals
        (``Struct { .. }`` / ``Self { .. }`` inside its impls) of the crate, qualified for the target file."""
        idx = self.idx
        out: list[tuple[str, str]] = []
        site_src = crate_src_of(idx.root, rel)
        for frel, (fns, blocks, text) in idx.files.items():
            if self.scope is not None and not any(frel.startswith(s + "/") for s in self.scope):
                continue
            if struct not in text:
                continue
            scope = None
            for m in re.finditer(r"(?<![A-Za-z0-9_])(" + re.escape(struct) + r"|Self)\s*\{", text):
                if m.group(1) == "Self":
                    enc = [b for b in blocks if b.kind == "impl" and b.start < m.start() < b.end]
                    if not enc or _self_type_parts(enc[-1].self_type)[0] != struct:
                        continue
                if re.search(r"(impl|for|struct|trait|mod|contract)\s*(<[^{]*>)?\s*$", text[max(0, m.start() - 60):m.start()]):
                    continue
                close = NS.matching(text, m.end() - 1, "{", "}")
                for part in NS.split_top(text[m.end():close - 1]):
                    fm = re.match(re.escape(fname) + r"\s*:\s*(.+)$", part.strip(), re.S)
                    if not fm:
                        continue
                    v = re.sub(r"\s+", " ", fm.group(1)).strip()
                    expr = None
                    if v.startswith("|"):
                        expr = v
                    elif re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*", v):
                        if frel == self.t.path:
                            expr = v
                        else:
                            fsrc = crate_src_of(idx.root, frel)
                            if scope is None and fsrc:
                                scope = file_scope_paths(idx, frel, fsrc)
                            head = v.split("::")[0]
                            full = scope.get(head) + v[len(head):] if scope and head in scope else None
                            expr = _usable_path(full, fsrc, self.t) if full else None
                    if expr and expr not in [e for e, _ in out]:
                        out.append((expr, f"{frel}:{NS.line_of(text, m.start())} `{struct} {{ {fname}: {v[:80]} }}`"))
        return out


# ------------------------------------------------------------------------------------------ wrappers

def subst(text: str, assign: dict, self_alias: str | None) -> str:
    def rep(m):
        w = m.group(0)
        if w == "Self" and self_alias:
            return self_alias
        return assign.get(w, w)
    return re.sub(r"(?<![A-Za-z0-9_:])[A-Za-z_][A-Za-z0-9_]*(?![A-Za-z0-9_])", rep, text)


@dataclass
class Wrapper:
    name: str
    text: str
    call: str
    outputs: list[str]
    mut_params: list[str]
    notes: list[str] = field(default_factory=list)       # values built for non-ABI parameters


def wrapper(t: Target, assign: dict, wid: str, qualify: str = "", method_call: bool = False,
            fn_args: dict | None = None, builder: ValueBuilder | None = None) -> Wrapper:
    """The ``#[export]`` wrapper for ``t`` under ``assign``; ``qualify`` prefixes free functions and is the
    module path for stdlib wrappers (``std::hash::``); ``fn_args`` gives the expression of function-typed
    parameters (by index); ``builder`` builds values of non-ABI parameter types from ABI inputs."""
    fn, imp = t.fn, t.fn.impl
    name = f"boole_det_{wid}"
    lines = []
    # a literal argument of a numeric generic declared with another type than u32 (`let D: u64`) is typed u32
    # in type positions; such values become typed globals, as the repositories write them
    assign = dict(assign)
    for _, g in t.generics:
        v = assign.get(g.name)
        if g.numeric and g.num_type.strip() not in ("", "u32") and v is not None and re.fullmatch(r"\d+", v):
            gname = f"BOOLE_{wid}_{g.name}".upper()
            lines.append(f"global {gname}: {g.num_type.strip()} = {v};")
            assign[g.name] = gname
    self_concrete = None
    alias = None
    if imp is not None:
        self_ty = imp.self_type if imp.kind == "impl" else assign.get("__implementor__", "")
        if not self_ty:
            raise ValueError("trait default method without an implementing type")
        self_concrete = fold_literals(subst(self_ty, assign, None))
        if qualify.startswith("std::"):
            self_concrete = self_concrete.replace("crate::", "std::")
        # the concrete type is written out everywhere (a type alias would fix numeric generic arguments to u32,
        # and an alias of one instantiation can select another impl); in expressions a generic type needs the
        # turbofish form `Base::<args>`
        alias = self_concrete
        alias_expr = re.sub(r"^([A-Za-z_][A-Za-z0-9_:]*)\s*<", r"\1::<", self_concrete)
    params, pre, args, mut_outs, notes = [], [], [], [], []
    for i, p in enumerate(fn.params):
        if fn_args and i in fn_args:
            args.append(fn_args[i])
            continue
        if p.pattern == "self":
            ty = p.type
            if ty in ("Self", "&Self", "&mut Self"):
                base = self_concrete
            else:
                base = subst(re.sub(r"^&\s*(mut\s+)?", "", ty), assign, self_concrete)
            if builder is not None and builder.needs(base):
                b = builder.build(base, "s_self")
                params += b.params
                pre += b.pre
                mutable = ty.startswith("&mut")
                pre.append(f"let {'mut ' if mutable else ''}s_self = {b.expr};")
                args.append("&mut s_self" if mutable else "&s_self" if ty.startswith("&") else "s_self")
                mut_outs += b.outs + ([(f"s_self.{f}", ft) for f, ft in b.abi_fields] if mutable else [])
                notes += b.notes
                continue
            params.append(f"s_self: {base}")
            if ty.startswith("&mut") or ty == "&mut Self":
                pre.append("let mut s_self = s_self;")
                args.append("&mut s_self")
                mut_outs.append(("s_self", base))
            elif ty.startswith("&"):
                args.append("&s_self")
            else:
                args.append("s_self")
            continue
        ty = subst(p.type, assign, self_concrete)
        an = f"a{i}"
        m = re.match(r"&\s*mut\s+(.+)$", ty)
        inner_ty = m.group(1) if m else (ty[1:].strip() if ty.startswith("&") else ty)
        if builder is not None and builder.needs(inner_ty):
            b = builder.build(inner_ty, an)
            params += b.params
            pre += b.pre
            mut_outs += b.outs + ([(f"{an}.{f}", ft) for f, ft in b.abi_fields] if m else [])
            notes += b.notes
            if m:
                pre.append(f"let mut {an} = {b.expr};")
                args.append(f"&mut {an}")
            elif ty.startswith("&"):
                pre.append(f"let {an} = {b.expr};")
                args.append(f"&{an}")
            else:
                args.append(b.expr)
            continue
        if m:
            params.append(f"{an}: {m.group(1)}")
            pre.append(f"let mut {an} = {an};")
            args.append(f"&mut {an}")
            mut_outs.append((an, m.group(1)))
        elif ty.startswith("&"):
            params.append(f"{an}: {ty[1:].strip()}")
            args.append(f"&{an}")
        else:
            params.append(f"{an}: {ty}")
            args.append(an)
    fn_gen = [assign[g.name] for g in fn.generics]
    turbo = f"::<{', '.join(fn_gen)}>" if fn_gen else ""
    if imp is None:
        callee = f"{qualify}{fn.name}{turbo}"
    elif method_call and fn.params and fn.params[0].pattern == "self":
        callee = None                     # trait method through method-call syntax (see noir_det.compile_candidate)
    elif imp.kind == "trait" or imp.trait:
        trait = imp.trait if imp.kind == "impl" else (imp.self_type + (
            "<" + ", ".join(assign[g.name] for g in imp.generics) + ">" if imp.generics else ""))
        trait = subst(trait, assign, alias)
        if qualify and not trait.startswith("std::") and imp.kind == "impl":
            trait = _qualify_trait(trait, qualify)
        callee = f"<{alias} as {trait}>::{fn.name}{turbo}"
    elif fn.params and fn.params[0].pattern == "self":
        # inherent method with a receiver: method-call syntax (auto-ref), dispatched on the value's type
        callee = None
    else:
        callee = f"{alias_expr}::{fn.name}{turbo}"
    if callee is None:
        call = f"s_self.{fn.name}{turbo}({', '.join(args[1:])})"
    else:
        call = f"{callee}({', '.join(args)})"
    ret = subst(fn.ret, assign, self_concrete).strip() if fn.ret else ""
    if imp is not None:
        # associated constants `Self::N` in types: `<Alias as Trait>::N` (trait impls) or `Alias::N`
        trait_txt = imp.trait if imp.kind == "impl" else imp.self_type
        assoc = (f"<{alias} as {subst(trait_txt, assign, alias)}>::" if (imp.trait or imp.kind == "trait")
                 else f"{alias}::")
        sc = re.escape(self_concrete)
        params = [re.sub(sc + r"::(?=[A-Z_][A-Za-z0-9_]*)", lambda m: assoc, x) for x in params]
        ret = re.sub(sc + r"::(?=[A-Z_][A-Za-z0-9_]*)", lambda m: assoc, ret)
    if qualify.startswith("std::"):
        ret = ret.replace("crate::", "std::")
        params = [x.replace("crate::", "std::") for x in params]
    outs = ([("r", ret)] if ret and ret != "()" else []) + mut_outs
    body = ["    " + x for x in pre]
    if ret and ret != "()":
        body.append(f"    let r = {call};")
    else:
        body.append(f"    {call};")
    if len(outs) == 1:
        body.append(f"    {outs[0][0]}")
        rtype = f" -> {outs[0][1]}"
    elif outs:
        body.append("    (" + ", ".join(o for o, _ in outs) + ")")
        rtype = " -> (" + ", ".join(ty for _, ty in outs) + ")"
    else:
        rtype = ""
    lines += ["#[export]", f"fn {name}({', '.join(params)}){rtype} {{", *body, "}"]
    text = fold_literals("\n".join(lines) + "\n")
    return Wrapper(name, text, fold_literals(call), [o for o, _ in outs], [o for o, _ in mut_outs], notes)


def fold_literals(text: str) -> str:
    """``4 + 1`` -> ``5`` for integer literals produced by substituting numeric generics (Noir checks the
    type of an arithmetic generic expression against the parameter's declared numeric type)."""
    pat = re.compile(r"(?<![A-Za-z0-9_.])(\d+)\s*([+*-])\s*(\d+)(?![A-Za-z0-9_.])")
    while True:
        m = pat.search(text)
        if not m:
            return text
        a, op, b = int(m.group(1)), m.group(2), int(m.group(3))
        v = a + b if op == "+" else a * b if op == "*" else a - b
        if v < 0:
            return text
        text = text[:m.start()] + str(v) + text[m.end():]


def _qualify_trait(trait: str, qualify: str) -> str:
    return trait


def std_module_path(rel_in_src: str, up: int = 0) -> str:
    """``std::a::b::`` for ``a/b.nr``; ``up`` drops trailing modules (public re-exports of items in private
    modules are found at an ancestor)."""
    parts = NS.mod_path(rel_in_src)
    parts = parts[:len(parts) - up] if up else parts
    return "std::" + "".join(p + "::" for p in parts)


def std_import_candidates(t: "Target", wr_text: str, rel_in_src: str, idx: "RepoIndex | None" = None) \
        -> dict[str, list[str]]:
    """Candidate public paths for every capitalized name the stdlib wrapper uses (Noir has no glob imports):
    the file's own ``use`` of the name (``crate::`` -> ``std::``), then the name in the file's module and in
    each ancestor module (public re-exports), then ``std::Name``; with ``idx``, then the module declaring the
    name in the stdlib and that module's ancestors."""
    mod = NS.mod_path(rel_in_src)
    file_uses: dict[str, list[str]] = {}
    for u in NS.uses(NS.blank_comments_strings(t.src, strings=False), top_level=True):
        for path in _expand_use(u, mod):
            file_uses.setdefault(path.split("::")[-1], []).append(path)
    names = []
    body = re.sub(r"type BooleSelf_\w+ =", "", wr_text)
    for m in re.finditer(r"(?<![A-Za-z0-9_:])([A-Z][A-Za-z0-9_]*)", body):
        n = m.group(1)
        if n in PRIMITIVES or n == "Self" or n.startswith("BooleSelf_") or n in names:
            continue
        names.append(n)
    out = {}
    for n in names:
        cands = list(dict.fromkeys(file_uses.get(n, []) + ["std::" + "".join(p + "::" for p in mod[:k]) + n
                                                            for k in range(len(mod), -1, -1)]))
        if idx is not None:
            for drel in idx.decl_files.get(n, []):
                if drel.startswith("noir_stdlib/src/"):
                    dmod = NS.mod_path(drel[len("noir_stdlib/src/"):])
                    cands += ["std::" + "".join(p + "::" for p in dmod[:k]) + n for k in range(len(dmod), 0, -1)]
            cands = list(dict.fromkeys(cands))
        out[n] = cands
    return out


def _expand_use(u: str, mod: list[str]) -> list[str]:
    """``a::{b, c::d}`` -> [a::b, a::c::d], with crate/super/self resolved to std paths."""
    u = u.strip()
    m = re.match(r"(.*?)\{(.*)\}\s*$", u)
    if m:
        prefix = m.group(1)
        out = []
        for part in NS.split_top(m.group(2)):
            if part.strip():
                out += _expand_use(prefix + part.strip(), mod)
        return out
    u = re.sub(r"\s+as\s+[A-Za-z_][A-Za-z0-9_]*$", "", u)
    if u.startswith("crate::"):
        return ["std::" + u[len("crate::"):]]
    if u.startswith("super::"):
        parent = mod[:-1]
        rest = u[len("super::"):]
        while rest.startswith("super::"):
            parent, rest = parent[:-1], rest[len("super::"):]
        return ["std::" + "".join(p + "::" for p in parent) + rest]
    if u.startswith("self::"):
        return ["std::" + "".join(p + "::" for p in mod) + u[len("self::"):]]
    if u.startswith("std::"):
        return [u]
    return []


def std_use_lines(text_src: str, rel_in_src: str) -> list[str]:
    """(Kept for reference: the file's top-level ``use`` declarations rewritten for a wrapper crate.)"""
    mod = NS.mod_path(rel_in_src)
    out = []
    for u in NS.uses(NS.blank_comments_strings(text_src, strings=False), top_level=True):
        out += [f"use {p};" for p in _expand_use(u, mod)]
    return out
