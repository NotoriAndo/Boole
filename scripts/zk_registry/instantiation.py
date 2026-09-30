"""Instantiation rules for circom templates.

A DET problem needs a concrete instantiation.  Candidates come from the repository itself, in
this order of rule tiers (the first tier with at least one compilable candidate is used):

``parameter-free``      the template takes no parameters: ``T()``
``repo-main``           a ``component main = T(args)`` declaration anywhere in the repository, a
                        circomkit ``circuits.json`` entry, or a ``component main`` written by a
                        non-test repository script (literal parameters only)
``repo-test``           ``T(args)`` inside a test/example wrapper (a file with ``component main``
                        or under ``test/``); ``var`` bindings of the wrapper are substituted; or a
                        main declared by a JS/TS test (circomkit ``WitnessTester``/``ProofTester``
                        parameters, ``component main`` strings with literal ``${CONST}`` values)
``repo-internal``       ``T(args)`` with literal arguments inside another library template
``repo-derived``        ``T(args)`` inside a template ``E`` (any file, test wrappers included) whose
                        own parameters are grounded by any tier; ``E``'s parameters, single-assignment
                        ``var`` bindings and the values of simple ``for`` loops the arguments depend
                        on are substituted until the arguments are closed (chains of any length from
                        mains and tests, repeated until nothing new is derived)
``documented-default``  a small default set from :data:`DOCUMENTED_DEFAULTS`, allowed only when
                        the repository documents the parameter domain (the entry cites it)
``probed``              only when no other tier has a candidate and the template's own top-level
                        asserts bound every parameter from above: a fixed small value set
                        (:data:`PROBE_VALUES`) inside those bounds.  Probed packages are labelled
                        ``instantiation.rule = probed``; a DET counterexample on them is not a finding

Otherwise the template is UNINSTANTIABLE, with the reason recorded.  Within the selected tier the
driver compiles every candidate and keeps the one with the largest constraint count inside the
size policy (ties: fewer wires, then argument text); if none fits, the smallest is reported as
TOO-LARGE.  No parameter is invented: every non-probed candidate is an expression the repository
itself writes, evaluated with parameters the repository itself supplies.
"""
from __future__ import annotations

import itertools
import re
from dataclasses import dataclass, field

from . import circom_eval as V
from . import circom_source as cs

TIERS = ["parameter-free", "repo-main", "repo-test", "repo-internal", "repo-derived", "documented-default", "probed"]
MAX_DERIVED_PER_TEMPLATE = 8
DERIVATION_ROUNDS = 12
LOOP_LIMIT = 64                 # values enumerated per loop variable
COMBO_LIMIT = 256               # loop-value combinations per call site
PROBE_VALUES = (1, 2, 3, 4, 8, 16, 32, 64)

# (repository id, template name) -> {"args": [[...], ...], "domain_source": "<file:line or doc URL>",
# "reason": "..."}.  Entries are allowed only when the repository documents the parameter domain.
# circomlib v2.0.5 documents no parameter domain for its otherwise uninstantiable templates (the
# README entries for them are empty stubs), so it has no entries.
DOCUMENTED_DEFAULTS: dict[tuple[str, str], dict] = {}


@dataclass
class Candidate:
    tier: str
    args: tuple[str, ...]
    provenance: list[str] = field(default_factory=list)   # e.g. ["test/circuits/x.circom:5 Main()"]

    @property
    def call(self) -> str:
        return "(" + ", ".join(self.args) + ")"


@dataclass
class TemplatePlan:
    template: cs.Template
    candidates: dict[str, list[Candidate]]                  # tier -> candidates (deduplicated, sorted)
    uninstantiable_reason: str | None = None

    def tiers_in_order(self) -> list[tuple[str, list[Candidate]]]:
        return [(t, self.candidates[t]) for t in TIERS if self.candidates.get(t)]


# ------------------------------------------------------------------------------------------ helpers

_ASSIGN_OPS = r"(?:=(?!=)|\+=|-=|\*=|/=|\\=|%=|\*\*=|<<=|>>=|&=|\|=|\^=|\+\+|--)"


def _depths(body: str) -> list[tuple[int, int]]:
    """(brace depth, paren depth) at each character of ``body``."""
    out, b, p = [], 0, 0
    for ch in body:
        out.append((b, p))
        if ch == "{":
            b += 1
        elif ch == "}":
            b -= 1
        elif ch == "(":
            p += 1
        elif ch == ")":
            p -= 1
    return out


