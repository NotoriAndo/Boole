#!/usr/bin/env python3
"""Ratchet problems (verified circuit optimization) for Noir references of the registry (compiled ACIR).

The Circom ratchet (``ratchet.py``) carried over to Noir functions:

* **Reference meaning**: the registry package's DET model of the reference (the flattened ACIR of the function's
  ``#[export]`` wrapper, the model file byte for byte).  The reference's DET must be machine-checked (the battery P3
  mechanical proof, or a battery closure re-checked as a proof) by the production checker within the proof limits.
* **Record**: a cost vector of the compiled ACIR that is meaningful when the function is embedded in a caller:
  ``nonlinear`` (degree-2 terms of AssertZero opcodes: multiplication gates), ``range_bits`` (bits of RANGE
  opcodes), ``logic_bits`` (bits of AND / XOR opcodes), ``black_box`` (enabled calls per black-box function) and
  ``memory`` (memory-block elements initialized plus memory operations).  Linear-only AssertZero opcodes are free (a
  caller substitutes them, as in the Circom non-linear metric), so are the RANGE checks the wrapper's ABI types put on
  its parameter witnesses (``abi_range_bits``: a caller's values are already typed) and Brillig calls (unconstrained
  hints cost nothing in proving); all three are reported.  A candidate counts only if it is no larger in any priced component and
  strictly smaller in at least one (component-wise order): then it is no more expensive under every backend whose
  cost grows with these components, whatever their relative weights, and strictly cheaper wherever the improved
  component has a positive weight.  ``priced`` (the sum) is a summary for size bands and reports, not the order.
* **Candidate**: a Noir function ``boole_ratchet_candidate`` (plus helper items) in ``Candidate.nr``.  The harness
  appends it to the reference's source file (library functions) or to the wrapper crate (standard-library items),
  next to the reference's own wrapper text in which the one call of the reference is replaced by a call of the
  candidate with the same arguments; it is compiled by the reference's pinned ``nargo`` (digest-checked) with the
  reference's command (``nargo export --silence-warnings``) against the problem's crate snapshot (manifest-checked).
  The wrapper's ABI, input witnesses and number of return witnesses must equal the reference's.
* **Admissibility**: exactly one top-level ``fn boole_ratchet_candidate`` that is neither ``unconstrained`` nor
  ``comptime``; no ``fn main``, ``mod`` or ``contract`` items; attributes only from a fixed list (no new oracle,
  foreign or builtin declarations, no ``#[export]``).  Unconstrained helpers (Brillig hints) are allowed: their
  outputs are free in the model, so the proof has to pin them down.
* **Statement** (generated per candidate, both directions of the input-output relation, black boxes shared):
  for every interpretation ``bb`` of the black boxes, numbered like the reference's model, every input vector ``x``
  and output vector ``y``: some assignment satisfies the reference's constraints with inputs ``x`` and outputs ``y``
  iff some assignment satisfies the candidate's.
* **Screen** (``simulate``): both programs through the pinned executor (the compiler's own ACVM solver in
  ``boole-acir-tool``; oracle calls answered with zeros) on the same structured and seeded random ABI inputs; the
  candidate must fail and succeed on the same inputs and return the same values; every candidate witness is
  re-checked against the candidate's decoded ACIR by the independent Python evaluator, and every return witness must
  occur in a constrained opcode.  A screen, not a proof: an under-constrained hint that the Brillig code fills
  correctly passes it.
* **Final check**: the candidate pipeline (component-wise smaller than the record) and the unchanged production
  checker (``check.py``) on the generated package.

Usage::

    python3 -m zk_registry.ratchet_noir count     --problem DIR --candidate Candidate.nr --out DIR --tools DIR
                                                  [--snapshot DIR]
    python3 -m zk_registry.ratchet_noir simulate  --problem DIR --candidate Candidate.nr --out DIR --tools DIR
                                                  [--n 2000]
    python3 -m zk_registry.ratchet_noir statement --problem DIR --candidate Candidate.nr --out DIR --tools DIR
                                                  --lean-env E.json
    python3 -m zk_registry.ratchet_noir check     --problem DIR --candidate Candidate.nr --solution S --out DIR
                                                  --tools DIR --lean-env E.json

``--tools DIR`` holds ``<tag>/nargo`` and ``<tag>/boole-acir-tool`` (digest-checked against the problem on every
use).  Problems are built by :func:`build_problem` (the wave driver supplies the DET evidence and the snapshot).  Exit
codes as in ``ratchet.py``: ``count`` / ``simulate`` / ``statement``: 0 smaller (``simulate``: and no mismatch), 4
rejected or not smaller, 5 simulation mismatch, 3 error; ``check``: 0 PASS, 1 FAIL, 2 INVALID, 3 ERROR, 4 REJECTED.
"""
from __future__ import annotations

import argparse
import collections
import hashlib
import json
import os
import random
import re
import shutil
import sys
import time

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C              # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import mech_acir as M          # noqa: E402
from zk_registry import noir_det as ND           # noqa: E402
from zk_registry import noir_acir as A          # noqa: E402
from zk_registry import noir_lean_emit as NE    # noqa: E402
from zk_registry import noir_source as NS       # noqa: E402
from zk_registry import noir_toolchain as NT    # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import ratchet as RT           # noqa: E402

GENERATOR_NAME = RT.GENERATOR_NAME
GENERATOR_VERSION = "1-noir"
GENERATOR_SOURCES = ["ratchet_noir.py", "ratchet.py", "schema/ratchet_noir_problem.schema.json", "noir_acir.py",
                     "noir_lean_emit.py", "noir_source.py", "noir_toolchain.py", "noir_det.py", "mech_acir.py",
                     "package.py", "check.py"]
HERE = os.path.dirname(os.path.abspath(__file__))

PLANS = ("export", "std-export")                 # library functions and standard-library items (wrapper programs)
CANDIDATE_FN = "boole_ratchet_candidate"
NARGO_COMMAND = ["export", "--silence-warnings"]
WRAPPER_HEADER = "\n\n// generated by boole-zk-registry (noir DET wrapper)\n"    # noir_det.compile_candidate
CANDIDATE_HEADER = "\n\n// ratchet candidate (Candidate.nr)\n"
STD_MANIFEST = '[package]\nname = "boole_wrapper"\ntype = "lib"\n\n[dependencies]\n'  # noir_det.compile_std
ALLOWED_ATTRIBUTES = ("inline_always", "no_predicates", "fold", "allow", "derive")
SCALAR_COMPONENTS = ("nonlinear", "range_bits", "logic_bits", "memory")
COMPILE_TIMEOUT, COMPILE_RSS_MB = 900, 12288
SIM_SEED = "boole-ratchet-noir-sim"
EXIT = RT.EXIT
DET_PROOF_LIMITS = RT.DET_PROOF_LIMITS

METRIC = ("component-wise cost of the flattened ACIR of the wrapper program: nonlinear (degree-2 terms of AssertZero "
          "opcodes), range_bits (bits of RANGE opcodes), logic_bits (bits of AND / XOR opcodes), black_box (enabled "
          "calls per black-box function) and memory (memory-block elements initialized plus memory operations); "
          "linear-only AssertZero opcodes are free (a caller substitutes them), so are the RANGE checks of the "
          "wrapper's parameters at their ABI widths (the caller's values are already typed) and Brillig calls "
          "(unconstrained hints); a candidate counts only if no priced component is larger than the record's and at "
          "least one is smaller (then it is no more expensive under any backend cost that grows with these "
          "components); priced = the sum, a summary only")
