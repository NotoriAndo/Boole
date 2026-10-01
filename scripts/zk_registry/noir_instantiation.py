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
    for prm in fn.params:
        if re.search(r"(?<![A-Za-z0-9_])(fn\s*[\[(]|impl\s+Fn)", prm.type) or re.match(r"\s*\[[^;\]]+\]\s*$", prm.type):
            kind = "a function (closure) type" if "fn" in prm.type or "Fn" in prm.type else "a slice/vector type"
            return "NO-INSTANTIATION", (f"parameter `{prm.pattern}: {prm.type}` has {kind}, which is not an ABI type, "
                                        "so no circuit takes it as an input")
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
                    continue                      # test-only types are not instantiation candidates
                idx.structs[m.group(1)] = bool(m.group(2))
                idx.struct_files.setdefault(m.group(1), []).append(rel)
            test_file = is_test_path(rel)
            test_mods = [b for b in blocks if b.kind == "mod" and re.search(r"test", b.self_type)]
            for b in blocks:
                if test_file or any(m.start < b.start < m.end for m in test_mods):
                    continue                      # implementors in test code are not candidates
                if b.kind == "impl" and b.trait and not b.generics:
                    tm = re.match(r"\s*([A-Za-z_][A-Za-z0-9_:]*)", b.trait)
                    if tm:
                        idx.trait_impls.setdefault(tm.group(1).split("::")[-1], []).append(b.self_type)
        return idx

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


def candidates(t: Target, idx: RepoIndex | None) -> list[tuple[str, dict, list[str]]]:
    """Candidate assignments (tier, {generic: value}, provenance) in tier order, at most
    :data:`MAX_CANDIDATES_PER_TIER` per tier."""
    gens = t.generics
    if not gens:
        return [("parameter-free", {}, ["no generic parameters"])]
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
    for g in types:
        bs = [b for b in g.bounds + bounds.get(g.name, []) if b]
        type_values.append(implementors(idx, bs, visible_in(t, idx)) if bs and idx is not None else list(PROBE_TYPES))
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
        out.append(("probed", a, ["probed: " + ", ".join(f"{k}={v}" for k, v in a.items())]))
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


def visible_in(t: Target, idx: RepoIndex):
    """Predicate: a struct name the target's crate can name (declared outside test code in the crate or a
    path dependency)."""
    scope = crate_scope(t)

    def ok(name: str) -> bool:
        return any(f == s or f.startswith(s + "/") for f in idx.struct_files.get(name, []) for s in scope)
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


def implementors(idx: RepoIndex, bounds: list[str], visible=None) -> list[str]:
    """Non-generic types the repository implements every bound trait for (primitives first); ``visible``
    filters out types the target's crate cannot name (declared in other crates or in test code)."""
    sets = []
    for b in bounds:
        name = re.match(r"\s*([A-Za-z_][A-Za-z0-9_:]*)", b)
        if not name:
            continue
        impls = idx.trait_impls.get(name.group(1).split("::")[-1], [])
        sets.append({x.strip() for x in impls if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", x.strip())})
    if not sets:
        return list(PROBE_TYPES)
    common = set.intersection(*sets)
    if visible is not None:
        common = {x for x in common if x in PRIMITIVES or visible(x)}
    return sorted(common, key=lambda x: (x not in PRIMITIVES, PROBE_TYPES.index(x) if x in PROBE_TYPES else 99, x))[:4]


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


def wrapper(t: Target, assign: dict, wid: str, qualify: str = "", method_call: bool = False) -> Wrapper:
    """The ``#[export]`` wrapper for ``t`` under ``assign``; ``qualify`` prefixes free functions and is the
    module path for stdlib wrappers (``std::hash::``)."""
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
    params, pre, args, mut_outs = [], [], [], []
    for i, p in enumerate(fn.params):
        if p.pattern == "self":
            ty = p.type
            if ty in ("Self", "&Self", "&mut Self"):
                base = self_concrete
            else:
                base = subst(re.sub(r"^&\s*(mut\s+)?", "", ty), assign, self_concrete)
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
    return Wrapper(name, text, fold_literals(call), [o for o, _ in outs], [o for o, _ in mut_outs])


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


def std_import_candidates(t: "Target", wr_text: str, rel_in_src: str) -> dict[str, list[str]]:
    """Candidate public paths for every capitalized name the stdlib wrapper uses (Noir has no glob imports):
    the file's own ``use`` of the name (``crate::`` -> ``std::``), then the name in the file's module and in
    each ancestor module (public re-exports), then ``std::Name``."""
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
