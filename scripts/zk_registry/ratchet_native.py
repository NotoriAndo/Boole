"""Shared checker plumbing for native ZoKrates and halo2 ratchet models.

Framework modules own compile/admissibility/model/cost/simulation. This module
does not change the production proof checker or its allowed axioms.
"""
from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import shutil

from . import check as C
from . import gates as G
from . import lean_emit as E
from . import lean_runner as L
from . import package as P
from . import ratchet as RT


def read(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def load_problem(directory: str, schema: str) -> dict:
    problem = read(os.path.join(directory, "problem.json"))
    if problem.get("schema_version") != schema or problem.get("status") != "OPEN":
        raise RT.Reject("not an OPEN problem of this ratchet family")
    errors = P.validate_problem(problem, directory)
    if errors:
        raise RuntimeError("invalid reference problem: " + "; ".join(errors[:5]))
    return problem


def resolve_snapshot(directory: str, problem: dict, given: str | None = None) -> str:
    if given:
        return given
    for parent in [Path(directory).resolve(), *Path(directory).resolve().parents]:
        path = parent / "snapshots" / problem["snapshot"]["id"]
        if path.is_dir():
            return str(path)
    raise RuntimeError("pinned snapshot not found; use --snapshot")


def namespaces(problem):
    return RT.namespaces(problem)


def generator_info(name: str, files: list[str]) -> dict:
    here = Path(__file__).parent
    digest = hashlib.sha256()
    for file in sorted(files):
        digest.update(file.encode() + b"\0" + P.sha256_file(str(here / file)).encode() + b"\n")
    return dict(name=name, version="1.0", sources_sha256=digest.hexdigest())


def statement_text(problem: dict, candidate: dict) -> str:
    base, ref_ns, cand_ns = namespaces(problem)
    ref = problem["reference"]
    rf = ref_ns + "."
    return "\n".join([
        f"import {E.model_module(ref_ns)}", f"import {E.model_module(cand_ns)}", "", f"namespace {base}", "",
        f"/-- {problem['family']} ratchet equivalence for `{ref['registry_package_id']}`.",
        f"Reference model SHA256 `{ref['model']['sha256']}`; checked DET `{problem['det']['method']}`.",
        f"Candidate source digest `{candidate['source_sha256']}`; model digest `{candidate['model_sha256']}`.",
        "The pinned cost/admissibility pipeline measures the record and candidate independently.",
        "For every input vector x and output vector y, the realizable input/output relations agree in both directions.",
        "Simulation is only a screen; this theorem, accepted by the unchanged production checker, is the proof. -/",
        f"theorem equiv [Fact (Nat.Prime {rf}p)] :", f"    ∀ x y : List {rf}F,",
        f"      (∃ w : Fin {rf}nWires → {rf}F, {rf}Constraints w ∧ {rf}Inputs.map w = x ∧ {rf}Outputs.map w = y) ↔",
        "      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by",
        "  sorry", "", f"end {base}", "",
    ])


def write_candidate_package(problem: dict, problem_dir: str, candidate: dict, model_text: str, out: str) -> tuple:
    outp = Path(out)
    outp.mkdir(parents=True, exist_ok=True)
    _, _, cand_ns = namespaces(problem)
    meta = problem["reference"]["model"]
    source = Path(problem_dir) / meta["file"]
    if P.sha256_file(str(source)) != meta["sha256"]:
        raise RuntimeError("reference model changed")
    dest = outp / meta["file"]
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, dest)
    RT.put(str(outp / E.model_relpath(cand_ns)), model_text)
    statement = statement_text(problem, candidate)
    RT.put(str(outp / "Statement.lean"), statement)
    return statement, RT.checker_files(problem, statement, model_text)


def candidate_problem(problem: dict, candidate: dict, files: list, statement: str, type_sha: str) -> dict:
    out = RT.candidate_problem(problem, candidate, files, statement, type_sha)
    out.pop("_dir", None)
    # Same prime hypothesis as the DET model; names of non-Circom fields are explicit.
    out["statement"]["assumptions"] = ["[Fact (Nat.Prime p)]: the reference's recorded prime field modulus is prime."]
    return out


