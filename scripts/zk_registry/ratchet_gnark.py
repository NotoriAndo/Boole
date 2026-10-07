#!/usr/bin/env python3
"""Ratchet problems (verified circuit optimization) for gnark references of the registry (Go circuits and gadgets).

The Circom ratchet (``ratchet.py``) carried over to gnark:

* **Reference meaning**: the registry package's DET model of the reference, byte for byte: the R1CS that gnark's
  builder compiles from the registry's wrapper circuit (range checks through gnark's own non-commitment checker; a
  commitment held at D + 1 fixed challenges), outputs compared as in the registry statement (native outputs by value,
  emulated outputs by value modulo their modulus).  The reference's DET must be machine-checked (the battery P3
  mechanical proof, or a battery closure re-checked as a proof) by the production checker within the proof limits.
* **Record**: the number of non-linear constraints of that model (``A * B = C`` with a non-constant wire in both A and
  B).  Linear constraints are free: an embedding caller substitutes them (gnark itself keeps linear combinations
  symbolic and only materializes them at assertions; the wrapper's exposure constraints are linear and identical in
  every candidate).  The count is over the statement's own constraint system, so the commitment model prices a
  challenge-dependent check once per challenge point (D + 1 copies): the model of a commitment-based circuit is itself
  a commitment-free R1CS of that size, so moving checks behind a commitment never buys a count that a commitment-free
  circuit could not reach.  Hint outputs are free wires: a hint saves constraints only where the remaining
  constraints still pin the outputs down (the proof decides; the screen flags outputs fixed only by a free wire).
  Range checks are priced as gnark's non-commitment checker (bit decomposition) in the record and in every candidate;
  gnark's production count (log-derivative range checks, its commitment) is reported alongside.
* **Candidate**: ``Candidate.go``, a file of the reference's own Go package (its unexported identifiers are in
  scope) declaring ``BooleRatchetCandidate``: a method of the reference's receiver type (methods and circuit
  ``Define``) or a function (functions) with the reference's signature.  The harness adds it to the package of the
  manifest-checked module snapshot through a build overlay (the snapshot is never written), takes the registry's
  wrapper circuit text with the one call of the reference renamed to ``BooleRatchetCandidate`` (same receiver, type
  arguments and arguments) and compiles it with the pinned Go toolchain (digest-checked; ``-mod=vendor``, no network,
  no cgo) against the snapshot's pinned dependencies (vendored).
* **Admissibility**: package clause of the reference's package; exactly one top-level ``BooleRatchetCandidate`` of
  the reference's kind; imports only from a fixed list of the Go standard library, the repository module and the
  vendored dependencies (no ``unsafe``, ``reflect``, ``os``, cgo or harness packages); no ``//go:`` directives or
  build constraints.  The wrapper's public and secret variables, output paths and emulated output groups must equal
  the reference's.  Hints are allowed (free in the model); a commitment follows the registry's commitment model (a
  polynomial identity in the challenge, at most one; anything else is not modelled and rejected).
* **Statement** (generated per candidate, both directions of the input-output relation): for every input vector
  ``x``, native output vector ``y`` and emulated output values ``z``: some assignment satisfies the reference's
  constraints with those inputs, native outputs and emulated output values (modulo their moduli) iff some assignment
  satisfies the candidate's.
* **Screen** (``simulate``): both wrappers through gnark's own solver (the harness ``simulate`` runner) on the same
  structured edge vectors and seeded random vectors; the candidate must be solvable exactly where the reference is
  and give the same outputs (emulated outputs by value modulo the modulus); witnesses of the first vectors are
  re-checked against both models by the Python R1CS evaluator, the reference model is rebuilt and compared with the
  problem's digest, and an output fixed only by a free wire is a mismatch.  A screen, not a proof.
* **Final check**: the candidate pipeline (strictly fewer non-linear constraints than the record) and the unchanged
  production checker (``check.py``) on the generated package.

Usage::

    python3 -m zk_registry.ratchet_gnark count     --problem DIR --candidate Candidate.go --out DIR --tools DIR
    python3 -m zk_registry.ratchet_gnark simulate  --problem DIR --candidate Candidate.go --out DIR --tools DIR
                                                   [--n 2000]
    python3 -m zk_registry.ratchet_gnark statement --problem DIR --candidate Candidate.go --out DIR --tools DIR
                                                   --lean-env E.json
    python3 -m zk_registry.ratchet_gnark check     --problem DIR --candidate Candidate.go --solution S --out DIR
                                                   --tools DIR --lean-env E.json

``--tools DIR`` holds ``go<version>/`` (a GOROOT, digest-checked against the problem on every use) and, unless
``--gocache`` names another directory, the shared Go build cache ``gocache/`` (content-addressed, safe for concurrent
builds).  ``--snapshot`` defaults to ``<problems>/../snapshots/<id>``.  Problems are built by :func:`build_problem`
(the wave driver supplies the DET evidence and the snapshot made by :func:`make_snapshot`).  Exit codes as in
``ratchet.py``: ``count`` / ``simulate`` / ``statement``: 0 smaller (``simulate``: and no mismatch), 4 rejected or
not smaller, 5 simulation mismatch, 3 error; ``check``: 0 PASS, 1 FAIL, 2 INVALID, 3 ERROR, 4 REJECTED.
"""
from __future__ import annotations

import argparse
import collections
import hashlib
import json
import os
import re
import shutil
import sys
import time

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C              # noqa: E402
from zk_registry import gnark_lean_emit as GE   # noqa: E402
from zk_registry import gnark_r1cs as GR        # noqa: E402
from zk_registry import lean_emit as E          # noqa: E402
from zk_registry import lean_runner as L        # noqa: E402
from zk_registry import package as P            # noqa: E402
from zk_registry import r1cs as R               # noqa: E402
from zk_registry import ratchet as RT           # noqa: E402

GENERATOR_NAME = RT.GENERATOR_NAME
GENERATOR_VERSION = "1-gnark"
TOOL_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "gnark_tool")
TOOL_FILES = ["main.go", "harness/builder.go", "harness/expose.go", "harness/registry.go", "harness/run.go",
              "harness/sample.go", "harness/simulate.go"]
GENERATOR_SOURCES = ["ratchet_gnark.py", "ratchet.py", "schema/ratchet_gnark_problem.schema.json", "gnark_r1cs.py",
                     "gnark_lean_emit.py", "lean_emit.py", "r1cs.py", "mech_r1cs.py", "package.py", "check.py"] + \
                    [f"gnark_tool/{f}" for f in TOOL_FILES]
HERE = os.path.dirname(os.path.abspath(__file__))

CANDIDATE_NAME = "BooleRatchetCandidate"
CANDIDATE_FILE = "boole_ratchet_candidate.go"          # the overlay name inside the reference's package directory
TOOL_SUBDIR = "boolegnarkx"                           # the harness tool, overlaid inside the module (never on disk)
HARNESS_IMPORT = "boole.local/gnarkx/"
COMPILE_LIMIT = 100000                                 # gnarkx run -limit (the registry's guard)
CANDIDATE_SIZE_POLICY = 4000                           # largest candidate model (constraints)
MAX_POINTS = 64
BUILD_TIMEOUT, BUILD_RSS_MB = 900, 12288
RUN_TIMEOUT, RUN_RSS_MB = 900, 12288
SIM_SEED = "boole-ratchet-gnark-sim"
SIM_WITNESSES = 64                                     # vectors whose witnesses are re-checked by the evaluator
SIM_PROFILES = ["zero", "one", "two", "max", "half", "bit", "byte", "u16", "u32", "u64", "uniform"]
EXIT = RT.EXIT
DET_PROOF_LIMITS = RT.DET_PROOF_LIMITS
STD_IMPORTS = ("bytes", "cmp", "container/heap", "container/list", "crypto/elliptic", "crypto/sha256",
               "crypto/sha512", "crypto/subtle", "encoding/binary", "encoding/hex", "errors", "fmt", "hash", "iter",
               "maps", "math", "math/big", "math/bits", "slices", "sort", "strconv", "strings", "unicode",
               "unicode/utf8")

