"""Pinned Lean environment description and a guarded ``lean`` process runner.

The environment is a JSON file (see :func:`describe_project`) naming the toolchain directory and
the ``LEAN_PATH`` of an already-built lake project.  Every run uses a restricted ``PATH`` (the
toolchain's ``bin`` first), takes the sysroot from ``LEAN_SYSROOT`` and points ``ELAN_HOME`` at an
unused scratch directory, so no elan proxy can install a toolchain as a side effect.
"""
from __future__ import annotations
from zk_registry.safe_fs import open
from zk_registry import safe_fs as FS

import json
import os
import platform
from contextlib import contextmanager
from contextvars import ContextVar
import signal
import subprocess
import threading
import time
from dataclasses import dataclass, field

SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
_SANDBOX = ContextVar('zk_registry_sandbox', default=None)


def availability_exit(code: int | None) -> bool:
    """A signal (direct or shell encoded) or deadline is not a proof rejection."""
    return code is not None and (code < 0 or code == 124 or code >= 128)


@contextmanager
def sandbox_context(sandbox, readable: list[str], writable: list[str], runner=None):
    """Apply one boundary to compile, replay-tool analysis and export commands in this context."""
    token = _SANDBOX.set((sandbox, readable, writable, runner))
    try:
        with FS.submission_io(sandbox.exposed_writers):
            yield
    finally:
        _SANDBOX.reset(token)


def current_sandbox():
    """Return the active boundary, including its accumulated writer exposure."""
    active = _SANDBOX.get()
    return active[0] if active is not None else None


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


def production_descriptor() -> dict:
    """Read the one tracked production environment descriptor, not operator configuration."""
    path = os.path.join(os.path.dirname(__file__), 'data', 'acceptance_environment_v2.json')
    with open(path, encoding='utf-8') as stream:
        return json.load(stream)


def testnet_caps() -> dict:
    """One tracked cap profile; deterministic fuel enforcement is a separate upgrade."""
    path = os.path.join(os.path.dirname(__file__), 'data', 'testnet_caps_v1.json')
    with open(path, encoding='utf-8') as stream:
        return json.load(stream)


def elaboration_options(opts: list[str]) -> list[str]:
    """The gate owns uniform compiler limits, including for older pinned packages."""
    kept = []
    index = 0
    while index < len(opts):
        option = opts[index]
        if option in ('--tstack', '-s'):
            index += 2
            continue
        if option == '-D' and index + 1 < len(opts) and opts[index + 1].split('=', 1)[0] in ('maxHeartbeats', 'maxRecDepth'):
            index += 2
            continue
        if not (option.startswith(('-DmaxHeartbeats=', '-DmaxRecDepth=', '--tstack='))
                or (option.startswith('-s') and option[2:].isdigit())):
            kept.append(option)
        index += 1
    caps = testnet_caps()['elaboration']
    return kept + [f"-DmaxHeartbeats={caps['max_heartbeats']}",
                   f"-DmaxRecDepth={caps['max_rec_depth']}",
                   f"--tstack={caps['worker_stack_bytes'] // 1024}"]


def production_environment_reason(env: LeanEnv, descriptor: dict | None = None) -> str | None:
    """Return the environment-stage reason; fixtures may be passed only as arguments."""
    expected = production_descriptor() if descriptor is None else descriptor
    if env.lean_version != expected['lean']:
        return 'environment-retired'
    pins = env.pins()
    if any(pins[key] != expected[key] for key in ('mathlib', 'lake_manifest_sha256', 'packages')):
        return 'environment-mismatch'
    if 'artifact_profiles' in expected:
        from . import artifact_pins
        profile = platform.system() + '-' + platform.machine()
        if profile not in expected['artifact_profiles']:
            return 'environment-unpinned'
        try:
            if artifact_pins.snapshot(env) != expected['artifact_profiles'][profile]:
                return 'environment-mismatch'
        except (OSError, ValueError, RuntimeError):
            return 'environment-mismatch'
    return None


def load_production_env(path: str) -> LeanEnv:
    """Load provisioned paths while taking production pins exclusively from tracked data."""
    provisioned = load_env(path)
    reason = production_environment_reason(provisioned)
    if reason:
        raise ValueError(reason)
    expected = production_descriptor()
    return LeanEnv(os.path.realpath(provisioned.toolchain), [os.path.realpath(path) for path in provisioned.lean_path], expected['lean'],
                   dict(expected['packages']), expected['lake_manifest_sha256'], provisioned.scratch,
                   dict(provisioned.extra))


