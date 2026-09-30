"""Light circom source scanner.

This is not a circom parser.  It finds, in comment-free text: ``include`` directives, template
declarations with their parameter lists and bodies, the main component's signal declarations,
``component main`` declarations and template instantiations with literal arguments.  The circom
compiler remains the authority: every instantiation found here is compiled, and the compiled
``.sym`` / ``.r1cs`` are cross-checked against what the scanner reports.
"""
from __future__ import annotations

import json
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
    tags: list[str] = field(default_factory=list)       # circom tags, e.g. ["binary"] for `signal input {binary} x`


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
        dm = re.match(r"(?:private\s+)?(input|output)\b", stmt)        # circom 1: `signal private input`
        if dm:
            direction = dm.group(1)
            stmt = stmt[dm.end():].strip()
        tags: list[str] = []
        if stmt.startswith("{"):
            close = match_bracket(stmt, 0)
            tags = [x.strip() for x in stmt[1:close].split(",") if x.strip()]
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
            decls.append(SignalDecl(direction, nm.group(1), dims, list(tags)))
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
    pos: int = -1           # offset of the call in the enclosing template's body


@dataclass
class SourceFile:
    path: str               # repository-relative
    text: str
    clean: str
    includes: list[str] = field(default_factory=list)          # resolved repository-relative paths
    unresolved_includes: list[str] = field(default_factory=list)
    templates: list[Template] = field(default_factory=list)
    mains: list[MainDecl] = field(default_factory=list)
    dependency: bool = False    # reached only through an include into a library path (e.g. node_modules)

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


def scan_repo(repo_root: str, rel_paths: list[str], lib_dirs: list[str] | None = None,
              dependencies: bool = False) -> dict[str, SourceFile]:
    """Scan the given files; with ``dependencies``, also every file their includes reach inside the
    checkout that is not in ``rel_paths`` (library packages such as ``node_modules/circomlib``), marked
    ``dependency`` so that templates they declare can be resolved but are never in scope."""
    files = {rel: scan_file(repo_root, rel, lib_dirs) for rel in sorted(rel_paths)}
    todo = sorted({inc for sf in files.values() for inc in sf.includes if inc not in files}) if dependencies else []
    while todo:
        rel = todo.pop()
        if rel in files or not rel.endswith(".circom"):
            continue
        try:
            sf = scan_file(repo_root, rel, lib_dirs)
        except (OSError, UnicodeDecodeError):
            continue
        sf.dependency = True
        files[rel] = sf
        todo += [inc for inc in sf.includes if inc not in files]
    return dict(sorted(files.items()))


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
            out.append(Instantiation(sf.path, t.name, name, args, t.line + body.count("\n", 0, m.start()), m.start()))
    return out


_LITERAL_ARG_RE = re.compile(r"^[0-9\s+\-*/%()]+$")


def is_literal_arg(arg: str) -> bool:
    """A compile-time integer expression built only from decimal literals and + - * / % ( )."""
    a = arg.strip()
    return bool(a) and bool(_LITERAL_ARG_RE.match(a)) and any(ch.isdigit() for ch in a)


def normalize_args(args: list[str]) -> tuple[str, ...]:
    return tuple(re.sub(r"\s+", "", a) for a in args)


# ------------------------------------------------------------------------------------------ config mains

@dataclass
class ConfigMain:
    """A main component declared outside ``.circom`` sources: a circomkit ``circuits.json`` entry, a circomkit
    ``WitnessTester`` / ``ProofTester`` call in a JS/TS test, or a ``component main`` inside a JS/TS string."""
    source: str             # repository-relative path of the config or script
    line: int
    kind: str               # "circomkit-config" | "js-test" | "js-main"
    template: str
    args: list[str]         # circom argument text (literals only)
    target: str | None      # repository-relative .circom file declaring the template, when it can be located


_SCRIPT_EXT = (".js", ".ts", ".mjs", ".cjs")
_JS_CONST_RE = re.compile(r"\b(?:const|let|var)\s+(" + _IDENT + r")\s*(?::\s*number\s*)?=\s*(-?\d+)\s*[;\n]")
_JS_OBJ_KEY = r"""\b{key}\s*:\s*["'`]([^"'`]+)["'`]"""
_TEST_PATH_RE = re.compile(r"(^|/)(test|tests|__tests__|spec)(/|$)|\.(test|spec)\.[cm]?[jt]s$")


def _json_arg(v) -> str | None:
    if isinstance(v, bool) or v is None:
        return None
    if isinstance(v, int):
        return str(v)
    if isinstance(v, list):
        parts = [_json_arg(x) for x in v]
        return None if any(p is None for p in parts) else "[" + ",".join(parts) + "]"
    return None


def _js_consts(text: str) -> dict[str, str]:
    found: dict[str, list[str]] = {}
    for m in _JS_CONST_RE.finditer(text):
        found.setdefault(m.group(1), []).append(m.group(2))
    return {k: v[0] for k, v in found.items() if len(v) == 1}


def _js_arg(a: str, consts: dict[str, str]) -> str | None:
    a = re.sub(r"\$\{\s*(" + _IDENT + r")\s*\}", lambda m: consts.get(m.group(1), m.group(0)), a.strip())
    a = consts.get(a, a)
    if is_literal_arg(a):
        return re.sub(r"\s+", "", a)
    if re.fullmatch(r"\[\s*-?\d+(\s*,\s*-?\d+)*\s*\]", a):
        return re.sub(r"\s+", "", a)
    return None


