"""Light ZoKrates (``.zok``) source scanner: function declarations, imports, call sites and the
compile-time integer facts needed to ground a generic function's const parameters from the
repository's own call sites (the ``.zok`` analogue of :mod:`circom_source` / :mod:`circom_eval`).

Two source eras are in the ledger population: the current brace-delimited syntax (curly braces,
``;``-terminated statements, ``from "path" import Name;`` / ``import "path" as Name;``, introduced in
ZoKrates 0.8.0) and the older colon/indentation syntax of ZoKrates <= 0.7.x (``def main(...):`` with a
tab/space-indented body, ``import "path" as Name;`` only, no ``<generics>``).  :func:`find_def` and
:func:`function_body` detect the era from the character after the parameter list (``{`` or ``:``) and
handle both; the comma/bracket helpers and call-site scanner are era-independent text operations.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

_IDENT = r"[A-Za-z_][A-Za-z0-9_]*"


def strip_comments(text: str) -> str:
    """Blank out ``//`` and ``/* */`` comments (length-preserving, so positions still line up)."""
    out = list(text)
    i, n = 0, len(text)
    while i < n:
        if text[i] == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif text[i] == "/" and i + 1 < n and text[i + 1] == "*":
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


def match_bracket(text: str, open_pos: int) -> int:
    """Index of the bracket/paren/brace matching ``text[open_pos]``, or -1."""
    opens, closes = "([{", ")]}"
    o = text[open_pos]
    c = closes[opens.index(o)]
    depth = 0
    for i in range(open_pos, len(text)):
        if text[i] == o:
            depth += 1
        elif text[i] == c:
            depth -= 1
            if depth == 0:
                return i
    return -1


def split_top_level(text: str, sep: str = ",") -> list[str]:
    """Split on ``sep`` outside of ``()``/``[]``/``{}``; empty for blank input."""
    text = text.strip()
    if not text:
        return []
    depth, start, out = 0, 0, []
    for i, ch in enumerate(text):
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == sep and depth == 0:
            out.append(text[start:i].strip())
            start = i + 1
    out.append(text[start:].strip())
    return out


# ------------------------------------------------------------------------------------------ declarations

@dataclass
class Param:
    type: str              # e.g. "u32[K][16]"; generic names appear as bare identifiers in it
    name: str
    private: bool = True   # explicit `private`; `public` or unmarked -> False (entry-point default)


@dataclass
class FuncDecl:
    name: str
    generics: list[str]
    params: list[Param]
    ret: str                # return type text, "" for none; a tuple keeps its own "(A, B)" text
    header_end: int         # index just after the opening `{` or the `:` + its newline
    body: str
    style: str               # "brace" | "colon"
    line: int


def _header_re(name: str) -> re.Pattern:
    return re.compile(r"(?m)^[ \t]*def\s+" + re.escape(name) + r"\s*(?:<([^>]*)>)?\s*\(")


def find_def(text: str, name: str, line: int | None = None) -> FuncDecl | None:
    """The top-level ``def name`` in ``text`` (comments stripped first); None if absent.  Several
    declarations may share ``name`` (ZoKrates overloads, e.g. the stdlib's many ``cast`` functions,
    disambiguated by the ledger's ``name@L<line>`` symbol form): with ``line`` given, the one whose
    ``def`` keyword starts at that source line (1-indexed, in the ORIGINAL ``text``, comments included
    since stripping only blanks them without removing lines); with ``line`` None, the first and only
    match (an error upstream if the name is actually overloaded)."""
    clean = strip_comments(text)
    matches = list(_header_re(name).finditer(clean))
    if line is not None:
        matches = [mm for mm in matches if clean.count("\n", 0, mm.start()) + 1 == line]
    if len(matches) != 1:
        return None
    m = matches[0]
    line = clean.count("\n", 0, m.start()) + 1
    popen = clean.index("(", m.end() - 1)
    pclose = match_bracket(clean, popen)
    if pclose < 0:
        return None
    params_text = clean[popen + 1:pclose]
    j = pclose + 1
    while j < len(clean) and clean[j].isspace():
        j += 1
    ret = ""
    if clean.startswith("->", j):
        j += 2
        k = j
        while k < len(clean) and clean[k] not in "{:":
            k += 1
        ret = clean[j:k].strip()
        j = k
    if j >= len(clean) or clean[j] not in "{:":
        return None
    style = "brace" if clean[j] == "{" else "colon"
    if style == "brace":
        close = match_bracket(clean, j)
        if close < 0:
            return None
        body = clean[j + 1:close]
        header_end = close + 1
    else:
        nl = clean.find("\n", j)
        nl = len(clean) if nl < 0 else nl + 1
        body_start = nl
        i = body_start
        while i < len(clean):
            line_end = clean.find("\n", i)
            line_end = len(clean) if line_end < 0 else line_end
            ln = clean[i:line_end]
            if ln.strip() and not ln[:1].isspace():
                break
            i = line_end + 1
        body = clean[body_start:i]
        header_end = i
    generics = [g.strip() for g in (m.group(1) or "").split(",") if g.strip()]
    params = _parse_params(params_text)
    return FuncDecl(name, generics, params, ret, header_end, body, style, line)


