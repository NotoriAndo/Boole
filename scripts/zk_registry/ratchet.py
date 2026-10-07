#!/usr/bin/env python3
"""Ratchet problems (verified circuit optimization) for Circom references of the registry.

A ratchet problem is a reference circuit plus its current **record**.  A submission is a smaller circuit (the
candidate) plus a Lean proof that it is equivalent to the reference; an accepted submission becomes the next record.

* **Reference meaning**: the registry package's DET model of the reference (its ``--O0`` constraint system, the
  model file byte for byte).  The reference's own output determinism must be established by a machine-checked
  proof (the battery P3 mechanical proof, or a battery closure re-checked by the production checker), recorded in
  the problem; by DET of the reference and the equivalence, every accepted candidate is deterministic too.
* **Metric**: the number of **non-linear** constraints of the canonicalized ``--O2`` compile with every main input
  public: constraints ``A * B = C`` whose A and B both contain a non-constant wire.  Linear constraints are free (they
  are substituted away when the template is embedded in a larger circuit), so compressing linear constraints into
  quadratic ones never counts.  Totals are reported alongside.  Compilers: the registry's pinned release binaries
  (v2.1.9, v2.2.3) and its pinned v2.0.9 source build use ``--O2``; circom 1 (the pinned npm package 0.5.46) has no
  ``--O2`` and uses its only optimization, the full linear constraint reduction (no ``-f``).  Reference and
  candidates always use exactly the same compiler and flags, at the most simplifying level the compiler offers:
  the non-linear count can still depend on the level (substituting linear constraints can turn a product with a
  factor that becomes constant into a linear constraint; wave RT-C2 measured --O1 above --O2 on 18% of 664
  references, by a median of 3 constraints, never below), so records are compared only within one toolchain.
* **Admissibility** of a candidate: it declares a template with the reference's name and arity (the harness writes
  ``component main {public [<inputs>]} = <reference call>``), it has no ``component main`` and no custom templates,
  it includes only files next to it or files of the problem's pinned repository snapshot (the repository at the
  pinned commit with its recorded dependencies, circomlib included), it is compiled by the reference's pinned
  compiler with the same flags, and its main input / output signals (names, order, prime) equal the reference's.
  Every ``--O2`` R1CS is canonicalized (terms by wire, constraints sorted; header, wire numbering and labels
  unchanged) before it is counted, digested or modelled, because circom writes it in a run-dependent order.
* **Statement** (generated per candidate): for every input vector ``x`` and output vector ``y``, some assignment
  satisfies the reference constraints with inputs ``x`` and outputs ``y`` iff some assignment satisfies the candidate
  constraints with inputs ``x`` and outputs ``y`` (both directions of the input-output relation).
* **Screen** (``simulate``): the circuits' own wasm witness generators on the same structured and seeded random
  inputs; the candidate must reject what the reference rejects and give the same outputs elsewhere.  It cannot see
  an under-constrained hint that the generator fills correctly; only the proof decides.
* **Final check**: the candidate pipeline (admissibility, compile, metric: strictly fewer non-linear constraints than
  the record) and then the unchanged production checker (``check.py``) on the generated package.

Answers (candidates and proofs) are never stored in the repository.

Usage::

    python3 -m zk_registry.ratchet build    --registry-package DIR --snapshot DIR --det DET.json --out DIR
                                            --compilers C.json [--node NODE] [--work DIR]
    python3 -m zk_registry.ratchet count    --problem DIR --candidate F --out DIR --compilers C.json [--snapshot DIR]
    python3 -m zk_registry.ratchet simulate --problem DIR --candidate F --out DIR --compilers C.json [--n 2000]
    python3 -m zk_registry.ratchet statement --problem DIR --candidate F --out DIR --compilers C.json --lean-env E.json
    python3 -m zk_registry.ratchet check    --problem DIR --candidate F --solution S --out DIR --compilers C.json
                                            --lean-env E.json

``C.json`` maps compiler tags (``v2.2.3``, ``v2.1.9``, ``v2.0.9``; ``v0.5.46``: the circom 1 npm install root) to
pinned compilers (digest-checked on every use); circom 1 runs under ``--node``.  Exit codes:
``count`` / ``simulate`` / ``statement``: 0 smaller (``simulate``: and no mismatch), 4 rejected or not smaller,
5 simulation mismatch, 3 error; ``check``: 0 PASS, 1 FAIL, 2 INVALID, 3 ERROR, 4 REJECTED.
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
import subprocess
import sys
import tempfile
import time

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C              # noqa: E402
from zk_registry import circom_det as CD        # noqa: E402
from zk_registry import circom_source as cs     # noqa: E402
from zk_registry import gates as G              # noqa: E402
from zk_registry import instantiation as I      # noqa: E402
from zk_registry import lean_emit as E          # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import r1cs as R               # noqa: E402

GENERATOR_NAME = "boole-zk-registry-ratchet"
GENERATOR_VERSION = "1"
GENERATOR_SOURCES = ["ratchet.py", "ratchet_sim.js", "schema/ratchet_problem.schema.json", "r1cs.py", "lean_emit.py",
                     "instantiation.py", "circom_source.py", "package.py", "check.py", "subinstances.py"]
HERE = os.path.dirname(os.path.abspath(__file__))
SIM_RUNNER = os.path.join(HERE, "ratchet_sim.js")

FLAGS_MODEL = ["--r1cs", "--sym", "--O0"]       # the reference meaning (R1CS byte-identical to the registry model's)
FLAGS_RECORD = ["--r1cs", "--sym", "--O2"]      # the cost compile of the record and of every candidate
FLAGS_WASM = ["--wasm"]                         # added for the simulation compiles (R1CS must stay byte-identical)
# circom 1 (npm circom 0.5.46): ``-f`` = no constraint reduction (the --O0 model); without ``-f`` circom 1 runs its
# only optimization, a full linear reduction of the constraints over internal signals (main inputs and outputs, public
# or private, are never removed), which is the record and candidate compile
CIRCOM1_FLAGS_MODEL = ["-f", "-r", "main.r1cs", "-s", "main.sym"]
CIRCOM1_FLAGS_RECORD = ["-r", "main.r1cs", "-s", "main.sym"]
CIRCOM1_FLAGS_WASM = ["-w", "main.wasm"]
CIRCOM1_PRIMES = {"bls12381": "BLS12381"}       # registry prime name -> circom 1 ``-p`` value (default bn128)
RELEASE_COMPILERS = ("v2.1.9", "v2.2.3")        # pinned release binaries with --O2
SOURCE_BUILD_COMPILERS = ("v2.0.9",)            # the registry's pinned source build of the release tag, with --O2
CIRCOM1_COMPILERS = ("v0.5.46",)                # circom 1, the registry's pinned npm package (full reduction)
PINNED_COMPILERS = RELEASE_COMPILERS + SOURCE_BUILD_COMPILERS + CIRCOM1_COMPILERS
SIZE_LIMIT = P.R1_MAX_CONSTRAINTS               # reference --O0 size: the recovery-R1 circom size policy
DET_PROOF_LIMITS = {"rss_mb": 20000, "timeout_s": 1800}   # Lean guard per proof check (ratchet pilot P2)
DET_METHODS = ("mech-p3", "battery-closure", "det-problem")   # det-problem: an accepted proof of an issued DET problem
COMPILE_TIMEOUT, COMPILE_RSS_MB = 600, 16000
SIM_SEED = "boole-ratchet-sim"
LADDER = [1, 2, 4, 8, 16, 32, 64, 128, 160, 248, 252, 253]   # bit widths of the edge cases and random draws
SIM_HARNESS_ERRORS = ("Signal ", "Not enough values", "Too many values", "Not all inputs have been set")
EXIT = {"PASS": 0, "FAIL": 1, "INVALID": 2, "ERROR": 3, "REJECTED": 4}

METRIC = ("non-linear constraints of the canonicalized --O2 compile with every main input public: constraints "
          "A * B = C whose A and B both contain a non-constant wire (wire != 0 with a non-zero coefficient); linear "
          "constraints are free because they are substituted away when the template is embedded; a candidate counts "
          "only if its non-linear count is strictly below the record's; totals are reported")
ADMISSIBILITY = [
    "the candidate declares `template <reference template>` with the reference's number of parameters; the harness "
    "writes `component main {public [<reference inputs>]} = <reference call>;`",
    "no `component main` and no `pragma custom_templates` in the candidate files; `pragma circom` at most the "
    "pinned compiler's version",
    "includes: bare `<name>.circom` files next to the candidate, or files of the pinned repository snapshot "
    "(`repo/<path>`, or a path that the reference's library paths resolve to a snapshot file); nothing else",
    "compiled by the reference's pinned compiler (digest-checked) with the record's flags and prime",
    "main inputs and outputs equal the reference's (prime, names, order, counts)",
    "every --O2 R1CS is canonicalized before it is counted, digested or modelled",
]
METRIC_CIRCOM1 = ("non-linear constraints of the canonicalized circom 1 compile with its full constraint reduction (no "
                  "-f; circom 1's only optimization level, which eliminates internal signals through linear "
                  "constraints and keeps every main input and output): constraints A * B = C whose A and B both "
                  "contain a non-constant wire; linear constraints are free; the record and every candidate use the "
                  "same pinned compiler and flags; a candidate counts only if its non-linear count is strictly below "
                  "the record's; totals are reported")
ADMISSIBILITY_CIRCOM1 = [
    "the candidate declares `template <reference template>` with the reference's number of parameters; the harness "
    "writes `component main = <reference call>;` (circom 1 has no `{public [..]}`: main inputs keep the reference's "
    "public / private declarations)",
    "no `component main`, no `pragma` in the candidate files (circom 1 has none)",
    "includes: bare `<name>.circom` files next to the candidate, or files of the pinned repository snapshot "
    "(`repo/<path>`); nothing else",
    "compiled by the reference's pinned circom 1 package (package-lock digest-checked) with the record's flags and "
    "prime",
    "main inputs and outputs equal the reference's (prime, names, order, counts, public / private split)",
    "every record R1CS is canonicalized before it is counted, digested or modelled",
]
SUBCOMPONENT_MEANING = (
    "The reference is a sub-component of the listed parent instantiation(s) of the registry, as instantiated there: "
    "the same template with the same concrete parameters, prime and compiler; its standalone --O0 constraints equal "
    "(up to renaming) the sub-circuit of each listed instance in the parent's unoptimized compile, and every other "
    "constraint of the parent that touches the instance touches only its input and output signals.  A candidate "
    "accepted for this problem has the same input-output relation as the reference, so substituting it for the "
    "sub-component in the parent preserves the parent's input-output relation (its DET included); the parent's own "
    "constraint count drops by the difference.")
STATEMENT_DESCRIPTION = (
    "over F = ZMod p ([Fact (Nat.Prime p)]): for all input vectors x and output vectors y, "
    "(∃ w, Ref.Constraints w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y) ↔ "
    "(∃ w', Cand.Constraints w' ∧ Cand.Inputs.map w' = x ∧ Cand.Outputs.map w' = y), Ref being the registry DET "
    "model of the reference and Cand the model of the candidate's canonical --O2 R1CS; inputs and outputs are "
    "matched by position (the pipeline requires identical name lists)")
DETERMINISM_NOTE = (
    "reference DET + equiv => candidate DET: two candidate assignments with equal inputs x and outputs y1, y2 make "
    "(x, y1) and (x, y2) reference-realizable, so y1 = y2 by the reference's DET")


class Reject(Exception):
    """The candidate is not admissible (exit 4)."""


class NotEligible(Exception):
    """The reference cannot be a ratchet problem; ``code`` is a short reason class."""

    def __init__(self, code: str, detail: str, data: dict | None = None):
        super().__init__(f"{code}: {detail}")
        self.code, self.detail, self.data = code, detail, data or {}


sha = lambda b: hashlib.sha256(b).hexdigest()   # noqa: E731
now = lambda: time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())   # noqa: E731


def read_json(path: str):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def put(path: str, text: str) -> None:
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def fresh_dir(path: str) -> str:
    """An empty working directory (an existing one is removed first; callers pass paths under their own output)."""
    if os.path.islink(path):
        raise RuntimeError(f"refusing to replace a symbolic link: {path}")
    if os.path.isdir(path):
        shutil.rmtree(path)
    os.makedirs(path)
    return path


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in GENERATOR_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


# ------------------------------------------------------------------------------------------ metric

def nonlinear(c) -> bool:
    """A constraint (A, B, C) is non-linear iff A and B both contain a non-constant wire (wire != 0, coefficient
    != 0); every other constraint is linear (A or B is a constant)."""
    a, b, _ = c
    return any(w != 0 and k != 0 for w, k in a) and any(w != 0 and k != 0 for w, k in b)


def counts_of(r: R.R1cs) -> dict:
    nl = sum(1 for c in r.constraints if nonlinear(c))
    return {"nonlinear": nl, "linear": len(r.constraints) - nl, "total": len(r.constraints)}


def counts(r1cs_path: str) -> dict:
    return counts_of(R.read_r1cs(r1cs_path))


def score(record: dict, cnt: dict) -> dict:
    """Candidate counts against the record: smaller iff its non-linear count is below the record's."""
    def pct(a, b):
        return round(100.0 * (a - b) / a, 2) if a else 0.0
    return {"nonlinear": cnt["nonlinear"], "linear": cnt["linear"], "total": cnt["total"],
            "smaller": cnt["nonlinear"] < record["nonlinear"],
            "reduction_pct": pct(record["nonlinear"], cnt["nonlinear"]),
            "total_reduction_pct": pct(record["total"], cnt["total"])}