ADMISSIBILITY = [
    "Candidate.nr declares exactly one top-level `fn boole_ratchet_candidate`, neither `unconstrained` nor "
    "`comptime`; helper items are allowed (unconstrained helpers are Brillig hints: free in the model)",
    "no `fn main`, `mod` or `contract` items; attributes only `#[inline_always]`, `#[no_predicates]`, `#[fold]`, "
    "`#[allow(..)]`, `#[derive(..)]` (no new oracle, foreign or builtin declarations, no `#[export]`)",
    "the harness appends Candidate.nr after the reference's own wrapper text, in which the reference call is "
    "replaced by `boole_ratchet_candidate(<the same arguments>)`, to the reference's source file of the pinned crate "
    "snapshot (standard-library items: to the wrapper crate)",
    "compiled by the reference's pinned nargo (digest-checked) with `nargo export --silence-warnings`, the crate "
    "snapshot and its dependencies manifest-checked",
    "the wrapper's ABI (parameters and return type), input witnesses and number of return witnesses equal the "
    "reference's",
]
STATEMENT_DESCRIPTION = (
    "over F = ZMod p (BN254, [Fact (Nat.Prime p)]): for every interpretation bb of the black boxes (numbered like the "
    "reference model, shared by both sides) and all input vectors x and output vectors y, "
    "(∃ w, Ref.Constraints bb w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y) ↔ "
    "(∃ w', Cand.Constraints bb w' ∧ Cand.Inputs.map w' = x ∧ Cand.Outputs.map w' = y); Ref is the registry DET "
    "model of the reference, Cand the model of the candidate's ACIR (the bb binder is omitted when neither model has "
    "black boxes); inputs and outputs are matched by position (identical ABI)")
DETERMINISM_NOTE = (
    "reference DET + equiv => candidate DET: for one interpretation bb, two candidate assignments with equal inputs x "
    "and outputs y1, y2 make (x, y1) and (x, y2) reference-realizable under bb, so y1 = y2 by the reference's DET")

Reject, NotEligible = RT.Reject, RT.NotEligible
sha, now, read_json, put, fresh_dir = RT.sha, RT.now, RT.read_json, RT.put, RT.fresh_dir


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in GENERATOR_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


# ------------------------------------------------------------------------------------------ metric

def abi_ranges(abi: dict | None, inputs: list[int] | None) -> dict[int, int]:
    """The range checks the wrapper's ABI types impose on its parameter witnesses (witness k is the k-th ABI element):
    the integer width, 1 for a bool, 8 per string character.  Empty unless the inputs are exactly 0..n-1."""
    widths: list[int | None] = []

    def walk(t: dict) -> None:
        k = t.get("kind")
        if k == "array":
            for _ in range(int(t["length"])):
                walk(t["type"])
        elif k == "string":
            widths.extend([8] * int(t["length"]))
        elif k == "struct":
            for f in t["fields"]:
                walk(f["type"])
        elif k == "tuple":
            for f in t["fields"]:
                walk(f)
        else:
            widths.append(int(t["width"]) if k == "integer" else 1 if k == "boolean" else None)
    for prm in (abi or {}).get("parameters", []):
        walk(prm["type"])
    if inputs is None or list(inputs) != list(range(len(widths))):
        return {}
    return {i: w for i, w in enumerate(widths) if w}


def cost_of(ops: list[dict], abi_rng: dict[int, int] | None = None) -> dict:
    """The cost vector of a flattened program (see the module doc).  A black-box call with a constant-zero predicate
    is disabled (the model drops it) and costs nothing; a RANGE on a parameter witness at its ABI width
    (``abi_rng``) is the wrapper's input typing, absent when the function is embedded, and is free like the linear
    opcodes (identical in the reference and every candidate, since the wrapper is the same)."""
    abi_rng = abi_rng or {}
    nl = lin = rb = lb = mem = br = arb = 0
    bb: collections.Counter = collections.Counter()
    for op in ops:
        k = op["kind"]
        if k == "assert_zero":
            m = sum(1 for q, _, _ in op["expr"].mul if q % A.BN254)
            if m:
                nl += m
            else:
                lin += 1
        elif k == "range":
            if op["input"][0] == "w" and abi_rng.get(op["input"][1]) == op["bits"]:
                arb += op["bits"]
            else:
                rb += op["bits"]
        elif k in ("and", "xor"):
            lb += op["bits"]
        elif k == "bb":
            pred = op["predicate"]
            if pred is not None and pred[0] == "c" and pred[1] % A.BN254 == 0:
                continue
            bb[op["key"]] += 1
        elif k == "mem_init":
            mem += len(op["init"])
        elif k == "mem_op":
            mem += 1
        elif k == "brillig":
            br += 1
    out = {"nonlinear": nl, "range_bits": rb, "logic_bits": lb, "black_box": dict(sorted(bb.items())), "memory": mem}
    out["priced"] = P.noir_priced(out)
    out.update(linear=lin, abi_range_bits=arb, brillig=br, n_opcodes=len(ops))
    return out


def score(record: dict, cnt: dict) -> dict:
    """Candidate cost against the record: smaller iff component-wise no larger and strictly smaller somewhere."""
    def pct(a, b):
        return round(100.0 * (a - b) / a, 2) if a else 0.0
    keys = ("nonlinear", "range_bits", "logic_bits", "black_box", "memory", "priced", "linear", "abi_range_bits",
            "brillig")
    out = {k: cnt[k] for k in keys}
    out.update(smaller=P.noir_cost_smaller(record, cnt), larger_components=P.noir_larger_components(record, cnt),
               reduction_pct=pct(record["priced"], cnt["priced"]),
               nonlinear_reduction_pct=pct(record["nonlinear"], cnt["nonlinear"]))
    return out


def cost_text(c: dict) -> str:
    bb = ", ".join(f"{k} x{v}" for k, v in c["black_box"].items()) or "none"
    return (f"{c['nonlinear']} non-linear terms, {c['range_bits']} range bits, {c['logic_bits']} logic bits, "
            f"{c['memory']} memory units, black boxes: {bb} (priced {c['priced']}; free: {c['linear']} linear, "
            f"{c['abi_range_bits']} ABI range bits, {c['brillig']} Brillig)")


# ------------------------------------------------------------------------------------------ reference

def crate_layout(repo_root: str, path: str) -> dict:
    """The crate of a repository file: repo-relative crate directory, crate type and the file the wrapper is appended
    to (``noir_det.compile_candidate``: a binary or contract crate is compiled as a library copy whose root is
    ``src/lib.nr``, made from ``src/main.nr``)."""
    crate = NT.crate_of(os.path.join(repo_root, path), repo_root)
    if crate is None:
        raise NotEligible("snapshot", f"{path}: no Nargo.toml package above the file")
    ctype = NT.crate_info(crate)["type"]
    rel_in_src = os.path.relpath(os.path.join(repo_root, path), os.path.join(crate, "src"))
    target = "src/lib.nr" if ctype in ("bin", "contract") and rel_in_src == "main.nr" else "src/" + rel_in_src
    return {"crate": os.path.relpath(crate, repo_root), "crate_type": ctype, "target_file": target}


def compiler_tag(spec: dict) -> str:
    """The pinned compiler of the reference: the registry's recorded attempt with the package's nargo version (a
    pinned release binary; source builds are not ratchet compilers)."""
    version = spec["nargo_version"]
    tags = [t for t in spec["compiler_attempts"] if t in NT.COMPILERS and NT.COMPILERS[t].version == version]
    if not tags:
        raise NotEligible("compiler", f"nargo {version} is not a recorded pinned compiler of {spec['repo']}")
    tag = tags[0]
    if not NT.COMPILERS[tag].asset_sha256:
        raise NotEligible("compiler", f"nargo {tag} is a source build (release binaries only)")
    return tag


