#!/usr/bin/env python3
"""Generalized submission checker for zk-registry problem packages.

Derived from the ZK-PILOT-DIFFICULTY-P2 checker, including its fix F1 (the kernel replay runs with
the package's ``--tstack`` option, on a dedicated thread of the replay tool).  Everything that was
item-specific there comes from the package's ``problem.json`` here.  One change: the theorem type is
compared modulo binder names (see ``lean/ZkReplay.lean``), because anonymous binders get hygienic names
that depend on the module name and on declarations above the theorem.

Usage::

    PYTHONDONTWRITEBYTECODE=1 python3 -m zk_registry.check <package-dir> <proof-file> --lean-env ENV.json
        [--workdir DIR] [--report FILE] [--timeout SECS] [--keep] [--work-root DIR]

Verdicts (exit code): PASS (0) | FAIL (1: does not elaborate, errors, ``sorry``) | INVALID (2: forbidden
construct, disallowed axiom, changed statement text or imports, modified imported file, type mismatch,
failed kernel replay) | ERROR (3: package, environment or checker problem).

Checks, in order:
  1. package and environment integrity: ``problem.json`` validates, the package files match their
     recorded sha256, the Lean environment matches the recorded pins, and the replay tool matches the
     one that recorded the reference type; copies of imported files in the solver's directory must match;
  2. text: the submission equals ``Statement.lean`` except for helper declarations inserted directly
     above the theorem and the proof replacing the ``sorry`` line;
  3. forbidden constructs in the solver-written parts (comments and strings ignored);
  4. compile (only if 1-3 are clean): the pristine model module is compiled from the package, then the
     submission as module ``ProofMod`` with ``#print axioms`` appended; no error, no ``sorry`` warning,
     printed axioms within the allowed set;
  5. kernel post-check (``lean/ZkReplay.lean``): replay of every constant, the theorem exists as a theorem,
     its elaborated type equals the reference up to binder names, its axioms are allowed, and the module
     declares no axiom
     and no unsafe constant.
"""
from __future__ import annotations

import argparse
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

from zk_registry import gates as G            # noqa: E402
from zk_registry import lean_runner as L      # noqa: E402
from zk_registry import package as P          # noqa: E402

ALLOWED_AXIOMS = {"propext", "Classical.choice", "Quot.sound"}
FORBIDDEN = [
    (r"\bsorry\b", "sorry"), (r"\badmit\b", "admit"), (r"\baxiom\b", "axiom"),
    (r"\bnative_decide\b", "native_decide"), (r"\bunsafe\b", "unsafe"), (r"\bimplemented_by\b", "implemented_by"),
    (r"\bextern\b", "extern"), (r"skipKernelTC", "debug.skipKernelTC"),
    (r"addDeclWithoutChecking", "addDeclWithoutChecking"), (r"\bofReduce(Bool|Nat)\b", "Lean.ofReduceBool/Nat"),
    (r"#exit\b", "#exit"),
]
FORBIDDEN_LABELS = [label for _, label in FORBIDDEN]
EXIT = {"PASS": 0, "FAIL": 1, "INVALID": 2, "ERROR": 3}


# ------------------------------------------------------------------------------------------ text

def parse_statement(text: str, thm: str) -> dict:
    """Split a statement file with a single ``sorry`` hole in theorem ``thm``.

    Returns dict(pre, decl, sig, body, post) with text == pre + decl + body + post; ``decl`` runs from
    the start of the theorem command (including a directly attached ``open … in`` / ``set_option … in``
    prefix, attributes and doc comment) to just after the ``:=`` that starts the proof."""
    m = list(re.finditer(r"(?m)^[ \t]*theorem[ \t]+" + re.escape(thm) + r"(?![\w.'!?])", text))
    if len(m) != 1:
        raise ValueError(f"expected exactly one `theorem {thm}` line, found {len(m)}")
    kw = m[0].start() + len(m[0].group(0)) - len(m[0].group(0).lstrip())
    s = text.find("sorry", kw)
    if s < 0:
        raise ValueError("no sorry after the theorem")
    a = text.rfind(":=", kw, s)
    if a < 0:
        raise ValueError("no := before the sorry")
    le = text.find("\n", s)
    sorry_line_end = len(text) if le < 0 else le
    start = text.rfind("\n", 0, kw) + 1
    while True:
        stripped = text[:start].rstrip()
        if stripped.endswith("-/"):
            d = stripped.rfind("/--")
            b = stripped.rfind("/-")
            if d >= 0 and d == b:
                start = stripped.rfind("\n", 0, d) + 1
                continue
            break
        line_start = stripped.rfind("\n") + 1
        last = stripped[line_start:]
        if re.match(r"^\s*(open\s.+\sin|set_option\s.+\sin|@\[.*\])\s*$", last):
            start = line_start
            continue
        break
    return dict(pre=text[:start], decl=text[start:a + 2], sig=text[kw:a], body=text[a + 2:sorry_line_end],
                post=text[sorry_line_end:])