def canonicalize(r: R.R1cs) -> R.R1cs:
    """The same R1CS (header, wire numbering and labels unchanged) with every linear combination's terms ordered by
    wire and the constraints in sorted order."""
    key = sorted(tuple(tuple(sorted(lc)) for lc in c) for c in r.constraints)
    return R.R1cs(r.prime, r.field_bytes, r.n_wires, r.n_pub_out, r.n_pub_in, r.n_prv_in, r.n_labels,
                  [tuple(list(lc) for lc in c) for c in key], list(r.wire_to_label), list(r.section_order))


def canonical_r1cs(path: str) -> str:
    """Write ``<stem>.canon.r1cs`` next to ``path``; returns its path."""
    out = path[:-len(".r1cs")] + ".canon.r1cs"
    with open(out, "wb") as f:
        f.write(R.encode_r1cs(canonicalize(R.read_r1cs(path))))
    return out


# ------------------------------------------------------------------------------------------ snapshots

def manifest(base: str) -> dict[str, str]:
    """sha256 of every file under ``base`` (relative path -> digest), MANIFEST.sha256 and links excluded."""
    out = {}
    for root, dirs, names in os.walk(base):
        dirs.sort()
        for n in sorted(names):
            p = os.path.join(root, n)
            rel = os.path.relpath(p, base)
            if rel != "MANIFEST.sha256" and not os.path.islink(p):
                out[rel] = P.sha256_file(p)
    return dict(sorted(out.items()))


def write_manifest(base: str) -> str:
    m = manifest(base)
    put(os.path.join(base, "MANIFEST.sha256"), "".join(f"{h}  {rel}\n" for rel, h in m.items()))
    return P.sha256_file(os.path.join(base, "MANIFEST.sha256"))


def verify_snapshot(base: str, want_sha: str | None = None) -> dict[str, str]:
    """A snapshot must match its manifest exactly (no extra, missing or changed file)."""
    mpath = os.path.join(base, "MANIFEST.sha256")
    if want_sha and P.sha256_file(mpath) != want_sha:
        raise RuntimeError(f"snapshot manifest {mpath} is not the pinned one")
    want = {}
    with open(mpath, encoding="utf-8") as f:
        for ln in f:
            if ln.strip():
                h, rel = ln.rstrip("\n").split("  ", 1)
                want[rel] = h
    got = manifest(base)
    if got != want:
        bad = sorted(set(got.items()) ^ set(want.items()))[:5]
        raise RuntimeError(f"snapshot {base} differs from its manifest: {bad}")
    return want


def snapshot_id(repo: str, commit: str, deps: list[dict]) -> str:
    sid = f"{repo.replace('/', '__')}@{commit[:12]}"
    if deps:
        sid += "+" + sha(json.dumps(deps, sort_keys=True).encode())[:8]
    return sid


def nomain_copy(base: str, include: str) -> bool:
    """The registry compiles an include-context file that declares ``component main`` through a sibling copy with
    the declaration blanked (``<file>.boole-nomain``); writes that copy into the snapshot if needed."""
    p = os.path.join(base, "repo", include)
    with open(p, encoding="utf-8") as f:
        text = f.read()
    if not cs._MAIN_RE.search(cs.strip_comments(text)):
        return False
    if not os.path.exists(p + ".boole-nomain"):
        put(p + ".boole-nomain", cs.blank_mains(text))
    return True


def scan_closure(base: str, start: str, libs: list[str]) -> dict[str, cs.SourceFile]:
    """Files reached from ``start`` (snapshot-relative, e.g. ``repo/x.circom``), resolved like circom: relative to
    the including file, then the library paths ``repo/<lib>``."""
    lib_dirs = [os.path.normpath("repo/" + lib) for lib in libs]
    files, todo = {}, [start]
    while todo:
        p = todo.pop()
        if p in files:
            continue
        sf = cs.scan_file(base, p, lib_dirs)
        if sf.unresolved_includes:
            raise RuntimeError(f"{p}: unresolved includes {sf.unresolved_includes}")
        files[p] = sf
        todo.extend(reversed(sf.includes))
    return files


# ------------------------------------------------------------------------------------------ compile

def compiler_kind(tag: str) -> str:
    """``circom2`` (release binaries and the v2.0.9 source build) or ``circom1`` (the npm package)."""
    rel = CD.CIRCOM_RELEASES.get(tag)
    if rel is None:
        raise NotEligible("compiler", f"circom {tag} is not a pinned compiler")
    return rel["kind"]


def compiler_binary(compilers: dict, tag: str) -> str:
    """The pinned compiler of ``tag`` (registry ``CIRCOM_RELEASES`` digests), digest-checked on every use: a binary,
    or for circom 1 the npm install root."""
    if tag not in PINNED_COMPILERS:
        raise NotEligible("compiler", f"circom {tag} is not a pinned compiler of the ratchet")
    if tag not in compilers:
        raise RuntimeError(f"no binary configured for circom {tag}")
    CD.verify_compiler(tag, compilers[tag])
    return compilers[tag]


def is_circom1(binary: str) -> bool:
    """A circom 1 compiler is configured as its npm install root (a directory)."""
    return os.path.isdir(binary)


