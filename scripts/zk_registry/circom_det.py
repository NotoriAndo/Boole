#!/usr/bin/env python3
"""Circom DET problem generator: wave driver.

Subcommands::

    fetch-circom  --version v2.2.3 --asset macos-amd64 --dest DIR     download + sha256-verify a release binary
                                                                       (v2.2.3, v2.1.9; v2.0.9 is a pinned source
                                                                       build, circom 1 a pinned npm install)
    lean-env      --project DIR --toolchain DIR --scratch DIR --out F  describe a built lake project
    wave          --config wave.json [--config ...] [--jobs N]         generate, gate and package repositories
                                                                       (one shared pool of template workers)
    validate      --index INDEX.jsonl --packages DIR [--collections-root]
                                                                       re-validate every record and package

The ``wave`` configuration names the pinned repository checkout, the circom binary, the Lean
environment, the ledger, and the output directory; optionally the repository's library paths
(``circom -l``), per-path prime / library rules, the materialized library packages (recorded in
``ids.dependencies``), a ledger id prefix, a template subset (``only``) and the compile guard.
Further pinned compilers (``circoms``: circom 2.0.x / 2.1.x lines and circom 1) are chosen per compilation
by the include closure's ``pragma circom`` (:func:`compiler_order`).  All tool state goes to the configured
work directory.  The driver never writes a proof: the only
Lean proofs attempted are the automatic G-TRIV battery runs, whose closures fail the gate.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import copy
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C                # noqa: E402
from zk_registry import circom_source as cs       # noqa: E402
from zk_registry import content as CT             # noqa: E402
from zk_registry import decompose as DC           # noqa: E402
from zk_registry import det_search                # noqa: E402
from zk_registry import gates as G                # noqa: E402
from zk_registry import instantiation as I        # noqa: E402
from zk_registry import lean_emit as E            # noqa: E402
from zk_registry import lean_runner as L          # noqa: E402
from zk_registry import package as P              # noqa: E402
from zk_registry import r1cs as R                 # noqa: E402
from zk_registry import tags as TG                # noqa: E402
from zk_registry import witness as W              # noqa: E402

# Pinned circom compilers, one per language line.  The driver refuses any binary (or, for circom 1,
# installed npm tree) whose sha256 is not listed here.  ``line`` is the ``pragma circom`` line the
# compiler serves (see :func:`compiler_order`).
CIRCOM_RELEASES = {
    "v2.2.3": {
        "line": (2, 2), "kind": "circom2",
        "commit": "ad44e915a12bb047b05745c2884aad9cc8326bc6",
        "url": "https://github.com/iden3/circom/releases/download/v2.2.3/circom-{asset}",
        "sha256": {
            "linux-amd64": "85342c7ff332d948df7c0c50ecf201e6129349aef550ce873f3c811b79fe53a3",
            "macos-amd64": "e006332b3fe225f11c3b87bd2debbf5d7f568d6efbde25e5a6a12cd6988c8ecb",
            "windows-amd64.exe": "e43f132ee6f0aa79b705beceb59c2a7e6a54d7bdeab917ca34e9fc1951d185e1",
        },
        "digest_source": "sha256 digests published on the GitHub release page",
    },
    "v2.1.9": {
        "line": (2, 1), "kind": "circom2",
        "commit": "2eaaa6dface934356972b34cab64b25d382e59de",
        "url": "https://github.com/iden3/circom/releases/download/v2.1.9/circom-{asset}",
        "sha256": {"macos-amd64": "5c7dedaec105844dd90dc42c1ba9d7f67c265c5692fb3467465285fc09177e9f"},
        "digest_source": "sha256 of the release asset at its first anonymous download (the release page publishes "
                         "no digest for v2.1.9); the macos asset is an arm64 Mach-O and was cross-checked against a "
                         "source build of the tag commit (identical R1CS on the cross-check circuits)",
    },
    "v2.0.9": {
        "line": (2, 0), "kind": "circom2",
        "commit": "bdc9d9d57490f113161f3adf818120f064b7b5b2",
        "url": None,
        "sha256": {"macos-arm64-source": "efa7adf102ae6c266103d04a6fec08d89e55ca8675932eb46993bad8f70c2d70"},
        "source_build": {"cargo_lock_sha256_at_tag": "628d4786acea7c3e68f3267dd47ddb803a28b118ef3923591ded7a6b5f36776e",
                         "cargo_lock_sha256_built": "8e3343e0f9a62ec1aa5145c2293a186c9f95a12ab4da27ebc11e5b8c2891835f",
                         "rustc": "1.60.0", "command": "cargo build --release --locked -p circom"},
        "digest_source": "source build of the release tag commit: the v2.0.x macos release assets are x86_64 "
                         "binaries and the build host has no x86_64 translation; the tag's Cargo.lock differs from "
                         "its Cargo.toml only in the versions of 7 workspace crates (refreshed with `cargo update "
                         "--workspace --offline`), every registry crate is built at its locked version and checksum; "
                         "the digest pins this build",
    },
    "v0.5.46": {
        "line": (1, 0), "kind": "circom1",
        "npm": "circom@0.5.46",
        "tarball_integrity": "sha512-clvfqJudyBlHAubTu4dKY04dVgst8OxGS7SAxdbXKbGO2c6XGOzP2TSygNUmYHanLDvUgJpOqQYe/AkLt9x/1g==",
        "tarball_sha256": "4a30cb13f07a1d61bf7bd3e801e1b77ddc77bfcc7e0007401ae5a048645b09bb",
        # the npm install tree (``npm install --ignore-scripts circom@0.5.46``) is pinned by its lockfile,
        # which carries the registry integrity of all 136 packages (circom, circom_runtime, ffjavascript, ...)
        "sha256": {"package-lock": "0133311ef0bd412d202c7fbc0957753370c4a70119168510ec0f52dc0f1e96bd"},
        "cli_sha256": "f2b1f22302b66fe308a339bc10d1086d04175d6a1dc1033fb551719c7c81b96d",
        "digest_source": "npm registry tarball integrity (sha512) of circom 0.5.46, the last circom 1 release; the "
                         "installed tree is pinned by the sha256 of its package-lock.json",
    },
}
SIZE_FLAGS = ["--r1cs", "--O0"]          # the constraint count needs no .sym (wave 0 also wrote --sym)
CIRCOM_FLAGS = ["--r1cs", "--sym", "--wasm", "--O0"]
# circom 1 (npm circom 0.5.x): ``-f`` = "Do not optimize constraints" (the --O0 counterpart)
CIRCOM1_SIZE_FLAGS = ["-f", "-r", "main.r1cs"]
CIRCOM1_FLAGS = ["-f", "-r", "main.r1cs", "-s", "main.sym", "-w", "main.wasm"]
DEFAULT_COMPILER = "v2.2.3"
PROBED_NOT_A_FINDING = ("not-a-finding: probed instantiation (parameter values chosen inside the template's own "
                        "assert bounds, not taken from the repository)")
CIRCOM1_TAG = "v0.5.46"
_PRAGMA_RE = re.compile(r"\bpragma\s+circom\s+(\d+)\.(\d+)\.(\d+)\s*;")
REAL_WANTED = 12
MUTANTS = 12
DET_SEARCH_POOL = 200
SAMPLE_ATTEMPTS = 600


# ------------------------------------------------------------------------------------------ toolchains

def sha256_file(path: str) -> str:
    return P.sha256_file(path)


def fetch_circom(version: str, asset: str, dest_dir: str) -> str:
    """Download a pinned circom release asset (anonymous request) and verify its published sha256."""
    rel = CIRCOM_RELEASES[version]
    want = rel["sha256"][asset]
    os.makedirs(dest_dir, exist_ok=True)
    dest = os.path.join(dest_dir, f"circom-{version}-{asset}")
    if not (os.path.exists(dest) and sha256_file(dest) == want):
        tmp = dest + ".part"
        with urllib.request.urlopen(rel["url"].format(asset=asset), timeout=300) as resp, open(tmp, "wb") as f:
            shutil.copyfileobj(resp, f)
        got = sha256_file(tmp)
        if got != want:
            os.remove(tmp)
            raise ValueError(f"sha256 mismatch for circom {version} {asset}: {got} != {want}")
        os.replace(tmp, dest)
    os.chmod(dest, 0o755)
    return dest


def verify_compiler(tag: str, path: str) -> str:
    """The pinned digest of compiler ``tag`` at ``path`` (a binary, or for circom 1 the npm install root);
    raises ValueError for an unknown tag or a digest that is not pinned."""
    rel = CIRCOM_RELEASES.get(tag)
    if rel is None:
        raise ValueError(f"unknown circom compiler {tag}")
    if rel["kind"] == "circom1":
        with open(os.path.join(path, "node_modules", "circom", "package.json"), encoding="utf-8") as f:
            version = json.load(f).get("version")
        got = sha256_file(os.path.join(path, "package-lock.json"))
        if f"v{version}" != tag or got not in rel["sha256"].values():
            raise ValueError(f"circom {tag} install at {path} does not match the pinned package-lock digest")
        return got
    got = sha256_file(path)
    if got not in rel["sha256"].values():
        raise ValueError(f"circom {tag} binary {path} does not match a pinned digest")
    return got


def closure_pragmas(files: dict, include_rel: str) -> list[tuple[int, int, int]]:
    """``pragma circom X.Y.Z`` versions declared in the include closure of ``include_rel`` (comments ignored)."""
    out = []
    for p in cs.include_closure(files, include_rel):
        out += [tuple(int(x) for x in m.groups()) for m in _PRAGMA_RE.finditer(files[p].clean)]
    return out


def compiler_order(pragmas: list[tuple[int, int, int]], available) -> list[str]:
    """Compilers to try, in order, for an include closure with these ``pragma circom`` versions.

    The newest declared version selects its language line: the pinned release of that line first, then the
    pinned releases of the newer lines (2.0 -> v2.0.9, v2.1.9, v2.2.3; 2.1 -> v2.1.9, v2.2.3; 2.2 or newer ->
    v2.2.3).  Without any pragma, circom 2's documented default (the latest compiler) comes first and
    circom 1 second, since pre-2.0 sources carry no pragma and circom 2 rejects their grammar.  Only the
    ``available`` (configured) compilers are returned; a wave configured with v2.2.3 alone behaves as before."""
    lines = sorted((rel["line"], tag) for tag, rel in CIRCOM_RELEASES.items() if rel["kind"] == "circom2")
    if pragmas:
        top = max(pragmas)[:2]
        order = [tag for line, tag in lines if line >= top] or [lines[-1][1]]
    else:
        order = [lines[-1][1], CIRCOM1_TAG]
    return [t for t in order if t in available]


def compile_flags(kind: str, full: bool) -> list[str]:
    if kind == "circom1":
        return list(CIRCOM1_FLAGS if full else CIRCOM1_SIZE_FLAGS)
    return list(CIRCOM_FLAGS if full else SIZE_FLAGS)


def tool_version(cmd: list[str]) -> str:
    return subprocess.run(cmd, capture_output=True, text=True, timeout=60).stdout.strip()


def git_head(repo_dir: str) -> str:
    return L._git_head(repo_dir)


# ------------------------------------------------------------------------------------------ config

@dataclass
class WaveConfig:
    repo_dir: str
    repo_id: str
    repo_url: str
    release: str
    commit: str
    collection: str                       # e.g. "circomlib-v2.0.5"
    scope_prefix: str
    exclude: list[str]
    circom: str
    circom_version_tag: str
    lean_env: str
    node: str
    work: str
    out: str
    ledger: str | None = None
    jobs: int = 6
    max_constraints: int = P.MAX_CONSTRAINTS
    battery_heartbeats: int = 200000
    battery_file_wall: float = 900
    battery_single_wall: float = 180
    det_search_budget_s: float = 20.0
    only: list[str] = field(default_factory=list)
    # library search paths (circom -l), repository-relative
    include_paths: list[str] = field(default_factory=list)
    # [{"prefix": "circuits.gl/", "prime": "goldilocks", "include_paths": [...], "source": "..."}]: the
    # first rule whose prefix matches the template's path overrides the prime and the library paths
    path_rules: list[dict] = field(default_factory=list)
    # materialized library packages (recorded in ids.dependencies)
    dependencies: list[dict] = field(default_factory=list)
    # ledger item id prefix when it is not "<repo_id>:" (census ids such as "pil-stark:CC/<path>#<T>")
    ledger_item_prefix: str | None = None
    # resource guard for circom runs: a compile above this RSS, wall time or output size is stopped and recorded
    compile_rss_mb: int = 12288
    compile_timeout_s: float = 900
    compile_output_mb: int = 1536
    # total sizing wall time per template; later candidates are recorded as skipped
    sizing_budget_s: float = 2400
    # keep per-template work directories after the record is written (wave 0 kept them)
    keep_work: bool = True
    # further pinned compilers by tag (``v2.0.9``, ``v2.1.9``: binaries; ``v0.5.46``: the npm install root
    # of circom 1); ``circom`` / ``circom_version_tag`` stay the default compiler.  The compiler of each
    # compilation follows the include closure's ``pragma circom`` (:func:`compiler_order`).
    circoms: dict = field(default_factory=dict)
    # the ``probed`` instantiation tier (template asserts bound every parameter; results are not findings)
    probe: bool = True

    @staticmethod
    def load(path: str) -> "WaveConfig":
        with open(path, encoding="utf-8") as f:
            return WaveConfig(**json.load(f))


@dataclass
class Compiler:
    tag: str                              # CIRCOM_RELEASES key
    kind: str                             # "circom2" | "circom1"
    path: str                             # binary, or the npm install root of circom 1
    version: str                          # as reported by the compiler
    sha256: str                           # the pinned digest it matched

    def command(self, node: str) -> list[str]:
        if self.kind == "circom1":
            return [node, os.path.join(self.path, "node_modules", "circom", "cli.js")]
        return [self.path]

    def source(self) -> str:
        rel = CIRCOM_RELEASES[self.tag]
        if self.kind == "circom1":
            return (f"npm {rel['npm']} (tarball {rel['tarball_integrity'][:23]}…, install pinned by its "
                    f"package-lock.json sha256)")
        how = "release binary" if rel.get("url") else "source build of the release tag"
        return f"iden3/circom {self.tag} {how} (commit {rel['commit']})"


@dataclass
class Shared:
    cfg: WaveConfig
    env: L.LeanEnv
    files: dict
    ledger_ids: set
    ledger_sha256: str | None
    circom_sha256: str
    circom_version: str
    node_version: str
    generator: dict
    compilers: dict = field(default_factory=dict)       # tag -> Compiler
    functions: set = field(default_factory=set)         # circom function names of the scanned files

    def env_record(self, compiler: "Compiler | None" = None) -> dict:
        pins = self.env.pins()
        return {"circom": compiler.version if compiler else self.circom_version,
                "circom_binary_sha256": compiler.sha256 if compiler else self.circom_sha256, "lean": pins["lean"],
                "mathlib": pins["mathlib"], "lake_manifest_sha256": pins["lake_manifest_sha256"],
                "packages": pins["packages"], "node": self.node_version, "python": platform.python_version()}

    def default_compiler(self) -> "Compiler":
        return self.compilers.get(self.cfg.circom_version_tag) or Compiler(
            self.cfg.circom_version_tag, "circom2", self.cfg.circom, self.circom_version, self.circom_sha256)


# ------------------------------------------------------------------------------------------ compile

def include_contexts(files: dict, target_path: str) -> list[str]:
    """The template's own file, then library files whose include closure reaches it (nearest first),
    then non-test files with their own ``component main`` that reach it (compiled through their
    main-free copy; some repositories include shared definitions only from their top-level circuits)."""
    out = [target_path]
    ranked = []
    for path, sf in files.items():
        if path == target_path or path.startswith("test/"):
            continue
        closure = cs.include_closure(files, path)
        if target_path in closure:
            ranked.append((sf.is_harness, closure.index(target_path), len(closure), path))
    out += [p for *_, p in sorted(ranked)]
    return out


def build_options(cfg: WaveConfig, rel_path: str) -> tuple[str | None, list[str], str | None]:
    """(prime or None for circom's default, library paths, rule source) for a template file."""
    for rule in cfg.path_rules:
        if rel_path.startswith(rule["prefix"]):
            return (rule.get("prime"), list(rule.get("include_paths", cfg.include_paths)),
                    rule.get("source") or rule["prefix"])
    return None, list(cfg.include_paths), None


def option_flags(prime: str | None, libs: list[str], root: str | None = None) -> list[str]:
    """``--prime`` / ``-l`` flags; library paths are made absolute against ``root`` when given."""
    out = ["--prime", prime] if prime else []
    for lib in libs:
        out += ["-l", os.path.normpath(os.path.join(root, lib)) if root else lib]
    return out


def main_free_copy(sh: Shared, include_rel: str) -> str | None:
    """A sibling copy of ``include_rel`` with its ``component main`` blanked, if it declares one."""
    sf = sh.files.get(include_rel)
    if sf is None or not sf.mains:
        return None
    src = os.path.join(sh.cfg.repo_dir, include_rel)
    dest = src + ".boole-nomain"               # not *.circom: never scanned as a repository file
    text = cs.blank_mains(sf.text)
    current = None
    if os.path.exists(dest):
        with open(dest, encoding="utf-8") as f:
            current = f.read()
    if current != text:
        tmp = f"{dest}.{os.getpid()}.{threading.get_ident()}"
        with open(tmp, "w", encoding="utf-8") as f:
            f.write(text)
        os.replace(tmp, dest)
    return dest


_CUSTOM_PRAGMA_RE = re.compile(r"\bpragma\s+custom_templates\s*;")


def uses_custom_templates_pragma(files: dict, include_rel: str) -> bool:
    """Whether a file in the include closure declares ``pragma custom_templates`` (circom then
    requires the pragma in the main file as well)."""
    return any(_CUSTOM_PRAGMA_RE.search(files[p].clean) for p in cs.include_closure(files, include_rel))


def output_guard(workdir: str, limit_mb: int) -> str:
    """Non-empty when the files under ``workdir`` exceed ``limit_mb`` (a compile writing a huge R1CS)."""
    total = 0
    for root, _, names in os.walk(workdir):
        for n in names:
            try:
                total += os.path.getsize(os.path.join(root, n))
            except OSError:
                pass
    return f"compiler output > {limit_mb} MB" if total > limit_mb * 1024 * 1024 else ""


CIRCOM1_PRIMES = {"bn128": None, "bls12381": "BLS12381"}      # circom 1 `-p` names (default: bn128)


def compile_main(sh: Shared, workdir: str, include_rel: str, template: str, args: tuple[str, ...],
                 full: bool, rule_path: str | None = None, compiler: Compiler | None = None,
                 tag_template: cs.Template | None = None) -> dict:
    """Compile ``template(args)`` as the main component (``tag_template``: the template's inputs carry known
    tags; it is compiled through the untagged wrapper of :func:`tags.wrapper_source`)."""
    compiler = compiler or sh.default_compiler()
    os.makedirs(workdir, exist_ok=True)
    prime, libs, _ = build_options(sh.cfg, rule_path or include_rel)
    nomain = main_free_copy(sh, include_rel)
    custom = uses_custom_templates_pragma(sh.files, include_rel)
    pragma = None if compiler.kind == "circom1" else "2.0.0"
    comment = f"compiled with the component main declaration of {include_rel} blanked" if nomain else None
    main = os.path.join(workdir, "main.circom")
    if tag_template is not None:
        closure = max(closure_pragmas(sh.files, include_rel), default=TG.TAG_PRAGMA)
        try:
            main_text = TG.wrapper_source(nomain or os.path.join(sh.cfg.repo_dir, include_rel), tag_template, args,
                                          closure, sh.functions, custom_templates=custom)
            portable = TG.wrapper_source(include_rel, tag_template, args, closure, sh.functions, comment=comment,
                                         custom_templates=custom)
        except TG.TagError as exc:
            return {"rc": None, "secs": 0.0, "include_context": include_rel, "compiler": compiler.tag, "flags": [],
                    "main_removed": bool(nomain), "workdir": workdir, "error": f"tag wrapper: {exc}", "tag_error": True,
                    "main_sha256": "0" * 64}
    else:
        main_text = I.main_source(nomain or os.path.join(sh.cfg.repo_dir, include_rel), template, args, pragma=pragma,
                                  custom_templates=custom)
        # the recorded main names the include relative to the repository root (no local paths)
        portable = I.main_source(include_rel, template, args, pragma=pragma, comment=comment, custom_templates=custom)
    with open(main, "w", encoding="utf-8") as f:
        f.write(main_text)
    with open(os.path.join(workdir, "main.portable.circom"), "w", encoding="utf-8") as f:
        f.write(portable)
    flags = compile_flags(compiler.kind, full)
    env = dict(os.environ)
    if compiler.kind == "circom1":
        # circom 1 resolves includes relative to the including file only (no library paths)
        if prime not in (None, *CIRCOM1_PRIMES):
            return {"rc": None, "secs": 0.0, "include_context": include_rel, "compiler": compiler.tag,
                    "flags": flags, "main_removed": bool(nomain), "workdir": workdir,
                    "main_sha256": hashlib.sha256(portable.encode("utf-8")).hexdigest(),
                    "error": f"circom 1 has no prime `{prime}`"}
        opt = ["-p", CIRCOM1_PRIMES[prime]] if CIRCOM1_PRIMES.get(prime) else []
        cmd = compiler.command(sh.cfg.node) + ["main.circom", *flags, *opt]
        recorded = [*flags, *opt]
        env.update(PATH=os.path.dirname(sh.cfg.node) + ":/usr/bin:/bin", NODE_OPTIONS="--max-old-space-size=16384")
    else:
        cmd = compiler.command(sh.cfg.node) + ["main.circom", *flags, *option_flags(prime, libs, sh.cfg.repo_dir), "-o", "."]
        recorded = [*flags, *option_flags(prime, libs)]
    run = L.run_process(cmd, env, workdir, sh.cfg.compile_timeout_s, sh.cfg.compile_rss_mb,
                        watch=lambda: output_guard(workdir, sh.cfg.compile_output_mb))
    log, rc = run.out, run.rc
    guard = None
    if run.timeout or run.memkill or run.killed_by:
        what = (run.killed_by or (f"RSS > {sh.cfg.compile_rss_mb} MB" if run.memkill else
                                  f"wall time > {sh.cfg.compile_timeout_s:g} s"))
        guard = f"stopped by the resource guard ({what})"
        log, rc = f"circom {guard}\n" + log, -1
    log = re.sub(r"\x1b\[[0-9;]*m", "", log)
    with open(os.path.join(workdir, "circom.log"), "w", encoding="utf-8") as f:
        f.write(log)
    res = {"rc": rc, "secs": run.secs, "peak_rss_mb": run.peak_rss_mb, "include_context": include_rel,
           "main_sha256": hashlib.sha256(portable.encode("utf-8")).hexdigest(), "workdir": workdir,
           "flags": recorded, "main_removed": bool(nomain), "compiler": compiler.tag}
    if compiler.kind == "circom1":
        res.update(witness_js=os.path.join(compiler.path, "node_modules", "circom_runtime"),
                   wasm=os.path.join(workdir, "main.wasm"))
    else:
        res.update(witness_js=os.path.join(workdir, "main_js", "witness_calculator.js"),
                   wasm=os.path.join(workdir, "main_js", "main.wasm"))
    r1cs_path = os.path.join(workdir, "main.r1cs")
    if guard:
        res["guard"] = guard
        res["error"] = guard
        return res
    if rc != 0 or not os.path.exists(r1cs_path):
        err = [ln.strip() for ln in log.splitlines() if "error" in ln.lower() or "Calling" in ln or "unknown" in ln]
        res["error"] = " | ".join(err[:4])[:400] or log[-400:]
        return res
    hdr = R.read_header(r1cs_path)
    if hdr.custom_gate_uses:
        # PLONK custom gates are not part of the R1CS constraints; a model of the R1CS alone would
        # be incomplete, so no size is reported and the candidate cannot be packaged
        res["error"] = (f"the instantiation uses {hdr.custom_gate_uses} circom custom gate application(s) "
                        "(PLONK custom templates), which the R1CS model cannot represent")
        res["custom_gates"] = hdr.custom_gate_uses
        return res
    res.update(constraints=hdr.n_constraints, wires=hdr.n_wires, r1cs=r1cs_path, sym=os.path.join(workdir, "main.sym"),
               r1cs_sha256=sha256_file(r1cs_path))
    return res


def size_candidates(sh: Shared, plan: I.TemplatePlan, dir_base: str,
                    wrapper: bool = False) -> tuple[str | None, list[dict]]:
    """Compile candidates tier by tier; returns (tier used, candidate records).

    The first tier with a compiled candidate, or with a candidate stopped by the resource guard
    (compiling, but too large to finish), is used.  After ``sizing_budget_s`` the remaining
    candidates are recorded as skipped.  ``wrapper``: compile through the tag wrapper (circom >= 2.1)."""
    t = plan.template
    records_all = []
    t0 = time.time()
    for tier, cands in plan.tiers_in_order():
        records = []
        for c in cands[:I.MAX_DERIVED_PER_TEMPLATE]:
            slug = P.args_slug(c.args) or "noargs"
            rec = {"tier": tier, "call": f"{t.name}{c.call}", "args": list(c.args), "provenance": c.provenance}
            if time.time() - t0 > sh.cfg.sizing_budget_s:
                rec["compile_result"] = {"rc": None, "include_context": t.path,
                                         "skipped": f"sizing budget of {sh.cfg.sizing_budget_s:g} s exhausted"}
                records.append(rec)
                continue
            attempts = []
            for ctx in include_contexts(sh.files, t.path):
                available = sh.compilers or [sh.cfg.circom_version_tag]
                order = compiler_order(closure_pragmas(sh.files, ctx), available) or [sh.cfg.circom_version_tag]
                if wrapper:
                    order = [x for x in order if CIRCOM_RELEASES[x]["line"] >= TG.TAG_PRAGMA[:2]] or order[-1:]
                for tag in order:
                    comp = sh.compilers.get(tag) or sh.default_compiler()
                    res = compile_main(sh, os.path.join(dir_base, "size", slug, re.sub(r"[^A-Za-z0-9]+", "_", ctx),
                                                        tag), ctx, t.name, c.args, False, rule_path=t.path,
                                       compiler=comp, tag_template=t if wrapper else None)
                    if res.get("r1cs") and os.path.exists(res["r1cs"]):
                        os.remove(res["r1cs"])    # sizes and digest are kept; large systems would fill the disk
                    rec["compile_result"] = res
                    attempts.append({"include_context": ctx, "compiler": tag,
                                     "result": "ok" if "constraints" in res else (res.get("guard") or "error")})
                    if (res["rc"] == 0 and "constraints" in res) or res.get("guard") or res.get("custom_gates"):
                        break                     # compiled (or stopped by the guard / custom gates)
                else:
                    continue
                break
            rec["compile_result"]["attempts"] = attempts
            records.append(rec)
        records_all += records
        if any("constraints" in r["compile_result"] or r["compile_result"].get("guard") for r in records):
            return tier, records_all
    return None, records_all


def select(records: list[dict], tier: str, max_constraints: int) -> tuple[dict, bool]:
    ok = [r for r in records if r["tier"] == tier and "constraints" in r["compile_result"]]
    fit = [r for r in ok if r["compile_result"]["constraints"] <= max_constraints]
    if fit:
        return max(fit, key=lambda r: (r["compile_result"]["constraints"], -r["compile_result"]["wires"],
                                       _neg_text(r["call"]))), True
    return min(ok, key=lambda r: (r["compile_result"]["constraints"], r["compile_result"]["wires"], r["call"])), False


def _neg_text(s: str) -> tuple:
    return tuple(-ord(ch) for ch in s)


def candidate_summary(records: list[dict]) -> list[dict]:
    out = []
    for r in records:
        cr = r["compile_result"]
        row = {"tier": r["tier"], "call": r["call"][:300],
               "compile": "ok" if "constraints" in cr else "skipped" if cr.get("skipped") else "error",
               "include_context": cr["include_context"]}
        if cr.get("compiler"):
            row["compiler"] = cr["compiler"]
        if len(cr.get("attempts") or []) > 1:
            row["attempts"] = cr["attempts"][:12]
        if "constraints" in cr:
            row.update(constraints=cr["constraints"], wires=cr["wires"])
        else:
            row["error"] = (cr.get("skipped") or cr.get("error", ""))[:300]
        out.append(row)
    return out


# ------------------------------------------------------------------------------------------ one template

def base_record(sh: Shared, t: cs.Template, dir_name: str) -> dict:
    cfg = sh.cfg
    item_id = f"{ledger_prefix(cfg)}{t.path}#{t.name}"
    ids = {"ledger_item_id": item_id, "repo": cfg.repo_id, "repo_url": cfg.repo_url, "release": cfg.release,
           "commit": cfg.commit, "path": t.path, "template": t.name, "template_line": t.line,
           "source_sha256": sha256_file(os.path.join(cfg.repo_dir, t.path))}
    if cfg.dependencies:
        ids["dependencies"] = cfg.dependencies
    if cfg.ledger:
        ids["ledger"] = {"file": os.path.basename(cfg.ledger), "sha256": sh.ledger_sha256,
                         "row_found": item_id in sh.ledger_ids}
    return {"schema_version": P.SCHEMA_VERSION, "package_id": f"{cfg.collection}/{dir_name}",
            "property": dict(P.DET_PROPERTY), "ids": ids, "spec": dict(P.DET_SPEC),
            "env": sh.env_record(), "generator": sh.generator}


def compiler_of(sh: Shared, cr: dict) -> Compiler:
    return sh.compilers.get(cr.get("compiler")) or sh.default_compiler()


def circuit_record(sh: Shared, r: R.R1cs | None, cr: dict, n_in=None, n_out=None, sym_sha=None) -> dict:
    comp = compiler_of(sh, cr)
    rec = {"compiler": {"name": "circom", "version": comp.version,
                        "flags": cr.get("flags") or (CIRCOM_FLAGS if r else SIZE_FLAGS),
                        "binary_sha256": comp.sha256, "source": comp.source()},
           "prime": str(r.prime if r else R.BN254_SCALAR),
           "prime_name": (r.prime_name if r else "bn128") or "unknown",
           "n_constraints": cr["constraints"], "n_wires": cr["wires"],
           "r1cs_sha256": cr["r1cs_sha256"],
           "size_policy": {"max_constraints": sh.cfg.max_constraints, "within": cr["constraints"] <= sh.cfg.max_constraints}}
    if n_in is not None:
        rec.update(n_inputs=n_in, n_outputs=n_out)
    if sym_sha:
        rec["sym_sha256"] = sym_sha
    return rec


def process_template(sh: Shared, plan: I.TemplatePlan) -> dict:
    rec = None
    try:
        rec = _process_template(sh, plan)
        return rec
    finally:
        if not sh.cfg.keep_work:          # the record and the package are written; the work tree is not needed
            t = plan.template
            dirs = {P.package_dir_name(t.path, t.name, (), sh.cfg.scope_prefix)}
            if rec is not None:
                dirs.add(rec["package_id"].split("/", 1)[1])
            for d in dirs:
                shutil.rmtree(os.path.join(sh.cfg.work, "items", d), ignore_errors=True)


def _process_template(sh: Shared, plan: I.TemplatePlan) -> dict:
    t = plan.template
    t0 = time.time()
    dir_probe = P.package_dir_name(t.path, t.name, (), sh.cfg.scope_prefix)
    work = os.path.join(sh.cfg.work, "items", dir_probe)
    rule_order = [x for x in I.TIERS]
    if not plan.candidates:
        rec = base_record(sh, t, dir_probe)
        rec.update(status="UNINSTANTIABLE", status_reason=plan.uninstantiable_reason,
                   instantiation={"rule": "none", "rule_order": rule_order, "args": [], "call": "",
                                  "params": t.params, "provenance": [], "selection": "no candidate", "candidates": []})
        return _done(rec, t0)
    pre = None
    if TG.input_tags(t):
        try:
            pre = TG.preconditions(t)
        except TG.TagError as exc:
            rec = base_record(sh, t, dir_probe)
            rec.update(status="UNINSTANTIABLE", status_reason=f"tagged inputs: {exc}"[:600],
                       instantiation={"rule": "none", "rule_order": rule_order, "args": [], "call": "",
                                      "params": t.params, "provenance": [],
                                      "selection": "not compiled: input tag without a known precondition",
                                      "candidates": []})
            return _done(rec, t0)
    tier, records = size_candidates(sh, plan, work, wrapper=pre is not None)
    if tier is None or not any("constraints" in r["compile_result"] for r in records if r["tier"] == tier):
        rec = base_record(sh, t, dir_probe)
        guarded = [r for r in records if r["compile_result"].get("guard")]
        reason = "no candidate instantiation compiles with circom (see candidates)"
        if guarded:
            reason = (f"no candidate instantiation compiles within the resource guard: {len(guarded)} of "
                      f"{len(records)} candidate compiles were {guarded[0]['compile_result']['guard']} (size "
                      f"unknown; decomposition candidate), the others failed or were skipped (see candidates)")
        rec.update(status="UNINSTANTIABLE", status_reason=reason,
                   instantiation={"rule": "none", "rule_order": rule_order, "args": [], "call": "", "params": t.params,
                                  "provenance": [], "selection": "no candidate compiled",
                                  "candidates": candidate_summary(records)})
        return _done(rec, t0)
    chosen, fits = select(records, tier, sh.cfg.max_constraints)
    args = tuple(chosen["args"])
    dir_name = P.package_dir_name(t.path, t.name, args, sh.cfg.scope_prefix)
    cr = chosen["compile_result"]
    inst = {"rule": tier, "rule_order": rule_order, "args": list(args), "call": chosen["call"], "params": t.params,
            "provenance": chosen["provenance"][:6],
            "selection": ("largest compiled constraint count within the size policy among the candidates of the "
                          "first tier with a compilable candidate" if fits else
                          "no candidate of the tier fits the size policy; the smallest is recorded"),
            "include_context": cr["include_context"], "main_sha256": cr["main_sha256"],
            "candidates": candidate_summary(records)}
    if cr.get("main_removed"):
        inst["include_main_removed"] = True
    rec = base_record(sh, t, dir_name)
    rec["env"] = sh.env_record(compiler_of(sh, cr))
    rec["instantiation"] = inst
    if not fits:
        rec.update(status="TOO-LARGE",
                   status_reason=f"{cr['constraints']} constraints > {sh.cfg.max_constraints} (decomposition candidate)",
                   circuit=circuit_record(sh, None, cr))
        return _done(rec, t0)
    if pre is not None:
        inst["tag_wrapper"] = {"wrapper": TG.WRAPPER, "tagged_inputs": TG.input_tags(t),
                               "tag_table_source": "; ".join(sorted({TG.KNOWN_TAGS[p.tag]["source"] for p in pre}))}
    rec = _done(build_package(sh, t, rec, cr, dir_name, os.path.join(sh.cfg.work, "items", dir_name), pre), t0)
    if rec["status"] in P.PACKAGED_STATUSES:
        rec = scrub(sh, rec)
        P.write_json(os.path.join(sh.cfg.out, dir_name, "problem.json"), rec)
    return rec


def _done(rec: dict, t0: float) -> dict:
    rec.setdefault("evidence", {})["wall_secs"] = round(time.time() - t0, 1)
    return rec


def scrub(sh: Shared, rec: dict) -> dict:
    """Replace local paths (work directory, checkout, scratch, home) in gate messages by placeholders."""
    text = json.dumps(rec, sort_keys=True, ensure_ascii=False)
    for path, tag in ((sh.cfg.work, "$WORK"), (sh.cfg.repo_dir, "$REPO"), (sh.env.scratch, "$SCRATCH"),
                      (sh.env.toolchain, "$TOOLCHAIN"), (os.path.expanduser("~"), "$HOME")):
        for variant in {path, os.path.realpath(path)}:
            if variant and len(variant) > 1:
                text = text.replace(json.dumps(variant)[1:-1], tag)
    return json.loads(text)


def build_package(sh: Shared, t: cs.Template, rec: dict, size_cr: dict, dir_name: str, work: str,
                  pre: list | None = None) -> dict:
    """Compile the chosen instantiation with --sym/--wasm, emit the Lean files, run the gates and write the
    package.  ``pre``: the tag preconditions (:func:`tags.preconditions`) when the template is compiled
    through the tag wrapper; the statement is then DET under those input preconditions."""
    cfg = sh.cfg
    args = tuple(rec["instantiation"]["args"])
    comp = compiler_of(sh, size_cr)
    full = compile_main(sh, os.path.join(work, "compile"), size_cr["include_context"], t.name, args, True,
                        rule_path=t.path, compiler=comp, tag_template=t if pre is not None else None)
    if full["rc"] != 0 or full.get("r1cs_sha256") != size_cr["r1cs_sha256"]:
        rec.update(status="UNINSTANTIABLE",
                   status_reason="full compile (with --wasm) failed or produced a different R1CS than the sizing compile",
                   circuit=circuit_record(sh, None, size_cr))
        return rec
    r = R.read_r1cs(full["r1cs"])
    syms = R.read_sym(full["sym"])
    io = R.main_io_wires(r, syms)
    names = R.wire_names(syms, r.n_wires)
    rec["circuit"] = circuit_record(sh, r, full, len(io.inputs), len(io.outputs), sha256_file(full["sym"]))
    wpre = TG.wire_preconditions(pre, io.inputs, io.input_names) if pre else None
    pre_fn = (lambda w: TG.holds(wpre, w)) if wpre else None
    pre_lists = [name for name, _, _ in E.precondition_lists(wpre)] if wpre else []
    ns = P.lean_namespace(cfg.collection, dir_name)
    meta = {"repo_id": cfg.repo_id, "instantiation": rec["instantiation"]["call"],
            "generator": f"{sh.generator['name']} v{sh.generator['version']}", "repo_url": cfg.repo_url,
            "commit": cfg.commit, "path": t.path, "template": t.name, "rule": rec["instantiation"]["rule"],
            "circom_version": comp.version, "circom_flags": full["flags"], "r1cs_sha256": full["r1cs_sha256"],
            "prime_name": r.prime_name or "unknown"}
    stage = os.path.join(work, "pkg")
    shutil.rmtree(stage, ignore_errors=True)
    model_rel = E.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(E.emit_model(ns, meta, r, io.outputs, io.inputs, names, wpre))
    statement_text = E.emit_statement(ns, meta, preconditions=bool(wpre))
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{E.STATEMENT_THEOREM}"
    rec["statement"] = {"file": "Statement.lean", "theorem": E.STATEMENT_THEOREM, "theorem_fqn": fqn,
                        "model_module": E.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": P.statement_assumptions(r.prime_name or "unknown"), "truth": "unknown"}
    if wpre:
        rec["statement"]["preconditions"] = [
            {"signal": g["signal"], "tag": g["tag"], "value": g["value"],
             "condition": TG.KNOWN_TAGS[g["tag"]]["precondition"], "wires": g["wires"]} for g in wpre]
        rec["statement"]["assumptions"].append(
            "Preconditions w₁ → Preconditions w₂: DET is stated under the input preconditions of the circom tags "
            "on the template's inputs (" + ", ".join(f"`{g['signal'][5:]}` {{{g['tag']}}}" for g in wpre) +
            "), which the template's callers promise; the main inputs are untagged copies made by the generated "
            "wrapper, and assignments outside the preconditions are not constrained by the statement.")
    gates: dict[str, dict] = {}
    evidence: dict = {"work": work}

    # witnesses
    domains = {p.signal: (p.tag if p.value is None else (p.tag, p.value)) for p in (pre or [])}
    signals = W.input_signals(io, domains)
    wc, wasm = full["witness_js"], full["wasm"]
    gen_dir = os.path.join(work, "wit-gen")
    if not signals:
        attempts, allowed = 1, []
        generated = W.run_generator(cfg.node, wc, wasm, [{}], gen_dir)
    else:
        first = W.run_generator(cfg.node, wc, wasm,
                                W.sample_inputs(signals, r.prime, rec["package_id"], W.uniform_attempts()),
                                gen_dir, tag="uniform")
        allowed = W.successful_strategies(first)
        rest = W.run_generator(cfg.node, wc, wasm,
                               W.sample_inputs(signals, r.prime, rec["package_id"] + "/mixed",
                                               SAMPLE_ATTEMPTS - W.uniform_attempts(), allowed or None, uniform=False),
                               gen_dir, tag="mixed")
        attempts, generated = SAMPLE_ATTEMPTS, first + rest
    accepted, rejected, gen_errors = W.collect_real(r, signals, generated, DET_SEARCH_POOL)
    grid = W.boundary_grid(signals, r.prime)
    grid_accepted = W.collect_real(r, signals, W.run_generator(cfg.node, wc, wasm, grid, gen_dir, tag="grid"),
                                   len(grid))[0] if grid else []
    if pre_fn:                                           # the samplers respect the preconditions; checked here
        accepted = [x for x in accepted if pre_fn(x[1])]
        grid_accepted = [x for x in grid_accepted if pre_fn(x[1])]
    evidence["witness_sampling"] = {"attempts": attempts, "mixed_phase_strategies": allowed or "all",
                                    "generator_errors": gen_errors,
                                    "oracle_rejected_generator_witnesses": len(rejected),
                                    "real_distinct": len(accepted), "boundary_grid": len(grid),
                                    "boundary_grid_accepted": len(grid_accepted)}
    real_w = [w for _, w in accepted][:REAL_WANTED]          # G-FID uses sampled witnesses only
    pool = [w for _, w in accepted] + [w for _, w in grid_accepted]
    muts = W.mutants(r, real_w, MUTANTS, rec["package_id"]) if real_w else []

    # G-ELAB
    build = os.path.join(work, "build")
    shutil.rmtree(build, ignore_errors=True)
    elab = G.g_elab(sh.env, stage, build, ns, os.path.join(work, "elab"))
    gates["G-ELAB"] = elab.to_json()

    # G-FID / G-NONVAC (needs the compiled model)
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build, model_rel[:-len(".lean")] + ".olean"))
    if model_ok:
        fid, nonvac = G.g_fid_nonvac(sh.env, build, ns, r.n_constraints, real_w, muts, not signals,
                                     os.path.join(work, "fid"), W.write_witness, pre=pre_fn)
    else:
        fid = G.Gate("G-FID", "SKIPPED", {"reason": "model did not compile"})
        nonvac = G.Gate("G-NONVAC", "SKIPPED", {"reason": "model did not compile"})
    gates["G-FID"], gates["G-NONVAC"] = fid.to_json(), nonvac.to_json()

    # DET search (Python oracle), confirmed in Lean
    ce, log = (det_search.search(r, pool, io.inputs, io.outputs, rec["package_id"], cfg.det_search_budget_s,
                                 pre=pre_fn) if pool else (None, {"bases": 0}))
    det_gate = {"status": "PASS", "truth": "unknown", "log": log,
                "note": "PASS means no counterexample was found by the cheap searches; DET truth is not established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        os.makedirs(cdir, exist_ok=True)
        W.write_witness(os.path.join(cdir, "w1.txt"), ce.base)
        W.write_witness(os.path.join(cdir, "w2.txt"), ce.other)
        verdicts, _ = (G.lean_verdicts(sh.env, build, ns, r.n_constraints,
                                       [("w1", os.path.join(cdir, "w1.txt")), ("w2", os.path.join(cdir, "w2.txt"))], cdir,
                                       preconditions=bool(wpre))
                       if model_ok else ({}, None))
        lean_ok = verdicts.get("w1") == "ACCEPT" and verdicts.get("w2") == "ACCEPT"
        if wpre:                                         # both assignments inside the preconditions, in Lean too
            lean_ok = lean_ok and verdicts.get("PRE:w1") == "ACCEPT" and verdicts.get("PRE:w2") == "ACCEPT"
        det_gate = {"status": "FAIL", "truth": "false-counterexample-found" if lean_ok else "unknown",
                    "method": ce.method, "changed_outputs": [names[o] for o in ce.changed_outputs][:20],
                    "lean_confirms_both_witnesses": lean_ok, "oracle_confirms": True, "log": log,
                    "files": ["evidence/counterexample/w1.txt", "evidence/counterexample/w2.txt"]}
        evidence["counterexample_dir"] = cdir
    gates["DET-SEARCH"] = det_gate

    # G-TRIV
    if elab.status != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement did not elaborate"}
    elif det_gate["truth"] == "false-counterexample-found":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement refuted by a confirmed counterexample"}
    else:
        gates["G-TRIV"] = G.g_triv(sh.env, build, ns, r.n_constraints, os.path.join(work, "triv"),
                                   cfg.battery_heartbeats, cfg.battery_file_wall, cfg.battery_single_wall,
                                   preconditions=bool(wpre), pre_lists=pre_lists).to_json()
    rec["gates"] = gates

    # status
    set_status(rec, gates, bool(io.outputs))

    # checker metadata
    if elab.status == "PASS":
        ref = elab.detail["reference_type_sha256"]
    else:
        ref = "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": E.STATEMENT_THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(E.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": E.model_module(ns),
                                 "sha256": sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    rec["evidence"] = evidence
    write_package(sh, rec, stage, work, dir_name)
    return rec


def set_status(rec: dict, gates: dict, has_outputs: bool) -> None:
    """Final status of a packaged record from its gates (DET-FALSE-CANDIDATE, GATE-FAIL or OPEN).

    A counterexample on a ``probed`` instantiation is still DET-FALSE-CANDIDATE, labelled not-a-finding."""
    det_gate = gates["DET-SEARCH"]
    if det_gate["truth"] == "false-counterexample-found":
        rec["status"] = "DET-FALSE-CANDIDATE"
        rec["status_reason"] = (f"{det_gate['method']} found two constraint-satisfying assignments with equal inputs "
                                f"and different outputs (confirmed by the oracle and the Lean model); private finding")
        if rec["instantiation"]["rule"] == "probed":
            det_gate["finding"] = PROBED_NOT_A_FINDING
            rec["status_reason"] += f"; {PROBED_NOT_A_FINDING}"
        rec["statement"]["truth"] = "false-counterexample-found"
        return
    failed = [g for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV") if gates[g]["status"] != "PASS"]
    if det_gate["status"] == "FAIL":
        failed.append("DET-SEARCH(unconfirmed-in-Lean)")
    if not has_outputs:
        failed.append("NO-OUTPUTS")
    if failed:
        rec["status"] = "GATE-FAIL"
        reasons = []
        for g in failed:
            gd = gates.get(g, {})
            if g == "NO-OUTPUTS":
                reasons.append("the main component has no output signals, so DET is vacuous")
            elif g == "G-TRIV" and gd.get("closed_by"):
                reasons.append(f"G-TRIV closed by {', '.join(gd['closed_by'][:4])}")
                rec["statement"]["truth"] = "closed-by-automation"
            else:
                reasons.append(f"{g}: {gd.get('reason') or gd.get('errors') or gd.get('status')}")
        rec["status_reason"] = "; ".join(str(x) for x in reasons)[:600]
    else:
        rec["status"] = "OPEN"
        rec["status_reason"] = "all gates pass; DET truth unknown"


def write_package(sh: Shared, rec: dict, stage: str, work: str, dir_name: str) -> None:
    dest = os.path.join(sh.cfg.out, dir_name)
    shutil.rmtree(dest, ignore_errors=True)
    os.makedirs(dest)
    shutil.copyfile(os.path.join(stage, "Statement.lean"), os.path.join(dest, "Statement.lean"))
    model_rel = rec["statement"]["model_file"]
    os.makedirs(os.path.dirname(os.path.join(dest, model_rel)), exist_ok=True)
    shutil.copyfile(os.path.join(stage, model_rel), os.path.join(dest, model_rel))
    ev = os.path.join(dest, "evidence")
    os.makedirs(ev)
    shutil.copyfile(os.path.join(work, "compile", "main.portable.circom"), os.path.join(ev, "main.circom"))
    for name in ("elab/reference.type.txt",):
        src = os.path.join(work, name)
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(ev, os.path.basename(name)))
    if "counterexample_dir" in rec["evidence"]:
        src = rec["evidence"].pop("counterexample_dir")
        os.makedirs(os.path.join(ev, "counterexample"))
        for fn in ("w1.txt", "w2.txt"):          # the witnesses only; the harness file names local paths
            shutil.copyfile(os.path.join(src, fn), os.path.join(ev, "counterexample", fn))
    triv = os.path.join(work, "triv")
    if os.path.isdir(triv):
        os.makedirs(os.path.join(ev, "triv"))
        for fn in sorted(os.listdir(triv)):
            if fn.endswith(".lean"):
                shutil.copyfile(os.path.join(triv, fn), os.path.join(ev, "triv", fn))
    rec["evidence"].pop("work", None)


# ------------------------------------------------------------------------------------------ wave

def ledger_prefix(cfg: WaveConfig) -> str:
    return cfg.ledger_item_prefix or f"{cfg.repo_id}:"


def load_ledger(path: str | None, item_prefix: str) -> tuple[set, str | None]:
    if not path:
        return set(), None
    ids = set()
    prefix = f'"item_id": "{item_prefix}'
    with open(path, encoding="utf-8") as f:
        for line in f:
            if prefix in line:
                ids.add(json.loads(line)["item_id"])
    return ids, sha256_file(path)


def prepare(cfg: WaveConfig) -> tuple[Shared, list[I.TemplatePlan], list[dict]]:
    """Pins checked, repository scanned and templates planned; plus records for ``only`` entries
    whose template is not declared in any scanned ``.circom`` file (recorded, never dropped)."""
    head = git_head(cfg.repo_dir)
    if head != cfg.commit:
        raise ValueError(f"repository is at {head}, the wave pins {cfg.commit}")
    compilers = {}
    for tag, path in {cfg.circom_version_tag: cfg.circom, **cfg.circoms}.items():
        digest = verify_compiler(tag, path)
        kind = CIRCOM_RELEASES[tag]["kind"]
        version = tag[1:] if kind == "circom1" else tool_version([path, "--version"]).replace("circom compiler ", "")
        compilers[tag] = Compiler(tag, kind, path, version, digest)
    circom_sha = compilers[cfg.circom_version_tag].sha256
    env = L.load_env(cfg.lean_env)
    rels = cs.list_circom_files(cfg.repo_dir)
    lib_dirs = list(dict.fromkeys(cfg.include_paths + [p for r in cfg.path_rules for p in r.get("include_paths", [])]))
    files = cs.scan_repo(cfg.repo_dir, rels, lib_dirs, dependencies=True)
    scope = [p for p in rels if p.startswith(cfg.scope_prefix) and p not in cfg.exclude]
    ledger_ids, ledger_sha = load_ledger(cfg.ledger, ledger_prefix(cfg))
    sh = Shared(cfg, env, files, ledger_ids, ledger_sha, circom_sha, compilers[cfg.circom_version_tag].version,
                tool_version([cfg.node, "--version"]), P.generator_info(), compilers, I.function_names(files))
    plans = I.plan_templates(files, scope, cfg.repo_id, config_mains=cs.scan_config_mains(cfg.repo_dir, files),
                             probe=cfg.probe)
    missing: list[dict] = []
    if cfg.only:
        plans = [pl for pl in plans if f"{pl.template.path}#{pl.template.name}" in cfg.only]
        found = {f"{pl.template.path}#{pl.template.name}" for pl in plans}
        missing = [missing_record(sh, key) for key in cfg.only if key not in found]
    return sh, plans, missing


def missing_record(sh: Shared, key: str) -> dict:
    """UNINSTANTIABLE record for a requested template that no scanned ``.circom`` file declares."""
    path, name = key.rsplit("#", 1)
    full = os.path.join(sh.cfg.repo_dir, path)
    if not os.path.isfile(full):              # the record needs the source digest; a wrong pin is fatal
        raise ValueError(f"{key}: source file missing at the pin")
    with open(full, encoding="utf-8", errors="replace") as f:
        clean = cs.strip_comments(f.read())
    m = re.search(r"\btemplate\s+(?:(?:parallel|custom)\s+)?" + re.escape(name) + r"\s*\(", clean)
    where = ("the declaring file is not a .circom source (it is rendered by the project's own tooling), so "
             "no instantiation can be compiled from the pinned sources" if not path.endswith(".circom") else
             "the template declaration was not found by the scanner")
    t = cs.Template(path, name, [], "", cs.line_of(clean, m.start()) if m else 1, "")
    rec = base_record(sh, t, P.package_dir_name(path, name, (), sh.cfg.scope_prefix))
    rec.update(status="UNINSTANTIABLE", status_reason=f"template not compiled: {where}",
               instantiation={"rule": "none", "rule_order": list(I.TIERS), "args": [], "call": "", "params": [],
                              "provenance": [], "selection": "template not declared in a scanned .circom file",
                              "candidates": []})
    return _done(rec, time.time())


def run_wave(cfg: WaveConfig) -> list[dict]:
    return run_waves([cfg], cfg.jobs)[cfg.out]


def run_waves(cfgs: list[WaveConfig], jobs: int) -> dict[str, list[dict]]:
    """Several repositories through one worker pool of ``jobs`` template workers; one INDEX.jsonl
    per configuration's ``out`` directory."""
    prepared = []
    for cfg in cfgs:
        sh, plans, missing = prepare(cfg)
        os.makedirs(cfg.out, exist_ok=True)
        os.makedirs(cfg.work, exist_ok=True)
        prepared.append((sh, plans, [scrub(sh, m) for m in missing]))
    results: dict[str, list[dict]] = {sh.cfg.out: list(missing) for sh, _, missing in prepared}
    total = sum(len(plans) for _, plans, _ in prepared)
    remaining = {sh.cfg.out: len(plans) for sh, plans, _ in prepared}
    done = 0
    # largest repositories first, so that the pool drains evenly
    order = sorted(((sh, pl) for sh, plans, _ in prepared for pl in plans), key=lambda x: -len(x[0].files))
    with cf.ThreadPoolExecutor(max_workers=jobs) as ex:
        futs = {ex.submit(process_template, sh, pl): (sh, pl) for sh, pl in order}
        for fut in cf.as_completed(futs):
            sh, pl = futs[fut]
            try:
                rec = fut.result()
            except Exception as exc:  # recorded, never silently dropped
                rec = base_record(sh, pl.template, P.package_dir_name(pl.template.path, pl.template.name, (),
                                                                      sh.cfg.scope_prefix))
                rec.update(status="UNINSTANTIABLE", status_reason=f"generator error: {type(exc).__name__}: {exc}"[:600],
                           instantiation={"rule": "none", "args": [], "call": "", "provenance": [],
                                          "selection": "generator error", "candidates": []})
            results[sh.cfg.out].append(scrub(sh, rec))
            done += 1
            print(f"[{done}/{total}] {rec['package_id']}: {rec['status']} "
                  f"({rec.get('evidence', {}).get('wall_secs', '?')} s)", flush=True)
            remaining[sh.cfg.out] -= 1
            if not remaining[sh.cfg.out]:             # repository complete: write its index now
                write_index(sh.cfg.out, results[sh.cfg.out])
    for sh, _, _ in prepared:
        write_index(sh.cfg.out, results[sh.cfg.out])
    return results


def write_index(out: str, records: list[dict]) -> None:
    records.sort(key=lambda x: (x["ids"]["path"], x["ids"]["template_line"], x["ids"]["template"]))
    with open(os.path.join(out, "INDEX.jsonl"), "w", encoding="utf-8") as f:
        for rec in records:
            f.write(json.dumps(rec, sort_keys=True, ensure_ascii=False) + "\n")


def _asset_for_host() -> str:
    system = platform.system()
    return {"Darwin": "macos-amd64", "Linux": "linux-amd64"}.get(system, "windows-amd64.exe")


def validate_all(index_path: str, packages_dir: str, collections_root: bool = False) -> list[str]:
    """Re-validate every record; ``collections_root``: ``packages_dir`` holds one directory per
    collection (a combined index), otherwise it is the collection directory itself."""
    schema = P.load_schema()
    problems = []
    with open(index_path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            rec = json.loads(line)
            pkg = os.path.join(packages_dir, rec["package_id"] if collections_root else rec["package_id"].split("/", 1)[1])
            has_dir = os.path.isdir(pkg)
            errs = P.validate_problem(rec, pkg if has_dir else None, schema)
            if rec["status"] in P.PACKAGED_STATUSES:
                if not has_dir:
                    errs.append("package directory missing")
                else:
                    with open(os.path.join(pkg, "problem.json"), encoding="utf-8") as g:
                        if json.load(g) != rec:
                            errs.append("problem.json differs from the index record")
            elif has_dir:
                errs.append(f"{rec['status']} record has a package directory")
            problems += [f"line {n} {rec['package_id']}: {e}" for e in errs]
    return problems


# ------------------------------------------------------------------------------------------ decomposition

def _size_node(sh: Shared, node: dict, work: str) -> None:
    """Size one harvested child instantiation (same compiler selection, tag wrapper and guard as the wave)."""
    t = node["template"]
    pre = None
    if TG.input_tags(t):
        try:
            pre = TG.preconditions(t)
        except TG.TagError as exc:
            node["error"] = f"tagged inputs: {exc}"
            return
    plan = I.TemplatePlan(t, {DC.RULE: [I.Candidate(DC.RULE, node["args"], node["provenance"][:1])]})
    _, recs = size_candidates(sh, plan, work, wrapper=pre is not None)
    node["size_records"] = recs
    cr = recs[0]["compile_result"]
    if "constraints" in cr:
        node.update(constraints=cr["constraints"], wires=cr["wires"])
    else:
        node["error"] = (cr.get("guard") or cr.get("skipped") or cr.get("error") or "compile failed")[:400]
        node["guard"] = bool(cr.get("guard"))


def _package_node(sh: Shared, key: str, node: dict, variants: list[dict]) -> dict:
    """The package of the selected harvested instantiation of one template content."""
    t, args = node["template"], node["args"]
    t0 = time.time()
    pre = TG.preconditions(t) if TG.input_tags(t) else None
    chosen = node["size_records"][0]
    cr = chosen["compile_result"]
    dir_name = P.package_dir_name(t.path, t.name, args, sh.cfg.scope_prefix)
    inst = {"rule": DC.RULE, "rule_order": [DC.RULE], "args": list(args), "call": chosen["call"], "params": t.params,
            "provenance": node["provenance"][:6],
            "selection": ("largest compiled constraint count within the size policy among the harvested "
                          "sub-component instantiations of this template content (decomposition of TOO-LARGE "
                          "instantiations)"),
            "include_context": cr["include_context"], "main_sha256": cr["main_sha256"],
            "candidates": candidate_summary([r for v in variants for r in v["size_records"]][:40]),
            "decomposition": {"parents": node["parents"][:20], "n_parents": len(node["parents"]),
                              "depth": node["depth"], "content_sha256": node["group"][1],
                              "variants": len(variants), "instance_key": key}}
    if cr.get("main_removed"):
        inst["include_main_removed"] = True
    if pre is not None:
        inst["tag_wrapper"] = {"wrapper": TG.WRAPPER, "tagged_inputs": TG.input_tags(t),
                               "tag_table_source": "; ".join(sorted({TG.KNOWN_TAGS[p.tag]["source"] for p in pre}))}
    rec = base_record(sh, t, dir_name)
    rec["env"] = sh.env_record(compiler_of(sh, cr))
    rec["instantiation"] = inst
    work = os.path.join(sh.cfg.work, "items", dir_name)
    try:
        rec = _done(build_package(sh, t, rec, cr, dir_name, work, pre), t0)
    finally:
        if not sh.cfg.keep_work:
            shutil.rmtree(work, ignore_errors=True)
    if rec["status"] in P.PACKAGED_STATUSES:
        rec = scrub(sh, rec)
        P.write_json(os.path.join(sh.cfg.out, dir_name, "problem.json"), rec)
    return scrub(sh, rec)


def run_decompose(cfgs: list[WaveConfig], parents_file: str, existing_file: str, edges_out: str, jobs: int,
                  max_depth: int = DC.MAX_DEPTH) -> dict:
    """Harvest, size, select and package the sub-component instantiations of the TOO-LARGE parents in
    ``parents_file`` (one JSON object per line: collection, package_id, path, template, args, call).
    ``existing_file``: instance keys of existing packages (key, package_id).  Writes one INDEX.jsonl per
    configuration ``out`` and every parent -> child edge with its resolution to ``edges_out``."""
    shs = {}
    for cfg in cfgs:
        sh, _, _ = prepare(cfg)
        os.makedirs(cfg.out, exist_ok=True)
        os.makedirs(cfg.work, exist_ok=True)
        shs[cfg.collection] = sh
    existing = {}
    with open(existing_file, encoding="utf-8") as f:
        for line in f:
            row = json.loads(line)
            existing.setdefault(row["key"], row["package_id"])
    names = {c: {t.name for sf in sh.files.values() for t in sf.templates} for c, sh in shs.items()}
    caches: dict[str, dict] = {c: {} for c in shs}
    frontier = []
    with open(parents_file, encoding="utf-8") as f:
        for line in f:
            p = json.loads(line)
            sh = shs[p["collection"]]
            t = next(x for x in sh.files[p["path"]].templates if x.name == p["template"])
            frontier.append((sh, t, tuple(p["args"]), 0, {"package_id": p["package_id"], "call": p["call"]}))
    nodes: dict[str, dict] = {}
    edges: list[dict] = []
    unresolved = []
    level = 0
    while frontier:
        level += 1
        new_keys = []
        for sh, t, args, depth, ref in frontier:
            coll = sh.cfg.collection
            kids, unres = I.children_of(sh.files, t, args, sh.functions, names[coll])
            if unres:
                unresolved.append({"parent": ref, "unresolved_call_sites": unres})
            for child, cargs, prov in kids:
                content = CT.content_hash(sh.files, child.path, child.name, caches[coll])
                prime = build_options(sh.cfg, child.path)[0] or "bn128"
                key = CT.instance_key(prime, content, cargs)
                call = f"{child.name}({', '.join(cargs)})"
                edges.append({"parent": ref, "child_key": key, "collection": coll, "child_path": child.path,
                              "child_template": child.name, "child_call": call[:300], "provenance": prov[:400],
                              "depth": depth + 1})
                if key in existing:
                    continue
                if key in nodes:
                    if len(nodes[key]["parents"]) < 200:
                        nodes[key]["parents"].append(ref)
                    continue
                nodes[key] = {"sh": sh, "template": child, "args": cargs, "depth": depth + 1,
                              "group": (prime, content), "call": call, "provenance": [prov], "parents": [ref]}
                new_keys.append(key)
        print(f"decompose level {level}: {len(frontier)} parents, {len(new_keys)} new child instantiations",
              flush=True)
        with cf.ThreadPoolExecutor(max_workers=jobs) as ex:
            futs = {ex.submit(_size_node, nodes[k]["sh"], nodes[k],
                              os.path.join(nodes[k]["sh"].cfg.work, "decompose", f"n{abs(hash(k)) % 10**12}")): k
                    for k in new_keys}
            for fut in cf.as_completed(futs):
                k = futs[fut]
                try:
                    fut.result()
                except Exception as exc:  # recorded, never dropped
                    nodes[k]["error"] = f"generator error: {type(exc).__name__}: {exc}"[:400]
                shutil.rmtree(os.path.join(nodes[k]["sh"].cfg.work, "decompose", f"n{abs(hash(k)) % 10**12}"),
                              ignore_errors=True)
        frontier = []
        for k in new_keys:
            n = nodes[k]
            big = (n.get("constraints") or 0) > n["sh"].cfg.max_constraints or n.get("guard")
            if big and n["depth"] < max_depth:
                n["expanded"] = True
                frontier.append((n["sh"], n["template"], n["args"], n["depth"], {"key": k, "call": n["call"]}))
    max_c = min(sh.cfg.max_constraints for sh in shs.values())
    chosen = DC.select_variants({k: {"group": n["group"], "constraints": n.get("constraints"), "wires": n.get("wires", 0),
                                     "call": n["call"]} for k, n in nodes.items()}, max_c)
    selected = sorted(set(chosen.values()))
    print(f"decompose: {len(nodes)} new child instantiations, {len(chosen)} within the size policy, "
          f"{len(selected)} template contents to package", flush=True)
    packaged: dict[str, dict] = {}
    with cf.ThreadPoolExecutor(max_workers=jobs) as ex:
        futs = {ex.submit(_package_node, nodes[k]["sh"], k, nodes[k],
                          [nodes[v] for v in chosen if chosen[v] == k]): k for k in selected}
        for i, fut in enumerate(cf.as_completed(futs), 1):
            k = futs[fut]
            try:
                rec = fut.result()
            except Exception as exc:  # recorded, never dropped
                n = nodes[k]
                rec = base_record(n["sh"], n["template"], P.package_dir_name(n["template"].path, n["template"].name,
                                                                         n["args"], n["sh"].cfg.scope_prefix))
                rec.update(status="UNINSTANTIABLE", status_reason=f"generator error: {type(exc).__name__}: {exc}"[:600],
                           instantiation={"rule": DC.RULE, "args": list(n["args"]), "call": n["call"],
                                          "provenance": n["provenance"][:6], "selection": "generator error",
                                          "candidates": []})
                rec = scrub(n["sh"], rec)
            packaged[k] = rec
            print(f"[{i}/{len(selected)}] {rec['package_id']}: {rec['status']} "
                  f"({rec.get('evidence', {}).get('wall_secs', '?')} s)", flush=True)
    by_out: dict[str, list] = {sh.cfg.out: [] for sh in shs.values()}
    for k, rec in packaged.items():
        by_out[nodes[k]["sh"].cfg.out].append(rec)
    for out, recs in by_out.items():
        write_index(out, recs)
    with open(edges_out, "w", encoding="utf-8") as f:
        for e in edges:
            e = dict(e, **DC.resolution(e["child_key"], nodes, existing, chosen, packaged, max_c))
            f.write(json.dumps(e, sort_keys=True) + "\n")
        for u in unresolved:
            f.write(json.dumps(dict(u, result="unresolved-call-sites"), sort_keys=True) + "\n")
    return {"nodes": len(nodes), "packaged": len(packaged), "edges": len(edges)}


# ------------------------------------------------------------------------------------------ battery P2 re-run

def apply_p2(rec: dict, p2: dict) -> dict:
    """The record after the P2 battery (``p2``: a G-TRIV gate from :func:`gates.g_triv_p2`) ran on its package:
    P2 closures join ``closed_by`` and an OPEN record becomes GATE-FAIL (closed by automation).  The package
    files are unchanged; ``supersedes`` keeps the previous status and generator."""
    new = copy.deepcopy(rec)
    g = dict(new["gates"]["G-TRIV"])
    closed = sorted(set(g.get("closed_by", [])) | set(p2["closed_by"]))
    g.update(closed_by=closed, forms=g.get("forms", 33) + p2["forms"], battery=G.BATTERY_P1_P2,
             p2={k: v for k, v in p2.items() if k != "status"},
             status="FAIL" if closed else g["status"])
    new["gates"]["G-TRIV"] = g
    new["supersedes"] = {"status": rec["status"], "status_reason": rec["status_reason"],
                         "generator_sources_sha256": rec["generator"]["sources_sha256"]}
    new["generator"] = P.generator_info()
    if p2["closed_by"] and rec["status"] == "OPEN":
        new["status"] = "GATE-FAIL"
        new["status_reason"] = f"G-TRIV (battery P2) closed by {', '.join(p2['closed_by'][:4])}"
        new["statement"]["truth"] = "closed-by-automation"
    return new


def battery_p2_one(env: L.LeanEnv, rec: dict, pkg: str, work: str, out: str, heartbeats: int = 200000,
                   file_wall: float = 900, single_wall: float = 180) -> dict:
    """Run the P2 battery on one packaged statement; a record whose status changes is written, with a copy of
    its package and the P2 battery file, under ``out``.  Returns the result row."""
    t0 = time.time()
    st = rec["statement"]
    ns = st["theorem_fqn"].rsplit(".", 1)[0]
    build = os.path.join(work, "build")
    shutil.rmtree(work, ignore_errors=True)
    r, msgs = L.compile_module(env, pkg, st["model_file"], build, E.LEAN_OPTIONS, 3600)
    row = {"package_id": rec["package_id"], "status_before": rec["status"]}
    if r.rc != 0 or L.errors(msgs):
        row.update(result="ERROR", error="model did not compile", secs=round(time.time() - t0, 1))
        return row
    groups = st.get("preconditions") or []
    pre_lists = [name for name, _, _ in E.precondition_lists(groups)] if groups else []
    gate = G.g_triv_p2(env, build, ns, rec["circuit"]["n_constraints"], os.path.join(work, "triv"), heartbeats,
                       file_wall, single_wall, preconditions=bool(groups), pre_lists=pre_lists).to_json()
    new = apply_p2(rec, gate)
    row.update(result="CLOSED" if gate["closed_by"] else "SURVIVES", closed_by=gate["closed_by"],
               timeouts=gate["timeouts"], status_after=new["status"], secs=round(time.time() - t0, 1))
    if new["status"] != rec["status"]:
        dest = os.path.join(out, rec["package_id"])
        shutil.rmtree(dest, ignore_errors=True)
        shutil.copytree(pkg, dest)
        shutil.copyfile(os.path.join(work, "triv", "Battery_P2.lean"),
                        os.path.join(dest, "evidence", "triv", "Battery_P2.lean"))
        P.write_json(os.path.join(dest, "problem.json"), new)
        row["record"] = new
    return row


def run_battery_p2(sources: list[tuple[str, str]], lean_env: str, work: str, out: str, jobs: int) -> list[dict]:
    """P2 battery over every OPEN record of the given (index, packages root) pairs."""
    env = L.load_env(lean_env)
    todo = []
    for index, root in sources:
        with open(index, encoding="utf-8") as f:
            for line in f:
                rec = json.loads(line)
                if rec["status"] == "OPEN":
                    todo.append((rec, os.path.join(root, rec["package_id"])))
    os.makedirs(out, exist_ok=True)
    rows = []
    with cf.ThreadPoolExecutor(max_workers=jobs) as ex:
        futs = {ex.submit(battery_p2_one, env, rec, pkg, os.path.join(work, rec["package_id"]), out): rec
                for rec, pkg in todo}
        for n, fut in enumerate(cf.as_completed(futs), 1):
            rec = futs[fut]
            try:
                row = fut.result()
            except Exception as exc:  # recorded, never silently dropped
                row = {"package_id": rec["package_id"], "result": "ERROR", "error": f"{type(exc).__name__}: {exc}"[:400]}
            shutil.rmtree(os.path.join(work, rec["package_id"]), ignore_errors=True)
            rows.append(row)
            print(f"[{n}/{len(todo)}] {row['package_id']}: {row['result']} {row.get('closed_by', '')} "
                  f"({row.get('secs', '?')} s)", flush=True)
    rows.sort(key=lambda x: x["package_id"])
    with open(os.path.join(out, "RESULTS.jsonl"), "w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps({k: v for k, v in row.items() if k != "record"}, sort_keys=True) + "\n")
    with open(os.path.join(out, "INDEX.jsonl"), "w", encoding="utf-8") as f:
        for row in rows:
            if "record" in row:
                f.write(json.dumps(row["record"], sort_keys=True, ensure_ascii=False) + "\n")
    return rows


# ------------------------------------------------------------------------------------------ report

SIZE_BUCKETS = [(0, 0), (1, 10), (11, 100), (101, 500), (501, 1000), (1001, 2000), (2001, 10000),
                (10001, 100000), (100001, None)]
GATE_NAMES = ["G-ELAB", "G-NONVAC", "G-FID", "G-TRIV", "DET-SEARCH"]


def summarize(records: list[dict]) -> dict:
    """Counts by status, constraint-size histogram, gate results and closures (pure; unit-tested)."""
    by_status = {s: 0 for s in P.STATUSES}
    for rec in records:
        by_status[rec["status"]] += 1
    hist = []
    for lo, hi in SIZE_BUCKETS:
        label = f"{lo}" if lo == hi else (f"{lo}-{hi}" if hi is not None else f">{lo - 1}")
        n = sum(1 for r in records if "circuit" in r and lo <= r["circuit"]["n_constraints"]
                and (hi is None or r["circuit"]["n_constraints"] <= hi))
        hist.append({"bucket": label, "count": n})
    gates = {g: {} for g in GATE_NAMES}
    for rec in records:
        for g, res in rec.get("gates", {}).items():
            gates[g][res["status"]] = gates[g].get(res["status"], 0) + 1
    closures = [{"package_id": r["package_id"], "closed_by": r["gates"]["G-TRIV"]["closed_by"]}
                for r in records if r.get("gates", {}).get("G-TRIV", {}).get("closed_by")]
    rules = {}
    for rec in records:
        rules[rec["instantiation"]["rule"]] = rules.get(rec["instantiation"]["rule"], 0) + 1
    walls = sorted(r.get("evidence", {}).get("wall_secs", 0) for r in records)
    return {"records": len(records), "by_status": by_status, "constraint_histogram": hist, "gates": gates,
            "triv_closures": closures, "rules": rules,
            "item_wall_secs": {"sum": round(sum(walls), 1), "median": walls[len(walls) // 2] if walls else 0,
                               "max": walls[-1] if walls else 0}}


def render_report(summary: dict, records: list[dict], extra: dict) -> str:
    L_ = ["# Wave 0 report: circom DET packages", ""]
    L_ += [f"- {k}: {v}" for k, v in extra.items()] + [""]
    L_ += ["## Counts by status", "", "| status | count |", "|---|---:|"]
    L_ += [f"| {k} | {v} |" for k, v in summary["by_status"].items()]
    L_ += [f"| total | {summary['records']} |", "", "## Instantiation rule used", "", "| rule | count |", "|---|---:|"]
    L_ += [f"| {k} | {v} |" for k, v in sorted(summary["rules"].items())]
    L_ += ["", "## Constraint-size histogram (compiled instantiations, `--O0`)", "", "| constraints | count |", "|---|---:|"]
    L_ += [f"| {h['bucket']} | {h['count']} |" for h in summary["constraint_histogram"]]
    L_ += ["", "## Gate results (packaged instantiations)", "", "| gate | PASS | FAIL | SKIPPED | ERROR |", "|---|---:|---:|---:|---:|"]
    for g, c in summary["gates"].items():
        L_.append(f"| {g} | {c.get('PASS', 0)} | {c.get('FAIL', 0)} | {c.get('SKIPPED', 0)} | {c.get('ERROR', 0)} |")
    L_ += ["", "## Per instantiation", "", "| package | rule | constraints | status | reason |", "|---|---|---:|---|---|"]
    for r in records:
        n = r.get("circuit", {}).get("n_constraints", "")
        reason = r["status_reason"].replace("|", "/")[:160]
        L_.append(f"| `{r['package_id'].split('/', 1)[1]}` `{r['instantiation']['call'][:60]}` | {r['instantiation']['rule']} "
                  f"| {n} | {r['status']} | {reason} |")
    return "\n".join(L_) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="circom DET problem generator")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a1 = sub.add_parser("fetch-circom")
    a1.add_argument("--version", default="v2.2.3")
    a1.add_argument("--asset", default=_asset_for_host())
    a1.add_argument("--dest", required=True)
    a2 = sub.add_parser("lean-env")
    a2.add_argument("--project", required=True)
    a2.add_argument("--toolchain", required=True)
    a2.add_argument("--scratch", required=True)
    a2.add_argument("--out", required=True)
    a3 = sub.add_parser("wave")
    a3.add_argument("--config", required=True, action="append", help="repeat to run several repositories")
    a3.add_argument("--jobs", type=int, help="template workers shared by all configurations")
    a5 = sub.add_parser("summary")
    a5.add_argument("--index", required=True)
    a6 = sub.add_parser("battery-p2", help="run the P2 battery on every OPEN package of the given indexes")
    a6.add_argument("--source", required=True, action="append", help="INDEX.jsonl=PACKAGES_ROOT (repeatable)")
    a6.add_argument("--lean-env", required=True)
    a6.add_argument("--work", required=True)
    a6.add_argument("--out", required=True)
    a6.add_argument("--jobs", type=int, default=6)
    a7 = sub.add_parser("decompose", help="package sub-component instantiations of TOO-LARGE parents")
    a7.add_argument("--config", required=True, action="append")
    a7.add_argument("--parents", required=True)
    a7.add_argument("--existing", required=True)
    a7.add_argument("--edges-out", required=True)
    a7.add_argument("--jobs", type=int, default=6)
    a7.add_argument("--max-depth", type=int, default=DC.MAX_DEPTH)
    a4 = sub.add_parser("validate")
    a4.add_argument("--index", required=True)
    a4.add_argument("--packages", required=True)
    a4.add_argument("--collections-root", action="store_true",
                    help="--packages holds one directory per collection (combined index)")
    a = ap.parse_args(argv)
    if a.cmd == "fetch-circom":
        print(fetch_circom(a.version, a.asset, a.dest))
    elif a.cmd == "lean-env":
        P.write_json(a.out, L.describe_project(a.project, a.toolchain, a.scratch))
    elif a.cmd == "wave":
        cfgs = [WaveConfig.load(c) for c in a.config]
        run_waves(cfgs, a.jobs or cfgs[0].jobs)
    elif a.cmd == "decompose":
        print(json.dumps(run_decompose([WaveConfig.load(c) for c in a.config], a.parents, a.existing, a.edges_out,
                                       a.jobs, a.max_depth)))
    elif a.cmd == "battery-p2":
        run_battery_p2([tuple(x.split("=", 1)) for x in a.source], a.lean_env, a.work, a.out, a.jobs)
    elif a.cmd == "summary":
        with open(a.index, encoding="utf-8") as f:
            print(json.dumps(summarize([json.loads(x) for x in f]), indent=1))
    elif a.cmd == "validate":
        errs = validate_all(a.index, a.packages, a.collections_root)
        for e in errs:
            print(e)
        print(f"{len(errs)} problem(s)")
        return 1 if errs else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