def append_text(plan: str, wrapper: str, imports: list[str]) -> str:
    """The exact text the registry compile added: appended to the target file (``export``) or the wrapper crate's
    ``src/lib.nr`` (``std-export``)."""
    if plan == "export":
        return WRAPPER_HEADER + "".join(f"use {x};\n" for x in imports) + wrapper
    return "\n".join(imports) + "\n\n" + wrapper


def reference_spec(reg_dir: str) -> dict:
    """The reference fields of a registry Noir DET package (``problem.json`` and ``evidence``)."""
    prob = read_json(os.path.join(reg_dir, "problem.json"))
    ids, ins, circ, st, chk, ev = (prob["ids"], prob["instantiation"], prob.get("circuit") or {}, prob["statement"],
                                   prob["checker"], prob["evidence"])
    plan = ev.get("plan")
    spec = {"registry_package_id": prob["package_id"], "property": prob["property"]["template"],
            "status": prob["status"], "repo": ids["repo"], "repo_url": ids["repo_url"], "commit": ids["commit"],
            "path": ids["path"], "function": ids["template"], "function_line": ids.get("template_line"),
            "plan": plan, "call": ins.get("call") or "", "rule": ins.get("rule"),
            "dependencies": ids.get("dependencies") or [],
            "nargo_version": (circ.get("compiler") or {}).get("version"),
            "nargo_sha256": (circ.get("compiler") or {}).get("binary_sha256"),
            "compiler_attempts": list((ev.get("compiler_selection") or {}).get("attempt_order") or []),
            "model": {"file": st["model_file"], "module": st["model_module"],
                      "namespace": st["model_module"][:-len(".Model")],
                      "sha256": next(f["sha256"] for f in chk["files"] if f["path"] == st["model_file"])},
            "det_theorem_fqn": st["theorem_fqn"], "lean_opts": chk["lean_opts"], "env": prob["env"]}
    wpath = os.path.join(reg_dir, "evidence", "wrapper.nr")
    if os.path.exists(wpath):
        with open(wpath, encoding="utf-8") as f:
            first, _, wrapper = f.read().partition("\n")
        if not first.startswith("// appended to "):
            raise NotEligible("provenance", "evidence/wrapper.nr has no recorded header line")
        spec["wrapper"] = wrapper
        m = re.search(r"#\[export\]\s*fn\s+(boole_det_[A-Za-z0-9_]+)\s*\(", wrapper)
        spec["wrapper_name"] = m.group(1) if m else None
        imports = ev.get("wrapper_imports") or [] if plan == "export" else ev.get("std_imports") or []
        spec["imports"] = list(imports)
    return spec


def static_exclusion(spec: dict) -> tuple[str, str] | None:
    if spec["property"] != "DET":
        return "property", f"{spec['property']} package"
    if spec["plan"] not in PLANS:
        return "plan", (f"compile plan {spec['plan']} (binary main / contract entrypoint: no wrapper call to replace; "
                        "the ratchet harness covers library and standard-library functions)")
    if not spec.get("wrapper") or not spec.get("wrapper_name") or not spec["call"]:
        return "provenance", "no recorded wrapper or reference call"
    if spec["wrapper"].count(spec["call"]) != 1:
        return "provenance", "the reference call does not occur exactly once in the wrapper"
    try:
        compiler_tag(spec)
    except NotEligible as ex:
        return ex.code, ex.detail
    return None


def measure_reference(reg_dir: str) -> dict:
    """The registry ACIR (``evidence/acir.json``), checked to render to the package's Model.lean (battery P3 loader),
    and its record cost."""
    try:
        pkg = M.load(reg_dir)
    except (M.ModelMismatch, A.Unsupported, OverflowError, KeyError, ValueError) as ex:
        raise NotEligible("provenance", f"registry ACIR does not render to the model: {ex}") from None
    with open(os.path.join(reg_dir, "evidence", "acir.json"), encoding="utf-8") as f:
        decoded = json.load(f)
    flat = pkg.flat
    return {"flat": flat, "abi": decoded.get("abi") or {}, "keys": list(pkg.keys), "n_conjuncts": len(pkg.entries),
            "memory": bool(pkg.mem), "acir_sha256": A.normalize_program(decoded).raw_sha256,
            "cost": cost_of(flat.opcodes, abi_ranges(decoded.get("abi"), flat.inputs)), "inputs": list(flat.inputs), "outputs": list(flat.outputs),
            "n_wires": flat.n_witnesses}


def model_key(flat: A.Flat) -> str:
    """Content key of a model: its conjuncts, witness count and I/O lists (two references with the same key state the
    same constraints)."""
    keys = A.bb_keys(flat.opcodes)
    body = {"conjuncts": NE.opcode_conjuncts(flat.opcodes, keys), "keys": keys, "n": flat.n_witnesses,
            "inputs": list(flat.inputs), "outputs": list(flat.outputs)}
    return sha(json.dumps(body, sort_keys=True).encode())


def abi_core(abi: dict) -> dict:
    """The ABI parts a candidate must keep (parameters and return type; error types follow the assert messages)."""
    return {"parameters": abi.get("parameters", []), "return_type": abi.get("return_type")}


def battery_det_solution(statement: str, n_conjuncts: int, bb: bool, memory: bool, form: str) -> str:
    """A ``det`` proof from a registry battery form (``triv_<variant>_<tactic>``) that closed the Noir statement: the
    statement with its ``sorry`` replaced by the form's tactic script (``noir_lean_emit.battery_prefix``)."""
    m = re.fullmatch(r"triv_(V[0-4])_([A-Za-z_]+)", form)
    if not m or m.group(2) in ("bv_decide", "exactQ", "applyQ"):
        raise ValueError(f"no standard-axiom proof for battery form {form!r}")
    variant, tactic = m.groups()
    lines = NE.battery_prefix(variant, n_conjuncts, bb, memory) + [f"all_goals {tactic}" if variant == "V4" else tactic]
    if variant in ("V3", "V4"):
        body = ["  set_option maxRecDepth 100000 in"] + [f"    {x}" for x in lines]
    else:
        body = [f"  {x}" for x in lines]
    if statement.count("\n  sorry\n") != 1:
        raise ValueError("statement has no single `sorry` line")
    return statement.replace("\n  sorry\n", "\n" + "\n".join(body) + "\n", 1)


# ------------------------------------------------------------------------------------------ toolchain and compile

def tool_paths(tools: str, prob: dict) -> tuple[str, str, str]:
    """(tag, nargo, boole-acir-tool) of the problem, both binaries digest-checked against the problem."""
    comp = prob["reference"]["compiler"]
    nargo = os.path.join(tools, comp["tag"], "nargo")
    tool = os.path.join(tools, comp["tag"], "boole-acir-tool")
    for path, want, what in ((nargo, comp["nargo_sha256"], "nargo"), (tool, prob["env"]["acir_tool_sha256"],
                                                                        "boole-acir-tool")):
        if not os.path.isfile(path):
            raise RuntimeError(f"no {what} for {comp['tag']} under {tools}")
        if P.sha256_file(path) != want:
            raise RuntimeError(f"{what} {path} is not the pinned binary of the problem")
    return comp["tag"], nargo, tool


def tool_env(home: str, tmp: str) -> dict:
    os.makedirs(tmp, exist_ok=True)
    env = NT.Toolchain("", home, os.path.dirname(tmp)).env()
    env["TMPDIR"] = tmp
    return env


