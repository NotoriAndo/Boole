"""Instantiation of gnark gadgets: wrapper circuits generated from the type catalog.

The catalog (``gnark_tool/catalog``, go/packages + go/types over the repository at its pin) gives
every target's signature as type trees, the named types it reaches, the exported functions of the
loaded packages, the concrete instantiations of generic types and functions written in the
repository (test and non-test files) and the call sites of the target packages' functions.

A wrapper is a circuit struct whose secret fields are the target's circuit inputs (parameters of
circuit-variable types: ``frontend.Variable``, emulated elements, curve points, field extension
elements, byte values, and arrays / slices / pointers of those; a data receiver), whose ``Define``
constructs the gadget object (receiver or object parameters) with the package's in-circuit
constructor, calls the target and passes pointers to its results (and to a data receiver taken by
pointer, which the call may update) to ``harness.Expose``.

Choices, by tier (first tier with a compiling candidate wins; within it the largest compiled
instantiation within the size policy): ``parameter-free`` (no type parameter, constant or slice
length to choose) -> ``repo-test`` (type arguments of concrete instantiations, constant arguments and
interface implementations of call sites written in test files) -> ``repo-derived`` (the same in
non-test files) -> ``probed`` (slice lengths 2 and 4, constants 2 and 8 / false and true; labelled,
a counterexample there is not a finding).  The native field is the curve the package's tests compile
with (BN254 when they name it or name none).
"""
from __future__ import annotations

import copy
import hashlib
import json
import os
import re
from dataclasses import dataclass, field

FRONTEND = "github.com/consensys/gnark/frontend"
EMULATED = "github.com/consensys/gnark/std/math/emulated"
HARNESS = "boole.local/gnarkx/harness"
ECC = "github.com/consensys/gnark-crypto/ecc"

# curves gnark's R1CS builder supports at the pin (frontend/cs/r1cs newBuilder), by ecc ID name
CURVES = {"BN254": "bn254", "BLS12_377": "bls12_377", "BLS12_381": "bls12_381", "BW6_761": "bw6_761",
          "GRUMPKIN": "grumpkin"}
CURVE_PREFERENCE = ["BN254", "BLS12_377", "BLS12_381", "BW6_761", "GRUMPKIN"]
PROBED_LENGTHS = (2, 4)
PROBED_INTS = ("2", "8")
PROBED_BOOLS = ("false", "true")
PROBED_BYTES = '[]byte("BOOLE-DET-PROBE")'
MAX_CANDIDATES = 8
MAX_TYPE_CHOICES = 4


class NoInstantiation(Exception):
    """The target has no instantiation (reason)."""


class NotApplicable(Exception):
    """DET does not apply to the target (reason)."""


# ------------------------------------------------------------------------------------------ catalog

class Catalog:
    def __init__(self, data: dict, root: str):
        self.data = data
        self.root = root
        self.named: dict = data.get("named") or {}
        self.funcs: list = data.get("funcs") or []
        self.by_pkg: dict[str, list] = {}
        for f in self.funcs:
            self.by_pkg.setdefault(f["pkg"], []).append(f)
        self.instances: dict[str, list] = {}
        for i in data.get("instances") or []:
            self.instances.setdefault(i["generic"], []).append(i)
        self.calls: dict[str, list] = {}
        for c in data.get("calls") or []:
            self.calls.setdefault(c["target"], []).append(c)
        self.targets = {(t["path"], t["symbol"]): t for t in data.get("targets") or []}
        self._curves: dict[str, list] = {}

    @classmethod
    def load(cls, path: str, root: str) -> "Catalog":
        with open(path, encoding="utf-8") as f:
            return cls(json.load(f), root)

    def named_info(self, t: dict) -> dict | None:
        return self.named.get(f"{t.get('pkg')}.{t.get('name')}")

    def test_curves(self, pkg_dir: str) -> list[tuple[str, list[str]]]:
        """Curves named by the package's test files (ecc.<ID>), most frequent first, with the files."""
        if pkg_dir in self._curves:
            return self._curves[pkg_dir]
        counts: dict[str, int] = {}
        files: dict[str, list] = {}
        d = os.path.join(self.root, pkg_dir)
        for fn in sorted(os.listdir(d)) if os.path.isdir(d) else []:
            if not fn.endswith("_test.go"):
                continue
            with open(os.path.join(d, fn), encoding="utf-8", errors="replace") as f:
                text = f.read()
            for m in re.finditer(r"\becc\.(BN254|BLS12_377|BLS12_381|BW6_761|GRUMPKIN|BLS24_315|BW6_633)\b", text):
                counts[m.group(1)] = counts.get(m.group(1), 0) + 1
                files.setdefault(m.group(1), [])
                if fn not in files[m.group(1)]:
                    files[m.group(1)].append(fn)
        out = sorted(counts, key=lambda c: (-counts[c], CURVE_PREFERENCE.index(c) if c in CURVE_PREFERENCE else 99))
        res = [(c, files[c]) for c in out]
        self._curves[pkg_dir] = res
        return res


def choose_curve(cat: Catalog, pkg_dir: str) -> tuple[str, str]:
    """(ecc ID name, provenance)."""
    named = cat.test_curves(pkg_dir)
    supported = [(c, fs) for c, fs in named if c in CURVES]
    if any(c == "BN254" for c, _ in supported) or not supported:
        fs = next((fs for c, fs in supported if c == "BN254"), [])
        why = (f"BN254: named by the package tests ({', '.join(fs[:3])})" if fs else
               "BN254: default (the package tests name no curve supported by gnark's R1CS builder)")
        return "BN254", why
    c, fs = supported[0]
    return c, f"{c}: the curve the package tests compile with ({', '.join(fs[:3])}; BN254 not named)"


