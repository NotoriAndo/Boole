"""Decomposition of TOO-LARGE Noir functions into the callee instances they compile (wave RT-N2).

ACIR inlines every call, so the callees of a TOO-LARGE function are read from the compiler's own monomorphised
program (``nargo export|compile --show-monomorphized``, the same compile as the registry's): every constrained
function instance reachable from the compiled entry point, with its concrete parameter types (structs are printed as
the tuples of their fields).  An instance is located in the repository source by name and parameter names (the
caller's crate and its path dependencies; the standard library and git dependencies are other repositories), its
generic arguments are recovered by unifying the declared parameter types with the instance's types (struct types
expanded to their field tuples; a type argument printed as a tuple is named by the structs of matching shape), and
the candidate is confirmed by compiling an ``#[export]`` wrapper with those arguments: the wrapper's instance of the
function must print, up to the renaming of ids, exactly as the caller's instance (the same body with the same callee
instances, see :func:`instance_key`).  Confirmed instances are packaged by the DET generator with rule
``decomposition``; the package states DET of the callee as instantiated, not of the caller.
"""
from __future__ import annotations

import hashlib
import itertools
import re
from dataclasses import dataclass, field

from . import noir_source as NS

MAX_GUESSES = 8
# the decomposition driver selects instances; its packages record the selection (instantiation provenance and
# decomposition) and are produced by the DET generator, so these sources are hashed by the wave's run record
SOURCES = ["noir_callees.py"]
PRIMS = {"Field", "bool", "u1", "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "()"}

# ------------------------------------------------------------------------------------------ monomorphised program

_HEADER = re.compile(r"^(?P<mods>(?:unconstrained\s+)?)fn (?P<name>[A-Za-z_][A-Za-z0-9_]*)\$f(?P<id>\d+)\((?P<rest>.*)$")
_GLOBAL = re.compile(r"^global (?P<name>[A-Za-z_][A-Za-z0-9_]*)\$g(?P<id>\d+)\b")
_REF = re.compile(r"(?<![A-Za-z0-9_$])([A-Za-z_][A-Za-z0-9_]*)\$f(\d+)\b")
_GREF = re.compile(r"(?<![A-Za-z0-9_$])([A-Za-z_][A-Za-z0-9_]*)\$g(\d+)\b")
_LREF = re.compile(r"\$l(\d+)\b")


@dataclass
class Instance:
    id: int
    name: str
    params: list[tuple[str, str]]          # (name without the $l suffix, monomorphised type)
    ret: str
    unconstrained: bool
    attrs: list[str]
    text: str                              # the printed function, attributes included
    refs: list[int] = field(default_factory=list)      # functions referenced by the body, in order
    grefs: list[int] = field(default_factory=list)     # globals referenced, in order


@dataclass
class Program:
    root: int
    instances: dict[int, Instance]
    globals: dict[int, str]


def _split_header_params(rest: str) -> tuple[str, str]:
    """``a$l0: T, b$l1: U) -> R {`` -> (params text, return type text)."""
    depth = 0
    for i, c in enumerate(rest):
        if c in "([":
            depth += 1
        elif c in ")]":
            if depth == 0:
                tail = rest[i + 1:].strip()
                m = re.match(r"->\s*(?:pub\s+)?(.*?)\s*\{\s*$", tail)
                return rest[:i], (m.group(1) if m else "()")
            depth -= 1
    raise ValueError(f"unbalanced header: {rest[:120]!r}")


def parse_params_mono(s: str) -> list[tuple[str, str]]:
    out = []
    for part in NS.split_top(s):
        part = part.strip()
        if not part:
            continue
        m = re.match(r"(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\$l\d+\s*:\s*(.+)$", part, re.S)
        if m:
            out.append((m.group(1), m.group(2).strip()))
        else:
            out.append(("_", part.split(":", 1)[-1].strip()))
    return out


def parse_programs(text: str) -> list[Program]:
    """Every program of a ``--show-monomorphized`` output (one per compiled entry point: a function with id 0 starts
    a new program; the globals printed before it belong to it)."""
    progs: list[Program] = []
    lines = text.splitlines()
    cur_inst: dict[int, Instance] = {}
    cur_glob: dict[int, str] = {}
    pending_glob: dict[int, str] = {}
    root = None
    i = 0
    attrs: list[str] = []

    def close():
        if root is not None:
            progs.append(Program(root, cur_inst, cur_glob))

    while i < len(lines):
        ln = lines[i]
        g = _GLOBAL.match(ln)
        if g:
            j = i
            buf = [ln]
            while not buf[-1].rstrip().endswith(";") and j + 1 < len(lines):
                j += 1
                buf.append(lines[j])
            pending_glob[int(g.group("id"))] = "\n".join(buf)
            i = j + 1
            continue
        if ln.startswith("#["):
            attrs.append(ln.strip())
            i += 1
            continue
        h = _HEADER.match(ln)
        if not h:
            attrs = []
            i += 1
            continue
        fid = int(h.group("id"))
        if fid == 0:
            close()
            cur_inst, cur_glob, root = {}, dict(pending_glob), 0
            pending_glob = {}
        elif pending_glob:
            cur_glob.update(pending_glob)
            pending_glob = {}
        j = i
        while j < len(lines) and lines[j] != "}":
            j += 1
        body = lines[i:j + 1]
        ptext, ret = _split_header_params(h.group("rest"))
        btxt = "\n".join(body[1:])
        refs, grefs = [], []
        for m in _REF.finditer(btxt):
            k = int(m.group(2))
            if k not in refs:
                refs.append(k)
        for m in _GREF.finditer("\n".join(body)):
            k = int(m.group(2))
            if k not in grefs:
                grefs.append(k)
        cur_inst[fid] = Instance(fid, h.group("name"), parse_params_mono(ptext), ret, bool(h.group("mods").strip()),
                                 attrs, "\n".join(attrs + body), refs, grefs)
        attrs = []
        i = j + 1
    close()
    return progs


def reachable(prog: Program, start: int | None = None) -> dict[int, int]:
    """Constrained instances reachable from ``start`` (default: the root) through constrained instances -> depth
    (calls into unconstrained functions are Brillig hints: their callees are not part of the circuit)."""
    start = prog.root if start is None else start
    depth = {start: 0}
    todo = [start]
    while todo:
        f = todo.pop(0)
        for k in prog.instances[f].refs:
            inst = prog.instances.get(k)
            if inst is None or inst.unconstrained or k in depth:
                continue
            depth[k] = depth[f] + 1
            todo.append(k)
    return depth


def canonical(prog: Program, fid: int) -> str:
    """The instance and everything it references (constrained or not) with function, local and global ids renamed by
    first occurrence: equal texts are the same function at the same generic arguments with the same callees."""
    order, seen, todo = [], set(), [fid]
    while todo:
        f = todo.pop(0)
        if f in seen or f not in prog.instances:
            continue
        seen.add(f)
        order.append(f)
        todo += prog.instances[f].refs
    gl = []
    for f in order:
        for g in prog.instances[f].grefs:
            if g not in gl:
                gl.append(g)
    text = "\n".join([prog.globals.get(g, f"global ?$g{g}") for g in gl] + [prog.instances[f].text for f in order])
    fmap, gmap, lmap = {}, {}, {}

    def fr(m):
        return f"{m.group(1)}$F{fmap.setdefault(m.group(2), len(fmap))}"

    def gr(m):
        return f"{m.group(1)}$G{gmap.setdefault(m.group(2), len(gmap))}"

    def lr(m):
        return f"$L{lmap.setdefault(m.group(1), len(lmap))}"
    text = _REF.sub(fr, text)
    text = _GREF.sub(gr, text)
    return _LREF.sub(lr, text)


def instance_key(prog: Program, fid: int) -> str:
    return hashlib.sha256(canonical(prog, fid).encode()).hexdigest()


def wrapper_instance(prog: Program, fn_name: str) -> int | None:
    """The instance of ``fn_name`` the wrapper (the program root) calls directly."""
    for k in prog.instances[prog.root].refs:
        inst = prog.instances.get(k)
        if inst is not None and inst.name == fn_name:
            return k
    return None


# ------------------------------------------------------------------------------------------ types

@dataclass(frozen=True)
class Ty:
    k: str                       # name | arr | slice | tuple | ref | str | num | fn | other
    name: str = ""               # name: path; num: text; ref: "mut" | ""
    args: tuple = ()             # name: generic args; tuple: elements; arr/slice/ref: (elem,); arr: (elem, len)
    text: str = ""


def parse_type(s: str) -> Ty:
    s = s.strip()
    if not s:
        return Ty("other", text=s)
    if s.startswith("&"):
        m = re.match(r"&\s*(mut\s+)?(.*)$", s, re.S)
        return Ty("ref", "mut" if m.group(1) else "", (parse_type(m.group(2)),), s)
    if s.startswith("[") and s.endswith("]"):
        inner = s[1:-1]
        parts = NS.split_top(inner, ";")
        if len(parts) == 2:
            return Ty("arr", "", (parse_type(parts[0]), parse_num(parts[1])), s)
        return Ty("slice", "", (parse_type(inner),), s)
    if s.startswith("(") and s.endswith(")") and NS.matching(s, 0, "(", ")") == len(s):
        inner = s[1:-1].strip()
        if not inner:
            return Ty("name", "()", (), s)
        parts = [p for p in NS.split_top(inner) if p.strip()]
        if len(parts) == 1 and not inner.rstrip().endswith(","):
            return parse_type(parts[0])
        return Ty("tuple", "", tuple(parse_type(p) for p in parts), s)
    if re.match(r"(unconstrained\s+)?fn\b", s) or s.startswith("impl "):
        return Ty("fn", text=s)
    m = re.match(r"((?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*[A-Za-z_][A-Za-z0-9_]*)\s*(<.*>)?$", s, re.S)
    if m:
        name = re.sub(r"\s+", "", m.group(1))
        args = tuple(parse_arg(a) for a in NS.split_top(m.group(2)[1:-1])) if m.group(2) else ()
        if name in ("str", "fmtstr"):
            return Ty("str", name, args, s)
        return Ty("name", name, args, s)
    if re.fullmatch(r"[0-9_]+(?:\s*as\s*u32)?", s) or re.search(r"[+*/-]", s):
        return parse_num(s)
    return Ty("other", text=s)


def parse_num(s: str) -> Ty:
    s = s.strip()
    return Ty("num", s, (), s)


def parse_arg(s: str) -> Ty:
    s = s.strip()
    if re.fullmatch(r"[0-9_]+", s) or re.search(r"\s[+*/-]\s", s):
        return parse_num(s)
    return parse_type(s)


def last(name: str) -> str:
    return name.split("::")[-1]


class Unify:
    """Structural unification of declared (source) types with monomorphised types: binds the target's generic
    parameters.  ``structs(name) -> (generic names, field types) | None`` expands a struct type to the tuple the
    monomorphiser prints; ``consts(name) -> int | None`` evaluates a global numeric constant."""

    def __init__(self, generics: list[NS.Generic], structs, consts, self_type: str | None = None):
        self.gen = {g.name: g for g in generics}
        self.structs = structs
        self.consts = consts
        self.self_type = self_type
        self.env: dict[str, Ty] = {}
        self.ok = True

    def bind(self, name: str, mono: Ty) -> None:
        old = self.env.get(name)
        if old is None:
            self.env[name] = mono
        elif old.text.replace(" ", "") != mono.text.replace(" ", ""):
            self.ok = False

    def num(self, src: Ty, mono: Ty) -> None:
        txt = src.name if src.k == "num" else src.text
        txt = txt.strip()
        mval = mono.name if mono.k == "num" else mono.text
        try:
            mv = int(str(mval).replace("_", "").split()[0])
        except ValueError:
            return
        if txt in self.gen and self.gen[txt].numeric:
            self.bind(txt, Ty("num", str(mv), (), str(mv)))
            return
        if re.fullmatch(r"[0-9_]+", txt):
            if int(txt.replace("_", "")) != mv:
                self.ok = False
            return
        v = self.consts(last(txt)) if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_:]*", txt) else None
        if v is not None and v != mv:
            self.ok = False

    def unify(self, src: Ty, mono: Ty, depth: int = 0) -> None:
        if not self.ok or depth > 12:
            return
        if src.k == "name" and src.name == "Self" and self.self_type:
            src = parse_type(self.self_type)
        if src.k == "name" and not src.args and src.name in self.gen and not self.gen[src.name].numeric:
            self.bind(src.name, mono)
            return
        if src.k == "ref":
            if mono.k == "ref":
                self.unify(src.args[0], mono.args[0], depth + 1)
            else:
                self.unify(src.args[0], mono, depth + 1)
            return
        if src.k == "arr":
            if mono.k != "arr":
                self.ok = mono.k in ("other", "fn") and self.ok
                return
            self.unify(src.args[0], mono.args[0], depth + 1)
            self.num(src.args[1], mono.args[1])
            return
        if src.k == "slice":
            if mono.k == "slice":
                self.unify(src.args[0], mono.args[0], depth + 1)
            return
        if src.k == "tuple":
            if mono.k == "tuple" and len(mono.args) == len(src.args):
                for a, b in zip(src.args, mono.args):
                    self.unify(a, b, depth + 1)
            else:
                self.ok = False
            return
        if src.k == "str":
            if mono.k == "str" and src.args and mono.args:
                self.num(src.args[0], mono.args[0])
            return
        if src.k == "name":
            if src.name in PRIMS or last(src.name) in PRIMS:
                if mono.k == "name" and mono.name != last(src.name):
                    self.ok = False
                return
            st = self.structs(last(src.name))
            if st is None:
                return
            gnames, ftypes = st
            sub = dict(zip(gnames, src.args))
            fields = [subst_ty(parse_type(ft), sub) for ft in ftypes]
            if mono.k == "tuple" and len(mono.args) == len(fields):
                for a, b in zip(fields, mono.args):
                    self.unify(a, b, depth + 1)
            elif len(fields) == 1 and mono.k != "tuple":
                self.unify(fields[0], mono, depth + 1)
            elif mono.k in ("name", "arr", "tuple"):
                self.ok = False


