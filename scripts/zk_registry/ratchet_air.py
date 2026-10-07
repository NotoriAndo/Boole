#!/usr/bin/env python3
"""Ratchet problems (verified circuit optimization) for zkVM AIR references of the registry (SP1, Pico, OpenVM).

The Circom / Noir ratchet (``ratchet.py``, ``ratchet_noir.py``) carried over to the AIR chips of wave Z0:

* **Reference meaning**: the registry package's DET model of the reference AIR (one row window of the AIR's
  extracted constraint and interaction DAG, ``boole-air-ir/v1``, with the bus model's roles; the model file byte for
  byte).  The reference's DET must be machine-checked (the battery P3 proof, or a battery closure re-checked as a
  proof) by the production checker within the proof limits.  Only one-row windows are references: a one-row AIR
  constrains every row on its own, so an equivalence of row windows is an equivalence of traces row by row (a
  two-row window relation does not compose to the trace).
* **Record**: a proving-cost vector of the AIR (see :data:`METRIC`): main trace columns, bus interactions, the
  constraint count and the cumulative counts of constraints of degree >= k, and the maximum degree of an
  interaction expression.  A candidate counts only if no component is larger and at least one is smaller
  (component-wise order): it is then no more expensive under every prover cost that grows with these components.
* **Candidate**: a modified chip -- an overlay of Rust source files (the chip's ``eval`` and, as needed, its trace
  generation) on the problem's workspace snapshot (the zkVM's tracked files at the census pin with the registry
  extractor installed, manifest-checked), built with the recorded toolchain and extracted by the same harness.
* **Admissibility**: overlay files only under the problem's editable roots, Rust sources only, no harness / manifest
  / build-script files, no new ``unsafe`` / reflection / environment / file / include constructs in added lines; every
  other AIR of the machine extracts to the same IR; the target keeps its name, position, field, preprocessed width,
  public values, row selectors and one-row window; its bus interface keeps the same input and output messages (same
  order, directions, buses, scopes, message shapes, count weights and roles) and its table lookups go only to tables
  the reference uses (no new table, no other assumption kind).
* **Statement** (generated per candidate): for every value of the fixed variables and all input and output message
  lists, some window satisfies the reference's constraints and bus assumptions with those fixed values and bus
  contributions iff some window satisfies the candidate's (both directions, over the reference's field).
* **Screen** (``simulate``): the zkVM's own trace generation of both builds on the sample programs (every row of every
  dumped trace) and on random executions (the same generated programs with other seeds): every candidate row
  satisfies the candidate's constraints and assumptions, the preprocessed rows and public values are the reference's,
  and every trace makes the same multiset of active input and output messages; a DET counterexample search on the
  candidate (a counterexample refutes the equivalence, since the reference's DET is proved) and battery P3
  propagation as an informational DET screen.  A screen, not a proof.
* **Final check**: the candidate pipeline (component-wise smaller than the record) and the unchanged production
  checker (``check.py``) on the generated package.

Usage::

    python3 -m zk_registry.ratchet_air count     --problem DIR --candidate DIR --out DIR --build DIR [--snapshot DIR]
    python3 -m zk_registry.ratchet_air simulate  --problem DIR --candidate DIR --out DIR --build DIR
                                                 --ref-build DIR [--seeds 4]
    python3 -m zk_registry.ratchet_air statement --problem DIR --candidate DIR --out DIR --build DIR --lean-env E.json
    python3 -m zk_registry.ratchet_air check     --problem DIR --candidate DIR --solution S --out DIR --build DIR
                                                 --lean-env E.json

``--candidate DIR`` is an overlay (or a whole edited copy of the workspace): every file under it whose content
differs from the snapshot is part of the candidate.  ``--build DIR`` holds the build tree (``DIR/repo``, synchronized
with the snapshot plus the overlay; changed files get a fresh modification time) and the cargo target directory
(``DIR/target``); ``CARGO_HOME`` comes from the environment.  Exit codes as in ``ratchet.py``: ``count`` /
``simulate`` / ``statement``: 0 smaller (``simulate``: and no mismatch), 4 rejected or not smaller, 5 simulation
mismatch, 3 error; ``check``: 0 PASS, 1 FAIL, 2 INVALID, 3 ERROR, 4 REJECTED.
"""
from __future__ import annotations

import argparse
import collections
import difflib
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import air_bus as AB           # noqa: E402
from zk_registry import air_det as AD           # noqa: E402
from zk_registry import air_ir as IR            # noqa: E402
from zk_registry import air_lean as AL          # noqa: E402
from zk_registry import air_search as AS        # noqa: E402
from zk_registry import check as C              # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import mech_air as M           # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import ratchet as RT           # noqa: E402

GENERATOR_NAME = RT.GENERATOR_NAME
GENERATOR_VERSION = "1-air"
GENERATOR_SOURCES = ["ratchet_air.py", "ratchet.py", "schema/ratchet_air_problem.schema.json", "air_ir.py",
                     "air_lean.py", "air_bus.py", "air_search.py", "air_det.py", "mech_air.py", "package.py",
                     "check.py"]
HERE = os.path.dirname(os.path.abspath(__file__))
SCHEMA_VERSION = P.RATCHET_AIR_SCHEMA_VERSION
EXIT = RT.EXIT
DET_PROOF_LIMITS = RT.DET_PROOF_LIMITS
SIM_SEED = "boole-ratchet-air-sim"
MAX_TRACES = 4                      # ratchet mode of the harness: traces per AIR and sample program
MAX_HEIGHT = 1 << 16                # ratchet mode of the harness: taller traces are skipped
TRACE_CELLS = 1 << 21               # ... at most about this many cells per dumped trace (narrow AIRs: 2^16 rows)
LABEL_CELLS = 1 << 22               # ... and per sample program (fewer traces of wide AIRs)
MIN_HEIGHT = 1 << 8
SEARCH_BUDGET_S = 10.0
SEARCH_POOL = 64
BUILD_TIMEOUT, RUN_TIMEOUT = 3600, 3600

Reject, NotEligible = RT.Reject, RT.NotEligible
sha, now, read_json, put, fresh_dir = RT.sha, RT.now, RT.read_json, RT.put, RT.fresh_dir

# Per-zkVM build and run recipe (the registry's wave Z0 configuration).  ``editable``: roots a candidate overlay may
# write (the chips and their trace generation); ``protected``: never (the extractor and build inputs).
ZKVMS = {
    "sp1": {
        "command": ["cargo", "build", "--release", "--offline", "--locked", "-p", "boole-air-extract"],
        "run": "target/release/boole-air-extract <out> [--no-rows]",
        "features": "default (no mprotect)",
        "editable": ["crates/core/machine/src/", "crates/recursion/machine/src/"],
        "protected": ["boole-air-extract/"],
    },
    "pico": {
        "command": ["cargo", "test", "--release", "--offline", "--locked", "-p", "pico-vm", "--lib", "--no-default-features",
                    "--features", "rayon,nightly-features", "--no-run", "--message-format=json"],
        "run": "BOOLE_OUT=<out> <pico-vm test binary> boole_air_extract::extract --nocapture --test-threads 1 "
               "(working directory vm/)",
        "features": "pico-vm --no-default-features --features rayon,nightly-features (default 'strict' = "
                    "deny(warnings) dropped)",
        "editable": ["vm/src/chips/"],
        "protected": ["vm/src/boole_air_extract.rs", "vm/src/boole_air_ir.rs", "vm/src/lib.rs"],
    },
    "openvm": {
        "command": ["cargo", "build", "--release", "--offline", "--locked", "-p", "boole-air-extract"],
        "run": "target/release/boole-air-extract <out> <repo> [--no-rows]",
        "features": "openvm-circuit parallel; openvm-stark-sdk cpu-backend; SdkVmConfig::standard()",
        "editable": ["crates/vm/src/system/", "crates/circuits/", "extensions/*/circuit/src/"],
        "protected": ["boole-air-extract/"],
    },
}
PROTECTED_NAMES = ("Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml", "MANIFEST.sha256")
FORBIDDEN_RUST = [
    (r"\bunsafe\b", "`unsafe`"), (r"\bTypeId\b", "`TypeId`"), (r"\btype_name(_of_val)?\b", "`type_name`"),
    (r"\b(std|core)\s*::\s*any\b", "`std::any`"), (r"\btransmute\b", "`transmute`"),
    (r"\binclude(_str|_bytes)?\s*!", "`include!`"), (r"\b(option_)?env\s*!", "`env!`"),
    (r"\bstd\s*::\s*(env|fs|process|net|io)\b", "`std::env` / `fs` / `process` / `net` / `io`"),
    (r"#\s*!?\s*\[\s*cfg", "`#[cfg]`"), (r"\bcfg\s*!", "`cfg!`"), (r"\b(global_)?asm\s*!", "`asm!`"),
    (r"\bextern\b", "`extern`"), (r"no_mangle|link_section|export_name", "linkage attributes"),
    (r"\bstatic\s+mut\b", "`static mut`"), (r"\bthread_local\b", "`thread_local`"),
]

METRIC = ("component-wise proving cost of the extracted AIR: main_columns (main trace width: committed and "
          "low-degree-extended cells per row), interactions (bus sends and receives, table lookups included: LogUp "
          "terms per row), constraints (constraint polynomials evaluated per row) and constraints_deg_ge[k] for every "
          "k >= 2 (constraints of degree at least k, cumulative, so lowering a degree never counts as a larger "
          "component; the largest k with a non-zero count is the maximum degree, which sets the quotient domain), "
          "and interaction_degree (maximum degree of a multiplicity or message value); a candidate counts only if no "
          "component is larger than the record's and at least one is smaller (it is then no more expensive under "
          "every prover cost that grows with these components, whatever their weights); priced = main_columns + "
          "interactions + constraints, a summary only; preprocessed columns are fixed (admissibility)")