def strip_artifact(art: str, out: str) -> str:
    """The executor's inputs of a nargo artifact ({noir_version, abi, bytecode}: no debug symbols or file map)."""
    with open(art, encoding="utf-8") as f:
        a = json.load(f)
    P.write_json(out, {k: a[k] for k in ("noir_version", "abi", "bytecode") if k in a})
    return out


def decode(tool: str, art: str, work: str) -> dict:
    r = L.run_process([tool, "decode", art], tool_env(os.path.join(work, "home"), os.path.join(work, "tmp")), work,
                      1800, 16384)
    if r.rc != 0:
        raise RuntimeError(f"decoder failed: {r.out[-300:]}")
    return json.loads(r.out)


def compile_program(prob: dict, problem_dir: str, snapshot: str, tools: str, work: str,
                    candidate_text: str | None = None) -> dict:
    """The reference's wrapper program (or, with ``candidate_text``, the candidate's: the reference call replaced and
    the candidate appended) built as the registry built it; returns the decoded program and the stripped artifact."""
    ref = prob["reference"]
    tag, nargo, tool = tool_paths(tools, prob)
    with open(os.path.join(problem_dir, ref["append"]["file"]), encoding="utf-8") as f:
        text = f.read()
    if sha(text.encode()) != ref["append"]["sha256"]:
        raise RuntimeError("reference wrapper text differs from the problem record")
    if candidate_text is not None:
        text = text.replace(ref["call"], candidate_call(ref["call"], text), 1) + CANDIDATE_HEADER + candidate_text
    fresh_dir(work)
    crate = os.path.join(work, "crate")
    if ref["plan"] == "export":
        ND.copy_crate(os.path.join(snapshot, "repo", ref["crate"]), crate,
                      as_lib=ref["crate_type"] in ("bin", "contract"), strip_contract=ref["crate_type"] == "contract")
        with open(os.path.join(crate, ref["target_file"]), "a", encoding="utf-8") as f:
            f.write(text)
    else:
        put(os.path.join(crate, "Nargo.toml"), STD_MANIFEST)
        put(os.path.join(crate, "src", "lib.nr"), text)
    home = os.path.join(work, "home")
    os.makedirs(os.path.join(home, "nargo"))
    deps = os.path.join(snapshot, "home", "nargo")
    for e in sorted(os.listdir(deps)) if os.path.isdir(deps) else []:
        # the dependency trees are linked (read-only use); nargo's own lock file stays in the work directory
        os.symlink(os.path.join(deps, e), os.path.join(home, "nargo", e))
    r = L.run_process([nargo] + NARGO_COMMAND, tool_env(home, os.path.join(work, "tmp")), crate, COMPILE_TIMEOUT,
                      COMPILE_RSS_MB)
    put(os.path.join(work, "nargo.log"), r.out[-20000:])
    art = os.path.join(crate, "export", ref["wrapper_name"] + ".json")
    res = {"rc": r.rc, "compile_secs": r.secs, "compile_peak_rss_mb": r.peak_rss_mb, "text_sha256": sha(text.encode())}
    if r.rc != 0 or not os.path.exists(art):
        res["error"] = ("timeout" if r.timeout else "memory guard" if r.memkill else
                        ND.first_error(r.out) or "nargo export produced no artifact")[:1200]
        res["error"] = res["error"].replace(work, "<work>").replace(snapshot, "<snapshot>")
        return res
    program = strip_artifact(art, os.path.join(work, "program.json"))
    decoded = decode(tool, program, work)
    prog = A.normalize_program(decoded)
    res.update(program=program, program_sha256=P.sha256_file(program), decoded=decoded, prog=prog,
               acir_sha256=prog.raw_sha256, flat=A.flatten(prog))
    return res


# ------------------------------------------------------------------------------------------ candidates

_ATTR = re.compile(r"#!?\[\s*([A-Za-z_][A-Za-z0-9_:]*)")


