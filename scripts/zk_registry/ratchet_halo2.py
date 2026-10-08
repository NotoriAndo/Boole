"""halo2 ratchet order over the concrete MockProver layout, not a wall-clock benchmark."""
from __future__ import annotations

import copy
import json
import os
import re
from pathlib import Path
import shutil
import tempfile

from . import halo2_ir as H
from . import halo2_lean_emit as HE
from . import halo2_det as HD
from . import ratchet_air as RA
from . import ratchet as RT
from . import ratchet_native as N
from . import package as P
from . import lean_runner as L

GENERATOR_NAME = "boole-zk-registry-ratchet-halo2"
GENERATOR_VERSION = "1.0"
SCHEMA_VERSION = "zk-registry-ratchet-halo2-problem/v1"
GENERATOR_SOURCES = sorted(set(P.GENERATOR_SOURCES + RT.GENERATOR_SOURCES + RA.GENERATOR_SOURCES + HD.generator_files() +
    ["ratchet_halo2.py", "ratchet_native.py", "schema/ratchet_halo2_problem.schema.json"]))
COST_SCALARS = ("domain_rows", "advice_columns", "fixed_columns", "gate_instances", "copies", "lookups")
METRIC = ("Component-wise concrete-layout cost: domain rows (2^k), advice columns, fixed columns, nonzero "
          "gate instances after substituting fixed cells/selectors, copy equalities, lookup instances, and "
          "cumulative gate-degree counts. No component may grow; at least one must shrink. This is a layout "
          "proxy, not a measured prover-time or gas claim.")


def cost_errors(cost):
    errors=[]
    rows=cost['domain_rows']
    if rows<=0 or rows & (rows-1):errors.append('domain_rows must be a power of two')
    hist=cost['degree_ge'];keys=sorted(hist,key=lambda x:int(x) if x.isdigit() else -1)
    if any(not x.isdigit() or int(x)<2 for x in keys):
        return errors+['degree_ge keys must be integer degrees >= 2']
    if keys!=[str(k) for k in range(2,cost['max_degree']+1)]:
        errors.append('degree histogram must include every degree through max_degree')
    previous=cost['gate_instances']
    for k in keys:
        if hist[k]>previous:errors.append('degree histogram must be cumulative and bounded by gate_instances')
        previous=hist[k]
    if cost['max_degree']>=2 and not previous:errors.append('max_degree must have a nonzero gate count')
    return errors


def cost_components(cost: dict, degrees=None) -> dict:
    out = {k: cost[k] for k in COST_SCALARS}
    for k in sorted(degrees if degrees is not None else cost["degree_ge"], key=int):
        out["degree_ge[" + k + "]"] = cost["degree_ge"].get(k, 0)
    return out


def cost_compare(record: dict, candidate: dict) -> tuple[list, list]:
    degrees = set(record["degree_ge"]) | set(candidate["degree_ge"])
    a, b = cost_components(record, degrees), cost_components(candidate, degrees)
    return [k for k in a if b[k] > a[k]], [k for k in a if b[k] < a[k]]


def cost_smaller(record: dict, candidate: dict) -> bool:
    grows, shrinks = cost_compare(record, candidate)
    return not grows and bool(shrinks)


def priced(cost: dict) -> int:
    """An informational summary only: the admissibility order remains component-wise."""
    return cost["domain_rows"] * (cost["advice_columns"] + cost["fixed_columns"]) + sum(
        cost[k] for k in ("gate_instances", "copies", "lookups"))


def cost_of(doc: dict, model: H.Model | None = None) -> dict:
    model = model or H.flatten(doc)
    # Native polynomial degrees; no simplification beyond H.flatten's fixed-cell substitution.
    degrees = []
    for node in model.nodes:
        op = node[0]
        degrees.append(0 if op == "const" else 1 if op == "main" else
                       degrees[node[1]] if op == "neg" else
                       degrees[node[1]] + degrees[node[2]] if op == "mul" else
                       max(degrees[node[1]], degrees[node[2]]))
    gates = [degrees[k] for k, _ in model.polys]
    out = dict(domain_rows=1 << doc["k"], advice_columns=doc["num_advice"], fixed_columns=doc["num_fixed"],
               gate_instances=len(model.polys), copies=len(model.copies), lookups=len(model.lookups),
               degree_ge={str(k): sum(d >= k for d in gates) for k in range(2, max(gates, default=0) + 1)})
    out.update(priced=priced(out), variables=model.n_vars, nodes=len(model.nodes),
               max_degree=max(gates, default=0))
    return out