def constant_bindings(body: str) -> dict[str, str]:
    """Top-level ``var NAME[dims] = EXPR;`` declarations that are never assigned again."""
    depth = _depths(body)
    found: dict[str, list[tuple[int, str]]] = {}
    for m in re.finditer(r"\bvar\s+(" + cs._IDENT + r")\s*((?:\[[^\]]*\]\s*)*)=\s*([^;]+);", body):
        name = m.group(1)
        found.setdefault(name, []).append((m.start(1), m.group(3).strip()))
        if depth[m.start()] != (0, 0):
            found[name].append((-1, ""))   # nested or for-header declaration: never substitutable
    out = {}
    for name, decls in found.items():
        if len(decls) != 1:
            continue
        pos, expr = decls[0]
        reassigned = [m for m in re.finditer(r"(?<![A-Za-z0-9_$.])" + re.escape(name) + r"\s*(?:\[[^\]]*\]\s*)*" + _ASSIGN_OPS, body)
                      if m.start() != pos]
        pre_ops = re.search(r"(\+\+|--)\s*" + re.escape(name) + r"\b", body)
        if not reassigned and not pre_ops:
            out[name] = expr
    return out


def _substitute(expr: str, env: dict[str, str]) -> str:
    def rep(m: re.Match) -> str:
        v = env.get(m.group(0))
        return m.group(0) if v is None else _paren(v)
    return re.sub(r"(?<![A-Za-z0-9_$.])" + cs._IDENT + r"(?![A-Za-z0-9_$])", rep, expr)


def _paren(v: str) -> str:
    v = v.strip()
    if re.fullmatch(r"-?\d+", v) or re.fullmatch(cs._IDENT, v):
        return v
    if v.startswith("[") and cs.match_bracket(v, 0) == len(v) - 1:
        return v
    return f"({v})"


def _closed(expr: str, functions: set[str]) -> bool:
    """Only literals, operators, brackets and calls of known functions remain."""
    for m in re.finditer(r"(?<![A-Za-z0-9_$.])" + cs._IDENT, expr):
        name = m.group(0)
        after = expr[m.end():].lstrip()
        if not (name in functions and after.startswith("(")):
            return False
    return True


def _simplify(arg: str) -> str:
    a = re.sub(r"\s+", "", arg)
    while a.startswith("(") and cs.match_bracket(a, 0) == len(a) - 1:
        a = a[1:-1]
    return a


def resolve_args(args: list[str], env: dict[str, str], functions: set[str]) -> tuple[str, ...] | None:
    out = []
    for a in args:
        cur = a
        for _ in range(6):
            nxt = _substitute(cur, env)
            if nxt == cur:
                break
            cur = nxt
        if not _closed(cur, functions):
            return None
        out.append(_simplify(cur))
    return tuple(out)


def function_names(files: dict[str, cs.SourceFile]) -> set[str]:
    names = set()
    for sf in files.values():
        names.update(m.group(1) for m in re.finditer(r"\bfunction\s+(" + cs._IDENT + r")\s*\(", sf.clean))
    return names


# ------------------------------------------------------------------------------------------ call sites

def scoped_bindings(body: str) -> dict[str, str]:
    """:func:`constant_bindings` plus ``var NAME = EXPR;`` declarations inside nested blocks (e.g. a loop
    body) whose name is declared exactly once in the template and never assigned again.  ``for``-header
    variables are excluded.  A call site can only see such a variable inside its scope, so substituting
    it at every site that names it is sound."""
    depth = _depths(body)
    found: dict[str, list[tuple[int, str, bool]]] = {}
    for m in re.finditer(r"\bvar\s+(" + cs._IDENT + r")\s*((?:\[[^\]]*\]\s*)*)=\s*([^;]+);", body):
        found.setdefault(m.group(1), []).append((m.start(1), m.group(3).strip(), depth[m.start()][1] > 0))
    for m in re.finditer(r"\bvar\s+(" + cs._IDENT + r")\s*(?:\[[^\]]*\]\s*)*;", body):
        found.setdefault(m.group(1), []).append((m.start(1), "", True))       # declared without a value
    out = {}
    for name, decls in found.items():
        if len(decls) != 1 or decls[0][2]:
            continue
        pos, expr, _ = decls[0]
        reassigned = [m for m in re.finditer(r"(?<![A-Za-z0-9_$.])" + re.escape(name) + r"\s*(?:\[[^\]]*\]\s*)*" + _ASSIGN_OPS, body)
                      if m.start() != pos]
        if not reassigned and not re.search(r"(\+\+|--)\s*" + re.escape(name) + r"\b", body):
            out[name] = expr
    return out