def run_candidate(framework, problem_dir: str, candidate_path: str, out: str, tools: str,
                  snapshot: str | None = None, env: L.LeanEnv | None = None, require_smaller=False,
                  build_dir: str | None = None):
    problem = load_problem(problem_dir, framework.SCHEMA_VERSION)
    snapshot = resolve_snapshot(problem_dir, problem, snapshot)
    report, result = framework.compile_candidate(problem, problem_dir, candidate_path, out, snapshot, tools, build_dir)
    candidate = report["candidate"]
    if require_smaller and not candidate["smaller"]:
        raise RT.Reject("NOT-SMALLER: no component-wise strict reduction from the record")
    model_text = framework.candidate_model(problem, result)
    candidate["model_sha256"] = RT.sha(model_text.encode())
    pkg = os.path.join(out, "pkg")
    statement, files = write_candidate_package(problem, problem_dir, candidate, model_text, pkg)
    report["statement_sha256"] = RT.sha(statement.encode())
    if env is not None:
        base, _, _ = namespaces(problem)
        elab = RT.elaborate(env, pkg, [f["path"] for f in files[1:]], base + ".equiv",
                            problem["reference"]["lean_opts"], os.path.join(out, "elab"))
        report["elab"] = elab
        if elab["status"] != "PASS":
            raise RuntimeError("candidate statement did not elaborate: " + str(elab.get("errors")))
        checker_problem = candidate_problem(problem, candidate, files, statement, elab["reference_type_sha256"])
        errors = P.validate_problem(checker_problem, pkg)
        if errors:
            raise RuntimeError("invalid checker package: " + "; ".join(errors[:5]))
        P.write_json(os.path.join(pkg, "problem.json"), checker_problem)
        report["package"] = pkg
    P.write_json(os.path.join(out, "candidate.json"), report)
    return report


def run_check(framework, problem_dir, candidate_path, solution, out, tools, env, snapshot=None, build_dir=None):
    try:
        report = run_candidate(framework, problem_dir, candidate_path, os.path.join(out, "candidate"), tools,
                               snapshot, env, True, build_dir)
    except RT.Reject as ex:
        result=dict(verdict="REJECTED", reason=str(ex))
        P.write_json(os.path.join(out,"verdict.json"),result)
        return "REJECTED", result
    checked = C.check(report["package"], solution, env, None, RT.DET_PROOF_LIMITS["timeout_s"], False,
                      os.path.join(out, "checker"))
    report.update(verdict=checked["verdict"], checker=checked)
    P.write_json(os.path.join(out, "verdict.json"), report)
    return checked["verdict"], report


def reference_spec(reg_dir: str, family: str):
    problem = read(os.path.join(reg_dir, "problem.json"))
    if problem.get("status") != "OPEN" or problem.get("property", {}).get("template") != "DET":
        raise RT.NotEligible("status", "a reference must be an effective OPEN DET package")
    errors = P.validate_problem(problem, reg_dir)
    if errors:
        raise RT.NotEligible("package", "; ".join(errors[:3]))
    imported = [x for x in problem["checker"]["files"] if x["role"] == "import"]
    if len(imported) != 1:
        raise RT.NotEligible("model", "expected one reference model module")
    m = imported[0]
    ids = problem["ids"]
    return dict(registry_package_id=problem["package_id"], repo=ids["repo"], repo_url=ids["repo_url"],
                commit=ids["commit"], path=ids["path"], call=problem["instantiation"]["call"],
                det_theorem_fqn=problem["checker"]["theorem_fqn"], lean_opts=problem["checker"]["lean_opts"],
                model=dict(file=m["path"], module=m["module"], namespace=m["module"].removesuffix(".Model"),
                           sha256=m["sha256"]), compiler=problem["circuit"]["compiler"], family=family,
                original=problem)