METRIC = ("non-linear constraints of the reference's DET model (the statement's constraint system: gnark's R1CS "
          "builder at the pinned version, range checks through gnark's non-commitment checker, a commitment's "
          "challenge-dependent part once per challenge point): constraints A * B = C whose A and B both contain a "
          "non-constant wire; linear constraints are free (substituted when the gadget is embedded); hint outputs "
          "are free wires; a candidate counts only if its model has strictly fewer non-linear constraints than the "
          "record; totals and gnark's production count are reported")
ADMISSIBILITY = [
    "Candidate.go is a file of the reference's Go package (its package clause), added to the package directory of "
    "the pinned module snapshot by a build overlay (the snapshot is not modified)",
    "exactly one top-level `BooleRatchetCandidate`: a method of the reference's receiver type (methods, circuit "
    "Define) or a function (functions); the harness renames the one reference call of the registry wrapper to it "
    "(same receiver, type arguments and arguments)",
    "imports: a fixed list of the Go standard library, packages of the repository module (not the harness) and the "
    "snapshot's vendored dependencies; no cgo, no `//go:` directives or build constraints",
    "compiled by the pinned Go toolchain (digest-checked) with -mod=vendor, no network and no cgo against the "
    "manifest-checked snapshot; the wrapper's public and secret variables, output paths and emulated output groups "
    "equal the reference's",
    "hints are free wires; a commitment must follow the registry's commitment model (one, a polynomial identity in "
    "the challenge, held at D + 1 points); other checks (log-derivative lookups) are not modelled and rejected; the "
    f"candidate model has at most {CANDIDATE_SIZE_POLICY} constraints",
]
STATEMENT_DESCRIPTION = (
    "over F = ZMod p ([Fact (Nat.Prime p)]): for all input vectors x, native output vectors y and emulated output "
    "values z, (∃ w, Ref.Constraints w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y ∧ Ref.EmulatedOutputs.map "
    "(fun g => Ref.emValue w g.1 g.2.1 % g.2.2) = z) ↔ (the same for Cand); Ref is the registry DET model of the "
    "reference, Cand the model of the candidate's wrapper (the z part is omitted when the outputs have no emulated "
    "elements); inputs and outputs are matched by position (identical wrapper variables and output paths)")
DETERMINISM_NOTE = (
    "reference DET + equiv => candidate DET: two candidate assignments with equal inputs x and outputs (y1, z1), "
    "(y2, z2) make both output pairs reference-realizable for x, so y1 = y2 and z1 = z2 by the reference's DET")

Reject, NotEligible = RT.Reject, RT.NotEligible
sha, now, read_json, put, fresh_dir = RT.sha, RT.now, RT.read_json, RT.put, RT.fresh_dir


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in GENERATOR_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


def tool_sha256() -> str:
    """Digest of the harness tool sources the builds overlay (``gnark_tool``)."""
    h = hashlib.sha256()
    for rel in TOOL_FILES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(TOOL_DIR, rel)).encode() + b"\n")
    return h.hexdigest()


# ------------------------------------------------------------------------------------------ metric

def counts_of(r: R.R1cs) -> dict:
    return RT.counts_of(r)


def score(record: dict, cnt: dict) -> dict:
    def pct(a, b):
        return round(100.0 * (a - b) / a, 2) if a else 0.0
    return {"nonlinear": cnt["nonlinear"], "linear": cnt["linear"], "total": cnt["total"],
            "smaller": cnt["nonlinear"] < record["nonlinear"],
            "reduction_pct": pct(record["nonlinear"], cnt["nonlinear"]),
            "total_reduction_pct": pct(record["total"], cnt["total"])}


def harness_facts(res: dict, model: GR.Model) -> dict:
    """What the record reports besides the counts: gnark's production count, the commitment, range checks, hints."""
    sym, prod = res.get("symbolic") or {}, res.get("production") or {}
    com = model.commitment
    return {"production_constraints": prod.get("n_constraints"), "commitments": int(com.get("commits") or 0),
            "challenge_degree": com.get("degree"), "challenge_points": int(com.get("points") or 0),
            "range_checks": int(sym.get("range_checks") or 0),
            "range_check_bits": int(sym.get("range_check_bits") or 0),
            "hint_wires": int(sum(model.hints.values())), "n_wires": model.r.n_wires}


def io_signature(res: dict) -> dict:
    """The wrapper's variables and exposed outputs (what a candidate must keep)."""
    sym = res["symbolic"]
    return {"n_public": sym["n_public"], "n_secret": sym["n_secret"],
            "public_names": list(sym.get("public_names") or []),
            "secret_names": list(sym.get("secret_names") or []), "output_paths": list(sym["output_paths"]),
            "groups": [dict(g) for g in sym["groups"]], "n_inputs": sym["n_public"] + sym["n_secret"] - 1,
            "n_outputs": len(sym["outputs"])}


def io_mismatch(ref_io: dict, sig: dict) -> str | None:
    for k in ("n_public", "n_secret", "public_names", "secret_names", "output_paths", "groups"):
        if sig[k] != ref_io[k]:
            return f"the wrapper's {k.replace('_', ' ')} differ from the reference's: {str(sig[k])[:160]}"
    return None


# ------------------------------------------------------------------------------------------ reference

def reference_spec(reg_dir: str) -> dict:
    """The reference fields of a registry gnark DET package (``problem.json``, ``evidence/wrapper.go``)."""
    prob = read_json(os.path.join(reg_dir, "problem.json"))
    ids, ins, st, chk = prob["ids"], prob["instantiation"], prob.get("statement") or {}, prob.get("checker") or {}
    circ = prob.get("circuit") or {}
    gn = circ.get("gnark") or {}
    spec = {"registry_package_id": prob["package_id"], "property": prob["property"]["template"],
            "status": prob["status"], "repo": ids["repo"], "repo_url": ids["repo_url"], "commit": ids["commit"],
            "path": ids["path"], "symbol": ids["template"], "symbol_line": ids.get("template_line"),
            "call": ins.get("call") or "", "rule": ins.get("rule"), "wrapper_id": gn.get("wrapper_id"),
            "curve": gn.get("curve") or circ.get("prime_name"), "prime": circ.get("prime"),
            "sizing_only": bool(gn.get("sizing_only")), "r1cs_sha256": circ.get("r1cs_sha256"),
            "gnark_version": (circ.get("compiler") or {}).get("version"), "env": prob["env"],
            "generator": prob.get("generator") or {}, "emulated": bool(st.get("emulated_outputs")),
            "decomposition": RT.decomposition_reference(prob),
            "size_policy": (circ.get("size_policy") or {}).get("max_constraints") or P.MAX_CONSTRAINTS}
    if st:
        spec.update(model={"file": st["model_file"], "module": st["model_module"],
                           "namespace": st["model_module"][:-len(".Model")],
                           "sha256": next(f["sha256"] for f in chk["files"] if f["path"] == st["model_file"])},
                    det_theorem_fqn=st["theorem_fqn"], lean_opts=chk["lean_opts"])
    wpath = os.path.join(reg_dir, "evidence", "wrapper.go")
    if os.path.exists(wpath):
        with open(wpath, encoding="utf-8") as f:
            first, _, wrapper = f.read().partition("\n")
        if not first.startswith("// wrapper compiled against "):
            raise NotEligible("provenance", "evidence/wrapper.go has no recorded header line")
        spec["wrapper"] = wrapper
    return spec


def reference_policy(n: int | None) -> int:
    """The size policy a reference is rebuilt under: the registry's own (2,000 constraints for wave G1, 4,000 for the
    decomposition records of wave RT-G2, the candidate cap), never below the default."""
    return min(max(P.MAX_CONSTRAINTS, int(n or 0)), CANDIDATE_SIZE_POLICY)


def target_name(spec: dict) -> str:
    """The Go name of the reference (method, function or ``Define`` of a circuit type)."""
    return "Define" if spec["call"].endswith(").Define(api)") else spec["symbol"].rsplit(".", 1)[-1]


_CALL = r"(?P<target>(?:recv|c\.In0|p_[A-Za-z0-9_]+))\.(?P<name>%s)(?=[\[(])"