def subst_ty(t: Ty, sub: dict[str, Ty]) -> Ty:
    if t.k == "name" and not t.args and t.name in sub:
        return sub[t.name]
    if t.k == "num" and t.name in sub:
        return sub[t.name]
    if not t.args:
        return t
    return Ty(t.k, t.name, tuple(subst_ty(a, sub) if isinstance(a, Ty) else a for a in t.args), t.text)


def source_text(t: Ty) -> str | None:
    """A monomorphised type that is nameable as written (primitives, arrays, tuples, strings of those)."""
    if t.k == "num":
        return t.name
    if t.k == "name":
        return t.name if t.name in PRIMS else None
    if t.k == "arr":
        e = source_text(t.args[0])
        return None if e is None else f"[{e}; {t.args[1].name}]"
    if t.k == "tuple":
        es = [source_text(a) for a in t.args]
        return None if any(e is None for e in es) else "(" + ", ".join(es) + ("," if len(es) == 1 else "") + ")"
    if t.k == "str" and t.args:
        return f"str<{t.args[0].name}>"
    return None


def shape_of(name: str, args: tuple, structs, depth: int = 0) -> str | None:
    """The monomorphised text of a source struct type (fields expanded), for matching a tuple-typed argument."""
    if depth > 8:
        return None
    st = structs(last(name))
    if st is None:
        return None
    gnames, ftypes = st
    if len(gnames) != len(args):
        return None
    sub = dict(zip(gnames, args))
    parts = []
    for ft in ftypes:
        s = mono_text(subst_ty(parse_type(ft), sub), structs, depth + 1)
        if s is None:
            return None
        parts.append(s)
    return "(" + ", ".join(parts) + ("," if len(parts) == 1 else "") + ")"