def admit_candidate(path: str) -> str:
    """Source rules (raises Reject); returns the candidate text."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    clean = NS.blank_comments_strings(text)
    for name in _ATTR.findall(clean):
        if name not in ALLOWED_ATTRIBUTES:
            raise Reject(f"attribute #[{name}] is not allowed (allowed: {', '.join(ALLOWED_ATTRIBUTES)})")
    for pat, what in ((r"(?<![A-Za-z0-9_])fn\s+main\s*[<(]", "`fn main`"),
                      (r"(?<![A-Za-z0-9_])mod\s+[A-Za-z_]", "`mod` items"),
                      (r"(?<![A-Za-z0-9_])contract\s+[A-Za-z_]", "`contract` blocks"),
                      (r"boole_det_", "names of the registry wrapper (`boole_det_*`)")):
        if re.search(pat, clean):
            raise Reject(f"Candidate.nr must not declare {what}")
    fns, _, _ = NS.scan(text)
    own = [f for f in fns if f.name == CANDIDATE_FN]
    if len(own) != 1:
        raise Reject(f"Candidate.nr must declare exactly one `fn {CANDIDATE_FN}` ({len(own)} found)")
    fn = own[0]
    if fn.blocks:
        raise Reject(f"`fn {CANDIDATE_FN}` must be a top-level function (not inside an impl, trait or module)")
    bad = [m for m in fn.modifiers if m in ("unconstrained", "comptime")]
    if bad:
        raise Reject(f"`fn {CANDIDATE_FN}` must be constrained ({' '.join(bad)} found): its body is the circuit")
    if not fn.has_body:
        raise Reject(f"`fn {CANDIDATE_FN}` has no body")
    return text


def _call_args(call: str) -> tuple[str, str]:
    """(callee, argument text) of a call expression ending in its balanced argument list."""
    if not call.endswith(")"):
        raise ValueError(f"not a call expression: {call!r}")
    depth = 0
    for i in range(len(call) - 1, -1, -1):
        c = call[i]
        if c == ")":
            depth += 1
        elif c == "(":
            depth -= 1
            if depth == 0:
                return call[:i], call[i + 1:-1]
    raise ValueError(f"unbalanced call expression: {call!r}")


def candidate_call(call: str, wrapper_text: str) -> str:
    """The reference call with the callee replaced by the candidate: path calls keep their arguments; a method call
    ``s_self.f(args)`` passes the receiver first (``&mut s_self`` when the wrapper binds it mutably)."""
    callee, args = _call_args(call)
    m = re.fullmatch(r"s_self\.[A-Za-z_][A-Za-z0-9_]*(?:::<.*>)?", callee.strip(), re.S)
    if m:
        recv = "&mut s_self" if re.search(r"\blet\s+mut\s+s_self\b", wrapper_text) else "s_self"
        args = recv + (", " + args.strip() if args.strip() else "")
    return f"{CANDIDATE_FN}({args})"


def io_mismatch(prob: dict, abi: dict, flat: A.Flat) -> str | None:
    io = prob["reference"]["io"]
    if sha(json.dumps(abi_core(abi), sort_keys=True).encode()) != io["abi_sha256"]:
        return "the wrapper's ABI (parameters or return type) differs from the reference's"
    if list(flat.inputs) != io["inputs"]:
        return "input witnesses differ from the reference's"
    if len(flat.outputs) != len(io["outputs"]):
        return f"{len(flat.outputs)} return witnesses; the reference has {len(io['outputs'])}"
    return None


def load_problem(problem_dir: str) -> dict:
    prob = read_json(os.path.join(problem_dir, "problem.json"))
    errs = P.validate_problem(prob, problem_dir)
    if errs or prob.get("schema_version") != P.RATCHET_NOIR_SCHEMA_VERSION:
        raise RuntimeError(f"{problem_dir}: not a valid Noir ratchet problem: {errs[:5]}")
    return prob


def resolve_snapshot(problem_dir: str, prob: dict, snapshot: str | None) -> str:
    return snapshot or os.path.join(os.path.dirname(os.path.abspath(problem_dir)), os.pardir, "snapshots",
                                    prob["snapshot"]["id"])


def compile_candidate(prob: dict, problem_dir: str, cand_path: str, out: str, snapshot: str, tools: str) \
        -> tuple[dict, dict]:
    """Source rules, compile, ABI and I/O, metric.  Returns (report, compile result); raises Reject."""
    RT.verify_snapshot(snapshot, prob["snapshot"]["manifest_sha256"])
    os.makedirs(out, exist_ok=True)
    text = admit_candidate(cand_path)
    rep: dict = {"problem": prob["package_id"], "candidate_file": os.path.abspath(cand_path), "started_utc": now(),
                 "source_sha256": sha(text.encode())}
    res = compile_program(prob, problem_dir, snapshot, tools, os.path.join(out, "compile"), text)
    rep["compile"] = {k: v for k, v in res.items() if k not in ("decoded", "prog", "flat", "program")}
    if "error" in res:
        raise Reject(f"compile failed: {res['error']}")
    why = io_mismatch(prob, res["decoded"].get("abi") or {}, res["flat"])
    if why:
        raise Reject(why)
    try:
        cnt = cost_of(res["flat"].opcodes, abi_ranges(res["decoded"].get("abi"), res["flat"].inputs))
        NE.opcode_conjuncts(res["flat"].opcodes, A.bb_keys(res["flat"].opcodes))
    except A.Unsupported as ex:
        raise Reject(f"the candidate's ACIR has no model: {ex}") from None
    rep["candidate"] = {"source_sha256": rep["source_sha256"], "local_files": ["Candidate.nr"],
                        "acir_sha256": res["acir_sha256"], "program_sha256": res["program_sha256"],
                        "n_wires": res["flat"].n_witnesses, **score(prob["record"], cnt)}
    return rep, res


# ------------------------------------------------------------------------------------------ statement

def namespaces(prob: dict) -> tuple[str, str, str]:
    return RT.namespaces(prob)


def candidate_keys(prob: dict, flat: A.Flat) -> list[str] | None:
    """Black-box numbering of the candidate model: the reference's keys, then keys new in the candidate (None when the
    candidate calls no black box)."""
    own = A.bb_keys(flat.opcodes)
    if not own:
        return None
    ref = list(prob["reference"]["model"]["black_boxes"])
    return ref + [k for k in own if k not in ref]


def candidate_model(prob: dict, res: dict) -> tuple[str, dict]:
    _, _, cand_ns = namespaces(prob)
    ref = prob["reference"]
    flat = res["flat"]
    names = {i: f"param `{n}`" for i, n in enumerate(NE.abi_witness_names(res["decoded"].get("abi") or {}))
             if i in set(flat.inputs)}
    for k, o in enumerate(flat.outputs):
        names[o] = f"return value [{k}]"
    meta = {"repo_id": "ratchet candidate for " + ref["repo"], "instantiation": ref["call"],
            "repo_url": "(candidate file)", "commit": "-", "path": "Candidate.nr", "template": CANDIDATE_FN,
            "rule": "ratchet-candidate",
            "generator": f"{GENERATOR_NAME} v{GENERATOR_VERSION}", "nargo_version": ref["compiler"]["version"],
            "nargo_command": "nargo " + " ".join(NARGO_COMMAND), "acir_sha256": res["acir_sha256"]}
    text, info = NE.emit_model(cand_ns, meta, flat, names, candidate_keys(prob, flat))
    return text.replace("# DET model: ", "# Ratchet candidate model: ", 1), info


def statement_text(prob: dict, cand: dict, cand_bb: bool) -> str:
    base, ref_ns, cand_ns = namespaces(prob)
    ref, rec, io = prob["reference"], prob["record"], prob["reference"]["io"]
    rf = ref_ns + "."
    ref_bb = bool(ref["model"]["black_boxes"])
    binder = f"(bb : ℕ → List {rf}F → List {rf}F) " if (ref_bb or cand_bb) else ""
    rarg, carg = ("bb " if ref_bb else ""), ("bb " if cand_bb else "")
    keys = ", ".join(f"{k}: {x}" for k, x in enumerate(ref["model"]["black_boxes"])) or "none"
    return "\n".join([
        f"import {NE.model_module(ref_ns)}",
        f"import {NE.model_module(cand_ns)}",
        "",
        f"namespace {base}",
        "",
        f"/-- Ratchet equivalence for {ref['repo']} `{ref['call']}` ({ref['path']} at {ref['commit'][:12]}).",
        f"Reference: the registry DET model `{ref_ns}` of package `{ref['registry_package_id']}`",
        f"(nargo {ref['compiler']['version']}, ACIR sha256 `{ref['model']['acir_sha256']}`); its DET is "
        f"machine-checked ({prob['det']['method']}).",
        f"Record: {cost_text(rec)}.",
        f"Candidate `Cand`: source sha256 `{cand['source_sha256']}`, compiled in the reference's wrapper",
        f"(ACIR sha256 `{cand['acir_sha256']}`): {cost_text(cand)}.",
        f"Inputs (same ABI, same witnesses): {len(io['inputs'])}; outputs: {len(io['outputs'])} return witness(es).",
        f"Black boxes are uninterpreted and shared: `bb k` is black box k of the reference numbering ({keys}),",
        "keys new in the candidate follow.",
        "For every interpretation of the black boxes, input vector `x` and output vector `y`: some assignment",
        "satisfies the reference constraints with inputs `x` and outputs `y` iff some assignment satisfies the",
        "candidate constraints with inputs `x` and outputs `y`. -/",
        f"theorem equiv [Fact (Nat.Prime {rf}p)] :",
        f"    ∀ {binder}(x y : List {rf}F),",
        f"      (∃ w : Fin {rf}nWires → {rf}F, {rf}Constraints {rarg}w ∧ {rf}Inputs.map w = x ∧ "
        f"{rf}Outputs.map w = y) ↔",
        f"      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints {carg}w ∧ Cand.Inputs.map w = x ∧ "
        "Cand.Outputs.map w = y) := by",
        "  sorry",
        "",
        f"end {base}",
        "",
    ])


def statement_assumptions(prob: dict, cand_bb: bool) -> list[str]:
    out = ["[Fact (Nat.Prime p)]: primality of the BN254 scalar field order, supplied as an instance hypothesis so "
           "that field lemmas apply; p is prime, so the hypothesis does not weaken the statement."]
    if prob["reference"]["model"]["black_boxes"] or cand_bb:
        out.append("Black boxes are uninterpreted functions `bb k` (numbered like the reference model), quantified "
                   "over every interpretation and shared by both models: the equivalence holds for each "
                   "interpretation, in particular for the functions ACVM computes.")
    out.append("Brillig calls add no constraint: their outputs are free witnesses in both models.")
    return out


def checker_files(prob: dict, stmt: str, cmodel: str) -> list[dict]:
    _, ref_ns, cand_ns = namespaces(prob)
    m = prob["reference"]["model"]
    return [{"path": "Statement.lean", "role": "statement", "sha256": sha(stmt.encode())},
            {"path": m["file"], "role": "import", "module": NE.model_module(ref_ns), "sha256": m["sha256"]},
            {"path": NE.model_relpath(cand_ns), "role": "import", "module": NE.model_module(cand_ns),
             "sha256": sha(cmodel.encode())}]


def candidate_problem(prob: dict, cand: dict, files: list[dict], stmt: str, type_sha: str, cand_bb: bool) -> dict:
    """The checker package record (status CANDIDATE)."""
    from zk_registry import gates as G
    base, ref_ns, cand_ns = namespaces(prob)
    fqn = base + ".equiv"
    out = {k: v for k, v in prob.items() if k != "annotations"}
    out.update(status="CANDIDATE", candidate=cand, created_utc=now(),
               statement={"file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn, "text": stmt,
                          "imports": [NE.model_module(ref_ns), NE.model_module(cand_ns)],
                          "assumptions": statement_assumptions(prob, cand_bb)},
               checker={"statement_file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn,
                        "lean_opts": prob["reference"]["lean_opts"], "files": files,
                        "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                        "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS})
    return out


def write_candidate_package(prob: dict, problem_dir: str, cand: dict, res: dict, pkg: str) -> tuple[str, list, bool]:
    """Statement and models of a candidate under ``pkg``; returns (statement text, checker files, candidate has
    black boxes)."""
    _, _, cand_ns = namespaces(prob)
    cmodel, info = candidate_model(prob, res)
    stmt = statement_text(prob, cand, info["bb"])
    fresh_dir(pkg)
    m = prob["reference"]["model"]
    os.makedirs(os.path.dirname(os.path.join(pkg, m["file"])), exist_ok=True)
    shutil.copyfile(os.path.join(problem_dir, m["file"]), os.path.join(pkg, m["file"]))
    if P.sha256_file(os.path.join(pkg, m["file"])) != m["sha256"]:
        raise RuntimeError("reference model file changed")
    put(os.path.join(pkg, NE.model_relpath(cand_ns)), cmodel)
    put(os.path.join(pkg, "Statement.lean"), stmt)
    return stmt, checker_files(prob, stmt, cmodel), info["bb"]


def run_candidate(problem_dir: str, cand_path: str, out: str, tools: str, snapshot: str | None = None,
                  env: L.LeanEnv | None = None, require_smaller: bool = False) -> dict:
    """The trusted candidate pipeline: admissibility, compile, metric, statement and models, and with a Lean
    environment the statement's elaboration and the checker package (``<out>/pkg``)."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, res = compile_candidate(prob, problem_dir, cand_path, out, snapshot, tools)
    cand = rep["candidate"]
    if require_smaller and not cand["smaller"]:
        P.write_json(os.path.join(out, "candidate.json"), rep)
        raise Reject(f"not smaller: {cost_text(cand)}; record {cost_text(prob['record'])}")
    pkg = os.path.join(out, "pkg")
    stmt, files, cand_bb = write_candidate_package(prob, problem_dir, cand, res, pkg)
    rep["statement_sha256"] = files[0]["sha256"]
    if env is not None:
        base, _, _ = namespaces(prob)
        el = RT.elaborate(env, pkg, [f["path"] for f in files[1:]], base + ".equiv", prob["reference"]["lean_opts"],
                          os.path.join(out, "elab"))
        rep["elab"] = el
        if el["status"] != "PASS":
            raise RuntimeError(f"statement does not elaborate: {el.get('errors')}")
        cprob = candidate_problem(prob, cand, files, stmt, el["reference_type_sha256"], cand_bb)
        errs = P.validate_problem(cprob, pkg)
        if errs:
            raise RuntimeError(f"checker package does not validate: {errs[:5]}")
        P.write_json(os.path.join(pkg, "problem.json"), cprob)
        rep["package"] = pkg
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "candidate.json"), rep)
    return rep


