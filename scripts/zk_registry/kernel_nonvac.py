"""Opt-in G-NONVAC-K: a concrete joint witness through the acceptance gate's two kernels.

Callers supply the authentic witness and all premises, retaining their provenance. This
supplementary stage never updates a catalog outcome. UNKNOWN is not a vacuity claim.
"""

from __future__ import annotations
import hashlib
import time
from .safe_fs import Path
from . import axiom_policy as A, closure as D, correctness_runtime as CR, lean_runner as L


def proof_source(
    imports: list[str], witness_definition: str, premises: list[str], *, reference=False
) -> str:
    if not premises or any(not isinstance(p, str) or not p.strip() for p in premises):
        raise ValueError("all joint witness premises required")
    body = "\n".join("import " + module for module in imports)
    body += "\nset_option maxHeartbeats 0\nset_option maxRecDepth 100000\nset_option synthInstance.maxSize 1000000\n"
    body += witness_definition + "\n"
    body += "theorem BooleNonvac.witness : " + " ∧ ".join("(" + p + ")" for p in premises)
    return body + " := by " + ("sorry" if reference else "decide") + "\n"


def run(
    env: L.LeanEnv,
    imports: list[str],
    witness_definition: str,
    premises: list[str],
    work: str,
    tools: D.Tools,
    sandbox,
    *,
    timeout=1800,
    axioms=(),
) -> dict:
    """N4 covers compilation, exports and both kernels; no native evaluation proves the target."""
    start = time.monotonic()
    timeout = min(float(timeout), 1800)
    result = dict(
        stage="G-NONVAC-K",
        status="UNKNOWN",
        assurance=sandbox.mode,
        n4_seconds=timeout,
        guard_mib=19000,
        premises=list(premises),
        method="kernel decide; acceptance two-kernel gate",
    )
    remaining = lambda: max(0, timeout - (time.monotonic() - start))
    try:
        if timeout <= 0:
            raise TimeoutError("N4")
        if axioms:
            result["reason"] = "measuring-stick-axioms-unsupported"
            return result
        source = proof_source(imports, witness_definition, premises)
        result["source_sha256"] = hashlib.sha256(source.encode()).hexdigest()
        directory = Path(work)
        directory.mkdir(parents=True, exist_ok=False)
        policy = A.preflight(list(axioms))
        if policy["verdict"] != "PASS":
            result["reason"] = "axiom-policy-unavailable"
            return result
        exported = []
        for role in ["Challenge", "Solution"]:
            root = directory / role
            root.mkdir()
            (root / "ProofMod.lean").write_text(
                proof_source(imports, witness_definition, premises, reference=role == "Challenge")
            )
            build = root / "build"
            build.mkdir()
            guarded = CR.runner(str(root / "compile-processes"), remaining())

            def runner(argv, task_env, cwd, limit, rss=None, *args, **kwargs):
                return guarded(
                    argv, task_env, cwd, limit, min(rss or 19000, 19000), *args, **kwargs
                )

            with L.sandbox_context(sandbox, [str(root)], [str(root)], runner):
                compiled, messages = L.compile_module(
                    env, str(root), "ProofMod.lean", str(build), ["--tstack=400000"], remaining()
                )
            result[role.lower() + "_compile"] = dict(
                rc=compiled.rc,
                seconds=compiled.secs,
                peak_rss_mb=compiled.peak_rss_mb,
                process_receipt=compiled.process_receipt,
                boundary=compiled.boundary,
            )
            if D.resource_reason(compiled) or remaining() <= 0:
                result["reason"] = "N4-resource-limit"
                return result
            if compiled.boundary and compiled.boundary.get("verdict") != "PASS":
                result["reason"] = "sandbox-unavailable"
                return result
            if compiled.rc != 0 or L.errors(messages):
                result.update(
                    status="FAIL" if role == "Solution" else "UNKNOWN",
                    reason=(
                        "witness-proof-not-constructed"
                        if role == "Solution"
                        else "reference-build-unavailable"
                    ),
                )
                return result
            record = D.export_module(
                env,
                "ProofMod",
                "BooleNonvac.witness",
                str(directory / "sealed" / (role + ".ndjson")),
                tools,
                sandbox,
                str(root),
                [str(build)],
                remaining(),
                policy["allowed_axioms"],
                exposure=sandbox.exposed_writers,
                identity=role == "Challenge",
            )
            result[role.lower() + "_export"] = record
            if record["verdict"] != "PASS":
                result["reason"] = record["reason"]
                return result
            exported.append(record)
        result["statement_closure_sha256"] = exported[0]["statement_closure_sha256"]
        if remaining() <= 0:
            result["reason"] = "N4-resource-limit"
            return result
        compared = D.compare_exports(
            exported[0]["path"],
            exported[1]["path"],
            "BooleNonvac.witness",
            tools,
            sandbox,
            str(directory / "kernels"),
            remaining(),
            policy["allowed_axioms"],
        )
        result["two_kernels"] = compared
        result["status"] = {"PASS": "PASS", "INVALID": "FAIL", "ERROR": "UNKNOWN"}[
            compared["verdict"]
        ]
        result["reason"] = compared["reason"]
    except (OSError, ValueError, KeyError, TypeError, TimeoutError) as exc:
        result["reason"] = "stage-unavailable"
        result["exception_type"] = type(exc).__name__
    finally:
        result["seconds"] = time.monotonic() - start
        if result["status"] == "PASS" and result["seconds"] > timeout:
            result.update(status="UNKNOWN", reason="N4-resource-limit")
    return result


