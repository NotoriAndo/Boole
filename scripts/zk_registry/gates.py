"""Automatic package gates.

* G-ELAB    the model module compiles and the statement elaborates with exactly one ``sorry``
            warning and no error; the kernel replay tool records the reference theorem type.
* G-NONVAC  a real witness from circom's witness generator satisfies the compiled R1CS (Python
            oracle) and the Lean model (``decide (Constraints w)`` evaluated by ``#eval``).
* G-FID     at least :data:`FID_MIN_REAL` distinct real witnesses are accepted by both the oracle
            and the Lean model (a template without inputs has exactly one witness), and on
            :data:`FID_MIN_MUTANTS` single-wire mutants the Lean verdict equals the oracle verdict.
            Every real witness and every mutant is evaluated in Lean; none is sampled away.
* G-TRIV    the ZK-PILOT-TRIV-P1 battery (11 tactics x V0/V1/V2) plus the P2 forms for linear and copy
            circuits (V3/V4: flatten the literal input/output lists, unfold, then simp / simp_all / decide /
            omega / grind) on the statement, with a capped budget; any closure fails the gate.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import dataclass, field

from . import lean_emit as E
from . import lean_runner as L

FID_MIN_REAL = 10
FID_MIN_MUTANTS = 10
ALLOWED_AXIOMS = {"propext", "Classical.choice", "Quot.sound"}
REPLAY_TOOL = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lean", "ZkReplay.lean")


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_text(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


@dataclass
class Gate:
    name: str
    status: str                     # PASS | FAIL | SKIPPED | ERROR
    detail: dict = field(default_factory=dict)

    def to_json(self) -> dict:
        return {"status": self.status, **self.detail}


# ------------------------------------------------------------------------------------------ replay

def parse_replay(path: str) -> dict:
    post = {"consts": [], "axioms": [], "target": None, "type_str": None, "replay": None}
    if not os.path.exists(path):
        return post
    with open(path, encoding="utf-8") as f:
        for ln in f.read().splitlines():
            parts = ln.split("\t")
            tag = parts[0]
            if tag == "CONST":
                post["consts"].append({"name": parts[1], "kind": parts[2], "unsafe": parts[3] == "true",
                                       "partial": parts[4] == "true"})
            elif tag == "REPLAY":
                post["replay"] = "ok"
            elif tag == "REPLAY-FAIL":
                post["replay"] = "FAIL: " + (parts[1] if len(parts) > 1 else "")
            elif tag == "TARGET":
                post["target"] = {"kind": parts[1], "levels": parts[2]}
            elif tag == "TARGET-MISSING":
                post["target"] = "missing"
            elif tag == "AXIOM":
                post["axioms"].append(parts[1])
            elif tag == "TYPE":
                post["type_str"] = ln[len("TYPE\t"):]
    if post["type_str"] is not None:
        post["type_sha256"] = sha256_text(post["type_str"])
    return post


def run_replay(env: L.LeanEnv, olean: str, fqn: str, out_file: str, opts: list[str], lean_path: list[str],
               timeout: float) -> tuple[L.RunResult, dict]:
    tstack = [o for o in opts if o.startswith("--tstack")]      # fix F1: same stack as the compile
    r = L.run_lean(env, tstack + ["--run", REPLAY_TOOL, olean, fqn, out_file], os.path.dirname(out_file),
                   timeout, extra_lean_path=lean_path)
    return r, parse_replay(out_file)


# ------------------------------------------------------------------------------------------ G-ELAB

def g_elab(env: L.LeanEnv, pkg_dir: str, build_dir: str, ns: str, work: str, timeout: float = 3600) -> Gate:
    os.makedirs(work, exist_ok=True)
    rel = E.model_relpath(ns)
    r, msgs = L.compile_module(env, pkg_dir, rel, build_dir, E.LEAN_OPTIONS, timeout)
    detail = {"model_compile_secs": r.secs, "model_compile_rc": r.rc}
    if r.rc != 0 or r.timeout or L.errors(msgs):
        detail["errors"] = [L.fmt_msg(m) for m in L.errors(msgs)[:5]] or [r.out[-500:]]
        return Gate("G-ELAB", "FAIL", detail)
    r, msgs = L.compile_module(env, pkg_dir, "Statement.lean", build_dir, E.LEAN_OPTIONS, timeout)
    warns = [m for m in msgs if m.get("severity") == "warning"]
    detail.update(statement_compile_secs=r.secs, statement_compile_rc=r.rc,
                  statement_warnings=[m.get("data", "")[:200] for m in warns])
    if r.rc != 0 or r.timeout or L.errors(msgs):
        detail["errors"] = [L.fmt_msg(m) for m in L.errors(msgs)[:5]] or [r.out[-500:]]
        return Gate("G-ELAB", "FAIL", detail)
    if len(warns) != 1 or "sorry" not in warns[0].get("data", ""):
        detail["errors"] = ["expected exactly one `declaration uses sorry` warning"]
        return Gate("G-ELAB", "FAIL", detail)
    fqn = f"{ns}.{E.STATEMENT_THEOREM}"
    rr, post = run_replay(env, os.path.join(build_dir, "Statement.olean"), fqn, os.path.join(work, "replay.out"),
                          E.LEAN_OPTIONS, [build_dir], timeout)
    detail.update(replay=post["replay"], replay_secs=rr.secs, axioms=post["axioms"],
                  target=post["target"])
    ok = (post["replay"] == "ok" and isinstance(post["target"], dict) and post["target"]["kind"] == "theorem"
          and set(post["axioms"]) <= ALLOWED_AXIOMS | {"sorryAx"} and "sorryAx" in post["axioms"])
    if not ok:
        detail["errors"] = [f"replay record unexpected: {rr.out[-300:]}"]
        return Gate("G-ELAB", "FAIL", detail)
    with open(os.path.join(work, "reference.type.txt"), "w", encoding="utf-8") as f:
        f.write(post["type_str"])
    detail["reference_type_sha256"] = post["type_sha256"]
    return Gate("G-ELAB", "PASS", detail)


# ------------------------------------------------------------------------------------------ G-FID / G-NONVAC

def lean_verdicts(env: L.LeanEnv, build_dir: str, ns: str, n_constraints: int, files: list[tuple[str, str]],
                  work: str, timeout: float = 3600, preconditions: bool = False) -> tuple[dict[str, str], L.RunResult]:
    """Lean verdicts per witness tag; with ``preconditions`` also ``PRE:<tag>`` for ``decide (Preconditions w)``."""
    src = os.path.join(work, "Fid.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(E.emit_fid_runner(ns, n_constraints, files, preconditions))
    r = L.run_lean(env, E.LEAN_OPTIONS + ["--json", src], work, timeout, extra_lean_path=[build_dir])
    text = "\n".join(m.get("data", "") for m in L.parse_messages(r.out))
    verdicts = dict(re.findall(r"^FID (\S+) (ACCEPT|REJECT)$", text, re.M))
    verdicts.update({f"PRE:{k}": v for k, v in re.findall(r"^PRE (\S+) (ACCEPT|REJECT)$", text, re.M)})
    if "FID-DONE" not in text:
        verdicts["__incomplete__"] = "; ".join(L.fmt_msg(m) for m in L.errors(L.parse_messages(r.out))[:3]) or r.out[-300:]
    return verdicts, r


def g_fid_nonvac(env: L.LeanEnv, build_dir: str, ns: str, n_constraints: int, real: list[list[int]],
                 muts: list[dict], input_free: bool, work: str, write_witness, pre=None) -> tuple[Gate, Gate]:
    """``pre``: the input preconditions (Python predicate on a full assignment) when the statement has
    them; Lean's ``decide (Preconditions w)`` must then agree with it on every witness, and every real
    witness must satisfy it."""
    wdir = os.path.join(work, "wit")
    os.makedirs(wdir, exist_ok=True)
    files = []
    for k, w in enumerate(real):
        p = os.path.join(wdir, f"real_{k:03d}.txt")
        write_witness(p, w)
        files.append((f"real_{k:03d}", p))
    for k, m in enumerate(muts):
        p = os.path.join(wdir, f"mut_{k:03d}.txt")
        write_witness(p, m["witness"])
        files.append((f"mut_{k:03d}", p))
    if not files:
        detail = {"real_witnesses": 0, "reason": "the witness generator produced no oracle-accepted witness"}
        return Gate("G-FID", "FAIL", dict(detail)), Gate("G-NONVAC", "FAIL", dict(detail))
    verdicts, r = lean_verdicts(env, build_dir, ns, n_constraints, files, work, preconditions=pre is not None)
    real_accept = sum(verdicts.get(f"real_{k:03d}") == "ACCEPT" for k in range(len(real)))
    agree = sum(verdicts.get(f"mut_{k:03d}") == ("ACCEPT" if m["oracle"] else "REJECT") for k, m in enumerate(muts))
    detail = {
        "method": "Lean #eval of decide (Constraints w) on every witness file, compared with the Python R1CS oracle",
        "real_witnesses": len(real), "real_lean_accept": real_accept, "real_oracle_accept": len(real),
        "mutants": len(muts), "mutants_oracle_reject": sum(not m["oracle"] for m in muts),
        "mutants_lean_agree": agree, "lean_eval_secs": r.secs,
    }
    pre_ok = True
    if pre is not None:
        py = [pre(w) for w in real] + [pre(m["witness"]) for m in muts]
        tags = [f"real_{k:03d}" for k in range(len(real))] + [f"mut_{k:03d}" for k in range(len(muts))]
        pre_agree = sum(verdicts.get(f"PRE:{t}") == ("ACCEPT" if v else "REJECT") for t, v in zip(tags, py))
        detail.update(preconditions_lean_python_agree=pre_agree, preconditions_evaluated=len(tags),
                      real_satisfy_preconditions=sum(py[:len(real)]))
        pre_ok = pre_agree == len(tags) and all(py[:len(real)])
    if "__incomplete__" in verdicts:
        detail["error"] = verdicts["__incomplete__"][:400]
        detail["reason"] = "the Lean evaluation did not complete (harness error); no verdict is inferred"
        return Gate("G-FID", "ERROR", dict(detail)), Gate("G-NONVAC", "ERROR", dict(detail))
    need_real = 1 if input_free else FID_MIN_REAL
    fid_ok = (real_accept == len(real) and len(real) >= need_real and len(muts) >= FID_MIN_MUTANTS
              and agree == len(muts) and pre_ok)
    fid_detail = dict(detail, required_real=need_real, input_free=input_free)
    if not fid_ok:
        reasons = []
        if len(real) < need_real:
            reasons.append(f"only {len(real)} distinct real witnesses (need {need_real})")
        if real_accept != len(real):
            reasons.append("the Lean model rejected a real witness")
        if agree != len(muts) or len(muts) < FID_MIN_MUTANTS:
            reasons.append("Lean and oracle verdicts differ on a mutant (or too few mutants)")
        if not pre_ok:
            reasons.append("Lean and Python disagree on the input preconditions (or a real witness violates them)")
        fid_detail["reason"] = "; ".join(reasons)
    nonvac_ok = real_accept >= 1
    return (Gate("G-FID", "PASS" if fid_ok else "FAIL", fid_detail),
            Gate("G-NONVAC", "PASS" if nonvac_ok else "FAIL",
                 {"real_witnesses_accepted": real_accept, "method": "witness generator + oracle + Lean #eval"}))


# ------------------------------------------------------------------------------------------ G-TRIV

def _battery_layout(text: str) -> dict[str, tuple[int, int]]:
    """theorem name -> (first line, line of its #print axioms)."""
    lines = text.split("\n")
    starts, prints = {}, {}
    for i, ln in enumerate(lines, 1):
        m = re.match(r"theorem (triv_\w+)", ln)
        if m:
            starts[m.group(1)] = i
        m = re.match(r"#print axioms (triv_\w+)", ln)
        if m:
            prints[m.group(1)] = i
    return {n: (starts[n], prints[n]) for n in starts}


def classify_battery(text: str, msgs: list[dict], timed_out: bool) -> dict[str, dict]:
    layout = _battery_layout(text)
    out = {}
    for name, (lo, hi) in layout.items():
        errs = [m for m in msgs if m.get("severity") == "error" and m.get("pos") and lo <= m["pos"]["line"] < hi]
        ax = [m.get("data", "") for m in msgs if m.get("pos") and m["pos"]["line"] == hi
              and ("depends on axioms" in m.get("data", "") or "does not depend on any axioms" in m.get("data", ""))]
        if not ax:
            status = "timeout" if timed_out else "failed"
            out[name] = {"status": status, "closed": False}
            continue
        axioms = ax[-1]
        closed = not errs and "sorryAx" not in axioms
        out[name] = {"status": "closed" if closed else "failed", "closed": closed,
                     "first_error": errs[0].get("data", "")[:300] if errs else "",
                     "axioms": axioms[:300]}
    return out


def _battery_file(env: L.LeanEnv, build_dir: str, ns: str, n_constraints: int, work: str, fname: str,
                  forms: list[tuple[str, str]], heartbeats: int, file_wall: float, single_wall: float,
                  rss_limit_mb: int, preconditions: bool, pre_lists) -> tuple[dict[str, dict], dict]:
    """Run one battery file; forms that time out are rerun alone with ``single_wall``."""
    text = E.emit_battery_forms(ns, n_constraints, forms, heartbeats, preconditions, pre_lists)
    path = os.path.join(work, f"{fname}.lean")
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    r = L.run_lean(env, E.LEAN_OPTIONS + ["--json", path], work, file_wall, extra_lean_path=[build_dir],
                   rss_limit_mb=rss_limit_mb)
    with open(os.path.join(work, f"{fname}.out"), "w", encoding="utf-8") as f:
        f.write(r.out[-200000:])
    run = {"variant": fname.replace("Battery_", ""), "secs": r.secs, "timeout": r.timeout, "memkill": r.memkill,
           "peak_rss_mb": r.peak_rss_mb}
    results = {}
    for name, c in classify_battery(text, L.parse_messages(r.out), r.timeout or r.memkill).items():
        if c["status"] == "timeout":
            form = next(fm for fm in forms if E.battery_theorem_name(*fm) == name)
            single = E.emit_battery_forms(ns, n_constraints, [form], heartbeats, preconditions, pre_lists)
            spath = os.path.join(work, f"Single_{name}.lean")
            with open(spath, "w", encoding="utf-8") as f:
                f.write(single)
            rs = L.run_lean(env, E.LEAN_OPTIONS + ["--json", spath], work, single_wall,
                            extra_lean_path=[build_dir], rss_limit_mb=rss_limit_mb)
            c = classify_battery(single, L.parse_messages(rs.out), rs.timeout or rs.memkill)[name]
            c["rerun_single"] = {"secs": rs.secs, "timeout": rs.timeout, "memkill": rs.memkill}
        results[name] = c
    return results, run


def _triv_gate(results: dict, runs: list, budget: dict, battery: str) -> Gate:
    closed = sorted(n for n, c in results.items() if c["closed"])
    detail = {"forms": len(results), "closed_by": closed, "battery": battery,
              "timeouts": sorted(n for n, c in results.items() if c["status"] == "timeout"),
              "budget": budget, "runs": runs}
    return Gate("G-TRIV", "FAIL" if closed else "PASS", detail)


BATTERY_P1_P2 = "P1 (V0-V2, 33 forms) + P2 (V3-V4, 7 forms)"
BATTERY_P2_ONLY = "P2 (V3-V4, 7 forms)"


def g_triv(env: L.LeanEnv, build_dir: str, ns: str, n_constraints: int, work: str, heartbeats: int = 200000,
           file_wall: float = 900, single_wall: float = 180, rss_limit_mb: int = 16384, preconditions: bool = False,
           pre_lists=(), p2: bool = True) -> Gate:
    """The P1 battery (one file per variant V0-V2) and, with ``p2``, the P2 file (V3-V4)."""
    os.makedirs(work, exist_ok=True)
    results: dict[str, dict] = {}
    runs = []
    files = [(f"Battery_{v}", [(v, t) for t in E.BATTERY_TACTICS]) for v in E.BATTERY_VARIANTS]
    if p2:
        files.append(("Battery_P2", [(v, t) for v, ts in E.BATTERY_P2 for t in ts]))
    for fname, forms in files:
        res, run = _battery_file(env, build_dir, ns, n_constraints, work, fname, forms, heartbeats, file_wall,
                                 single_wall, rss_limit_mb, preconditions, pre_lists)
        results.update(res)
        runs.append(run)
    budget = {"maxHeartbeats": heartbeats, "file_wall_s": file_wall, "single_wall_s": single_wall}
    return _triv_gate(results, runs, budget, BATTERY_P1_P2 if p2 else "P1 (V0-V2, 33 forms)")


def g_triv_p2(env: L.LeanEnv, build_dir: str, ns: str, n_constraints: int, work: str, heartbeats: int = 200000,
              file_wall: float = 900, single_wall: float = 180, rss_limit_mb: int = 16384,
              preconditions: bool = False, pre_lists=()) -> Gate:
    """The P2 file alone (re-run of packages whose P1 battery already ran)."""
    os.makedirs(work, exist_ok=True)
    res, run = _battery_file(env, build_dir, ns, n_constraints, work, "Battery_P2",
                             [(v, t) for v, ts in E.BATTERY_P2 for t in ts], heartbeats, file_wall, single_wall,
                             rss_limit_mb, preconditions, pre_lists)
    budget = {"maxHeartbeats": heartbeats, "file_wall_s": file_wall, "single_wall_s": single_wall}
    return _triv_gate(res, [run], budget, BATTERY_P2_ONLY)


def write_json(path: str, obj) -> None:
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=1, sort_keys=True)
        f.write("\n")