# ------------------------------------------------------------------------------------------ types

def is_named(t: dict | None, pkg: str, name: str) -> bool:
    return bool(t) and t.get("k") == "named" and t.get("pkg") == pkg and t.get("name") == name


def is_api(t) -> bool:
    return is_named(t, FRONTEND, "API")


def is_variable(t) -> bool:
    return is_named(t, FRONTEND, "Variable")


def is_error(t) -> bool:
    return bool(t) and t.get("k") == "named" and not t.get("pkg") and t.get("name") == "error"


def is_element(t) -> bool:
    return is_named(t, EMULATED, "Element")


def is_option(t) -> bool:
    """A variadic option list (``...SomeOption``): omitted (defaults)."""
    if not t or t.get("k") != "slice":
        return False
    e = t["elem"]
    return e.get("k") == "named" and re.search(r"(Option|Opt)$", e.get("name", "")) is not None


def has_tparam(t) -> bool:
    if not isinstance(t, dict):
        return False
    if t.get("k") == "tparam":
        return True
    return any(has_tparam(a) for a in t.get("args") or []) or has_tparam(t.get("elem"))


def subst(t: dict, env: dict) -> dict:
    if t is None:
        return None
    if t.get("k") == "tparam":
        if t["name"] not in env:
            return t
        return copy.deepcopy(env[t["name"]])
    out = dict(t)
    if t.get("args"):
        out["args"] = [subst(a, env) for a in t["args"]]
    if t.get("elem"):
        out["elem"] = subst(t["elem"], env)
    return out


def key_of(t: dict) -> str:
    return json.dumps(t, sort_keys=True, separators=(",", ":"))


def show(t: dict) -> str:
    """Short human-readable form (package names, not paths)."""
    k = t.get("k")
    if k == "named":
        base = (t["pkg"].rsplit("/", 1)[-1] + "." if t.get("pkg") else "") + t["name"]
        return base + ("[" + ", ".join(show(a) for a in t["args"]) + "]" if t.get("args") else "")
    if k == "ptr":
        return "*" + show(t["elem"])
    if k == "slice":
        return "[]" + show(t["elem"])
    if k == "array":
        return f"[{t['len']}]" + show(t["elem"])
    if k in ("basic", "tparam"):
        return t["name"]
    return t.get("str", k)


def underlying(cat: Catalog, t: dict) -> dict | None:
    info = cat.named_info(t) if t.get("k") == "named" else None
    if info is None:
        return None
    u = info["underlying"]
    env = {tp["name"]: a for tp, a in zip(info.get("tparams") or [], t.get("args") or [])}
    return subst(u, env)


def field_types(cat: Catalog, t: dict) -> list[tuple[str, dict, str]] | None:
    """Exported fields (name, type with the named type's arguments substituted, tag) of a named struct
    type, or None for other types."""
    info = cat.named_info(t) if t.get("k") == "named" else None
    if info is None or info["underlying"].get("k") != "struct":
        return None
    env = {tp["name"]: a for tp, a in zip(info.get("tparams") or [], t.get("args") or [])}
    return [(f["name"], subst(f["type"], env), f.get("tag", "")) for f in info.get("fields") or []
            if f["name"][:1].isupper() or f.get("embedded")]


def carries_variables(cat: Catalog, t: dict, depth: int = 0) -> bool:
    """The type holds circuit variables in exported positions (a result worth exposing)."""
    if depth > 12 or t is None:
        return False
    k = t.get("k")
    if is_variable(t) or is_element(t) or k == "tparam":
        return True
    if k in ("ptr", "slice", "array"):
        return carries_variables(cat, t["elem"], depth + 1)
    if k == "named":
        fs = field_types(cat, t)
        if fs is not None:
            return any(carries_variables(cat, ft, depth + 1) for _, ft, tag in fs if '"-"' not in tag)
        u = underlying(cat, t)
        return u is not None and u.get("k") in ("array", "slice") and carries_variables(cat, u, depth + 1)
    return False


def alloc(cat: Catalog, t: dict, depth: int = 0) -> tuple[str | None, bool]:
    """(reason or None, needs slice lengths): can a struct field of type ``t`` be allocated by gnark's
    schema and assigned by the sampler (variables, emulated elements, named structs / arrays / slices of
    those; variable-carrying slices get a length)."""
    if depth > 12:
        return "type nesting too deep", False
    if t is None:
        return "unknown type", False
    if has_tparam(t):
        return f"unresolved type parameter in {show(t)}", False
    k = t.get("k")
    if is_variable(t) or is_element(t):
        return None, False
    if k in ("array", "slice"):
        r, sh = alloc(cat, t["elem"], depth + 1)
        return r, sh or k == "slice"
    if k == "named":
        fs = field_types(cat, t)
        if fs is None:
            u = underlying(cat, t)
            if u is not None and u.get("k") in ("array", "slice"):
                return alloc(cat, u, depth + 1)
            return f"{show(t)} is not a struct of circuit variables", False
        n_var, shape = 0, False
        for name, ft, tag in fs:
            if '"-"' in tag or ft.get("k") in ("ptr", "basic"):
                continue                    # pointer fields stay nil (precomputed data); Go values
            if not carries_variables(cat, ft, depth + 1):
                continue
            if ft.get("k") in ("iface", "func", "map", "chan"):
                return f"{show(t)}.{name} cannot be allocated", False
            r, sh = alloc(cat, ft, depth + 1)
            if r is not None:
                return r, False
            n_var += 1
            shape = shape or sh
        if n_var == 0:
            return f"{show(t)} has no circuit variables", False
        return None, shape
    return f"{show(t)} is not a circuit-variable type", False