def mono_text(t: Ty, structs, depth: int = 0) -> str | None:
    if t.k == "num":
        return t.name
    if t.k == "name":
        if t.name in PRIMS or last(t.name) in PRIMS:
            return last(t.name)
        return shape_of(t.name, t.args, structs, depth)
    if t.k == "arr":
        e = mono_text(t.args[0], structs, depth)
        return None if e is None else f"[{e}; {t.args[1].name}]"
    if t.k == "tuple":
        es = [mono_text(a, structs, depth) for a in t.args]
        return None if any(e is None for e in es) else "(" + ", ".join(es) + ("," if len(es) == 1 else "") + ")"
    if t.k == "str" and t.args:
        return f"str<{t.args[0].name}>"
    return None


def norm(s: str) -> str:
    return re.sub(r"\s+", "", s)


# ------------------------------------------------------------------------------------------ location and arguments

def param_names(fn: NS.Function) -> list[str]:
    return [re.sub(r"^mut\s+", "", p.pattern).strip() for p in fn.params]


def matches_names(fn: NS.Function, inst: Instance) -> bool:
    if fn.name != inst.name or len(fn.params) != len(inst.params):
        return False
    for src, (mn, _) in zip(param_names(fn), inst.params):
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", src) and src != mn and not (src.startswith("_") and mn == "_"):
            return False
    return True