def io_signature(doc, model):
    # The H1 model already fixes the semantic order of wrapper inputs/outputs.
    # Coordinate names are intentionally exact: source changes may not silently
    # relabel public instances or exchange same-typed outputs.
    return dict(prime=str(model.p),prime_name=model.field_name,
                input_names=[model.cell_name(v) for v in model.inputs],
                output_names=[model.cell_name(v) for v in model.outputs],instance_columns=doc["num_instance"])


def load_reference(problem, problem_dir):
    record = problem["reference"]["ir"]
    path = os.path.join(problem_dir,record["file"])
    if P.sha256_file(path) != record["sha256"]:
        raise RuntimeError("reference IR changed")
    doc = H.load(path)
    model = H.flatten(doc)
    if model.sha256() != record["model_sha256"]:
        raise RuntimeError("reference IR model digest mismatch")
    return doc,model


def layout_sha(doc,model):
    # Scroll serializes only nonzero instance values: an honest zero input may
    # disappear from doc.instance. The model's cells are the interface support,
    # determined by gates/copies/notes, not by this sparse witness encoding.
    normalized=dict(doc,instance_values={(c,r):0 for kind,c,r in model.cells if kind=='i'})
    return H.structure_sha256(normalized)


def guard_harness_references(snapshot,overlay):
    """A chip overlay cannot redeclare/call the exporter or alter harness imports/declarations."""
    pattern=re.compile(r'\bboole_\w*|\bMockProver\b|#\s*\[\s*test\s*\]')
    for rel,data in overlay.items():
        old=Path(snapshot,'repo',rel).read_text() if Path(snapshot,'repo',rel).is_file() else ''
        def protected_lines(text):return [ln for ln in text.splitlines() if pattern.search(ln)]
        if protected_lines(old)!=protected_lines(data.decode('utf-8')):
            raise RT.Reject('candidate changes or accesses the protected native harness/exporter boundary')


def _native_tools(tools, build):
    pins = N.read(os.path.join(tools,"rust-toolchains.json"))
    pin = pins[build["toolchain"]]
    for name in ("rustc","cargo"):
        path = os.path.join(pin["bin"],name)
        if P.sha256_file(path) != pin[name+"_sha256"] or pin[name+"_sha256"] != build[name+"_sha256"]:
            raise RuntimeError("not the pinned Rust toolchain")
    return pin


def build_extract(problem, snapshot, overlay, build_dir, out, tools, samples=16, seed=0):
    man = RA.manifest_of(snapshot,problem["snapshot"]["manifest_sha256"])
    repo = os.path.join(build_dir,"repo")
    RA.sync_tree(snapshot,man,overlay,repo)
    pin = _native_tools(tools,problem["reference"]["build"])
    os.makedirs(out,exist_ok=True)
    tmp = os.path.join(out,"tmp")
    os.makedirs(tmp,exist_ok=True)
    env = dict(PATH=pin["bin"]+":/usr/bin:/bin",RUSTC=os.path.join(pin["bin"],"rustc"),
               RUSTDOC=os.path.join(pin["bin"],"rustdoc"),RUSTUP_AUTO_INSTALL="0",CARGO_NET_OFFLINE="true",
               CARGO_HOME=os.path.join(tools,"cargo-home"),CARGO_TARGET_DIR=os.path.join(build_dir,"target"),
               CARGO_INCREMENTAL="0",CARGO_BUILD_JOBS="2",CARGO_TERM_COLOR="never",HOME=out,TMPDIR=tmp,
               GIT_TERMINAL_PROMPT="0",GIT_CONFIG_GLOBAL="/dev/null",GIT_CONFIG_NOSYSTEM="1")
    target = problem["reference"]["target"]
    command = [os.path.join(pin["bin"],"cargo"),*problem["reference"]["build"]["command"]]
    run = L.run_process(command,env,os.path.join(repo,target["workspace"]),5400,12288)
    RT.put(os.path.join(out,"build.log"),run.out)
    if run.rc != 0:
        raise RT.Reject("pinned offline halo2 build failed: "+run.out[-700:])
    executable = None
    for line in run.out.splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get("reason")=="compiler-artifact" and row.get("executable") and row.get("target",{}).get("name")==target["package"].replace("-","_"):
            executable = row["executable"]
    if executable is None:
        raise RuntimeError("cargo did not report the target wrapper test binary")
    dump = os.path.join(out,"extraction")
    os.makedirs(dump,exist_ok=True)
    env.update(BOOLE_H1_OUT=dump,BOOLE_H1_SAMPLES=str(samples),BOOLE_H1_SEED=str(seed),BOOLE_H1_MUTANTS="0")
    extract = L.run_process([executable,target["test"],"--exact","--test-threads=1"],env,out,1800,12288)
    RT.put(os.path.join(out,"run.log"),extract.out)
    paths = sorted(Path(dump).rglob("sample_*.json"))
    if extract.rc != 0 or len(paths)!=samples:
        raise RT.Reject("MockProver target failed or did not export every sample")
    docs = [H.load(str(p)) for p in paths]
    models = [H.flatten(d) for d in docs]
    structures = {(layout_sha(d,m),m.sha256()) for d,m in zip(docs,models)}
    if len(structures)!=1:
        raise RT.Reject("witness-dependent layout/fixed table is not admissible")
    if any(not m.within_policy() for m in models):
        raise RT.Reject("halo2 model exceeds the registry's concrete-layout size policy")
    if any(H.check(m,H.witness(m,d)) for m,d in zip(models,docs)):
        raise RuntimeError("MockProver accepted a witness the independent model rejects")
    return dict(doc=docs[0],model=models[0],docs=docs,models=models,paths=[str(p) for p in paths],
                binary_sha256=P.sha256_file(executable),build_secs=run.secs,extract_secs=extract.secs,
                peak_rss_mb=max(run.peak_rss_mb,extract.peak_rss_mb),structure_sha256=next(iter(structures))[0])