def call_site(wrapper: str, name: str) -> tuple[str, int]:
    """(kind, offset of the name) of the one call of the reference in the wrapper's ``Define`` body: ``method``
    (``recv.Name(``), ``circuit`` (``c.In0.Define(``) or ``function`` (``p_pkg.Name(`` / ``p_pkg.Name[T](``)."""
    m = re.search(r"\) Define\(api frontend\.API\) error \{\n", wrapper)
    if not m:
        raise ValueError("the wrapper has no Define method")
    body_start = m.end()
    end = wrapper.find("\n}\n", body_start)
    hits = list(re.finditer(_CALL % re.escape(name), wrapper[body_start:end]))
    if len(hits) != 1:
        raise ValueError(f"the reference call `.{name}(` occurs {len(hits)} times in the wrapper's Define body")
    h = hits[0]
    kind = {"recv": "method", "c.In0": "circuit"}.get(h.group("target"), "function")
    return kind, body_start + h.start("name")


def candidate_wrapper(wrapper: str, name: str) -> str:
    """The registry wrapper with the reference call renamed to the candidate (same receiver and arguments)."""
    _, off = call_site(wrapper, name)
    return wrapper[:off] + CANDIDATE_NAME + wrapper[off + len(name):]


def static_exclusion(spec: dict) -> tuple[str, str] | None:
    if spec["property"] != "DET":
        return "property", f"{spec['property']} package"
    if spec["sizing_only"] or not spec.get("wrapper") or not spec.get("wrapper_id") or "model" not in spec:
        return "provenance", "no recorded wrapper, wrapper id or model"
    try:
        call_site(spec["wrapper"], target_name(spec))
    except ValueError as ex:
        return "provenance", str(ex)
    return None


def registry_model_meta(spec: dict, res: dict, model: GR.Model) -> dict:
    """The metadata the registry's model emitter was given (``gnark_det.build_package``)."""
    com = model.commitment
    commitment_txt = ("no commitment" if not com.get("commits") else
                      f"one commitment; challenge degree {com['degree']}, held at {com['points']} fixed challenges")
    gen = spec["generator"]
    return {"repo_id": spec["repo"], "instantiation": spec["call"],
            "generator": f"{gen.get('name')} v{gen.get('version')}",
            "repo_url": spec["repo_url"], "commit": spec["commit"], "path": spec["path"], "template": spec["symbol"],
            "wrapper_id": spec["wrapper_id"], "rule": spec["rule"], "call": spec["call"],
            "gnark_version": spec["gnark_version"], "go_version": str(spec["env"].get("go", "")).split(" ")[0],
            "curve": res["curve"], "commitment": commitment_txt, "r1cs_sha256": spec["r1cs_sha256"]}


def measure_reference(reg_dir: str, spec: dict | None = None) -> dict:
    """The registry harness result (``evidence/harness_result.json``): the model it assembles must have the registry
    R1CS digest and re-render to the package's Model.lean byte for byte; returns the record counts and the I/O."""
    spec = spec or reference_spec(reg_dir)
    try:
        res = read_json(os.path.join(reg_dir, "evidence", "harness_result.json"))
        model = GR.build(res)
    except (OSError, ValueError, KeyError) as ex:
        raise NotEligible("provenance", f"registry harness result does not assemble: {ex}") from None
    digest = sha(R.encode_r1cs(model.r))
    if digest != spec["r1cs_sha256"]:
        raise NotEligible("provenance", "the registry harness result does not give the registry R1CS")
    text = GE.emit_model(spec["model"]["namespace"], registry_model_meta(spec, res, model), model.r, model.inputs,
                         model.native_outputs, model.group_wires(), model.wire_names)
    if sha(text.encode()) != spec["model"]["sha256"]:
        raise NotEligible("provenance", "the registry harness result does not render to the package's model file")
    return {"res": res, "model": model, "counts": counts_of(model.r), "facts": harness_facts(res, model),
            "io": io_signature(res), "r1cs_sha256": digest}


def model_key(meas: dict) -> str:
    """Content key of a reference model: its R1CS and I/O (two references with the same key state the same thing)."""
    io = meas["io"]
    body = {"r1cs": meas["r1cs_sha256"], "outputs": io["output_paths"], "groups": io["groups"],
            "n_inputs": io["n_inputs"]}
    return sha(json.dumps(body, sort_keys=True).encode())


def battery_det_solution(statement: str, n_constraints: int, emulated: bool, form: str) -> str:
    """A ``det`` proof from a registry battery form that closed the gnark statement: the statement with its ``sorry``
    replaced by the form's tactic script (``gnark_lean_emit.battery_prefix``)."""
    m = re.fullmatch(r"triv_(V[0-4])_([A-Za-z_]+)", form)
    if not m or m.group(2) in ("bv_decide", "exactQ", "applyQ"):
        raise ValueError(f"no standard-axiom proof for battery form {form!r}")
    variant, tactic = m.groups()
    lines = GE.battery_prefix(variant, n_constraints, emulated) + [f"all_goals {tactic}" if variant == "V4" else tactic]
    if variant in ("V3", "V4"):
        body = ["  set_option maxRecDepth 100000 in"] + [f"    {x}" for x in lines]
    else:
        body = [f"  {x}" for x in lines]
    if statement.count("\n  sorry\n") != 1:
        raise ValueError("statement has no single `sorry` line")
    return statement.replace("\n  sorry\n", "\n" + "\n".join(body) + "\n", 1)


# ------------------------------------------------------------------------------------------ snapshot and toolchain

SNAPSHOT_RULE = ("repo/: the repository module at the ledger pin (go.mod, go.sum and every non-test .go file; no "
                 "_test.go files, testdata or .git) with vendor/ written by `go mod vendor` of that module with the "
                 "harness tool present (the dependency versions pinned by the repository's go.mod and go.sum)")


def module_info(snapshot: str, module_dir: str) -> tuple[str, str]:
    """(module root, module path) of the snapshot."""
    root = os.path.join(snapshot, "repo", module_dir) if module_dir else os.path.join(snapshot, "repo")
    with open(os.path.join(root, "go.mod"), encoding="utf-8") as f:
        m = re.search(r"^module\s+(\S+)", f.read(), re.M)
    if not m:
        raise NotEligible("snapshot", "go.mod has no module line")
    return root, m.group(1)


def package_layout(snapshot: str, module_dir: str, path: str) -> dict:
    """Module and package of the reference's file in the snapshot."""
    root, mpath = module_info(snapshot, module_dir)
    src = os.path.join(snapshot, "repo", path)
    if not os.path.isfile(src):
        raise NotEligible("snapshot", f"{path} is not in the snapshot")
    with open(src, encoding="utf-8") as f:
        m = re.search(r"^package\s+([A-Za-z_][A-Za-z0-9_]*)", f.read(), re.M)
    pdir = os.path.relpath(os.path.dirname(src), root)
    if not m or pdir.startswith(".."):
        raise NotEligible("snapshot", f"{path}: no package clause inside the module")
    if os.path.exists(os.path.join(os.path.dirname(src), CANDIDATE_FILE)) or os.path.exists(
            os.path.join(root, TOOL_SUBDIR)):
        raise NotEligible("snapshot", "the snapshot holds a reserved harness path")
    pdir = "" if pdir == "." else pdir
    return {"module_dir": module_dir, "module_path": mpath, "package_dir": pdir, "package_name": m.group(1),
            "import_path": mpath + ("/" + pdir if pdir else "")}


def go_root(tools: str, env: dict) -> str:
    """The pinned Go toolchain of a problem (``<tools>/go<version>``), its go and compile binaries digest-checked."""
    root = os.path.join(tools, "go" + env["go"])
    for rel, key in (("bin/go", "go_sha256"), (env["go_compile"], "go_compile_sha256")):
        p = os.path.join(root, rel)
        if not os.path.isfile(p):
            raise RuntimeError(f"no Go {env['go']} toolchain under {tools}")
        if P.sha256_file(p) != env[key]:
            raise RuntimeError(f"{p} is not the pinned Go {env['go']} toolchain")
    return root


def go_pins(goroot: str) -> dict:
    """The pins of a Go toolchain: version, digests of bin/go and of the compiler."""
    with open(os.path.join(goroot, "VERSION"), encoding="utf-8") as f:
        version = f.readline().strip()
    tdir = os.path.join(goroot, "pkg", "tool")
    tool = next(os.path.join("pkg", "tool", d, "compile") for d in sorted(os.listdir(tdir))
                if os.path.isfile(os.path.join(tdir, d, "compile")))
    return {"go": version[2:] if version.startswith("go") else version,
            "go_sha256": P.sha256_file(os.path.join(goroot, "bin", "go")), "go_compile": tool,
            "go_compile_sha256": P.sha256_file(os.path.join(goroot, tool))}