def literals(canon: str, limit: int = MAX_GUESSES) -> list[str]:
    """Integer literals of an instance text: loop bounds and array lengths first, then by frequency (guesses for a
    numeric generic the types do not determine; every guess is confirmed by the instance comparison)."""
    c: dict[str, int] = {}
    shaped: set[str] = set()
    for m in re.finditer(r"(?<![A-Za-z0-9_$.])(\d+)(?![A-Za-z0-9_])", canon):
        v = str(int(m.group(1)))
        c[v] = c.get(v, 0) + 1
    for m in re.finditer(r"(?:\.\.\s*|;\s*)(\d+)(?![A-Za-z0-9_])", canon):
        shaped.add(str(int(m.group(1))))
    return [v for v, _ in sorted(c.items(), key=lambda x: (x[0] not in shaped, -x[1], int(x[0])))][:limit]


def assignments(fn: NS.Function, inst: Instance, generics: list[NS.Generic], structs, consts,
                named_candidates, env_names: tuple = (), guesses: list[str] | None = None) -> tuple[list[dict], str]:
    """Generic assignments of ``fn`` (impl generics first) that reproduce the instance's parameter and return
    types: ([{generic: source text}], reason when none).  ``named_candidates(generic, mono text)`` lists source
    type texts (struct types) whose expansion prints as the given tuple; closure environments (``env_names``) are
    left to inference (``_``); a numeric generic the types do not determine takes each of ``guesses`` (literals of
    the instance), to be confirmed by the instance comparison."""
    imp = fn.impl
    u = Unify(generics, structs, consts, imp.self_type if imp is not None else None)
    for p, (_, mty) in zip(fn.params, inst.params):
        if p.type:
            u.unify(parse_type(p.type), parse_type(mty))
    if fn.ret and fn.ret.strip() not in ("", "()"):
        u.unify(parse_type(re.sub(r"^pub\s+", "", fn.ret.strip())), parse_type(inst.ret))
    if not u.ok:
        return [], "declared types do not match the instance"
    choices = []
    for g in generics:
        v = u.env.get(g.name)
        if g.name in env_names:
            choices.append([(g.name, "_")])
            continue
        if v is None and g.numeric and guesses:
            choices.append([(g.name, x) for x in guesses])
            continue
        if v is None:
            return [], f"generic `{g.name}` is not determined by the parameter and return types"
        if g.numeric:
            choices.append([(g.name, v.name if v.k == "num" else v.text)])
            continue
        txt = source_text(v)
        named = named_candidates(g, v.text) if v.k == "tuple" or txt is None else []
        # a tuple-printed argument is named by the structs of its shape first (the monomorphiser prints structs as
        # tuples); the tuple type itself is the last resort (the same instance text when no trait method of the
        # argument is called)
        opts = [n for n in named if n != txt] + ([txt] if txt is not None else [])
        if not opts:
            return [], f"no nameable type for `{g.name}` (instance type {v.text[:120]})"
        choices.append([(g.name, o) for o in opts[:MAX_GUESSES]])
    out = []
    for combo in itertools.product(*choices):
        out.append(dict(combo))
        if len(out) >= MAX_GUESSES:
            break
    return out, ""