def compile_circuit(main_text: str, workdir: str, flags: list[str], binary: str, snapshot: str,
                    files: dict[str, str] | None = None, libs: list[str] = (), node: str | None = None,
                    canonical: bool | None = None) -> dict:
    """Compile ``main_text`` (plus ``files``: name -> text next to it) in a fresh ``workdir``; the snapshot's
    top-level entries are linked into it (read-only use) and ``libs`` (repository-relative library paths) become
    ``-l <workdir>/repo/<lib>``.  Every ``--O2`` R1CS is canonicalized (``canonical``: also another record compile,
    circom 1's reduced one).  ``binary`` is a circom 2 binary or the circom 1 npm install root (run by ``node``;
    circom 1 has no library paths: includes resolve relative to the including file)."""
    fresh_dir(workdir)
    for name, text in (files or {}).items():
        put(os.path.join(workdir, name), text)
    for e in sorted(os.listdir(snapshot)):
        if e != "MANIFEST.sha256":
            os.symlink(os.path.join(snapshot, e), os.path.join(workdir, e))
    put(os.path.join(workdir, "main.circom"), main_text)
    c1 = is_circom1(binary)
    if c1:
        if libs:
            raise RuntimeError("circom 1 has no library paths")
        nb = node_binary(node)
        cmd = [nb, os.path.join(binary, "node_modules", "circom", "cli.js"), "main.circom", *flags]
        env = {"PATH": os.path.dirname(nb) + ":/usr/bin:/bin", "HOME": workdir, "TMPDIR": workdir,
               "NODE_OPTIONS": "--max-old-space-size=16384"}
    else:
        lflags = [x for lib in libs for x in ("-l", os.path.normpath(os.path.join(workdir, "repo", lib)))]
        cmd = [binary, "main.circom", *flags, *lflags, "-o", "."]
        env = {"PATH": "/usr/bin:/bin", "HOME": workdir, "TMPDIR": workdir}
    run = L.run_process(cmd, env, workdir, COMPILE_TIMEOUT, COMPILE_RSS_MB)
    log = re.sub(r"\x1b\[[0-9;]*m", "", run.out)
    put(os.path.join(workdir, "circom.log"), log)
    res = {"rc": run.rc, "compile_secs": run.secs, "compile_peak_rss_mb": run.peak_rss_mb,
           "main_sha256": sha(main_text.encode())}
    stem = "main_c" if "--c" in flags else "main"
    r1cs = os.path.join(workdir, stem + ".r1cs")
    if run.rc != 0 or run.timeout or run.memkill or not os.path.exists(r1cs):
        err = [ln.strip() for ln in log.splitlines() if "error" in ln.lower()]
        res["error"] = (" | ".join(err[:6]) or log[-600:] or ("timeout" if run.timeout else "memory guard"))[:1200]
        return res
    if R.read_header(r1cs).custom_gate_uses:
        res["error"] = "the circuit uses circom custom gates (PLONK), which have no R1CS form"
        return res
    sym = os.path.join(workdir, stem + ".sym")
    res.update(n_constraints=R.read_header(r1cs).n_constraints, r1cs=r1cs, sym=sym,
               r1cs_sha256=P.sha256_file(r1cs), sym_sha256=P.sha256_file(sym))
    if ("--O2" in flags) if canonical is None else canonical:
        canon = canonical_r1cs(r1cs)
        res.update(r1cs_raw_sha256=res["r1cs_sha256"], r1cs=canon, r1cs_sha256=P.sha256_file(canon))
    res["n_wires"] = R.read_header(res["r1cs"]).n_wires
    if c1 and "-w" in flags:
        res.update(js_dir=workdir, wasm=os.path.join(workdir, flags[flags.index("-w") + 1]),
                   witness_js=os.path.join(binary, "node_modules", "circom_runtime"))
    elif "--wasm" in flags:
        res["js_dir"] = os.path.join(workdir, stem + "_js")
        res.update(wasm=os.path.join(res["js_dir"], stem + ".wasm"),
                   witness_js=os.path.join(res["js_dir"], "witness_calculator.js"))
    return res


def public_clause(public: list[str]) -> str:
    """``{public [a, b]}``: every main input is public in the cost compile (circom --O2 may otherwise substitute a
    private main input away)."""
    return f" {{public [{', '.join(public)}]}}" if public else ""


def input_bases(names: list[str]) -> list[str]:
    out = []
    for nm in names:
        b = R.signal_base(nm)[len("main."):]
        if b not in out:
            out.append(b)
    return out


def signature(res: dict) -> dict:
    """Main-component I/O signature: wires and .sym names in R1CS order (outputs, then inputs)."""
    r = R.read_r1cs(res["r1cs"])
    io = R.main_io_wires(r, R.read_sym(res["sym"]))
    return {"prime": str(r.prime), "prime_name": r.prime_name, "n_pub_out": r.n_pub_out,
            "n_inputs": r.n_pub_in + r.n_prv_in, "n_pub_in": r.n_pub_in, "outputs": io.outputs, "inputs": io.inputs,
            "output_names": io.output_names, "input_names": io.input_names}


def io_mismatch(ref_io: dict, sig: dict) -> str | None:
    """Admissibility of a candidate's main I/O: same prime, names and order; the main inputs public as in the
    record (circom 2: every main input, ``public_inputs``; circom 1, whose reduction never removes a main input,
    keeps the reference's own public / private split)."""
    pub = set(ref_io["public_inputs"])
    want = {"prime": ref_io["prime"], "n_pub_out": len(ref_io["output_names"]),
            "n_pub_in": sum(1 for nm in ref_io["input_names"] if R.signal_base(nm)[len("main."):] in pub),
            "output_names": ref_io["output_names"], "input_names": ref_io["input_names"]}
    for k, v in want.items():
        if sig[k] != v:
            return f"main I/O differs from the reference ({k}): {str(sig[k])[:200]} != {str(v)[:200]}"
    if sig["n_inputs"] != len(ref_io["input_names"]):
        return "main inputs must all be public in the cost compile"
    return None


def kind_of(spec: dict) -> str:
    """The compiler kind of a reference spec or of ``spec_of(problem)`` (circom 2 when no compiler is named)."""
    tag = (spec.get("compiler") or {}).get("tag")
    return compiler_kind(tag) if tag else "circom2"


def prime_flags(spec: dict) -> list[str]:
    if not spec.get("prime_flag"):
        return []
    return ["-p" if kind_of(spec) == "circom1" else "--prime", spec["prime_flag"]]


def model_flags(spec: dict) -> list[str]:
    return list(CIRCOM1_FLAGS_MODEL if kind_of(spec) == "circom1" else FLAGS_MODEL) + prime_flags(spec)


def record_flags(spec: dict) -> list[str]:
    return list(CIRCOM1_FLAGS_RECORD if kind_of(spec) == "circom1" else FLAGS_RECORD) + prime_flags(spec)


def wasm_flags(spec: dict) -> list[str]:
    return list(CIRCOM1_FLAGS_WASM if kind_of(spec) == "circom1" else FLAGS_WASM)


def record_optimization(spec: dict) -> str:
    """The optimization level of the record compile (the same for every candidate)."""
    if kind_of(spec) == "circom1":
        return ("circom 1 full constraint reduction (its only optimization: linear constraints eliminate internal "
                "signals; main inputs and outputs are kept)")
    return "--O2 (full constraint simplification)"


# ------------------------------------------------------------------------------------------ reference

def reference_spec(reg_dir: str) -> dict:
    """The reference fields of a registry DET package (``problem.json`` of the effective record)."""
    prob = read_json(os.path.join(reg_dir, "problem.json"))
    ids, ins, circ, st, chk = prob["ids"], prob["instantiation"], prob["circuit"], prob["statement"], prob["checker"]
    flags = circ["compiler"]["flags"]
    model = st["model_file"]
    return {
        "registry_package_id": prob["package_id"], "property": prob["property"]["template"], "status": prob["status"],
        "repo": ids["repo"], "repo_url": ids["repo_url"], "commit": ids["commit"], "path": ids["path"],
        "template": ids["template"], "args": [str(a) for a in ins["args"]], "call": ins["call"],
        "include": ins.get("include_context") or ids["path"],
        "libs": [flags[i + 1] for i, x in enumerate(flags) if x == "-l"],
        "prime_flag": (flags[flags.index("--prime") + 1] if "--prime" in flags else
                       flags[flags.index("-p") + 1] if "-p" in flags else None),
        "dependencies": ids.get("dependencies") or [],
        "compiler": {"tag": "v" + circ["compiler"]["version"], "version": circ["compiler"]["version"],
                     "binary_sha256": circ["compiler"]["binary_sha256"]},
        "main_sha256": ins.get("main_sha256"), "tag_wrapper": bool(ins.get("tag_wrapper")),
        "preconditions": bool(st.get("preconditions")),
        "r1cs_sha256": circ["r1cs_sha256"], "n_constraints": circ["n_constraints"], "n_wires": circ["n_wires"],
        "prime": circ["prime"], "prime_name": circ.get("prime_name"),
        "model": {"file": model, "module": st["model_module"], "namespace": st["model_module"][:-len(".Model")],
                  "sha256": next(f["sha256"] for f in chk["files"] if f["path"] == model)},
        "det_theorem_fqn": st["theorem_fqn"], "lean_opts": chk["lean_opts"], "env": prob["env"],
    }


def static_exclusion(spec: dict) -> tuple[str, str] | None:
    """(reason class, detail) when a registry record cannot be a ratchet reference before any compile."""
    if spec["property"] != "DET":
        return "property", f"{spec['property']} package (the reference meaning must be a full DET model)"
    if spec["compiler"]["tag"] not in PINNED_COMPILERS:
        return "compiler", f"circom {spec['compiler']['tag']}: not a pinned compiler of the ratchet"
    if spec["tag_wrapper"] or spec["preconditions"]:
        return "tag-wrapper", "tagged inputs compiled through a wrapper main (DET under input preconditions)"
    if spec["n_constraints"] > SIZE_LIMIT:
        return "size", f"--O0 model of {spec['n_constraints']} constraints above {SIZE_LIMIT}"
    return None


def det_exclusion(det: dict | None, model_sha256: str) -> tuple[str, str] | None:
    """The reference's DET must be machine-checked on this exact model file within the proof limits."""
    if not det:
        return "det", "no machine-checked DET proof of the reference model"
    if det.get("method") not in DET_METHODS or (det.get("checker") or {}).get("verdict") != "PASS":
        return "det", f"DET evidence {det.get('method')} without a production checker PASS"
    if det.get("model_sha256") != model_sha256:
        return "det", "DET proof is not of this model file"
    chk = det["checker"]
    lim = DET_PROOF_LIMITS
    if (chk.get("peak_rss_mb") or 0) > lim["rss_mb"] or (chk.get("secs") or 0) > lim["timeout_s"]:
        return "det-limits", (f"DET proof check above the proof limits ({chk.get('peak_rss_mb')} MB, "
                              f"{chk.get('secs')} s; limits {DET_PROOF_LIMITS})")
    return None