def run_check(problem_dir: str, cand_path: str, solution: str, out: str, tools: str, env: L.LeanEnv,
              snapshot: str | None = None, timeout: float = DET_PROOF_LIMITS["timeout_s"]) -> tuple[str, dict]:
    """Final check: the candidate pipeline (component-wise smaller) and the production checker."""
    rep: dict = {"problem_dir": os.path.abspath(problem_dir), "solution_file": os.path.abspath(solution),
                 "started_utc": now()}
    try:
        cr = run_candidate(problem_dir, cand_path, os.path.join(out, "candidate"), tools, snapshot, env,
                           require_smaller=True)
    except Reject as ex:
        rep.update(verdict="REJECTED", reason=str(ex))
        return "REJECTED", rep
    rep["candidate"] = cr["candidate"]
    if not os.path.isfile(solution):
        rep.update(verdict="ERROR", reason=f"no solution file {solution}")
        return "ERROR", rep
    chk = C.check(cr["package"], solution, env, None, timeout, False, os.path.join(out, "checker"))
    rep["checker"] = chk
    rep["verdict"] = chk["verdict"]
    return chk["verdict"], rep


# ------------------------------------------------------------------------------------------ simulation screen

SIM_PROFILES = ("zero", "one", "max", "small", "digits", "ascii")


def sim_vectors(abi: dict, seed: str, n_random: int) -> list[tuple[str, dict]]:
    """Deterministic ABI inputs: every parameter at one profile (zero, one, max, small, digits, ascii), each parameter
    at its maximum (others zero) and at zero (others maximal), then ``n_random`` seeded vectors mixing profiles (a
    prefix of a longer run)."""
    params = abi.get("parameters", [])
    out, seen = [], set()

    def add(label, obj):
        key = json.dumps(obj, sort_keys=True)
        if key not in seen:
            seen.add(key)
            out.append((label, obj))

    def vec(label, prof_of):
        rng = random.Random(f"{seed}|{label}")
        add(label, {p["name"]: ND.sample_value(p["type"], rng, prof_of(p["name"])) for p in params})
    for prof in SIM_PROFILES:
        vec(f"edge: all {prof}", lambda name, prof=prof: prof)
    if len(params) > 1:
        for p in params:
            vec(f"edge: {p['name']} max, others zero", lambda name, q=p["name"]: "max" if name == q else "zero")
            vec(f"edge: {p['name']} zero, others max", lambda name, q=p["name"]: "zero" if name == q else "max")
    rng = random.Random(seed)
    for j in range(n_random):
        prof = rng.choice(["random", "random", "mixed", "mixed", "small", "max", "digits", "ascii"])
        obj = {p["name"]: ND.sample_value(p["type"], rng, prof) for p in params}
        out.append((f"random #{j} ({prof})", obj))
    return out


HARNESS_ERRORS = ("input line:", "input:", "encode:")


def run_executor(tool: str, program: str, vectors: list[dict], work: str, tag: str) -> list[dict]:
    """The pinned executor (``boole-acir-tool execute``, oracle calls answered with zeros) on every input object."""
    os.makedirs(work, exist_ok=True)
    inp, outp = os.path.join(work, tag + ".inputs.jsonl"), os.path.join(work, tag + ".results.jsonl")
    put(inp, "".join(json.dumps(v) + "\n" for v in vectors))
    r = L.run_process([tool, "execute", program, inp, outp], tool_env(os.path.join(work, "home"),
                      os.path.join(work, "tmp")), work, 3600, 16384)
    if r.rc != 0 or not os.path.exists(outp):
        raise RuntimeError(f"executor failed: {r.out[-300:]}")
    with open(outp, encoding="utf-8") as f:
        res = [json.loads(ln) for ln in f if ln.strip()]
    if len(res) != len(vectors):
        raise RuntimeError("executor returned a different number of results")
    bad = [x["error"] for x in res if not x.get("ok") and str(x.get("error", "")).startswith(HARNESS_ERRORS)]
    if bad:
        raise RuntimeError(f"simulation input objects do not fit the ABI: {bad[0][:200]}")
    return res


def ints(m: dict) -> dict[int, int]:
    return {int(k): int(v, 16) for k, v in m.items()}