# ------------------------------------------------------------------------------------------ struct naming

STD_STRUCTS = {     # standard-library structs that appear in repository signatures (field order of the library)
    "Option": (["T"], ["bool", "T"]),
    "BoundedVec": (["T", "MaxLen"], ["[T; MaxLen]", "u32"]),
    "EmbeddedCurvePoint": ([], ["Field", "Field", "bool"]),
    "EmbeddedCurveScalar": ([], ["Field", "Field"]),
}


class Structs:
    """Struct expansion and naming over the caller crate's scope (repository index) plus :data:`STD_STRUCTS`."""

    def __init__(self, idx, scope: list[str], visible):
        self.idx = idx
        self.scope = scope
        self.visible = visible
        self._by_arity: dict[int, list] | None = None

    def __call__(self, name: str):
        d = self.idx.struct_def(name, self.scope) if self.idx is not None else None
        if d is not None:
            sd = d[1]
            return [g.name for g in sd.generics], [ft for _, _, ft in sd.fields]
        return STD_STRUCTS.get(name)

    def by_arity(self, n: int) -> list:
        if self._by_arity is None:
            self._by_arity = {}
            for name, defs in (self.idx.struct_defs.items() if self.idx is not None else []):
                for rel, sd, in_test in defs:
                    if in_test or not any(rel == s or rel.startswith(s + "/") for s in self.scope):
                        continue
                    if not self.visible(name):
                        continue
                    self._by_arity.setdefault(len(sd.fields), []).append(sd)
        return self._by_arity.get(n, [])

    def name_tuple(self, mono: Ty, depth: int = 0, hint: str = "") -> list[str]:
        """Source type texts whose expansion prints as the monomorphised ``mono`` (structs of matching shape, generic
        ones with every combination of nameable arguments), ranked by how many of their field names the instance
        text ``hint`` binds (the monomorphiser prints a struct literal as ``let <field>$l.. = ..``)."""
        txt = source_text(mono)
        if depth > 3:
            return [txt] if txt is not None else []
        elems = list(mono.args) if mono.k == "tuple" else [mono]
        scored: list[tuple[int, str]] = []
        for sd in self.by_arity(len(elems)):
            u = Unify(sd.generics, self, lambda n: None)
            for (_, _, ft), m in zip(sd.fields, elems):
                u.unify(parse_type(ft), m)
            if not u.ok:
                continue
            opts = []
            for g in sd.generics:
                v = u.env.get(g.name)
                if v is None:
                    break
                if g.numeric:
                    opts.append([v.name if v.k == "num" else v.text])
                    continue
                names = (self.name_tuple(v, depth + 1, hint) if (v.k == "tuple" or source_text(v) is None)
                         else [source_text(v)])
                if not names:
                    break
                opts.append(names[:MAX_GUESSES])
            else:
                score = sum(1 for _, f, _ in sd.fields if re.search(r"\blet " + re.escape(f) + r"\$[lL]\d", hint))
                for combo in itertools.islice(itertools.product(*opts), MAX_GUESSES):
                    cand = sd.name + (f"<{', '.join(combo)}>" if combo else "")
                    mt = mono_text(parse_type(cand), self)
                    if mt is not None and norm(mt) == norm(mono.text) and cand not in [c for _, c in scored]:
                        scored.append((score, cand))
        scored.sort(key=lambda x: -x[0])                   # stable: declaration order among equal scores
        out = [c for _, c in scored][:MAX_GUESSES]
        return out + ([txt] if txt is not None and txt not in out else [])