DECOMPOSITION_MEANING = (
    "The reference is a callee of the TOO-LARGE parent record(s) as the parent compiles it: the same function at the "
    "same generic (type) arguments, packaged alone by the registry's DET generator. Its problem is about this callee "
    "alone. A candidate equivalent to it (the same input-output relation) can replace it at its call sites in a parent "
    "without changing the parent's input-output relation, so a cheaper accepted candidate is a cheaper implementation "
    "of that part of the parent; no statement is made about the parent's DET, cost or proof.")


CALLER_INSTANCE = "caller instance: "      # provenance prefix of an instantiation taken from a TOO-LARGE caller


def decomposition_reference(prob: dict) -> dict | None:
    """``reference.decomposition`` of a ratchet problem whose registry package is a callee instance of TOO-LARGE
    parents (decomposition by instances: provenance :data:`CALLER_INSTANCE`): its parents, instance key and
    meaning."""
    ins = prob.get("instantiation") or {}
    d = ins.get("decomposition")
    if not d or not any(str(p).startswith(CALLER_INSTANCE) for p in ins.get("provenance") or []):
        return None
    return {"parents": [p["package_id"] for p in d.get("parents") or [] if p.get("package_id")],
            "n_parents": d.get("n_parents"), "depth": d.get("depth"), "instance_key": d.get("instance_key"),
            "meaning": DECOMPOSITION_MEANING}


def det_record(method: str, spec: dict, proof_path: str, verdict: dict, source: str, form: str | None = None,
               location: str | None = None) -> dict:
    """The problem's DET evidence: a proof of ``det`` for the reference model file that the production checker
    accepted (``verdict``: its verdict, peak RSS and seconds; ``location``: how to find the proof, default its
    path)."""
    rec = {"method": method, "registry_package_id": spec["registry_package_id"],
           "theorem_fqn": spec["det_theorem_fqn"], "model_file": spec["model"]["file"],
           "model_sha256": spec["model"]["sha256"], "proof_sha256": P.sha256_file(proof_path),
           "proof_location": location or proof_path, "source": source,
           "checker": {"verdict": verdict.get("verdict"), "peak_rss_mb": verdict.get("peak_rss_mb"),
                       "secs": verdict.get("secs"), "limits": dict(DET_PROOF_LIMITS)}}
    if form:
        rec["form"] = form
    return rec


def battery_det_solution(statement: str, n_constraints: int, form: str) -> str:
    """A ``det`` proof from a registry battery form that closed the statement (``triv_<variant>_<tactic>``): the
    statement with its ``sorry`` replaced by the form's tactic script, for the production checker."""
    m = re.fullmatch(r"triv_(V[0-4])_([A-Za-z_]+)", form)
    if not m or m.group(2) in ("bv_decide", "exactQ", "applyQ"):
        raise ValueError(f"no standard-axiom proof for battery form {form!r}")
    variant, tactic = m.groups()
    lines = E.battery_prefix(variant, n_constraints) + [f"all_goals {tactic}" if variant == "V4" else tactic]
    if variant in ("V3", "V4"):
        body = ["  set_option maxRecDepth 100000 in"] + [f"    {x}" for x in lines]
    else:
        body = [f"  {x}" for x in lines]
    if statement.count("\n  sorry\n") != 1:
        raise ValueError("statement has no single `sorry` line")
    return statement.replace("\n  sorry\n", "\n" + "\n".join(body) + "\n", 1)


def reference_main(spec: dict, nomain: bool, custom: bool, public: list[str] | None = None) -> str:
    """The registry's generated main (``instantiation.main_source``) with the include path inside the snapshot;
    with ``public`` the record's cost main (every main input public).  circom 1 has neither ``pragma`` nor
    ``{public [..]}``: its record main is the model main (its reduction keeps every main input)."""
    inc = "repo/" + spec["include"] + (".boole-nomain" if nomain else "")
    if kind_of(spec) == "circom1":
        return I.main_source(inc, spec["template"], tuple(spec["args"]), pragma=None, custom_templates=custom)
    text = I.main_source(inc, spec["template"], tuple(spec["args"]), pragma="2.0.0", custom_templates=custom)
    return text.replace("component main = ", f"component main{public_clause(public or [])} = ", 1)


def registry_main(spec: dict, nomain: bool, custom: bool) -> str:
    """The registry package's recorded main text (provenance: its sha256 is ``instantiation.main_sha256``)."""
    comment = (f"compiled with the component main declaration of {spec['include']} blanked" if nomain else None)
    pragma = None if kind_of(spec) == "circom1" else "2.0.0"
    return I.main_source(spec["include"], spec["template"], tuple(spec["args"]), pragma=pragma, comment=comment,
                         custom_templates=custom)


def node_binary(node: str | None = None) -> str:
    return node or shutil.which("node") or "node"


def run_generator(node: str, js_dir: str, r1cs: str, vectors: list[dict], work: str, tag: str,
                  witness_js: str | None = None, wasm: str | None = None) -> list[dict]:
    """The circuit's own wasm witness generator on every input object, each witness re-checked against the R1CS
    (``witness_js`` / ``wasm``: the compile result's generator, default circom 2's files in ``js_dir``; circom 1
    wasm files run through the pinned ``circom_runtime``)."""
    os.makedirs(work, exist_ok=True)
    inp, outp = os.path.join(work, tag + ".inputs.jsonl"), os.path.join(work, tag + ".results.jsonl")
    put(inp, "".join(json.dumps(v) + "\n" for v in vectors))
    wasm = wasm or os.path.join(js_dir, sorted(n for n in os.listdir(js_dir) if n.endswith(".wasm"))[0])
    witness_js = witness_js or os.path.join(js_dir, "witness_calculator.js")
    env = {"PATH": os.path.dirname(node) + ":/usr/bin:/bin", "HOME": work, "NODE_OPTIONS": "--max-old-space-size=8192"}
    r = subprocess.run([node, SIM_RUNNER, witness_js, wasm, r1cs, inp, outp], capture_output=True, text=True,
                       timeout=3600, env=env, cwd=work)
    if r.returncode != 0:
        raise RuntimeError(f"simulation runner failed: {(r.stderr or r.stdout)[-500:]}")
    with open(outp, encoding="utf-8") as f:
        res = [json.loads(ln) for ln in f]
    if len(res) != len(vectors):
        raise RuntimeError("simulation runner returned a different number of results")
    bad = [x["err"] for x in res if x["s"] == "R" and x["err"].startswith(SIM_HARNESS_ERRORS)]
    if bad:
        raise RuntimeError(f"simulation input objects do not fit the circuit: {bad[0]}")
    return res


def input_groups(input_names: list[str]) -> list[tuple[str, int]]:
    """Main input signals (witness-calculator key, number of elements) in wire order."""
    out: list[list] = []
    for nm in input_names:
        b = R.signal_base(nm)[len("main."):]
        if out and out[-1][0] == b:
            out[-1][1] += 1
        else:
            out.append([b, 1])
    return [(k, n) for k, n in out]


def vec(groups: list[tuple[str, int]], fill) -> dict:
    return {k: [str(fill(k, i)) for i in range(n)] for k, n in groups}


def probe_vectors(groups: list[tuple[str, int]], prime: int) -> tuple[list[dict], list[tuple[str, int]]]:
    vectors, idx = [], []
    for g, _ in groups:
        for k in range(255):
            vectors.append(vec(groups, lambda kk, i: min(2 ** k - 1, prime - 1) if kk == g else 0))
            idx.append((g, k))
    return vectors, idx


def probe_ranges(groups: list[tuple[str, int]], idx: list[tuple[str, int]], results: list[dict]) -> dict:
    """Declared range of each main input: the largest k such that the reference accepts the input at 2^j - 1 for
    every j <= k with the other inputs 0 (None if it rejects even 0)."""
    ok: dict = collections.defaultdict(dict)
    for (g, k), r in zip(idx, results):
        ok[g][k] = r["s"] == "A"
    out = {}
    for g, _ in groups:
        k = -1
        while k + 1 < 255 and ok[g].get(k + 1):
            k += 1
        out[g] = k if k >= 0 else None
    return out


def generator_file(witness_js: str) -> str:
    """The witness-calculator source file: circom 2's ``witness_calculator.js``, or the entry file of the pinned
    ``circom_runtime`` package (circom 1)."""
    if os.path.isdir(witness_js):
        return os.path.join(witness_js, read_json(os.path.join(witness_js, "package.json"))["main"])
    return witness_js


