"""Pinned Lean environment description and a guarded ``lean`` process runner.

The environment is a JSON file (see :func:`describe_project`) naming the toolchain directory and
the ``LEAN_PATH`` of an already-built lake project.  Every run uses a restricted ``PATH`` (the
toolchain's ``bin`` first), takes the sysroot from ``LEAN_SYSROOT`` and points ``ELAN_HOME`` at an
unused scratch directory, so no elan proxy can install a toolchain as a side effect.
"""
from __future__ import annotations

import json
import os
import signal
import subprocess
import threading
import time
from dataclasses import dataclass, field

SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"


@dataclass
class LeanEnv:
    toolchain: str
    lean_path: list[str]
    lean_version: str
    packages: dict[str, str]              # package name -> git revision
    manifest_sha256: str
    scratch: str                          # writable directory for caches / tmp
    extra: dict = field(default_factory=dict)

    @property
    def lean(self) -> str:
        return os.path.join(self.toolchain, "bin", "lean")

    def pins(self) -> dict:
        return {"lean": self.lean_version, "mathlib": self.packages.get("mathlib", ""),
                "lake_manifest_sha256": self.manifest_sha256, "packages": dict(sorted(self.packages.items()))}


def _git_head(pkg_dir: str) -> str:
    head = os.path.join(pkg_dir, ".git", "HEAD")
    with open(head, encoding="utf-8") as f:
        ref = f.read().strip()
    if ref.startswith("ref: "):
        with open(os.path.join(pkg_dir, ".git", ref[5:]), encoding="utf-8") as f:
            return f.read().strip()
    return ref


def describe_project(project_dir: str, toolchain: str, scratch: str) -> dict:
    """Environment record for a built lake project; every package HEAD must equal its manifest rev."""
    import hashlib
    manifest_path = os.path.join(project_dir, "lake-manifest.json")
    with open(manifest_path, "rb") as f:
        raw = f.read()
    manifest = json.loads(raw)
    packages, lean_path, unbuilt = {}, [], []
    for pkg in manifest["packages"]:
        pdir = os.path.join(project_dir, manifest.get("packagesDir", ".lake/packages"), pkg["name"])
        head = _git_head(pdir)
        if head != pkg["rev"]:
            raise ValueError(f"package {pkg['name']} is at {head}, manifest pins {pkg['rev']}")
        packages[pkg["name"]] = pkg["rev"]
        lib = os.path.join(pdir, ".lake", "build", "lib", "lean")
        if os.path.isdir(lib):
            lean_path.append(lib)
        else:
            unbuilt.append(pkg["name"])     # e.g. Cli, needed only by lake itself
    with open(os.path.join(project_dir, "lean-toolchain"), encoding="utf-8") as f:
        declared = f.read().strip()
    version = declared.split(":")[-1]
    if not os.path.basename(toolchain.rstrip("/")).endswith(version):
        raise ValueError(f"toolchain {toolchain} does not match lean-toolchain {declared}")
    return {"toolchain": toolchain, "lean_path": lean_path, "lean_version": version, "packages": packages,
            "manifest_sha256": hashlib.sha256(raw).hexdigest(), "scratch": scratch,
            "extra": {"unbuilt_packages": unbuilt}}


def load_env(path: str) -> LeanEnv:
    with open(path, encoding="utf-8") as f:
        d = json.load(f)
    return LeanEnv(d["toolchain"], d["lean_path"], d["lean_version"], d["packages"], d["manifest_sha256"],
                   d["scratch"], d.get("extra", {}))


def process_env(env: LeanEnv, extra_lean_path: list[str] | tuple[str, ...] = ()) -> dict:
    tmp = os.path.join(env.scratch, "tmp")
    os.makedirs(tmp, exist_ok=True)
    return {
        "PATH": f"{env.toolchain}/bin:{SYSTEM_PATH}",
        "LEAN_SYSROOT": env.toolchain,
        "LEAN_PATH": ":".join(list(extra_lean_path) + env.lean_path),
        "HOME": os.environ.get("HOME", tmp),
        "TMPDIR": tmp,
        "ELAN_HOME": os.path.join(env.scratch, "elan-unused"),
        "XDG_CACHE_HOME": os.path.join(env.scratch, "xdg"),
        "PYTHONDONTWRITEBYTECODE": "1",
        "LANG": "C.UTF-8",
    }


@dataclass
class RunResult:
    rc: int | None
    out: str
    secs: float
    timeout: bool
    memkill: bool = False
    peak_rss_mb: int = 0


def run_process(cmd: list[str], env: dict, cwd: str, timeout: float, rss_limit_mb: int | None = None) -> RunResult:
    """Run ``cmd`` in its own session; kill the whole group on timeout or when RSS exceeds the limit."""
    t0 = time.time()
    p = subprocess.Popen(cmd, env=env, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         start_new_session=True)
    state = {"memkill": False, "peak": 0}
    stop = threading.Event()

    def guard() -> None:
        while not stop.wait(2.0):
            try:
                o = subprocess.run(["ps", "-o", "rss=", "-p", str(p.pid)], capture_output=True, text=True).stdout.strip()
                kb = int(o) if o else 0
            except (OSError, ValueError):
                kb = 0
            state["peak"] = max(state["peak"], kb)
            if rss_limit_mb and kb > rss_limit_mb * 1024:
                state["memkill"] = True
                _kill(p)
                return

    th = threading.Thread(target=guard, daemon=True)
    th.start()
    timed_out = False
    try:
        out, _ = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        _kill(p)
        out, _ = p.communicate()
    stop.set()
    return RunResult(p.returncode, out.decode("utf-8", "replace"), round(time.time() - t0, 2), timed_out,
                     state["memkill"], state["peak"] // 1024)


def _kill(p: subprocess.Popen) -> None:
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def run_lean(env: LeanEnv, args: list[str], cwd: str, timeout: float, extra_lean_path=(),
             rss_limit_mb: int | None = None) -> RunResult:
    return run_process([env.lean] + args, process_env(env, extra_lean_path), cwd, timeout, rss_limit_mb)


def parse_messages(out: str) -> list[dict]:
    """``lean --json`` output: one JSON message per line; other lines become error messages."""
    msgs = []
    for ln in out.splitlines():
        if not ln.strip():
            continue
        try:
            m = json.loads(ln)
            if isinstance(m, dict):
                msgs.append(m)
                continue
        except ValueError:
            pass
        msgs.append({"severity": "error", "data": "non-JSON output: " + ln[:500], "pos": None})
    return msgs


def compile_module(env: LeanEnv, src_root: str, rel_path: str, build_dir: str, opts: list[str], timeout: float,
                   extra_lean_path=()) -> tuple[RunResult, list[dict]]:
    """Compile ``src_root/rel_path`` to ``build_dir`` (``.olean`` + ``.ilean``) with ``--json``."""
    stem = rel_path[:-len(".lean")]
    olean = os.path.join(build_dir, stem + ".olean")
    os.makedirs(os.path.dirname(olean), exist_ok=True)
    args = opts + ["--json", "-R", src_root, "-o", olean, "-i", os.path.join(build_dir, stem + ".ilean"),
                   os.path.join(src_root, rel_path)]
    r = run_lean(env, args, src_root, timeout, extra_lean_path=[build_dir, *extra_lean_path])
    return r, parse_messages(r.out)


def errors(msgs: list[dict]) -> list[dict]:
    return [m for m in msgs if m.get("severity") == "error"]


def fmt_msg(m: dict) -> str:
    pos = m.get("pos") or {}
    return f"{pos.get('line')}:{pos.get('column')}: {m.get('data', '')[:400]}"