# ------------------------------------------------------------------------------------------ driver pieces

MONO_FLAG = "--show-monomorphized"
CALLER_INSTANCE = "caller instance: "      # provenance prefix (ratchet.decomposition_reference)


def provenance(parents: list[str], key: str, assign: dict) -> list[str]:
    """Provenance of a decomposition record: the TOO-LARGE callers and the confirmed instance."""
    more = f" and {len(parents) - 3} more" if len(parents) > 3 else ""
    out = [CALLER_INSTANCE + f"monomorphised instance {key[:16]} of the TOO-LARGE caller(s) {', '.join(parents[:3])}"
           f"{more}, confirmed by the wrapper's own instance"]
    if assign:
        out.append("generic arguments from the instance: " + ", ".join(f"{k}={v}" for k, v in assign.items()))
    return out
NOT_REPO = ("not a function of the caller's crate or its path dependencies (standard library or a git dependency: "
            "another repository)")


def provenance_fn_args(fn: NS.Function, provenance: list[str], consts: bool = True) -> dict[int, str]:
    """The function arguments (``function argument `p` = `e` from ...``) and, with ``consts``, the compile-time
    constant arguments (``parameter `p` fixed to the compile-time constant `v` from ...``) a registry record fixed,
    by parameter index."""
    out = {}
    names = [p.pattern for p in fn.params]
    for pv in provenance:
        m = re.match(r"function argument `(.+?)` = `(.+?)` from ", pv) or \
            (re.match(r"parameter `(.+?)` fixed to the compile-time constant `(.+?)` from ", pv) if consts else None)
        if m and m.group(1) in names:
            out[names.index(m.group(1))] = m.group(2)
    return out