def measure_reference(spec: dict, snapshot: str, work: str, compilers: dict, node: str | None = None) -> dict:
    """Reference compiles: the --O0 model (byte-identical to the registry R1CS), the record (--O2, inputs public,
    canonical; circom 1: its full reduction, canonical) and the simulation reference (--O0 --wasm, same R1CS) with
    the probed input ranges."""
    binary = compiler_binary(compilers, spec["compiler"]["tag"])
    c1 = kind_of(spec) == "circom1"
    start = "repo/" + spec["include"]
    if not os.path.isfile(os.path.join(snapshot, start)):
        raise NotEligible("snapshot", f"{start} is not in the snapshot")
    with open(os.path.join(snapshot, start), encoding="utf-8") as f:
        nomain = bool(cs._MAIN_RE.search(cs.strip_comments(f.read())))
    if nomain and not os.path.isfile(os.path.join(snapshot, start + ".boole-nomain")):
        raise NotEligible("snapshot", f"{start} declares component main and the snapshot has no blanked copy")
    try:
        closure = scan_closure(snapshot, start, spec["libs"])
    except RuntimeError as ex:
        raise NotEligible("snapshot", str(ex)) from None
    custom = any(re.search(r"\bpragma\s+custom_templates\s*;", sf.clean) for sf in closure.values())
    m: dict = {"nomain": nomain, "custom_templates": custom, "closure_files": len(closure),
               "registry_main_identical": sha(registry_main(spec, nomain, custom).encode()) == spec["main_sha256"]}
    o0 = compile_circuit(reference_main(spec, nomain, custom), os.path.join(work, "O0"), model_flags(spec), binary,
                         snapshot, libs=spec["libs"], node=node)
    if "error" in o0:
        raise NotEligible("compile", f"--O0 compile failed: {o0['error'][:300]}")
    if o0["r1cs_sha256"] != spec["r1cs_sha256"]:
        raise NotEligible("provenance", "the --O0 compile is not byte-identical to the registry R1CS")
    if not m["registry_main_identical"]:
        raise NotEligible("provenance", "the reference main text differs from the registry's")
    sig = signature(o0)
    m["io"] = {k: sig[k] for k in ("prime", "prime_name", "output_names", "input_names")}
    m["public_inputs"] = input_bases(sig["input_names"][:sig["n_pub_in"]] if c1 else sig["input_names"])
    o2 = compile_circuit(reference_main(spec, nomain, custom, m["public_inputs"]), os.path.join(work, "O2"),
                         record_flags(spec), binary, snapshot, libs=spec["libs"], node=node, canonical=True)
    if "error" in o2:
        raise NotEligible("compile", f"--O2 compile failed: {o2['error'][:300]}")
    rsig = signature(o2)
    if io_mismatch(dict(m["io"], public_inputs=m["public_inputs"]), rsig):
        raise NotEligible("compile", "the record compile's main I/O differs from the model's")
    m["record"] = {"flags": record_flags_text(spec), "main_sha256": o2["main_sha256"],
                   "r1cs_sha256": o2["r1cs_sha256"], "r1cs_raw_sha256": o2["r1cs_raw_sha256"],
                   "n_wires": o2["n_wires"], **counts(o2["r1cs"])}
    if spec["compiler"]["tag"] not in RELEASE_COMPILERS:     # RT1 records (release binaries) keep their fields
        m["record"].update(optimization=record_optimization(spec),
                           compiler_sha256=CD.verify_compiler(spec["compiler"]["tag"], binary))
    m["model_counts"] = counts(o0["r1cs"])
    if m["record"]["nonlinear"] == 0:
        return m
    sw = compile_circuit(reference_main(spec, nomain, custom), os.path.join(work, "wasm"),
                         model_flags(spec) + wasm_flags(spec), binary, snapshot, libs=spec["libs"], node=node)
    if "error" in sw or sw["r1cs_sha256"] != spec["r1cs_sha256"]:
        raise NotEligible("compile", f"--O0 --wasm compile differs from the model: {sw.get('error', '')[:200]}")
    groups = input_groups(sig["input_names"])
    vectors, idx = probe_vectors(groups, int(sig["prime"]))
    res = run_generator(node_binary(node), sw["js_dir"], sw["r1cs"], vectors, os.path.join(work, "probe"), "probe",
                        sw["witness_js"], sw["wasm"])
    m["simulate"] = {"seed": None, "max_bits": probe_ranges(groups, idx, res), "ladder": list(LADDER),
                     "reference_generator": {"flags": model_flags_text(spec) + wasm_flags(spec),
                                             "wasm_sha256": P.sha256_file(sw["wasm"]),
                                             "witness_calculator_sha256": P.sha256_file(
                                                 generator_file(sw["witness_js"])),
                                             "r1cs_sha256": sw["r1cs_sha256"]}}
    return m


def model_flags_text(spec: dict) -> list[str]:
    return list(CIRCOM1_FLAGS_MODEL if kind_of(spec) == "circom1" else FLAGS_MODEL) + _flags_text(spec)


def record_flags_text(spec: dict) -> list[str]:
    return list(CIRCOM1_FLAGS_RECORD if kind_of(spec) == "circom1" else FLAGS_RECORD) + _flags_text(spec)


def _flags_text(spec: dict) -> list[str]:
    out = prime_flags(spec)
    for lib in spec["libs"]:
        out += ["-l", os.path.normpath(os.path.join("repo", lib))]
    return out


def problem_id(spec: dict) -> str:
    return "ratchet/" + spec["registry_package_id"]


def problem_record(spec: dict, measured: dict, det: dict, snapshot: dict, annotations: dict | None = None,
                   context: dict | None = None) -> dict:
    """The ratchet problem record (``problem.json``, status OPEN) from the reference fields, its measurements, the
    DET evidence and the snapshot pin.  ``context`` (a sub-component reference): ``{"kind": "sub-component",
    "parents": [{"registry_package_id", "call", "instance_paths", "n_instances"}], "located_by": ...}``; the record
    states the substitution meaning (:data:`SUBCOMPONENT_MEANING`)."""
    pid = problem_id(spec)
    c1 = kind_of(spec) == "circom1"
    sim = dict(measured["simulate"], seed=f"{SIM_SEED}|{pid}")
    env = {k: spec["env"][k] for k in ("lean", "mathlib", "lake_manifest_sha256", "packages") if k in spec["env"]}
    env.update(circom=spec["compiler"]["version"], circom_binary_sha256=spec["compiler"]["binary_sha256"])
    rec = {
        "schema_version": P.RATCHET_SCHEMA_VERSION, "kind": "ratchet", "package_id": pid, "status": "OPEN",
        "reference": {
            "registry_package_id": spec["registry_package_id"], "repo": spec["repo"], "repo_url": spec["repo_url"],
            "commit": spec["commit"], "path": spec["path"], "template": spec["template"], "args": spec["args"],
            "call": spec["call"], "include": spec["include"], "nomain": measured["nomain"],
            "custom_templates": measured["custom_templates"], "libs": spec["libs"], "prime_flag": spec["prime_flag"],
            "dependencies": spec["dependencies"], "compiler": spec["compiler"], "main_sha256": spec["main_sha256"],
            "model_flags": model_flags_text(spec),
            "model": dict(spec["model"], r1cs_sha256=spec["r1cs_sha256"], n_constraints=spec["n_constraints"],
                          n_wires=spec["n_wires"], counts=measured["model_counts"]),
            "io": dict(measured["io"], public_inputs=measured["public_inputs"]),
            "lean_opts": spec["lean_opts"]},
        "record": dict(measured["record"], rung=0, source=("reference (circom 1, full constraint reduction)"
                                                             if c1 else "reference (circom --O2)")),
        "metric": METRIC_CIRCOM1 if c1 else METRIC,
        "admissibility": list(ADMISSIBILITY_CIRCOM1 if c1 else ADMISSIBILITY), "statement_shape": STATEMENT_DESCRIPTION,
        "determinism": DETERMINISM_NOTE, "det": det, "simulate": sim, "snapshot": snapshot, "env": env,
        "generator": generator_info(), "created_utc": now(),
    }
    if context:
        rec["context"] = dict(context, meaning=SUBCOMPONENT_MEANING)
    if annotations:
        rec["annotations"] = annotations
    return rec


def build_problem(reg_dir: str, snapshot: str, out_dir: str, compilers: dict, det: dict | None,
                  snapshot_manifest_sha256: str | None = None, node: str | None = None, work: str | None = None,
                  annotations: dict | None = None, context: dict | None = None) -> dict:
    """Reference package -> ratchet problem package (``problem.json`` + the registry model file byte for byte +
    the reference main texts); raises NotEligible with the reason.  ``context``: see :func:`problem_record`."""
    spec = reference_spec(reg_dir)
    why = static_exclusion(spec) or det_exclusion(det, spec["model"]["sha256"])
    if why:
        raise NotEligible(*why)
    man = verify_snapshot(snapshot, snapshot_manifest_sha256)
    own_work = work is None
    work = work or tempfile.mkdtemp(prefix="ratchet-build-")
    try:
        measured = measure_reference(spec, snapshot, work, compilers, node)
    finally:
        if own_work:
            shutil.rmtree(work, ignore_errors=True)
    if measured["record"]["nonlinear"] == 0:
        raise NotEligible("linear-record", "the record has no non-linear constraint (every --O2 constraint is "
                                           "linear, free in context; no candidate can count)", measured)
    snap = {"id": snapshot_id(spec["repo"], spec["commit"], spec["dependencies"]),
            "manifest_sha256": P.sha256_file(os.path.join(snapshot, "MANIFEST.sha256")), "n_files": len(man),
            "rule": "the repository's .circom files at the pinned commit that the include closures of its references "
                    "reach, every .circom file of its circomlib dependency, and blanked copies of include files that "
                    "declare component main"}
    prob = problem_record(spec, measured, det, snap, annotations, context)
    src = os.path.join(reg_dir, spec["model"]["file"])
    dst = os.path.join(out_dir, spec["model"]["file"])
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(src, dst)
    nomain, custom = measured["nomain"], measured["custom_templates"]
    put(os.path.join(out_dir, "reference", "main.circom"), reference_main(spec, nomain, custom))
    put(os.path.join(out_dir, "reference", "record.circom"),
        reference_main(spec, nomain, custom, measured["public_inputs"]))
    errs = P.validate_problem(prob, out_dir)
    if errs:
        raise RuntimeError(f"ratchet problem does not validate: {errs[:5]}")
    P.write_json(os.path.join(out_dir, "problem.json"), prob)
    return prob


def load_problem(problem_dir: str) -> dict:
    prob = read_json(os.path.join(problem_dir, "problem.json"))
    errs = P.validate_problem(prob, problem_dir)
    if errs or prob.get("kind") != "ratchet":
        raise RuntimeError(f"{problem_dir}: not a valid ratchet problem: {errs[:5]}")
    return prob


