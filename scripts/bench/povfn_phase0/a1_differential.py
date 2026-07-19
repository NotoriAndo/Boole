"""PoVFN Phase 0-A / A1 — differential + binding experiment driver.

THROWAWAY offline experiment (operator directive 2026-07-19). Never wired
into self-test/consensus; reads the consensus checker only via a work-dir
copy (work/checker-export). Results go to a temp/--out path.

Pipeline per case:
  seed -> canonical module (derive_module, same boole-core code the node
  re-verifier uses) -> host elaboration under pinned budgets -> lean4export
  dependency closure -> nanoda_bin kernel check (Rust, independent) ->
  driver-side P-stage binding checks:
    B1 target theorem present in export
    B2 statement equality: structural hash of the type DAG == structural
       hash of an independently elaborated statement-only axiom module
    B3 axiom allowlist == Boole TB.1 list (enforced inside nanoda config)

K-stage = nanoda kernel verdict alone. P-stage(A1 scope) = K + B1 + B2 + B3.
Elaboration itself stays a host step (cap recorded in the report).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, "work", "checker-export")
DERIVE = os.path.join(HERE, "derive_module", "target", "release", "povfn-derive-module")
LEAN4EXPORT = os.path.join(HERE, "vendor", "lean4export", ".lake", "build", "bin", "lean4export")
NANODA = os.path.join(HERE, "vendor", "nanoda_lib", "target", "release", "nanoda_bin")
CHECKER_DIR = os.path.join(HERE, "..", "..", "..", "lean", "checker")
LAKE = os.path.expanduser("~/.elan/bin/lake")
LEANCHECKER = os.path.expanduser(
    "~/.elan/toolchains/leanprover--lean4---v4.29.1/bin/leanchecker"
)

CHECKER_ARTIFACT_HASH = "1dd3055acb05142816f2082f0b3ad000c49513c3a2401572ec68703542042be1"
PINNED_HEARTBEATS = "400000"
PINNED_REC_DEPTH = "512"
ALLOWED_AXIOMS = ["propext", "Classical.choice", "Quot.sound"]
THM_FULL_NAME = "BooleVerifyMod.instance_thm"

# fixture seed from fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json
REAL_SEED = "41031c76ab1e040a8423a560e5fbc4ef9ece71641cba2d95026d55202808aabe"


def sh(cmd, cwd=None, timeout=600, env=None):
    t0 = time.perf_counter()
    proc = subprocess.run(
        cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, env=env
    )
    return proc, time.perf_counter() - t0


def derive(seed: str, mode: str) -> str:
    proc, _ = sh([DERIVE, seed, CHECKER_ARTIFACT_HASH, mode])
    if proc.returncode != 0:
        raise RuntimeError(f"derive failed: {proc.stderr}")
    return proc.stdout


def elaborate(module_src: str, module_name: str) -> dict:
    src_path = os.path.join(WORK, f"{module_name}.lean")
    olean_path = os.path.join(WORK, ".lake", "build", "lib", "lean", f"{module_name}.olean")
    with open(src_path, "w") as f:
        f.write(module_src)
    proc, dt = sh(
        [
            LAKE, "env", "lean",
            f"-DmaxHeartbeats={PINNED_HEARTBEATS}",
            f"-DmaxRecDepth={PINNED_REC_DEPTH}",
            f"{module_name}.lean", "-o", olean_path,
        ],
        cwd=WORK,
    )
    return {"ok": proc.returncode == 0, "wall_s": dt, "stderr": proc.stderr[-400:]}


def export_closure(module_name: str, decls: list[str]) -> dict:
    out = os.path.join(WORK, f"{module_name}.ndjson")
    with open(out, "w") as f:
        proc = subprocess.Popen(
            [LAKE, "env", LEAN4EXPORT, module_name, "--", *decls],
            cwd=WORK, stdout=f, stderr=subprocess.PIPE, text=True,
        )
        t0 = time.perf_counter()
        _, err = proc.communicate(timeout=600)
        dt = time.perf_counter() - t0
    return {
        "ok": proc.returncode == 0,
        "wall_s": dt,
        "path": out,
        "bytes": os.path.getsize(out) if proc.returncode == 0 else 0,
        "stderr": err[-400:],
    }


def nanoda_check(ndjson_path: str, pp_decl: str | None = None) -> dict:
    cfg = {
        "export_file_path": ndjson_path,
        "use_stdin": False,
        "permitted_axioms": ALLOWED_AXIOMS,
        "unpermitted_axiom_hard_error": True,
        "nat_extension": True,
        "string_extension": True,
        "pp_declars": [pp_decl] if pp_decl else [],
        "pp_options": {"pp.explicit": False},
        "pp_to_stdout": bool(pp_decl),
        "print_success_message": True,
    }
    fd, cfg_path = tempfile.mkstemp(suffix=".json", dir=WORK)
    with os.fdopen(fd, "w") as f:
        json.dump(cfg, f)
    proc, dt = sh([NANODA, cfg_path])
    os.unlink(cfg_path)
    return {
        "accepted": proc.returncode == 0,
        "rc": proc.returncode,
        "wall_s": dt,
        "stdout": proc.stdout[-2000:],
        "stderr": proc.stderr[-400:],
    }


# ---------------------------------------------------------------------------
# Export parsing + structural statement hash (binding check B1/B2)
# ---------------------------------------------------------------------------

def load_export(path: str):
    names, levels, exprs, decls = {0: ("anon",)}, {0: ("zero",)}, {}, []
    with open(path) as f:
        for line in f:
            o = json.loads(line)
            if "in" in o:
                idx = o["in"]
                if "str" in o:
                    names[idx] = ("str", o["str"]["pre"], o["str"]["str"])
                else:
                    names[idx] = ("num", o["num"]["pre"], o["num"]["i"])
            elif "il" in o:
                idx = o["il"]
                body = {k: v for k, v in o.items() if k != "il"}
                levels[idx] = ("lvl", json.dumps(body, sort_keys=True))
            elif "ie" in o:
                idx = o["ie"]
                body = {k: v for k, v in o.items() if k != "ie"}
                exprs[idx] = body
            elif any(k in o for k in ("thm", "def", "axiom", "inductive", "opaque", "quot")):
                decls.append(o)
    return names, levels, exprs, decls


def name_str(names, idx) -> str:
    node = names.get(idx)
    if node is None or node[0] == "anon":
        return ""
    _, pre, part = node
    prefix = name_str(names, pre)
    return f"{prefix}.{part}" if prefix else str(part)


def structural_hash(names, levels, exprs, root: int) -> str:
    """Canonical content hash of an expression DAG, independent of the
    interning ids assigned by a particular export run."""
    memo_n, memo_l, memo_e = {}, {}, {}

    def hn(i):
        if i in memo_n:
            return memo_n[i]
        node = names[i]
        if node[0] == "anon":
            h = "anon"
        else:
            h = hashlib.sha256(
                f"{node[0]}|{hn(node[1])}|{node[2]}".encode()
            ).hexdigest()
        memo_n[i] = h
        return h

    def hl(i):
        if i in memo_l:
            return memo_l[i]
        node = levels[i]
        if node[0] == "zero":
            h = "zero"
        else:
            payload = json.loads(node[1])
            (kind, val), = payload.items()
            if kind == "param":
                inner = hn(val)
            elif kind == "succ":
                inner = hl(val)
            else:  # max / imax hold pairs
                if isinstance(val, dict):
                    inner = "|".join(hl(v) for v in sorted(val.values())) if kind == "max" else "|".join(
                        hl(v) for v in val.values()
                    )
                elif isinstance(val, list):
                    inner = "|".join(hl(v) for v in val)
                else:
                    inner = hl(val)
            h = hashlib.sha256(f"{kind}|{inner}".encode()).hexdigest()
        memo_l[i] = h
        return h

    def he(i):
        if i in memo_e:
            return memo_e[i]
        body = exprs[i]
        (kind, val), = body.items()
        if kind == "bvar":
            payload = f"bvar|{val}"
        elif kind == "sort":
            payload = f"sort|{hl(val)}"
        elif kind == "const":
            us = val.get("us", [])
            payload = f"const|{hn(val['name'])}|" + ",".join(hl(u) for u in us)
        elif kind == "app":
            payload = f"app|{he(val['fn'])}|{he(val['arg'])}"
        elif kind in ("lam", "forallE"):
            payload = (
                f"{kind}|{he(val['type'])}|{he(val['body'])}|{val.get('info','')}"
            )
        elif kind == "letE":
            payload = (
                f"letE|{he(val['type'])}|{he(val['value'])}|{he(val['body'])}"
            )
        elif kind == "proj":
            payload = f"proj|{hn(val['struct'])}|{val['idx']}|{he(val['expr'])}"
        elif kind == "natVal":
            payload = f"natVal|{val}"
        elif kind == "strVal":
            payload = f"strVal|{val}"
        elif kind == "mdata":
            payload = f"mdata|{he(val['expr'])}"
        else:
            payload = f"{kind}|{json.dumps(val, sort_keys=True)}"
        h = hashlib.sha256(payload.encode()).hexdigest()
        memo_e[i] = h
        return h

    return he(root)


def decl_type_hash(ndjson_path: str, full_name: str) -> str | None:
    names, levels, exprs, decls = load_export(ndjson_path)
    for d in decls:
        for kind in ("thm", "def", "axiom"):
            if kind in d:
                body = d[kind]
                if name_str(names, body["name"]) == full_name:
                    return structural_hash(names, levels, exprs, body["type"])
    return None


def expected_statement_hash(statement: str, tag: str) -> dict:
    """Independent path to the expected statement's type DAG hash: elaborate
    a statement-only axiom module (harness-side reference, not a miner
    submission) and hash its exported type."""
    mod = (
        "import Boole.Family.V0Helpers\n\nnamespace BooleStmtRef\n\n"
        "open Boole.Family.V0Helpers\n\n"
        f"axiom instance_stmt : {statement}\n\nend BooleStmtRef\n"
    )
    name = f"StmtRef{tag}"
    el = elaborate(mod, name)
    if not el["ok"]:
        return {"ok": False, "stage": "elaborate", "detail": el["stderr"]}
    ex = export_closure(name, ["BooleStmtRef.instance_stmt"])
    if not ex["ok"]:
        return {"ok": False, "stage": "export", "detail": ex["stderr"]}
    h = decl_type_hash(ex["path"], "BooleStmtRef.instance_stmt")
    return {"ok": h is not None, "hash": h}


# ---------------------------------------------------------------------------
# cases
# ---------------------------------------------------------------------------

def run_valid_case(seed: str, tag: str, reps: int) -> dict:
    rec = {"case": f"valid-{tag}", "seed": seed, "kind": "valid"}
    module = derive(seed, "module")
    statement = derive(seed, "statement").strip()
    name = f"Proof{tag}"

    el_times, ex_times, nk_times = [], [], []
    for _ in range(reps):
        el = elaborate(module, name)
        if not el["ok"]:
            rec["error"] = f"elaboration failed: {el['stderr']}"
            return rec
        el_times.append(el["wall_s"])
        ex = export_closure(name, [THM_FULL_NAME])
        if not ex["ok"]:
            rec["error"] = f"export failed: {ex['stderr']}"
            return rec
        ex_times.append(ex["wall_s"])
        nk = nanoda_check(ex["path"])
        nk_times.append(nk["wall_s"])
    rec["module_bytes"] = len(module)
    rec["export_bytes"] = ex["bytes"]
    rec["elaborate_s"] = {"median": statistics.median(el_times), "n": reps}
    rec["export_s"] = {"median": statistics.median(ex_times), "n": reps}
    rec["nanoda_s"] = {"median": statistics.median(nk_times), "n": reps}
    rec["nanoda_accepted"] = nk["accepted"]

    # binding checks
    got = decl_type_hash(ex["path"], THM_FULL_NAME)
    exp = expected_statement_hash(statement, tag)
    rec["binding"] = {
        "target_present": got is not None,
        "expected_hash_ok": exp.get("ok", False),
        "statement_match": exp.get("ok") and got == exp.get("hash"),
    }

    # pinned checker + leanchecker (single rep; process-dominated)
    src_path = os.path.join(WORK, f"{name}.lean")
    proc, dt = sh(
        [LAKE, "exec", "boole_check", src_path, PINNED_HEARTBEATS, PINNED_REC_DEPTH],
        cwd=CHECKER_DIR,
    )
    rec["boole_check"] = {"accepted": proc.returncode == 0, "wall_s": dt}
    env = dict(os.environ)
    env["LEAN_PATH"] = os.path.join(WORK, ".lake", "build", "lib", "lean")
    proc, dt = sh([LEANCHECKER, name], cwd=WORK, env=env)
    rec["leanchecker"] = {"accepted": proc.returncode == 0, "wall_s": dt}
    rec["all_judges_agree"] = (
        rec["nanoda_accepted"]
        and rec["boole_check"]["accepted"]
        and rec["leanchecker"]["accepted"]
        and rec["binding"]["statement_match"]
    )
    return rec


def run_wrong_seed_binding(seed_a: str, seed_b: str) -> dict:
    """Kernel-valid proof for seed A judged against seed B's expected
    statement: the kernel must accept, the binding layer must reject."""
    rec = {"case": "binding-wrong-seed", "kind": "binding-tamper"}
    module = derive(seed_a, "module")
    stmt_b = derive(seed_b, "statement").strip()
    name = "ProofWrongSeed"
    el = elaborate(module, name)
    ex = export_closure(name, [THM_FULL_NAME])
    nk = nanoda_check(ex["path"])
    got = decl_type_hash(ex["path"], THM_FULL_NAME)
    exp = expected_statement_hash(stmt_b, "WrongSeed")
    rec["kernel_accepted"] = nk["accepted"]
    rec["binding_rejected"] = not (exp.get("ok") and got == exp.get("hash"))
    rec["pass"] = rec["kernel_accepted"] and rec["binding_rejected"]
    return rec


def run_export_tampers(seed: str) -> list[dict]:
    """Kernel-level tampers on the real module's export bytes."""
    module = derive(seed, "module")
    name = "ProofTamper"
    elaborate(module, name)
    ex = export_closure(name, [THM_FULL_NAME])
    with open(ex["path"]) as f:
        original = f.read()
    statement = derive(seed, "statement").strip()
    exp = expected_statement_hash(statement, "Tamper")

    def check_variant(tag, text, expect_kernel_reject, note=""):
        p = os.path.join(WORK, f"tamper-{tag}.ndjson")
        with open(p, "w") as f:
            f.write(text)
        nk = nanoda_check(p)
        got = decl_type_hash(p, THM_FULL_NAME) if nk["accepted"] else None
        binding_match = (
            nk["accepted"] and exp.get("ok") and got == exp.get("hash")
        )
        p_stage_accepted = nk["accepted"] and binding_match
        return {
            "case": f"tamper-{tag}",
            "kind": "export-tamper",
            "note": note,
            "kernel_accepted": nk["accepted"],
            "kernel_rc": nk["rc"],
            "expect_kernel_reject": expect_kernel_reject,
            "p_stage_accepted": p_stage_accepted,
            # every tamper must be P-stage rejected
            "pass": not p_stage_accepted,
        }

    out = []
    # 1. flip the chain constant in the STATEMENT type (interned literal swap)
    out.append(
        check_variant(
            "stmt-literal", original.replace('"natVal":"2"', '"natVal":"3"', 1),
            expect_kernel_reject=False,
            note="statement literal swap; may stay kernel-valid, binding must catch",
        )
    )
    # 2. corrupt an app node reference inside the proof value region
    lines = original.splitlines(keepends=True)
    for i in range(len(lines) - 1, -1, -1):
        if '"app"' in lines[i]:
            lines[i] = lines[i].replace('"fn":', '"fn":1000000, "_x":', 1) if False else lines[i]
            break
    corrupted = original.replace('{"app":{"arg":', '{"app":{"arg":999999,"_":', 1)
    out.append(
        check_variant(
            "proof-node", corrupted, expect_kernel_reject=True,
            note="dangling expr reference in proof DAG",
        )
    )
    # 3. truncation (drop the final thm lines)
    trunc = "".join(original.splitlines(keepends=True)[:-3])
    out.append(
        check_variant(
            "truncate", trunc, expect_kernel_reject=False,
            note="kernel may accept remaining decls; target-present must catch",
        )
    )
    # 4. inject an unpermitted axiom declaration and use nothing
    inj = original + '{"axiom":{"name":1,"levelParams":[],"type":3}}\n'
    out.append(
        check_variant(
            "axiom-inject", inj, expect_kernel_reject=True,
            note="unpermitted axiom present in export (allowlist hard error)",
        )
    )
    return out