def compile_candidate(problem,problem_dir,candidate_path,out,snapshot,tools,build_dir=None):
    ref=problem["reference"]
    rules=dict(build=ref["build"],snapshot=problem["snapshot"])
    try:
        overlay=RA.overlay_of(rules,snapshot,candidate_path)
    except RT.Reject as ex:
        if str(ex)!="the candidate changes no file of the snapshot":
            raise
        overlay={}  # valid reference-as-candidate control; will be NOT-SMALLER
    guard_harness_references(snapshot,overlay)
    build_dir=build_dir or os.path.join(out,"build-tree")
    result=build_extract(problem,snapshot,overlay,build_dir,os.path.join(out,"compile"),tools)
    if io_signature(result["doc"],result["model"]) != ref["io"]:
        raise RT.Reject("candidate changes the concrete input/output interface")
    # Other rows, columns and tables may change only through the selected source
    # overlay; every resulting fixed value is explicit in the proof model.
    cost=cost_of(result["doc"],result["model"])
    larger,smaller=cost_compare(problem["record"],cost)
    candidate=dict(source_sha256=RA.overlay_digest(overlay),ir_sha256=P.sha256_file(result["paths"][0]),
                   io=ref["io"],cost=cost,smaller=not larger and bool(smaller),
                   larger_components=larger,smaller_components=smaller)
    return dict(candidate=candidate,native_binary_sha256=result["binary_sha256"],build_secs=result["build_secs"],extract_secs=result["extract_secs"],
                peak_rss_mb=result["peak_rss_mb"]),result


def candidate_model(problem,result):
    _,_,ns=N.namespaces(problem)
    ref=problem["reference"]
    meta=dict(repo_id=ref["repo"],target=ref["call"],generator=GENERATOR_NAME+" v1.0",repo_url="(local candidate)",
              commit="-",path=ref["path"],symbol=ref["call"],wrapper=ref["target"]["test"],rule="ratchet-candidate",
              call=ref["call"],halo2_line=ref["target"]["line"],ir_sha256=P.sha256_file(result["paths"][0]),
              model_sha256=result["model"].sha256())
    text,_=HE.emit_model(ns,meta,result["model"])
    return text.replace("# DET model:","# Ratchet candidate model:",1)