def spec_of(prob: dict) -> dict:
    ref = prob["reference"]
    return {"libs": ref["libs"], "prime_flag": ref["prime_flag"], "compiler": ref["compiler"]}


# ------------------------------------------------------------------------------------------ candidates

_LOCAL_INC = re.compile(r"^[A-Za-z0-9_-]+\.circom$")
_PRAGMA = re.compile(r"\bpragma\s+circom\s+(\d+)\.(\d+)\.(\d+)\s*;")


def admit_sources(path: str, prob: dict, snapshot_files: dict[str, str]) -> tuple[dict[str, str], list[str], str]:
    """Source rules (raises Reject).  Returns ({build name: text}, [snapshot files included directly], pragma)."""
    ref = prob["reference"]
    base = os.path.dirname(os.path.abspath(path))
    tops = {rel.split("/", 1)[0] for rel in snapshot_files}
    reserved = {"main.circom", "Candidate.circom"} | tops
    lib_dirs = [os.path.normpath("repo/" + lib) for lib in ref["libs"]]
    c1 = kind_of(spec_of(prob)) == "circom1"
    out, snap, todo, versions = {}, [], [(path, "Candidate.circom")], [] if c1 else [(2, 0, 0)]
    templates = []
    while todo:
        src, name = todo.pop()
        if name in out:
            continue
        if not os.path.isfile(src):
            raise Reject(f"include {name!r}: no such file next to the candidate")
        with open(src, encoding="utf-8") as f:
            text = f.read()
        clean = cs.strip_comments(text)
        if cs._MAIN_RE.search(clean):
            raise Reject(f"{name}: declares `component main` (the harness writes the main component)")
        if re.search(r"\bpragma\s+custom_templates\b", clean):
            raise Reject(f"{name}: `pragma custom_templates` (custom gates have no R1CS form)")
        if c1 and re.search(r"\bpragma\b", clean):
            raise Reject(f"{name}: `pragma` (the reference's compiler is circom 1, which has no pragma)")
        versions += [tuple(int(x) for x in m.groups()) for m in _PRAGMA.finditer(clean)]
        templates += cs.find_templates(clean, name)
        out[name] = text
        for inc in cs._INCLUDE_RE.findall(clean):
            if ".." in inc.split("/") or inc.startswith("/") or "\\" in inc:
                raise Reject(f"{name}: include {inc!r} leaves the allowed directories")
            if _LOCAL_INC.match(inc) and inc not in reserved and os.path.isfile(os.path.join(base, inc)):
                todo.append((os.path.join(base, inc), inc))
                continue
            first = [os.path.normpath(inc)] if inc.split("/")[0] in tops else []
            hit = next((c for c in first + [os.path.normpath(os.path.join(d, inc)) for d in lib_dirs]
                        if c in snapshot_files), None)
            if hit is None or not hit.endswith(".circom"):
                raise Reject(f"{name}: include {inc!r} is neither a `<name>.circom` file next to the candidate nor a "
                             f"file of the pinned repository snapshot (`repo/<path>` or a path under the reference's "
                             f"library paths {['repo/' + x for x in ref['libs']]})")
            snap.append(hit)
    if c1:
        versions = [(0, 0, 0)]
    top = max(versions)
    pinned = tuple(int(x) for x in ref["compiler"]["version"].split("."))
    if top > pinned:
        raise Reject(f"pragma circom {'.'.join(map(str, top))} is newer than the pinned compiler "
                     f"{ref['compiler']['version']}")
    own = [t for t in templates if t.name == ref["template"]]
    if not own:
        raise Reject(f"the candidate declares no `template {ref['template']}` (same template name as the reference)")
    if len(own[0].params) != len(ref["args"]):
        raise Reject(f"`template {ref['template']}` takes {len(own[0].params)} parameter(s); the reference call "
                     f"`{ref['call']}` passes {len(ref['args'])}")
    return out, sorted(set(snap)), "none" if c1 else ".".join(map(str, top))


def candidate_main(prob: dict, pragma: str) -> str:
    ref = prob["reference"]
    if kind_of(spec_of(prob)) == "circom1":
        return f'include "Candidate.circom";\ncomponent main = {ref["call"]};\n'
    return (f'pragma circom {pragma};\ninclude "Candidate.circom";\n'
            f'component main{public_clause(ref["io"]["public_inputs"])} = {ref["call"]};\n')


def resolve_snapshot(problem_dir: str, prob: dict, snapshot: str | None) -> str:
    """The snapshot directory: given, or ``<problems root>/../snapshots/<id>`` next to the problem collection."""
    return snapshot or os.path.join(os.path.dirname(os.path.abspath(problem_dir)), os.pardir, "snapshots",
                                    prob["snapshot"]["id"])


def compile_candidate(prob: dict, cand_path: str, out: str, snapshot: str, compilers: dict,
                      node: str | None = None) -> tuple[dict, dict]:
    """Source rules, compile, main I/O, metric.  Returns (report, compile result); raises Reject."""
    man = verify_snapshot(snapshot, prob["snapshot"]["manifest_sha256"])
    os.makedirs(out, exist_ok=True)
    srcs, snap, pragma = admit_sources(cand_path, prob, man)
    rep: dict = {"problem": prob["package_id"], "candidate_file": os.path.abspath(cand_path), "started_utc": now(),
                 "source_sha256": sha(srcs["Candidate.circom"].encode()), "local_files": sorted(srcs),
                 "snapshot_includes": snap, "pragma": pragma}
    binary = compiler_binary(compilers, prob["reference"]["compiler"]["tag"])
    res = compile_circuit(candidate_main(prob, pragma), os.path.join(out, "compile"), record_flags(spec_of(prob)),
                          binary, snapshot, srcs, prob["reference"]["libs"], node=node, canonical=True)
    rep["compile"] = {k: v for k, v in res.items() if k not in ("r1cs", "sym", "js_dir", "wasm", "witness_js")}
    if "error" in res:
        raise Reject(f"compile failed: {res['error']}")
    why = io_mismatch(prob["reference"]["io"], signature(res))
    if why:
        raise Reject(why)
    rep["candidate"] = {"source_sha256": rep["source_sha256"], "local_files": rep["local_files"],
                        "snapshot_includes": snap, "pragma": pragma, "r1cs_sha256": res["r1cs_sha256"],
                        "sym_sha256": res["sym_sha256"], "n_wires": res["n_wires"],
                        **score(prob["record"], counts(res["r1cs"]))}
    rep["_srcs"], rep["_pragma"] = srcs, pragma
    return rep, res


# ------------------------------------------------------------------------------------------ statement

def namespaces(prob: dict) -> tuple[str, str, str]:
    """(statement namespace, reference model namespace, candidate model namespace)."""
    ref_ns = prob["reference"]["model"]["namespace"]
    tail = ref_ns.split(".", 1)[1] if ref_ns.startswith("ZkDet.") else E.lean_ident(ref_ns)
    base = "ZkRatchet." + tail
    return base, ref_ns, base + ".Cand"


def candidate_model(prob: dict, res: dict) -> str:
    _, _, cand_ns = namespaces(prob)
    ref = prob["reference"]
    r = R.read_r1cs(res["r1cs"])
    syms = R.read_sym(res["sym"])
    io = R.main_io_wires(r, syms)
    meta = {"repo_id": "ratchet candidate for " + ref["repo"], "instantiation": ref["call"],
            "repo_url": "(candidate file)", "commit": "-", "path": "Candidate.circom", "template": ref["template"],
            "rule": "ratchet-candidate", "generator": f"{GENERATOR_NAME} v{GENERATOR_VERSION}",
            "circom_version": ref["compiler"]["version"], "circom_flags": record_flags_text(spec_of(prob)) +
            ["(canonical constraint order)"], "r1cs_sha256": res["r1cs_sha256"],
            "prime_name": r.prime_name or "unknown"}
    text = E.emit_model(cand_ns, meta, r, io.outputs, io.inputs, R.wire_names(syms, r.n_wires))
    return text.replace("# DET model: ", "# Ratchet candidate model: ", 1)


def statement_text(prob: dict, cand: dict) -> str:
    base, ref_ns, cand_ns = namespaces(prob)
    ref, rec, io = prob["reference"], prob["record"], prob["reference"]["io"]
    rf = ref_ns + "."
    return "\n".join([
        f"import {E.model_module(ref_ns)}",
        f"import {E.model_module(cand_ns)}",
        "",
        f"namespace {base}",
        "",
        f"/-- Ratchet equivalence for {ref['repo']} `{ref['call']}` ({ref['path']} at {ref['commit'][:12]}).",
        f"Reference: the registry DET model `{ref_ns}` of package `{ref['registry_package_id']}`",
        f"(circom {ref['compiler']['version']} `--O0`, R1CS sha256 `{ref['model']['r1cs_sha256']}`); its DET is "
        f"machine-checked ({prob['det']['method']}).",
        f"Record: {rec['nonlinear']} non-linear ({rec['total']} total) constraints (circom "
        f"{ref['compiler']['version']} `--O2`, main inputs public).",
        f"Candidate `Cand`: source sha256 `{cand['source_sha256']}`, compiled like the record",
        f"(canonical R1CS sha256 `{cand['r1cs_sha256']}`): {cand['nonlinear']} non-linear ({cand['total']} total).",
        f"Inputs (same names and order in both): {E.summarize_names(io['input_names'])}; outputs:",
        f"{E.summarize_names(io['output_names'])}.",
        "For every input vector `x` and output vector `y`: some assignment satisfies the reference constraints",
        "with inputs `x` and outputs `y` iff some assignment satisfies the candidate constraints with inputs `x`",
        "and outputs `y`. -/",
        f"theorem equiv [Fact (Nat.Prime {rf}p)] :",
        f"    ∀ x y : List {rf}F,",
        f"      (∃ w : Fin {rf}nWires → {rf}F, {rf}Constraints w ∧ {rf}Inputs.map w = x ∧ {rf}Outputs.map w = y) ↔",
        "      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by",
        "  sorry",
        "",
        f"end {base}",
        "",
    ])