def constructors(cat: Catalog, t: dict) -> list[dict]:
    """In-circuit constructors of the gadget object type ``t``: functions of its package that take a
    frontend.API and return ``t`` or ``*t`` first; fewest parameters first."""
    out = []
    for f in cat.by_pkg.get(t.get("pkg"), []):
        res = f["sig"]["results"]
        if not res:
            continue
        r = res[0]["type"]
        r = r["elem"] if r.get("k") == "ptr" else r
        if r.get("k") != "named" or r.get("pkg") != t["pkg"] or r.get("name") != t["name"]:
            continue
        if not any(is_api(p["type"]) for p in f["sig"]["params"]):
            continue
        out.append(f)
    out.sort(key=lambda f: (sum(1 for p in f["sig"]["params"] if not is_api(p["type"]) and not is_option(p["type"])),
                            f["name"]))
    return out


# ------------------------------------------------------------------------------------------ Go rendering

class GoFile:
    def __init__(self):
        self.imports: dict[str, str] = {FRONTEND: "frontend", HARNESS: "harness"}
        self.used: set[str] = {FRONTEND, HARNESS}

    def alias(self, pkg: str) -> str:
        self.used.add(pkg)
        if pkg in self.imports:
            return self.imports[pkg]
        if "." not in pkg.split("/")[0]:                 # standard library: its own name
            name = pkg.rsplit("/", 1)[-1]
            self.imports[pkg] = name
            return name
        base = "p_" + re.sub(r"[^A-Za-z0-9]", "_", pkg.rsplit("/", 1)[-1])
        name, k = base, 2
        while name in self.imports.values():
            name, k = f"{base}{k}", k + 1
        self.imports[pkg] = name
        return name

    def typ(self, t: dict) -> str:
        k = t.get("k")
        if k == "named":
            if not t.get("pkg"):
                return t["name"]
            if not t["name"][:1].isupper():
                raise NoInstantiation(f"type {show(t)} is unexported")
            s = f"{self.alias(t['pkg'])}.{t['name']}"
            if t.get("args"):
                s += "[" + ", ".join(self.typ(a) for a in t["args"]) + "]"
            return s
        if k == "ptr":
            return "*" + self.typ(t["elem"])
        if k == "slice":
            return "[]" + self.typ(t["elem"])
        if k == "array":
            return f"[{t['len']}]" + self.typ(t["elem"])
        if k == "basic":
            return t["name"]
        if k == "tparam":
            raise NoInstantiation(f"type parameter {t['name']} not instantiated")
        raise NoInstantiation(f"type {show(t)} cannot be written in a wrapper")

    def header(self) -> str:
        lines = ["// Code generated by scripts/zk_registry/gnark_instantiation.py; DO NOT EDIT.", "",
                 "package wrappers", "", "import ("]
        for pkg in sorted(self.used):
            a = self.imports[pkg]
            lines.append(f'\t"{pkg}"' if a == pkg.rsplit("/", 1)[-1] else f'\t{a} "{pkg}"')
        lines.append(")")
        return "\n".join(lines) + "\n"


def shape_expr(cat: Catalog, g: GoFile, t: dict, length: int, depth: int = 0) -> str | None:
    """A Go expression for a zero value of ``t`` whose variable-carrying slices have ``length``
    elements (recursively), or None when ``t`` needs no initialisation."""
    if depth > 12:
        raise NoInstantiation("shape nesting too deep")
    k = t.get("k")
    if is_variable(t) or is_element(t) or not carries_variables(cat, t):
        return None
    if k == "slice":
        inner = shape_expr(cat, g, t["elem"], length, depth + 1)
        ty = g.typ(t)
        if inner is None:
            return f"make({ty}, {length})"
        return (f"func() {ty} {{ v := make({ty}, {length}); for i := range v {{ v[i] = {inner} }}; "
                f"return v }}()")
    if k == "array":
        inner = shape_expr(cat, g, t["elem"], length, depth + 1)
        if inner is None:
            return None
        ty = g.typ(t)
        return f"func() (v {ty}) {{ for i := range v {{ v[i] = {inner} }}; return v }}()"
    if k == "named":
        fs = field_types(cat, t)
        if fs is None:
            u = underlying(cat, t)
            if u is not None and u.get("k") in ("slice", "array"):
                inner = shape_expr(cat, g, u, length, depth + 1)
                return None if inner is None else f"{g.typ(t)}({inner})"
            return None
        parts = []
        for name, ft, tag in fs:
            if '"-"' in tag or ft.get("k") in ("ptr", "basic") or not carries_variables(cat, ft):
                continue
            inner = shape_expr(cat, g, ft, length, depth + 1)
            if inner is not None:
                parts.append(f"{name}: {inner}")
        if not parts:
            return None
        return f"{g.typ(t)}{{{', '.join(parts)}}}"
    return None


# ------------------------------------------------------------------------------------------ plans

@dataclass
class Choice:
    tier: str
    env: dict = field(default_factory=dict)            # type parameter -> type tree
    consts: dict = field(default_factory=dict)         # parameter index -> Go literal
    length: int = 0                                    # slice length (0: no slice to size)
    iface: dict = field(default_factory=dict)          # parameter index -> (origin key, origin args)
    provenance: list = field(default_factory=list)

    def label(self) -> str:
        parts = [f"{k}={show(v)}" for k, v in sorted(self.env.items())]
        parts += [f"arg{k}={v}" for k, v in sorted(self.consts.items())]
        parts += [f"slice length {self.length}"] if self.length else []
        parts += [f"arg{k}<-{v[0].rsplit('/', 1)[-1]}" for k, v in sorted(self.iface.items())]
        return ", ".join(parts)


