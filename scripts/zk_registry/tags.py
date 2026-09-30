"""circom input tags as DET preconditions.

circom forbids tags on the inputs of the main component, and a tag on a template input is a
precondition its callers promise (e.g. ``{binary}``: every element is 0 or 1).  Compiling such a
template through an untagged wrapper and stating DET without the precondition would make
counterexamples outside the promised domain spurious.  Instead, for tags whose meaning is fixed by
circom's documentation and circomlib, the template is compiled through a generated wrapper whose
main inputs are untagged copies, and the DET statement takes the tag's precondition on those input
wires as a hypothesis (DET under input preconditions).  Templates with other tags (project-defined
tags whose meaning is set by the project's own templates) stay UNINSTANTIABLE with the reason.
"""
from __future__ import annotations

import re
from dataclasses import dataclass

from . import circom_source as cs

# Tags with a meaning fixed outside the project.  ``valued`` tags carry a value set by the caller
# (``x.maxbit = n``); the precondition needs that value.
KNOWN_TAGS = {
    "binary": {"valued": False, "precondition": "each element is 0 or 1",
               "source": "circom language documentation, Tags (docs.circom.io/circom-language/tags): `binary`; "
                         "circomlib tag convention"},
    "maxbit": {"valued": True, "precondition": "each element is below 2^maxbit (as an integer in [0, p))",
               "source": "circom language documentation, Tags (docs.circom.io/circom-language/tags): `maxbit`; "
                         "circomlib tag convention"},
}
TAG_PRAGMA = (2, 1, 0)                  # tags exist from circom 2.1.0 on
WRAPPER = "BooleTagWrapper"


class TagError(ValueError):
    """The template's tags cannot be turned into DET preconditions (the reason is the message)."""


@dataclass
class Precondition:
    signal: str             # input signal name of the template (and of the wrapper's main)
    tag: str
    value: int | None = None


def io_declarations(t: cs.Template) -> tuple[list[cs.SignalDecl], list[cs.SignalDecl]]:
    decls = cs.signal_declarations(t.body)
    return [d for d in decls if d.direction == "input"], [d for d in decls if d.direction == "output"]


def input_tags(t: cs.Template) -> dict[str, list[str]]:
    """Input signal -> its tags (only tagged inputs)."""
    return {d.name: list(d.tags) for d in io_declarations(t)[0] if d.tags}


def preconditions(t: cs.Template, values: dict | None = None) -> list[Precondition]:
    """The DET preconditions of ``t``'s tagged inputs; raises :class:`TagError` for an unknown tag or a
    valued tag without a value (``values``: {(signal, tag): int})."""
    out = []
    values = values or {}
    for sig, tags in input_tags(t).items():
        for tag in tags:
            known = KNOWN_TAGS.get(tag)
            if known is None:
                raise TagError(f"input `{sig}` carries the tag `{tag}`, which is not in the known circomlib tag "
                               f"table ({', '.join(sorted(KNOWN_TAGS))}); its precondition is defined by the "
                               "project's own templates, so no DET precondition is stated")
            if known["valued"] and (sig, tag) not in values:
                raise TagError(f"input `{sig}` carries the valued tag `{tag}`, and no repository-grounded tag "
                               "value is available for the precondition")
            out.append(Precondition(sig, tag, values.get((sig, tag))))
    return out


def _dim_loops(name: str, dims: list[str], body: str, indent: str) -> str:
    idx = "".join(f"[i{k}]" for k in range(len(dims)))
    line = body.format(idx=idx)
    for k in reversed(range(len(dims))):
        line = f"for (var i{k} = 0; i{k} < {dims[k]}; i{k}++) {{ {line} }}"
    return indent + line


