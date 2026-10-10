"""ZoKrates ratchet: pinned R1CS cost and snapshot-confined source boundary."""

from __future__ import annotations

from pathlib import Path
import argparse
import copy
import json
import os
import re
import shutil
import tempfile

from . import ratchet as RT
from . import ratchet_native as N
from . import package as P
from . import lean_emit as E
from . import lean_runner as L
from . import mech_r1cs as M
from . import r1cs as R
from . import zokrates_det as ZD
from . import zokrates_r1cs as ZR
from . import zokrates_legacy as ZL
from . import zokrates_toolchain as T
from . import zokrates_witness as ZW
from . import zokrates_source as ZS

GENERATOR_NAME = "boole-zk-registry-ratchet-zokrates"
GENERATOR_VERSION = "1.0"
SCHEMA_VERSION = "zk-registry-ratchet-zokrates-problem/v1"
GENERATOR_SOURCES = sorted(
    set(
        P.GENERATOR_SOURCES
        + RT.GENERATOR_SOURCES
        + ZD.generator_files()
        + M.generator_files()
        + ["ratchet_zokrates.py", "ratchet_native.py", "schema/ratchet_zokrates_problem.schema.json"]
    )
)
counts_of = RT.counts_of
Reject = RT.Reject
NotEligible = RT.NotEligible
METRIC = (
    "Non-linear R1CS constraints of the pinned ZoKrates compilation: both A and B contain a non-constant "
    "wire. Linear constraints are free; totals are reported. Inputs, outputs, ABI scalar types and "
    "public/private input partition must be identical to the reference."
)


def smaller(record: dict, candidate: dict) -> bool:
    """Whether the candidate strictly lowers the reference's nonlinear constraint count."""
    return candidate["nonlinear"] < record["nonlinear"]


def io_mismatch(reference: dict, candidate: dict) -> str | None:
    """The first ABI mismatch reason, or None when field, types, order and visibility agree."""
    for k in ("prime", "input_names", "output_names", "input_types", "output_types", "n_pub_in", "n_prv_in"):
        if reference.get(k) != candidate.get(k):
            return f"the candidate changes the reference ABI ({k})"
    return None


def source_closure(main: Path, root: Path) -> dict[str, str]:
    """Validate every filesystem import before allowing the pinned compiler to read it.

    ``root`` is an isolated copy of the verified snapshot, with repo/ and stdlib/.
    Compiler-native EMBED is not a filesystem import. No absolute or escaped
    path is accepted, including a symlink escape. Unknown import syntax fails
    closed instead of letting the compiler resolve an unchecked include.
    """
    root = root.resolve()
    todo, result = [main], {}
    while todo:
        p = todo.pop().resolve()
        try:
            rel = str(p.relative_to(root))
        except ValueError as ex:
            raise Reject("an import escapes the pinned snapshot") from ex
        if rel in result:
            continue
        if not p.is_file() or p.suffix != ".zok":
            raise Reject("an import is missing from the pinned snapshot")
        text = p.read_text(encoding="utf-8")
        imports = ZS.find_imports(text)
        starts = re.findall(r"(?m)^\s*(?:from|import)\b", ZS.strip_comments(text))
        if len(starts) != len(imports):
            raise Reject("unrecognized import syntax (unchecked imports are forbidden)")
        result[rel] = text
        for imp in imports:
            name = imp.module
            if name == "EMBED":
                continue
            if Path(name).is_absolute() or "\\" in name or "\0" in name:
                raise Reject("absolute or non-portable import path")
            name += "" if name.endswith(".zok") else ".zok"
            choices = [p.parent / name, root / "stdlib" / name]
            found = next((q for q in choices if q.is_file()), None)
            if found is None:
                raise Reject("an import is missing from the pinned snapshot")
            try:
                found.resolve().relative_to(root)
            except ValueError as ex:
                raise Reject("an import escapes the pinned snapshot") from ex
            todo.append(found)
    return result


def validate_snapshot(snapshot, want_sha=None):
    """Verify the regular-file manifest and reject links before the isolated snapshot copy."""
    manifest = RT.verify_snapshot(snapshot, want_sha)
    # RT's regular-file manifest omits links. Reject them BEFORE copytree can
    # dereference a link and turn an unpinned external file into a local import.
    for directory, dirs, files in os.walk(snapshot):
        if any(Path(directory, name).is_symlink() for name in dirs + files):
            raise RT.Reject('ZoKrates snapshots may not contain symbolic links')
    return manifest