_PARAM_LEAD = re.compile(r"^(private|public)\s+")


def _parse_params(text: str) -> list[Param]:
    """``[private|public] TYPE [mut] NAME`` (``mut``: ZoKrates >= 0.8.0, a parameter the function body
    may reassign; irrelevant to a caller, so it is parsed and dropped, not carried in ``Param.type``)."""
    out = []
    for p in split_top_level(text):
        if not p:
            continue
        m = _PARAM_LEAD.match(p)
        private = bool(m and m.group(1) == "private")
        rest = p[m.end():].strip() if m else p
        parts = rest.rsplit(None, 1)
        if len(parts) != 2:
            continue
        head, name = parts
        head_parts = head.rsplit(None, 1)
        ty = head_parts[0] if len(head_parts) == 2 and head_parts[1] == "mut" else head
        out.append(Param(ty.strip(), name.strip(), private=private))
    return out


def substitute_identifiers(text: str, env: dict[str, str]) -> str:
    """Replace whole-word occurrences of every key of ``env`` by its value (generic name -> literal)."""
    if not env:
        return text
    pat = re.compile(r"(?<![A-Za-z0-9_])(" + "|".join(re.escape(k) for k in env) + r")(?![A-Za-z0-9_])")
    return pat.sub(lambda m: env[m.group(1)], text)


# ------------------------------------------------------------------------------------------ imports

@dataclass
class Import:
    module: str             # "ecc/babyjubjubParams", "EMBED", "./util", "utils/casts/u32_to_bits"
    names: list[tuple[str, str]]   # (imported name, local alias); for `import X as Y`, [("main", "Y")]
    line: int


_FROM_IMPORT = re.compile(r'(?m)^[ \t]*from\s+"([^"]+)"\s+import\s+([^;]+);')
_IMPORT_AS = re.compile(r'(?m)^[ \t]*import\s+"([^"]+)"(?:\s+as\s+(' + _IDENT + r'))?\s*;?\s*$')


def find_imports(text: str) -> list[Import]:
    clean = strip_comments(text)
    out = []
    for m in _FROM_IMPORT.finditer(clean):
        names = []
        for item in m.group(2).split(","):
            item = item.strip()
            if not item:
                continue
            if " as " in item:
                orig, alias = item.split(" as ", 1)
                names.append((orig.strip(), alias.strip()))
            else:
                names.append((item, item))
        out.append(Import(m.group(1), names, clean.count("\n", 0, m.start()) + 1))
    for m in _IMPORT_AS.finditer(clean):
        alias = m.group(2) or m.group(1).rsplit("/", 1)[-1]
        out.append(Import(m.group(1), [("main", alias)], clean.count("\n", 0, m.start()) + 1))
    return out


# ------------------------------------------------------------------------------------------ call sites

@dataclass
class CallSite:
    turbofish: list[str] | None     # explicit `name::<...>` generics, parsed text, or None
    args: list[str]
    pos: int


def find_calls(text: str, name: str) -> list[CallSite]:
    """Every ``name(...)`` / ``name::<...>(...)`` call in ``text`` (comments stripped); a definition's
    own header (``def name(``) is excluded by requiring the match not be preceded by ``def\\s+``."""
    clean = strip_comments(text)
    out = []
    for m in re.finditer(r"(?<![A-Za-z0-9_.])" + re.escape(name) + r"(?:::<([^>]*)>)?\s*\(", clean):
        pre = clean[:m.start()]
        if re.search(r"\bdef\s*$", pre[-40:]):
            continue
        popen = m.end() - 1
        close = match_bracket(clean, popen)
        if close < 0:
            continue
        tf = [g.strip() for g in m.group(1).split(",")] if m.group(1) else None
        out.append(CallSite(tf, split_top_level(clean[popen + 1:close]), m.start()))
    return out