def elaborate(env: L.LeanEnv, pkg: str, modules: list[str], fqn: str, opts: list[str], work: str,
              timeout: float = DET_PROOF_LIMITS["timeout_s"]) -> dict:
    """G-ELAB for a statement importing several models: each model, then ``Statement.lean`` (exactly one `sorry`
    warning), then the production replay tool; returns timings, peak RSS and the elaborated type digest."""
    fresh_dir(work)
    build = os.path.join(work, "build")
    d: dict = {"steps": []}
    for rel in modules + ["Statement.lean"]:
        r, msgs = L.compile_module(env, pkg, rel, build, opts, timeout)
        d["steps"].append({"file": rel, "rc": r.rc, "secs": r.secs, "peak_rss_mb": r.peak_rss_mb,
                           "memkill": r.memkill, "timeout": r.timeout})
        if r.rc != 0 or r.timeout or r.memkill or L.errors(msgs):
            return dict(d, status="FAIL", errors=[L.fmt_msg(m) for m in L.errors(msgs)[:5]] or [r.out[-500:]])
        if rel == "Statement.lean":
            warns = [m for m in msgs if m.get("severity") == "warning"]
            if len(warns) != 1 or "sorry" not in warns[0].get("data", ""):
                return dict(d, status="FAIL", errors=["expected exactly one `declaration uses sorry` warning"])
    rr, post = G.run_replay(env, os.path.join(build, "Statement.olean"), fqn, os.path.join(work, "replay.out"), opts,
                            [build], timeout)
    d["steps"].append({"file": "replay", "rc": rr.rc, "secs": rr.secs, "peak_rss_mb": rr.peak_rss_mb})
    ok = (post["replay"] == "ok" and isinstance(post["target"], dict) and post["target"]["kind"] == "theorem"
          and set(post["axioms"]) <= G.ALLOWED_AXIOMS | {"sorryAx"} and "sorryAx" in post["axioms"])
    if not ok:
        return dict(d, status="FAIL", errors=[f"replay record unexpected: {rr.out[-300:]}"])
    put(os.path.join(work, "reference.type.txt"), post["type_str"])
    return dict(d, status="PASS", reference_type_sha256=post["type_sha256"],
                total_secs=round(sum(s["secs"] for s in d["steps"]), 2),
                peak_rss_mb=max(s["peak_rss_mb"] for s in d["steps"]))


def checker_files(prob: dict, stmt: str, cmodel: str) -> list[dict]:
    _, ref_ns, cand_ns = namespaces(prob)
    m = prob["reference"]["model"]
    return [{"path": "Statement.lean", "role": "statement", "sha256": sha(stmt.encode())},
            {"path": m["file"], "role": "import", "module": E.model_module(ref_ns), "sha256": m["sha256"]},
            {"path": E.model_relpath(cand_ns), "role": "import", "module": E.model_module(cand_ns),
             "sha256": sha(cmodel.encode())}]


def candidate_problem(prob: dict, cand: dict, files: list[dict], stmt: str, type_sha: str) -> dict:
    """The checker package record (status CANDIDATE): the problem plus the candidate, its statement and the checker
    block that the production checker reads."""
    base, ref_ns, cand_ns = namespaces(prob)
    fqn = base + ".equiv"
    out = {k: v for k, v in prob.items() if k not in ("annotations",)}
    out.update(status="CANDIDATE", candidate=cand, created_utc=now(),
               statement={"file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn, "text": stmt,
                          "imports": [E.model_module(ref_ns), E.model_module(cand_ns)],
                          "assumptions": P.statement_assumptions(prob["reference"]["io"]["prime_name"] or "")},
               checker={"statement_file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn,
                        "lean_opts": prob["reference"]["lean_opts"], "files": files,
                        "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                        "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS})
    return out


def write_candidate_package(prob: dict, problem_dir: str, cand: dict, res: dict, pkg: str) -> tuple[str, list[dict]]:
    """Statement and models of a candidate under ``pkg``; returns (statement text, checker files)."""
    _, _, cand_ns = namespaces(prob)
    stmt = statement_text(prob, cand)
    cmodel = candidate_model(prob, res)
    fresh_dir(pkg)
    m = prob["reference"]["model"]
    os.makedirs(os.path.dirname(os.path.join(pkg, m["file"])), exist_ok=True)
    shutil.copyfile(os.path.join(problem_dir, m["file"]), os.path.join(pkg, m["file"]))
    if P.sha256_file(os.path.join(pkg, m["file"])) != m["sha256"]:
        raise RuntimeError("reference model file changed")
    put(os.path.join(pkg, E.model_relpath(cand_ns)), cmodel)
    put(os.path.join(pkg, "Statement.lean"), stmt)
    return stmt, checker_files(prob, stmt, cmodel)


def run_candidate(problem_dir: str, cand_path: str, out: str, compilers: dict, snapshot: str | None = None,
                  env: L.LeanEnv | None = None, require_smaller: bool = False, node: str | None = None) -> dict:
    """The trusted candidate pipeline: admissibility, compile, metric, statement and models, and with a Lean
    environment the statement's elaboration and the checker package (``<out>/pkg``)."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, res = compile_candidate(prob, cand_path, out, snapshot, compilers, node)
    rep.pop("_srcs"), rep.pop("_pragma")
    cand = rep["candidate"]
    if require_smaller and not cand["smaller"]:
        P.write_json(os.path.join(out, "candidate.json"), rep)
        raise Reject(f"not smaller: {cand['nonlinear']} non-linear constraints ({cand['total']} total); record "
                     f"{prob['record']['nonlinear']} non-linear ({prob['record']['total']} total)")
    pkg = os.path.join(out, "pkg")
    stmt, files = write_candidate_package(prob, problem_dir, cand, res, pkg)
    rep["statement_sha256"] = files[0]["sha256"]
    if env is not None:
        base, _, _ = namespaces(prob)
        el = elaborate(env, pkg, [f["path"] for f in files[1:]], base + ".equiv", prob["reference"]["lean_opts"],
                       os.path.join(out, "elab"))
        rep["elab"] = el
        if el["status"] != "PASS":
            raise RuntimeError(f"statement does not elaborate: {el.get('errors')}")
        cprob = candidate_problem(prob, cand, files, stmt, el["reference_type_sha256"])
        errs = P.validate_problem(cprob, pkg)
        if errs:
            raise RuntimeError(f"checker package does not validate: {errs[:5]}")
        P.write_json(os.path.join(pkg, "problem.json"), cprob)
        rep["package"] = pkg
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "candidate.json"), rep)
    return rep


def run_check(problem_dir: str, cand_path: str, solution: str, out: str, compilers: dict, env: L.LeanEnv,
              snapshot: str | None = None, timeout: float = DET_PROOF_LIMITS["timeout_s"],
              node: str | None = None) -> tuple[str, dict]:
    """Final check: the candidate pipeline (strictly fewer non-linear constraints) and the production checker."""
    rep: dict = {"problem_dir": os.path.abspath(problem_dir), "solution_file": os.path.abspath(solution),
                 "started_utc": now()}
    try:
        cr = run_candidate(problem_dir, cand_path, os.path.join(out, "candidate"), compilers, snapshot, env,
                           require_smaller=True, node=node)
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

def sim_vectors(prob: dict, n_random: int) -> list[tuple[str, dict]]:
    """Deterministic input vectors: structured edge cases (zeros, ones, twos, p-1, 2^k - 1 and 2^k over the
    ladder and the declared input widths, each input at / just above its declared range, alternating patterns),
    then ``n_random`` seeded random vectors (a prefix of a longer run)."""
    io = prob["reference"]["io"]
    p = int(io["prime"])
    groups = input_groups(io["input_names"])
    rng_bits = prob["simulate"]["max_bits"]
    cap = lambda v: min(v, p - 1)                             # noqa: E731
    out, seen = [], set()

    def add(label, obj):
        key = json.dumps(obj, sort_keys=True)
        if key not in seen:
            seen.add(key)
            out.append((label, obj))
    add("edge: all 0", vec(groups, lambda k, i: 0))
    add("edge: all 1", vec(groups, lambda k, i: 1))
    add("edge: all 2", vec(groups, lambda k, i: 2))
    add("edge: all p-1", vec(groups, lambda k, i: p - 1))
    for b in sorted(set(LADDER) | {v for v in rng_bits.values() if v}):
        add(f"edge: all 2^{b}-1", vec(groups, lambda k, i: cap(2 ** b - 1)))
        add(f"edge: all 2^{b}", vec(groups, lambda k, i: cap(2 ** b)))
    mx = {k: (2 ** v - 1 if v is not None else p - 1) for k, v in rng_bits.items()}
    add("edge: every input at its declared max", vec(groups, lambda k, i: cap(mx[k])))
    add("edge: alternating 0 / declared max", vec(groups, lambda k, i: cap(mx[k]) if i % 2 else 0))
    add("edge: alternating declared max / 0", vec(groups, lambda k, i: 0 if i % 2 else cap(mx[k])))
    add("edge: ascending 0, 1, 2, ... (mod declared max + 1)", vec(groups, lambda k, i: i % (mx[k] + 1)))
    for g, _ in groups:
        add(f"edge: {g} at declared max, others 0", vec(groups, lambda k, i: cap(mx[k]) if k == g else 0))
        add(f"edge: {g} just above declared max, others at max",
            vec(groups, lambda k, i: cap(mx[k] + 1) if k == g else cap(mx[k])))
        add(f"edge: {g} = p-1, others at max", vec(groups, lambda k, i: p - 1 if k == g else cap(mx[k])))
    rng = random.Random(prob["simulate"]["seed"])
    modes = ["in-range"] * 9 + ["one-out"] * 3 + ["ladder"] * 3 + ["per-element"] * 2 + ["field"] * 2 + ["near-p"]

    def width(k):
        v = rng_bits.get(k)
        return v if v is not None and v < 254 else rng.choice(LADDER)

    def draw(w):
        if rng.random() < 0.25:
            return cap(rng.choice([0, 1, 2 ** w - 1, 2 ** max(w - 1, 0)]))
        return rng.randrange(min(2 ** w, p))
    for j in range(n_random):
        mode = rng.choice(modes)
        if mode in ("in-range", "one-out"):
            obj = {k: [str(draw(width(k))) for _ in range(n)] for k, n in groups}
            if mode == "one-out":
                k, n = rng.choice(groups)
                w = rng_bits.get(k)
                bad = rng.choice(([2 ** w + rng.randrange(4)] if w is not None and w < 253 else [])
                                 + [p - 1 - rng.randrange(4), rng.randrange(p)])
                obj[k][rng.randrange(n)] = str(cap(bad))
        elif mode == "ladder":
            b = rng.choice(LADDER)
            obj = vec(groups, lambda k, i: rng.randrange(min(2 ** b, p)))
        elif mode == "per-element":
            obj = vec(groups, lambda k, i: rng.randrange(min(2 ** rng.choice(LADDER + [width(k)]), p)))
        elif mode == "field":
            obj = vec(groups, lambda k, i: rng.randrange(p))
        else:
            obj = vec(groups, lambda k, i: p - 1 - rng.randrange(16))
        out.append((f"random #{j} ({mode})", obj))
    return out


def describe(r: dict) -> str:
    if r["s"] == "R":
        return f"rejected ({r['err'][:80]})"
    o = r["out"]
    head = ", ".join(o[:6]) + (f", ... ({len(o)} outputs)" if len(o) > 6 else "")
    return ("outputs [" if r["s"] == "A" else f"witness violates constraint {r['c']}; outputs [") + head + "]"


def compare_runs(vectors: list[tuple[str, dict]], ref: list[dict], cand: list[dict], cand_r1cs: R.R1cs,
                 cand_outputs: list[int], cand_output_names: list[str]) -> dict:
    """The screen's verdict.  Per vector the reference rejects (its generator fails) or yields outputs; the candidate
    must reject exactly the same vectors and give the same outputs.  Also mismatches: a candidate witness that
    violates the candidate's own R1CS, and a candidate output wire that occurs in no constraint."""
    mism, kinds = [], collections.Counter()
    for (label, obj), a, b in zip(vectors, ref, cand):
        kind = None
        if b["s"] == "V":
            kind = "candidate witness violates the candidate R1CS"
        elif a["s"] == "R" and b["s"] != "R":
            kind = "reference rejects, candidate accepts"
        elif a["s"] != "R" and b["s"] == "R":
            kind = "reference accepts, candidate rejects"
        elif a["s"] != "R" and a["out"] != b["out"]:
            kind = "different outputs"
        if kind:
            kinds[kind] += 1
            if len(mism) < 20:
                mism.append({"vector": label, "kind": kind, "input": obj, "reference": describe(a),
                             "candidate": describe(b)})
    used = {w for c in cand_r1cs.constraints for lc in c for w, k in lc if k}
    free_out = [nm for w, nm in zip(cand_outputs, cand_output_names) if w not in used]
    if free_out:
        kinds["structural: output wire in no constraint"] += len(free_out)
    return {"vectors": len(vectors), "edge_cases": sum(1 for lb, _ in vectors if lb.startswith("edge")),
            "reference_accepts": sum(1 for x in ref if x["s"] == "A"),
            "reference_rejects": sum(1 for x in ref if x["s"] == "R"),
            "reference_violations": sum(1 for x in ref if x["s"] == "V"),
            "mismatches": sum(kinds.values()), "mismatch_kinds": dict(kinds), "examples": mism,
            "unconstrained_outputs": free_out[:20], "verdict": "PASS" if not kinds else "MISMATCH"}


