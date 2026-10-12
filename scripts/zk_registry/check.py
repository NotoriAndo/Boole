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
import uuid

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import gates as G            # noqa: E402
from zk_registry import lean_runner as L      # noqa: E402
from zk_registry import package as P          # noqa: E402
from zk_registry.sandbox import Sandbox, SandboxUnavailable  # noqa: E402

ALLOWED_AXIOMS = {"propext", "Classical.choice", "Quot.sound"}
FORBIDDEN_V1 = [
    (r"\bsorry\b", "sorry"), (r"\badmit\b", "admit"), (r"\baxiom\b", "axiom"),
    (r"\bnative_decide\b", "native_decide"), (r"\bunsafe\b", "unsafe"), (r"\bimplemented_by\b", "implemented_by"),
    (r"\bextern\b", "extern"), (r"skipKernelTC", "debug.skipKernelTC"),
    (r"addDeclWithoutChecking", "addDeclWithoutChecking"), (r"\bofReduce(Bool|Nat)\b", "Lean.ofReduceBool/Nat"),
    (r"#exit\b", "#exit"),
]
FORBIDDEN = FORBIDDEN_V1 + [
    (r"#eval\b", "#eval"), (r"\brun_cmd\b", "run_cmd"), (r"\brun_elab\b", "run_elab"),
    (r"\brun_meta\b", "run_meta"), (r"\binitialize\b", "initialize"),
    (r"\bbuiltin_initialize\b", "builtin_initialize"), (r"@\s*\[", "elaboration-attribute"),
    (r"\btrustCompiler\b", "Lean.trustCompiler"),
]
FORBIDDEN_LABELS_V1 = [label for _, label in FORBIDDEN_V1]
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
    """Run compilation and olean analysis only inside the mandatory submission boundary."""
    return compile_in_sandbox(env, problem, pkg_dir, text, workdir, timeout)


def compile_in_sandbox(env: L.LeanEnv, problem: dict, pkg_dir: str, text: str, workdir: str,
                       timeout: float, sandbox: Sandbox | None = None) -> dict:
    """Allow a development boundary only through an explicit argument, never loaded configuration."""
    try:
        selected = sandbox or Sandbox.production()
    except SandboxUnavailable:
        return {'error': 'sandbox-unavailable'}
    reason = L.production_environment_reason(env)
    if reason and selected.mode != 'test-fixture':
        return {'error': reason}
    from zk_registry import correctness_runtime as CR
    guarded = CR.runner(os.path.join(workdir, 'processes'), timeout)
    with L.sandbox_context(selected, [pkg_dir, workdir, os.path.dirname(G.REPLAY_TOOL)], [workdir], guarded):
        result = _compile_and_postcheck(env, problem, pkg_dir, text, workdir, timeout)
    result['assurance'] = selected.mode
    return result


def _compile_and_postcheck(env: L.LeanEnv, problem: dict, pkg_dir: str, text: str, workdir: str,
                           timeout: float) -> dict:
    """Compile pristine imports and submission; this helper inherits the sandbox context."""
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
        res["imports"].append({"path": fr["path"], "rc": r.rc, "secs": r.secs, "errors": len(L.errors(msgs)),
                               "peak_rss_mb": r.peak_rss_mb})
        if r.timeout or r.memkill or r.killed_by:
            res['error'] = 'resource-unavailable'
            return res
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
               compile_memkill=r.memkill, compile_killed_by=r.killed_by, compile_peak_rss_mb=r.peak_rss_mb,
               errors=[L.fmt_msg(m) for m in errs[:10]],
               sorry_warnings=sum(1 for m in msgs if m.get("severity") == "warning" and "sorry" in m.get("data", "")),
               printed_axioms=parse_printed_axioms(msgs, fqn))
    if r.rc != 0 or r.timeout or not os.path.exists(olean):
        res["post"] = None
        return res
    rr, post = G.run_replay(env, olean, fqn, os.path.join(proof_dir, "replay.out"), opts, [build], timeout)
    post.update(rc=rr.rc, secs=rr.secs, timeout=rr.timeout, memkill=rr.memkill,
                killed_by=rr.killed_by, peak_rss_mb=rr.peak_rss_mb, stdout=rr.out[-2000:])
    res["post"] = post
    return res


# ------------------------------------------------------------------------------------------ main

