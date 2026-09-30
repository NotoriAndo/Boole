"""Content identity of a circom template, for deduplication by content and parameters.

The content hash of a template is the sha256 over the sorted, normalized declarations it reaches:
the template itself plus every template, function and bus it references by name, transitively,
resolved through its file's include closure.  Normalization removes comments and canonicalizes
whitespace.  Two templates with the same hash compile to the same circuit for the same parameters
and prime (the wave-1 ledger deduplication used the same definition).  Parameters are compared by
their integer value where they evaluate to one, otherwise by their whitespace-free text.
"""
from __future__ import annotations

import hashlib
import re

from . import circom_eval as V
from . import circom_source as cs

_FUNC_RE = re.compile(r"\bfunction\s+(" + cs._IDENT + r")\s*\(")
_BUS_RE = re.compile(r"\bbus\s+(" + cs._IDENT + r")\s*\(")
_IDENT_CALL = re.compile(r"(?<![A-Za-z0-9_$.])(" + cs._IDENT + r")\s*\(")
_WORD = re.compile(r"(?<![A-Za-z0-9_$.])(" + cs._IDENT + r")(?![A-Za-z0-9_$])")
_PUNCT = re.compile(r"\s*([^A-Za-z0-9_$\s])\s*")


def normalize(text: str) -> str:
    """Comment-free text with whitespace canonicalized: runs collapse to one space, and spaces next to
    punctuation are removed (token boundaries between identifiers / numbers are kept)."""
    return _PUNCT.sub(r"\1", re.sub(r"\s+", " ", text).strip())


def declarations(sf: cs.SourceFile) -> dict[str, list[tuple[str, str, str]]]:
    """name -> [(kind, normalized declaration, body)] for the templates, functions and buses of a file."""
    out: dict[str, list[tuple[str, str, str]]] = {}
    clean = sf.clean
    for kind, rx in (("template", cs._TEMPLATE_RE), ("function", _FUNC_RE), ("bus", _BUS_RE)):
        for m in rx.finditer(clean):
            po = m.end() - 1
            pc = cs.match_bracket(clean, po)
            if pc < 0:
                continue
            rest = clean[pc + 1:]
            b = len(rest) - len(rest.lstrip())
            if rest[b:b + 1] != "{":
                continue
            bo = pc + 1 + b
            bc = cs.match_bracket(clean, bo)
            if bc < 0:
                continue
            name = m.group(2) if kind == "template" else m.group(1)
            out.setdefault(name, []).append((kind, normalize(clean[m.start():bc + 1]), clean[bo + 1:bc]))
    return out


def content_hash(files: dict, path: str, name: str, cache: dict | None = None) -> str | None:
    """The content hash of template ``name`` declared in ``path`` (None if it is not declared there)."""
    cache = {} if cache is None else cache

    def decls(p: str):
        if p not in cache:
            cache[p] = declarations(files[p])
        return cache[p]

    visible: dict[str, list] = {}
    for p in cs.include_closure(files, path):
        for n, ds in decls(p).items():
            visible.setdefault(n, []).extend(ds)
    own = [d for d in decls(path).get(name, []) if d[0] == "template"]
    if not own:
        return None
    todo, seen, parts = [(name, own[:1])], set(), []
    while todo:
        n, ds = todo.pop()
        if n in seen:
            continue
        seen.add(n)
        for kind, text, body in sorted(set(ds), key=lambda x: (x[0], x[1])):
            parts.append(f"{kind} {text}")
            for m in _IDENT_CALL.finditer(body):
                ref = m.group(1)
                if ref in visible and ref not in seen:
                    todo.append((ref, visible[ref]))
            for m in _WORD.finditer(body):
                ref = m.group(1)
                if ref in visible and ref not in seen and any(k == "bus" for k, _, _ in visible[ref]):
                    todo.append((ref, [d for d in visible[ref] if d[0] == "bus"]))
    return hashlib.sha256("\n".join(sorted(parts)).encode("utf-8")).hexdigest()


def args_key(args) -> tuple[str, ...]:
    """Parameters compared by integer value where they evaluate to one (``3+1`` == ``4``), else by text."""
    out = []
    for a in args:
        v = V.eval_int(a)
        out.append(str(v) if v is not None else re.sub(r"\s+", "", a))
    return tuple(out)


def instance_key(prime: str, content: str, args) -> str:
    """Deduplication key of one instantiation: compilation prime, template content and parameters."""
    return f"{prime}:{content}:{','.join(args_key(args))}"