def go_env(goroot: str, work: str, gocache: str, online: bool = False) -> dict:
    """Build environment: the pinned toolchain only, vendored modules, no network (``online``: module downloads from
    the public proxy, for ``make_snapshot``), no cgo, every cache and temporary file in ``work`` / ``gocache``."""
    env = {"PATH": os.path.join(goroot, "bin") + ":/usr/bin:/bin", "GOROOT": goroot, "HOME": os.path.join(work, "home"),
           "TMPDIR": os.path.join(work, "tmp"), "GOPATH": os.path.join(work, "gopath"),
           "GOMODCACHE": os.path.join(work, "gomod"), "GOCACHE": gocache, "GOTOOLCHAIN": "local", "GOWORK": "off",
           "GOENV": "off", "GOTELEMETRY": "off", "CGO_ENABLED": "0", "GOFLAGS": "-mod=vendor", "GOPROXY": "off",
           "GOSUMDB": "off", "GIT_TERMINAL_PROMPT": "0", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}
    if online:
        env.update(GOFLAGS="-mod=mod", GOPROXY="https://proxy.golang.org", GOSUMDB="sum.golang.org")
    for k in ("HOME", "TMPDIR", "GOPATH", "GOMODCACHE", "GOCACHE"):
        os.makedirs(env[k], exist_ok=True)
    return env


def tool_texts(module_path: str) -> dict[str, str]:
    """The harness tool sources with their import path inside the module."""
    out = {}
    for rel in TOOL_FILES:
        with open(os.path.join(TOOL_DIR, rel), encoding="utf-8") as f:
            out[rel] = f.read().replace(HARNESS_IMPORT, f"{module_path}/{TOOL_SUBDIR}/")
    out["wrappers/doc.go"] = ("// Package wrappers holds generated wrapper circuits.\npackage wrappers\n\nimport (\n"
                              "\t_ \"github.com/consensys/gnark-crypto/ecc\"\n"
                              f"\t_ \"{module_path}/{TOOL_SUBDIR}/harness\"\n)\n")
    return out


def _copy_module(src: str, dst: str) -> int:
    n = 0
    for root, dirs, names in os.walk(src):
        dirs[:] = sorted(d for d in dirs if d not in (".git", "testdata", "vendor", TOOL_SUBDIR))
        for nm in sorted(names):
            rel = os.path.relpath(os.path.join(root, nm), src)
            if (nm.endswith(".go") and not nm.endswith("_test.go")) or rel in ("go.mod", "go.sum"):
                q = os.path.join(dst, rel)
                os.makedirs(os.path.dirname(q), exist_ok=True)
                shutil.copyfile(os.path.join(root, nm), q)
                n += 1
    return n


def make_snapshot(repo_root: str, module_dir: str, dest: str, goroot: str, work: str) -> dict:
    """The module snapshot of a repository checkout (``SNAPSHOT_RULE``); module downloads from the public proxy into
    ``work``.  Returns its manifest digest and file count."""
    fresh_dir(dest)
    mod_src = os.path.join(repo_root, module_dir) if module_dir else repo_root
    mod_dst = os.path.join(dest, "repo", module_dir) if module_dir else os.path.join(dest, "repo")
    _copy_module(mod_src, mod_dst)
    with open(os.path.join(mod_dst, "go.mod"), encoding="utf-8") as f:
        mpath = re.search(r"^module\s+(\S+)", f.read(), re.M).group(1)
    tdir = os.path.join(mod_dst, TOOL_SUBDIR)
    for rel, text in tool_texts(mpath).items():
        put(os.path.join(tdir, rel), text)
    env = go_env(goroot, work, os.path.join(work, "gocache"), online=True)
    r = L.run_process(["go", "mod", "vendor"], env, mod_dst, BUILD_TIMEOUT, BUILD_RSS_MB)
    shutil.rmtree(tdir)
    if r.rc != 0:
        raise RuntimeError(f"go mod vendor failed: {r.out[-600:]}")
    sha_m = RT.write_manifest(dest)
    return {"manifest_sha256": sha_m, "n_files": len(RT.manifest(dest)), "module_path": mpath}


def _scrub(text: str, *roots: str) -> str:
    for root in sorted({r for r in roots if r}, key=len, reverse=True):
        text = text.replace(root, "<" + ("work" if "work" in root else "snapshot") + ">")
    return text


def build_error(r: L.RunResult, work: str, snapshot: str) -> str:
    """The first lines of a failed build, with the overlaid files named as the candidate sees them."""
    lines = [ln for ln in r.out.splitlines() if ln.strip() and not ln.startswith("#")]
    msg = "timeout" if r.timeout else "memory guard" if r.memkill else " | ".join(lines[:6]) or "go build failed"
    msg = re.sub(r"[^\s:]*/Candidate\.go", "Candidate.go", msg)
    msg = re.sub(r"[^\s:]*/tool/wrappers/(w[0-9a-f]{12}_\d+\.go)", r"wrapper \1", msg)
    return _scrub(msg, work, snapshot)[:1200]


def build_tool(prob: dict, snapshot: str, tools: str, work: str, wrapper_text: str,
               candidate_text: str | None = None, gocache: str | None = None) -> dict:
    """``gnarkx`` built from the snapshot with the harness tool, the wrapper and (candidates) Candidate.go overlaid;
    returns {"binary"} or {"error"} with timings."""
    ref, env_pins = prob["reference"], prob["env"]
    goroot = go_root(tools, env_pins)
    root, mpath = module_info(snapshot, ref["module_dir"])
    fresh_dir(work)
    overlay = {}
    for rel, text in tool_texts(mpath).items():
        put(os.path.join(work, "tool", rel), text)
        overlay[os.path.join(root, TOOL_SUBDIR, rel)] = os.path.join(work, "tool", rel)
    wid = ref["wrapper"]["id"]
    wfile = os.path.join(work, "tool", "wrappers", wid + ".go")
    put(wfile, wrapper_text.replace(HARNESS_IMPORT, f"{mpath}/{TOOL_SUBDIR}/"))
    overlay[os.path.join(root, TOOL_SUBDIR, "wrappers", wid + ".go")] = wfile
    if candidate_text is not None:
        put(os.path.join(work, "Candidate.go"), candidate_text)
        pdir = os.path.join(root, ref["package_dir"]) if ref["package_dir"] else root
        overlay[os.path.join(pdir, CANDIDATE_FILE)] = os.path.join(work, "Candidate.go")
    for target in overlay:
        if os.path.exists(target):
            raise RuntimeError(f"overlay target exists in the snapshot: {target}")
    P.write_json(os.path.join(work, "overlay.json"), {"Replace": overlay})
    env = go_env(goroot, work, gocache or os.path.join(tools, "gocache"))
    binary = os.path.join(work, "gnarkx")
    r = L.run_process(["go", "build", "-trimpath", "-overlay", os.path.join(work, "overlay.json"), "-o", binary,
                       "./" + TOOL_SUBDIR], env, root, BUILD_TIMEOUT, BUILD_RSS_MB)
    put(os.path.join(work, "build.log"), r.out[-20000:])
    out = {"build_secs": r.secs, "build_peak_rss_mb": r.peak_rss_mb}
    if r.rc != 0 or not os.path.isfile(binary):
        out["error"] = build_error(r, work, snapshot)
        return out
    out["binary"] = binary
    return out


def run_tool(binary: str, args: list[str], work: str) -> L.RunResult:
    env = {"PATH": "/usr/bin:/bin", "HOME": os.path.join(work, "home"), "TMPDIR": os.path.join(work, "tmp")}
    for k in ("HOME", "TMPDIR"):
        os.makedirs(env[k], exist_ok=True)
    return L.run_process([binary] + args, env, work, RUN_TIMEOUT, RUN_RSS_MB)