def check(pkg_dir: str, proof_path: str, env: L.LeanEnv, workdir: str | None = None, timeout: float = 3600,
          keep: bool = False, work_root: str | None = None, *, policy_fixture: dict | None = None,
          sandbox: Sandbox | None = None, tools=None, conformance_fixture: dict | None = None) -> dict:
    rep: dict = {"package": os.path.abspath(pkg_dir), "proof_file": os.path.abspath(proof_path),
                 "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    invalid: list[str] = []
    fail: list[str] = []
    error: list[str] = []
    rep.update(invalid=invalid, fail=fail, error=error)
    rep['assurance'] = 'test-fixture' if policy_fixture is not None else 'production'
    if sandbox is not None and policy_fixture is None and conformance_fixture is None:
        rep['assurance'] = sandbox.mode
    if conformance_fixture is not None:
        rep['assurance'] = 'test-fixture'
    try:
        with open(os.path.join(pkg_dir, "problem.json"), encoding="utf-8") as f:
            problem = json.load(f)
    except (OSError, ValueError) as ex:
        error.append(f"cannot read problem.json: {ex}")
        return _finish(rep)
    expected = L.production_descriptor() if policy_fixture is None else policy_fixture
    environment_reason = L.production_environment_reason(env, expected)
    if problem.get('env', {}).get('lean') != expected['lean']:
        environment_reason = 'environment-retired'
    if environment_reason:
        error.append(environment_reason)
        rep['reason'] = environment_reason
        rep['compile'] = 'skipped'
        return _finish(rep)
    perrs = P.validate_problem(problem, pkg_dir)
    if perrs:
        error += [f"package: {e}" for e in perrs[:10]]
        return _finish(rep)
    if not P.has_statement(problem):
        error.append(f"status {problem['status']} has no Lean statement")
        return _finish(rep)
    rep["status"] = problem["status"]
    chk = problem["checker"]
    rep["theorem"] = chk["theorem_fqn"]
    with open(proof_path, "rb") as f:
        proof_bytes = f.read()
    rep["proof_sha256"] = hashlib.sha256(proof_bytes).hexdigest()

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
    proof = proof_bytes.decode('utf-8')
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
    from zk_registry import closure as D
    from zk_registry import axiom_policy as A
    from zk_registry import acceptance as B
    acceptance = {}
    catalog_root = env.extra.get('acceptance_root')
    root_bound_candidate = (problem['status'] == 'CANDIDATE' and isinstance(catalog_root, B.RootBinding)
                            and catalog_root.package_id == problem['package_id'])
    try:
        binding = B.read(pkg_dir, problem, required=policy_fixture is None and not root_bound_candidate,
                         conformance_fixture=conformance_fixture)
        if binding is not None:
            acceptance = binding.manifest
        elif root_bound_candidate:
            rep['catalog_reference'] = B.public(catalog_root)
    except (OSError, ValueError, KeyError, TypeError) as ex:
        error.append(str(ex))
        rep['reason'] = str(ex)
        return _finish(rep)
    manifest = acceptance.get('measuring_stick_axioms', catalog_root.manifest['measuring_stick_axioms']
                              if root_bound_candidate else [])
    policy = A.preflight(manifest, conformance_fixture=conformance_fixture)
    if policy['verdict'] != 'PASS':
        invalid.append(policy['reason'])
        rep['reason'] = policy['reason']
        return _finish(rep)
    allowed = policy['allowed_axioms']
    if sandbox is None and isinstance(env.extra.get('acceptance_sandbox'), Sandbox):
        sandbox = env.extra['acceptance_sandbox']
    if tools is None and isinstance(env.extra.get('acceptance_tools'), D.Tools):
        tools = env.extra['acceptance_tools']
    try:
        selected = sandbox or (Sandbox.fixture() if policy_fixture is not None else Sandbox.production())
        rep['assurance'] = ('test-fixture' if conformance_fixture is not None or policy_fixture is not None
                            else selected.mode)
    except SandboxUnavailable:
        error.append('sandbox-unavailable')
        rep['reason'] = 'sandbox-unavailable'
        rep['compile'] = 'skipped'
        return _finish(rep)
    if tools is None and policy_fixture is None:
        error.append('kernels-unavailable')
        rep['reason'] = 'kernels-unavailable'
        rep['compile'] = 'skipped'
        return _finish(rep)
    root = work_root or os.path.join(env.scratch, "check")
    nonce = uuid.uuid4().hex
    tmp = os.path.join(root, f"{os.path.basename(os.path.abspath(pkg_dir))}-{nonce}")
    remaining = lambda: max(0.001, timeout - (time.monotonic() - started))
    started = time.monotonic()
    if tools is not None:
        sealed = os.path.join(os.path.dirname(env.scratch), 'acceptance-exports', nonce)
        reference = D.prepare_reference(env, problem, pkg_dir, tmp + '-reference',
                                        os.path.join(sealed, 'Challenge.ndjson'), tools, selected,
                                        remaining(), allowed)
        rep['reference_export'] = reference
        if reference['verdict'] != 'PASS':
            error.append(reference['reason'])
            return _finish(rep)
        if acceptance:
            if acceptance['challenge_export']['sha256'] != reference['sha256']:
                invalid.append('challenge-export-mismatch')
                return _finish(rep)
        try:
            bindings = A.resolve(env, manifest, os.path.join(tmp + '-reference', 'build'), selected, remaining())
        except A.BindingUnavailable:
            error.append('resource-unavailable')
            return _finish(rep)
        policy = A.validate(manifest, bindings, conformance_fixture=conformance_fixture)
        rep['axiom_policy'] = policy
        if policy['verdict'] != 'PASS':
            invalid.append(policy['reason'])
            return _finish(rep)
    r = compile_in_sandbox(env, problem, pkg_dir, proof, tmp, remaining(), selected)
    rep["compile"] = {k: v for k, v in r.items() if k != "post"}
    if "error" in r:
        error.append(r["error"])
    else:
        apply_compile_result(r, chk, invalid, fail, error, allowed_axioms=set(allowed))
        if r.get("post") is not None:
            post = r["post"]
            rep["postcheck"] = {k: v for k, v in post.items() if k not in ("type_str", "consts", "stdout")}
            rep["postcheck"]["n_consts"] = len(post["consts"])
    if tools is not None and not (invalid or fail or error):
        exported = D.export_module(env, 'ProofMod', chk['theorem_fqn'], os.path.join(sealed, 'Solution.ndjson'),
                                   tools, selected, os.path.join(tmp, 'proof'),
                                   [os.path.join(tmp, 'proof'), os.path.join(tmp, 'build')],
                                   remaining(), allowed)
        rep['solution_export'] = exported
        if exported['verdict'] != 'PASS':
            error.append(exported['reason'])
        else:
            compared = D.compare_exports(reference['path'], exported['path'], chk['theorem_fqn'], tools, selected,
                                         os.path.join(sealed, 'kernels'), remaining(), allowed)
            rep['two_kernels'] = compared
            if compared['verdict'] != 'PASS':
                (error if compared['verdict'] == 'ERROR' else invalid).append(compared['reason'])
    rep["check_dir"] = tmp
    if not keep and not error:
        shutil.rmtree(tmp, ignore_errors=True)
        rep["check_dir"] = None
    return _finish(rep)


def apply_compile_result(r: dict, chk: dict, invalid: list[str], fail: list[str], error: list[str], *,
                         allowed_axioms: set[str] | None = None) -> None:
    """Turn a compile + post-check record into verdict reasons (pure; unit-tested without Lean)."""
    from zk_registry import axiom_policy as A
    permitted = ALLOWED_AXIOMS if allowed_axioms is None else allowed_axioms
    if r.get("compile_timeout"):
        error.append('resource-unavailable')
        return
    if r.get('compile_memkill') or r.get('compile_killed_by'):
        error.append('resource-unavailable')
        return
    if any('maximum number of heartbeats' in text.lower() or 'maxheartbeats' in text.lower()
           for text in r.get('errors', [])):
        invalid.append('resource-cap')
        return
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
        bad = [x for x in pa if x != 'sorryAx' and (x not in permitted or A.native_evaluator_axiom(x))]
        if bad:
            invalid.append(f"#print axioms lists disallowed axioms: {bad}")
    post = r.get("post")
    if post and (post.get('timeout') or post.get('memkill') or post.get('killed_by')):
        error.append('resource-unavailable')
        return
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
    bad = sorted(x for x in axs if x != 'sorryAx' and (x not in permitted or A.native_evaluator_axiom(x)))
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
    ap.add_argument('--tools-root', help='directory containing the exact pinned Comparator/exporter/nanoda tools')
    ap.add_argument('--dev-unsandboxed', action='store_true', help='macOS test-only pipeline, never production evidence')
    a = ap.parse_args(argv)
    selected, tools = None, None
    from zk_registry import closure as D
    try:
        if a.dev_unsandboxed:
            selected = Sandbox.development()
        if a.tools_root:
            tools = D.Tools.load(a.tools_root)
            if selected is None:
                selected = Sandbox.production(tools.landrun)
        rep = check(a.package, a.proof, L.load_env(a.lean_env), a.workdir, a.timeout, a.keep, a.work_root,
                    sandbox=selected, tools=tools)
    except (SandboxUnavailable, D.ToolsUnavailable) as ex:
        rep = _finish(dict(error=[str(ex)], invalid=[], fail=[], reason=str(ex), compile='skipped'))
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