@dataclass
class Wrapper:
    id: str
    type_name: str
    source: str
    curve: str
    choice: Choice
    call: str                                 # the target call as written
    inputs: list[str]
    probed_length: bool = False


@dataclass
class Plan:
    kind: str                                 # gadget | circuit
    decl: dict
    params: list[tuple[str, str, dict]]       # (role before instantiation, name, declared type)
    tparams: list[str]
    curve: str
    curve_why: str
    choices: list[Choice]
    doc: str = ""
    needs_length: bool = False


TIERS = ["parameter-free", "repo-test", "repo-derived", "probed"]


def role(cat: Catalog, t: dict) -> str:
    """Role of a parameter type: api, omit (variadic options), const, bigint, bytes, iface, object,
    input, or reject.  With type parameters inside, variable-carrying types count as inputs."""
    if is_api(t):
        return "api"
    if is_option(t):
        return "omit"
    k = t.get("k")
    if k in ("slice", "array"):
        e = t["elem"]
        e2 = e["elem"] if e.get("k") == "ptr" else e
        if k == "slice" and e.get("k") == "basic" and e.get("name") in ("byte", "uint8"):
            return "bytes"
        if e2.get("k") == "slice":
            e3 = e2["elem"]["elem"] if e2["elem"].get("k") == "ptr" else e2["elem"]
            return "input" if carries_variables(cat, e3) else "reject"
        return "input" if carries_variables(cat, e2) else "reject"
    base = t["elem"] if k == "ptr" else t
    if is_variable(base) or is_element(base) or base.get("k") == "tparam":
        return "input"
    if base.get("k") == "basic":
        return "const" if k != "ptr" else "reject"
    if is_named(base, "math/big", "Int"):
        return "bigint"
    if base.get("k") == "named":
        info = cat.named_info(base) or {}
        uk = (info.get("underlying") or {}).get("k")
        if uk == "basic":
            return "const"
        if uk == "iface":
            return "iface"
        if uk == "func":
            return "reject"
        if carries_variables(cat, base):
            return "input"
        if constructors(cat, base):
            return "object"
    return "reject"


def _reject_reason(cat: Catalog, t: dict) -> str:
    k = t.get("k")
    info = cat.named_info(t) if k == "named" else None
    if k == "func" or (info and info["underlying"].get("k") == "func"):
        return "function type"
    if k == "iface":
        return "interface type " + t.get("str", "")
    return show(t)


def plan_target(cat: Catalog, tgt: dict) -> Plan:
    decl = tgt["decl"]
    if decl.get("pkg_name") == "main":
        raise NoInstantiation("declared in package main (a program, not an importable package)")
    pkg_dir = os.path.dirname(decl["file"])
    curve, curve_why = choose_curve(cat, pkg_dir)
    recv = decl.get("recv")
    if decl["kind"] == "type" or (decl["name"] == "Define" and recv is not None):
        tdecl = decl if decl["kind"] == "type" else None
        if tdecl is not None and not tdecl.get("has_define"):
            raise NotApplicable(f"type {decl['name']} has no Define method")
        rt = {"k": "named", "pkg": decl["pkg"], "name": decl["name"] if tdecl else recv["name"]}
        tps = [a["name"] for a in (recv or {}).get("args") or [] if a.get("k") == "tparam"]
        if tps or (cat.named_info(rt) or {}).get("tparams"):
            raise NoInstantiation("generic circuit type")
        r, sh = alloc(cat, rt)
        if r is not None and "no circuit variables" not in r:
            raise NoInstantiation(f"circuit type cannot be allocated: {r}")
        choices = [Choice("probed", length=L, provenance=[f"probed slice length {L}"]) for L in PROBED_LENGTHS] \
            if sh else [Choice("parameter-free")]
        return Plan("circuit", decl, [], [], curve, curve_why, choices, decl.get("doc", ""), sh)
    sig = decl["sig"]
    fn_tps = [tp["name"] for tp in sig.get("tparams") or []]
    recv_tps = [a["name"] for a in (recv or {}).get("args") or [] if a.get("k") == "tparam"]
    params = [(role(cat, p["type"]), p["name"] or f"p{i}", p["type"]) for i, p in enumerate(sig["params"])]
    has_input = any(r == "input" for r, _, _ in params)
    recv_data = recv is not None and carries_variables(cat, recv)
    has_output = any(carries_variables(cat, r["type"]) for r in sig["results"]) or (recv_data and decl.get("recv_ptr"))
    if not has_input and not has_output and not recv_data:
        raise NotApplicable("constructor or configuration function: no circuit-variable inputs or outputs "
                            f"(results: {', '.join(show(r['type']) for r in sig['results']) or 'none'})")
    for (r, name, t) in params:
        if r == "reject":
            raise NoInstantiation(f"parameter {name} of {_reject_reason(cat, t)} cannot be supplied by a wrapper")
    choices = enumerate_choices(cat, decl, params, recv_tps, fn_tps)
    if not choices:
        raise NoInstantiation("no candidate instantiation (type parameters without concrete instantiations in the "
                              "repository, or interface parameters without a provider at a call site)")
    return Plan("gadget", decl, params, recv_tps + fn_tps, curve, curve_why, choices, decl.get("doc", ""))


def _key_fn(decl: dict) -> str:
    return f"{decl['pkg']}.{decl['recv']['name']}.{decl['name']}" if decl.get("recv") else f"{decl['pkg']}.{decl['name']}"