def recheck(flat: A.Flat, ex: dict) -> bool:
    """An executor witness checked against the decoded ACIR by the Python evaluator (black boxes: the witness's own
    calls, which must agree with each other)."""
    w = A.flat_witness(flat, ints(ex["witness"]), [dict(c, witness=ints(c["witness"])) for c in ex.get("calls", [])])
    if w is None:
        return False
    interp = A.Interp()
    for key, ins, outs, _ in A.bb_calls(flat.opcodes, w):
        prev = interp(key, ins)
        if prev is not None and prev != outs:
            return False
        interp.add(key, ins, outs)
    return A.check(flat.opcodes, w, interp)[0]


def outputs_of(flat: A.Flat, ex: dict) -> list[str]:
    w = ints(ex["witness"])
    return [str(w.get(o, 0)) for o in flat.outputs]


def describe(ex: dict, flat: A.Flat) -> str:
    if not ex.get("ok"):
        return f"fails ({str(ex.get('error'))[:80]})"
    o = outputs_of(flat, ex)
    return "returns [" + ", ".join(o[:6]) + (f", ... ({len(o)} values)" if len(o) > 6 else "") + "]"


def constrained(ops: list[dict]) -> set[int]:
    return set(ND.constrained_witnesses(ops))


def compare_runs(vectors: list[tuple[str, dict]], ref: list[dict], cand: list[dict], ref_flat: A.Flat,
                 cand_flat: A.Flat) -> dict:
    """The screen's verdict: the candidate must fail on exactly the reference's failing vectors and return the same
    values elsewhere; a candidate witness that the evaluator rejects and a return witness in no constrained opcode
    are mismatches too."""
    mism, kinds = [], collections.Counter()
    ref_bad = 0
    for (label, obj), a, b in zip(vectors, ref, cand):
        kind = None
        if a.get("ok") and not recheck(ref_flat, a):
            ref_bad += 1
        if b.get("ok") and not recheck(cand_flat, b):
            kind = "candidate witness violates the candidate ACIR"
        elif not a.get("ok") and b.get("ok"):
            kind = "reference fails, candidate succeeds"
        elif a.get("ok") and not b.get("ok"):
            kind = "reference succeeds, candidate fails"
        elif a.get("ok") and outputs_of(ref_flat, a) != outputs_of(cand_flat, b):
            kind = "different outputs"
        if kind:
            kinds[kind] += 1
            if len(mism) < 20:
                mism.append({"vector": label, "kind": kind, "input": obj, "reference": describe(a, ref_flat),
                             "candidate": describe(b, cand_flat)})
    free = [k for k, o in enumerate(cand_flat.outputs)
            if o not in constrained(cand_flat.opcodes) and o not in set(cand_flat.inputs)]
    if free:
        kinds["structural: return witness in no constrained opcode"] += len(free)
    return {"vectors": len(vectors), "edge_cases": sum(1 for lb, _ in vectors if lb.startswith("edge")),
            "reference_succeeds": sum(1 for x in ref if x.get("ok")),
            "reference_fails": sum(1 for x in ref if not x.get("ok")), "reference_recheck_failures": ref_bad,
            "mismatches": sum(kinds.values()), "mismatch_kinds": dict(kinds), "examples": mism,
            "unconstrained_outputs": free[:20], "verdict": "PASS" if not kinds else "MISMATCH"}


def det_screen(flat: A.Flat) -> dict:
    """Informational: battery P3 propagation on the candidate's ACIR (DETERMINED, or STUCK: a possible
    under-constrained hint)."""
    try:
        res = M.solve(M.make_pkg(flat))
        return {"propagation": res.status}
    except Exception as ex:                                  # noqa: BLE001 - informational only
        return {"propagation": "ERROR", "error": str(ex)[:200]}