def det_for_reference(spec: dict, det: dict | None):
    problem = spec["original"]
    reason = RT.det_exclusion(det, spec["model"]["sha256"])
    if reason:
        raise RT.NotEligible(*reason)
    if det["registry_package_id"] != spec["registry_package_id"] or det["theorem_fqn"] != spec["det_theorem_fqn"]:
        raise RT.NotEligible("det", "DET evidence refers to a different package/theorem")
    if det["model_file"] != spec["model"]["file"]:
        raise RT.NotEligible("det", "DET evidence has a different model path")
    return copy.deepcopy(det)


def write_reference(problem: dict, spec: dict, reg_dir: str, out: str):
    if Path(out, "problem.json").exists():
        raise RT.NotEligible("output-exists", "will not replace an existing issued problem")
    path = Path(out) / spec["model"]["file"]
    path.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(Path(reg_dir) / spec["model"]["file"], path)
    P.write_json(os.path.join(out, "problem.json"), problem)
    errors = P.validate_problem(problem, out)
    if errors:
        raise RuntimeError("new problem did not validate: " + "; ".join(errors[:5]))


def cli(framework, argv=None):
    import argparse
    parser = argparse.ArgumentParser(description=framework.__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)
    build=sub.add_parser("build",help="Build an OPEN problem from already checked DET evidence and a pinned snapshot")
    for name in ("registry","snapshot","out","tools","det-record","env-pins","config"):
        build.add_argument("--"+name,required=True)
    for command in ("count", "simulate", "statement", "check"):
        p = sub.add_parser(command)
        p.add_argument("--problem", required=True)
        p.add_argument("--candidate", required=True)
        p.add_argument("--out", required=True)
        p.add_argument("--tools", required=True)
        p.add_argument("--snapshot")
        p.add_argument("--build")
        if command in ("statement", "check"):
            p.add_argument("--lean-env", required=True)
        if command == "check":
            p.add_argument("--solution", required=True)
        if command == "simulate":
            p.add_argument("--n", type=int, default=256)
    args = parser.parse_args(argv)
    L.set_lean_limits(2, 12288)
    if args.cmd=="build":
        config=read(args.config)
        kwargs=dict(env=read(args.env_pins),det=read(args.det_record))
        if framework.SCHEMA_VERSION==P.RATCHET_ZOKRATES_SCHEMA_VERSION:
            kwargs['compiler']=config['compiler']
        else:
            kwargs.update(target=config['target'],build=config['build'])
        try:
            result=framework.build_problem(args.registry,args.snapshot,args.out,args.tools,**kwargs)
            print(json.dumps(result,sort_keys=True));return 0
        except RT.NotEligible as ex:
            print(json.dumps(dict(verdict="EXCLUDED",reason=ex.code,detail=ex.detail)));return 4
        except Exception as ex:
            print(json.dumps(dict(verdict="ERROR",reason=f"{type(ex).__name__}: {ex}")));return 3
    env = L.load_env(args.lean_env) if args.cmd in ("statement", "check") else None
    try:
        if args.cmd == "check":
            verdict, result = run_check(framework, args.problem, args.candidate, args.solution, args.out, args.tools,
                                        env, args.snapshot, args.build)
            print(json.dumps(result, sort_keys=True))
            return {"PASS":0, "FAIL":1, "INVALID":2, "ERROR":3, "REJECTED":4}[verdict]
        if args.cmd == "simulate":
            result = framework.simulate(args.problem, args.candidate, args.out, args.n, args.tools,
                                        args.snapshot, args.build)
            print(json.dumps(result, sort_keys=True))
            return 5 if result["simulation"]["verdict"] != "PASS" else 0 if result["candidate"]["smaller"] else 4
        result = run_candidate(framework, args.problem, args.candidate, args.out, args.tools, args.snapshot,
                               env, False, args.build)
        print(json.dumps(result, sort_keys=True))
        return 0 if result["candidate"]["smaller"] else 4
    except RT.Reject as ex:
        print(json.dumps(dict(verdict="REJECTED", reason=str(ex))))
        return 4
    except Exception as ex:
        print(json.dumps(dict(verdict="ERROR", reason=f"{type(ex).__name__}: {ex}")))
        return 3