def process_env(env: LeanEnv, extra_lean_path: list[str] | tuple[str, ...] = ()) -> dict:
    tmp = os.path.join(env.scratch, "tmp")
    FS.makedirs(tmp, exist_ok=True)
    home = os.path.join(env.scratch, 'home')
    FS.makedirs(home, exist_ok=True)
    toolchain = os.path.realpath(env.toolchain)
    return {
        "PATH": f"{toolchain}/bin:{SYSTEM_PATH}",
        "LEAN_SYSROOT": toolchain,
        "LEAN_PATH": ":".join(os.path.realpath(path) for path in [*extra_lean_path, *env.lean_path]),
        "HOME": home,
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
    killed_by: str = ""                   # reason returned by the ``watch`` hook, if it stopped the run
    boundary: dict | None = None          # trusted production namespace/cgroup receipt
    process_receipt: dict | None = None   # copied host observation, independent of temporary receipt files


class ResourceUnavailable(RuntimeError):
    """A native code-execution window ended without a deterministic candidate verdict."""


class BoundaryInvalid(RuntimeError):
    """A trusted boundary rejected the submission for a deterministic containment reason."""


def boundary_outcome(error):
    """Preserve a typed boundary exception as one closed gate outcome."""
    return dict(verdict='INVALID' if isinstance(error, BoundaryInvalid) else 'ERROR', reason=str(error))


def enforce_boundary(result):
    """Translate trusted execution receipts without assigning meaning to payload exit codes."""
    from .sandbox import SandboxUnavailable, normalized_boundary
    result.boundary = normalized_boundary(result.boundary)
    if result.timeout or result.memkill or result.killed_by:
        raise ResourceUnavailable('resource-unavailable')
    if result.boundary and result.boundary.get('verdict') != 'PASS':
        reason = result.boundary.get('reason', 'sandbox-unavailable')
        if result.boundary.get('verdict') == 'INVALID':
            raise BoundaryInvalid(reason)
        if reason == 'resource-unavailable':
            raise ResourceUnavailable(reason)
        raise SandboxUnavailable(reason)
    return result


def run_submission(cmd: list[str], env: dict, cwd: str, timeout: float, rss_limit_mb: int | None = None,
                   watch=None, poll_s: float = 0.25) -> RunResult:
    """Execute native submission code only through an installed, argument-provisioned boundary."""
    from .sandbox import Sandbox, SandboxUnavailable, submission_code
    if not isinstance(getattr(run_process, 'submission_boundary', None), Sandbox):
        raise SandboxUnavailable('sandbox-unavailable')
    env = dict(env, GOMAXPROCS='4', CARGO_BUILD_JOBS='4', RAYON_NUM_THREADS='4', RUST_TEST_THREADS='4')
    with submission_code():
        result = run_process(cmd, env, cwd, timeout, rss_limit_mb, watch, poll_s)
    return enforce_boundary(result)


def run_process(cmd: list[str], env: dict, cwd: str, timeout: float, rss_limit_mb: int | None = None,
                watch=None, poll_s: float = 2.0) -> RunResult:
    """Run ``cmd`` in its own session; kill the whole group on timeout, when RSS exceeds the limit, or
    when ``watch()`` (called every poll) returns a non-empty reason."""
    t0 = time.time()
    p = subprocess.Popen(cmd, env=env, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         start_new_session=True)
    state = {"memkill": False, "peak": 0, "killed_by": ""}
    stop = threading.Event()

    def guard() -> None:
        while not stop.wait(poll_s):
            why = watch() if watch else ""
            if why:
                state["killed_by"] = why
                _kill(p)
                return
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
                     state["memkill"], state["peak"] // 1024, state["killed_by"])


def _kill(p: subprocess.Popen) -> None:
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


# Optional process-wide Lean limits (unset by default): at most ``slots`` concurrent ``lean`` processes, and an
# RSS limit for runs that do not pass their own.  Set once by a driver before its worker pool starts.
_LIMITS: dict = {"slots": None, "rss_mb": None}


def set_lean_limits(slots: int | None = None, rss_mb: int | None = None) -> None:
    """Limit concurrent ``lean`` processes (``slots``) and give runs without their own limit an RSS cap."""
    _LIMITS["slots"] = threading.BoundedSemaphore(slots) if slots else None
    _LIMITS["rss_mb"] = rss_mb


def run_lean(env: LeanEnv, args: list[str], cwd: str, timeout: float, extra_lean_path=(),
             rss_limit_mb: int | None = None, *, submission_controlled: bool = False) -> RunResult:
    slots = _LIMITS["slots"]
    rss = rss_limit_mb or _LIMITS["rss_mb"]
    task_env = process_env(env, extra_lean_path)
    command = [env.lean, '-j', '4'] + args
    context = _SANDBOX.get()
    runner = run_process
    if context is not None:
        sandbox, readable, writable, guarded = context
        runner = guarded or runner
        command = sandbox.command(command, task_env, [os.path.realpath(env.toolchain),
                                  *(os.path.realpath(path) for path in env.lean_path), *readable],
                                  [task_env['TMPDIR'], *writable], submission_controlled=submission_controlled)
    if slots is None:
        return runner(command, task_env, cwd, timeout, rss)
    with slots:
        return runner(command, task_env, cwd, timeout, rss)


def parse_messages(out: str, *, structured_only: bool = False) -> list[dict]:
    """Parse Lean diagnostics; compilation can exclude synthetic raw-line reports.

    Other consumers retain their earlier prefixed, bounded raw-line messages.
    The complete unmodified output remains in RunResult.out and compile.log.
    """
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
        if not structured_only:
            msgs.append({"severity": "error", "data": "non-JSON output: " + ln[:500], "pos": None})
    return msgs


def compile_module(env: LeanEnv, src_root: str, rel_path: str, build_dir: str, opts: list[str], timeout: float,
                   extra_lean_path=()) -> tuple[RunResult, list[dict]]:
    """Compile ``src_root/rel_path`` to ``build_dir`` (``.olean`` + ``.ilean``) with ``--json``."""
    stem = rel_path[:-len(".lean")]
    olean = os.path.join(build_dir, stem + ".olean")
    FS.makedirs(os.path.dirname(olean), exist_ok=True)
    args = elaboration_options(opts) + ["--json", "-R", src_root, "-o", olean, "-i", os.path.join(build_dir, stem + ".ilean"),
                   os.path.join(src_root, rel_path)]
    r = run_lean(env, args, src_root, timeout, extra_lean_path=[build_dir, *extra_lean_path])
    return r, parse_messages(r.out)


def errors(msgs: list[dict]) -> list[dict]:
    return [m for m in msgs if m.get("severity") == "error"]


def fmt_msg(m: dict) -> str:
    pos = m.get("pos") or {}
    return f"{pos.get('line')}:{pos.get('column')}: {m.get('data', '')[:400]}"