def simulate(problem_dir: str, cand_path: str, out: str, n_random: int, tools: str,
             snapshot: str | None = None) -> dict:
    """The simulation screen (not a proof) on a candidate."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, res = compile_candidate(prob, problem_dir, cand_path, os.path.join(out, "count"), snapshot, tools)
    ref = prob["reference"]
    _, _, tool = tool_paths(tools, prob)
    ref_prog = os.path.join(problem_dir, ref["program"]["file"])
    if P.sha256_file(ref_prog) != ref["program"]["sha256"]:
        raise RuntimeError("reference program differs from the problem record")
    ref_flat = A.flatten(A.normalize_program(decode(tool, ref_prog, os.path.join(out, "refdecode"))))
    vectors = sim_vectors(read_json(ref_prog)["abi"], prob["simulate"]["seed"], n_random)
    objs = [v for _, v in vectors]
    t0 = time.time()
    a = run_executor(tool, ref_prog, objs, os.path.join(out, "run"), "ref")
    b = run_executor(tool, res["program"], objs, os.path.join(out, "run"), "cand")
    sim = compare_runs(vectors, a, b, ref_flat, res["flat"])
    sim.update(random=n_random, seed=prob["simulate"]["seed"], secs=round(time.time() - t0, 2),
               det_screen=det_screen(res["flat"]))
    rep["simulate"] = sim
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "simulate.json"), rep)
    return rep


# ------------------------------------------------------------------------------------------ problem build

def snapshot_rule() -> str:
    return ("library functions: every Nargo.toml and .nr file of the reference crates of the repository at the pinned "
            "commit and of their transitive path dependencies (repo/), and of every git dependency at its recorded "
            "tag in nargo's cache layout (home/nargo/); standard-library items: none (the library is embedded in the "
            "pinned nargo), a single file naming it")


def problem_record(spec: dict, layout: dict, meas: dict, det: dict, snapshot: dict, program: dict, append: dict,
                   tool_sha256: str, annotations: dict | None = None) -> dict:
    pid = "ratchet/" + spec["registry_package_id"]
    tag = compiler_tag(spec)
    env = {k: spec["env"][k] for k in ("lean", "mathlib", "lake_manifest_sha256", "packages") if k in spec["env"]}
    env.update(nargo=spec["nargo_version"], nargo_sha256=spec["nargo_sha256"], acir_tool_sha256=tool_sha256)
    rec_cost = {k: meas["cost"][k] for k in ("nonlinear", "range_bits", "logic_bits", "black_box", "memory", "priced",
                                              "linear", "abi_range_bits", "brillig", "n_opcodes")}
    names = NE.abi_witness_names(meas["abi"])
    rec = {
        "schema_version": P.RATCHET_NOIR_SCHEMA_VERSION, "kind": "ratchet", "family": "noir", "package_id": pid,
        "status": "OPEN",
        "reference": {
            "registry_package_id": spec["registry_package_id"], "repo": spec["repo"], "repo_url": spec["repo_url"],
            "commit": spec["commit"], "path": spec["path"], "function": spec["function"],
            "function_line": spec["function_line"], "call": spec["call"], "plan": spec["plan"],
            "crate": layout.get("crate"), "crate_type": layout.get("crate_type"),
            "target_file": layout.get("target_file"), "wrapper_name": spec["wrapper_name"], "append": append,
            "dependencies": spec["dependencies"],
            "compiler": {"tag": tag, "version": spec["nargo_version"], "nargo_sha256": spec["nargo_sha256"],
                         "command": "nargo " + " ".join(NARGO_COMMAND)},
            "model": dict(spec["model"], acir_sha256=meas["acir_sha256"], n_opcodes=len(meas["flat"].opcodes),
                          n_wires=meas["n_wires"], n_conjuncts=meas["n_conjuncts"], black_boxes=meas["keys"],
                          memory=meas["memory"]),
            "io": {"inputs": meas["inputs"], "outputs": meas["outputs"],
                   "input_names": [names[i] if i < len(names) else f"w{i}" for i in meas["inputs"]],
                   "abi_sha256": sha(json.dumps(abi_core(meas["abi"]), sort_keys=True).encode())},
            "program": program, "lean_opts": spec["lean_opts"]},
        "record": dict(rec_cost, rung=0, source="reference (registry ACIR, nargo export)"),
        "metric": METRIC, "admissibility": list(ADMISSIBILITY), "statement_shape": STATEMENT_DESCRIPTION,
        "determinism": DETERMINISM_NOTE, "det": det,
        "simulate": {"seed": f"{SIM_SEED}|{pid}", "executor": "boole-acir-tool execute (ACVM of the pinned noir "
                     "version; oracle calls answered with zeros)", "profiles": list(SIM_PROFILES)},
        "snapshot": snapshot, "env": env, "generator": generator_info(), "created_utc": now(),
    }
    if annotations:
        rec["annotations"] = annotations
    return rec


def build_problem(reg_dir: str, snapshot: str, out_dir: str, tools: str, det: dict | None, layout: dict,
                  snapshot_id: str, work: str, annotations: dict | None = None) -> dict:
    """Reference package -> ratchet problem package: ``problem.json``, the registry model file byte for byte, the
    exact wrapper text the registry appended (``reference/append.nr``) and the rebuilt reference program
    (``reference/program.json``, whose decoded ACIR must equal the registry's); raises NotEligible."""
    spec = reference_spec(reg_dir)
    why = static_exclusion(spec) or RT.det_exclusion(det, spec["model"]["sha256"])
    if why:
        raise NotEligible(*why)
    meas = measure_reference(reg_dir)
    if meas["cost"]["priced"] == 0:
        raise NotEligible("free-record", "the record has no priced cost (only linear AssertZero and Brillig calls: "
                                         "free in context, no candidate can count)")
    man = RT.verify_snapshot(snapshot)
    tag = compiler_tag(spec)
    tool_sha = P.sha256_file(os.path.join(tools, tag, "boole-acir-tool"))
    text = append_text(spec["plan"], spec["wrapper"], spec["imports"])
    append = {"file": "reference/append.nr", "sha256": sha(text.encode()),
              "how": "appended to target_file of the crate copy" if spec["plan"] == "export" else
                     "src/lib.nr of the wrapper crate (Nargo.toml: lib crate without dependencies)"}
    snap = {"id": snapshot_id, "manifest_sha256": P.sha256_file(os.path.join(snapshot, "MANIFEST.sha256")),
            "n_files": len(man), "rule": snapshot_rule()}
    stage = os.path.join(work, "stage")
    fresh_dir(stage)
    put(os.path.join(stage, "reference", "append.nr"), text)
    pre = {"reference": {"plan": spec["plan"], "crate": layout.get("crate"), "crate_type": layout.get("crate_type"),
                         "target_file": layout.get("target_file"), "wrapper_name": spec["wrapper_name"],
                         "call": spec["call"], "append": append,
                         "compiler": {"tag": tag, "nargo_sha256": spec["nargo_sha256"]}},
           "env": {"acir_tool_sha256": tool_sha}}
    res = compile_program(pre, stage, snapshot, tools, os.path.join(work, "rebuild"))
    if "error" in res:
        raise NotEligible("rebuild", f"the reference does not rebuild: {res['error'][:300]}")
    if res["acir_sha256"] != meas["acir_sha256"]:
        raise NotEligible("rebuild", "the rebuilt reference ACIR differs from the registry's")
    if abi_core(res["decoded"].get("abi") or {}) != abi_core(meas["abi"]):
        raise NotEligible("rebuild", "the rebuilt reference ABI differs from the registry's")
    program = {"file": "reference/program.json", "sha256": res["program_sha256"],
               "note": "the rebuilt reference artifact stripped to {noir_version, abi, bytecode} (executor input)"}
    prob = problem_record(spec, layout, meas, det, snap, program, append, tool_sha, annotations)
    fresh_dir(out_dir)
    dst = os.path.join(out_dir, spec["model"]["file"])
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(os.path.join(reg_dir, spec["model"]["file"]), dst)
    put(os.path.join(out_dir, "reference", "append.nr"), text)
    shutil.copyfile(res["program"], os.path.join(out_dir, "reference", "program.json"))
    errs = P.validate_problem(prob, out_dir)
    if errs:
        raise RuntimeError(f"ratchet problem does not validate: {errs[:5]}")
    P.write_json(os.path.join(out_dir, "problem.json"), prob)
    return prob


# ------------------------------------------------------------------------------------------ CLI

def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for nm in ("count", "simulate", "statement", "check"):
        x = sub.add_parser(nm)
        x.add_argument("--problem", required=True)
        x.add_argument("--candidate", required=True)
        x.add_argument("--out", required=True)
        x.add_argument("--tools", required=True)
        x.add_argument("--snapshot")
        if nm == "simulate":
            x.add_argument("--n", type=int, default=2000)
        if nm in ("statement", "check"):
            x.add_argument("--lean-env", required=True)
            x.add_argument("--rss-mb", type=int, default=DET_PROOF_LIMITS["rss_mb"])
        if nm == "check":
            x.add_argument("--solution", required=True)
    a = ap.parse_args(argv)
    try:
        return _run(a)
    except (RuntimeError, ValueError, OSError) as ex:
        print(f"ERROR {ex}")
        return EXIT["ERROR"]


def _run(a) -> int:
    env = None
    if a.cmd in ("statement", "check"):
        L.set_lean_limits(slots=1, rss_mb=a.rss_mb)
        env = L.load_env(a.lean_env)
    if a.cmd == "check":
        verdict, rep = run_check(a.problem, a.candidate, a.solution, a.out, a.tools, env, a.snapshot)
        rep["finished_utc"] = now()
        P.write_json(os.path.join(a.out, "verdict.json"), rep)
        print(f"{verdict} {rep.get('reason', '')}".rstrip())
        for k in ("error", "invalid", "fail"):
            for x in (rep.get("checker") or {}).get(k, []):
                print(f"  {k}: {x}")
        return EXIT[verdict]
    try:
        if a.cmd == "simulate":
            rep = simulate(a.problem, a.candidate, a.out, a.n, a.tools, a.snapshot)
        elif a.cmd == "count":
            prob = load_problem(a.problem)
            snap = resolve_snapshot(a.problem, prob, a.snapshot)
            rep, _ = compile_candidate(prob, a.problem, a.candidate, a.out, snap, a.tools)
            P.write_json(os.path.join(a.out, "candidate.json"), rep)
        else:
            rep = run_candidate(a.problem, a.candidate, a.out, a.tools, a.snapshot, env)
    except Reject as ex:
        print(f"REJECTED {ex}")
        return 4
    c = rep["candidate"]
    worse = (" larger: " + ", ".join(c["larger_components"])) if c["larger_components"] else ""
    print(f"{'SMALLER' if c['smaller'] else 'NOT-SMALLER'} nonlinear={c['nonlinear']} range_bits={c['range_bits']} "
          f"logic_bits={c['logic_bits']} memory={c['memory']} black_box={sum(c['black_box'].values())} "
          f"priced={c['priced']} (reduction {c['reduction_pct']}%){worse}")
    if a.cmd == "simulate":
        sm = rep["simulate"]
        print(f"SIMULATE {sm['verdict']} vectors={sm['vectors']} mismatches={sm['mismatches']} "
              f"det-screen={sm['det_screen'].get('propagation')}")
        for m in sm["examples"][:3]:
            print(f"  {m['kind']} at {m['vector']}: reference {m['reference'][:100]}; candidate {m['candidate'][:100]}")
        if sm["verdict"] != "PASS":
            return 5
    return 0 if c["smaller"] else 4


if __name__ == "__main__":
    sys.exit(main())