def det_screen(prob: dict, model_text: str) -> dict:
    """Informational: the battery P3 propagation rules on the candidate's model.  DETERMINED means its outputs are
    functions of its inputs; STUCK is a warning (a possible under-constrained hint), not a mismatch."""
    from zk_registry import mech_r1cs as M
    try:
        pr = M.propagate(M.parse_model(model_text, "theorem det :\n"))
        return {"propagation": pr.status, "undetermined_outputs": len(pr.undetermined_outputs)}
    except Exception as ex:                                  # noqa: BLE001 - informational only
        return {"propagation": "ERROR", "error": str(ex)[:200]}


def simulate(problem_dir: str, cand_path: str, out: str, n_random: int, compilers: dict, snapshot: str | None = None,
             node: str | None = None) -> dict:
    """The simulation screen (not a proof) on a candidate."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, res = compile_candidate(prob, cand_path, os.path.join(out, "count"), snapshot, compilers, node)
    srcs, pragma = rep.pop("_srcs"), rep.pop("_pragma")
    ref = prob["reference"]
    binary = compiler_binary(compilers, ref["compiler"]["tag"])
    resw = compile_circuit(candidate_main(prob, pragma), os.path.join(out, "wasm"),
                           record_flags(spec_of(prob)) + wasm_flags(spec_of(prob)), binary, snapshot, srcs,
                           ref["libs"], node=node, canonical=True)
    if "error" in resw or resw["r1cs_sha256"] != rep["candidate"]["r1cs_sha256"]:
        raise RuntimeError(f"--wasm compile differs from the cost compile: {resw.get('error')}")
    gen = prob["simulate"]["reference_generator"]
    refw = compile_circuit(reference_main(ref, ref["nomain"], ref["custom_templates"]), os.path.join(out, "reference"),
                           model_flags(spec_of(prob)) + wasm_flags(spec_of(prob)), binary, snapshot, libs=ref["libs"],
                           node=node)
    if "error" in refw or refw["r1cs_sha256"] != ref["model"]["r1cs_sha256"] or \
            P.sha256_file(refw["wasm"]) != gen["wasm_sha256"]:
        raise RuntimeError(f"the reference generator does not rebuild as recorded: {refw.get('error')}")
    vectors = sim_vectors(prob, n_random)
    objs = [v for _, v in vectors]
    t0 = time.time()
    nb = node_binary(node)
    a = run_generator(nb, refw["js_dir"], refw["r1cs"], objs, os.path.join(out, "run"), "ref", refw["witness_js"],
                      refw["wasm"])
    b = run_generator(nb, resw["js_dir"], resw["r1cs"], objs, os.path.join(out, "run"), "cand", resw["witness_js"],
                      resw["wasm"])
    sig = signature(resw)
    sim = compare_runs(vectors, a, b, R.read_r1cs(resw["r1cs"]), sig["outputs"], sig["output_names"])
    sim.update(random=n_random, seed=prob["simulate"]["seed"], secs=round(time.time() - t0, 2),
               det_screen=det_screen(prob, candidate_model(prob, res)))
    rep["simulate"] = sim
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "simulate.json"), rep)
    return rep


# ------------------------------------------------------------------------------------------ CLI

def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--registry-package", required=True)
    b.add_argument("--snapshot", required=True)
    b.add_argument("--det", required=True, help="DET evidence JSON (det_record)")
    b.add_argument("--out", required=True)
    b.add_argument("--compilers", required=True)
    b.add_argument("--node")
    b.add_argument("--work")
    for nm in ("count", "simulate", "statement", "check"):
        x = sub.add_parser(nm)
        x.add_argument("--problem", required=True)
        x.add_argument("--candidate", required=True)
        x.add_argument("--out", required=True)
        x.add_argument("--compilers", required=True)
        x.add_argument("--snapshot")
        x.add_argument("--node", help="node binary (circom 1 compiles and the simulation runner)")
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
    compilers = read_json(a.compilers)
    if a.cmd == "build":
        try:
            prob = build_problem(a.registry_package, a.snapshot, a.out, compilers, read_json(a.det), node=a.node,
                                 work=a.work)
        except NotEligible as ex:
            print(f"NOT-ELIGIBLE {ex}")
            return 4
        print(f"OPEN {prob['package_id']} record {prob['record']['nonlinear']} non-linear "
              f"({prob['record']['total']} total)")
        return 0
    env = None
    if a.cmd in ("statement", "check"):
        L.set_lean_limits(slots=1, rss_mb=a.rss_mb)
        env = L.load_env(a.lean_env)
    if a.cmd == "check":
        verdict, rep = run_check(a.problem, a.candidate, a.solution, a.out, compilers, env, a.snapshot,
                                 node=a.node)
        rep["finished_utc"] = now()
        P.write_json(os.path.join(a.out, "verdict.json"), rep)
        print(f"{verdict} {rep.get('reason', '')}".rstrip())
        for k in ("error", "invalid", "fail"):
            for x in (rep.get("checker") or {}).get(k, []):
                print(f"  {k}: {x}")
        return EXIT[verdict]
    try:
        if a.cmd == "simulate":
            rep = simulate(a.problem, a.candidate, a.out, a.n, compilers, a.snapshot, a.node)
        elif a.cmd == "count":
            prob = load_problem(a.problem)
            rep, _ = compile_candidate(prob, a.candidate, a.out, resolve_snapshot(a.problem, prob, a.snapshot),
                                       compilers, a.node)
            rep.pop("_srcs"), rep.pop("_pragma")
            P.write_json(os.path.join(a.out, "candidate.json"), rep)
        else:
            rep = run_candidate(a.problem, a.candidate, a.out, compilers, a.snapshot, env, node=a.node)
    except Reject as ex:
        print(f"REJECTED {ex}")
        return 4
    c = rep["candidate"]
    print(f"{'SMALLER' if c['smaller'] else 'NOT-SMALLER'} nonlinear={c['nonlinear']} total={c['total']} "
          f"(reduction {c['reduction_pct']}%)")
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