def compile_program(prob: dict, snapshot: str, tools: str, work: str, candidate_text: str | None = None,
                    size_policy: int = CANDIDATE_SIZE_POLICY, gocache: str | None = None) -> dict:
    """The wrapper (the registry's, or with the reference call renamed and Candidate.go added) built and compiled by
    ``gnarkx run``; the model assembled from the result.  Returns the result, the model and the counts, or
    {"error"}."""
    ref = prob["reference"]
    wrapper = reference_text(prob, "wrapper")
    if candidate_text is not None:
        wrapper = candidate_wrapper(wrapper, ref["name"])
    b = build_tool(prob, snapshot, tools, work, wrapper, candidate_text, gocache)
    if "error" in b:
        return dict(b, error="compile failed: " + b["error"])
    out = os.path.join(work, "harness.json")
    r = run_tool(b["binary"], ["run", ref["wrapper"]["id"], out, "-limit", str(COMPILE_LIMIT), "-size-policy",
                               str(size_policy), "-samples", "0", "-max-points", str(MAX_POINTS)], work)
    b.update(run_secs=r.secs, run_peak_rss_mb=r.peak_rss_mb)
    if r.rc != 0 or not os.path.isfile(out):
        first = next((ln for ln in r.out.splitlines() if ln.startswith(("panic:", "fatal error"))), r.out[-300:])
        return dict(b, error=_scrub("the harness run failed: " + first, work, snapshot)[:800])
    res = read_json(out)
    if res.get("status") != "ok":
        why = {"too-large": f"the model exceeds {size_policy} constraints ({res.get('model_size')}"
                            + (" or more" if res.get("size_lower_bound") else "") + ")",
               "not-modelled": "not modelled by the registry's commitment model: " + str(res.get("error", "")),
               "no-exposure": "the wrapper exposed no outputs"}.get(res.get("status"),
                                                                    f"{res.get('status')}: {res.get('error', '')}")
        return dict(b, error=_scrub(why, work, snapshot)[:800], status=res.get("status"))
    try:
        model = GR.build(res)
    except GR.ModelError as ex:
        return dict(b, error=f"model not assembled: {ex}")
    b.update(result=res, model=model, counts=counts_of(model.r), facts=harness_facts(res, model),
             io=io_signature(res), r1cs_sha256=sha(R.encode_r1cs(model.r)))
    return b


def reference_text(prob: dict, key: str, problem_dir: str | None = None) -> str:
    """The text of a reference file recorded in the problem (``reference/wrapper.go``), digest-checked; the problem
    dict carries its directory under ``_dir``."""
    ref = prob["reference"]
    path = os.path.join(problem_dir or prob["_dir"], ref[key]["file"])
    with open(path, encoding="utf-8") as f:
        text = f.read()
    if sha(text.encode()) != ref[key]["sha256"]:
        raise RuntimeError(f"{ref[key]['file']} differs from the problem record")
    return text


# ------------------------------------------------------------------------------------------ candidates

_TOKEN = re.compile(r"""
    (?P<ws>[ \t\r]+) | (?P<nl>\n) | (?P<lc>//[^\n]*) | (?P<bc>/\*.*?\*/) |
    (?P<raw>`[^`]*`) | (?P<str>"(?:[^"\\\n]|\\.)*") | (?P<rune>'(?:[^'\\\n]|\\.)*') |
    (?P<id>[^\W\d]\w*) | (?P<num>\.?\d[\w.]*) | (?P<op>:=|\.\.\.|[^\s\w])
""", re.X | re.S)
_SEMI_AFTER = {"id", "num", "str", "raw", "rune"}


def go_tokens(text: str) -> list[tuple[str, str, int]]:
    """Tokens (kind, text, line) of a Go source, comments dropped; raises Reject on an unterminated literal."""
    out, line, pos = [], 1, 0
    while pos < len(text):
        m = _TOKEN.match(text, pos)
        if not m:
            raise Reject(f"Candidate.go: cannot read line {line} (unterminated literal or comment?)")
        kind = m.lastgroup
        tok = m.group()
        if kind not in ("ws", "nl", "lc", "bc"):
            out.append((kind, tok, line))
        line += tok.count("\n")
        pos = m.end()
    return out


def _decl_start(toks: list, i: int) -> bool:
    """Whether token ``i`` (``func`` at depth 0) starts a declaration: the first token, or after ``;`` or a line end
    that inserts a semicolon (an identifier, keyword, literal, ``)``, ``]``, ``}``, ``++`` or ``--`` ends the line)."""
    if i == 0:
        return True
    k, t, ln = toks[i - 1]
    if t == ";":
        return True
    return ln < toks[i][2] and (k in _SEMI_AFTER or t in (")", "]", "}", "++", "--"))


def admit_candidate(path: str, prob: dict, snapshot: str) -> str:
    """Source rules (raises Reject); returns the candidate text."""
    ref = prob["reference"]
    with open(path, encoding="utf-8") as f:
        text = f.read()
    for ln in text.splitlines():
        s = ln.strip()
        if s.startswith("//go:") or s.startswith("// +build") or s.startswith("//line "):
            raise Reject(f"Candidate.go must not carry directives or build constraints: {s[:60]!r}")
    toks = go_tokens(text)
    if len(toks) < 2 or toks[0][1] != "package":
        raise Reject("Candidate.go must start with a package clause")
    if toks[1][1] != ref["package_name"]:
        raise Reject(f"Candidate.go must be in the reference's package `{ref['package_name']}` (found "
                     f"`{toks[1][1]}`)")
    root, mpath = module_info(snapshot, ref["module_dir"])
    imports, i = [], 2
    while i < len(toks):
        while i < len(toks) and toks[i][1] == ";":
            i += 1
        if i >= len(toks) or toks[i][1] != "import":
            break
        i += 1
        group = toks[i][1] == "("
        i += 1 if group else 0
        while i < len(toks):
            if group and toks[i][1] == ")":
                i += 1
                break
            if toks[i][1] == ";":
                i += 1
                continue
            j = i
            while j < len(toks) and toks[j][0] not in ("str", "raw"):
                j += 1
            if j >= len(toks):
                raise Reject("Candidate.go: malformed import declaration")
            imports.append(toks[j][1][1:-1])
            i = j + 1
            if not group:
                break
    for imp in imports:
        allowed = imp in STD_IMPORTS
        if not allowed and "." in imp.split("/", 1)[0]:
            if imp == mpath or imp.startswith(mpath + "/"):
                rel = imp[len(mpath):].lstrip("/")
                allowed = TOOL_SUBDIR not in rel.split("/") and os.path.isdir(os.path.join(root, rel))
            else:
                allowed = os.path.isdir(os.path.join(root, "vendor", imp))
        if not allowed:
            raise Reject(f"import {imp!r} is not allowed (a fixed list of the standard library, the repository module "
                         "and the snapshot's vendored dependencies)")
    decls, depth = [], 0
    for k, (kind, t, ln) in enumerate(toks):
        if t in ("{", "(", "["):
            depth += 1
        elif t in ("}", ")", "]"):
            depth -= 1
        elif t == "func" and depth == 0 and _decl_start(toks, k):
            recv, j = None, k + 1
            if j < len(toks) and toks[j][1] == "(":
                d, ids, j = 1, [], j + 1
                while j < len(toks) and d:
                    if toks[j][1] in ("(", "["):
                        d += 1
                    elif toks[j][1] in (")", "]"):
                        d -= 1
                    elif d == 1 and toks[j][0] == "id":
                        ids.append(toks[j][1])
                    j += 1
                recv = ids[-1] if ids else ""
            if j < len(toks) and toks[j][0] == "id":
                decls.append((toks[j][1], recv))
    own = [d for d in decls if d[0] == CANDIDATE_NAME]
    if len(own) != 1:
        raise Reject(f"Candidate.go must declare exactly one top-level `{CANDIDATE_NAME}` ({len(own)} found)")
    want = ref["receiver_type"]
    got = own[0][1]
    if want is None and got is not None:
        raise Reject(f"`{CANDIDATE_NAME}` must be a function (the reference `{ref['symbol']}` is a function)")
    if want is not None and got != want:
        raise Reject(f"`{CANDIDATE_NAME}` must be a method of `{want}` (the reference `{ref['symbol']}` is one)")
    return text


def load_problem(problem_dir: str) -> dict:
    prob = read_json(os.path.join(problem_dir, "problem.json"))
    errs = P.validate_problem(prob, problem_dir)
    if errs or prob.get("schema_version") != P.RATCHET_GNARK_SCHEMA_VERSION:
        raise RuntimeError(f"{problem_dir}: not a valid gnark ratchet problem: {errs[:5]}")
    if prob["reference"]["tool"]["sha256"] != tool_sha256():
        raise RuntimeError("the harness tool sources differ from the ones the problem pins")
    prob["_dir"] = os.path.abspath(problem_dir)
    return prob