def _instances_for(cat: Catalog, decl: dict, recv_tps: list[str], fn_tps: list[str]) -> list[tuple[str, dict, str]]:
    """(tier, env, provenance) from concrete instantiations of the receiver type, its constructors and
    the function itself, and from receivers / type arguments at call sites."""
    key_fn = _key_fn(decl)
    out = []
    if recv_tps:
        recv = decl["recv"]
        for inst in cat.instances.get(f"{recv['pkg']}.{recv['name']}", []):
            if len(inst["args"]) == len(recv_tps):
                out.append(("repo-test" if inst["test"] else "repo-derived", dict(zip(recv_tps, inst["args"])),
                            f"{inst['file']}:{inst['line']} instantiates {recv['name']}"))
        for cons in constructors(cat, recv):
            r = cons["sig"]["results"][0]["type"]
            r = r["elem"] if r.get("k") == "ptr" else r
            ctps = [tp["name"] for tp in cons["sig"].get("tparams") or []]
            pos = {}
            for j, a in enumerate(r.get("args") or []):
                if a.get("k") == "tparam" and a["name"] in ctps:
                    pos[ctps.index(a["name"])] = j
            for inst in cat.instances.get(f"{cons['pkg']}.{cons['name']}", []):
                env = {recv_tps[j]: inst["args"][ci] for ci, j in pos.items()
                       if ci < len(inst["args"]) and j < len(recv_tps)}
                if len(env) == len(recv_tps):
                    out.append(("repo-test" if inst["test"] else "repo-derived", env,
                                f"{inst['file']}:{inst['line']} instantiates {cons['name']}"))
        for call in cat.calls.get(key_fn, []):
            rt = call.get("recv")
            if rt and rt.get("k") == "ptr":
                rt = rt["elem"]
            if rt and rt.get("args") and len(rt["args"]) == len(recv_tps) and not any(has_tparam(a) for a in rt["args"]):
                out.append(("repo-test" if call["test"] else "repo-derived", dict(zip(recv_tps, rt["args"])),
                            f"{call['file']}:{call['line']} calls {decl['name']}"))
    if fn_tps:
        sources = [(i["test"], i["args"], f"{i['file']}:{i['line']} instantiates {decl['name']}")
                   for i in cat.instances.get(key_fn, [])]
        sources += [(c["test"], c["targs"], f"{c['file']}:{c['line']} calls {decl['name']}")
                    for c in cat.calls.get(key_fn, []) if c.get("targs")]
        merged = []
        for test, args, prov in sources:
            if len(args) != len(fn_tps) or any(has_tparam(a) for a in args):
                continue
            env = dict(zip(fn_tps, args))
            tier = "repo-test" if test else "repo-derived"
            if recv_tps:
                for rtier, renv, rprov in out:
                    merged.append((max(tier, rtier, key=TIERS.index), {**renv, **env}, rprov + "; " + prov))
            else:
                merged.append((tier, env, prov))
        out = merged
    return out


def enumerate_choices(cat: Catalog, decl: dict, params, recv_tps, fn_tps) -> list[Choice]:
    key_fn = _key_fn(decl)
    envs: list[tuple[str, dict, list]] = []
    if recv_tps or fn_tps:
        seen: dict = {}
        for tier, env, prov in _instances_for(cat, decl, recv_tps, fn_tps):
            k = (tier, key_of(env))
            if k not in seen:
                seen[k] = (tier, env, [prov])
            elif len(seen[k][2]) < 3:
                seen[k][2].append(prov)
        groups: dict[str, list] = {}
        for (tier, _), v in seen.items():
            groups.setdefault(tier, []).append(v)
        for tier in ("repo-test", "repo-derived"):
            envs += sorted(groups.get(tier, []), key=lambda v: (-len(v[2]), key_of(v[1])))[:MAX_TYPE_CHOICES]
        if not envs:
            return []
    else:
        envs = [("parameter-free", {}, [])]
    calls = sorted(cat.calls.get(key_fn, []), key=lambda c: (not c["test"], c["file"], c["line"]))
    out: list[Choice] = []
    for tier0, env, prov in envs:
        sub = [(r, n, subst(t, env)) for r, n, t in params]
        const_idx = [i for i, (r, _, _) in enumerate(sub) if r in ("const", "bigint", "bytes")]
        iface_idx = [i for i, (r, _, _) in enumerate(sub) if r == "iface"]
        needs_len = any(r == "input" and alloc(cat, t)[1] for r, _, t in sub if not has_tparam(t)) or \
            bool(decl.get("recv") and alloc(cat, subst(decl["recv"], env))[1] if carries_variables(cat, decl.get("recv") or {}) else False)
        const_sets = []
        for c in calls:
            args = c.get("args") or []
            vals = {}
            for i in const_idx:
                if i < len(args):
                    v = arg_literal(sub[i], args[i])
                    if v is not None:
                        vals[i] = v
            if len(vals) == len(const_idx):
                const_sets.append(("repo-test" if c["test"] else "repo-derived", vals, f"{c['file']}:{c['line']}"))
        iface_sets = []
        for c in calls:
            args = c.get("args") or []
            vals = {i: (args[i]["origin"], args[i].get("origin_args") or []) for i in iface_idx
                    if i < len(args) and args[i].get("origin")}
            if len(vals) == len(iface_idx):
                iface_sets.append(("repo-test" if c["test"] else "repo-derived", vals, f"{c['file']}:{c['line']}"))
        if iface_idx and not iface_sets:
            continue
        cs_list = _dedupe(const_sets)[:2] if const_idx else [(None, {}, "")]
        if const_idx and not cs_list:
            cs_list = [("probed", dict(zip(const_idx, combo)), "probed constants")
                       for combo in _probe_consts([sub[i] for i in const_idx])]
        is_list = _dedupe(iface_sets)[:1] if iface_idx else [(None, {}, "")]
        lens = [("probed", L, f"probed slice length {L}") for L in PROBED_LENGTHS] if needs_len else [(None, 0, "")]
        for ct, cv, cp in cs_list:
            for it, iv, ip in is_list:
                for lt, lv, lp in lens:
                    tiers = [t for t in (tier0, ct, it, lt) if t]
                    tier = max(tiers, key=TIERS.index)
                    if tier == "parameter-free" and (cv or iv):
                        tier = max(ct or "repo-test", it or "repo-test", key=TIERS.index)
                    out.append(Choice(tier, env, cv, lv, iv, list(prov) + [x for x in (cp, ip, lp) if x]))
    order = {t: i for i, t in enumerate(TIERS)}
    out.sort(key=lambda c: order[c.tier])
    return out[:MAX_CANDIDATES]