def int_env(env: dict[str, str]) -> dict[str, int]:
    """Names of ``env`` (text bindings) whose value evaluates to an integer."""
    ints: dict[str, int] = {}
    changed = True
    while changed:
        changed = False
        for name, expr in env.items():
            if name not in ints:
                v = V.eval_int(expr, ints)
                if v is not None:
                    ints[name] = v
                    changed = True
    return ints


def _names(expr: str) -> set[str]:
    return {m.group(0) for m in re.finditer(r"(?<![A-Za-z0-9_$.])" + cs._IDENT + r"(?![A-Za-z0-9_$])", expr)}


def _substitute_fix(expr: str, env: dict[str, str]) -> str:
    cur = expr
    for _ in range(6):
        nxt = _substitute(cur, env)
        if nxt == cur:
            break
        cur = nxt
    return cur


_BODY_CACHE: dict[str, tuple[dict[str, str], list]] = {}


def body_facts(body: str) -> tuple[dict[str, str], list]:
    """(scoped bindings, loops) of a template body, memoized (bodies are immutable text)."""
    hit = _BODY_CACHE.get(body)
    if hit is None:
        if len(_BODY_CACHE) > 4096:
            _BODY_CACHE.clear()
        hit = _BODY_CACHE[body] = (scoped_bindings(body), V.parse_loops(body))
    return hit


def site_arg_sets(ins: cs.Instantiation, enclosing: cs.Template, parent_args: tuple[str, ...],
                  functions: set[str]) -> list[tuple[tuple[str, ...], str]]:
    """Closed argument tuples of call site ``ins`` inside ``enclosing`` instantiated with ``parent_args``:
    the enclosing parameters and single-assignment ``var`` bindings are substituted, and loop variables
    the arguments depend on are enumerated over the values their (evaluable) ``for`` headers give.
    Returns (args, note) pairs, note naming the loop values (``[i=2]``) or empty."""
    scoped, all_loops = body_facts(enclosing.body)
    env = dict(scoped)
    env.update(zip(enclosing.params, parent_args))
    closed = resolve_args(ins.args, env, functions)
    if closed is not None:
        return [(closed, "")]
    if ins.pos < 0:
        return []
    loops = [lp for lp in V.enclosing_loops(all_loops, ins.pos) if lp.var]
    by_var = {lp.var: lp for lp in loops}
    need = {n for a in ins.args for n in _names(_substitute_fix(a, env))} - functions
    if not need or not need <= set(by_var):
        return []
    grew = True
    while grew:                           # loops whose bounds the needed loops depend on
        grew = False
        for var in list(need):
            lp = by_var[var]
            more = (_names(_substitute_fix(lp.start, env)) | _names(_substitute_fix(lp.bound, env))) & set(by_var)
            if not more <= need:
                need |= more
                grew = True
    ints = int_env(env)
    combos: list[dict[str, int]] = [{}]
    for lp in loops:                      # outermost first
        if lp.var not in need:
            continue
        nxt = []
        for c in combos:
            vals = V.loop_values(lp, {**ints, **c}, LOOP_LIMIT)
            if vals is None:
                return []
            nxt += [dict(c, **{lp.var: v}) for v in vals]
        combos = nxt[:COMBO_LIMIT]
    out, seen = [], set()
    for c in combos:
        args = resolve_args(ins.args, dict(env, **{k: str(v) for k, v in c.items()}), functions)
        if args is not None and args not in seen:
            seen.add(args)
            out.append((args, "[" + ", ".join(f"{k}={v}" for k, v in c.items()) + "]"))
    return out


# ------------------------------------------------------------------------------------------ probed tier

_ASSERT_RE = re.compile(r"\bassert\s*\(")
_BOUND_RE = re.compile(r"^\s*(.+?)\s*(<=|<|>=|>)\s*(.+?)\s*$", re.S)