def c_plan(package: str, problem: dict) -> dict:
    """Bind one archived C-family solver assignment to every antecedent of its contract.

    The caller authenticates the catalog metadata. This adapter verifies all source and
    witness bytes, regenerates the contract and decider, and never substitutes an input.
    Other families require their own witness adapter and remain UNKNOWN.
    """
    import json
    import re
    from pathlib import PurePosixPath
    from . import correctness_gnark as CG, spec_problem as SP

    if problem.get("kind") != "C" or problem.get("property") not in ("soundness", "completeness"):
        raise ValueError("unsupported witness family")
    root = Path(package)

    def read(relative, expected):
        parts = PurePosixPath(relative).parts
        if (
            not parts
            or relative != "/".join(parts)
            or relative.startswith("/")
            or ".." in parts
            or "\\" in relative
        ):
            raise ValueError("unsafe witness source path")
        with (root / relative).open("rb") as stream:
            raw = stream.read(64 * 1024**2 + 1)
        if len(raw) > 64 * 1024**2 or hashlib.sha256(raw).hexdigest() != expected:
            raise ValueError("witness source digest differs")
        return raw

    unit, nv = problem["binding"], problem["nonvacuity"]
    model = unit["model_module"]
    namespace = model.rsplit(".", 1)[0]
    ident = unit["id"]
    if any(
        not re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*(\.[A-Za-z_][A-Za-z_0-9]*)*", x)
        for x in (model, namespace, ident)
    ):
        raise ValueError("unsupported Lean namespace")
    rows = problem["checker"]["files"]
    by_path = {row["path"]: row for row in rows}
    if len(rows) != len(by_path):
        raise ValueError("duplicate source path")
    required = [
        unit["model_file"],
        "Candidate.lean",
        "Bindings.lean",
        "EvaluationSupport.lean",
        "Statements.lean",
    ]
    sources = {name: read(name, by_path[name]["sha256"]).decode("utf-8") for name in required}
    generated = CG.render_modules(unit)
    if any(sources[name] != generated[name] for name in ("Bindings.lean", "Statements.lean")):
        raise ValueError("contract differs from binding")
    if sources["EvaluationSupport.lean"] != CG.evaluation_support(
        sources[unit["model_file"]], model
    ):
        raise ValueError("constraint decider differs from model")
    evidence = json.loads(read(nv["file"], nv["sha256"]))
    witness = read(nv["witness_file"], nv["witness_sha256"])
    if (
        nv.get("status") != "NONVACUOUS"
        or evidence.get("actual_solver_assignment") is not True
        or evidence.get("completed") is not True
        or evidence.get("binding_sha256") != SP.package_digest(unit)
        or evidence.get("witness_sha256") != nv["witness_sha256"]
    ):
        raise ValueError("authentic joint solver witness unavailable")
    values = witness.decode("ascii").splitlines()
    prime = int(unit["native_prime"])
    if prime <= 1:
        raise ValueError("invalid native field")
    if (
        not 0 < unit["n_wires"] <= 1000000
        or len(values) != unit["n_wires"]
        or any(
            not re.fullmatch(r"0|[1-9][0-9]*", v) or len(v) > len(str(prime)) or int(v) >= prime
            for v in values
        )
    ):
        raise ValueError("noncanonical or wrong-length witness")
    kind = problem["property"]
    flags = ["valid_length", "Constraints", "Assumptions", "CanonicalInputs", "SpecDomain"]
    if kind == "completeness":
        flags += ["AcceptedData", "BindInputs"]
    if not any(
        row.get("witness_sha256") == nv["witness_sha256"] and all(row.get(k) is True for k in flags)
        for row in evidence.get("rows", [])
        if isinstance(row, dict)
    ):
        raise ValueError("no single recorded joint witness")
    w, data = "BooleNonvac.w", "BooleNonvac.data"
    definition = (
        "namespace BooleNonvac\ndef numbers : Array Nat := #[" + ", ".join(values) + "]\n"
        f"def w : Fin {namespace}.nWires → {namespace}.F := fun i => Nat.cast (numbers[i.val]!)\n"
        f"def data : {ident}.InputData := {ident}.dataOf w\nend BooleNonvac\n"
    )
    decidable = [
        ("Assumptions " + w, "assumptions " + w),
        ("CanonicalInputs " + w, "canonicalData (" + ident + ".dataOf " + w + ")"),
        ("SpecDomain " + w, "domainData (" + ident + ".dataOf " + w + ")"),
        ("CanonicalData " + data, "canonicalData " + data),
        ("AcceptedData " + data, "acceptedData " + data),
        ("BindInputs " + w + " " + data, "binds " + w + " " + data),
    ]
    for prop, boolean in decidable:
        definition += f"private instance : Decidable ({ident}.{prop}) := inferInstanceAs (Decidable ({ident}.{boolean} = true))\n"
    premises = CG.contract_premises(unit, kind, w, data)
    modules = [
        dict(
            path=name, module=by_path[name].get("module", name[:-5]), sha256=by_path[name]["sha256"]
        )
        for name in required[:-1]
    ]
    # Module identifiers must match the file's exact import name; sources cannot escape the work tree.
    if modules[0]["module"] != model or unit["model_file"] != model.replace(".", "/") + ".lean":
        raise ValueError("model import binding differs")
    if any(row["module"] != row["path"][:-5].replace("/", ".") for row in modules):
        raise ValueError("module path binding differs")
    return dict(
        family="C",
        property=kind,
        imports=["Bindings", "EvaluationSupport"],
        witness_definition=definition,
        premises=premises,
        modules=modules,
        source_sha256={name: by_path[name]["sha256"] for name in required},
        witness_sha256=nv["witness_sha256"],
        evidence_sha256=nv["sha256"],
        binding_sha256=SP.package_digest(unit),
    )