ADMISSIBILITY = [
    "the candidate is an overlay of files on the problem's workspace snapshot (the zkVM's tracked files at the census "
    "pin with the registry extractor installed, manifest-checked): only `.rs` files under the problem's editable "
    "roots, none of the protected paths, no Cargo.toml / Cargo.lock / build.rs / toolchain files; deletions are not "
    "supported",
    "added lines contain no `unsafe`, reflection (`TypeId`, `type_name`, `std::any`), `transmute`, `include!`, "
    "`env!`, `std::env` / `fs` / `process` / `net` / `io`, `#[cfg]` / `cfg!`, `asm!`, `extern`, linkage attributes, "
    "`static mut` or `thread_local`",
    "built with the problem's toolchain and command (offline, locked dependencies of the snapshot) and extracted by the "
    "same harness; every other AIR of the machine extracts to the same IR as the reference build",
    "the target AIR keeps its name, position in the machine, rust type, field, preprocessed width and public value "
    "count; its window is one row and its fixed variables (preprocessed cells, public values, row selectors) are the "
    "reference's; within the registry size policy",
    "bus interface: the same input and output messages (same order, direction, bus, scope, number of values, count "
    "weight and role, split fields included); table lookups (bus assumptions) only into tables the reference uses",
]
STATEMENT_DESCRIPTION = (
    "over F = ZMod p (the zkVM's field, [Fact (Nat.Prime p)]): for every list f of fixed values (preprocessed cells, "
    "public values, row selectors; same layout in both models) and all input and output message lists x and y, "
    "(∃ w, Ref.Constraints w ∧ Ref.Assumptions w ∧ Ref.Fixed.map w = f ∧ Ref.BusEq (Ref.In w) x ∧ "
    "Ref.BusEq (Ref.Out w) y) ↔ (∃ w', the same for Cand); Ref is the registry DET model of the reference AIR (one "
    "row window), Cand the model of the candidate AIR with the same bus model; BusEq compares contributions (equal "
    "multiplicities, equal values where the multiplicity is non-zero)")
DETERMINISM_NOTE = (
    "reference DET + equiv => candidate DET: two candidate windows with equal fixed values and input contributions "
    "map to reference windows with the same fixed values and contributions, whose outputs agree by the reference's "
    "DET, so the candidate outputs agree too")


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in GENERATOR_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


# ------------------------------------------------------------------------------------------ metric

def degrees(air: IR.Air) -> list[int]:
    """Degree of every node (row selectors and cells have degree 1, as in ``Air.degree``)."""
    deg: list[int] = []
    for n in air.nodes:
        op = n[0]
        if op == "const":
            deg.append(0)
        elif op in IR.LEAF_OPS:
            deg.append(1)
        elif op == "neg":
            deg.append(deg[n[1]])
        elif op == "mul":
            deg.append(deg[n[1]] + deg[n[2]])
        else:
            deg.append(max(deg[n[1]], deg[n[2]]))
    return deg


def reachable(air: IR.Air) -> int:
    """DAG nodes reachable from the constraints and interactions (evaluation work per row, reported only)."""
    seen = set()
    stack = list(air.constraints) + [x for it in air.interactions for x in [it.mult] + it.values]
    while stack:
        k = stack.pop()
        if k in seen:
            continue
        seen.add(k)
        n = air.nodes[k]
        if n[0] in IR.BIN_OPS:
            stack += [n[1], n[2]]
        elif n[0] == "neg":
            stack.append(n[1])
    return len(seen)


def cost_of(air: IR.Air, bus: AB.BusModel | None = None) -> dict:
    deg = degrees(air)
    cdeg = [deg[c] for c in air.constraints]
    top = max(cdeg, default=0)
    ge = {str(k): sum(1 for d in cdeg if d >= k) for k in range(2, top + 1)}
    ideg = max((deg[x] for it in air.interactions for x in [it.mult] + it.values), default=0)
    lookups = 0
    if bus is not None:
        lookups = sum(1 for k, it in enumerate(air.interactions) if bus.rule(air, k, it).role == "assume")
    out = {"main_columns": air.width, "interactions": len(air.interactions), "constraints": len(air.constraints),
           "constraints_deg_ge": ge, "interaction_degree": ideg, "max_degree": top}
    out["priced"] = priced(out)
    out.update(lookups=lookups, n_nodes=reachable(air))
    return out


priced = P.air_priced
compare = P.air_cost_compare
cost_smaller = P.air_cost_smaller


def score(record: dict, cnt: dict) -> dict:
    larger, smaller = compare(record, cnt)
    pct = round(100.0 * (record["priced"] - cnt["priced"]) / record["priced"], 2) if record["priced"] else 0.0
    return {"cost": cnt, "smaller": not larger and bool(smaller), "larger_components": larger,
            "smaller_components": smaller, "reduction_pct": pct}


def cost_text(c: dict) -> str:
    ge = ", ".join(f"deg>={k}: {v}" for k, v in sorted(c["constraints_deg_ge"].items(), key=lambda kv: int(kv[0])))
    return (f"{c['main_columns']} main columns, {c['interactions']} interactions ({c['lookups']} table lookups), "
            f"{c['constraints']} constraints ({ge or 'all of degree <= 1'}), interaction degree "
            f"{c['interaction_degree']} (priced {c['priced']})")


# ------------------------------------------------------------------------------------------ interface

def shape(it: IR.Interaction) -> tuple:
    return (it.direction, it.kind, it.kind_name, it.bus, it.scope, len(it.values), it.count_weight)


def iface_roles(iface: dict) -> dict[int, tuple]:
    """Interaction -> (role, table, input fields, output fields) of a recorded interface."""
    out: dict[int, list] = {}
    for side in ("inputs", "outputs", "assumptions"):
        for rec in iface[side]:
            k = rec["interaction"]
            cur = out.setdefault(k, [rec["role"], rec.get("table"), None, None])
            if rec["role"] == "split":
                cur[2 if side == "inputs" else 3] = tuple(rec["fields"])
    return {k: tuple(v) for k, v in out.items()}


def rule_key(r: AB.Rule) -> tuple:
    if r.role == "split":
        return (r.role, r.table, tuple(r.in_fields), tuple(r.out_fields))
    return (r.role, r.table, None, None)


def reference_negative(air: IR.Air, iface: dict, bus: AB.BusModel) -> list[int]:
    """The interactions the registry assigned with the negative-multiplicity rule (their recorded role differs from
    the bus model's role without it); the recorded interface must be reproducible from the bus model."""
    rec = iface_roles(iface)
    neg = []
    for k, it in enumerate(air.interactions):
        if k not in rec:
            raise NotEligible("interface", f"interaction {k} has no recorded role")
        if rule_key(bus.rule(air, k, it, False)) != rec[k]:
            if rule_key(bus.rule(air, k, it, True)) != rec[k]:
                raise NotEligible("interface", f"interaction {k}: the recorded role is not the bus model's")
            neg.append(k)
    return neg


def interface_view(iface: dict) -> dict:
    """The parts of an interface a candidate must keep: input and output messages in order (direction, bus, role,
    fields) and the tables of the lookups."""
    def msg(rec):
        return [rec["direction"], rec["bus"], rec["role"], rec.get("fields")]
    return {"inputs": [msg(r) for r in iface["inputs"]], "outputs": [msg(r) for r in iface["outputs"]],
            "tables": sorted({r["table"] for r in iface["assumptions"]})}


def candidate_roles(prob: dict, ref_air: IR.Air, cand_air: IR.Air, bus: AB.BusModel) -> tuple[AL.Roles, dict]:
    """The candidate's roles: the bus model's, with the negative-multiplicity rule on the interactions that match a
    reference interaction assigned with it; raises Reject unless the interface is the reference's."""
    ref = prob["reference"]
    rec = iface_roles(ref["interface"])
    ref_msgs = [k for k in range(len(ref_air.interactions)) if rec[k][0] != "assume"]
    cand_msgs = [k for k, it in enumerate(cand_air.interactions) if bus.rule(cand_air, k, it).role != "assume"]
    if len(cand_msgs) != len(ref_msgs):
        raise Reject(f"bus interface: {len(cand_msgs)} non-lookup interactions, the reference has {len(ref_msgs)}")
    neg = set(ref["negative"])
    cand_neg = set()
    for j, (a, b) in enumerate(zip(ref_msgs, cand_msgs)):
        sa, sb = shape(ref_air.interactions[a]), shape(cand_air.interactions[b])
        if sa != sb:
            raise Reject(f"bus interface: message {j} (reference interaction {a}, candidate interaction {b}) has shape "
                         f"{list(sb)}; the reference's is {list(sa)} (direction, kind, name, bus, scope, values, "
                         "count weight)")
        if a in neg:
            cand_neg.add(b)
    roles, iface = bus.roles(cand_air, frozenset(cand_neg))
    want, got = interface_view(ref["interface"]), interface_view(iface)
    for side in ("inputs", "outputs"):
        if got[side] != want[side]:
            raise Reject(f"bus interface: the {side} differ from the reference's ({got[side][:3]} ... vs "
                         f"{want[side][:3]} ...)")
    extra = sorted(set(got["tables"]) - set(want["tables"]))
    if extra:
        raise Reject(f"bus interface: lookups into tables the reference does not use: {extra}")
    iface["negative"] = sorted(cand_neg)
    return roles, iface