def resolve_snapshot(problem_dir: str, prob: dict, snapshot: str | None) -> str:
    return RT.resolve_snapshot(problem_dir, prob, snapshot)


def compile_candidate(prob: dict, cand_path: str, out: str, snapshot: str, tools: str,
                      gocache: str | None = None) -> tuple[dict, dict]:
    """Source rules, build, compile, I/O, metric.  Returns (report, compile result); raises Reject."""
    RT.verify_snapshot(snapshot, prob["snapshot"]["manifest_sha256"])
    os.makedirs(out, exist_ok=True)
    text = admit_candidate(cand_path, prob, snapshot)
    rep: dict = {"problem": prob["package_id"], "candidate_file": os.path.abspath(cand_path), "started_utc": now(),
                 "source_sha256": sha(text.encode())}
    res = compile_program(prob, snapshot, tools, os.path.join(out, "compile"), text, gocache=gocache)
    rep["compile"] = {k: v for k, v in res.items() if k not in ("result", "model", "binary")}
    if "error" in res:
        raise Reject(res["error"])
    why = io_mismatch(prob["reference"]["io"], res["io"])
    if why:
        raise Reject(why)
    rep["candidate"] = {"source_sha256": rep["source_sha256"], "local_files": ["Candidate.go"],
                        "r1cs_sha256": res["r1cs_sha256"], "n_wires": res["model"].r.n_wires,
                        **score(prob["record"], res["counts"]),
                        **{k: res["facts"][k] for k in ("production_constraints", "commitments", "challenge_points",
                                                        "range_check_bits", "hint_wires")}}
    return rep, res


# ------------------------------------------------------------------------------------------ statement

def namespaces(prob: dict) -> tuple[str, str, str]:
    return RT.namespaces(prob)


def candidate_model(prob: dict, res: dict) -> str:
    _, _, cand_ns = namespaces(prob)
    ref = prob["reference"]
    model = res["model"]
    com = model.commitment
    meta = {"repo_id": "ratchet candidate for " + ref["repo"], "instantiation": ref["call"],
            "generator": f"{GENERATOR_NAME} v{GENERATOR_VERSION}", "repo_url": "(candidate file)", "commit": "-",
            "path": "Candidate.go", "template": CANDIDATE_NAME, "wrapper_id": ref["wrapper"]["id"],
            "rule": "ratchet-candidate", "call": ref["call"], "gnark_version": ref["compiler"]["gnark"],
            "go_version": prob["env"]["go"], "curve": res["result"]["curve"],
            "commitment": ("no commitment" if not com.get("commits") else
                           f"one commitment; challenge degree {com['degree']}, held at {com['points']} fixed "
                           "challenges"), "r1cs_sha256": res["r1cs_sha256"]}
    text = GE.emit_model(cand_ns, meta, model.r, model.inputs, model.native_outputs, model.group_wires(),
                         model.wire_names)
    return text.replace("# DET model: ", "# Ratchet candidate model: ", 1)


def side(ns: str, emulated: bool) -> str:
    """One side of the equivalence: some assignment of model ``ns`` with inputs x and outputs y (and z)."""
    q = ns + "."
    out = f"(∃ w : Fin {q}nWires → {q}F, {q}Constraints w ∧ {q}Inputs.map w = x ∧ {q}Outputs.map w = y"
    if emulated:
        out += f" ∧\n         {q}EmulatedOutputs.map (fun g => {q}emValue w g.1 g.2.1 % g.2.2) = z"
    return out + ")"


def statement_text(prob: dict, cand: dict) -> str:
    base, ref_ns, _ = namespaces(prob)
    ref, rec, io = prob["reference"], prob["record"], prob["reference"]["io"]
    rf = ref_ns + "."
    em = bool(io["groups"])
    binder = f"∀ (x y : List {rf}F) (z : List ℕ)," if em else f"∀ x y : List {rf}F,"
    outs = (f"{io['n_outputs']} output wire(s) ({len(io['groups'])} emulated element(s) compared by value modulo "
            "their modulus)") if em else f"{io['n_outputs']} output wire(s)"
    return "\n".join([
        f"import {E.model_module(ref_ns)}",
        f"import {E.model_module(base + '.Cand')}",
        "",
        f"namespace {base}",
        "",
        f"/-- Ratchet equivalence for {ref['repo']} `{ref['call']}` ({ref['path']} at {ref['commit'][:12]}).",
        f"Reference: the registry DET model `{ref_ns}` of package `{ref['registry_package_id']}`",
        f"(gnark {ref['compiler']['gnark']}, Go {prob['env']['go']}, R1CS sha256 `{ref['model']['r1cs_sha256']}`); its "
        f"DET is machine-checked ({prob['det']['method']}).",
        f"Record: {rec['nonlinear']} non-linear ({rec['total']} total) constraints of the model.",
        f"Candidate `Cand`: source sha256 `{cand['source_sha256']}`, compiled in the reference's wrapper",
        f"(model R1CS sha256 `{cand['r1cs_sha256']}`): {cand['nonlinear']} non-linear ({cand['total']} total).",
        f"Inputs (the same wrapper variables): {io['n_inputs']}; outputs: {outs}.",
        "For every input vector `x` and output vector `y`" + (" and emulated output values `z`" if em else "") +
        ": some assignment satisfies",
        "the reference constraints with these inputs and outputs iff some assignment satisfies the candidate",
        "constraints with these inputs and outputs. -/",
        f"theorem equiv [Fact (Nat.Prime {rf}p)] :",
        f"    {binder}",
        f"      {side(ref_ns, em)} ↔",
        f"      {side('Cand', em)} := by",
        "  sorry",
        "",
        f"end {base}",
        "",
    ])


def statement_assumptions(prob: dict, cand: dict) -> list[str]:
    curve = prob["reference"]["compiler"]["curve"]
    out = [f"[Fact (Nat.Prime p)]: primality of the {curve} scalar field order, supplied as an instance hypothesis so "
           "that field lemmas apply; p is prime, so the hypothesis does not weaken the statement.",
           "Range checks in both models are gnark's non-commitment range checker (bit decomposition: the predicate "
           "value < 2^bits).",
           "Hint outputs are free wires of both models (unconstrained prover inputs)."]
    if prob["record"]["commitments"] or cand.get("commitments"):
        out.append("A commitment (Fiat-Shamir challenge) is modelled soundly: its challenge-dependent checks are "
                   "polynomial identities held at D + 1 distinct fixed challenges (equivalent to holding for every "
                   "challenge).")
    if prob["reference"]["io"]["groups"]:
        out.append("Emulated outputs are compared by value: the integer of the little-endian limbs modulo the "
                   "emulated modulus.")
    return out


def checker_files(prob: dict, stmt: str, cmodel: str) -> list[dict]:
    _, ref_ns, cand_ns = namespaces(prob)
    m = prob["reference"]["model"]
    return [{"path": "Statement.lean", "role": "statement", "sha256": sha(stmt.encode())},
            {"path": m["file"], "role": "import", "module": E.model_module(ref_ns), "sha256": m["sha256"]},
            {"path": E.model_relpath(cand_ns), "role": "import", "module": E.model_module(cand_ns),
             "sha256": sha(cmodel.encode())}]