def _declares(files: dict, rel: str, template: str) -> bool:
    sf = files.get(os.path.normpath(rel))
    return sf is not None and any(t.name == template for t in sf.templates)


def _unique_declaring_file(files: dict, template: str) -> str | None:
    hits = [p for p, sf in files.items() if not sf.dependency and any(t.name == template for t in sf.templates)]
    return hits[0] if len(hits) == 1 else None


def _circomkit_bases(repo_root: str, start_dir: str) -> list[str]:
    """Candidate circuit directories of a circomkit project enclosing ``start_dir`` (repository-relative)."""
    out, cur = [], start_dir
    while True:
        cfg = os.path.join(repo_root, cur, "circomkit.json")
        if os.path.isfile(cfg):
            try:
                with open(cfg, encoding="utf-8") as f:
                    d = json.load(f)
            except ValueError:
                d = {}
            out.append(os.path.normpath(os.path.join(cur, d.get("dirCircuits", "./circuits"))))
            out.append(os.path.normpath(cur))
            break
        if cur in ("", "."):
            break
        cur = os.path.dirname(cur)
    return out


def _target(files: dict, bases: list[str], file_ref: str, template: str) -> str | None:
    for b in bases:
        rel = os.path.normpath(os.path.join(b, file_ref if file_ref.endswith(".circom") else file_ref + ".circom"))
        if _declares(files, rel, template):
            return rel
    return None


def scan_config_mains(repo_root: str, files: dict) -> list[ConfigMain]:
    """Main components declared by the repository outside ``.circom`` files (see :class:`ConfigMain`).

    Only literal parameters are taken (JSON numbers and arrays; in scripts, numbers or ``${NAME}`` /
    ``NAME`` where ``NAME`` is a single ``const``/``let``/``var`` numeric literal of the same file)."""
    out: list[ConfigMain] = []
    for dirpath, dirnames, filenames in os.walk(repo_root):
        dirnames[:] = sorted(d for d in dirnames if d not in (".git", "node_modules"))
        for fn in sorted(filenames):
            full = os.path.join(dirpath, fn)
            rel = os.path.relpath(full, repo_root)
            if fn == "circuits.json" or fn.endswith(_SCRIPT_EXT):
                try:
                    if os.path.getsize(full) > 2_000_000:
                        continue
                    with open(full, encoding="utf-8", errors="replace") as f:
                        text = f.read()
                except OSError:
                    continue
            else:
                continue
            here = os.path.dirname(rel)
            if fn == "circuits.json":
                try:
                    data = json.loads(text)
                except ValueError:
                    continue
                if not isinstance(data, dict):
                    continue
                bases = _circomkit_bases(repo_root, here) + [here]
                for name, entry in data.items():
                    if not isinstance(entry, dict) or not isinstance(entry.get("template"), str):
                        continue
                    params = entry.get("params", [])
                    args = [_json_arg(v) for v in params] if isinstance(params, list) else [None]
                    if any(a is None for a in args):
                        continue
                    target = _target(files, bases, str(entry.get("file", "")), entry["template"])
                    if target:
                        line = line_of(text, max(text.find(json.dumps(name)), 0))
                        out.append(ConfigMain(rel, line, "circomkit-config", entry["template"], args, target))
                continue
            consts = _js_consts(text)
            bases = _circomkit_bases(repo_root, here) + [here]
            for m in re.finditer(_JS_OBJ_KEY.format(key="template"), text):
                lo = text.rfind("{", 0, m.start())
                hi = match_bracket(text, lo) if lo >= 0 else -1
                if hi < 0:
                    continue
                obj = text[lo:hi + 1]
                fm = re.search(_JS_OBJ_KEY.format(key="file"), obj)
                pm = re.search(r"\bparams\s*:\s*\[", obj)
                args: list[str | None] = []
                if pm:
                    close = match_bracket(obj, pm.end() - 1)
                    if close < 0:
                        continue
                    args = [_js_arg(a, consts) for a in split_top_level(obj[pm.end():close]) if a]
                if not fm or any(a is None for a in args):
                    continue
                target = _target(files, bases, fm.group(1), m.group(1))
                if target:
                    out.append(ConfigMain(rel, line_of(text, m.start()), "js-test", m.group(1), args, target))
            for m in _MAIN_RE.finditer(text):
                po = m.end() - 1
                pc = match_bracket(text, po)
                if pc < 0:
                    continue
                args = [_js_arg(a, consts) for a in split_top_level(text[po + 1:pc]) if a]
                if any(a is None for a in args):
                    continue
                template = m.group(2)
                target = None
                start = max(text.rfind("`", 0, m.start()), 0)
                for inc in _INCLUDE_RE.finditer(text, start, m.start()):
                    cand = os.path.normpath(os.path.join(here, inc.group(1)))
                    if _declares(files, cand, template):
                        target = cand
                target = target or _unique_declaring_file(files, template)
                if target:
                    kind = "js-test" if _TEST_PATH_RE.search(rel) else "js-main"
                    out.append(ConfigMain(rel, line_of(text, m.start()), kind, template, args, target))
    return out