def screen_limits(air: IR.Air) -> tuple[int, int]:
    """(traces per sample program, maximum trace height) of the problem's screen: at most ``TRACE_CELLS`` main and
    preprocessed cells per dumped trace (a power of two between ``MIN_HEIGHT`` and ``MAX_HEIGHT`` rows) and about
    ``LABEL_CELLS`` per sample program (at least one trace, at most ``MAX_TRACES``)."""
    w = air.width + air.prep_width
    h = MAX_HEIGHT
    while h > MIN_HEIGHT and h * w > TRACE_CELLS:
        h //= 2
    return max(1, min(MAX_TRACES, LABEL_CELLS // (h * w))), h


def fixed_names(air: IR.Air) -> list[str]:
    lay = IR.Layout.of(air)
    names = lay.names()
    return [names[i] for i in lay.fixed()]


# ------------------------------------------------------------------------------------------ overlay (source rules)

def _under(rel: str, root: str) -> bool:
    if "*" in root:
        pat = "^" + re.escape(root).replace(r"\*", "[^/]+")
        return re.match(pat, rel) is not None
    return rel.startswith(root)


def manifest_of(snapshot: str, want_sha: str | None = None) -> dict[str, str]:
    """The verified snapshot manifest (relative path under ``repo/`` -> sha256); the snapshot's symbolic links must be
    the ones ``LINKS.json`` (part of the manifest) lists."""
    man = RT.verify_snapshot(snapshot, want_sha)
    snapshot_links(snapshot)
    return {rel[len("repo/"):]: h for rel, h in man.items() if rel.startswith("repo/")}


def snapshot_links(snapshot: str) -> dict[str, str]:
    path = os.path.join(snapshot, "LINKS.json")
    links = read_json(path) if os.path.isfile(path) else {}
    found = {}
    for root, dirs, names in os.walk(os.path.join(snapshot, "repo")):
        for n in dirs + names:
            p = os.path.join(root, n)
            if os.path.islink(p):
                found[os.path.relpath(p, os.path.join(snapshot, "repo")).replace(os.sep, "/")] = os.readlink(p)
    if found != links:
        raise RuntimeError(f"snapshot {snapshot}: symbolic links differ from LINKS.json")
    return links


def make_snapshot(tree: str, dest: str) -> str:
    """A snapshot of a prepared workspace tree (``target/`` and ``.git/`` excluded): ``dest/repo``, ``LINKS.json``
    and ``MANIFEST.sha256``; returns the manifest's sha256."""
    if os.path.exists(dest):
        raise RuntimeError(f"{dest} exists")
    shutil.copytree(tree, os.path.join(dest, "repo"), symlinks=True,
                    ignore=lambda d, names: [n for n in names if d == tree and n in ("target", ".git")])
    links = {}
    for root, dirs, names in os.walk(os.path.join(dest, "repo")):
        for n in dirs + names:
            p = os.path.join(root, n)
            if os.path.islink(p):
                links[os.path.relpath(p, os.path.join(dest, "repo")).replace(os.sep, "/")] = os.readlink(p)
    P.write_json(os.path.join(dest, "LINKS.json"), dict(sorted(links.items())))
    return RT.write_manifest(dest)


def overlay_of(prob: dict, snapshot: str, cand_dir: str, man: dict[str, str] | None = None) -> dict[str, bytes]:
    """The candidate's overlay: every file under ``cand_dir`` whose content differs from the snapshot (raises Reject
    on a source rule)."""
    if not os.path.isdir(cand_dir):
        raise Reject(f"candidate {cand_dir} is not a directory")
    man = man if man is not None else manifest_of(snapshot, prob["snapshot"]["manifest_sha256"])
    build = prob["build"]
    out: dict[str, bytes] = {}
    for root, dirs, names in os.walk(cand_dir):
        dirs[:] = sorted(d for d in dirs if not (root == cand_dir and d in ("target", ".git")))
        for n in sorted(names):
            p = os.path.join(root, n)
            rel = os.path.relpath(p, cand_dir).replace(os.sep, "/")
            if os.path.islink(p):
                if rel in man:
                    raise Reject(f"{rel}: symbolic links are not allowed")
                continue
            with open(p, "rb") as f:
                data = f.read()
            if man.get(rel) == sha(data):
                continue
            out[rel] = data
    if not out:
        raise Reject("the candidate changes no file of the snapshot")
    for rel, data in sorted(out.items()):
        base = rel.rsplit("/", 1)[-1]
        if base in PROTECTED_NAMES or any(rel.startswith(x) or rel == x for x in build["protected"]):
            raise Reject(f"{rel}: protected file (extractor, manifests, build scripts, toolchain)")
        if not rel.endswith(".rs"):
            raise Reject(f"{rel}: only Rust sources (.rs) may change")
        if not any(_under(rel, r) for r in build["editable"]):
            raise Reject(f"{rel}: outside the editable roots {build['editable']}")
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError:
            raise Reject(f"{rel}: not UTF-8 text") from None
        old = ""
        if rel in man:
            with open(os.path.join(snapshot, "repo", rel), encoding="utf-8") as f:
                old = f.read()
        added = [ln[1:] for ln in difflib.unified_diff(old.splitlines(), text.splitlines(), lineterm="", n=0)
                 if ln.startswith("+") and not ln.startswith("+++")]
        for ln in added:
            code = re.sub(r"//.*$", "", ln)
            for pat, what in FORBIDDEN_RUST:
                if re.search(pat, code):
                    raise Reject(f"{rel}: added line uses {what}: {ln.strip()[:120]}")
    return out


def overlay_digest(overlay: dict[str, bytes]) -> str:
    h = hashlib.sha256()
    for rel in sorted(overlay):
        h.update(rel.encode() + b"\0" + sha(overlay[rel]).encode() + b"\n")
    return h.hexdigest()


# ------------------------------------------------------------------------------------------ build and extraction

def sync_tree(snapshot: str, man: dict[str, str], overlay: dict[str, bytes], dest: str) -> dict:
    """Make ``dest`` the snapshot's ``repo/`` plus the overlay: a file is (re)written only when its content differs
    (with a fresh modification time, so cargo rebuilds exactly the changed crates); files of earlier overlays that
    the snapshot does not have are removed."""
    os.makedirs(dest, exist_ok=True)
    want = dict(man)
    for rel, data in overlay.items():
        want[rel] = sha(data)
    written = removed = 0
    for rel, target in snapshot_links(snapshot).items():
        p = os.path.join(dest, rel)
        if os.path.islink(p) and os.readlink(p) == target:
            continue
        if os.path.lexists(p):
            raise RuntimeError(f"build tree {dest}: {rel} should be a symbolic link")
        os.makedirs(os.path.dirname(p), exist_ok=True)
        os.symlink(target, p)
    for rel, h in want.items():
        p = os.path.join(dest, rel)
        if os.path.isfile(p) and not os.path.islink(p) and P.sha256_file(p) == h:
            continue
        if os.path.islink(p) or os.path.isdir(p):
            raise RuntimeError(f"build tree {dest}: {rel} is not a regular file")
        os.makedirs(os.path.dirname(p), exist_ok=True)
        if rel in overlay:
            with open(p, "wb") as f:
                f.write(overlay[rel])
        else:
            shutil.copyfile(os.path.join(snapshot, "repo", rel), p)
        written += 1
    for root, dirs, names in os.walk(dest):
        dirs[:] = [d for d in dirs if not (root == dest and d in ("target", ".git"))
                   and not os.path.islink(os.path.join(root, d))]
        for n in names:
            p = os.path.join(root, n)
            rel = os.path.relpath(p, dest).replace(os.sep, "/")
            if rel not in want and rel.endswith(".rs") and not os.path.islink(p):
                os.remove(p)
                removed += 1
    return {"written": written, "removed": removed}


def build_env(prob: dict, build_dir: str) -> dict:
    env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTC_WRAPPER",
                                                            "CARGO_ENCODED_RUSTFLAGS", "BOOLE_RATCHET_AIRS",
                                                            "BOOLE_AIR_SEED", "BOOLE_AIR_NO_ROWS")}
    env.update(RUSTUP_TOOLCHAIN=prob["build"]["toolchain"], RUSTUP_AUTO_INSTALL="0",
               CARGO_TARGET_DIR=os.path.join(build_dir, "target"), CARGO_TERM_COLOR="never",
               CARGO_NET_OFFLINE="true", GIT_TERMINAL_PROMPT="0")
    env.setdefault("CARGO_INCREMENTAL", "0")
    return env


def _run(cmd: list[str], env: dict, cwd: str, timeout: float, log: str) -> tuple[int, str]:
    t0 = time.time()
    with open(log, "w", encoding="utf-8") as f:
        try:
            r = subprocess.run(cmd, env=env, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                               timeout=timeout)
            out, rc = r.stdout, r.returncode
        except subprocess.TimeoutExpired as ex:
            out, rc = (ex.stdout or "") if isinstance(ex.stdout, str) else "", -9
        f.write(out)
        f.write(f"\n[exit {rc} after {time.time() - t0:.1f} s]\n")
    return rc, out