_INT_LIT = re.compile(r"^\s*(\d+)\s*$")


def eval_int_literal(expr: str) -> int | None:
    m = _INT_LIT.match(expr)
    return int(m.group(1)) if m else None


def array_literal_len(expr: str) -> int | None:
    """Outer length of an array-literal expression: ``[a, b, c]`` -> 3; ``[V; N]`` (repeat) -> N (N a
    literal integer); ``[[...]; N]`` likewise.  None when ``expr`` is not an (outer) array literal."""
    expr = expr.strip()
    if not expr.startswith("[") or match_bracket(expr, 0) != len(expr) - 1:
        return None
    inner = expr[1:-1]
    parts = split_top_level(inner, ";")
    if len(parts) == 2:
        return eval_int_literal(parts[1])
    return len(split_top_level(inner)) if inner.strip() else 0


def generics_from_call(decl: FuncDecl, call: CallSite) -> dict[str, int] | None:
    """Concrete values for every one of ``decl.generics`` from one call site: explicit turbofish first,
    else the outer length of an array-literal argument at a parameter position whose type is
    ``<elem>[GEN]`` (direct use; one level of nesting in the type is matched: ``T[GEN][k]`` against a
    literal ``[[...]; N]`` / ``[e0, e1, ...]``).  None if any generic cannot be determined."""
    if call.turbofish is not None:
        if len(call.turbofish) != len(decl.generics):
            return None
        vals = [eval_int_literal(g) for g in call.turbofish]
        if any(v is None for v in vals):
            return None
        return dict(zip(decl.generics, vals))
    if len(call.args) != len(decl.params):
        return None
    found: dict[str, int] = {}
    for param, arg in zip(decl.params, call.args):
        for gen in decl.generics:
            if gen in found:
                continue
            if not re.search(r"(?<![A-Za-z0-9_])" + re.escape(gen) + r"(?![A-Za-z0-9_])\s*[\[\]]", "[" + param.type + "]"):
                continue
            if not re.match(r"^\s*" + re.escape(gen) + r"\b", param.type) and param.type.split("[")[0].strip() != "" \
                    and not re.match(r"^[^\[]*\[\s*" + re.escape(gen) + r"\s*\]", param.type):
                continue
            n = array_literal_len(arg.strip())
            if n is not None:
                found[gen] = n
    if all(g in found for g in decl.generics):
        return found
    return None


# ------------------------------------------------------------------------------------------ probed bounds

_ASSERT_CALL = re.compile(r"\bassert\s*\(")
_BOUND_CONJUNCT = re.compile(r"^\s*(.+?)\s*(<=|<|>=|>)\s*(.+?)\s*$")


def assert_bounds(body: str, generics: list[str]) -> dict[str, tuple[int | None, int | None]]:
    """Per generic name, (lower, upper) bounds from the function body's own ``assert`` calls, over
    ``&&``-joined conjuncts ``g <= K`` / ``g < K`` / ``g >= K`` / ``g > K`` (either side; literal ``K``;
    e.g. the stdlib's ``assert(N > 0 && N <= 6);``)."""
    out = {g: [None, None] for g in generics}
    for am in _ASSERT_CALL.finditer(body):
        close = match_bracket(body, am.end() - 1)
        if close < 0:
            continue
        inner = body[am.end():close]
        if "||" in inner:
            continue
        for conjunct in inner.split("&&"):
            bm = _BOUND_CONJUNCT.match(conjunct)
            if not bm:
                continue
            lhs, op, rhs = bm.groups()
            if lhs.strip() in out and eval_int_literal(rhs) is not None:
                g, k = lhs.strip(), eval_int_literal(rhs)
            elif rhs.strip() in out and eval_int_literal(lhs) is not None:
                g, k = rhs.strip(), eval_int_literal(lhs)
                op = {"<": ">", "<=": ">=", ">": "<", ">=": "<="}[op]
            else:
                continue
            lo, hi = out[g]
            if op in ("<", "<="):
                bound = k - 1 if op == "<" else k
                out[g][1] = bound if hi is None else min(hi, bound)
            else:
                bound = k + 1 if op == ">" else k
                out[g][0] = bound if lo is None else max(lo, bound)
    return {g: (lo, hi) for g, (lo, hi) in out.items()}
