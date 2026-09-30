"""Compile-time integer evaluation and ``for``-loop structure of circom template bodies.

Used by the instantiation planner to propagate concrete parameters into sub-component calls:
loop bounds, ``var`` bindings and template parameters are evaluated to integers where that is
possible without running circom.  Anything else (function calls, arrays, field division with a
remainder, signals) evaluates to ``None``; the planner then leaves the expression to circom or
gives up on the call site.  circom remains the authority: every derived instantiation is compiled.
"""
from __future__ import annotations

import re
from dataclasses import dataclass

from . import circom_source as cs

MAX_ABS = 1 << 300          # larger compile-time values are not needed for sizes and loop bounds
MAX_SHIFT = 512

_TOKEN_RE = re.compile(r"\s*(?:(0x[0-9A-Fa-f]+|\d+)|([A-Za-z_$][A-Za-z0-9_$]*)|"
                       r"(\*\*|<<|>>|<=|>=|==|!=|&&|\|\||[-+*/\\%&|^~!<>?:()]))")

_BINARY = {  # operator -> (precedence, right associative)
    "?": (1, True), "||": (2, False), "&&": (3, False), "|": (4, False), "^": (5, False), "&": (6, False),
    "==": (7, False), "!=": (7, False), "<": (8, False), "<=": (8, False), ">": (8, False), ">=": (8, False),
    "<<": (9, False), ">>": (9, False), "+": (10, False), "-": (10, False),
    "*": (11, False), "/": (11, False), "\\": (11, False), "%": (11, False), "**": (13, True),
}
_UNARY_PREC = 12


class _Fail(Exception):
    pass


def _tokens(expr: str) -> list[tuple[str, str]]:
    out, pos = [], 0
    expr = expr.strip()
    while pos < len(expr):
        m = _TOKEN_RE.match(expr, pos)
        if not m or m.end() == pos:
            raise _Fail()
        num, ident, op = m.groups()
        out.append(("num", num) if num else ("id", ident) if ident else ("op", op))
        pos = m.end()
    return out


def _check(v: int) -> int:
    if abs(v) > MAX_ABS:
        raise _Fail()
    return v


def _apply(op: str, a: int, b: int) -> int:
    if op == "+":
        return _check(a + b)
    if op == "-":
        return _check(a - b)
    if op == "*":
        return _check(a * b)
    if op == "/":                     # field division; only exact integer quotients are evaluated
        if b == 0 or a % b:
            raise _Fail()
        return a // b
    if op == "\\":
        if b <= 0 or a < 0:
            raise _Fail()
        return a // b
    if op == "%":
        if b <= 0 or a < 0:
            raise _Fail()
        return a % b
    if op == "**":
        if b < 0 or (abs(a) > 1 and b > 1024):
            raise _Fail()
        return _check(a ** b)
    if op == "<<":
        if b < 0 or b > MAX_SHIFT:
            raise _Fail()
        return _check(a << b)
    if op == ">>":
        if b < 0:
            raise _Fail()
        return a >> b if a >= 0 else _fail()
    if op in ("&", "|", "^"):
        if a < 0 or b < 0:
            raise _Fail()
        return {"&": a & b, "|": a | b, "^": a ^ b}[op]
    if op in ("==", "!=", "<", "<=", ">", ">="):
        return int({"==": a == b, "!=": a != b, "<": a < b, "<=": a <= b, ">": a > b, ">=": a >= b}[op])
    if op == "&&":
        return int(bool(a) and bool(b))
    if op == "||":
        return int(bool(a) or bool(b))
    raise _Fail()


def _fail():
    raise _Fail()


class _Parser:
    def __init__(self, toks: list[tuple[str, str]], env: dict[str, int]):
        self.toks, self.i, self.env = toks, 0, env

    def peek(self):
        return self.toks[self.i] if self.i < len(self.toks) else ("end", "")

    def take(self):
        t = self.peek()
        self.i += 1
        return t

    def expect(self, op: str) -> None:
        if self.take() != ("op", op):
            raise _Fail()

    def primary(self) -> int:
        kind, text = self.take()
        if kind == "num":
            return int(text, 16) if text.startswith("0x") else int(text)
        if kind == "id":
            if self.peek() == ("op", "(") or text not in self.env:
                raise _Fail()             # function calls and unknown names are left to circom
            return self.env[text]
        if (kind, text) == ("op", "("):
            v = self.expr(0)
            self.expect(")")
            return v
        if kind == "op" and text in ("-", "+", "!"):
            v = self.expr(_UNARY_PREC)
            return {"-": -v, "+": v, "!": int(not v)}[text]
        raise _Fail()

    def expr(self, min_prec: int) -> int:
        left = self.primary()
        while True:
            kind, op = self.peek()
            if kind != "op" or op not in _BINARY:
                return left
            prec, right_assoc = _BINARY[op]
            if prec < min_prec:
                return left
            self.take()
            if op == "?":
                a = self.expr(0)
                self.expect(":")
                b = self.expr(prec)
                left = a if left else b
                continue
            right = self.expr(prec if right_assoc else prec + 1)
            left = _apply(op, left, right)