def build(prob: dict, snapshot: str, man: dict[str, str], overlay: dict[str, bytes], build_dir: str,
          work: str) -> dict:
    """Synchronize the build tree and build the extractor; returns the binary and the build record."""
    zk = prob["reference"]["zkvm"]
    repo = os.path.join(build_dir, "repo")
    t0 = time.time()
    sync = sync_tree(snapshot, man, overlay, repo)
    env = build_env(prob, build_dir)
    os.makedirs(work, exist_ok=True)
    rc, out = _run(prob["build"]["command"], env, repo, BUILD_TIMEOUT, os.path.join(work, "build.log"))
    rec = {"sync": sync, "rc": rc, "secs": round(time.time() - t0, 1)}
    if rc != 0:
        errs = [ln for ln in out.splitlines() if ln.startswith("error")][:5]
        rec["error"] = ("; ".join(errs) or out[-600:]).replace(build_dir, "<build>")[:1500]
        return rec
    if zk == "pico":
        exe = None
        for ln in out.splitlines():
            try:
                d = json.loads(ln)
            except ValueError:
                continue
            if d.get("reason") == "compiler-artifact" and d.get("executable") and \
                    (d.get("target") or {}).get("name") == "pico_vm":
                exe = d["executable"]
        if not exe:
            rec["error"] = "no pico-vm test binary in the build output"
            return rec
        rec["binary"] = exe
    else:
        rec["binary"] = os.path.join(build_dir, "target", "release", "boole-air-extract")
    rec["binary_sha256"] = P.sha256_file(rec["binary"])
    return rec


def extract(prob: dict, build_dir: str, binary: str, out: str, rows: bool, seed: int = 0) -> dict:
    """Run the harness: every AIR's IR (``out/airs``) and, with ``rows``, every row of the target's traces
    (``out/rows-all/<index>``) on the sample programs of ``seed``."""
    zk = prob["reference"]["zkvm"]
    repo = os.path.join(build_dir, "repo")
    fresh_dir(out)
    env = build_env(prob, build_dir)
    env.update(BOOLE_RATCHET_AIRS=str(prob["reference"]["air_index"]), BOOLE_AIR_SEED=str(seed),
               BOOLE_RATCHET_MAX_TRACES=str(prob["simulate"]["max_traces"]),
               BOOLE_RATCHET_MAX_HEIGHT=str(prob["simulate"]["max_height"]))
    res_dir = os.path.join(out, "out")
    if zk == "pico":
        env["BOOLE_OUT"] = res_dir
        if not rows:
            env["BOOLE_AIR_NO_ROWS"] = "1"
        cmd, cwd = [binary, "boole_air_extract::extract", "--nocapture", "--test-threads", "1"], os.path.join(repo, "vm")
    else:
        cmd = [binary, res_dir] + ([repo] if zk == "openvm" else []) + ([] if rows else ["--no-rows"])
        cwd = repo
    t0 = time.time()
    rc, text = _run(cmd, env, cwd, RUN_TIMEOUT, os.path.join(out, "run.log"))
    rec = {"rc": rc, "secs": round(time.time() - t0, 1), "dir": res_dir, "seed": seed}
    if rc != 0 or not os.path.isfile(os.path.join(res_dir, "manifest.jsonl")):
        rec["error"] = text[-600:].replace(build_dir, "<build>")
    return rec


def load_machine(res_dir: str) -> tuple[list[dict], dict[int, str]]:
    """The extraction manifest and the IR file of every extracted AIR."""
    with open(os.path.join(res_dir, "manifest.jsonl"), encoding="utf-8") as f:
        rows = [json.loads(ln) for ln in f if ln.strip()]
    files = {r["index"]: os.path.join(res_dir, "airs", f"{r['index']}.json") for r in rows
             if r["status"] == "extracted"}
    return rows, files


def machine_record(rows: list[dict], files: dict[int, str]) -> list[list]:
    """[index, name, IR content sha256 or the failure] of every AIR (the problem's ``reference/machine.json``)."""
    out = []
    for r in rows:
        if r["index"] in files:
            out.append([r["index"], r["name"], IR.load(files[r["index"]]).content_sha256()])
        else:
            out.append([r["index"], r["name"], "failed: " + r.get("detail", "")[:200]])
    return out


# ------------------------------------------------------------------------------------------ candidate evaluation

def load_problem(problem_dir: str) -> dict:
    prob = read_json(os.path.join(problem_dir, "problem.json"))
    errs = P.validate_problem(prob, problem_dir)
    if errs or prob.get("schema_version") != SCHEMA_VERSION:
        raise RuntimeError(f"{problem_dir}: not a valid AIR ratchet problem: {errs[:5]}")
    return prob


def resolve_snapshot(problem_dir: str, prob: dict, snapshot: str | None) -> str:
    """The given snapshot, or ``snapshots/<id>`` next to an ancestor of the problem (``<wave>/problems/<zkvm>/<dir>``
    -> ``<wave>/snapshots/<id>``)."""
    if snapshot:
        return snapshot
    d = os.path.dirname(os.path.abspath(problem_dir))
    for _ in range(3):
        cand = os.path.join(d, "snapshots", prob["snapshot"]["id"])
        if os.path.isdir(cand):
            return cand
        d = os.path.dirname(d)
    raise RuntimeError(f"no snapshot {prob['snapshot']['id']} next to the problem; pass --snapshot")


def reference_air(prob: dict, problem_dir: str) -> IR.Air:
    path = os.path.join(problem_dir, prob["reference"]["ir"]["file"])
    if P.sha256_file(path) != prob["reference"]["ir"]["sha256"]:
        raise RuntimeError("reference IR differs from the problem record")
    return IR.load(path)


def bus_for(prob: dict, airs: list[IR.Air]) -> AB.BusModel:
    bus = AB.model_for(prob["reference"]["zkvm"])
    bus.observe(airs)
    return bus


def evaluate(prob: dict, problem_dir: str, res_dir: str) -> dict:
    """Admissibility of an extraction (machine, target, interface) and the candidate's cost; returns the target IR,
    its roles and interface, the bus model and the report (raises Reject)."""
    ref = prob["reference"]
    rows, files = load_machine(res_dir)
    mpath = os.path.join(problem_dir, ref["machine"]["file"])
    if P.sha256_file(mpath) != ref["machine"]["sha256"]:
        raise RuntimeError("reference machine record differs from the problem")
    want = {w[0]: w for w in read_json(mpath)}
    got = {g[0]: g for g in machine_record(rows, files)}
    if sorted(got) != sorted(want):
        raise Reject(f"the machine has {len(got)} AIRs; the reference build has {len(want)}")
    i = ref["air_index"]
    changed = [k for k in sorted(want) if k != i and want[k] != got[k]]
    if changed:
        names = ", ".join(f"{k} {want[k][1]}" for k in changed[:5])
        raise Reject(f"{len(changed)} other AIR(s) of the machine changed ({names}): only the target may change")
    if got[i][1] != ref["air_name"] or i not in files:
        raise Reject(f"AIR {i} is {got[i][1]!r} ({got[i][2][:60]}); the target is {ref['air_name']!r}")
    airs = [IR.load(files[k]) for k in sorted(files)]
    cand = next(a for a in airs if a.index == i)
    ref_air = reference_air(prob, problem_dir)
    for what, a, b in (("field", cand.field_name, ref_air.field_name), ("rust type", cand.rust_type, ref_air.rust_type),
                       ("group", cand.group, ref_air.group), ("preprocessed width", cand.prep_width,
                                                                ref_air.prep_width),
                       ("public value count", cand.n_public, ref_air.n_public)):
        if a != b:
            raise Reject(f"the target's {what} is {a}; the reference's is {b}")
    if cand.window_rows() != 1:
        raise Reject("the candidate references the next row: only one-row windows are ratchet candidates")
    if fixed_names(cand) != ref["layout"]["fixed"]:
        raise Reject("the window's fixed variables (preprocessed cells, public values, row selectors used) differ "
                     "from the reference's")
    if len(cand.constraints) > AD.MAX_CONSTRAINTS or len(cand.nodes) > AD.MAX_NODES:
        raise Reject("the candidate AIR is outside the registry size policy")
    bus = bus_for(prob, airs)
    roles, iface = candidate_roles(prob, ref_air, cand, bus)
    cnt = cost_of(cand, bus)
    with open(files[i], "rb") as f:
        ir_bytes = f.read()
    rep = {"ir_sha256": sha(ir_bytes), "content_sha256": cand.content_sha256(), "n_vars": IR.Layout.of(cand).n_vars,
           **score(prob["record"], cnt)}
    return {"air": cand, "roles": roles, "iface": iface, "bus": bus, "report": rep, "ir_path": files[i]}


def compile_candidate(prob: dict, problem_dir: str, cand_dir: str, out: str, snapshot: str, build_dir: str,
                      rows: bool = False) -> tuple[dict, dict]:
    """Source rules, build, extraction, admissibility and cost.  Returns (report, evaluation); raises Reject."""
    man = manifest_of(snapshot, prob["snapshot"]["manifest_sha256"])
    os.makedirs(out, exist_ok=True)
    overlay = overlay_of(prob, snapshot, cand_dir, man)
    rep: dict = {"problem": prob["package_id"], "candidate_dir": os.path.abspath(cand_dir), "started_utc": now(),
                 "overlay_sha256": overlay_digest(overlay), "files": sorted(overlay)}
    b = build(prob, snapshot, man, overlay, build_dir, os.path.join(out, "build"))
    rep["build"] = {k: v for k, v in b.items() if k != "binary"}
    if "error" in b:
        raise Reject(f"build failed: {b['error']}")
    x = extract(prob, build_dir, b["binary"], os.path.join(out, "extract"), rows)
    rep["extract"] = {k: v for k, v in x.items() if k != "dir"}
    if "error" in x:
        raise Reject(f"extraction failed: {x['error'][-400:]}")
    ev = evaluate(prob, problem_dir, x["dir"])
    ev["binary"], ev["res_dir"] = b["binary"], x["dir"]
    rep["candidate"] = {"overlay_sha256": rep["overlay_sha256"], "files": rep["files"], **ev["report"]}
    return rep, ev


# ------------------------------------------------------------------------------------------ statement

def namespaces(prob: dict) -> tuple[str, str, str]:
    return RT.namespaces(prob)