def run_c(
    env: L.LeanEnv, package: str, problem: dict, work: str, tools: D.Tools, sandbox, *, timeout=1800
) -> dict:
    """Supplementary C measurement including source preparation in the same N4 clock.

    A caller may allocate a shorter measurement budget; UNKNOWN at that boundary is
    not evidence that the complete 30-minute N4 limit would have been exceeded.
    """
    from dataclasses import replace

    start = time.monotonic()
    budget = min(float(timeout), 1800)
    result = dict(
        stage="G-NONVAC-K",
        status="UNKNOWN",
        assurance=sandbox.mode,
        budget_seconds=budget,
        n4_limit_seconds=1800,
        guard_mib=19000,
    )
    remaining = lambda: max(0, budget - (time.monotonic() - start))
    preparation = []
    try:
        if remaining() <= 0:
            raise TimeoutError("measurement budget")
        plan = c_plan(package, problem)
        result["witness_plan"] = {k: v for k, v in plan.items() if k not in ("witness_definition",)}
        root = Path(work)
        root.mkdir(parents=True, exist_ok=False)
        source = root / "sources"
        source.mkdir()
        build = root / "build"
        build.mkdir()
        env = replace(env, lean_path=[str(build)] + env.lean_path)
        guarded = CR.runner(str(root / "prepare-processes"), remaining())

        def runner(argv, task_env, cwd, limit, rss=None, *args, **kwargs):
            return guarded(
                argv,
                task_env,
                cwd,
                min(limit, remaining()),
                min(rss or 19000, 19000),
                *args,
                **kwargs,
            )

        for row in plan["modules"]:
            if remaining() <= 0:
                raise TimeoutError("measurement budget")
            raw = (Path(package) / row["path"]).read_bytes()
            if hashlib.sha256(raw).hexdigest() != row["sha256"]:
                raise ValueError("source changed")
            target = source / row["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
            with L.sandbox_context(sandbox, [str(source), str(build)], [str(build)], runner):
                compiled, messages = L.compile_module(
                    env, str(source), row["path"], str(build), ["--tstack=400000"], remaining()
                )
            preparation.append(
                dict(
                    module=row["module"],
                    rc=compiled.rc,
                    seconds=compiled.secs,
                    peak_rss_mb=compiled.peak_rss_mb,
                    process_receipt=compiled.process_receipt,
                    resource_reason=D.resource_reason(compiled),
                    boundary=compiled.boundary,
                )
            )
            if (
                compiled.rc
                or L.errors(messages)
                or D.resource_reason(compiled)
                or remaining() <= 0
                or compiled.boundary
                and compiled.boundary.get("verdict") != "PASS"
            ):
                result["reason"] = "witness-model-preparation-unavailable"
                return result
        result.update(
            run(
                env,
                plan["imports"],
                plan["witness_definition"],
                plan["premises"],
                str(root / "gate"),
                tools,
                sandbox,
                timeout=remaining(),
            )
        )
    except (OSError, ValueError, KeyError, TypeError, UnicodeError, TimeoutError) as exc:
        result.update(
            reason="authentic-witness-stage-unavailable", exception_type=type(exc).__name__
        )
    finally:
        result.update(
            preparation=preparation,
            seconds=time.monotonic() - start,
            budget_seconds=budget,
            n4_limit_seconds=1800,
        )
        if result["status"] == "PASS" and result["seconds"] > budget:
            result.update(status="UNKNOWN", reason="measurement-budget-exhausted")
        if result["status"] == "UNKNOWN" and result["seconds"] >= budget and budget < 1800:
            result["reason"] = "measurement-budget-exhausted"
    return result