def arg_literal(param: tuple, arg: dict) -> str | None:
    r, _, t = param
    if r == "bigint":
        m = re.fullmatch(r"big\.NewInt\((-?\d+)\)", arg.get("expr") or "")
        if m:
            return f"big.NewInt({m.group(1)})"
        if arg.get("const") is not None and re.fullmatch(r"-?\d+", arg["const"]):
            return f"big.NewInt({arg['const']})"
        return None
    if r == "bytes":
        m = re.fullmatch(r'\[\]byte\(("[^"\\]*")\)', arg.get("expr") or "")
        return f"[]byte({m.group(1)})" if m else None
    if arg.get("const") is None:
        return None
    return go_const(t, arg["const"])


def _dedupe(sets):
    seen, out = set(), []
    for tier, vals, prov in sorted(sets, key=lambda s: TIERS.index(s[0])):
        k = json.dumps({str(a): b for a, b in vals.items()}, sort_keys=True)
        if k in seen:
            continue
        seen.add(k)
        out.append((tier, vals, prov))
    return out


def _probe_consts(items):
    opts = []
    for r, _, t in items:
        if r == "bytes":
            opts.append([PROBED_BYTES])
        elif r == "bigint":
            opts.append(["big.NewInt(1 << 16)"])
        elif t.get("k") == "basic" and t["name"] == "bool":
            opts.append(list(PROBED_BOOLS))
        elif t.get("k") == "basic" and t["name"] == "string":
            opts.append(['"BOOLE"'])
        else:
            opts.append(list(PROBED_INTS))
    combos = [[]]
    for o in opts:
        combos = [c + [x] for c in combos for x in o]
    return combos[:4]


def go_const(t: dict, exact: str) -> str | None:
    """Go literal for a constant argument (``exact`` from go/constant ExactString)."""
    if t.get("k") == "basic" and t["name"] == "bool":
        return exact if exact in ("true", "false") else None
    if t.get("k") == "basic" and t["name"] == "string":
        return exact if exact.startswith('"') else None
    if re.fullmatch(r"-?\d+", exact):
        return exact
    return None


# ------------------------------------------------------------------------------------------ wrapper source

def wrapper_id(item_id: str, k: int) -> str:
    return "w" + hashlib.sha256(item_id.encode()).hexdigest()[:12] + f"_{k}"


class _Ctx:
    def __init__(self, cat: Catalog, g: GoFile, choice: Choice):
        self.cat, self.g, self.choice = cat, g, choice
        self.fields: list[tuple[str, str, bool]] = []
        self.shapes: list[str] = []
        self.inputs: list[str] = []
        self.probed_length = False

    def add_input(self, fname: str, t: dict, note: str = "", tag: bool = True) -> None:
        """A wrapper input field holding a value of ``t`` (no pointers); ``tag``: secret visibility (circuit
        types keep the visibilities of their own fields)."""
        r, sh = alloc(self.cat, t)
        if r is not None and not (not tag and "no circuit variables" in r):
            raise NoInstantiation(f"{fname}: {r}")
        self.fields.append((fname, self.g.typ(t), tag))
        self.inputs.append(f"{fname} {show(t)}{note}")
        if sh:
            length = self.choice.length
            if not length:                       # e.g. a constructor taking a slice of variables
                length, self.probed_length = PROBED_LENGTHS[0], True
            s = shape_expr(self.cat, self.g, t, length)
            if s is not None:
                self.shapes.append(f"{fname}: {s}")


