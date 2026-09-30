"""Instantiation rules for circom templates.

A DET problem needs a concrete instantiation.  Candidates come from the repository itself, in
this order of rule tiers (the first tier with at least one compilable candidate is used):

``parameter-free``      the template takes no parameters: ``T()``
``repo-main``           a ``component main = T(args)`` declaration anywhere in the repository
``repo-test``           ``T(args)`` inside a test/example wrapper (a file with ``component main``
                        or under ``test/``); ``var`` bindings of the wrapper are substituted
``repo-internal``       ``T(args)`` with literal arguments inside another library template
``repo-derived``        ``T(args)`` inside a library template ``E`` whose own parameters are
                        grounded by an earlier tier; ``E``'s parameters and single-assignment
                        ``var`` bindings are substituted until the arguments are closed
``documented-default``  a small default set from :data:`DOCUMENTED_DEFAULTS`, allowed only when
                        the repository documents the parameter domain (the entry cites it)

Otherwise the template is UNINSTANTIABLE, with the reason recorded.  Within the selected tier the
driver compiles every candidate and keeps the one with the largest constraint count inside the
size policy (ties: fewer wires, then argument text); if none fits, the smallest is reported as
TOO-LARGE.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

from . import circom_source as cs

TIERS = ["parameter-free", "repo-main", "repo-test", "repo-internal", "repo-derived", "documented-default"]
MAX_DERIVED_PER_TEMPLATE = 8
DERIVATION_ROUNDS = 4

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


# ------------------------------------------------------------------------------------------ planning

def plan_templates(files: dict[str, cs.SourceFile], scope: list[str], repo_id: str) -> list[TemplatePlan]:
    """Instantiation candidates, by tier, for every template declared in the ``scope`` files."""
    in_scope = [t for p in scope for t in files[p].templates]
    all_names = {t.name for sf in files.values() for t in sf.templates}
    functions = function_names(files)
    cands: dict[tuple[str, str], dict[str, dict[tuple[str, ...], Candidate]]] = {t.key: {} for t in in_scope}

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

    for t in in_scope:
        if not t.params:
            add(t, "parameter-free", (), f"{t.path}:{t.line} {t.name}()")

    for path, sf in files.items():
        for m in sf.mains:
            t = cs.resolve_template(files, path, m.template)
            args = resolve_args(m.args, {}, functions)
            if args is not None:
                add(t, "repo-main", args, f"{path}:{m.line} component main = {m.template}({', '.join(m.args)})")

    instantiations = {path: cs.find_instantiations(sf, all_names) for path, sf in files.items()}
    templates_by_key = {t.key: t for sf in files.values() for t in sf.templates}

    for path, sf in files.items():
        harness = sf.is_harness or path.startswith("test/")
        for ins in instantiations[path]:
            t = cs.resolve_template(files, path, ins.template)
            enclosing = next(e for e in sf.templates if e.name == ins.enclosing)
            prov = f"{path}:{ins.line} in {ins.enclosing}: {ins.template}({', '.join(ins.args)})"
            if harness:
                if enclosing.params:
                    continue
                args = resolve_args(ins.args, constant_bindings(enclosing.body), functions)
                if args is not None:
                    add(t, "repo-test", args, prov)
            elif all(cs.is_literal_arg(a) for a in ins.args):
                add(t, "repo-internal", cs.normalize_args(ins.args), prov)

    # repo-derived: propagate grounded parameters of an enclosing library template
    def grounded(key: tuple[str, str]) -> list[Candidate]:
        tiers = cands.get(key, {})
        for tier in TIERS[:4]:
            if tiers.get(tier):
                return sorted(tiers[tier].values(), key=lambda c: c.args)
        return sorted(tiers.get("repo-derived", {}).values(), key=lambda c: c.args)

    for _ in range(DERIVATION_ROUNDS):
        changed = False
        for path, sf in files.items():
            if sf.is_harness or path.startswith("test/"):
                continue
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
                    env = dict(constant_bindings(enclosing.body))
                    env.update(zip(enclosing.params, parent.args))
                    args = resolve_args(ins.args, env, functions)
                    if args is None:
                        continue
                    prov = (f"{path}:{ins.line} in {enclosing.name}({', '.join(parent.args)}) "
                            f"[{parent.tier}]: {ins.template}({', '.join(ins.args)})")
                    if add(t, "repo-derived", args, prov):
                        changed = True
                    if len(cands[t.key]["repo-derived"]) >= MAX_DERIVED_PER_TEMPLATE:
                        break
        if not changed:
            break

    for t in in_scope:
        d = DOCUMENTED_DEFAULTS.get((repo_id, t.name))
        if d:
            for args in d["args"]:
                add(t, "documented-default", tuple(args), f"default ({d['domain_source']}): {d['reason']}")

    plans = []
    for t in in_scope:
        tiers = {tier: sorted(v.values(), key=lambda c: c.args) for tier, v in cands[t.key].items() if v}
        reason = None
        if not tiers:
            reason = ("parametric template with no repository-grounded instantiation (no component main, "
                      "test wrapper, literal library use or derivable use) and no documented parameter domain")
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