def candidate_model(prob: dict, ev: dict) -> tuple[str, dict]:
    _, _, cand_ns = namespaces(prob)
    ref = prob["reference"]
    air = ev["air"]
    meta = {"zkvm_name": ev["bus"].display, "release": ref["release"],
            "generator": f"{GENERATOR_NAME} v{GENERATOR_VERSION}", "repo_url": "(candidate overlay on " + ref["repo"] +
            ")", "extractor": prob["build"]["extractor"], "ir_sha256": ev["report"]["ir_sha256"]}
    text, summary = AL.emit_model(cand_ns, meta, air, IR.Layout.of(air), ev["roles"], ev["bus"].tables)
    return text.replace("# DET model: ", "# Ratchet candidate model: ", 1), summary


def statement_text(prob: dict, cand: dict) -> str:
    base, ref_ns, cand_ns = namespaces(prob)
    ref, rec = prob["reference"], prob["record"]
    rf = ref_ns + "."
    iface = ref["interface"]
    return "\n".join([
        f"import {AL.model_module(ref_ns)}",
        f"import {AL.model_module(cand_ns)}",
        "",
        f"namespace {base}",
        "",
        f"/-- Ratchet equivalence for {ref['repo']} {ref['release']} AIR `{ref['air_name']}` (position "
        f"{ref['air_index']}, at {ref['commit'][:12]}).",
        f"Reference: the registry DET model `{ref_ns}` of package `{ref['registry_package_id']}`",
        f"(IR sha256 `{ref['ir']['sha256']}`, one row window); its DET is machine-checked ({prob['det']['method']}).",
        f"Record: {cost_text(rec)}.",
        f"Candidate `Cand`: overlay sha256 `{cand['overlay_sha256']}`, extracted by the same harness",
        f"(IR sha256 `{cand['ir_sha256']}`): {cost_text(cand['cost'])}.",
        f"Fixed variables (same layout in both): {len(ref['layout']['fixed'])}; inputs: {len(iface['inputs'])} "
        f"message(s); outputs: {len(iface['outputs'])} message(s) (same order, buses and roles in both).",
        "For every list `f` of fixed values and all input and output message lists `x` and `y`: some window",
        "satisfies the reference's constraints and bus assumptions with fixed values `f`, input contributions `x`",
        "and output contributions `y` iff some window satisfies the candidate's. -/",
        f"theorem equiv [Fact (Nat.Prime {rf}p)] :",
        f"    ∀ (f : List {rf}F) (x y : List {rf}Msg),",
        f"      (∃ w : Fin {rf}nVars → {rf}F, {rf}Constraints w ∧ {rf}Assumptions w ∧ {rf}Fixed.map w = f ∧",
        f"        {rf}BusEq ({rf}In w) x ∧ {rf}BusEq ({rf}Out w) y) ↔",
        "      (∃ w : Fin Cand.nVars → Cand.F, Cand.Constraints w ∧ Cand.Assumptions w ∧ Cand.Fixed.map w = f ∧",
        "        Cand.BusEq (Cand.In w) x ∧ Cand.BusEq (Cand.Out w) y) := by",
        "  sorry",
        "",
        f"end {base}",
        "",
    ])


def statement_assumptions(prob: dict, cand_iface: dict, bus: AB.BusModel) -> list[str]:
    out = AD.statement_assumptions(prob["reference"]["ir"]["field"])
    tables = sorted({r["table"] for r in prob["reference"]["interface"]["assumptions"]} |
                    {r["table"] for r in cand_iface["assumptions"]})
    for t in tables:
        out.append(f"Assumptions of both models: lookups into `{t}` hold whenever their multiplicity is non-zero — "
                   f"{bus.table_doc(t)} The table is provided by another chip; each side's lookups are part of its own "
                   "window relation, and bus balance (LogUp) is not modelled.")
    out.append("Messages are compared by contribution (BusEq): equal multiplicities, equal values where the "
               "multiplicity is non-zero; inputs and outputs are matched by position (same interface in both).")
    return out


def checker_files(prob: dict, stmt: str, cmodel: str) -> list[dict]:
    _, ref_ns, cand_ns = namespaces(prob)
    m = prob["reference"]["model"]
    return [{"path": "Statement.lean", "role": "statement", "sha256": sha(stmt.encode())},
            {"path": m["file"], "role": "import", "module": AL.model_module(ref_ns), "sha256": m["sha256"]},
            {"path": AL.model_relpath(cand_ns), "role": "import", "module": AL.model_module(cand_ns),
             "sha256": sha(cmodel.encode())}]


def candidate_problem(prob: dict, cand: dict, files: list[dict], stmt: str, type_sha: str,
                      assumptions: list[str]) -> dict:
    from zk_registry import gates as G
    base, ref_ns, cand_ns = namespaces(prob)
    fqn = base + ".equiv"
    out = {k: v for k, v in prob.items() if k != "annotations"}
    out.update(status="CANDIDATE", candidate=cand, created_utc=now(),
               statement={"file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn, "text": stmt,
                          "imports": [AL.model_module(ref_ns), AL.model_module(cand_ns)], "assumptions": assumptions},
               checker={"statement_file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn,
                        "lean_opts": prob["reference"]["lean_opts"], "files": files,
                        "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                        "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS})
    return out


def write_candidate_package(prob: dict, problem_dir: str, cand: dict, ev: dict, pkg: str) -> tuple[str, list[dict]]:
    _, _, cand_ns = namespaces(prob)
    cmodel, _ = candidate_model(prob, ev)
    stmt = statement_text(prob, cand)
    fresh_dir(pkg)
    m = prob["reference"]["model"]
    os.makedirs(os.path.dirname(os.path.join(pkg, m["file"])), exist_ok=True)
    shutil.copyfile(os.path.join(problem_dir, m["file"]), os.path.join(pkg, m["file"]))
    if P.sha256_file(os.path.join(pkg, m["file"])) != m["sha256"]:
        raise RuntimeError("reference model file changed")
    put(os.path.join(pkg, AL.model_relpath(cand_ns)), cmodel)
    put(os.path.join(pkg, "Statement.lean"), stmt)
    return stmt, checker_files(prob, stmt, cmodel)


def package_candidate(prob: dict, problem_dir: str, rep: dict, ev: dict, out: str, env: L.LeanEnv | None) -> dict:
    """Statement and models of an evaluated candidate (``<out>/pkg``); with a Lean environment the statement's
    elaboration and the checker package."""
    cand = rep["candidate"]
    pkg = os.path.join(out, "pkg")
    stmt, files = write_candidate_package(prob, problem_dir, cand, ev, pkg)
    rep["statement_sha256"] = files[0]["sha256"]
    if env is not None:
        base, _, _ = namespaces(prob)
        el = RT.elaborate(env, pkg, [f["path"] for f in files[1:]], base + ".equiv", prob["reference"]["lean_opts"],
                          os.path.join(out, "elab"))
        rep["elab"] = el
        if el["status"] != "PASS":
            raise RuntimeError(f"statement does not elaborate: {el.get('errors')}")
        cprob = candidate_problem(prob, cand, files, stmt, el["reference_type_sha256"],
                                  statement_assumptions(prob, ev["iface"], ev["bus"]))
        errs = P.validate_problem(cprob, pkg)
        if errs:
            raise RuntimeError(f"checker package does not validate: {errs[:5]}")
        P.write_json(os.path.join(pkg, "problem.json"), cprob)
        rep["package"] = pkg
    return rep


