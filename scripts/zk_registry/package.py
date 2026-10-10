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
AIR_SCHEMA_VERSION = "zk-registry-air-problem/v1"
AIR_SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema", "air_problem.schema.json")
RATCHET_SCHEMA_VERSION = "zk-registry-ratchet-problem/v1"
RATCHET_SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema", "ratchet_problem.schema.json")
RATCHET_NOIR_SCHEMA_VERSION = "zk-registry-ratchet-noir-problem/v1"
RATCHET_NOIR_SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema",
                                        "ratchet_noir_problem.schema.json")
RATCHET_GNARK_SCHEMA_VERSION = "zk-registry-ratchet-gnark-problem/v1"
RATCHET_GNARK_SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema",
                                         "ratchet_gnark_problem.schema.json")
RATCHET_AIR_SCHEMA_VERSION = "zk-registry-ratchet-air-problem/v1"
RATCHET_AIR_SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema",
                                       "ratchet_air_problem.schema.json")
RATCHET_ZOKRATES_SCHEMA_VERSION = "zk-registry-ratchet-zokrates-problem/v1"
RATCHET_ZOKRATES_SCHEMA_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "schema", "ratchet_zokrates_problem.schema.json"
)
RATCHET_HALO2_SCHEMA_VERSION = "zk-registry-ratchet-halo2-problem/v1"
RATCHET_HALO2_SCHEMA_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "schema", "ratchet_halo2_problem.schema.json"
)
SPEC_SCHEMA_VERSION = "zk-registry-spec-problem/v1"
SPEC_SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema", "spec_problem.schema.json")
CORRECTNESS_GNARK_SCHEMA_VERSION = "zk-registry-correctness-gnark-problem/v1"
CORRECTNESS_GNARK_SCHEMA_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "schema", "correctness_gnark_problem.schema.json"
)
PACKAGED_STATUSES = ("OPEN", "GATE-FAIL", "DET-FALSE-CANDIDATE")
STATUSES = ("OPEN", "TOO-LARGE", "UNINSTANTIABLE", "GATE-FAIL", "DET-FALSE-CANDIDATE")
MAX_CONSTRAINTS = 2000
# constraint caps a circom-schema record may name in ``circuit.size_policy``: the default policy and the cap chosen
# from the measured size ladder of recovery R1 (Circom; other families keep MAX_CONSTRAINTS)
R1_MAX_CONSTRAINTS = 4000
SIZE_POLICIES = (MAX_CONSTRAINTS, R1_MAX_CONSTRAINTS)

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

GENERATOR_SOURCES = ["__init__.py", "r1cs.py", "circom_source.py", "circom_eval.py", "instantiation.py", "lean_emit.py",
                     "witness.py", "witness_runner.js", "det_search.py", "tags.py", "content.py", "decompose.py",
                     "package.py", "jsonschema_lite.py", "lean_runner.py", "gates.py", "check.py", "circom_det.py",
                     "circom_detmod.py", "lean/ZkReplay.lean", "schema/problem.schema.json"]


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

def load_schema(version: str = SCHEMA_VERSION) -> dict:
    path = {
        AIR_SCHEMA_VERSION: AIR_SCHEMA_PATH,
        RATCHET_SCHEMA_VERSION: RATCHET_SCHEMA_PATH,
        RATCHET_NOIR_SCHEMA_VERSION: RATCHET_NOIR_SCHEMA_PATH,
        RATCHET_GNARK_SCHEMA_VERSION: RATCHET_GNARK_SCHEMA_PATH,
        RATCHET_AIR_SCHEMA_VERSION: RATCHET_AIR_SCHEMA_PATH,
        RATCHET_ZOKRATES_SCHEMA_VERSION: RATCHET_ZOKRATES_SCHEMA_PATH,
        RATCHET_HALO2_SCHEMA_VERSION: RATCHET_HALO2_SCHEMA_PATH,
        SPEC_SCHEMA_VERSION: SPEC_SCHEMA_PATH,
        CORRECTNESS_GNARK_SCHEMA_VERSION: CORRECTNESS_GNARK_SCHEMA_PATH,
    }.get(version, SCHEMA_PATH)
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def has_statement(problem: dict) -> bool:
    """Whether the package carries a Lean statement for the checker: a packaged DET-family status, or a ratchet
    checker package (status CANDIDATE, generated by the ratchet pipeline for one candidate)."""
    if problem.get("schema_version") == SPEC_SCHEMA_VERSION:
        return False
    if problem.get("schema_version") == CORRECTNESS_GNARK_SCHEMA_VERSION:
        return problem.get("status") in ("OPEN", "FACT")
    if problem.get("schema_version") in (
        RATCHET_SCHEMA_VERSION,
        RATCHET_NOIR_SCHEMA_VERSION,
        RATCHET_GNARK_SCHEMA_VERSION,
        RATCHET_AIR_SCHEMA_VERSION,
        RATCHET_ZOKRATES_SCHEMA_VERSION,
        RATCHET_HALO2_SCHEMA_VERSION,
    ):
        return problem.get("status") == "CANDIDATE"
    return problem.get("status") in PACKAGED_STATUSES