def run_elaboration_layer_cases() -> list[dict]:
    """Cases the pinned path rejects before/at elaboration — no export can
    exist, so the ZK path inherits the rejection by construction (no proof
    can be produced). Recorded for the comparison table."""
    cases = [
        (
            "corpus-false",
            "theorem corpus_false : 1 + 1 = 3 := by decide\n",
            "false statement fails elaboration",
        ),
        (
            "budget-heartbeats",
            "theorem corpus_burn : (List.range 400).foldl Nat.add 0 = 79800 := by decide\n",
            "exceeds pinned maxHeartbeats at elaboration",
        ),
    ]
    out = []
    for tag, src, note in cases:
        el = elaborate(src, f"Elab{tag.replace('-', '')}")
        out.append(
            {
                "case": f"elab-{tag}",
                "kind": "elaboration-layer",
                "note": note,
                "elaboration_ok": el["ok"],
                "pass": not el["ok"],  # must fail => no export => no ZK proof
            }
        )
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", default=None)
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("--extra-seeds", type=int, default=4)
    args = ap.parse_args()

    if not os.path.isdir(WORK):
        print("work/checker-export missing — copy lean/checker first", file=sys.stderr)
        return 1

    result = {
        "experiment": "povfn-phase0-a1-differential",
        "machine": os.uname().version,
        "pins": "see PINS.md",
        "budgets": {
            "maxHeartbeats": PINNED_HEARTBEATS,
            "maxRecDepth": PINNED_REC_DEPTH,
            "allowed_axioms": ALLOWED_AXIOMS,
        },
        "cases": [],
    }

    # real fixture seed + synthetic extra seeds (clearly labelled)
    result["cases"].append(run_valid_case(REAL_SEED, "Real", args.reps))
    for i in range(args.extra_seeds):
        seed = hashlib.sha256(f"povfn-a1-{i}".encode()).hexdigest()
        rec = run_valid_case(seed, f"Syn{i}", args.reps)
        rec["kind"] = "valid-synthetic"
        result["cases"].append(rec)

    seed_b = hashlib.sha256(b"povfn-a1-wrong").hexdigest()
    result["cases"].append(run_wrong_seed_binding(REAL_SEED, seed_b))
    result["cases"].extend(run_export_tampers(REAL_SEED))
    result["cases"].extend(run_elaboration_layer_cases())

    result["all_pass"] = all(
        c.get("pass", c.get("all_judges_agree", False)) for c in result["cases"]
    )

    out_path = args.out
    if out_path is None:
        fd, out_path = tempfile.mkstemp(prefix="povfn-a1-", suffix=".json")
        os.close(fd)
    with open(out_path, "w") as f:
        json.dump(result, f, indent=2)
        f.write("\n")
    print(f"[a1] all_pass={result['all_pass']} -> {out_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