def run_candidate(problem_dir: str, cand_dir: str, out: str, build_dir: str, snapshot: str | None = None,
                  env: L.LeanEnv | None = None, require_smaller: bool = False) -> dict:
    """The trusted candidate pipeline: source rules, build, extraction, admissibility, cost, statement and models,
    and with a Lean environment the statement's elaboration and the checker package (``<out>/pkg``)."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, ev = compile_candidate(prob, problem_dir, cand_dir, out, snapshot, build_dir)
    if require_smaller and not rep["candidate"]["smaller"]:
        P.write_json(os.path.join(out, "candidate.json"), rep)
        raise Reject(f"not smaller: {cost_text(rep['candidate']['cost'])}; record {cost_text(prob['record'])}")
    package_candidate(prob, problem_dir, rep, ev, out, env)
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "candidate.json"), rep)
    return rep


def run_check(problem_dir: str, cand_dir: str, solution: str, out: str, build_dir: str, env: L.LeanEnv,
              snapshot: str | None = None, timeout: float = DET_PROOF_LIMITS["timeout_s"]) -> tuple[str, dict]:
    """Final check: the candidate pipeline (component-wise smaller) and the production checker."""
    rep: dict = {"problem_dir": os.path.abspath(problem_dir), "solution_file": os.path.abspath(solution),
                 "started_utc": now()}
    try:
        cr = run_candidate(problem_dir, cand_dir, os.path.join(out, "candidate"), build_dir, snapshot, env,
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

def trace_docs(res_dir: str, index: int) -> list[dict]:
    d = os.path.join(res_dir, "rows-all", str(index))
    if not os.path.isdir(d):
        return []
    out = []
    for fn in sorted(os.listdir(d)):
        if fn.endswith(".json"):
            out.append(read_json(os.path.join(d, fn)))
    return out


def contributions(air: IR.Air, roles: AL.Roles, vals: list[int]) -> tuple[list, list]:
    """Active input and output contributions of one window: (message position, multiplicity, values) for every
    message whose multiplicity is non-zero."""
    def act(msgs):
        return [(k, vals[m.mult], tuple(vals[v] for v in m.values)) for k, m in enumerate(msgs) if vals[m.mult]]
    return act(roles.inputs), act(roles.outputs)


def trace_summary(air: IR.Air, roles: AL.Roles, bus: AB.BusModel, doc: dict, keep: int = 0) -> dict:
    """Every row of one dumped trace: constraint and assumption failures, the multisets of active input / output
    contributions, the preprocessed rows and public values the window reads, and up to ``keep`` windows."""
    layout = IR.Layout.of(air)
    ctx = AS.Context(air, layout, roles, bus.table_fn)
    windows = AD.real_windows(air, layout, doc)
    ins: collections.Counter = collections.Counter()
    outs: collections.Counter = collections.Counter()
    bad_c, bad_a, first_bad = 0, 0, None
    keep_w = []
    neg = set()
    half = air.p // 2
    for i, w in windows:
        vals = IR.eval_nodes(air, layout, w)
        fc = IR.failing_constraints(air, layout, w, vals)
        if fc:
            bad_c += 1
            first_bad = first_bad or {"row": i, "constraints": fc[:5]}
            continue
        if not AS.assumptions_hold(ctx, vals, w):
            bad_a += 1
            first_bad = first_bad or {"row": i, "assumptions": True}
            continue
        a, b = contributions(air, roles, vals)
        ins.update(a)
        outs.update(b)
        for k, it in enumerate(air.interactions):
            if vals[it.mult] > half:
                neg.add(k)
        if len(keep_w) < keep:
            keep_w.append(w)
    fixed = sorted({tuple(w[j] for j in layout.fixed()) for _, w in windows})
    return {"source": doc.get("source", ""), "height": doc.get("height", 0), "rows": len(windows),
            "constraint_failures": bad_c, "assumption_failures": bad_a, "first_failure": first_bad,
            "inputs": ins, "outputs": outs, "fixed": sha(json.dumps(fixed).encode()), "negative": sorted(neg),
            "windows": keep_w}


def multiset_digest(c: collections.Counter) -> str:
    items = sorted([list(k[:2]) + [list(k[2])], v] for k, v in c.items())
    return sha(json.dumps(items, separators=(",", ":")).encode())


def messages_record(summaries: list[dict]) -> dict:
    """The reference's ``reference/messages.json``: per trace its source, row count and the digests of its active
    input and output contributions and of its fixed values."""
    return {"format": "boole-ratchet-air-messages/v1",
            "traces": [{"source": s["source"], "height": s["height"], "rows": s["rows"],
                        "inputs": multiset_digest(s["inputs"]), "outputs": multiset_digest(s["outputs"]),
                        "n_inputs": sum(s["inputs"].values()), "n_outputs": sum(s["outputs"].values()),
                        "fixed": s["fixed"]} for s in summaries]}


def _first_diff(a: collections.Counter, b: collections.Counter) -> str:
    only_a = sorted((a - b).items())[:1]
    only_b = sorted((b - a).items())[:1]
    def fmt(x):
        if not x:
            return "nothing"
        (k, m, v), n = x[0]
        return f"message {k} (multiplicity {m}, values {list(v)[:8]}{' ...' if len(v) > 8 else ''}) x{n}"
    return f"reference has {fmt(only_a)} the candidate lacks; candidate has {fmt(only_b)} the reference lacks"


def compare_runs(ref_s: list[dict], cand_s: list[dict], label: str) -> tuple[collections.Counter, list[dict]]:
    """Trace by trace (matched by source): candidate rows must satisfy the candidate's constraints and assumptions,
    read the reference's fixed values and make the same multisets of active input and output contributions."""
    kinds: collections.Counter = collections.Counter()
    ex: list[dict] = []
    ref_by = {s["source"]: s for s in ref_s}
    cand_by = {s["source"]: s for s in cand_s}

    def note(kind: str, src: str, detail: str):
        kinds[kind] += 1
        if len(ex) < 20:
            ex.append({"run": label, "trace": src, "kind": kind, "detail": detail})
    for src in sorted(set(ref_by) | set(cand_by)):
        a, b = ref_by.get(src), cand_by.get(src)
        if a is None or b is None:
            note("trace missing", src, "only the " + ("candidate" if a is None else "reference") + " run has it")
            continue
        if a["constraint_failures"] or a["assumption_failures"]:
            note("reference row fails the reference model", src, json.dumps(a["first_failure"]))
        if b["constraint_failures"]:
            note("candidate row violates the candidate constraints", src, json.dumps(b["first_failure"]))
            continue
        if b["assumption_failures"]:
            note("candidate row violates a table lookup or selector assumption", src, json.dumps(b["first_failure"]))
            continue
        if a["fixed"] != b["fixed"]:
            note("different fixed values (preprocessed rows or public values)", src, "")
        if a["inputs"] != b["inputs"]:
            note("different input contributions", src, _first_diff(a["inputs"], b["inputs"]))
        if a["outputs"] != b["outputs"]:
            note("different output contributions", src, _first_diff(a["outputs"], b["outputs"]))
    return kinds, ex


def check_reference_messages(prob: dict, problem_dir: str, ref_s: list[dict]) -> None:
    """The reference build must reproduce the problem's recorded reference contributions (standard programs)."""
    rec = prob["reference"]["messages"]
    path = os.path.join(problem_dir, rec["file"])
    if P.sha256_file(path) != rec["sha256"]:
        raise RuntimeError("reference messages differ from the problem record")
    if read_json(path) != messages_record(ref_s):
        raise RuntimeError("the reference build does not reproduce the recorded reference messages (wrong build tree "
                           "or toolchain)")


def mech_problem(air: IR.Air, roles: AL.Roles, ns: str) -> M.Problem:
    """A battery P3 problem over an in-memory AIR model (as ``mech_air.load_air`` builds it from a package)."""
    layout = IR.Layout.of(air)
    pr = AL.Printer(air, layout, AL.roots_of(air, roles))
    var = [layout.var(n) if n[0] in IR.LEAF_OPS and n[0] != "const" else -1 for n in air.nodes]
    n_sel = 0
    if roles.selectors:
        n_sel = len(layout.selectors) + (1 if "trans" in layout.selectors and "last" in layout.selectors else 0)
    n_asm = len(roles.assumptions) + n_sel
    return M.Problem("air", "", "candidate", ns, air.p, air.nodes, var, layout.n_vars, "nVars", list(air.constraints),
                     len(air.constraints), AL.block_names(len(air.constraints)),
                     [M.Msg(i, m.mult, list(m.values)) for i, m in enumerate(roles.inputs)],
                     [M.Msg(i, m.mult, list(m.values)) for i, m in enumerate(roles.outputs)],
                     layout.fixed(), [], [], pr, "",
                     [M.Assume(i, a.table, a.mult, list(a.values)) for i, a in enumerate(roles.assumptions)],
                     n_asm, AL.asm_block_names(n_asm), [],
                     {"air_name": air.name, "zkvm": air.zkvm, "n_constraints": len(air.constraints),
                      "n_nodes": len(air.nodes), "window_rows": air.window_rows(), "p_literal": air.p})


def det_screen(air: IR.Air, roles: AL.Roles) -> dict:
    """Informational: battery P3 propagation on the candidate (DETERMINED, or STUCK)."""
    try:
        return {"propagation": M.solve(mech_problem(air, roles, "Cand")).status}
    except Exception as ex:                                 # noqa: BLE001 - informational only
        return {"propagation": "ERROR", "error": str(ex)[:200]}


def det_search(air: IR.Air, roles: AL.Roles, bus: AB.BusModel, windows: list[list[int]], seed: str,
               budget_s: float) -> dict:
    """The registry's sound DET counterexample search on the candidate model from its real windows: a confirmed
    counterexample refutes the equivalence (the reference's DET is machine-checked)."""
    if not roles.outputs or not windows:
        return {"status": "skipped", "reason": "no outputs" if not roles.outputs else "no real window"}
    ctx = AS.Context(air, IR.Layout.of(air), roles, bus.table_fn)
    ce, log = AS.search(ctx, windows[:SEARCH_POOL], seed, budget_s)
    if ce is None:
        return {"status": "no-counterexample", "log": log}
    return {"status": "counterexample", "method": ce.method, "changed_outputs": ce.changed_outputs,
            "base": ce.base, "other": ce.other}


def run_pair(prob: dict, problem_dir: str, ev: dict, ref_air: IR.Air, ref_roles: AL.Roles, ref_bus: AB.BusModel,
             ref_build: str, ref_bin: str, cand_build: str, seed: int, out: str, cache: str | None) -> dict:
    """Reference and candidate harness runs on the programs of one seed, summarized per trace."""
    i = prob["reference"]["air_index"]
    ref_s = None
    cpath = os.path.join(cache, f"ref-seed{seed}.json") if cache else None
    key = sha((prob["package_id"] + "|" + P.sha256_file(ref_bin) + f"|{seed}").encode())
    if cpath and os.path.isfile(cpath):
        c = read_json(cpath)
        if c.get("key") == key:
            ref_s = [_load_summary(s) for s in c["summaries"]]
    if ref_s is None:
        x = extract(prob, ref_build, ref_bin, os.path.join(out, f"ref-seed{seed}"), True, seed)
        if "error" in x:
            raise RuntimeError(f"reference harness run failed (seed {seed}): {x['error'][-300:]}")
        ref_s = [trace_summary(ref_air, ref_roles, ref_bus, d) for d in trace_docs(x["dir"], i)]
        shutil.rmtree(x["dir"], ignore_errors=True)
        if cpath:
            put(cpath, json.dumps({"key": key, "summaries": [_dump_summary(s) for s in ref_s]}))
    x = extract(prob, cand_build, ev["binary"], os.path.join(out, f"cand-seed{seed}"), True, seed)
    if "error" in x:
        return {"seed": seed, "error": x["error"][-400:], "ref": ref_s, "cand": []}
    cand_s = [trace_summary(ev["air"], ev["roles"], ev["bus"], d, keep=8) for d in trace_docs(x["dir"], i)]
    shutil.rmtree(x["dir"], ignore_errors=True)
    return {"seed": seed, "ref": ref_s, "cand": cand_s}