def render(cat: Catalog, plan: Plan, choice: Choice, wid: str) -> Wrapper:
    g = GoFile()
    ctx = _Ctx(cat, g, choice)
    tname = "W" + wid[1:]
    decl = plan.decl
    env = choice.env
    body: list[str] = []
    recv = decl.get("recv")
    if plan.kind == "circuit":
        rt = {"k": "named", "pkg": decl["pkg"], "name": decl["name"] if decl["kind"] == "type" else recv["name"]}
        ctx.add_input("In0", rt, " (circuit)", tag=False)
        body.append("\tif err := c.In0.Define(api); err != nil {\n\t\treturn err\n\t}")
        body.append("\treturn harness.Expose(api)")
        call = f"({show(rt)}).Define(api)"
    else:
        sig = decl["sig"]
        recv_expr, recv_expose = None, []
        if recv is not None:
            rt = subst(recv, env)
            if carries_variables(cat, rt):
                ctx.add_input("Recv", rt, " (receiver)")
                body.append("\trecv := c.Recv")
                recv_expr = "recv"
                if decl.get("recv_ptr"):
                    recv_expose.append("&recv")
            else:
                lines, recv_expr, _ = construct(ctx, rt, env, "recv", 0)
                body += lines
        call_args = []
        for i, (_, pname, t0) in enumerate(plan.params):
            t = subst(t0, env)
            r = role(cat, t)
            last = i == len(plan.params) - 1 and sig.get("variadic")
            if r == "api":
                call_args.append("api")
            elif r == "omit":
                continue
            elif r in ("const", "bigint", "bytes"):
                lit = choice.consts.get(i)
                if lit is None:
                    raise NoInstantiation(f"no value for constant parameter {pname}")
                if r == "bigint":
                    g.alias("math/big")
                elif t.get("k") == "named":
                    lit = f"{g.typ(t)}({lit})"
                call_args.append(lit + ("..." if last and r == "bytes" else ""))
            elif r == "iface":
                origin, oargs = choice.iface[i]
                lines, expr = construct_origin(ctx, origin, oargs, f"o{i}")
                body += lines
                call_args.append(expr)
            elif r == "object":
                base = t["elem"] if t.get("k") == "ptr" else t
                lines, expr, is_ptr = construct(ctx, base, env, f"o{i}", 1)
                body += lines
                want = t.get("k") == "ptr"
                call_args.append(expr if want == is_ptr else ("&" + expr if want else "*" + expr))
            elif r == "input":
                expr, pre = input_param(ctx, t, f"In{i}")
                body += pre
                call_args.append(expr + ("..." if last else ""))
            else:
                raise NoInstantiation(f"parameter {pname} of {_reject_reason(cat, t)} cannot be supplied by a wrapper")
        names, expose = [], []
        for k, res in enumerate(sig["results"]):
            if is_error(res["type"]):
                names.append("err")
            else:
                names.append(f"r{k}")
                expose.append(f"&r{k}")
        expose += recv_expose
        target = f"{recv_expr}.{decl['name']}" if recv is not None else f"{g.alias(decl['pkg'])}.{decl['name']}"
        fn_tps = [tp["name"] for tp in sig.get("tparams") or []]
        if recv is None and fn_tps:
            target += "[" + ", ".join(g.typ(env[tp]) for tp in fn_tps) + "]"
        call_expr = f"{target}({', '.join(call_args)})"
        if not names:
            body.append(f"\t{call_expr}")
        else:
            body.append(f"\t{', '.join(names)} := {call_expr}")
            if "err" in names:
                body.append("\tif err != nil {\n\t\treturn err\n\t}")
        body.append(f"\treturn harness.Expose(api{''.join(', ' + e for e in expose)})")
        call = (f"{show(subst(recv, env))}.{decl['name']}" if recv else f"{decl['pkg_name']}.{decl['name']}") + \
            f"({', '.join(call_args)})"
    ecc_alias = g.alias(ECC)
    src = [f"type {tname} struct {{"]
    for fname, ftype, tag in ctx.fields:
        src.append(f"\t{fname} {ftype}" + (" `gnark:\",secret\"`" if tag else ""))
    src += ["}", "", f"func (c *{tname}) Define(api frontend.API) error {{"] + body + ["}", ""]
    src.append("func init() {")
    src.append(f"\tharness.Register(harness.Spec{{ID: \"{wid}\", Field: {ecc_alias}.{plan.curve}.ScalarField(), "
               f"Curve: \"{CURVES[plan.curve]}\", New: func() frontend.Circuit {{")
    src.append(f"\t\treturn &{tname}{{{', '.join(ctx.shapes)}}}")
    src += ["\t}})", "}"]
    text = g.header() + "\n" + "\n".join(src) + "\n"
    return Wrapper(wid, tname, text, plan.curve, choice, call, ctx.inputs, ctx.probed_length)


def input_param(ctx: _Ctx, t: dict, fname: str) -> tuple[str, list[str]]:
    """(argument expression, statements before the call) for an input parameter; adds the field."""
    g = ctx.g
    k = t.get("k")
    if k == "ptr":
        ctx.add_input(fname, t["elem"])
        return f"&c.{fname}", []
    if k in ("slice", "array"):
        e = t["elem"]
        if e.get("k") == "ptr":
            inner_t = e["elem"]
            holder = dict(t, elem=inner_t)
            ctx.add_input(fname, holder)
            inner = g.typ(inner_t)
            if k == "array":
                pre = [f"\tvar p{fname} [{t['len']}]*{inner}"]
            else:
                pre = [f"\tp{fname} := make([]*{inner}, len(c.{fname}))"]
            pre.append(f"\tfor i := range c.{fname} {{\n\t\tp{fname}[i] = &c.{fname}[i]\n\t}}")
            return f"p{fname}", pre
        if e.get("k") == "slice" and e["elem"].get("k") == "ptr":
            inner_t = e["elem"]["elem"]
            holder = dict(t, elem=dict(e, elem=inner_t))
            ctx.add_input(fname, holder)
            inner = g.typ(inner_t)
            pre = [f"\tp{fname} := make([][]*{inner}, len(c.{fname}))",
                   f"\tfor i := range c.{fname} {{\n\t\tp{fname}[i] = make([]*{inner}, len(c.{fname}[i]))\n"
                   f"\t\tfor j := range c.{fname}[i] {{\n\t\t\tp{fname}[i][j] = &c.{fname}[i][j]\n\t\t}}\n\t}}"]
            return f"p{fname}", pre
    ctx.add_input(fname, t)
    return f"c.{fname}", []


def construct(ctx: _Ctx, t: dict, env: dict, var: str, depth: int) -> tuple[list[str], str, bool]:
    """Statements constructing the gadget object of type ``t`` into ``var``; (lines, expr, is pointer)."""
    if depth > 3:
        raise NoInstantiation(f"constructor chain too deep for {show(t)}")
    cons = constructors(ctx.cat, t)
    if not cons:
        raise NoInstantiation(f"no in-circuit constructor for {show(t)}")
    errors = []
    for f in cons:
        saved = (list(ctx.fields), list(ctx.shapes), list(ctx.inputs))
        try:
            lines = _construct_with(ctx, f, t, env, var, depth)
            return lines, var, f["sig"]["results"][0]["type"].get("k") == "ptr"
        except NoInstantiation as ex:
            ctx.fields, ctx.shapes, ctx.inputs = saved
            errors.append(f"{f['name']}: {ex}")
    raise NoInstantiation(f"no usable constructor for {show(t)} ({'; '.join(errors)[:300]})")