def signature(model, abi):
    """The native field and flattened ABI interface in the compiler's declaration order."""
    return dict(
        prime=str(model.r.prime),
        prime_name=model.r.prime_name,
        input_names=model.io.input_names,
        output_names=model.io.output_names,
        input_types=model.input_leaf_types,
        output_types=[ty for _, ty in ZR.flatten_outputs(abi)],
        n_pub_in=model.r.n_pub_in,
        n_prv_in=model.r.n_prv_in,
    )


def _constraint_key(r):
    """Sorted A*B-C polynomial keys, normalised only by nonzero field factors."""
    # M.load_package reads emitted Lean constraints, where a zero factor can be
    # omitted (0*0=C versus 1*0=C). Compare the actual A*B-C polynomial, never
    # just counts. Normalize only by a nonzero field scalar; zero sets are exact.
    polys = []
    for a, b, c in r.constraints:
        out = {}
        for u, x in a:
            for v, y in b:
                mon = tuple(sorted(w for w in (u, v) if w != 0))
                out[mon] = (out.get(mon, 0) + x * y) % r.prime
        for u, x in c:
            mon = (u,) if u else ()
            out[mon] = (out.get(mon, 0) - x) % r.prime
        terms = sorted((m, k) for m, k in out.items() if k)
        if terms:
            inv = pow(terms[0][1], -1, r.prime)
            terms = [(m, k * inv % r.prime) for m, k in terms]
        polys.append(tuple(terms))
    return (r.prime, r.n_wires, sorted(polys))


def reference_r1cs(directory):
    """The registry reference's R1CS and parsed Lean model, with original wire numbering."""
    model, problem, _ = M.load_package(directory)
    circuit = problem["circuit"]
    nout = len(model.outputs)
    npub = circuit.get("n_pub_in", 0)
    # Only the native constraints and wire numbering are compared; the header
    # public/private split is independently pinned through the ABI signature.
    constraints = [tuple(sorted((w, k % model.p) for w, k in lc.items()) for lc in (c.a, c.b, c.c)) for c in model.cons]
    return R.R1cs(model.p, 32, model.n_wires, nout, npub, len(model.inputs) - npub, model.n_wires, constraints), model


def tool_pin(tools, compiler, snapshot):
    """Digest-check the recorded compiler and select the snapshot's standard library."""
    pins = N.read(os.path.join(tools, "compilers.json"))
    pin = dict(pins[compiler["version"]])
    if pin["version"] != compiler["version"] or P.sha256_file(pin["binary"]) != compiler["binary_sha256"]:
        raise RuntimeError("not the problem's pinned ZoKrates binary")
    pin["stdlib"] = os.path.join(snapshot, "stdlib")
    return pin


def compile_program(text, relative_dir, snapshot, pin, out):
    """Compile a main program in a fresh isolated snapshot and return its native model and costs."""
    validate_snapshot(snapshot)
    os.makedirs(out, exist_ok=True)
    root = os.path.join(out, "source")
    if os.path.exists(root):
        raise RuntimeError("compile source directory exists; use a fresh output")
    shutil.copytree(snapshot, root, ignore=shutil.ignore_patterns("MANIFEST.sha256"))
    main = Path(root) / "repo" / relative_dir / "boole_ratchet_main.zok"
    if main.exists():
        raise RT.Reject("the snapshot reserves the candidate main path")
    main.parent.mkdir(parents=True, exist_ok=True)
    main.write_text(text, encoding="utf-8")
    closure = source_closure(main, Path(root))
    effective = dict(pin, stdlib=os.path.join(root, "stdlib"))
    program, abi = os.path.join(out, "out"), os.path.join(out, "abi.json")
    cmd = [pin["binary"], "compile", "-i", str(main), "-o", program, "-s", abi]
    style = pin["style"]
    raw = os.path.join(out, "out.r1cs") if style == "brace" else program + ".ztf"
    if style == "brace":
        cmd += ["-r", raw]
    tmp = os.path.join(out, "tmp")
    os.makedirs(tmp, exist_ok=True)
    env = dict(
        PATH="/usr/bin:/bin",
        HOME=out,
        TMPDIR=tmp,
        ZOKRATES_STDLIB=effective["stdlib"],
        ZOKRATES_HOME=effective["stdlib"],
    )
    run = L.run_process(cmd, env, out, 300, 12288)
    RT.put(os.path.join(out, "compile.log"), run.out)
    if run.rc != 0 or not os.path.isfile(raw):
        raise RT.Reject("ZoKrates compile failed: " + run.out[-500:])
    abi_doc = N.read(abi)
    if style == "brace":
        model = ZR.build_native(raw, abi_doc)
    else:
        model, renumber = ZL.build_model(Path(raw).read_text(), abi_doc)
        model._renumber = renumber
    if len(model.r.constraints) > 4000:
        raise RT.Reject("candidate exceeds the 4,000-constraint native model cap")
    return dict(
        model=model,
        abi=abi_doc,
        program=program,
        abi_path=abi,
        pin=effective,
        work=out,
        r1cs_sha256=RT.sha(R.encode_r1cs(RT.canonicalize(model.r))),
        raw_sha256=P.sha256_file(raw),
        io=signature(model, abi_doc),
        counts=counts_of(model.r),
        closure=list(sorted(closure)),
        compile_secs=run.secs,
        compile_peak_rss_mb=run.peak_rss_mb,
    )