def strip_comments_strings(src: str) -> str:
    """Remove Lean comments (line and nested block, incl. doc comments) and string literals."""
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("--", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif src.startswith("/-", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if src.startswith("/-", i):
                    depth, i = depth + 1, i + 2
                elif src.startswith("-/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            out.append(" ")
        elif c == '"' and not src.startswith("'\"'", i - 1):
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
            i += 1
            out.append('""')
        else:
            out.append(c)
            i += 1
    return "".join(out)


def text_check(orig: str, proof: str, thm: str) -> tuple[list[str], str, str]:
    """Returns (problems, helper text, proof body)."""
    st = parse_statement(orig, thm)
    probs = []
    if not proof.startswith(st["pre"]):
        a, b = st["pre"].split("\n"), proof.split("\n")
        k = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
        probs.append(f"text before the theorem (imports, definitions, comments) was changed (first difference at line {k + 1})")
        return probs, "", ""
    rest = proof[len(st["pre"]):]
    post = st["post"].rstrip()
    if post:
        r = rest.rstrip()
        if not r.endswith(post):
            probs.append("text after the proof (following the original `sorry` line) was changed")
            return probs, "", ""
        mid = r[: len(r) - len(post)]
    else:
        mid = rest
    cnt = mid.count(st["decl"])
    if cnt != 1:
        probs.append(f"theorem declaration text (prefix + signature up to `:=`) must occur exactly once unchanged; found {cnt}")
        return probs, "", ""
    k = mid.find(st["decl"])
    aux, body = mid[:k], mid[k + len(st["decl"]):]
    if not body.strip():
        probs.append("empty proof")
    return probs, aux, body


def forbidden_scan(parts: list[tuple[str, str]]) -> list[str]:
    hits = []
    for name, txt in parts:
        clean = strip_comments_strings(txt)
        for pat, label in FORBIDDEN:
            for mm in re.finditer(pat, clean):
                line = clean[: mm.start()].count("\n") + 1
                hits.append(f"{label} in {name} (line {line} of that part)")
    return hits


# ------------------------------------------------------------------------------------------ compile

def parse_printed_axioms(msgs: list[dict], fqn: str) -> list[str] | None:
    ax = [m for m in msgs if m.get("severity") == "information" and "axioms" in m.get("data", "")
          and fqn.split(".")[-1] in m.get("data", "")]
    if not ax:
        return None
    d = ax[-1]["data"]
    if "does not depend on any axioms" in d:
        return []
    mm = re.search(r"depends on axioms:\s*\[(.*)\]", d, re.S)
    return [a.strip() for a in mm.group(1).split(",")] if mm else None


def compile_and_postcheck(env: L.LeanEnv, problem: dict, pkg_dir: str, text: str, workdir: str, timeout: float) -> dict:
    chk = problem["checker"]
    fqn = chk["theorem_fqn"]
    opts = chk["lean_opts"]
    src_root = os.path.join(workdir, "src")
    build = os.path.join(workdir, "build")
    os.makedirs(src_root, exist_ok=True)
    res: dict = {"imports": []}
    for fr in chk["files"]:
        if fr["role"] != "import":
            continue
        dst = os.path.join(src_root, fr["path"])
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(os.path.join(pkg_dir, fr["path"]), dst)
        if P.sha256_file(dst) != fr["sha256"]:
            res["error"] = f"pristine copy of {fr['path']} changed while copying"
            return res
        r, msgs = L.compile_module(env, src_root, fr["path"], build, opts, timeout)
        res["imports"].append({"path": fr["path"], "rc": r.rc, "secs": r.secs, "errors": len(L.errors(msgs))})
        if r.rc != 0 or r.timeout or L.errors(msgs):
            res["error"] = f"pristine import {fr['path']} does not compile: " + "; ".join(L.fmt_msg(m) for m in L.errors(msgs)[:3])
            return res
    proof_dir = os.path.join(workdir, "proof")
    os.makedirs(proof_dir, exist_ok=True)
    src = os.path.join(proof_dir, "ProofMod.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(text + ("" if text.endswith("\n") else "\n") + f"\n#print axioms {fqn}\n")
    olean = os.path.join(proof_dir, "ProofMod.olean")
    r = L.run_lean(env, opts + ["--json", "-R", proof_dir, "-o", olean, src], proof_dir, timeout, extra_lean_path=[build])
    with open(os.path.join(proof_dir, "compile.log"), "w", encoding="utf-8") as f:
        f.write(r.out)
    msgs = L.parse_messages(r.out)
    errs = L.errors(msgs)
    res.update(compile_rc=r.rc, compile_secs=r.secs, compile_timeout=r.timeout, n_errors=len(errs),
               errors=[L.fmt_msg(m) for m in errs[:10]],
               sorry_warnings=sum(1 for m in msgs if m.get("severity") == "warning" and "sorry" in m.get("data", "")),
               printed_axioms=parse_printed_axioms(msgs, fqn))
    if r.rc != 0 or r.timeout or not os.path.exists(olean):
        res["post"] = None
        return res
    rr, post = G.run_replay(env, olean, fqn, os.path.join(proof_dir, "replay.out"), opts, [build], timeout)
    post.update(rc=rr.rc, secs=rr.secs, timeout=rr.timeout, stdout=rr.out[-2000:])
    res["post"] = post
    return res


# ------------------------------------------------------------------------------------------ main

def check(pkg_dir: str, proof_path: str, env: L.LeanEnv, workdir: str | None = None, timeout: float = 3600,
          keep: bool = False, work_root: str | None = None) -> dict:
    rep: dict = {"package": os.path.abspath(pkg_dir), "proof_file": os.path.abspath(proof_path),
                 "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    invalid: list[str] = []
    fail: list[str] = []
    error: list[str] = []
    rep.update(invalid=invalid, fail=fail, error=error)
    try:
        with open(os.path.join(pkg_dir, "problem.json"), encoding="utf-8") as f:
            problem = json.load(f)
    except (OSError, ValueError) as ex:
        error.append(f"cannot read problem.json: {ex}")
        return _finish(rep)
    perrs = P.validate_problem(problem, pkg_dir)
    if perrs:
        error += [f"package: {e}" for e in perrs[:10]]
        return _finish(rep)
    if problem["status"] not in P.PACKAGED_STATUSES:
        error.append(f"status {problem['status']} has no Lean statement")
        return _finish(rep)
    rep["status"] = problem["status"]
    chk = problem["checker"]
    rep["theorem"] = chk["theorem_fqn"]
    with open(proof_path, "rb") as f:
        rep["proof_sha256"] = hashlib.sha256(f.read()).hexdigest()

    # 1. environment integrity
    pins = env.pins()
    for key in ("lean", "mathlib", "lake_manifest_sha256"):
        if pins[key] != problem["env"][key]:
            error.append(f"environment {key} is {pins[key]}, the package pins {problem['env'][key]}")
    if P.sha256_file(G.REPLAY_TOOL) != chk["replay_tool_sha256"]:
        error.append("replay tool differs from the one that recorded the reference type")
    wd = workdir or os.path.dirname(os.path.abspath(proof_path))
    copies = 0
    for fr in chk["files"]:
        if fr["role"] != "import":
            continue
        p = os.path.join(wd, fr["path"])
        if os.path.exists(p):
            copies += 1
            if P.sha256_file(p) != fr["sha256"]:
                invalid.append(f"imported file {fr['path']} in {wd} was modified")
        elif workdir:
            invalid.append(f"imported file {fr['path']} missing from {wd}")
    rep["solver_copies_checked"] = copies

    # 2-3. text and forbidden constructs
    with open(os.path.join(pkg_dir, chk["statement_file"]), encoding="utf-8") as f:
        orig = f.read()
    with open(proof_path, encoding="utf-8") as f:
        proof = f.read()
    probs, aux, body = text_check(orig, proof, chk["theorem"])
    invalid += probs
    if not probs:
        rep["statement_signature_identical"] = True
        invalid += forbidden_scan([("helper declarations", aux), ("proof", body)])
        rep["helper_chars"], rep["proof_chars"] = len(aux), len(body)

    # 4-5. compile + kernel post-check (skipped when already INVALID or ERROR)
    if invalid or error:
        rep["compile"] = "skipped"
        return _finish(rep)
    root = work_root or os.path.join(env.scratch, "check")
    tmp = os.path.join(root, f"{os.path.basename(os.path.abspath(pkg_dir))}-{time.strftime('%Y%m%dT%H%M%S')}-{os.getpid()}")
    r = compile_and_postcheck(env, problem, pkg_dir, proof, tmp, timeout)
    rep["compile"] = {k: v for k, v in r.items() if k != "post"}
    if "error" in r:
        error.append(r["error"])
    else:
        apply_compile_result(r, chk, invalid, fail, error)
        if r.get("post") is not None:
            post = r["post"]
            rep["postcheck"] = {k: v for k, v in post.items() if k not in ("type_str", "consts", "stdout")}
            rep["postcheck"]["n_consts"] = len(post["consts"])
    rep["check_dir"] = tmp
    if not keep and not error:
        shutil.rmtree(tmp, ignore_errors=True)
        rep["check_dir"] = None
    return _finish(rep)


def apply_compile_result(r: dict, chk: dict, invalid: list[str], fail: list[str], error: list[str]) -> None:
    """Turn a compile + post-check record into verdict reasons (pure; unit-tested without Lean)."""
    if r.get("compile_timeout"):
        fail.append("compile timed out")
    if r.get("n_errors") or r.get("compile_rc") not in (0, None):
        fail.append(f"{r.get('n_errors')} error(s): " + " | ".join(r.get("errors", [])[:3]))
    if r.get("sorry_warnings"):
        fail.append("declaration uses 'sorry'")
    pa = r.get("printed_axioms")
    if pa is None and not fail:
        fail.append("`#print axioms` output missing")
    elif pa:
        if "sorryAx" in pa:
            fail.append("#print axioms lists sorryAx")
        bad = [x for x in pa if x not in ALLOWED_AXIOMS and x != "sorryAx"]
        if bad:
            invalid.append(f"#print axioms lists disallowed axioms: {bad}")
    post = r.get("post")
    if post is None:
        if r.get("compile_rc") == 0 and not r.get("compile_timeout"):
            error.append("no compiled module for the post-check")
        return
    if post.get("replay") != "ok":
        invalid.append(f"kernel replay did not succeed: {post.get('replay') or post.get('stdout', '')[-300:]}")
        return
    target = post.get("target")
    if target in (None, "missing"):
        invalid.append("theorem not found in the compiled module")
    elif target["kind"] != "theorem":
        invalid.append(f"target is a {target['kind']}, not a theorem")
    if post.get("type_sha256") != chk["reference_type_sha256"]:
        invalid.append("elaborated theorem type differs from the reference statement")
    axs = set(post.get("axioms", []))
    if "sorryAx" in axs and not any("sorry" in x for x in fail):
        fail.append("theorem depends on sorryAx")
    bad = sorted(x for x in axs if x not in ALLOWED_AXIOMS and x != "sorryAx")
    if bad:
        invalid.append(f"theorem depends on disallowed axioms: {bad}")
    for c in post.get("consts", []):
        if c["kind"] == "axiom":
            invalid.append(f"module declares axiom {c['name']}")
        if c["unsafe"]:
            invalid.append(f"module declares unsafe constant {c['name']}")


def verdict_of(rep: dict) -> str:
    return "ERROR" if rep["error"] else "INVALID" if rep["invalid"] else "FAIL" if rep["fail"] else "PASS"


def _finish(rep: dict) -> dict:
    rep["verdict"] = verdict_of(rep)
    rep["finished_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    return rep


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("package")
    ap.add_argument("proof")
    ap.add_argument("--lean-env", required=True, help="environment JSON (see lean_runner.describe_project)")
    ap.add_argument("--workdir", help="solver's copy of the package; its imported files are hash-checked")
    ap.add_argument("--report", help="write the full JSON report here")
    ap.add_argument("--timeout", type=int, default=3600)
    ap.add_argument("--keep", action="store_true", help="keep the check work directory")
    ap.add_argument("--work-root", help="parent directory for check work directories")
    a = ap.parse_args(argv)
    rep = check(a.package, a.proof, L.load_env(a.lean_env), a.workdir, a.timeout, a.keep, a.work_root)
    if a.report:
        P.write_json(a.report, rep)
    print(f"{rep['verdict']} package={a.package} theorem={rep.get('theorem')}")
    for k in ("error", "invalid", "fail"):
        for x in rep[k]:
            print(f"  {k}: {x}")
    if isinstance(rep.get("compile"), dict) and "compile_rc" in rep["compile"]:
        c = rep["compile"]
        print(f"  compile: rc={c['compile_rc']} {c['compile_secs']}s errors={c['n_errors']} "
              f"sorry_warnings={c['sorry_warnings']} #print axioms={c['printed_axioms']}")
    return EXIT[rep["verdict"]]


if __name__ == "__main__":
    sys.exit(main())