def _construct_with(ctx: _Ctx, f: dict, t: dict, env: dict, var: str, depth: int) -> list[str]:
    cat, g = ctx.cat, ctx.g
    r = f["sig"]["results"][0]["type"]
    rb = r["elem"] if r.get("k") == "ptr" else r
    ctps = [tp["name"] for tp in f["sig"].get("tparams") or []]
    cenv = {a["name"]: c for a, c in zip(rb.get("args") or [], t.get("args") or []) if a.get("k") == "tparam"}
    if any(tp not in cenv for tp in ctps):
        raise NoInstantiation("constructor type parameters not determined by the receiver")
    args, lines = [], []
    for j, p in enumerate(f["sig"]["params"]):
        pt = subst(p["type"], cenv)
        rr = role(cat, pt)
        if rr == "api":
            args.append("api")
        elif rr == "omit":
            continue
        elif rr == "object":
            base = pt["elem"] if pt.get("k") == "ptr" else pt
            ls, e, is_ptr = construct(ctx, base, env, f"{var}_{j}", depth + 1)
            lines += ls
            want = pt.get("k") == "ptr"
            args.append(e if want == is_ptr else ("&" + e if want else "*" + e))
        elif rr == "input":
            e, pre = input_param(ctx, pt, f"{var.capitalize()}In{j}")
            lines += pre
            args.append(e)
        else:
            prov = provider(ctx, pt, {**env, **cenv})
            if prov is None:
                prov = constructor_const(cat, f, j, (rr, p["name"], pt))
                if prov is not None and rr == "bigint":
                    g.alias("math/big")
            if prov is None:
                raise NoInstantiation(f"no value for constructor parameter {p['name']} ({show(pt)})")
            args.append(prov)
    fn = f"{g.alias(f['pkg'])}.{f['name']}"
    if ctps:
        fn += "[" + ", ".join(g.typ(cenv[tp]) for tp in ctps) + "]"
    res = f["sig"]["results"]
    if len(res) == 2 and is_error(res[1]["type"]):
        lines.append(f"\t{var}, err{var} := {fn}({', '.join(args)})")
        lines.append(f"\tif err{var} != nil {{\n\t\treturn err{var}\n\t}}")
    elif len(res) == 1:
        lines.append(f"\t{var} := {fn}({', '.join(args)})")
    else:
        raise NoInstantiation(f"constructor {f['name']} returns {len(res)} values")
    return lines


def provider(ctx: _Ctx, t: dict, env: dict) -> str | None:
    """A zero-argument function of the type's package, generic over type parameters bound in the current
    environment (by name), returning ``t``."""
    if t.get("k") != "named":
        return None
    for f in ctx.cat.by_pkg.get(t["pkg"], []):
        if f["sig"]["params"] or len(f["sig"]["results"]) != 1:
            continue
        r = f["sig"]["results"][0]["type"]
        if r.get("k") != "named" or r.get("pkg") != t["pkg"] or r.get("name") != t["name"]:
            continue
        tps = [tp["name"] for tp in f["sig"].get("tparams") or []]
        if tps and all(tp in env for tp in tps):
            return f"{ctx.g.alias(f['pkg'])}.{f['name']}[{', '.join(ctx.g.typ(env[tp]) for tp in tps)}]()"
    return None


def constructor_const(cat: Catalog, f: dict, j: int, param: tuple) -> str | None:
    for c in sorted(cat.calls.get(f"{f['pkg']}.{f['name']}", []), key=lambda c: (not c["test"], c["file"], c["line"])):
        args = c.get("args") or []
        if j < len(args):
            v = arg_literal(param, args[j])
            if v is not None:
                return v
    return None


def construct_origin(ctx: _Ctx, origin: str, oargs: list[str], var: str) -> tuple[list[str], str]:
    """Statements calling the function that produced an interface argument at the call site (``api`` and
    constant arguments only)."""
    cat, g = ctx.cat, ctx.g
    pkg, name = origin.rsplit(".", 1)
    f = next((x for x in cat.by_pkg.get(pkg, []) if x["name"] == name), None)
    if f is None:
        raise NoInstantiation(f"interface provider {origin} is not an exported function")
    params = f["sig"]["params"]
    args = []
    for p, a in zip(params, oargs):
        if is_api(p["type"]):
            args.append("api")
        elif re.fullmatch(r"-?\d+|true|false|\"[^\"]*\"", a or ""):
            args.append(a)
        else:
            raise NoInstantiation(f"interface provider {name} argument {a!r} is not a constant")
    if len(params) > len(oargs) and not all(is_option(p["type"]) for p in params[len(oargs):]):
        raise NoInstantiation(f"interface provider {name}: missing arguments")
    res = f["sig"]["results"]
    fn = f"{g.alias(pkg)}.{name}"
    lines = []
    if len(res) == 2 and is_error(res[1]["type"]):
        lines += [f"\t{var}, err{var} := {fn}({', '.join(args)})", f"\tif err{var} != nil {{\n\t\treturn err{var}\n\t}}"]
    else:
        lines.append(f"\t{var} := {fn}({', '.join(args)})")
    expr = var
    if res and res[0]["type"].get("k") == "named":
        info = cat.named_info(res[0]["type"]) or {}
        if (info.get("underlying") or {}).get("k") == "struct":
            expr = "&" + var
    return lines, expr
