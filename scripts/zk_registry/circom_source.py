"""Light circom source scanner.

This is not a circom parser.  It finds, in comment-free text: ``include`` directives, template
declarations with their parameter lists and bodies, the main component's signal declarations,
``component main`` declarations and template instantiations with literal arguments.  The circom
compiler remains the authority: every instantiation found here is compiled, and the compiled
``.sym`` / ``.r1cs`` are cross-checked against what the scanner reports.
"""
from __future__ import annotations

import os
import re
from dataclasses import dataclass, field

_IDENT = r"[A-Za-z_$][A-Za-z0-9_$]*"


_LEXEME_RE = re.compile(r'//[^\n]*|/\*.*?(?:\*/|\Z)|"(?:\\.|[^"\\])*"?', re.S)


def strip_comments(src: str) -> str:
    """Blank ``//`` and ``/* */`` comments (newlines kept, so offsets and line numbers survive).

    String literals are kept verbatim (include paths live in them) and are skipped while looking
    for comment starts."""
    def blank(m: re.Match) -> str:
        t = m.group(0)
        if t.startswith('"'):
            return t
        return re.sub(r"[^\n]", " ", t)
    return _LEXEME_RE.sub(blank, src)


def match_bracket(text: str, open_pos: int) -> int:
    """Index of the bracket closing the one at ``open_pos`` (``(``/``[``/``{``); -1 if unbalanced."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    opener = text[open_pos]
    closer = pairs[opener]
    depth = 0
    i = open_pos
    while i < len(text):
        ch = text[i]
        if ch == '"':
            j = text.find('"', i + 1)
            i = len(text) if j < 0 else j + 1
            continue
        if ch == opener:
            depth += 1
        elif ch == closer:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def split_top_level(text: str, sep: str = ",") -> list[str]:
    parts, depth, cur = [], 0, []
    for ch in text:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == sep and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur)
    if tail.strip() or parts:
        parts.append(tail)
    return [p.strip() for p in parts]


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


# ------------------------------------------------------------------------------------------ templates

@dataclass
class SignalDecl:
    direction: str          # "input" | "output" | "intermediate"
    name: str
    dims: list[str]         # dimension expressions, outermost first


@dataclass
class Template:
    path: str               # repository-relative path of the declaring file
    name: str
    params: list[str]
    kind: str               # "" | "parallel" | "custom"
    line: int
    body: str               # comment-free body text (without the braces)

    @property
    def key(self) -> tuple[str, str]:
        return (self.path, self.name)


_TEMPLATE_RE = re.compile(r"\btemplate\s+(?:(parallel|custom)\s+)?(" + _IDENT + r")\s*\(")
_SIGNAL_RE = re.compile(r"\bsignal\b")


def find_templates(clean: str, path: str) -> list[Template]:
    """Template declarations in comment-free text ``clean`` of the file at ``path``."""
    out = []
    for m in _TEMPLATE_RE.finditer(clean):
        po = m.end() - 1
        pc = match_bracket(clean, po)
        if pc < 0:
            continue
        rest = clean[pc + 1:]
        brace = len(rest) - len(rest.lstrip())
        if not rest[brace:brace + 1] == "{":
            continue
        bo = pc + 1 + brace
        bc = match_bracket(clean, bo)
        if bc < 0:
            continue
        params = [p for p in split_top_level(clean[po + 1:pc]) if p]
        out.append(Template(path, m.group(2), params, m.group(1) or "", line_of(clean, m.start()),
                            clean[bo + 1:bc]))
    return out


def signal_declarations(body: str) -> list[SignalDecl]:
    """``signal [input|output] [{tags}] name[dims] [, name2[dims]] ...`` statements in a template body.

    Declarations with an initializer (``signal output x <== e;``) are handled: the initializer is
    dropped."""
    decls = []
    for m in _SIGNAL_RE.finditer(body):
        end = body.find(";", m.end())
        if end < 0:
            continue
        stmt = body[m.end():end].strip()
        direction = "intermediate"
        dm = re.match(r"(input|output)\b", stmt)
        if dm:
            direction = dm.group(1)
            stmt = stmt[dm.end():].strip()
        if stmt.startswith("{"):
            close = match_bracket(stmt, 0)
            stmt = stmt[close + 1:].strip()
        stmt = re.split(r"<==|<--|==>|-->|=", stmt, maxsplit=1)[0]
        for part in split_top_level(stmt):
            nm = re.match(r"(" + _IDENT + r")\s*(.*)$", part, re.S)
            if not nm:
                continue
            dims = []
            rest = nm.group(2).strip()
            while rest.startswith("["):
                close = match_bracket(rest, 0)
                if close < 0:
                    break
                dims.append(rest[1:close].strip())
                rest = rest[close + 1:].strip()
            decls.append(SignalDecl(direction, nm.group(1), dims))
    return decls


# ------------------------------------------------------------------------------------------ files

_INCLUDE_RE = re.compile(r'\binclude\s+"([^"]+)"\s*;')
_MAIN_RE = re.compile(r"\bcomponent\s+main\s*(\{[^}]*\})?\s*=\s*(" + _IDENT + r")\s*\(")


@dataclass
class MainDecl:
    path: str
    template: str
    args: list[str]
    public: str
    line: int


@dataclass
class Instantiation:
    """A template call ``Name(args)`` found inside the body of another template."""
    path: str               # file of the enclosing template
    enclosing: str          # enclosing template name
    template: str
    args: list[str]
    line: int


@dataclass
class SourceFile:
    path: str               # repository-relative
    text: str
    clean: str
    includes: list[str] = field(default_factory=list)          # resolved repository-relative paths
    unresolved_includes: list[str] = field(default_factory=list)
    templates: list[Template] = field(default_factory=list)
    mains: list[MainDecl] = field(default_factory=list)

    @property
    def is_harness(self) -> bool:
        """A file that declares ``component main`` (an example or test wrapper)."""
        return bool(self.mains)


def _resolve_include(repo_root: str, from_path: str, inc: str, lib_dirs: list[str]) -> str | None:
    candidates = [os.path.normpath(os.path.join(os.path.dirname(from_path), inc))]
    candidates += [os.path.normpath(os.path.join(d, inc)) for d in lib_dirs]
    for c in candidates:
        if not c.startswith("..") and os.path.isfile(os.path.join(repo_root, c)):
            return c
    return None


def scan_file(repo_root: str, rel: str, lib_dirs: list[str] | None = None) -> SourceFile:
    with open(os.path.join(repo_root, rel), encoding="utf-8") as f:
        text = f.read()
    clean = strip_comments(text)
    sf = SourceFile(rel, text, clean)
    for m in _INCLUDE_RE.finditer(clean):
        r = _resolve_include(repo_root, rel, m.group(1), lib_dirs or [])
        (sf.includes if r else sf.unresolved_includes).append(r or m.group(1))
    sf.templates = find_templates(clean, rel)
    for m in _MAIN_RE.finditer(clean):
        po = m.end() - 1
        pc = match_bracket(clean, po)
        if pc < 0:
            continue
        sf.mains.append(MainDecl(rel, m.group(2), [a for a in split_top_level(clean[po + 1:pc]) if a],
                                 (m.group(1) or "").strip(), line_of(clean, m.start())))
    return sf


def blank_mains(text: str) -> str:
    """``text`` with every ``component main ... = T(...);`` declaration blanked (newlines kept).

    Used to include a file that declares both library templates and its own main component: circom
    accepts one main component per compilation, so the generated main includes this copy instead."""
    clean = strip_comments(text)
    out = list(text)
    for m in _MAIN_RE.finditer(clean):
        pc = match_bracket(clean, m.end() - 1)
        end = clean.find(";", pc) if pc >= 0 else -1
        if end < 0:
            continue
        for i in range(m.start(), end + 1):
            if out[i] != "\n":
                out[i] = " "
    return "".join(out)


def scan_repo(repo_root: str, rel_paths: list[str], lib_dirs: list[str] | None = None) -> dict[str, SourceFile]:
    return {rel: scan_file(repo_root, rel, lib_dirs) for rel in sorted(rel_paths)}


def list_circom_files(repo_root: str) -> list[str]:
    out = []
    for dirpath, dirnames, filenames in os.walk(repo_root):
        dirnames[:] = sorted(d for d in dirnames if d not in (".git", "node_modules"))
        for fn in sorted(filenames):
            if fn.endswith(".circom"):
                out.append(os.path.relpath(os.path.join(dirpath, fn), repo_root))
    return sorted(out)


def include_closure(files: dict[str, SourceFile], start: str) -> list[str]:
    seen, order, todo = set(), [], [start]
    while todo:
        p = todo.pop()
        if p in seen or p not in files:
            continue
        seen.add(p)
        order.append(p)
        todo.extend(reversed(files[p].includes))
    return order


def resolve_template(files: dict[str, SourceFile], from_path: str, name: str) -> Template | None:
    """The unique template ``name`` visible from ``from_path`` through its include closure."""
    found = [t for p in include_closure(files, from_path) for t in files[p].templates if t.name == name]
    return found[0] if len(found) == 1 else None


_CALL_RE = re.compile(r"(?<![A-Za-z0-9_$.])(" + _IDENT + r")\s*\(")


def find_instantiations(sf: SourceFile, template_names: set[str]) -> list[Instantiation]:
    """Calls ``T(args)`` of known template names in template bodies, where the call is the
    right-hand side of ``=`` / ``<==`` (named or anonymous components)."""
    out = []
    for t in sf.templates:
        body = t.body
        for m in _CALL_RE.finditer(body):
            name = m.group(1)
            if name not in template_names:
                continue
            before = body[:m.start()].rstrip()
            if not before.endswith("="):
                continue
            po = m.end() - 1
            pc = match_bracket(body, po)
            if pc < 0:
                continue
            args = [a for a in split_top_level(body[po + 1:pc]) if a]
            out.append(Instantiation(sf.path, t.name, name, args, t.line + body.count("\n", 0, m.start())))
    return out


_LITERAL_ARG_RE = re.compile(r"^[0-9\s+\-*/%()]+$")


def is_literal_arg(arg: str) -> bool:
    """A compile-time integer expression built only from decimal literals and + - * / % ( )."""
    a = arg.strip()
    return bool(a) and bool(_LITERAL_ARG_RE.match(a)) and any(ch.isdigit() for ch in a)


def normalize_args(args: list[str]) -> tuple[str, ...]:
    return tuple(re.sub(r"\s+", "", a) for a in args)
