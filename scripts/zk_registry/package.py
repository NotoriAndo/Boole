"""Problem package identities, ``problem.json`` validation and file layout.

Package directory (statuses OPEN, GATE-FAIL, DET-FALSE-CANDIDATE)::

    <package dir>/problem.json
    <package dir>/Statement.lean                  theorem with `sorry`
    <package dir>/ZkDet/<Ident>/Model.lean        generated model module
    <package dir>/evidence/...                    gate evidence (not part of the problem)

TOO-LARGE and UNINSTANTIABLE records have no package directory; they live in the wave index.
"""
from __future__ import annotations

import hashlib
import json
import os
import re

from . import GENERATOR_NAME, GENERATOR_VERSION
from . import jsonschema_lite
from . import lean_emit as E

SCHEMA_VERSION = "zk-registry-problem/v1"
SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema", "problem.schema.json")
PACKAGED_STATUSES = ("OPEN", "GATE-FAIL", "DET-FALSE-CANDIDATE")
STATUSES = ("OPEN", "TOO-LARGE", "UNINSTANTIABLE", "GATE-FAIL", "DET-FALSE-CANDIDATE")
MAX_CONSTRAINTS = 2000

DET_PROPERTY = {"template": "DET", "name": "output determinism (no under-constraint)"}
DET_SPEC_CLAUSES = [
    "Output determinism: for the compiled constraint system of the instantiated main component, any two "
    "wire assignments that satisfy every constraint (including the constant-one wire) and agree on every "
    "input wire of the main component agree on every output wire of the main component.",
    "The field is the circom prime of the compilation; inputs and outputs are the main component's "
    "`signal input` and `signal output` declarations, located by the R1CS header and cross-checked "
    "against the .sym names.",
]
DET_SPEC = {
    "tier": "T1",
    "source": "mathematical definition of output determinism (functional dependence of outputs on inputs)",
    "clauses": DET_SPEC_CLAUSES,
    "sha256": hashlib.sha256("\n".join(DET_SPEC_CLAUSES).encode("utf-8")).hexdigest(),
}
PRIME_DESCRIPTIONS = {
    "bn128": "the published BN254 scalar field order",
    "bls12381": "the published BLS12-381 scalar field order",
    "goldilocks": "the Goldilocks prime 2^64 - 2^32 + 1",
}


def statement_assumptions(prime_name: str = "bn128") -> list[str]:
    """The recorded statement assumptions for a circom prime (``circom --prime`` name)."""
    what = PRIME_DESCRIPTIONS.get(prime_name, "the compilation prime recorded in circuit.prime")
    return ["[Fact (Nat.Prime p)]: primality of the circom prime, supplied as an instance hypothesis so that field "
            f"lemmas apply; p is {what} (prime), so the hypothesis does not weaken the statement."]


STATEMENT_ASSUMPTIONS = statement_assumptions("bn128")

GENERATOR_SOURCES = ["__init__.py", "r1cs.py", "circom_source.py", "instantiation.py", "lean_emit.py", "witness.py",
                     "witness_runner.js", "det_search.py", "package.py", "jsonschema_lite.py", "lean_runner.py",
                     "gates.py", "check.py", "circom_det.py", "lean/ZkReplay.lean", "schema/problem.schema.json"]


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def generator_info() -> dict:
    here = os.path.dirname(os.path.abspath(__file__))
    h = hashlib.sha256()
    for rel in GENERATOR_SOURCES:
        h.update(rel.encode() + b"\0" + sha256_file(os.path.join(here, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


# ------------------------------------------------------------------------------------------ identities

def args_slug(args: tuple[str, ...] | list[str]) -> str:
    if not args:
        return ""
    raw = ",".join(args)
    slug = re.sub(r"[^A-Za-z0-9]+", "_", raw.replace("-", "m")).strip("_")
    if len(slug) > 40 or not re.fullmatch(r"[0-9_m]+", slug):
        slug = "h" + hashlib.sha256(raw.encode()).hexdigest()[:10]
    return slug


def package_dir_name(path: str, template: str, args, strip_prefix: str = "circuits/") -> str:
    rel = path[len(strip_prefix):] if path.startswith(strip_prefix) else path
    stem = re.sub(r"\.circom$", "", rel).replace("/", ".")
    parts = [stem, template]
    slug = args_slug(args)
    if slug:
        parts.append(slug)
    return re.sub(r"[^A-Za-z0-9._-]", "_", ".".join(parts))


def lean_namespace(collection: str, dir_name: str) -> str:
    return "ZkDet." + E.lean_ident(f"{collection}_{dir_name}")


# ------------------------------------------------------------------------------------------ validation

def load_schema() -> dict:
    with open(SCHEMA_PATH, encoding="utf-8") as f:
        return json.load(f)


def validate_problem(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """Schema errors plus semantic checks; with ``pkg_dir`` also file presence and hashes."""
    errs = jsonschema_lite.validate(problem, schema or load_schema())
    if errs:
        return errs
    status = problem["status"]
    if status in PACKAGED_STATUSES:
        chk, st = problem["checker"], problem["statement"]
        if chk["theorem_fqn"] != st["theorem_fqn"] or chk["theorem"] != st["theorem"]:
            errs.append("checker and statement disagree on the theorem")
        if not st["theorem_fqn"].endswith("." + st["theorem"]):
            errs.append("theorem_fqn does not end with the theorem name")
        roles = sorted(f["role"] for f in chk["files"])
        if roles.count("statement") != 1:
            errs.append("checker.files must list exactly one statement file")
        if problem["circuit"]["n_constraints"] > MAX_CONSTRAINTS or not problem["circuit"]["size_policy"]["within"]:
            errs.append("packaged status outside the size policy")
        if pkg_dir is not None:
            for fr in chk["files"]:
                p = os.path.join(pkg_dir, fr["path"])
                if not os.path.isfile(p):
                    errs.append(f"missing file {fr['path']}")
                elif sha256_file(p) != fr["sha256"]:
                    errs.append(f"sha256 mismatch for {fr['path']}")
            sp = os.path.join(pkg_dir, st["file"])
            if os.path.isfile(sp):
                with open(sp, encoding="utf-8") as f:
                    if f.read() != st["text"]:
                        errs.append("statement.text differs from Statement.lean")
    else:
        for key in ("statement", "checker", "gates"):
            if key in problem:
                errs.append(f"{status} record must not carry {key!r}")
        if status == "TOO-LARGE" and problem["circuit"]["n_constraints"] <= problem["circuit"]["size_policy"]["max_constraints"]:
            errs.append("TOO-LARGE record within the size policy")
    return errs


def write_json(path: str, obj) -> None:
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=1, sort_keys=True, ensure_ascii=False)
        f.write("\n")