def _dump_summary(s: dict) -> dict:
    def ms(c):
        return [[k[0], k[1], list(k[2]), v] for k, v in sorted(c.items())]
    return dict({k: v for k, v in s.items() if k != "windows"}, inputs=ms(s["inputs"]), outputs=ms(s["outputs"]))


def _load_summary(s: dict) -> dict:
    def ms(x):
        return collections.Counter({(k, m, tuple(v)): n for k, m, v, n in x})
    return dict(s, inputs=ms(s["inputs"]), outputs=ms(s["outputs"]), windows=[])


def simulate(problem_dir: str, cand_dir: str, out: str, build_dir: str, ref_build: str, seeds: int,
             snapshot: str | None = None, cache: str | None = None) -> dict:
    """The simulation screen (not a proof) on a candidate: the standard sample programs (seed 0) and ``seeds`` random
    executions (seeds 1..N), reference and candidate builds of the same harness."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, ev = compile_candidate(prob, problem_dir, cand_dir, os.path.join(out, "count"), snapshot, build_dir)
    man = manifest_of(snapshot, prob["snapshot"]["manifest_sha256"])
    rb = build(prob, snapshot, man, {}, ref_build, os.path.join(out, "ref-build"))
    if "error" in rb:
        raise RuntimeError(f"reference build failed: {rb['error']}")
    ref_air = reference_air(prob, problem_dir)
    ref_bus = ev["bus"]
    ref_roles = M.air_roles(ref_air, prob["reference"]["interface"])
    t0 = time.time()
    kinds: collections.Counter = collections.Counter()
    examples: list[dict] = []
    runs = []
    windows: list[list[int]] = []
    neg: set[int] = set()
    rows = traces = 0
    for s in range(0, seeds + 1):
        r = run_pair(prob, problem_dir, ev, ref_air, ref_roles, ref_bus, ref_build, rb["binary"], build_dir, s, out,
                     cache)
        if s == 0:
            check_reference_messages(prob, problem_dir, r["ref"])
        if "error" in r:
            kinds["candidate harness run failed"] += 1
            examples.append({"run": f"seed {s}", "kind": "candidate harness run failed", "detail": r["error"]})
            runs.append({"seed": s, "error": r["error"][-300:]})
            continue
        k, ex = compare_runs(r["ref"], r["cand"], f"seed {s}")
        kinds.update(k)
        examples += ex[:max(0, 20 - len(examples))]
        for c in r["cand"]:
            windows += c["windows"]
            neg |= set(c["negative"])
            rows += c["rows"]
            traces += 1
        runs.append({"seed": s, "traces": len(r["cand"]), "rows": sum(c["rows"] for c in r["cand"]),
                     "reference_traces": len(r["ref"])})
    if not traces:
        kinds["no candidate trace on the sample programs"] += 1
    extra_neg = sorted(neg - set(ev["iface"]["negative"]))
    if extra_neg:
        _, seen = ev["bus"].roles(ev["air"], frozenset(neg | set(ev["iface"]["negative"])))
        if interface_view(seen) != interface_view(ev["iface"]):
            kinds["the bus model gives other roles on the candidate's real rows"] += 1
            examples.append({"run": "all", "kind": "roles", "detail": f"interactions {extra_neg} take negated "
                             "multiplicities on real rows (a receive sent as a send) where the reference's do not"})
    budget = prob["simulate"]["search_budget_s"]
    search = det_search(ev["air"], ev["roles"], ev["bus"], windows, prob["simulate"]["seed"], budget)
    if search["status"] == "counterexample":
        kinds["candidate DET counterexample (refutes the equivalence)"] += 1
    sim = {"seeds": seeds, "runs": runs, "traces": traces, "rows": rows, "mismatches": sum(kinds.values()),
           "mismatch_kinds": dict(kinds), "examples": examples, "det_search": search,
           "det_screen": det_screen(ev["air"], ev["roles"]), "secs": round(time.time() - t0, 1),
           "verdict": "PASS" if not kinds else "MISMATCH"}
    rep["simulate"] = sim
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "simulate.json"), rep)
    return rep


# ------------------------------------------------------------------------------------------ problem build

def reference_spec(reg_dir: str) -> dict:
    """The reference fields of a registry AIR DET package (``problem.json``)."""
    prob = read_json(os.path.join(reg_dir, "problem.json"))
    ids, air, st, chk = prob["ids"], prob["air"], prob["statement"], prob["checker"]
    return {"registry_package_id": prob["package_id"], "property": prob["property"]["template"],
            "status": prob["status"], "zkvm": ids["zkvm"], "release": ids["release"], "repo": ids["repo"],
            "repo_url": ids["repo_url"], "commit": ids["commit"], "air_name": ids["air_name"],
            "air_index": ids["air_index"], "rust_type": ids["rust_type"], "group": ids.get("group", ""),
            "window_rows": air["window_rows"], "within": air["size_policy"]["within"], "interface": prob["interface"],
            "extractor": air["extractor"], "ir_sha256": air["ir_sha256"], "content_sha256": air["content_sha256"],
            "model": {"file": st["model_file"], "module": st["model_module"],
                      "namespace": st["model_module"][:-len(".Model")],
                      "sha256": next(f["sha256"] for f in chk["files"] if f["path"] == st["model_file"])},
            "det_theorem_fqn": st["theorem_fqn"], "lean_opts": chk["lean_opts"], "env": prob["env"],
            "coverage": prob.get("coverage"), "gates": prob.get("gates") or {}}


def static_exclusion(spec: dict) -> tuple[str, str] | None:
    if spec["property"] != "DET":
        return "property", f"{spec['property']} package"
    if spec["window_rows"] != 1:
        return "two-row", ("two-row window: the window relation does not compose to the trace (a ratchet reference "
                           "must be a one-row AIR)")
    if not spec["within"]:
        return "size", "outside the registry size policy"
    if (spec["gates"].get("G-FID") or {}).get("status") != "PASS":
        return "no-rows", ("no real rows (G-FID did not pass: the sample programs do not exercise the AIR), so the "
                           "simulation screen cannot run")
    return None


def battery_det_solution(statement: str, summary: dict, form: str) -> str:
    """A ``det`` proof from a registry battery form (``triv_<variant>_<tactic>``) that closed the AIR statement."""
    m = re.fullmatch(r"triv_(V[0-4])_([A-Za-z_]+)", form)
    if not m or m.group(2) in ("bv_decide", "exactQ", "applyQ"):
        raise ValueError(f"no standard-axiom proof for battery form {form!r}")
    variant, tactic = m.groups()
    lines = AL.battery_prefix(variant, summary) + [f"all_goals {tactic}" if variant == "V4" else tactic]
    if variant in ("V3", "V4"):
        body = ["  set_option maxRecDepth 100000 in"] + [f"    {x}" for x in lines]
    else:
        body = [f"  {x}" for x in lines]
    if statement.count("\n  sorry\n") != 1:
        raise ValueError("statement has no single `sorry` line")
    return statement.replace("\n  sorry\n", "\n" + "\n".join(body) + "\n", 1)


def model_summary(reg_dir: str) -> dict:
    """The model summary (hoisted definitions, blocks, tables) the battery prefix unfolds, regenerated from the IR."""
    spec = reference_spec(reg_dir)
    air = IR.load(os.path.join(reg_dir, "evidence", "air.ir.json"))
    roles = M.air_roles(air, spec["interface"])
    bus = AB.model_for(spec["zkvm"])
    _, summary = AL.emit_model(spec["model"]["namespace"], {"zkvm_name": "", "release": "", "generator": "",
                                                            "repo_url": "", "extractor": "", "ir_sha256": ""},
                               air, IR.Layout.of(air), roles, bus.tables)
    return summary


def snapshot_rule() -> str:
    return ("the zkVM repository's tracked files at the census pin (git archive of the pinned commit) with the "
            "registry AIR extractor installed as in wave Z0 (air_det.prepare_harness: a workspace member or a "
            "#[cfg(test)] module, and the Cargo.lock entry it adds), under repo/")


def problem_record(spec: dict, ref_air: IR.Air, ir_sha: str, machine: dict, messages: dict, negative: list[int],
                   det: dict, snapshot: dict, build_rec: dict, bus: AB.BusModel,
                   annotations: dict | None = None) -> dict:
    pid = "ratchet/" + spec["registry_package_id"]
    cost = cost_of(ref_air, bus)
    env = {k: spec["env"][k] for k in ("lean", "mathlib", "lake_manifest_sha256", "packages") if k in spec["env"]}
    lay = IR.Layout.of(ref_air)
    rec = {
        "schema_version": SCHEMA_VERSION, "kind": "ratchet", "family": "air", "package_id": pid, "status": "OPEN",
        "reference": {
            "registry_package_id": spec["registry_package_id"], "zkvm": spec["zkvm"], "release": spec["release"],
            "repo": spec["repo"], "repo_url": spec["repo_url"], "commit": spec["commit"],
            "air_name": spec["air_name"], "air_index": spec["air_index"], "rust_type": spec["rust_type"],
            "group": spec["group"],
            "ir": {"file": "reference/air.ir.json", "sha256": ir_sha, "content_sha256": ref_air.content_sha256(),
                   "field": ref_air.field_name, "p": ref_air.p},
            "machine": machine, "interface": spec["interface"], "negative": negative,
            "layout": {"n_vars": lay.n_vars, "window_rows": 1, "width": ref_air.width,
                       "prep_width": ref_air.prep_width, "n_public": ref_air.n_public, "fixed": fixed_names(ref_air)},
            "messages": messages, "model": spec["model"], "det_theorem_fqn": spec["det_theorem_fqn"],
            "lean_opts": spec["lean_opts"]},
        "record": dict(cost, rung=0, source="reference (registry IR, wave Z0 extraction)"),
        "metric": METRIC, "admissibility": list(ADMISSIBILITY), "statement_shape": STATEMENT_DESCRIPTION,
        "determinism": DETERMINISM_NOTE, "det": det,
        "simulate": {"programs": "the registry's sample programs at seed 0 (straight-line programs, in-tree ELFs, "
                                 "linear recursion programs; every row of every dumped trace)",
                     "random_seeds": "seeds 1..N: the generated programs with other operand values (ELFs skipped)",
                     "max_traces": screen_limits(ref_air)[0], "max_height": screen_limits(ref_air)[1],
                     "search_budget_s": SEARCH_BUDGET_S,
                     "seed": f"{SIM_SEED}|{pid}"},
        "snapshot": snapshot, "build": build_rec, "env": env, "generator": generator_info(), "created_utc": now(),
    }
    if annotations:
        rec["annotations"] = annotations
    return rec


def build_problem(reg_dir: str, det: dict | None, extraction: str, snapshot: str, snapshot_id: str,
                  build_rec: dict, out_dir: str, annotations: dict | None = None) -> dict:
    """Reference package -> AIR ratchet problem package: ``problem.json``, the registry model file and IR byte for
    byte, the machine record and the reference's bus contributions on the standard sample programs.  ``extraction``
    is a ratchet-mode harness run of the snapshot at seed 0 with the target among ``BOOLE_RATCHET_AIRS``; it must
    reproduce the registry IR.  Raises NotEligible."""
    spec = reference_spec(reg_dir)
    why = static_exclusion(spec) or RT.det_exclusion(det, spec["model"]["sha256"])
    if why:
        raise NotEligible(*why)
    try:
        mp = M.load(reg_dir)                                    # model <-> IR <-> interface provenance
    except M.MechError as ex:
        raise NotEligible("provenance", str(ex)) from None
    del mp
    ir_path = os.path.join(reg_dir, "evidence", "air.ir.json")
    ref_air = IR.load(ir_path)
    rows, files = load_machine(extraction)
    i = spec["air_index"]
    if i not in files or IR.load(files[i]).content_sha256() != ref_air.content_sha256() or \
            P.sha256_file(files[i]) != P.sha256_file(ir_path):
        raise NotEligible("rebuild", "the snapshot's extraction does not reproduce the registry IR")
    airs = [IR.load(files[k]) for k in sorted(files)]
    bus = AB.model_for(spec["zkvm"])
    bus.observe(airs)
    negative = reference_negative(ref_air, spec["interface"], bus)
    roles = M.air_roles(ref_air, spec["interface"])
    summaries = [trace_summary(ref_air, roles, bus, d) for d in trace_docs(extraction, i)]
    if not summaries or not sum(s["rows"] for s in summaries):
        raise NotEligible("screen-limits", "the ratchet-mode run dumps no trace of the AIR within the screen's trace "
                                           f"limits ({screen_limits(ref_air)[1]} rows)")
    bad = [s for s in summaries if s["constraint_failures"] or s["assumption_failures"]]
    if bad:
        raise NotEligible("rows", f"{len(bad)} reference trace(s) fail the reference model ({bad[0]['source']})")
    machine_rows = machine_record(rows, files)
    msgs = messages_record(summaries)
    fresh_dir(out_dir)
    dst = os.path.join(out_dir, spec["model"]["file"])
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(os.path.join(reg_dir, spec["model"]["file"]), dst)
    os.makedirs(os.path.join(out_dir, "reference"))
    shutil.copyfile(ir_path, os.path.join(out_dir, "reference", "air.ir.json"))
    P.write_json(os.path.join(out_dir, "reference", "machine.json"), machine_rows)
    P.write_json(os.path.join(out_dir, "reference", "messages.json"), msgs)
    machine = {"file": "reference/machine.json", "sha256": P.sha256_file(os.path.join(out_dir, "reference",
                                                                                         "machine.json")),
               "n_airs": len(machine_rows)}
    messages = {"file": "reference/messages.json",
                "sha256": P.sha256_file(os.path.join(out_dir, "reference", "messages.json")),
                "traces": len(summaries), "rows": sum(s["rows"] for s in summaries)}
    man = RT.verify_snapshot(snapshot)
    snap = {"id": snapshot_id, "manifest_sha256": P.sha256_file(os.path.join(snapshot, "MANIFEST.sha256")),
            "n_files": len(man), "rule": snapshot_rule()}
    prob = problem_record(spec, ref_air, P.sha256_file(ir_path), machine, messages, negative, det, snap, build_rec,
                          bus, annotations)
    errs = P.validate_problem(prob, out_dir)
    if errs:
        raise RuntimeError(f"ratchet problem does not validate: {errs[:5]}")
    P.write_json(os.path.join(out_dir, "problem.json"), prob)
    return prob


def build_record(zkvm: str, toolchain: str, pinned: str, rustc: str, harness_sha256: str, extractor: str) -> dict:
    installed = toolchain.split("-aarch64")[0].split("-x86_64")[0]
    deviation = "" if installed == pinned else (f"repository pins {pinned}, which is not installed; the installed "
                                                f"{installed} ({rustc}) is used, as in wave Z0")
    z = ZKVMS[zkvm]
    return {"toolchain": toolchain, "pinned_toolchain": pinned, "toolchain_deviation": deviation, "rustc": rustc,
            "features": z["features"], "command": list(z["command"]), "run": z["run"],
            "harness_sha256": harness_sha256, "extractor": extractor, "editable": list(z["editable"]),
            "protected": list(z["protected"])}


# ------------------------------------------------------------------------------------------ CLI

def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for nm in ("count", "simulate", "statement", "check"):
        x = sub.add_parser(nm)
        x.add_argument("--problem", required=True)
        x.add_argument("--candidate", required=True)
        x.add_argument("--out", required=True)
        x.add_argument("--build", required=True)
        x.add_argument("--snapshot")
        if nm == "simulate":
            x.add_argument("--ref-build", required=True)
            x.add_argument("--seeds", type=int, default=4)
            x.add_argument("--cache")
        if nm in ("statement", "check"):
            x.add_argument("--lean-env", required=True)
            x.add_argument("--rss-mb", type=int, default=DET_PROOF_LIMITS["rss_mb"])
        if nm == "check":
            x.add_argument("--solution", required=True)
    a = ap.parse_args(argv)
    try:
        return _run_cli(a)
    except (RuntimeError, ValueError, OSError) as ex:
        print(f"ERROR {ex}")
        return EXIT["ERROR"]


def summary_line(c: dict) -> str:
    worse = (" larger: " + ", ".join(c["larger_components"])) if c["larger_components"] else ""
    k = c["cost"]
    return (f"{'SMALLER' if c['smaller'] else 'NOT-SMALLER'} main_columns={k['main_columns']} "
            f"interactions={k['interactions']} constraints={k['constraints']} "
            f"deg_ge={json.dumps(k['constraints_deg_ge'], sort_keys=True)} interaction_degree="
            f"{k['interaction_degree']} priced={k['priced']} (reduction {c['reduction_pct']}%){worse}")


def _run_cli(a) -> int:
    env = None
    if a.cmd in ("statement", "check"):
        L.set_lean_limits(slots=1, rss_mb=a.rss_mb)
        env = L.load_env(a.lean_env)
    if a.cmd == "check":
        verdict, rep = run_check(a.problem, a.candidate, a.solution, a.out, a.build, env, a.snapshot)
        rep["finished_utc"] = now()
        P.write_json(os.path.join(a.out, "verdict.json"), rep)
        print(f"{verdict} {rep.get('reason', '')}".rstrip())
        for k in ("error", "invalid", "fail"):
            for x in (rep.get("checker") or {}).get(k, []):
                print(f"  {k}: {x}")
        return EXIT[verdict]
    try:
        if a.cmd == "simulate":
            rep = simulate(a.problem, a.candidate, a.out, a.build, a.ref_build, a.seeds, a.snapshot, a.cache)
        elif a.cmd == "count":
            prob = load_problem(a.problem)
            snap = resolve_snapshot(a.problem, prob, a.snapshot)
            rep, _ = compile_candidate(prob, a.problem, a.candidate, a.out, snap, a.build)
            P.write_json(os.path.join(a.out, "candidate.json"), rep)
        else:
            rep = run_candidate(a.problem, a.candidate, a.out, a.build, a.snapshot, env)
    except Reject as ex:
        print(f"REJECTED {ex}")
        return 4
    c = rep["candidate"]
    print(summary_line(c))
    if a.cmd == "simulate":
        sm = rep["simulate"]
        print(f"SIMULATE {sm['verdict']} traces={sm['traces']} rows={sm['rows']} mismatches={sm['mismatches']} "
              f"det-search={sm['det_search']['status']} det-screen={sm['det_screen'].get('propagation')}")
        for m in sm["examples"][:3]:
            print(f"  {m['kind']} ({m['run']}, {m.get('trace', '')[:60]}): {m.get('detail', '')[:200]}")
        if sm["verdict"] != "PASS":
            return 5
    return 0 if c["smaller"] else 4


if __name__ == "__main__":
    sys.exit(main())
