"""Full-closure exports and the pinned Comparator's built-in two-kernel path."""
from __future__ import annotations

from dataclasses import dataclass, replace
import hashlib
import json
import os
from .safe_fs import Path
import platform
import time
import uuid

from . import correctness_runtime as CR
from . import lean_runner as L
from .sandbox import Sandbox, boundary_failure

PRIMITIVES = [
    'Nat.add', 'Nat.sub', 'Nat.mul', 'Nat.pow', 'Nat.gcd', 'Nat.div', 'Nat.mod', 'Nat.beq', 'Nat.ble',
    'Nat.land', 'Nat.lor', 'Nat.xor', 'Nat.shiftLeft', 'Nat.shiftRight', 'String.ofList', 'Char.ofNat',
    'List', 'eagerReduce', 'Nat', 'String', 'String.mk', 'Char', 'optParam', 'autoParam', 'semiOutParam', 'outParam',
]
QUOTIENTS = ['Quot', 'Quot.mk', 'Quot.lift', 'Quot.ind']


class ToolsUnavailable(ValueError):
    """A declared checking tool or its exact binary pin is unavailable."""


def digest(path: str | Path) -> str:
    """Hash executed or exported bytes with bounded read buffers."""
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


@dataclass(frozen=True)
class Tools:
    comparator: str
    exporter: str
    nanoda: str
    kernel_prefix: str
    landrun: str | None
    comparator_version: str = 'v4.34.1'

    @classmethod
    def load(cls, root: str) -> Tools:
        """Load provisioned paths using only the tracked component and binary pins."""
        data = json.loads((Path(__file__).parent / 'data/acceptance_tools_v2.json').read_text())
        profile = platform.system() + '-' + platform.machine()
        pins = data['binary_profiles'].get(profile)
        if pins is None:
            raise ToolsUnavailable('kernels-unavailable')
        directory = Path(root).resolve()
        binaries = {}
        for name in ('comparator', 'exporter', 'nanoda'):
            row = pins[name]
            binary = directory / row['path']
            if not binary.is_file() or digest(binary.resolve()) != row['sha256'] or not os.access(binary, os.X_OK):
                raise ToolsUnavailable('kernels-unavailable')
            binaries[name] = str(binary.resolve())
        kernel_prefix = (directory / pins['kernel_prefix']).resolve()
        landrun = None
        if platform.system() == 'Linux':
            row = data.get('sandbox', {}).get('launcher_path')
            landrun = str(directory / row) if row else None
        return cls(**binaries, kernel_prefix=str(kernel_prefix), landrun=landrun,
                   comparator_version=data['comparator']['lean'])


def resource_reason(result: L.RunResult) -> str | None:
    """Wall, memory and observation failures are availability errors, not mathematical verdicts."""
    if result.timeout or result.memkill or result.killed_by or result.rc in (-9, -15, 124, 137):
        return 'resource-unavailable'
    return None


def prepare_reference(env: L.LeanEnv, problem: dict, package: str, work: str, sealed: str,
                      tools: Tools, sandbox: Sandbox, timeout: float, axioms: list[str]) -> dict:
    """Build and seal a pristine Challenge before compiling any solution code."""
    from . import check as C

    started = time.monotonic()
    if sandbox.mode == 'production' or sandbox.disabled_layer:
        sandbox = replace(sandbox, role='reference')
    statement = Path(package, problem['checker']['statement_file']).read_text()
    compiled = C.compile_in_sandbox(env, problem, package, statement, work, timeout, sandbox)
    Path(work).mkdir(parents=True, exist_ok=True)
    with Path(work, 'COMPILE-POSTCHECK.json').open('x') as stream:
        json.dump(compiled, stream, indent=2)
    if 'error' in compiled:
        return dict(verdict='ERROR', reason=compiled['error'], compiled=compiled)
    if compiled.get('boundary') and compiled['boundary']['verdict'] != 'PASS':
        return dict(verdict=compiled['boundary']['verdict'], reason=compiled['boundary']['reason'], compiled=compiled)
    if compiled.get('compile_timeout') or compiled.get('compile_memkill'):
        return dict(verdict='ERROR', reason='resource-unavailable', compiled=compiled)
    if compiled.get('compile_rc') != 0 or compiled.get('n_errors') or compiled.get('post') is None:
        return dict(verdict='ERROR', reason='challenge-build-unavailable', compiled=compiled)
    post = compiled['post']
    if post.get('timeout') or post.get('memkill') or post.get('killed_by'):
        return dict(verdict='ERROR', reason='resource-unavailable', compiled=compiled)
    if post.get('replay') != 'ok':
        return dict(verdict='ERROR', reason='challenge-build-unavailable', compiled=compiled)
    result = export_module(env, 'ProofMod', problem['checker']['theorem_fqn'], sealed, tools, sandbox,
                           str(Path(work) / 'proof'), [str(Path(work) / 'proof'), str(Path(work) / 'build')],
                           timeout, axioms)
    result['reference_type_sha256'] = post.get('type_sha256')
    result['compile_seconds'] = compiled.get('compile_secs', 0)
    result['pristine_compile_seconds'] = (sum(row['secs'] for row in compiled.get('imports', []))
                                         + compiled.get('compile_secs', 0) + post.get('secs', 0))
    result['stage_seconds'] = time.monotonic() - started
    return result