def candidate_problem(prob: dict, cand: dict, files: list[dict], stmt: str, type_sha: str) -> dict:
    """The checker package record (status CANDIDATE)."""
    from zk_registry import gates as G
    base, ref_ns, cand_ns = namespaces(prob)
    fqn = base + ".equiv"
    out = {k: v for k, v in prob.items() if k not in ("annotations", "_dir")}
    out.update(status="CANDIDATE", candidate=cand, created_utc=now(),
               statement={"file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn, "text": stmt,
                          "imports": [E.model_module(ref_ns), E.model_module(cand_ns)],
                          "assumptions": statement_assumptions(prob, cand)},
               checker={"statement_file": "Statement.lean", "theorem": "equiv", "theorem_fqn": fqn,
                        "lean_opts": prob["reference"]["lean_opts"], "files": files,
                        "reference_type_sha256": type_sha, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                        "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS})
    return out


def write_candidate_package(prob: dict, problem_dir: str, cand: dict, res: dict, pkg: str) -> tuple[str, list[dict]]:
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


def run_candidate(problem_dir: str, cand_path: str, out: str, tools: str, snapshot: str | None = None,
                  env: L.LeanEnv | None = None, require_smaller: bool = False, gocache: str | None = None) -> dict:
    """The trusted candidate pipeline: admissibility, build, compile, metric, statement and models, and with a Lean
    environment the statement's elaboration and the checker package (``<out>/pkg``)."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, res = compile_candidate(prob, cand_path, out, snapshot, tools, gocache)
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
        el = RT.elaborate(env, pkg, [f["path"] for f in files[1:]], base + ".equiv", prob["reference"]["lean_opts"],
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


def run_check(problem_dir: str, cand_path: str, solution: str, out: str, tools: str, env: L.LeanEnv,
              snapshot: str | None = None, timeout: float = DET_PROOF_LIMITS["timeout_s"],
              gocache: str | None = None) -> tuple[str, dict]:
    """Final check: the candidate pipeline (strictly fewer non-linear constraints) and the production checker."""
    rep: dict = {"problem_dir": os.path.abspath(problem_dir), "solution_file": os.path.abspath(solution),
                 "started_utc": now()}
    try:
        cr = run_candidate(problem_dir, cand_path, os.path.join(out, "candidate"), tools, snapshot, env,
                           require_smaller=True, gocache=gocache)
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

def read_sim(path: str) -> tuple[dict, list[dict]]:
    with open(path, encoding="utf-8") as f:
        rows = [json.loads(ln) for ln in f if ln.strip()]
    if not rows:
        raise RuntimeError("the simulation runner wrote nothing")
    return rows[0], rows[1:]


def merge_witness(hdr: dict, ws: list[list[str]]) -> list[int]:
    """The model witness of one vector from the solver witnesses of every compile (``gnark_r1cs.build``)."""
    vals = [[int(v) for v in x] for x in ws]
    if not hdr.get("commits"):
        return vals[0]
    b = hdr["boundary_wires"]
    if any(x[:b] != vals[0][:b] for x in vals[1:]):
        raise ValueError("solver witnesses differ before the commitment")
    out = list(vals[0][:b])
    for x in vals:
        out.extend(x[b:])
    return out


def output_view(hdr: dict, outs: list[str]) -> tuple[list[int], list[int]]:
    """(native output values, emulated output values modulo their moduli) in the statement's order."""
    vals = [int(v) for v in outs]
    grouped = set()
    em = []
    for g in hdr["groups"]:
        idx = list(range(g["start"], g["start"] + g["len"]))
        grouped |= set(idx)
        em.append(GR.em_value(vals, idx, int(g["bits"])) % int(g["modulus"]))
    return [v for k, v in enumerate(vals) if k not in grouped], em


def free_outputs(model: GR.Model) -> list[int]:
    """Outputs fixed only by a free wire: an output whose constraints contain a non-input wire that occurs in no other
    constraint (a hint the constraints do not pin down)."""
    r = model.r
    occ: dict[int, set[int]] = collections.defaultdict(set)
    for j, c in enumerate(r.constraints):
        for lc in c:
            for w, k in lc:
                if k:
                    occ[w].add(j)
    inputs = set(model.inputs)
    out = []
    for o in model.outputs:
        cs = occ.get(o, set())
        if not cs:
            out.append(o)
            continue
        others = {w for j in cs for lc in r.constraints[j] for w, k in lc if k and w not in (0, o) and w not in inputs}
        if any(occ[w] <= cs for w in others):
            out.append(o)
    return out


def compare_runs(ref_hdr: dict, ref: list[dict], cand_hdr: dict, cand: list[dict], ref_model: GR.Model,
                 cand_model: GR.Model) -> dict:
    """The screen's verdict: per vector the candidate must be solvable exactly where the reference is and give the
    same outputs (emulated by value modulo the modulus); witnesses written by the runner are re-checked against the
    models; an output fixed only by a free wire is a mismatch."""
    if [x["label"] for x in ref] != [x["label"] for x in cand]:
        raise RuntimeError("the two runs used different vectors")
    bad_inputs = [x["error"] for x in ref if not x["ok"] and str(x.get("error", "")).startswith("input:")]
    if bad_inputs:
        raise RuntimeError(f"simulation inputs do not fit the wrapper: {bad_inputs[0][:200]}")
    mism, kinds = [], collections.Counter()
    ref_bad = rechecked = 0
    for a, b in zip(ref, cand):
        kind = None
        if a["ok"] and a.get("witnesses"):
            if not R.satisfies(ref_model.r, merge_witness(ref_hdr, a["witnesses"])):
                ref_bad += 1
        if b["ok"] and b.get("witnesses"):
            rechecked += 1
            if not R.satisfies(cand_model.r, merge_witness(cand_hdr, b["witnesses"])):
                kind = "candidate witness violates the candidate model"
        if kind is None and not a["ok"] and b["ok"]:
            kind = "reference rejects, candidate accepts"
        elif kind is None and a["ok"] and not b["ok"]:
            kind = "reference accepts, candidate rejects"
        elif kind is None and a["ok"] and output_view(ref_hdr, a["outputs"]) != output_view(cand_hdr, b["outputs"]):
            kind = "different outputs"
        if kind:
            kinds[kind] += 1
            if len(mism) < 20:
                mism.append({"vector": a["label"], "kind": kind, "reference": describe(ref_hdr, a),
                             "candidate": describe(cand_hdr, b)})
    free = free_outputs(cand_model)
    if free:
        kinds["structural: output fixed only by a free wire"] += len(free)
    return {"vectors": len(ref), "edge_cases": sum(1 for x in ref if x["label"].startswith("edge")),
            "reference_accepts": sum(1 for x in ref if x["ok"]),
            "reference_rejects": sum(1 for x in ref if not x["ok"]),
            "reference_recheck_failures": ref_bad, "witnesses_rechecked": rechecked,
            "mismatches": sum(kinds.values()), "mismatch_kinds": dict(kinds), "examples": mism,
            "unconstrained_outputs": free[:20], "verdict": "PASS" if not kinds else "MISMATCH"}


def describe(hdr: dict, x: dict) -> str:
    if not x["ok"]:
        return f"rejected ({str(x.get('error'))[:80]})"
    nat, em = output_view(hdr, x["outputs"])
    vals = [str(v) for v in nat + em]
    return "outputs [" + ", ".join(vals[:6]) + (f", ... ({len(vals)} values)" if len(vals) > 6 else "") + "]"


def det_screen(model_text: str) -> dict:
    """Informational: battery P3 propagation on the candidate's model (DETERMINED, or STUCK: a possible
    under-constrained hint)."""
    from zk_registry import mech_r1cs as M
    try:
        pr = M.propagate(M.parse_model(model_text, "theorem det :\n"))
        return {"propagation": pr.status, "undetermined_outputs": len(pr.undetermined_outputs)}
    except Exception as ex:                                  # noqa: BLE001 - informational only
        return {"propagation": "ERROR", "error": str(ex)[:200]}


def run_simulate(binary: str, prob: dict, n_random: int, work: str, tag: str) -> tuple[dict, list[dict]]:
    out = os.path.join(work, tag + ".sim.jsonl")
    r = run_tool(binary, ["simulate", prob["reference"]["wrapper"]["id"], out, "-limit", str(COMPILE_LIMIT), "-n",
                          str(n_random), "-witnesses", str(SIM_WITNESSES), "-seed", prob["simulate"]["seed"],
                          "-max-points", str(MAX_POINTS)], work)
    if r.rc != 0 or not os.path.isfile(out):
        raise RuntimeError(f"simulation runner failed ({tag}): {r.out[-300:]}")
    hdr, rows = read_sim(out)
    if hdr["status"] != "ok":
        raise RuntimeError(f"simulation compile failed ({tag}): {hdr.get('error')}")
    return hdr, rows


def simulate(problem_dir: str, cand_path: str, out: str, n_random: int, tools: str, snapshot: str | None = None,
             gocache: str | None = None) -> dict:
    """The simulation screen (not a proof) on a candidate."""
    prob = load_problem(problem_dir)
    snapshot = resolve_snapshot(problem_dir, prob, snapshot)
    rep, res = compile_candidate(prob, cand_path, os.path.join(out, "count"), snapshot, tools, gocache)
    refc = compile_program(prob, snapshot, tools, os.path.join(out, "reference"), gocache=gocache,
                           size_policy=reference_policy(prob["reference"]["model"]["n_constraints"]))
    if "error" in refc or refc["r1cs_sha256"] != prob["reference"]["model"]["r1cs_sha256"]:
        raise RuntimeError(f"the reference does not rebuild to its model: {refc.get('error', 'R1CS digest differs')}")
    t0 = time.time()
    run = os.path.join(out, "run")
    os.makedirs(run, exist_ok=True)
    ra, a = run_simulate(refc["binary"], prob, n_random, run, "ref")
    rb, b = run_simulate(res["binary"], prob, n_random, run, "cand")
    sim = compare_runs(ra, a, rb, b, refc["model"], res["model"])
    sim.update(random=n_random, seed=prob["simulate"]["seed"], secs=round(time.time() - t0, 2),
               det_screen=det_screen(candidate_model(prob, res)))
    rep["simulate"] = sim
    rep["finished_utc"] = now()
    P.write_json(os.path.join(out, "simulate.json"), rep)
    return rep


# ------------------------------------------------------------------------------------------ problem build

def problem_record(spec: dict, layout: dict, meas: dict, det: dict, snapshot: dict, wrapper: dict, pins: dict,
                   annotations: dict | None = None) -> dict:
    pid = "ratchet/" + spec["registry_package_id"]
    name = target_name(spec)
    kind, _ = call_site(spec["wrapper"], name)
    recv = spec["symbol"].split(".")[0] if kind in ("method", "circuit") and "." in spec["symbol"] else (
        spec["symbol"] if kind == "circuit" else None)
    env = {k: spec["env"][k] for k in ("lean", "mathlib", "lake_manifest_sha256", "packages") if k in spec["env"]}
    env.update(go=pins["go"], go_sha256=pins["go_sha256"], go_compile=pins["go_compile"],
               go_compile_sha256=pins["go_compile_sha256"], gnark=spec["gnark_version"] or "")
    rec = {
        "schema_version": P.RATCHET_GNARK_SCHEMA_VERSION, "kind": "ratchet", "family": "gnark", "package_id": pid,
        "status": "OPEN",
        "reference": {
            "registry_package_id": spec["registry_package_id"], "repo": spec["repo"], "repo_url": spec["repo_url"],
            "commit": spec["commit"], "path": spec["path"], "symbol": spec["symbol"],
            "symbol_line": spec["symbol_line"], "call": spec["call"], "rule": spec["rule"], "kind": kind,
            "name": name, "receiver_type": recv, **layout, "wrapper": wrapper,
            "compiler": {"gnark": spec["gnark_version"] or "", "go": pins["go"], "curve": spec["curve"],
                         "prime": spec["prime"],
                         "command": "go build -trimpath -overlay <harness, wrapper[, Candidate.go]> ./" + TOOL_SUBDIR
                                    + " (GOFLAGS=-mod=vendor, GOPROXY=off, CGO_ENABLED=0); gnarkx run <wrapper id> "
                                    f"-limit {COMPILE_LIMIT} -samples 0"},
            "tool": {"files": [f"gnark_tool/{f}" for f in TOOL_FILES], "sha256": tool_sha256()},
            "model": dict(spec["model"], r1cs_sha256=meas["r1cs_sha256"], n_constraints=meas["model"].r.n_constraints,
                          n_wires=meas["model"].r.n_wires, counts=meas["counts"]),
            "io": meas["io"], "lean_opts": spec["lean_opts"],
            **({"decomposition": spec["decomposition"]} if spec.get("decomposition") else {})},
        "record": dict(meas["counts"], **meas["facts"], rung=0,
                       source="reference (registry DET model: gnark R1CS builder, non-commitment range checks)"),
        "metric": METRIC, "admissibility": list(ADMISSIBILITY), "statement_shape": STATEMENT_DESCRIPTION,
        "determinism": DETERMINISM_NOTE, "det": det,
        "simulate": {"seed": f"{SIM_SEED}|{pid}", "runner": "gnarkx simulate (gnark's solver on every compile of the "
                     "model)", "profiles": list(SIM_PROFILES), "witnesses_rechecked": SIM_WITNESSES},
        "snapshot": snapshot, "env": env, "generator": generator_info(), "created_utc": now(),
    }
    if annotations:
        rec["annotations"] = annotations
    return rec


def build_problem(reg_dir: str, snapshot: str, out_dir: str, tools: str, det: dict | None, module_dir: str,
                  snapshot_id: str, work: str, annotations: dict | None = None, gocache: str | None = None) -> dict:
    """Reference package -> ratchet problem package: ``problem.json``, the registry model file byte for byte and the
    registry wrapper text (``reference/wrapper.go``), after a rebuild of the reference from the snapshot with the
    pinned toolchain gave the registry R1CS; raises NotEligible."""
    spec = reference_spec(reg_dir)
    why = static_exclusion(spec) or RT.det_exclusion(det, spec["model"]["sha256"])
    if why:
        raise NotEligible(*why)
    meas = measure_reference(reg_dir, spec)
    if meas["counts"]["nonlinear"] == 0:
        raise NotEligible("linear-record", "the record has no non-linear constraint (every constraint of the model is "
                                           "linear, free in context; no candidate can count)")
    man = RT.verify_snapshot(snapshot)
    layout = package_layout(snapshot, module_dir, spec["path"])
    goroot = os.path.join(tools, f"go{str(spec['env'].get('go', '')).split(' ')[0]}")
    if not os.path.isdir(goroot):
        raise NotEligible("toolchain", f"no Go toolchain for the registry's Go {spec['env'].get('go')}")
    pins = go_pins(goroot)
    wrapper = {"file": "reference/wrapper.go", "sha256": sha(spec["wrapper"].encode()), "id": spec["wrapper_id"],
               "note": "the registry's wrapper circuit (evidence/wrapper.go without its header line)"}
    snap = {"id": snapshot_id, "manifest_sha256": P.sha256_file(os.path.join(snapshot, "MANIFEST.sha256")),
            "n_files": len(man), "rule": SNAPSHOT_RULE}
    stage = os.path.join(work, "stage")
    fresh_dir(stage)
    put(os.path.join(stage, "reference", "wrapper.go"), spec["wrapper"])
    pre = {"reference": {**layout, "wrapper": wrapper, "name": target_name(spec)}, "env": pins, "_dir": stage}
    res = compile_program(pre, snapshot, tools, os.path.join(work, "rebuild"),
                          size_policy=reference_policy(spec["size_policy"]), gocache=gocache)
    if "error" in res:
        raise NotEligible("rebuild", f"the reference does not rebuild: {res['error'][:300]}")
    if res["r1cs_sha256"] != meas["r1cs_sha256"]:
        raise NotEligible("rebuild", "the rebuilt reference R1CS differs from the registry's")
    if res["io"] != meas["io"]:
        raise NotEligible("rebuild", "the rebuilt reference I/O differs from the registry's")
    prob = problem_record(spec, layout, meas, det, snap, wrapper, pins, annotations)
    fresh_dir(out_dir)
    dst = os.path.join(out_dir, spec["model"]["file"])
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(os.path.join(reg_dir, spec["model"]["file"]), dst)
    put(os.path.join(out_dir, "reference", "wrapper.go"), spec["wrapper"])
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
        x.add_argument("--gocache")
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
        verdict, rep = run_check(a.problem, a.candidate, a.solution, a.out, a.tools, env, a.snapshot, gocache=a.gocache)
        rep["finished_utc"] = now()
        P.write_json(os.path.join(a.out, "verdict.json"), rep)
        print(f"{verdict} {rep.get('reason', '')}".rstrip())
        for k in ("error", "invalid", "fail"):
            for x in (rep.get("checker") or {}).get(k, []):
                print(f"  {k}: {x}")
        return EXIT[verdict]
    try:
        if a.cmd == "simulate":
            rep = simulate(a.problem, a.candidate, a.out, a.n, a.tools, a.snapshot, a.gocache)
        elif a.cmd == "count":
            prob = load_problem(a.problem)
            rep, _ = compile_candidate(prob, a.candidate, a.out, resolve_snapshot(a.problem, prob, a.snapshot),
                                       a.tools, a.gocache)
            P.write_json(os.path.join(a.out, "candidate.json"), rep)
        else:
            rep = run_candidate(a.problem, a.candidate, a.out, a.tools, a.snapshot, env, gocache=a.gocache)
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