def wrapper_source(include: str, t: cs.Template, args: tuple[str, ...], pragma: tuple[int, int, int],
                   functions: set[str], comment: str | None = None, custom_templates: bool = False) -> str:
    """A main file that instantiates ``t`` through an untagged wrapper: the wrapper declares ``t``'s inputs and
    outputs without tags (same names and dimensions), copies each tagged input into a tagged intermediate
    signal and wires everything through.  Dimension expressions may use ``t``'s parameters and its
    single-assignment top-level ``var`` bindings (copied into the wrapper); raises :class:`TagError` otherwise."""
    from . import instantiation as I
    ins, outs = io_declarations(t)
    tagged = input_tags(t)
    bindings = I.constant_bindings(t.body)
    need: list[str] = []
    todo = [n for d in ins + outs for dim in d.dims for n in _idents(dim)]
    while todo:
        n = todo.pop(0)
        if n in t.params or n in functions or n in need:
            continue
        if n not in bindings:
            raise TagError(f"a signal dimension of `{t.name}` uses `{n}`, which is neither a parameter nor a "
                           "single-assignment top-level var, so the wrapper cannot declare the signal")
        need.append(n)
        todo += _idents(bindings[n])
    order = [m.group(1) for m in re.finditer(r"\bvar\s+(" + cs._IDENT + r")\b", t.body) if m.group(1) in need]
    ver = max(pragma, TAG_PRAGMA)
    lines = ([f"// {comment}"] if comment else []) + [f"pragma circom {ver[0]}.{ver[1]}.{ver[2]};"]
    if custom_templates:
        lines.append("pragma custom_templates;")
    lines += [f'include "{include}";', "",
              f"// untagged wrapper: DET is stated under the preconditions of the tags of {t.name}'s inputs",
              f"template {WRAPPER}({', '.join(t.params)}) {{"]
    for n in dict.fromkeys(order):
        lines.append(f"    var {n} = {bindings[n]};")
    for d in ins:
        lines.append(f"    signal input {d.name}{''.join(f'[{x}]' for x in d.dims)};")
    for d in outs:
        lines.append(f"    signal output {d.name}{''.join(f'[{x}]' for x in d.dims)};")
    lines.append(f"    component boole_c = {t.name}({', '.join(t.params)});")
    for d in ins:
        if d.name in tagged:
            tagset = ", ".join(tagged[d.name])
            lines.append(f"    signal {{{tagset}}} {d.name}_boole_tag{''.join(f'[{x}]' for x in d.dims)};")
            lines.append(_dim_loops(d.name, d.dims, f"{d.name}_boole_tag{{idx}} <== {d.name}{{idx}};", "    "))
            lines.append(_dim_loops(d.name, d.dims, f"boole_c.{d.name}{{idx}} <== {d.name}_boole_tag{{idx}};", "    "))
        else:
            lines.append(_dim_loops(d.name, d.dims, f"boole_c.{d.name}{{idx}} <== {d.name}{{idx}};", "    "))
    for d in outs:
        lines.append(_dim_loops(d.name, d.dims, f"{d.name}{{idx}} <== boole_c.{d.name}{{idx}};", "    "))
    lines += ["}", "", f"component main = {WRAPPER}({', '.join(args)});", ""]
    return "\n".join(lines)


def _idents(expr: str) -> list[str]:
    return [m.group(0) for m in re.finditer(r"(?<![A-Za-z0-9_$.])" + cs._IDENT + r"(?![A-Za-z0-9_$])", expr)]


# ------------------------------------------------------------------------------------------ evaluation

def wire_preconditions(pre: list[Precondition], input_wires: list[int], input_names: list[str]) -> list[dict]:
    """Preconditions per main input wire: [{"signal", "tag", "value", "wires"}] (names are ``main.<signal>[..]``)."""
    from .r1cs import signal_base
    out = []
    for pc in pre:
        wires = [w for w, n in zip(input_wires, input_names) if signal_base(n) == f"main.{pc.signal}"]
        out.append({"signal": f"main.{pc.signal}", "tag": pc.tag, "value": pc.value, "wires": wires})
    return out


def holds(wpre: list[dict], w: list[int]) -> bool:
    """The Python evaluation of the preconditions on a full assignment."""
    for g in wpre:
        for i in g["wires"]:
            v = w[i]
            if g["tag"] == "binary" and v not in (0, 1):
                return False
            if g["tag"] == "maxbit" and not v < 2 ** g["value"]:
                return False
    return True