def export_module(env: L.LeanEnv, module: str, theorem: str, output: str, tools: Tools, sandbox: Sandbox,
                  cwd: str, extra_lean_path: list[str], timeout: float, axioms: list[str]) -> dict:
    """Export the target's transitive type/value closure, including primitive and import dependencies."""
    target = Path(output)
    target.parent.mkdir(parents=True, exist_ok=True)
    task_env = L.process_env(env, extra_lean_path)
    roots = list(dict.fromkeys([theorem, *axioms, *PRIMITIVES, *QUOTIENTS]))
    argv = [tools.exporter, module, '--', *roots]
    command = sandbox.command(argv, task_env, [env.toolchain, *env.lean_path, *extra_lean_path, cwd],
                              [task_env['TMPDIR']])
    run = CR.runner(str(target.parent / 'processes' / target.name), timeout)
    result = run(command, task_env, cwd, timeout, 12000)
    record = dict(stage='closure-export', assurance=sandbox.mode, seconds=result.secs,
                  peak_rss_mb=result.peak_rss_mb, rc=result.rc, boundary=result.boundary,
                  process_receipt=result.process_receipt)
    stopped = boundary_failure(result)
    if stopped:
        return dict(record, verdict=stopped['verdict'], reason=stopped['reason'])
    reason = resource_reason(result)
    if reason:
        return dict(record, verdict='ERROR', reason=reason)
    if result.rc != 0:
        return dict(record, verdict='ERROR', reason='export-unavailable', stdout=result.out[-2000:])
    with target.open('x', encoding='utf-8') as stream:
        stream.write(result.out)
    target.chmod(0o444)
    return dict(record, verdict='PASS', reason=None, path=str(target), sha256=digest(target),
                bytes=target.stat().st_size, roots=roots)