def compile_candidate(problem, problem_dir, candidate_path, out, snapshot, tools, build_dir=None):
    """Validate one candidate source, compile it at the pin and enforce the reference ABI."""
    validate_snapshot(snapshot, problem["snapshot"]["manifest_sha256"])
    path = Path(candidate_path)
    if path.is_symlink() or not path.is_file() or path.suffix != ".zok":
        raise RT.Reject("candidate must be one ordinary UTF-8 .zok main-program file")
    text = path.read_text(encoding="utf-8")
    pin = tool_pin(tools, problem["reference"]["compiler"], snapshot)
    result = compile_program(text, problem["reference"]["source_dir"], snapshot, pin, os.path.join(out, "compile"))
    why = io_mismatch(problem["reference"]["io"], result["io"])
    if why:
        raise RT.Reject(why)
    candidate = dict(
        source_sha256=RT.sha(text.encode()),
        r1cs_sha256=result["r1cs_sha256"],
        raw_sha256=result["raw_sha256"],
        io=result["io"],
        **result["counts"],
        smaller=smaller(problem["record"], result["counts"]),
    )
    return (
        dict(
            candidate=candidate, compile_secs=result["compile_secs"], compile_peak_rss_mb=result["compile_peak_rss_mb"]
        ),
        result,
    )


def candidate_model(problem, result):
    """Emit the compiled candidate's production Lean model in its checker namespace."""
    _, _, ns = N.namespaces(problem)
    ref, model = problem["reference"], result["model"]
    meta = dict(
        repo_id=ref["repo"],
        instantiation=ref["call"],
        generator=GENERATOR_NAME + " v1.0",
        repo_url="(local candidate)",
        commit="-",
        path="Candidate.zok",
        template="main",
        rule="ratchet-candidate",
        circom_version=ref["compiler"]["version"],
        circom_flags=["ZoKrates pinned native compile"],
        r1cs_sha256=result["r1cs_sha256"],
        prime_name=model.r.prime_name,
    )
    return (
        E.emit_model(ns, meta, model.r, model.outputs, model.inputs, model.wire_names)
        .replace("# DET model:", "# Ratchet candidate model:", 1)
        .replace("Compiled with circom", "Compiled with ZoKrates")
        .replace("circom prime", "ZoKrates field")
    )


def build_problem(reg_dir, snapshot, out, tools, det, compiler, env=None):
    """Build an OPEN problem after checked DET binding and exact native reference reproduction."""
    spec = N.reference_spec(reg_dir, "zokrates")
    if spec["compiler"]["name"] != "zokrates":
        raise RT.NotEligible("family", "not a ZoKrates reference")
    evidence = N.det_for_reference(spec, det)
    man = validate_snapshot(snapshot)
    wrapper_path = Path(reg_dir) / "evidence/wrapper.zok"
    if not wrapper_path.is_file():
        raise RT.NotEligible("source", "missing exact registry wrapper")
    text = wrapper_path.read_text()
    pin = tool_pin(tools, compiler, snapshot)
    work_parent = Path(out).parent / ".build-reference"
    work_parent.mkdir(parents=True, exist_ok=True)
    work = tempfile.mkdtemp(prefix="zokrates-", dir=work_parent)
    result = compile_program(text, str(Path(spec["path"]).parent), snapshot, pin, work)
    native, parsed = reference_r1cs(reg_dir)
    if (
        _constraint_key(native) != _constraint_key(result["model"].r)
        or parsed.inputs != result["model"].inputs
        or parsed.outputs != result["model"].outputs
    ):
        raise RT.NotEligible("rebuild", "pinned source rebuild does not reproduce the DET model")
    counts = result["counts"]
    if counts["nonlinear"] == 0:
        raise RT.NotEligible("zero-record", "the reference already has zero priced constraints")
    ref = {k: spec[k] for k in ("registry_package_id", "repo", "repo_url", "commit", "path", "call", "lean_opts")}
    ref.update(
        model=dict(spec["model"], r1cs_sha256=result["r1cs_sha256"], n_wires=native.n_wires, counts=counts),
        compiler=dict(compiler),
        source_dir=str(Path(spec["path"]).parent),
        source=dict(file="reference/main.zok", sha256=RT.sha(text.encode())),
        io=result["io"],
    )
    record = dict(
        counts, r1cs_sha256=result["r1cs_sha256"], rung=0, source="pinned reference compile (DET model reproduced)"
    )
    problem = dict(
        schema_version=SCHEMA_VERSION,
        kind="ratchet",
        family="zokrates",
        package_id="ratchet/" + spec["registry_package_id"],
        status="OPEN",
        reference=ref,
        record=record,
        metric=METRIC,
        admissibility=[
            "one complete main .zok program",
            "same ABI types, names and public/private input partition",
            "snapshot-confined imports and pinned compiler",
            "strictly fewer nonlinear constraints",
            "both directions of the input/output relation proved by the production checker",
        ],
        statement_shape="existential input/output relation equivalence",
        determinism=RT.DETERMINISM_NOTE,
        det=evidence,
        simulate=dict(
            seed="boole-ratchet-zokrates|" + spec["registry_package_id"],
            profiles="native typed edge and seeded random vectors",
        ),
        snapshot=dict(
            id=Path(snapshot).name,
            manifest_sha256=P.sha256_file(os.path.join(snapshot, "MANIFEST.sha256")),
            n_files=len(man),
        ),
        env=dict(env or spec["original"]["env"]),
        generator=N.generator_info(GENERATOR_NAME, GENERATOR_SOURCES),
        created_utc=RT.now(),
    )
    Path(out, "reference").mkdir(parents=True, exist_ok=True)
    RT.put(os.path.join(out, "reference/main.zok"), text)
    N.write_reference(problem, spec, reg_dir, out)
    return problem