def validate_problem(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """Schema errors plus semantic checks; with ``pkg_dir`` also file presence and hashes.  The schema follows
    ``schema_version`` (circom packages, AIR packages or ratchet problems)."""
    if problem.get("schema_version") == SPEC_SCHEMA_VERSION:
        return validate_spec(problem, pkg_dir, schema)
    if problem.get("schema_version") == CORRECTNESS_GNARK_SCHEMA_VERSION:
        return validate_correctness_gnark(problem, pkg_dir, schema)
    if problem.get("schema_version") == RATCHET_SCHEMA_VERSION:
        return validate_ratchet(problem, pkg_dir, schema)
    if problem.get("schema_version") == RATCHET_NOIR_SCHEMA_VERSION:
        return validate_ratchet_noir(problem, pkg_dir, schema)
    if problem.get("schema_version") == RATCHET_GNARK_SCHEMA_VERSION:
        return validate_ratchet_gnark(problem, pkg_dir, schema)
    if problem.get("schema_version") == RATCHET_AIR_SCHEMA_VERSION:
        return validate_ratchet_air(problem, pkg_dir, schema)
    if problem.get("schema_version") in (RATCHET_ZOKRATES_SCHEMA_VERSION, RATCHET_HALO2_SCHEMA_VERSION):
        return validate_ratchet_native(problem, pkg_dir, schema)
    air = problem.get("schema_version") == AIR_SCHEMA_VERSION
    errs = jsonschema_lite.validate(problem, schema or load_schema(problem.get("schema_version", SCHEMA_VERSION)))
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
        size = problem["air"] if air else problem["circuit"]
        # AIR and Noir (ACIR) records carry their generator's size policy; circom records may name the default or the
        # recovery-R1 policy, gnark records the default or the decomposition policy of wave RT-G2 (the gnark ratchet's
        # 4,000-constraint model cap) (SIZE_POLICIES); the other families use the global one
        if air or "acir" in size:
            limit = size["size_policy"]["max_constraints"]
        elif size.get("compiler", {}).get("name") in ("circom", "gnark"):
            limit = size["size_policy"]["max_constraints"]
            if limit not in SIZE_POLICIES:
                errs.append(f"unknown size policy of {limit} constraints")
                limit = MAX_CONSTRAINTS
        else:
            limit = MAX_CONSTRAINTS
        if size["n_constraints"] > limit or not size["size_policy"]["within"]:
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
        if status == "TOO-LARGE" and not air and \
                problem["circuit"]["n_constraints"] <= problem["circuit"]["size_policy"]["max_constraints"]:
            errs.append("TOO-LARGE record within the size policy")
        if status == "TOO-LARGE" and air and problem["air"]["size_policy"]["within"]:
            errs.append("TOO-LARGE record within the size policy")
    return errs


def _file_errors(pkg_dir: str, files: list[dict]) -> list[str]:
    errs = []
    for fr in files:
        p = os.path.join(pkg_dir, fr["path"])
        if not os.path.isfile(p):
            errs.append(f"missing file {fr['path']}")
        elif sha256_file(p) != fr["sha256"]:
            errs.append(f"sha256 mismatch for {fr['path']}")
    return errs


def _bound_file_errors(files: list[dict], pkg_dir: str | None) -> list[str]:
    """Reject escaped, duplicate, symlinked or changed files in an S/C manifest."""
    from pathlib import PurePosixPath
    errs, seen = [], set()
    for record in files:
        rel = record['path']
        parts = PurePosixPath(rel).parts
        if rel.startswith('/') or '\\' in rel or '..' in parts or not parts or rel != '/'.join(parts):
            errs.append('unsafe package path: ' + rel)
            continue
        if rel in seen:
            errs.append('duplicate package path: ' + rel)
        seen.add(rel)
        if pkg_dir is not None:
            path = os.path.join(pkg_dir, rel)
            root = os.path.realpath(pkg_dir)
            real = os.path.realpath(path)
            if not real.startswith(root + os.sep) or os.path.islink(path):
                errs.append('package file escapes or is a symlink: ' + rel)
            elif not os.path.isfile(path):
                errs.append('missing file ' + rel)
            elif sha256_file(path) != record['sha256']:
                errs.append('sha256 mismatch for ' + rel)
    return errs


def validate_spec(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """Validate S request identity and local acceptance without claiming a public challenge."""
    from . import spec_problem as S
    errs = jsonschema_lite.validate(problem, schema or load_schema(SPEC_SCHEMA_VERSION))
    if errs:
        return errs
    if problem['package_id'] != S.request_identity(problem):
        errs.append('S request identity differs from its bound request inputs')
    errs += _bound_file_errors(problem['files'], pkg_dir)
    accepted = problem.get('specification')
    if problem['status'] == 'OPEN' and accepted is not None:
        errs.append('OPEN S must not carry an accepted specification')
    if problem['status'] == 'ACCEPTED-LOCAL':
        if accepted is None:
            return errs + ['ACCEPTED-LOCAL requires a specification and D/M/C evidence']
        records = {row['path']: row['sha256'] for row in problem['files']}
        if records.get(accepted['file']) != accepted['sha256']:
            errs.append('accepted specification is not bound in the file manifest')
        for gate in accepted['gate_evidence'].values():
            if records.get(gate['file']) != gate['sha256']:
                errs.append('specification gate evidence is not bound in the file manifest')
        if pkg_dir is not None and not errs:
            errs += S.historical_binding_errors(problem)
        if pkg_dir is not None and not errs:
            errs += _spec_acceptance_errors(problem, pkg_dir)
    return errs


def _spec_acceptance_errors(problem: dict, pkg_dir: str) -> list[str]:
    """Validate retained Task M D/M/C acceptance records, without rerunning or inventing gates."""
    from pathlib import Path
    from datetime import datetime, timezone
    from . import spec_problem as S
    if problem['protocol']['version'] != 'Task M D/M/C frozen':
        return ['unsupported local specification acceptance protocol']
    missing = S.authoring_prerequisites(problem)
    if missing:
        return ['accepted specification lacks request bindings: ' + ', '.join(missing)]
    root, errs = Path(pkg_dir), []
    try:
        for key in ('D', 'M'):
            gate = problem['specification']['gate_evidence'][key]
            record = json.loads((root / gate['file']).read_text())
            if not isinstance(record, dict) or record.get('status') != 'PASS':
                errs.append('specification ' + key + ' evidence is not a passing object')
                continue
            entries = record.get('entries')
            if not isinstance(entries, list) or not entries:
                errs.append('specification ' + key + ' evidence lacks evaluated entries')
                continue
            for entry in entries:
                if (not isinstance(entry, dict) or entry.get('D') != 'PASS'
                        or type(entry.get('seeded_rows')) is not int or entry['seeded_rows'] < 10000
                        or type(entry.get('M_killed')) is not int
                        or entry['M_killed'] != problem['mutants']['count']
                        or not isinstance(entry.get('unit'), str) or not re.fullmatch(r'M[0-9]{2}', entry['unit'])
                        or not isinstance(entry.get('evaluation'), str) or not entry['evaluation'].endswith('.json')
                        or not isinstance(entry.get('seed_sha256'), str)
                        or not re.fullmatch(r'[0-9a-f]{64}', entry['seed_sha256'])
                        or not isinstance(entry.get('mutant_disagreements'), list)
                        or len(entry['mutant_disagreements']) != problem['mutants']['count']
                        or any(not isinstance(mutant, dict) or type(mutant.get('index')) is not int
                               or not isinstance(mutant.get('input'), dict)
                               or not isinstance(mutant.get('Lean'), list)
                               or not isinstance(mutant.get('mutant'), list)
                               or mutant['Lean'] == mutant['mutant']
                               for mutant in entry['mutant_disagreements'])):
                    errs.append('specification D/M entry does not satisfy the frozen gate counts')
        gate = problem['specification']['gate_evidence']['C']
        challenger = json.loads((root / gate['file']).read_text())
        if (not isinstance(challenger, dict) or challenger.get('status') != 'NO_COUNTEREXAMPLE'
                or not isinstance(challenger.get('agent'), str) or not challenger['agent']
                or not isinstance(challenger.get('start_utc'), str) or not challenger['start_utc'].endswith('Z')
                or not isinstance(challenger.get('end_utc'), str) or not challenger['end_utc'].endswith('Z')
                or type(challenger.get('input_count')) is not int or challenger['input_count'] < 1):
            errs.append('specification independent challenger evidence is incomplete or rejected')
        else:
            start = datetime.fromisoformat(challenger['start_utc'].replace('Z', '+00:00'))
            end = datetime.fromisoformat(challenger['end_utc'].replace('Z', '+00:00'))
            if start.tzinfo != timezone.utc or end.tzinfo != timezone.utc or start > end:
                errs.append('specification challenger timestamps are not ordered UTC')
    except (OSError, ValueError, KeyError, TypeError):
        errs.append('invalid specification acceptance evidence')
    return errs


def validate_correctness_gnark(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """Validate exact C source/spec/interface bindings and the completed issuance gates."""
    from . import check as C, correctness_gnark as CG, spec_problem as S
    errs = jsonschema_lite.validate(problem, schema or load_schema(CORRECTNESS_GNARK_SCHEMA_VERSION))
    if errs:
        return errs
    unit, kind = problem['binding'], problem['property']
    try:
        modules = CG.render_modules(unit)
    except (ValueError, KeyError, TypeError, IndexError) as ex:
        return [f'invalid correctness interface: {ex}']
    if problem['binding_sha256'] != S.package_digest(unit):
        errs.append('correctness binding digest mismatch')
    identity = S.package_digest(dict(binding=unit, spec=problem['spec'], property=kind))
    if problem['package_id'] != 'C/gnark/' + identity:
        errs.append('correctness package identity mismatch')
    if problem['files'] != problem['checker']['files']:
        errs.append('checker and package file manifests differ')
    errs += _bound_file_errors(problem['files'], pkg_dir)
    files = {row['path']: row for row in problem['files']}
    expected_imports = [unit['model_file'], 'Candidate.lean', 'Bindings.lean', 'Statements.lean']
    if [row['path'] for row in problem['files'] if row['role'] == 'import'] != expected_imports:
        errs.append('correctness import closure must be the original model, accepted spec, pure bindings and propositions')
    if [row['path'] for row in problem['files'] if row['role'] == 'statement'] != ['ProofStatement.lean']:
        errs.append('correctness checker must bind exactly its reference proof statement')
    if files.get(unit['model_file'], {}).get('sha256') != unit['model_sha256']:
        errs.append('original model digest differs from checker import')
    if files.get('Candidate.lean', {}).get('sha256') != problem['spec']['specification_sha256']:
        errs.append('accepted specification digest differs from checker import')
    for path in ('Bindings.lean', 'Statements.lean'):
        expected = hashlib.sha256(modules[path].encode()).hexdigest()
        if files.get(path, {}).get('sha256') != expected:
            errs.append('helper-free generated module binding differs: ' + path)
    template = CG.theorem_template(unit, kind)
    chk = problem['checker']
    name = 'Soundness' if kind == 'soundness' else 'Completeness'
    if problem['statement']['text'] != template or chk['theorem_fqn'] != 'MProof.' + unit['id'] + name + '.solution':
        errs.append('correctness reference theorem differs from generated proposition')
    if problem['statement']['theorem_fqn'] != chk['theorem_fqn']:
        errs.append('statement and checker theorem names differ')
    if files.get('ProofStatement.lean', {}).get('sha256') != hashlib.sha256(template.encode()).hexdigest():
        errs.append('correctness reference statement digest mismatch')
    if chk['lean_opts'] != CG.LEAN_OPTIONS or chk['allowed_axioms'] != sorted(C.ALLOWED_AXIOMS):
        errs.append('correctness checker options or allowed axioms differ from production policy')
    if chk['forbidden_tokens'] != C.FORBIDDEN_LABELS:
        errs.append('correctness forbidden constructs differ from production core')
    if problem['battery']['forms'] != CG.BATTERY_FORMS:
        errs.append('correctness battery differs from the frozen seven forms')
    status = CG.classify(problem['nonvacuity']['status'], problem['battery']['attempts'],
                         problem['battery']['checker_verdict'])
    if status != problem['status']:
        errs.append('correctness status is not justified by non-vacuity and accepted battery evidence')
    if problem['trivial'] != dict(structural=unit['reference_cost']['nonlinear'] < 3, battery=status == 'FACT'):
        errs.append('trivial flags disagree with original counts or battery classification')
    for key in ('nonvacuity', 'battery'):
        record = problem[key]
        if files.get(record['file'], {}).get('sha256') != record['sha256']:
            errs.append(key + ' evidence file is not pinned')
    nv = problem['nonvacuity']
    if files.get(nv['witness_file'], {}).get('sha256') != nv['witness_sha256']:
        errs.append('non-vacuity witness is not pinned')
    if pkg_dir is not None and not errs:
        errs += _correctness_content_errors(problem, pkg_dir)
    return errs


def _correctness_closure_errors(report: dict, checker: dict) -> list[str]:
    """Recompute FACT acceptance from the unchanged core, not from a supplied PASS label."""
    from . import check as C
    if not isinstance(report, dict):
        return ['FACT closure checker record is not an object']
    if any(report.get(key) != [] for key in ('invalid', 'fail', 'error')) or report.get('verdict') != 'PASS':
        return ['FACT closure checker contains rejected or missing verdict reasons']
    compiled = report.get('compile')
    if not isinstance(compiled, dict):
        return ['FACT closure compilation record is missing']
    post = compiled.get('post')
    if (compiled.get('compile_rc') != 0 or compiled.get('compile_timeout') is not False
            or compiled.get('n_errors') != 0 or compiled.get('sorry_warnings') != 0
            or compiled.get('errors') != [] or 'error' in compiled
            or not isinstance(compiled.get('printed_axioms'), list)
            or not isinstance(compiled.get('imports'), list) or not isinstance(post, dict)):
        return ['FACT closure compilation is incomplete or unsuccessful']
    if (post.get('rc') != 0 or post.get('timeout') is not False
            or not isinstance(post.get('target'), dict) or not isinstance(post.get('axioms'), list)
            or not isinstance(post.get('consts'), list)
            or any(not isinstance(row, dict) or row.get('rc') != 0 or row.get('errors') != 0
                   for row in compiled['imports'])
            or any(not isinstance(row, dict) or 'kind' not in row or 'unsafe' not in row
                   for row in post['consts'])):
        return ['FACT closure kernel replay or import records are incomplete']
    invalid, fail, error = [], [], []
    C.apply_compile_result(compiled, checker, invalid, fail, error)
    return invalid + fail + error


def _correctness_content_errors(problem: dict, pkg_dir: str) -> list[str]:
    """Recheck the embedded S record, original R1CS, concrete row and bound evaluation result."""
    from pathlib import Path
    from . import correctness_gnark as CG, gnark_r1cs as GR, r1cs as R, ratchet_gnark as RG, spec_problem as S
    from . import correctness_build as CB, lean_runner as L
    root, errs = Path(pkg_dir), []
    unit, nv = problem['binding'], problem['nonvacuity']
    try:
        linked = json.loads((root / problem['spec']['file']).read_text())
        if (validate_spec(linked, str(root / 'spec')) or linked['status'] != 'ACCEPTED-LOCAL'
                or S.package_digest(linked) != problem['spec']['sha256']
                or linked['package_id'] != problem['spec']['package_id']
                or linked['specification']['sha256'] != problem['spec']['specification_sha256']
                or linked['family'] != unit['reference_operation']):
            errs.append('embedded accepted S specification binding differs')
        original_path = root / 'reference/problem.json'
        original = json.loads(original_path.read_text())
        if sha256_file(str(original_path)) != unit['original_metadata_sha256']:
            errs.append('original metadata byte binding differs')
        if original['ids'] != unit['source'] or original['instantiation'] != unit['instantiation']:
            errs.append('source pin or original instantiation differs')
        if original['statement']['assumptions'] != unit['model_assumptions']:
            errs.append('original range/commitment assumptions differ')
        if original['circuit'].get('gnark', {}) != unit['gnark_commitment_model']:
            errs.append('original commitment model differs')
        if original['evidence'].get('hints', {}) != unit['hints']:
            errs.append('original untrusted hint binding differs')
        harness_path = root / 'reference/evidence/harness_result.json'
        if sha256_file(str(harness_path)) != unit['harness_sha256']:
            errs.append('original harness byte binding differs')
        if sha256_file(str(root / 'reference/evidence/wrapper.go')) != unit['wrapper_sha256']:
            errs.append('original wrapper byte binding differs')
        if sha256_file(str(root / 'reference/source.go')) != unit['source_fullfile_sha256']:
            errs.append('original source file byte binding differs')
        original_model_file = original['statement']['model_file']
        original_model = next(row for row in original['checker']['files'] if row['path'] == original_model_file)
        if (unit['model_file'] != original_model_file or unit['model_sha256'] != original_model['sha256']
                or unit['model_module'] != original['statement']['model_module']):
            errs.append('C model differs from the exact original metadata-pinned model')
        parameters = json.loads((root / 'reference/parameters.json').read_text())
        expected = CG.bind_registry(str(root / 'reference'), parameters, unit['id'], model_directory=str(root))
        expected['source_fullfile_sha256'] = sha256_file(str(root / 'reference/source.go'))
        expected['parameters_sha256'] = S.package_digest(parameters)
        if expected != unit:
            errs.append('semantic interface, parameters or branch differ from the original registry binding')
        model = GR.build(json.loads(harness_path.read_text()))
        digest = hashlib.sha256(R.encode_r1cs(model.r)).hexdigest()
        if digest != unit['original_r1cs_sha256'] or digest != original['circuit']['r1cs_sha256']:
            errs.append('original reconstructed R1CS binding differs')
        if (model.inputs != [row['wire'] for row in unit['inputs']]
                or model.native_outputs != [row['wire'] for row in unit['native_outputs']]
                or original['statement'].get('emulated_outputs', []) != unit['emulated_outputs']
                or RG.counts_of(model.r) != unit['reference_cost']
                or model.r.n_wires != unit['n_wires'] or str(model.r.prime) != unit['native_prime']):
            errs.append('original model IO, field, size or cost differs')
        witness = [int(value) for value in (root / nv['witness_file']).read_text().splitlines() if value.strip()]
        if len(witness) != model.r.n_wires or any(value < 0 or value >= model.r.prime for value in witness):
            errs.append('non-vacuity witness has wrong size or non-field entries')
        elif not R.satisfies(model.r, witness):
            errs.append('non-vacuity concrete witness violates original R1CS')
        evaluation = json.loads((root / nv['file']).read_text())
        if not isinstance(evaluation, dict) or not isinstance(evaluation.get('evaluator'), dict):
            return errs + ['non-vacuity evaluation and evaluator must be objects']
        evaluator = evaluation['evaluator']
        if (evaluation.get('actual_solver_assignment') is not True or evaluator.get('exit_code') != 0
                or evaluator.get('reason') not in ('', None)
                or evaluator.get('evaluation_sha256') != sha256_file(str(root / 'Evaluation.lean'))
                or evaluator.get('support_sha256') != sha256_file(str(root / 'EvaluationSupport.lean'))
                or (root / 'Evaluation.lean').read_text() != CG.render_modules(unit)['Evaluation.lean']
                or (root / 'EvaluationSupport.lean').read_text() != CG.evaluation_support(
                    (root / unit['model_file']).read_text(), unit['model_module'])):
            errs.append('non-vacuity evaluator did not succeed or its exact generated sources differ')
        rows = evaluation.get('rows', [])
        if not isinstance(rows, list) or not rows or any(not isinstance(row, dict) for row in rows):
            return errs + ['non-vacuity evaluation rows are malformed']
        observed = []
        for line in (root / 'evidence/evaluation.stdout').read_text().splitlines():
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if isinstance(row, dict) and 'witness' in row:
                observed.append(row)
        if any({key: value for key, value in row.items() if key != 'witness_sha256'} not in observed for row in rows):
            errs.append('selected non-vacuity row is absent from the pinned Lean evaluation output')
        if (evaluation.get('binding_sha256') != problem['binding_sha256']
                or evaluation.get('witness_sha256') != nv['witness_sha256']
                or CG.nonvacuity_status(evaluation.get('rows', []), problem['property'],
                                       evaluation.get('completed') is True) != 'NONVACUOUS'):
            errs.append('non-vacuity joint-premise evaluation is missing or bound to a different statement')
        if len(witness) == unit['n_wires']:
            expected_premises = CG.concrete_input_premises(unit, witness)
            if any(row.get('witness_sha256') != nv['witness_sha256']
                   or any(row.get(key) is not value for key, value in expected_premises.items())
                   for row in evaluation.get('rows', [])):
                errs.append('non-vacuity input/domain flags do not match the bound concrete witness')
        battery = json.loads((root / problem['battery']['file']).read_text())
        if battery != {key: value for key, value in problem['battery'].items() if key not in ('file', 'sha256')}:
            errs.append('fixed-battery evidence bytes differ from the package result')
        template = CG.theorem_template(unit, problem['property'])
        goal = unit['id'] + ('.Soundness' if problem['property'] == 'soundness' else '.Completeness')
        for attempt in battery['attempts']:
            index = attempt['index']
            qualified = CG.qualify_battery(unit, problem['property'], CG.BATTERY_FORMS[index - 1])
            expected_source = template.replace('  sorry', '  unfold ' + goal + '\n  ' + qualified)
            source_file = 'evidence/battery/' + problem['property'] + '-' + str(index) + '.lean'
            stdout_file = source_file[:-5] + '.stdout'
            if (attempt.get('form') != qualified or attempt.get('source_file') != source_file
                    or attempt.get('stdout_file') != stdout_file
                    or attempt.get('source_sha256') != hashlib.sha256(expected_source.encode()).hexdigest()
                    or (root / source_file).read_text() != expected_source
                    or attempt.get('stdout_sha256') != sha256_file(str(root / stdout_file))):
                errs.append('battery attempt source, form or output binding differs')
            result = L.RunResult(attempt.get('exit_code'), '', 0, False, False, 0, attempt.get('reason', ''))
            messages = L.parse_messages((root / stdout_file).read_text())
            completed = CB.battery_completed(result, messages)
            closed = completed and result.rc == 0 and not L.errors(messages)
            if attempt.get('completed') is not completed or attempt.get('closed') is not closed:
                errs.append('battery result is not justified by its process exit and Lean output')
        if problem['status'] == 'FACT':
            check = json.loads((root / 'evidence' / (problem['property'] + '-closure-check.json')).read_text())
            closure_errors = _correctness_closure_errors(check, problem['checker'])
            if closure_errors:
                return errs + closure_errors
            closed = next(attempt for attempt in battery['attempts'] if attempt['closed'])
            if (check.get('verdict') != 'PASS' or check.get('proof_sha256') != closed['source_sha256']
                    or battery.get('closure_sha256') != closed['source_sha256']
                    or check.get('compile', {}).get('post', {}).get('type_sha256')
                    != problem['checker']['reference_type_sha256']):
                errs.append('FACT lacks the accepted exact-type fixed-battery closure evidence')
    except (OSError, ValueError, KeyError, TypeError, IndexError, AttributeError, StopIteration) as ex:
        errs.append('invalid correctness content: ' + str(ex))
    return errs


def _ratchet_statement_errors(problem: dict, ref_model: dict, pkg_dir: str | None) -> list[str]:
    errs = []
    st, chk = problem["statement"], problem["checker"]
    if chk["theorem_fqn"] != st["theorem_fqn"] or not st["theorem_fqn"].endswith(".equiv"):
        errs.append("checker and statement disagree on the theorem")
    if sorted(f["role"] for f in chk["files"]).count("statement") != 1:
        errs.append("checker.files must list exactly one statement file")
    imports = {f["path"]: f["sha256"] for f in chk["files"] if f["role"] == "import"}
    if imports.get(ref_model["file"]) != ref_model["sha256"]:
        errs.append("checker.files must import the reference model file at its recorded sha256")
    if pkg_dir is not None:
        errs += _file_errors(pkg_dir, chk["files"])
        sp = os.path.join(pkg_dir, st["file"])
        if os.path.isfile(sp):
            with open(sp, encoding="utf-8") as f:
                if f.read() != st["text"]:
                    errs.append("statement.text differs from Statement.lean")
    return errs


def validate_ratchet(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """A ratchet problem (OPEN) or a ratchet checker package (CANDIDATE): schema plus semantic checks; with
    ``pkg_dir`` also the reference model file and, for CANDIDATE, every checker file and the statement text."""
    errs = jsonschema_lite.validate(problem, schema or load_schema(RATCHET_SCHEMA_VERSION))
    if errs:
        return errs
    ref, rec, det = problem["reference"], problem["record"], problem["det"]
    if rec["nonlinear"] + rec["linear"] != rec["total"]:
        errs.append("record counts do not add up")
    if det["model_sha256"] != ref["model"]["sha256"] or det["model_file"] != ref["model"]["file"]:
        errs.append("DET evidence is not about the reference model file")
    if ref["compiler"]["tag"] in ("v2.0.9", "v0.5.46") and not ("optimization" in rec and "compiler_sha256" in rec):
        errs.append("a record of a source-built or circom 1 compiler must name its optimization and compiler digest")
    for par in (problem.get("context") or {}).get("parents", []):
        if par["n_instances"] < len(par["instance_paths"]):
            errs.append("context parent lists more instance paths than instances")
    if problem["status"] == "OPEN":
        for key in ("candidate", "statement", "checker"):
            if key in problem:
                errs.append(f"OPEN ratchet problem must not carry {key!r}")
        if pkg_dir is not None:
            errs += _file_errors(pkg_dir, [{"path": ref["model"]["file"], "sha256": ref["model"]["sha256"]}])
        return errs
    cand = problem["candidate"]
    if cand["nonlinear"] + cand["linear"] != cand["total"]:
        errs.append("candidate counts do not add up")
    if cand["smaller"] != (cand["nonlinear"] < rec["nonlinear"]):
        errs.append("candidate.smaller disagrees with the record")
    return errs + _ratchet_statement_errors(problem, ref["model"], pkg_dir)


NOIR_COST_SCALARS = ("nonlinear", "range_bits", "logic_bits", "memory")


def validate_ratchet_native(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """ZoKrates/halo2 schema, DET binding, actual metric, I/O and checker-file boundaries."""
    version = problem.get("schema_version")
    errs = jsonschema_lite.validate(problem, schema or load_schema(version))
    if errs:
        return errs
    ref, rec, det = problem["reference"], problem["record"], problem["det"]
    from . import ratchet as RT

    if det["method"] not in RT.DET_METHODS or det["checker"]["verdict"] != "PASS":
        errs.append("reference DET has no accepted evidence method")
    if problem["package_id"] != "ratchet/" + ref["registry_package_id"]:
        errs.append("package identity differs from the reference")
    if (det["registry_package_id"], det["model_file"], det["model_sha256"]) != (
        ref["registry_package_id"],
        ref["model"]["file"],
        ref["model"]["sha256"],
    ):
        errs.append("DET evidence is not bound to this reference model")
    from pathlib import PurePosixPath

    if problem["family"] == "zokrates":
        path = PurePosixPath(ref["source_dir"])
        if path.is_absolute() or ".." in path.parts or "\\" in ref["source_dir"]:
            errs.append("source_dir escapes the pinned repository")
    for path in [ref["model"]["file"]] + (
        [ref["source"]["file"]] if problem["family"] == "zokrates" else [ref["ir"]["file"]]
    ):
        if PurePosixPath(path).is_absolute() or ".." in PurePosixPath(path).parts or "\\" in path:
            errs.append("reference file path escapes the package")
    if errs:
        return errs
    cand = problem.get("candidate")
    if problem["family"] == "zokrates":
        cnt = {k: rec[k] for k in ("nonlinear", "linear", "total")}
        if rec["nonlinear"] + rec["linear"] != rec["total"]:
            errs.append("record counts do not add up")
        if rec["rung"] == 0:
            if cnt != ref["model"]["counts"] or rec["r1cs_sha256"] != ref["model"]["r1cs_sha256"]:
                errs.append("reference record is not the reproduced model count/digest")
        elif rec["nonlinear"] >= ref["model"]["counts"]["nonlinear"]:
            errs.append("later record must be strictly cheaper than the immutable reference")
        if ref["model"]["counts"]["nonlinear"] <= 0:
            errs.append("reference has no positive bound native cost record")
        if ref["compiler"]["style"] != ("brace" if ref["compiler"]["version"] == "0.8.8" else "colon"):
            errs.append("compiler version and native/legacy mode disagree")
        io = ref["io"]
        if (
            len(io["input_names"]) != len(io["input_types"])
            or len(io["input_names"]) != io["n_pub_in"] + io["n_prv_in"]
        ):
            errs.append("reference ABI input lengths disagree")
        if len(io["output_names"]) != len(io["output_types"]):
            errs.append("reference ABI output lengths disagree")
        if cand is not None:
            if cand["nonlinear"] + cand["linear"] != cand["total"]:
                errs.append("candidate counts do not add up")
            if cand["smaller"] != (cand["nonlinear"] < rec["nonlinear"]):
                errs.append("candidate smaller flag disagrees with the nonlinear metric")
    else:
        from . import ratchet_halo2 as H

        errs += H.cost_errors(rec)
        if cand is not None:
            errs += H.cost_errors(cand["cost"])
        if errs:
            return errs
        cost = {k: rec[k] for k in ref["model"]["cost"]}
        if H.priced(rec) != rec["priced"]:
            errs.append("halo2 record priced summary does not match its components")
        if rec["rung"] == 0 and cost != ref["model"]["cost"]:
            errs.append("halo2 initial record is not the reference concrete-layout cost")
        elif rec["rung"] > 0 and not H.cost_smaller(ref["model"]["cost"], rec):
            errs.append("later record must be strictly cheaper than the immutable reference")
        if ref["ir"]["sha256"] != ref["model"]["ir_sha256"]:
            errs.append("reference IR digest differs from the recorded model")
        if cand is not None:
            larger, smaller = H.cost_compare(rec, cand["cost"])
            if H.priced(cand["cost"]) != cand["cost"]["priced"]:
                errs.append("candidate priced summary does not match its components")
            if (
                cand["smaller"] != (not larger and bool(smaller))
                or larger != cand["larger_components"]
                or smaller != cand["smaller_components"]
            ):
                errs.append("candidate halo2 component-wise order disagrees with its cost")
    if cand is not None and cand["io"] != ref["io"]:
        errs.append("candidate changes the recorded input/output interface")
    if problem["status"] == "OPEN":
        if any(k in problem for k in ("candidate", "statement", "checker")):
            errs.append("OPEN native ratchet problem must not contain an answer/checker statement")
        if pkg_dir is not None:
            attrs = ("model", "source") if problem["family"] == "zokrates" else ("model", "ir")
            errs += _file_errors(pkg_dir, [dict(path=ref[k]["file"], sha256=ref[k]["sha256"]) for k in attrs])
    elif cand is None:
        errs.append("CANDIDATE needs a candidate record")
    else:
        errs += _ratchet_statement_errors(problem, ref["model"], pkg_dir)
        from . import ratchet_native as N, lean_emit as E

        _, _, ns = N.namespaces(problem)
        imports = {f["path"]: f["sha256"] for f in problem["checker"]["files"] if f["role"] == "import"}
        if len(imports) != 2 or imports.get(E.model_relpath(ns)) != cand["model_sha256"]:
            errs.append("checker imports must contain exactly the bound reference and candidate models")
    return errs


def noir_priced(c: dict) -> int:
    """Sum of the priced components of a Noir ratchet cost vector (a summary; the order is component-wise)."""
    return sum(c[k] for k in NOIR_COST_SCALARS) + sum(c["black_box"].values())


def noir_larger_components(record: dict, cand: dict) -> list[str]:
    """Priced components in which ``cand`` exceeds ``record`` (black boxes per function key)."""
    out = [k for k in NOIR_COST_SCALARS if cand[k] > record[k]]
    keys = sorted(set(record["black_box"]) | set(cand["black_box"]))
    return out + [f"black_box[{k}]" for k in keys if cand["black_box"].get(k, 0) > record["black_box"].get(k, 0)]


def noir_cost_smaller(record: dict, cand: dict) -> bool:
    """Component-wise order: no priced component larger and at least one smaller."""
    return not noir_larger_components(record, cand) and noir_priced(cand) < noir_priced(record)


def validate_ratchet_noir(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """A Noir ratchet problem (OPEN) or its checker package (CANDIDATE): schema plus semantic checks; with ``pkg_dir``
    also the reference model file, the wrapper text and the reference program (OPEN) or every checker file and the
    statement text (CANDIDATE)."""
    errs = jsonschema_lite.validate(problem, schema or load_schema(RATCHET_NOIR_SCHEMA_VERSION))
    if errs:
        return errs
    ref, rec, det = problem["reference"], problem["record"], problem["det"]
    if noir_priced(rec) != rec["priced"]:
        errs.append("record priced cost is not the sum of its components")
    if det["model_sha256"] != ref["model"]["sha256"] or det["model_file"] != ref["model"]["file"]:
        errs.append("DET evidence is not about the reference model file")
    if (ref["plan"] == "export") != bool(ref["crate"] and ref["target_file"]):
        errs.append("library references need a crate and a target file (standard-library references none)")
    if problem["status"] == "OPEN":
        for key in ("candidate", "statement", "checker"):
            if key in problem:
                errs.append(f"OPEN ratchet problem must not carry {key!r}")
        if pkg_dir is not None:
            errs += _file_errors(pkg_dir, [{"path": ref[k]["file"], "sha256": ref[k]["sha256"]}
                                           for k in ("model", "program", "append")])
        return errs
    cand = problem["candidate"]
    if noir_priced(cand) != cand["priced"]:
        errs.append("candidate priced cost is not the sum of its components")
    if cand["smaller"] != noir_cost_smaller(rec, cand):
        errs.append("candidate.smaller disagrees with the record")
    return errs + _ratchet_statement_errors(problem, ref["model"], pkg_dir)


def write_json(path: str, obj) -> None:
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=1, sort_keys=True, ensure_ascii=False)
        f.write("\n")


def validate_ratchet_gnark(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """A gnark ratchet problem (OPEN) or its checker package (CANDIDATE): schema plus semantic checks; with
    ``pkg_dir`` also the reference model file and the wrapper text (OPEN) or every checker file and the statement
    text (CANDIDATE)."""
    errs = jsonschema_lite.validate(problem, schema or load_schema(RATCHET_GNARK_SCHEMA_VERSION))
    if errs:
        return errs
    ref, rec, det = problem["reference"], problem["record"], problem["det"]
    if rec["nonlinear"] + rec["linear"] != rec["total"]:
        errs.append("record counts do not add up")
    if {k: rec[k] for k in ("nonlinear", "linear", "total")} != ref["model"]["counts"]:
        errs.append("the record is not the reference model's count")
    if det["model_sha256"] != ref["model"]["sha256"] or det["model_file"] != ref["model"]["file"]:
        errs.append("DET evidence is not about the reference model file")
    if (ref["kind"] == "function") != (ref["receiver_type"] is None):
        errs.append("a method or circuit reference names its receiver type, a function none")
    if problem["status"] == "OPEN":
        for key in ("candidate", "statement", "checker"):
            if key in problem:
                errs.append(f"OPEN ratchet problem must not carry {key!r}")
        if pkg_dir is not None:
            errs += _file_errors(pkg_dir, [{"path": ref[k]["file"], "sha256": ref[k]["sha256"]}
                                           for k in ("model", "wrapper")])
        return errs
    cand = problem["candidate"]
    if cand["nonlinear"] + cand["linear"] != cand["total"]:
        errs.append("candidate counts do not add up")
    if cand["smaller"] != (cand["nonlinear"] < rec["nonlinear"]):
        errs.append("candidate.smaller disagrees with the record")
    return errs + _ratchet_statement_errors(problem, ref["model"], pkg_dir)


def air_priced(c: dict) -> int:
    """Summary of an AIR ratchet cost vector (main columns + interactions + constraints; the order is component-wise)."""
    return c["main_columns"] + c["interactions"] + c["constraints"]


def air_cost_components(c: dict, degrees: set[str] | None = None) -> dict[str, int]:
    """The ordered components of an AIR ratchet cost vector (``constraints_deg_ge[k]``: constraints of degree >= k)."""
    out = {"main_columns": c["main_columns"], "interactions": c["interactions"], "constraints": c["constraints"],
           "interaction_degree": c["interaction_degree"]}
    for k in sorted(degrees if degrees is not None else set(c["constraints_deg_ge"]), key=int):
        out[f"constraints_deg_ge[{k}]"] = c["constraints_deg_ge"].get(k, 0)
    return out


def air_cost_compare(record: dict, cand: dict) -> tuple[list[str], list[str]]:
    """(components in which ``cand`` is larger than ``record``, components in which it is smaller)."""
    keys = set(record["constraints_deg_ge"]) | set(cand["constraints_deg_ge"])
    a, b = air_cost_components(record, keys), air_cost_components(cand, keys)
    return [k for k in a if b[k] > a[k]], [k for k in a if b[k] < a[k]]


def air_cost_smaller(record: dict, cand: dict) -> bool:
    """Component-wise order: no component larger and at least one smaller."""
    larger, smaller = air_cost_compare(record, cand)
    return not larger and bool(smaller)


def _air_cost_errors(c: dict, what: str) -> list[str]:
    errs = []
    if air_priced(c) != c["priced"]:
        errs.append(f"{what} priced cost is not main_columns + interactions + constraints")
    ge = c["constraints_deg_ge"]
    top = max((int(k) for k, v in ge.items() if v), default=0)
    if c["max_degree"] > 1 and top != c["max_degree"] or c["max_degree"] <= 1 and top:
        errs.append(f"{what} max_degree disagrees with constraints_deg_ge")
    vals = [ge.get(str(k), 0) for k in range(2, max(top, 1) + 1)]
    if any(v > c["constraints"] for v in vals) or vals != sorted(vals, reverse=True):
        errs.append(f"{what} constraints_deg_ge is not cumulative")
    return errs


def validate_ratchet_air(problem: dict, pkg_dir: str | None = None, schema: dict | None = None) -> list[str]:
    """An AIR ratchet problem (OPEN) or its checker package (CANDIDATE): schema plus semantic checks; with ``pkg_dir``
    also the reference model, IR, machine and message records (OPEN) or every checker file and the statement text
    (CANDIDATE)."""
    errs = jsonschema_lite.validate(problem, schema or load_schema(RATCHET_AIR_SCHEMA_VERSION))
    if errs:
        return errs
    ref, rec, det = problem["reference"], problem["record"], problem["det"]
    errs += _air_cost_errors(rec, "record")
    if det["model_sha256"] != ref["model"]["sha256"] or det["model_file"] != ref["model"]["file"]:
        errs.append("DET evidence is not about the reference model file")
    if rec["main_columns"] != ref["layout"]["width"]:
        errs.append("record main_columns differ from the reference width")
    if problem["status"] == "OPEN":
        for key in ("candidate", "statement", "checker"):
            if key in problem:
                errs.append(f"OPEN ratchet problem must not carry {key!r}")
        if pkg_dir is not None:
            errs += _file_errors(pkg_dir, [{"path": ref[k]["file"], "sha256": ref[k]["sha256"]}
                                           for k in ("model", "ir", "machine", "messages")])
        return errs
    cand = problem["candidate"]
    errs += _air_cost_errors(cand["cost"], "candidate")
    if cand["smaller"] != air_cost_smaller(rec, cand["cost"]):
        errs.append("candidate.smaller disagrees with the record")
    return errs + _ratchet_statement_errors(problem, ref["model"], pkg_dir)
