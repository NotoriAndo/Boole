"""Decomposition of TOO-LARGE gnark records into the gadget instances they compile (wave RT-G2).

For every TOO-LARGE record the instantiated function (its recorded type arguments) is the root of a call graph of
the instantiated code: the Go tool ``gnark_tool/callees`` builds the repository's packages in SSA form with generic
functions instantiated and runs Rapid Type Analysis from a generated method expression or function value of the root
(static calls, closures, and interface calls resolved by the types the reachable code creates).  Every reachable
function of the module is listed with the type arguments of its instance.  The exported ones (a wrapper outside the
package can call them) become rows of the DET generator with those type arguments forced (tier ``decomposition``;
the other parameters are chosen by the generator's tiers); unexported functions, functions of other modules and
instances equal to an existing record are recorded as edges only.  No compositional statement is made: each package
states DET of the callee as instantiated.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess

from zk_registry import gnark_instantiation as I

# the decomposition driver selects instances (like noir_callees); hashed by the wave's run record
SOURCES = ["gnark_callees.py", "gnark_tool/callees/main.go"]
XTOOLS = "v0.48.0"                      # golang.org/x/tools of the catalog tool (go/ssa, callgraph/rta)
ROOTS_DIR = "boole_callees_roots"
ROOT_VAR = "BooleRoot"


def build_tool(build_dir: str, src_dir: str, env: dict) -> str:
    """The callees tool, built once in its own module (it imports only golang.org/x/tools)."""
    cdir = os.path.join(build_dir, "_callees")
    binary = os.path.join(cdir, "calleesx")
    if os.path.exists(binary):
        return binary
    os.makedirs(os.path.join(cdir, "callees"), exist_ok=True)
    shutil.copyfile(os.path.join(src_dir, "callees", "main.go"), os.path.join(cdir, "callees", "main.go"))
    with open(os.path.join(cdir, "go.mod"), "w", encoding="utf-8") as f:
        f.write(f"module boole.local/gnarkcallees\n\ngo 1.24\n\nrequire golang.org/x/tools {XTOOLS}\n")
    for args in (["go", "mod", "tidy"], ["go", "build", "-o", binary, "./callees"]):
        r = subprocess.run(args, cwd=cdir, env=env, capture_output=True, text=True, timeout=1800)
        if r.returncode != 0:
            raise RuntimeError(f"callees tool: {' '.join(args)} failed: " + r.stderr[-1500:])
    return binary


def split_key(key: str) -> tuple[str, str]:
    """``pkg/path.Name`` or ``pkg/path.Type.Method`` -> (package path, symbol)."""
    head, _, last = key.rpartition("/")
    first, _, symbol = last.partition(".")
    return (head + "/" + first if head else first), symbol


def exported(key: str) -> bool:
    """A wrapper outside the package can call it: an exported function, or an exported method of an exported type."""
    _, sym = split_key(key)
    return all(p[:1].isupper() for p in sym.split("."))


class GoText:
    """Go type expressions with import aliases for a generated file."""

    def __init__(self):
        self.imports: dict[str, str] = {}          # path -> alias

    def alias(self, path: str) -> str:
        if path not in self.imports:
            base = re.sub(r"[^A-Za-z0-9_]", "_", path.rsplit("/", 1)[-1])
            a, k = base, 1
            while a in self.imports.values():
                k += 1
                a = f"{base}{k}"
            self.imports[path] = a
        return self.imports[path]

    def typ(self, t: dict) -> str:
        k = t.get("k")
        if k == "named":
            s = (self.alias(t["pkg"]) + "." if t.get("pkg") else "") + t["name"]
            if t.get("args"):
                s += "[" + ", ".join(self.typ(a) for a in t["args"]) + "]"
            return s
        if k == "ptr":
            return "*" + self.typ(t["elem"])
        if k == "slice":
            return "[]" + self.typ(t["elem"])
        if k == "array":
            return f"[{t['len']}]" + self.typ(t["elem"])
        if k == "basic":
            return t["name"]
        raise ValueError(f"type not nameable in a generated file: {t}")


def recv_tparams(decl: dict) -> list[str]:
    """Type parameter names of a method's receiver as the method declares them (the generator's names)."""
    return [a["name"] for a in (decl.get("recv") or {}).get("args") or [] if a.get("k") == "tparam"]


def root_expr(g: GoText, decl: dict, env: dict) -> str:
    """A method expression or function value of the declaration at the type arguments ``env``."""
    pkg = g.alias(decl["pkg"])
    if decl.get("recv"):
        tps = recv_tparams(decl)
        args = "[" + ", ".join(g.typ(env[tp]) for tp in tps) + "]" if tps else ""
        return f"(*{pkg}.{decl['recv']['name']}{args}).{decl['name']}"
    if decl["kind"] == "type":
        return f"(*{pkg}.{decl['name']}).Define"
    tps = [tp["name"] for tp in (decl.get("sig") or {}).get("tparams") or []]
    args = "[" + ", ".join(g.typ(env[tp]) for tp in tps) + "]" if tps else ""
    return f"{pkg}.{decl['name']}{args}"


def roots_file(decl: dict, env: dict) -> str:
    g = GoText()
    expr = root_expr(g, decl, env)
    imports = "".join(f'\t{a} "{p}"\n' for p, a in sorted(g.imports.items()))
    return (f"// Code generated by boole-zk-registry (gnark decomposition roots). DO NOT EDIT.\n\npackage {ROOTS_DIR}\n\n"
            f"import (\n{imports})\n\nvar {ROOT_VAR}0 = {expr}\n")


def env_of_record(cat: I.Catalog, tgt: dict, args: list[str]) -> dict | None:
    """The type arguments a registry record names (``T=emparams.BN254Fp`` labels of its instantiation) as type trees,
    resolved against the catalog's named types; None when a label does not resolve."""
    decl = tgt["decl"]
    tps = recv_tparams(decl) + [tp["name"] for tp in (decl.get("sig") or {}).get("tparams") or []]
    labels = dict(a.split("=", 1) for a in args if "=" in a and a.split("=", 1)[0] in tps)
    if set(labels) != set(tps):
        return None
    by_short: dict[str, list[dict]] = {}
    for key, info in (cat.data.get("named") or {}).items():
        if not info:
            continue
        short = info["pkg"].rsplit("/", 1)[-1] + "." + info["name"]
        by_short.setdefault(short, []).append({"k": "named", "pkg": info["pkg"], "name": info["name"]})

    def parse(s: str) -> dict | None:
        s = s.strip()
        if s.startswith("*"):
            e = parse(s[1:])
            return None if e is None else {"k": "ptr", "elem": e}
        m = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*\.[A-Za-z_][A-Za-z0-9_]*)(?:\[(.*)\])?", s)
        if not m or len(by_short.get(m.group(1), [])) != 1:
            return None
        t = dict(by_short[m.group(1)][0])
        if m.group(2):
            parts, depth, cur = [], 0, ""
            for ch in m.group(2):
                if ch == "," and depth == 0:
                    parts.append(cur)
                    cur = ""
                    continue
                depth += ch == "["
                depth -= ch == "]"
                cur += ch
            parts.append(cur)
            args_ = [parse(p) for p in parts]
            if any(a is None for a in args_):
                return None
            t["args"] = args_
        return t
    env = {k: parse(v) for k, v in labels.items()}
    return None if any(v is None for v in env.values()) else env