def _run_witness(result, values, tag):
    """Run the native solver on one flat typed vector and recheck any returned witness."""
    work = os.path.join(result["work"], tag)
    os.makedirs(work, exist_ok=True)
    if result["pin"]["style"] == "brace":
        w, error = ZW.compute_witness_modern(result["pin"], result["program"], result["abi_path"], values, work)
    else:
        m = result["model"]
        w, error = ZW.compute_witness_legacy(
            result["pin"], result["program"], result["abi_path"], values, work, m._renumber, m.r.n_wires, m.r.prime
        )
    if w is not None and not R.satisfies(result["model"].r, w):
        raise RuntimeError("native solver witness violates the exported model")
    return w, error


def simulate(problem_dir, candidate_path, out, n, tools, snapshot=None, build_dir=None):
    """Compare native solver acceptance and outputs on typed samples; a screen, not a proof."""
    problem = N.load_problem(problem_dir, SCHEMA_VERSION)
    snapshot = N.resolve_snapshot(problem_dir, problem, snapshot)
    report, candidate = compile_candidate(problem, problem_dir, candidate_path, out, snapshot, tools)
    pin = tool_pin(tools, problem["reference"]["compiler"], snapshot)
    text = Path(problem_dir, problem["reference"]["source"]["file"]).read_text()
    if RT.sha(text.encode()) != problem["reference"]["source"]["sha256"]:
        raise RuntimeError("reference main source changed")
    reference = compile_program(
        text, problem["reference"]["source_dir"], snapshot, pin, os.path.join(out, "reference-compile")
    )
    if reference["r1cs_sha256"] != problem["reference"]["model"]["r1cs_sha256"] or io_mismatch(
        problem["reference"]["io"], reference["io"]
    ):
        raise RuntimeError("reference rebuild changed")
    leaves = ZR.flatten_inputs(reference["abi"]["inputs"])
    vectors = ZW.sample_flat_values(
        [ty for _, _, ty in leaves], reference["model"].r.prime, problem["simulate"]["seed"], n
    )
    mismatches = accepted = rechecked = 0
    for index, vals in enumerate(vectors):
        rw, _ = _run_witness(reference, vals, "wit-" + str(index))
        cw, _ = _run_witness(candidate, vals, "wit-" + str(index))
        accepted += rw is not None
        rechecked += (rw is not None) + (cw is not None)
        if (
            (rw is None) != (cw is None)
            or rw is not None
            and [rw[v] for v in reference["model"].outputs] != [cw[v] for v in candidate["model"].outputs]
        ):
            mismatches += 1
    report["simulation"] = dict(
        vectors=len(vectors),
        reference_accepts=accepted,
        witnesses_rechecked=rechecked,
        mismatches=mismatches,
        verdict="PASS" if mismatches == 0 else "MISMATCH",
    )
    P.write_json(os.path.join(out, "simulate.json"), report)
    return report


def main(argv=None):
    """Dispatch the shared native-family CLI with the ZoKrates compile pipeline."""
    import sys

    return N.cli(sys.modules[__name__], argv)


if __name__ == "__main__":
    raise SystemExit(main())