def assert_bounds(t: cs.Template) -> dict[str, tuple[int | None, int | None, list[str]]]:
    """Per parameter (lower, upper, asserts): bounds stated by the template's own top-level asserts, from
    conjuncts ``p <= K``, ``p < K``, ``p >= K``, ``p > K`` (either side) with a literal ``K``."""
    depth = _depths(t.body)
    out: dict[str, list] = {p: [None, None, []] for p in t.params}
    for m in _ASSERT_RE.finditer(t.body):
        if depth[m.start()] != (0, 0):
            continue
        pc = cs.match_bracket(t.body, m.end() - 1)
        if pc < 0:
            continue
        text = t.body[m.end():pc]
        conj = [c for c in re.split(r"&&", text)]
        if "||" in text:
            continue
        for c in conj:
            c = c.strip()
            while c.startswith("(") and cs.match_bracket(c, 0) == len(c) - 1:
                c = c[1:-1].strip()
            bm = _BOUND_RE.match(c)
            if not bm:
                continue
            lhs, op, rhs = bm.groups()
            if lhs in out and V.eval_int(rhs) is not None:
                p, k = lhs, V.eval_int(rhs)
            elif rhs in out and V.eval_int(lhs) is not None:
                p, k = rhs, V.eval_int(lhs)
                op = {"<": ">", "<=": ">=", ">": "<", ">=": "<="}[op]
            else:
                continue
            lo, hi, src = out[p]
            if op in ("<", "<="):
                bound = k - 1 if op == "<" else k
                hi = bound if hi is None else min(hi, bound)
            else:
                bound = k + 1 if op == ">" else k
                lo = bound if lo is None else max(lo, bound)
            src.append(f"assert({text.strip()})")
            out[p] = [lo, hi, src]
    return {p: (lo, hi, src) for p, (lo, hi, src) in out.items()}


def probe_candidates(t: cs.Template) -> list[tuple[tuple[str, ...], str]]:
    """``probed`` tier: every parameter needs an upper bound from the template's own asserts (lower bound
    from asserts, else 1); values come from :data:`PROBE_VALUES` inside the bounds; at most
    :data:`MAX_DERIVED_PER_TEMPLATE` combinations (parameter order, smallest first)."""
    if not t.params:
        return []
    bounds = assert_bounds(t)
    ranges = []
    for p in t.params:
        lo, hi, _ = bounds[p]
        if hi is None:
            return []
        lo = 1 if lo is None else lo
        vals = [v for v in PROBE_VALUES if lo <= v <= hi]
        if not vals:
            return []
        ranges.append(vals)
    srcs = sorted({s for p in t.params for s in bounds[p][2]})
    note = f"probe values {list(PROBE_VALUES)} within the bounds of the template's own " + "; ".join(srcs)
    out = []
    for combo in itertools.product(*ranges):
        out.append((tuple(str(v) for v in combo), note))
        if len(out) >= MAX_DERIVED_PER_TEMPLATE:
            break
    return out


# ------------------------------------------------------------------------------------------ planning