def build_problem(reg_dir,snapshot,out,tools,det,target,build,env=None,build_dir=None):
    spec=N.reference_spec(reg_dir,"halo2")
    if spec["compiler"]["name"]!="halo2":
        raise RT.NotEligible("family","not a halo2 reference")
    evidence=N.det_for_reference(spec,det)
    man=RA.manifest_of(snapshot)
    source=Path(reg_dir)/"evidence/ir_sample_000.json"
    doc=H.load(str(source))
    model=H.flatten(doc)
    if model.sha256()!=spec["original"]["circuit"]["r1cs_sha256"]:
        raise RT.NotEligible("model","H1 native model digest is not its recorded DET model")
    cost=cost_of(doc,model)
    ref={k:spec[k] for k in ("registry_package_id","repo","repo_url","commit","path","call","lean_opts")}
    ref.update(model=dict(spec["model"],ir_sha256=P.sha256_file(str(source)),n_wires=max(model.n_vars,1),cost=cost),
               ir=dict(file="reference/layout.ir.json",sha256=P.sha256_file(str(source)),model_sha256=model.sha256()),
               io=io_signature(doc,model),target=target,build=build,compiler=dict(spec["compiler"]))
    # Historical compiler flags are explanatory; the OPEN problem pins the
    # Rust compiler/build/harness separately and retains a strict compiler record.
    ref["compiler"]={k:ref["compiler"][k] for k in ("version","binary_sha256","source")}
    problem=dict(schema_version=SCHEMA_VERSION,kind="ratchet",family="halo2",package_id="ratchet/"+spec["registry_package_id"],
                 status="OPEN",reference=ref,record=dict(cost,rung=0,source="reference concrete MockProver layout"),metric=METRIC,
                 admissibility=["Rust overlay restricted to the selected source roots; harness/wrapper/manifests unchanged",
                                "pinned offline Rust build and witness-independent layout","exact input/output interface",
                                "no priced component grows and one shrinks","input/output relation equivalence checked by Lean"],
                 statement_shape="existential input/output relation equivalence",determinism=RT.DETERMINISM_NOTE,det=evidence,
                 simulate=dict(seed="boole-ratchet-halo2|"+spec["registry_package_id"],profiles="pinned H1 wrapper seeded inputs"),
                 snapshot=dict(id=Path(snapshot).name,manifest_sha256=P.sha256_file(os.path.join(snapshot,"MANIFEST.sha256")),n_files=len(man)),
                 env=dict(env or spec["original"]["env"]),generator=N.generator_info(GENERATOR_NAME,GENERATOR_SOURCES),created_utc=RT.now())
    work=Path(out).parent/".build-reference"
    work.mkdir(parents=True,exist_ok=True)
    fresh=tempfile.mkdtemp(prefix="halo2-",dir=work)
    result=build_extract(problem,snapshot,{},build_dir or os.path.join(fresh,"tree"),os.path.join(fresh,"run"),tools)
    if result["model"].sha256()!=model.sha256() or io_signature(result["doc"],result["model"])!=ref["io"] or cost_of(result["doc"],result["model"])!=cost:
        raise RT.NotEligible("rebuild","snapshot does not reproduce the DET layout model and cost")
    ref["compiler"]["binary_sha256"]=result["binary_sha256"]
    ref["compiler"]["source"]="pinned rebuilt H1 wrapper test binary; original H1 binary SHA256 "+spec["compiler"]["binary_sha256"]+"; native DET model exactly reproduced"
    Path(out,"reference").mkdir(parents=True,exist_ok=True)
    shutil.copyfile(source,Path(out,"reference/layout.ir.json"))
    N.write_reference(problem,spec,reg_dir,out)
    return problem


def simulate(problem_dir,candidate_path,out,n,tools,snapshot=None,build_dir=None):
    problem=N.load_problem(problem_dir,SCHEMA_VERSION)
    snapshot=N.resolve_snapshot(problem_dir,problem,snapshot)
    report,result=compile_candidate(problem,problem_dir,candidate_path,out,snapshot,tools,build_dir)
    parent=os.path.join(out,"reference-build")
    reference=build_extract(problem,snapshot,{},parent,os.path.join(out,"reference-run"),tools,samples=n,seed=1)
    # The candidate's second extraction uses the same seeded input stream and
    # its already-built source tree, but does not reuse serialized reference IR.
    overlay=RA.overlay_of(dict(build=problem["reference"]["build"],snapshot=problem["snapshot"]),snapshot,candidate_path) if report["candidate"]["source_sha256"]!=RA.overlay_digest({}) else {}
    candidate=build_extract(problem,snapshot,overlay,build_dir or os.path.join(out,"build-tree"),os.path.join(out,"candidate-run"),tools,samples=n,seed=1)
    ref_doc,ref_model=load_reference(problem,problem_dir)
    if reference["model"].sha256()!=ref_model.sha256():
        raise RuntimeError("reference simulation model changed")
    if candidate["model"].sha256()!=result["model"].sha256() or candidate["structure_sha256"]!=result["structure_sha256"]:
        raise RT.Reject('candidate layout/model changes between compilation and simulation seeds')
    mismatches=0
    for rd,rm,cd,cm in zip(reference["docs"],reference["models"],candidate["docs"],candidate["models"]):
        rw,cw=H.witness(rm,rd),H.witness(cm,cd)
        if [rw[v] for v in rm.inputs]!=[cw[v] for v in cm.inputs] or [rw[v] for v in rm.outputs]!=[cw[v] for v in cm.outputs]:
            mismatches+=1
    report["simulation"]=dict(vectors=n,witnesses_rechecked=2*n,mismatches=mismatches,verdict="PASS" if not mismatches else "MISMATCH")
    P.write_json(os.path.join(out,"simulate.json"),report)
    return report


def main(argv=None):
    import sys
    return N.cli(sys.modules[__name__],argv)


if __name__=="__main__":
    raise SystemExit(main())