def compare_exports(challenge: str, solution: str, theorem: str, tools: Tools, sandbox: Sandbox,
                    work: str, timeout: float, axioms: list[str], max_constants: int = 1000000,
                    nanoda_binary: str | None = None) -> dict:
    """Run Comparator once; both its empty Lean kernel and built-in nanoda consume the sealed solution."""
    directory = Path(work)
    directory.mkdir(parents=True, exist_ok=True)
    before = {path: digest(path) for path in (challenge, solution)}
    config = dict(challenge_module='Challenge', solution_module='ProofMod', theorem_names=[theorem],
                  permitted_axioms=axioms, enable_nanoda=True, max_exported_constants=max_constants)
    config_path = directory / 'comparator.json'
    with config_path.open('x') as stream:
        json.dump(config, stream, sort_keys=True)
    tmp = directory / 'tmp'
    tmp.mkdir()
    env = {
        'PATH': tools.kernel_prefix + '/bin:' + L.SYSTEM_PATH,
        'HOME': str(directory), 'TMPDIR': str(tmp), 'LANG': 'C.UTF-8',
        'COMPARATOR_LEAN4EXPORT': tools.exporter,
        'COMPARATOR_NANODA': nanoda_binary or tools.nanoda,
    }
    status_directory = None
    if sandbox.mode == 'production':
        if not sandbox.inner_executable or not Path(sandbox.inner_executable).is_file():
            return dict(stage='comparator', verdict='ERROR', reason='sandbox-unavailable', assurance=sandbox.mode)
        status_directory = Path('/run/user') / str(os.getuid()) / 'boole-gate-inner' / uuid.uuid4().hex
        status_directory.mkdir(mode=0o700, parents=True)
        inner_status = status_directory / 'inner-boundary.status'
        with inner_status.open('xb'):
            pass
        env['COMPARATOR_LANDRUN'] = sandbox.inner_executable
        env['COMPARATOR_BOUNDARY_STATUS'] = str(inner_status)
    else:
        passthrough = Path(__file__).parent / 'data/development-landrun.sh'
        env['COMPARATOR_LANDRUN'] = str(passthrough)
    command = [tools.comparator, str(config_path), '--exports', challenge, solution]
    command = sandbox.command(command, env,
                              [tools.kernel_prefix, tools.exporter, tools.nanoda, challenge, solution, str(directory)],
                              [str(directory), *([str(status_directory)] if status_directory else [])])
    run = CR.runner(str(directory / 'processes'), timeout)
    result = run(command, env, str(directory), timeout, 12000)
    signals = None
    cleanup_errors = []
    if status_directory is not None:
        try:
            raw = inner_status.read_bytes()
            if len(raw) <= 16384:
                parsed = [json.loads(line) for line in raw.splitlines()]
                if len(parsed) <= 32 and all(isinstance(row, dict) for row in parsed):
                    signals = parsed
        except (OSError, ValueError):
            pass
        finally:
            for cleanup in (lambda: inner_status.unlink(missing_ok=True), status_directory.rmdir):
                try:
                    cleanup()
                except Exception as ex:
                    cleanup_errors.append(type(ex).__name__ + ': ' + str(ex)[:2048])
    (directory / 'comparator.stdout').write_text(result.out)
    record = dict(stage='comparator', assurance='test-fixture' if nanoda_binary else sandbox.mode,
                  seconds=result.secs, peak_rss_mb=result.peak_rss_mb, rc=result.rc,
                  solution_export_sha256=before[solution], comparator_lean=tools.comparator_version,
                  boundary=result.boundary, inner_signals=signals, process_receipt=result.process_receipt,
                  cleanup_errors=cleanup_errors)
    if any(digest(path) != value for path, value in before.items()):
        return dict(record, verdict='INVALID', reason='export-integrity')
    if sandbox.mode == 'production' and result.rc not in (0, 2):
        # Only 0/2 are Comparator verdicts. Even plausible kernel output cannot
        # turn a crash, signal or unknown exit status into a semantic rejection.
        observation = (result.boundary or {}).get('observation') or {}
        resource_events = observation.get('oom_kill_events') or observation.get('pids_max_events')
        guarded_resource_stop = result.timeout or result.memkill or result.killed_by in {'wall-time', 'memory-rss', 'checker-deadline'}
        reason = ('resource-unavailable' if result.rc != 70 and (guarded_resource_stop or resource_events)
                  else 'sandbox-unavailable')
        return dict(record, verdict='ERROR', reason=reason)
    if resource_reason(result):
        stopped = boundary_failure(result)
        if stopped:
            return dict(record, verdict=stopped['verdict'], reason=stopped['reason'])
        return dict(record, verdict='ERROR', reason='resource-unavailable')
    stopped = boundary_failure(result)
    if stopped:
        return dict(record, verdict=stopped['verdict'], reason=stopped['reason'])
    if sandbox.mode == 'production':
        # Patched Comparator: 2 is a semantic rejection before any kernel launch; 70 is transport/setup.
        if result.rc == 70 or signals is None or any('setup_error' in row for row in signals):
            return dict(record, verdict='ERROR', reason='sandbox-unavailable')
        if (any(row.get('inner_exec') != 'ok' or type(row.get('exit')) is not int
                or not 0 <= row['exit'] < 128 for row in signals)
                or (result.rc != 2 and not signals)):
            return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    lean_accepts = 'Lean default kernel accepts the solution' in result.out
    nanoda_accepts = 'nanoda kernel accepts the solution' in result.out
    record.update(lean_accepts=lean_accepts, nanoda_accepts=nanoda_accepts)
    if result.rc == 0 and lean_accepts and nanoda_accepts:
        return dict(record, verdict='PASS', reason=None)
    if 'resource-cap:' in result.out:
        return dict(record, verdict='INVALID', reason='resource-cap')
    if lean_accepts != nanoda_accepts:
        return dict(record, verdict='INVALID', reason='kernel-disagreement')
    if 'axiom' in result.out.lower():
        return dict(record, verdict='INVALID', reason='disallowed-axiom')
    if 'mismatch' in result.out.lower() or 'not match' in result.out.lower():
        return dict(record, verdict='INVALID', reason='comparator-statement-mismatch')
    return dict(record, verdict='INVALID' if result.rc else 'ERROR', reason='kernel-rejection')
