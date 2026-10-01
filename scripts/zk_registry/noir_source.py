"""Light Noir source scanner: items, impl / trait blocks, function signatures and modules.

Not a parser of the full language: it blanks comments and string literals (keeping offsets and line
numbers), then finds ``fn`` items with balanced-bracket scanning and records for each function its
name, line, attributes, modifiers (``pub``, ``unconstrained``, ``comptime``), generics, parameters,
return type, ``where`` clause, body range, and the innermost enclosing ``impl`` / ``trait`` /
``contract`` / ``mod`` block.  Everything the instantiation planner derives from source goes through
here, so it is tested on fixtures.
"""
from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field

IDENT = r"[A-Za-z_][A-Za-z0-9_]*"


def blank_comments_strings(src: str, strings: bool = True) -> str:
    """Comments (and, with ``strings``, string literal contents) replaced by spaces; newlines and quotes
    kept, so offsets and line numbers are unchanged."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', src[i:i + 8]) and (i == 0 or not re.match(r"[A-Za-z0-9_]", src[i - 1])):
            m = re.match(r'r(#*)"', src[i:])
            hashes = m.group(1)
            end = src.find('"' + hashes, i + len(m.group(0)))
            end = n if end < 0 else end + 1 + len(hashes)
            if strings:
                blank(i + len(m.group(0)), end - 1 - len(hashes))
            i = end
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            if strings:
                blank(i + 1, min(j, n))
            i = j + 1
        else:
            i += 1
    return "".join(out)


def strip_for_hash(src: str) -> str:
    """Comments removed and whitespace canonicalized (content identity of declarations)."""
    return re.sub(r"\s+", " ", blank_comments_strings(src, strings=False)).strip()


def matching(text: str, i: int, open_c: str, close_c: str) -> int:
    """Index just after the bracket that closes ``text[i]`` (== open_c)."""
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == open_c:
            depth += 1
        elif c == close_c:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def matching_angle(text: str, i: int) -> int:
    """Index after the ``>`` closing the ``<`` at ``i`` (``->`` arrows and nested brackets respected)."""
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "<":
            depth += 1
        elif c == ">" and text[i - 1] != "-":
            depth -= 1
            if depth == 0:
                return i + 1
        elif c in "({[":
            i = matching(text, i, c, {"(": ")", "{": "}", "[": "]"}[c]) - 1
        i += 1
    return n


def split_top(s: str, sep: str = ",") -> list[str]:
    """Split at top-level separators (outside (), [], {}, <> — arrows excepted)."""
    parts, depth, cur = [], 0, []
    i = 0
    while i < len(s):
        c = s[i]
        if c in "([{<" and not (c == "<" and False):
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == ">" and (i == 0 or s[i - 1] != "-"):
            depth -= 1
        if c == sep and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        i += 1
    tail = "".join(cur).strip()
    if tail:
        parts.append(tail)
    return parts


@dataclass
class Generic:
    name: str
    numeric: bool
    num_type: str = ""             # e.g. u32 for `let N: u32`
    bounds: list[str] = field(default_factory=list)

    def to_json(self) -> dict:
        return {"name": self.name, "numeric": self.numeric, "num_type": self.num_type, "bounds": self.bounds}


def parse_generics(s: str) -> list[Generic]:
    """``<T, let N: u32, U: Hash + Eq>`` (with or without the brackets)."""
    s = s.strip()
    if s.startswith("<"):
        s = s[1:-1]
    out = []
    for part in split_top(s):
        part = part.strip()
        if not part:
            continue
        m = re.match(r"let\s+(" + IDENT + r")\s*:\s*(.+)$", part)
        if m:
            out.append(Generic(m.group(1), True, m.group(2).strip()))
            continue
        m = re.match(r"(" + IDENT + r")\s*(?::\s*(.+))?$", part)
        if m:
            bounds = [b.strip() for b in split_top(m.group(2), "+")] if m.group(2) else []
            out.append(Generic(m.group(1), False, "", bounds))
    return out


@dataclass
class Param:
    pattern: str
    type: str
    mutable_binding: bool = False

    @property
    def name(self) -> str:
        return re.sub(r"^mut\s+", "", self.pattern).strip()

    def to_json(self) -> dict:
        return {"pattern": self.pattern, "type": self.type}


def parse_params(s: str) -> list[Param]:
    out = []
    for part in split_top(s):
        part = part.strip()
        if not part:
            continue
        if re.fullmatch(r"(&\s*mut\s+|&\s*)?(mut\s+)?self", part):
            ty = "&mut Self" if "&" in part and "mut" in part.split("self")[0] else ("&Self" if "&" in part else "Self")
            out.append(Param("self", ty, part.startswith("mut")))
            continue
        # `pattern: type` (the first top-level colon)
        depth, k = 0, -1
        for i, c in enumerate(part):
            if c in "([{<":
                depth += 1
            elif c in ")]}>":
                depth -= 1
            elif c == ":" and depth == 0 and part[i:i + 2] != "::" and (i == 0 or part[i - 1] != ":"):
                k = i
                break
        if k < 0:
            out.append(Param(part, ""))
            continue
        pat, ty = part[:k].strip(), part[k + 1:].strip()
        out.append(Param(pat, ty, pat.startswith("mut ")))
    return out


@dataclass
class Block:
    kind: str                       # impl | trait | contract | mod
    header: str
    start: int                      # offset of the opening brace
    end: int                        # offset after the closing brace
    line: int
    generics: list[Generic] = field(default_factory=list)
    self_type: str = ""             # impl: the type; trait: the trait name
    trait: str = ""                 # impl Trait for Type: the trait path (with arguments)
    where: str = ""
    attrs: list[str] = field(default_factory=list)


@dataclass
class Function:
    name: str
    line: int
    start: int
    body_start: int
    end: int
    attrs: list[str]
    modifiers: list[str]
    visibility: str
    generics: list[Generic]
    params: list[Param]
    ret: str
    ret_visibility: str
    where: str
    blocks: list[Block]             # enclosing blocks, outermost first
    has_body: bool = True

    @property
    def impl(self) -> Block | None:
        for b in reversed(self.blocks):
            if b.kind in ("impl", "trait"):
                return b
        return None

    @property
    def unconstrained(self) -> bool:
        return "unconstrained" in self.modifiers

    @property
    def comptime(self) -> bool:
        return "comptime" in self.modifiers

    def attr(self, name: str) -> bool:
        return any(re.match(r"\s*" + re.escape(name) + r"\b", a) for a in self.attrs)

    def to_json(self) -> dict:
        imp = self.impl
        return {"name": self.name, "line": self.line, "attrs": self.attrs, "modifiers": self.modifiers,
                "visibility": self.visibility, "generics": [g.to_json() for g in self.generics],
                "params": [p.to_json() for p in self.params], "ret": self.ret, "where": self.where,
                "impl": None if imp is None else {"kind": imp.kind, "self_type": imp.self_type, "trait": imp.trait,
                                                   "generics": [g.to_json() for g in imp.generics], "line": imp.line}}


_ATTR = re.compile(r"#\[([^\]]*(?:\[[^\]]*\][^\]]*)*)\]")


def _attrs_before(text: str, pos: int, src: str | None = None) -> tuple[list[str], int]:
    """Attributes (``#[...]``) directly above ``pos`` and the offset where they start; the attribute text is
    taken from ``src`` (string arguments intact) when given."""
    attrs = []
    start = pos
    while True:
        k = start
        while k > 0 and text[k - 1] in " \t\r\n":
            k -= 1
        if k > 0 and text[k - 1] == "]":
            # find the matching "#["
            depth, j = 0, k - 1
            while j >= 0:
                if text[j] == "]":
                    depth += 1
                elif text[j] == "[":
                    depth -= 1
                    if depth == 0:
                        break
                j -= 1
            if j > 0 and text[j - 1] == "#":
                attrs.insert(0, (src if src is not None else text)[j + 1:k - 1].strip())
                start = j - 1
                continue
        return attrs, start


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


def _blocks(text: str) -> list[Block]:
    out = []
    for m in re.finditer(r"(?<![A-Za-z0-9_])(impl|trait|contract|mod)\b", text):
        kind = m.group(1)
        i = m.end()
        # header runs to the first `{` or `;` at depth 0
        j, depth = i, 0
        while j < len(text):
            c = text[j]
            if c in "(<[":
                if c == "<":
                    j = matching_angle(text, j)
                    continue
                depth += 1
            elif c in ")]":
                depth -= 1
            elif c == ";" and depth == 0:
                j = -1
                break
            elif c == "{" and depth == 0:
                break
            j += 1
        if j < 0 or j >= len(text):
            continue
        header = text[m.start():j].strip()
        attrs, _ = _attrs_before(text, m.start())
        b = Block(kind, header, j, matching(text, j, "{", "}"), line_of(text, m.start()), attrs=attrs)
        rest = header[len(kind):].strip()
        if kind == "impl":
            if rest.startswith("<"):
                k = matching_angle(rest, 0)
                b.generics = parse_generics(rest[:k])
                rest = rest[k:].strip()
            wm = re.search(r"\bwhere\b", rest)
            if wm:
                b.where, rest = rest[wm.end():].strip(), rest[:wm.start()].strip()
            fm = re.search(r"\sfor\s", " " + rest + " ")
            if fm:
                b.trait = rest[:fm.start()].strip()
                b.self_type = rest[fm.start() + 4 - 1:].strip()
            else:
                b.self_type = rest
        elif kind == "trait":
            nm = re.match(r"(" + IDENT + r")\s*(<.*?>)?", rest)
            b.self_type = nm.group(1) if nm else rest
            if nm and nm.group(2):
                b.generics = parse_generics(nm.group(2))
        else:
            nm = re.match(IDENT, rest)
            b.self_type = nm.group(0) if nm else rest
        out.append(b)
    return out


_FN = re.compile(r"(?<![A-Za-z0-9_])fn\s+(" + IDENT + r")")


def scan(src: str) -> tuple[list[Function], list[Block], str]:
    text = blank_comments_strings(src)
    blocks = _blocks(text)
    fns = []
    for m in _FN.finditer(text):
        name = m.group(1)
        # modifiers / visibility just before `fn`
        pre = text[max(0, m.start() - 80):m.start()]
        mm = re.search(r"((?:(?:pub(?:\s*\([^)]*\))?|unconstrained|comptime|unsafe)\s+)*)$", pre)
        mods_text = mm.group(1) if mm else ""
        mods = re.findall(r"unconstrained|comptime|unsafe", mods_text)
        vis_m = re.search(r"pub(?:\s*\([^)]*\))?", mods_text)
        item_start = m.start() - len(mods_text)
        attrs, astart = _attrs_before(text, item_start, src)
        i = m.end()
        gen: list[Generic] = []
        while i < len(text) and text[i] in " \t\n":
            i += 1
        if i < len(text) and text[i] == "<":
            k = matching_angle(text, i)
            gen = parse_generics(text[i:k])
            i = k
        while i < len(text) and text[i] in " \t\n":
            i += 1
        if i >= len(text) or text[i] != "(":
            continue
        k = matching(text, i, "(", ")")
        params = parse_params(text[i + 1:k - 1])
        i = k
        # return type, where clause, body or `;`
        j, depth = i, 0
        while j < len(text):
            c = text[j]
            if c == "<":
                j = matching_angle(text, j)
                continue
            if c in "([":
                depth += 1
            elif c in ")]":
                depth -= 1
            elif depth == 0 and c in "{;":
                break
            j += 1
        sig_tail = text[i:j].strip()
        has_body = j < len(text) and text[j] == "{"
        end = matching(text, j, "{", "}") if has_body else j + 1
        ret, where, ret_vis = "", "", ""
        wm = re.search(r"\bwhere\b", sig_tail)
        if wm:
            where, sig_tail = sig_tail[wm.end():].strip(), sig_tail[:wm.start()].strip()
        if sig_tail.startswith("->"):
            ret = sig_tail[2:].strip()
            vm = re.match(r"(pub|call_data\(\d+\)|return_data)\s+", ret)
            if vm:
                ret_vis, ret = vm.group(1), ret[vm.end():].strip()
        enclosing = [b for b in blocks if b.start < m.start() < b.end]
        enclosing.sort(key=lambda b: b.start)
        fns.append(Function(name, line_of(text, m.start()), astart, j, end, attrs, mods,
                            vis_m.group(0) if vis_m else "", gen, params, ret, ret_vis, where, enclosing, has_body))
    return fns, blocks, text


def mod_path(rel: str) -> list[str]:
    """Module path of a crate source file relative to ``src/``: ``a/b.nr`` -> [a, b]; ``a/mod.nr`` -> [a];
    ``lib.nr`` / ``main.nr`` -> []."""
    parts = rel.split("/")
    if parts[-1] in ("lib.nr", "main.nr") and len(parts) == 1:
        return []
    if parts[-1] == "mod.nr":
        parts = parts[:-1]
    else:
        parts[-1] = parts[-1][:-3]
    return parts


def find_function(fns: list[Function], symbol: str, line: int | None) -> tuple[Function | None, str]:
    """The function a ledger symbol names: ``name``, ``name:L<line>``, ``Type::name``, ``Type::name@L<line>``.

    Exact line first (the ``fn`` line, or the attribute block above it), then the unique function of that
    name (and impl type), then the nearest by line."""
    sym = symbol
    lm = re.search(r"[:@]L(\d+)$", sym)
    if lm:
        line = int(lm.group(1))
        sym = sym[:lm.start()]
    owner = None
    if "::" in sym:
        owner, sym = sym.rsplit("::", 1)
        owner = owner.split("::")[-1]
    cands = [f for f in fns if f.name == sym]
    if owner:
        def own(f: Function) -> bool:
            imp = f.impl
            if imp is None:
                return False
            base = re.match(r"\s*([A-Za-z_][A-Za-z0-9_:]*)", imp.self_type)
            return bool(base) and base.group(1).split("::")[-1] == owner
        oc = [f for f in cands if own(f)]
        cands = oc or cands
    if not cands:
        return None, "no function of that name"
    if line is not None:
        exact = [f for f in cands if f.line == line]
        if exact:
            return exact[0], "line"
        near = [f for f in cands if 0 < f.line - line <= 6]     # attribute lines above `fn`
        if len(near) == 1:
            return near[0], "line-attr"
    if len(cands) == 1:
        return cands[0], "unique-name"
    if line is not None:
        best = min(cands, key=lambda f: abs(f.line - line))
        return best, "nearest-line"
    return None, f"ambiguous ({len(cands)} functions)"


def uses(text: str, top_level: bool = False) -> list[str]:
    """``use`` declarations (blanked text), one string per declaration; with ``top_level`` only those at
    brace depth 0 (not inside ``mod tests { }`` or function bodies)."""
    depth_at = None
    if top_level:
        depth_at, d = [], 0
        for c in text:
            depth_at.append(d)
            if c == "{":
                d += 1
            elif c == "}":
                d -= 1
    out = []
    for m in re.finditer(r"(?<![A-Za-z0-9_])use\s+([^;]+);", text):
        if depth_at is not None and depth_at[m.start()] != 0:
            continue
        out.append(re.sub(r"\s+", " ", m.group(1)).strip())
    return out


def globals_with_values(text: str) -> dict[str, str]:
    """``global NAME: T = <literal>;`` (also ``pub global``) -> literal text."""
    out = {}
    for m in re.finditer(r"(?<![A-Za-z0-9_])global\s+(" + IDENT + r")\s*(?::\s*[^=;]+)?=\s*([^;]+);", text):
        out[m.group(1)] = m.group(2).strip()
    return out


def sha256_text(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()