def plan_templates(files: dict[str, cs.SourceFile], scope: list[str], repo_id: str,
                   config_mains: list | tuple = (), probe: bool = True) -> list[TemplatePlan]:
    """Instantiation candidates, by tier, for every template declared in the ``scope`` files.

    Candidates are tracked for every scanned template (dependency files and test wrappers included), so
    chains from any ``component main`` (in ``.circom`` files or in ``config_mains``) or test wrapper reach
    the in-scope templates through intermediate templates of any file."""
    in_scope = [t for p in scope for t in files[p].templates]
    all_templates = [t for sf in files.values() for t in sf.templates]
    all_names = {t.name for t in all_templates}
    functions = function_names(files)
    cands: dict[tuple[str, str], dict[str, dict[tuple[str, ...], Candidate]]] = {t.key: {} for t in all_templates}

    def add(t: cs.Template | None, tier: str, args: tuple[str, ...], prov: str) -> bool:
        if t is None or t.key not in cands:
            return False
        if len(args) != len(t.params):
            return False
        slot = cands[t.key].setdefault(tier, {})
        if args in slot:
            if prov not in slot[args].provenance:
                slot[args].provenance.append(prov)
            return False
        slot[args] = Candidate(tier, args, [prov])
        return True

    for t in all_templates:
        if not t.params:
            add(t, "parameter-free", (), f"{t.path}:{t.line} {t.name}()")

    for path, sf in files.items():
        for m in sf.mains:
            t = cs.resolve_template(files, path, m.template)
            args = resolve_args(m.args, {}, functions)
            if args is not None:
                add(t, "repo-main", args, f"{path}:{m.line} component main = {m.template}({', '.join(m.args)})")
    for c in config_mains:
        sf = files.get(c.target)
        t = next((x for x in sf.templates if x.name == c.template), None) if sf else None
        add(t, "repo-test" if c.kind == "js-test" else "repo-main", tuple(c.args),
            f"{c.source}:{c.line} {c.kind}: {c.template}({', '.join(c.args)})")

    instantiations = {path: cs.find_instantiations(sf, all_names) for path, sf in files.items()}
    templates_by_key = {t.key: t for t in all_templates}

    const_cache: dict[tuple[str, str], dict[str, str]] = {}
    for path, sf in files.items():
        harness = sf.is_harness or path.startswith("test/")
        for ins in instantiations[path]:
            t = cs.resolve_template(files, path, ins.template)
            enclosing = next(e for e in sf.templates if e.name == ins.enclosing)
            prov = f"{path}:{ins.line} in {ins.enclosing}: {ins.template}({', '.join(ins.args)})"
            if harness:
                if enclosing.params:
                    continue
                if enclosing.key not in const_cache:
                    const_cache[enclosing.key] = constant_bindings(enclosing.body)
                args = resolve_args(ins.args, const_cache[enclosing.key], functions)
                if args is not None:
                    add(t, "repo-test", args, prov)
            elif all(cs.is_literal_arg(a) for a in ins.args):
                add(t, "repo-internal", cs.normalize_args(ins.args), prov)

    # repo-derived: chains of concrete instantiations.  A call site inside a template E with grounded
    # candidates (any file, test wrappers included) gets E's parameters, bindings and loop values
    # substituted; rounds repeat until nothing new is derived.
    def grounded(key: tuple[str, str]) -> list[Candidate]:
        tiers = cands.get(key, {})
        for tier in TIERS[:4]:
            if tiers.get(tier):
                return sorted(tiers[tier].values(), key=lambda c: c.args)
        return sorted(tiers.get("repo-derived", {}).values(), key=lambda c: c.args)

    for _ in range(DERIVATION_ROUNDS):
        changed = False
        for path, sf in files.items():
            for ins in instantiations[path]:
                if all(cs.is_literal_arg(a) for a in ins.args):
                    continue
                t = cs.resolve_template(files, path, ins.template)
                if t is None or t.key not in cands:
                    continue
                if any(cands[t.key].get(tier) for tier in TIERS[:4]):
                    continue
                enclosing = templates_by_key[(path, ins.enclosing)]
                if len(cands[t.key].get("repo-derived", {})) >= MAX_DERIVED_PER_TEMPLATE:
                    continue
                for parent in grounded(enclosing.key):
                    for args, note in site_arg_sets(ins, enclosing, parent.args, functions):
                        prov = (f"{path}:{ins.line} in {enclosing.name}({', '.join(parent.args)}) "
                                f"[{parent.tier}]{(' ' + note) if note else ''}: {ins.template}({', '.join(ins.args)})")
                        if add(t, "repo-derived", args, prov):
                            changed = True
                        if len(cands[t.key]["repo-derived"]) >= MAX_DERIVED_PER_TEMPLATE:
                            break
                    if len(cands[t.key].get("repo-derived", {})) >= MAX_DERIVED_PER_TEMPLATE:
                        break
        if not changed:
            break

    for t in in_scope:
        d = DOCUMENTED_DEFAULTS.get((repo_id, t.name))
        if d:
            for args in d["args"]:
                add(t, "documented-default", tuple(args), f"default ({d['domain_source']}): {d['reason']}")

    if probe:
        for t in in_scope:
            if not any(cands[t.key].values()):
                for args, note in probe_candidates(t):
                    add(t, "probed", args, note)

    plans = []
    for t in in_scope:
        tiers = {tier: sorted(v.values(), key=lambda c: c.args) for tier, v in cands[t.key].items() if v}
        reason = None
        if not tiers:
            reason = ("parametric template with no repository-grounded instantiation (no component main, "
                      "test wrapper, literal library use or derivable use) and no documented parameter domain"
                      + ("; its own asserts do not bound every parameter, so it is not probed" if probe else ""))
        plans.append(TemplatePlan(t, tiers, reason))
    return plans


def main_source(template_path_abs: str, template: str, args: tuple[str, ...], pragma: str | None = "2.0.0",
                comment: str | None = None, custom_templates: bool = False) -> str:
    """The generated main file for one instantiation (``comment``: an optional leading line;
    ``custom_templates``: the included files declare ``pragma custom_templates``, which circom
    accepts from language version 2.0.6 on; ``pragma=None``: circom 1, which has no pragma)."""
    if pragma is not None and custom_templates and tuple(int(x) for x in pragma.split(".")) < (2, 0, 6):
        pragma = "2.0.6"
    return ((f"// {comment}\n" if comment else "") +
            (f'pragma circom {pragma};\n' if pragma is not None else "") +
            ("pragma custom_templates;\n" if custom_templates else "") +
            f'include "{template_path_abs}";\n'
            f'component main = {template}({", ".join(args)});\n')