def located(sh, t_caller, inst: Instance) -> list:
    """Source functions of the caller's crate and its path dependencies that the instance can be (name and
    parameter names; constrained, with a body, outside test code)."""
    from . import noir_det as ND
    from . import noir_instantiation as I
    from . import noir_toolchain as T
    out = []
    for p, fn, text, src in ND.crate_functions(t_caller).get(inst.name, []):
        if fn.unconstrained or fn.comptime or not fn.has_body or not matches_names(fn, inst):
            continue
        if "$" in "".join([p.type for p in fn.params] + [fn.ret, fn.impl.self_type if fn.impl is not None else ""]):
            continue                      # an item of a `quote { }` block (comptime macro text, not a declaration)
        root = t_caller.root
        if not p.startswith(root + "/") or I.is_test_path(p[len(root) + 1:]):
            continue
        crate = T.crate_of(p, root)
        out.append(I.Target(f"decomposition-n2:{t_caller.repo}:{p[len(root) + 1:]}#{fn.name}@L{fn.line}", t_caller.repo,
                            root, p[len(root) + 1:], fn, text, src, crate,
                            T.crate_info(crate)["type"] if crate else "lib", False))
    return out


def instance_assignments(sh, t, inst: Instance, guesses: list[str] | None = None, hint: str = "") \
        -> tuple[list[dict], str]:
    from . import noir_instantiation as I
    idx = sh.indexes.get(t.repo)
    scope = I.crate_scope(t)
    st = Structs(idx, scope, I.visible_in(t, idx) if idx is not None else (lambda n: False))

    def consts(name):
        v = idx.resolve(name) if idx is not None else None
        try:
            return int(v) if v is not None else None
        except ValueError:
            return None

    def named(g, mono_txt):
        return st.name_tuple(parse_type(mono_txt), 0, hint)
    return assignments(t.fn, inst, [g for _, g in t.generics], st, consts, named, tuple(I.env_generics(t)), guesses)


_PRICED = re.compile(r"==|!=|<|>|[-+*/%&|^]|\bassert|\bas\b|\bfor\b|\bif\b|\bwhile\b|\$(?![FLG]\d)[A-Za-z_]"
                     r"|\[(?!\d+\])")


def trivially_free(canon: str) -> bool:
    """No operation of the instance (and everything it calls) can produce a priced opcode: no arithmetic, comparison,
    cast, assertion, branch, loop or builtin call (data movement only: its circuit is linear)."""
    body = "\n".join(ln for ln in canon.splitlines()
                     if not ln.startswith(("fn ", "unconstrained fn ", "global ", "#[")))
    body = re.sub(r"&mut\s", " ", body)                            # references passed on (no operation)
    return not _PRICED.search(body)