def eval_int(expr: str, env: dict[str, int] | None = None) -> int | None:
    """The integer value of a circom compile-time expression, or ``None`` when it cannot be evaluated here."""
    try:
        p = _Parser(_tokens(expr), env or {})
        v = p.expr(0)
        if p.peek()[0] != "end":
            return None
        return v
    except (_Fail, RecursionError, ValueError, OverflowError):
        return None


# ------------------------------------------------------------------------------------------ loops

@dataclass
class Loop:
    var: str | None          # None: the header is not a simple counting loop
    start: str
    cmp: str
    bound: str
    step: int
    header_pos: int
    body_start: int
    body_end: int

    def contains(self, pos: int) -> bool:
        return self.body_start <= pos < self.body_end


_FOR_RE = re.compile(r"\bfor\s*\(")
_IDENT = cs._IDENT


def _parse_header(header: str) -> tuple[str, str, str, str, int] | None:
    parts = cs.split_top_level(header, ";")
    if len(parts) != 3:
        return None
    init, cond, upd = (p.strip() for p in parts)
    m = re.fullmatch(r"(?:var\s+)?(" + _IDENT + r")\s*=\s*(.+)", init, re.S)
    if not m:
        return None
    var, start = m.group(1), m.group(2).strip()
    m = re.fullmatch(re.escape(var) + r"\s*(<=|<|>=|>|!=)\s*(.+)", cond, re.S)
    if not m:
        return None
    cmp, bound = m.group(1), m.group(2).strip()
    v = re.escape(var)
    step = None
    for pat, sign in ((v + r"\s*\+\+|\+\+\s*" + v, 1), (v + r"\s*--|--\s*" + v, -1)):
        if re.fullmatch(pat, upd):
            step = sign
    m = re.fullmatch(v + r"\s*(\+=|-=)\s*(\d+)", upd) or re.fullmatch(v + r"\s*=\s*" + v + r"\s*([+-])\s*(\d+)", upd)
    if m:
        step = int(m.group(2)) * (-1 if m.group(1).startswith("-") else 1)
    if not step:
        return None
    return var, start, cmp, bound, step


def parse_loops(body: str) -> list[Loop]:
    """Every ``for`` statement of a comment-free template body, with its body extent."""
    loops = []
    for m in _FOR_RE.finditer(body):
        po = m.end() - 1
        pc = cs.match_bracket(body, po)
        if pc < 0:
            continue
        rest = body[pc + 1:]
        k = pc + 1 + (len(rest) - len(rest.lstrip()))
        if body[k:k + 1] == "{":
            end = cs.match_bracket(body, k)
            if end < 0:
                continue
            end += 1
        else:
            end = body.find(";", k)
            if end < 0:
                continue
            end += 1
        hdr = _parse_header(body[po + 1:pc])
        if hdr:
            loops.append(Loop(*hdr, header_pos=m.start(), body_start=k, body_end=end))
        else:
            loops.append(Loop(None, "", "", "", 0, m.start(), k, end))
    return loops


def enclosing_loops(loops: list[Loop], pos: int) -> list[Loop]:
    return sorted((lp for lp in loops if lp.contains(pos)), key=lambda lp: lp.body_start)


def loop_values(loop: Loop, env: dict[str, int], limit: int) -> list[int] | None:
    """The values the loop variable takes (at most ``limit``), or None if they cannot be evaluated."""
    if loop.var is None:
        return None
    start = eval_int(loop.start, env)
    if start is None:
        return None
    out, v = [], start
    while True:
        bound = eval_int(loop.bound, dict(env, **{loop.var: v}))
        if bound is None:
            return None
        ok = {"<": v < bound, "<=": v <= bound, ">": v > bound, ">=": v >= bound, "!=": v != bound}[loop.cmp]
        if not ok:
            return out
        out.append(v)
        if len(out) >= limit:
            return out
        v += loop.step