def label_of(env: dict) -> list[str]:
    """The registry's labels of type arguments (``T=emparams.BN254Fp``), sorted."""
    return sorted(f"{k}={I.show(v)}" for k, v in env.items())


def instance_id(key: str, env: dict) -> str:
    return hashlib.sha256(json.dumps([key, env], sort_keys=True).encode()).hexdigest()


def run_root(binary: str, module_dir: str, decl: dict, env: dict, work: str, go_env: dict, prefix: str,
             timeout: float = 900) -> dict:
    """Callees of one root: {"key", "callees": [...]} or {"error"}.  The generated roots package is written inside
    the module (a scratch checkout) and removed afterwards."""
    pdir = os.path.join(module_dir, f"{ROOTS_DIR}_{hashlib.sha256(work.encode()).hexdigest()[:10]}")
    os.makedirs(pdir, exist_ok=True)
    try:
        with open(os.path.join(pdir, "roots.go"), "w", encoding="utf-8") as f:
            f.write(roots_file(decl, env).replace(f"package {ROOTS_DIR}", f"package {os.path.basename(pdir)}"))
        os.makedirs(work, exist_ok=True)
        out = os.path.join(work, "callees.json")
        r = subprocess.run([binary, "-dir", module_dir, "-roots", pdir, "-out", out, "-prefix", prefix], cwd=module_dir,
                           env=go_env, capture_output=True, text=True, timeout=timeout)
        if r.returncode != 0 or not os.path.exists(out):
            return {"error": (r.stderr or r.stdout).strip()[-600:]}
        with open(out, encoding="utf-8") as f:
            res = json.load(f) or []
        if not res:
            return {"error": "no root in the generated package"}
        return res[0]
    finally:
        shutil.rmtree(pdir, ignore_errors=True)
