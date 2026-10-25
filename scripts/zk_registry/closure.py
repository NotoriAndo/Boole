"""Full-closure exports and the pinned Comparator's built-in two-kernel path."""
from __future__ import annotations

from dataclasses import dataclass, replace
import hashlib
import json
import os
from .safe_fs import Path
from . import safe_fs as FS
import platform
import sys
import time
import uuid

from . import correctness_runtime as CR
from . import lean_runner as L
from . import tool_exit as X
from .statement_identity import VERSION as IDENTITY_VERSION
from .sandbox import Sandbox, boundary_failure
from .sandbox import SandboxUnavailable

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
    lean_replay: str = ''

    @classmethod
    def load(cls, root: str) -> Tools:
        """Load provisioned paths using only the tracked component and binary pins."""
        try:
            return cls._load(root)
        except (OSError, ValueError, RuntimeError, KeyError):
            # Provisioned paths are trusted inputs. Python <=3.12 reports
            # resolve() symlink loops as RuntimeError rather than OSError.
            raise ToolsUnavailable('kernels-unavailable') from None

    @classmethod
    def _load(cls, root: str) -> Tools:
        data = json.loads((Path(__file__).parent / 'data/acceptance_tools_v2.json').read_text())
        profile = platform.system() + '-' + platform.machine()
        pins = data['binary_profiles'].get(profile)
        if pins is None:
            raise ToolsUnavailable('kernels-unavailable')
        directory = Path(root).resolve()
        binaries = {}
        for name in ('comparator', 'exporter', 'nanoda', 'lean_replay'):
            row = pins[name]
            binary = directory / row['path']
            if not binary.is_file() or digest(binary.resolve()) != row['sha256'] or not os.access(binary, os.X_OK):
                raise ToolsUnavailable('kernels-unavailable')
            binaries[name] = str(binary.resolve())
        kernel_prefix = (directory / pins['kernel_prefix']).resolve()
        from . import artifact_pins
        authority = L.production_descriptor().get('artifact_profiles', {}).get(profile)
        if authority is None:
            raise ToolsUnavailable('kernels-unavailable')
        try:
            actual = artifact_pins.snapshot(L.LeanEnv(str(kernel_prefix), [], '', {}, '', ''))
            expected = dict(authority, olean_roots=authority['olean_roots'][:1])
            if actual != expected:
                raise ToolsUnavailable('kernels-unavailable')
        except (OSError, ValueError, KeyError):
            raise ToolsUnavailable('kernels-unavailable') from None
        landrun = None
        if platform.system() == 'Linux':
            row = data.get('sandbox', {}).get('launcher_path')
            landrun = str(directory / row) if row else None
        return cls(**binaries, kernel_prefix=str(kernel_prefix), landrun=landrun,
                   comparator_version=data['comparator']['lean'])


def resource_reason(result: L.RunResult) -> str | None:
    """Wall, memory and observation failures are availability errors, not mathematical verdicts."""
    if result.killed_by.startswith('output-cap') or result.output_capped:
        return 'output-cap'
    if result.killed_by.startswith('capture-failed'):
        return 'capture-failed'
    if result.timeout or result.memkill or result.killed_by or L.availability_exit(result.rc):
        return 'resource-unavailable'
    return None


def trusted_directory(path, exposure: dict) -> None:
    """A trusted directory must never alias an exposed root; configuration faults are ERROR."""
    try:
        value = Path(path)
        # Missing children inherit their nearest existing parent's containment check.
        while not value.exists():
            parent = value.parent
            if parent == value:
                raise OSError('trusted directory unavailable')
            value = parent
        identity = FS.root_identity(value)
        if identity in exposure.values():
            raise SandboxUnavailable('sandbox-unavailable')
    except OSError:
        raise SandboxUnavailable('sandbox-unavailable') from None


def statement_identity(export: str, theorem: str, work: str, timeout: float, *, task_env=None, cwd=None, expected_sha256=None, input_role='reference') -> dict:
    """Parse sealed export bytes in a bounded trusted worker; availability never becomes INVALID."""
    stage = X.outcome(X.Exit.ACCEPT, 'identity', input_role)['stage']
    if timeout <= 0:
        return dict(verdict='ERROR', reason='resource-unavailable')
    try:
        directory = Path(work)
        directory.mkdir(parents=True, exist_ok=True)
        task_env = task_env or dict(PATH=L.SYSTEM_PATH, HOME=str(directory), TMPDIR=str(directory), LANG='C.UTF-8')
        run = CR.runner(str(directory / 'processes'), timeout)
        from . import bounded_capture as BC
        with BC.capture(stdout=BC.SmallSink()):
            result = run([sys.executable, '-I', '-B', str(Path(__file__).with_name('statement_identity.py')),
                          str(Path(export).absolute()), theorem, '--timeout', str(timeout)],
                         task_env, cwd or str(directory), timeout, 19000)
    except (OSError, ValueError):
        return dict(verdict='ERROR', reason='statement-identity-unavailable')
    record = dict(stage=stage, seconds=result.secs, peak_rss_mb=result.peak_rss_mb, rc=result.rc,
                  process_receipt=result.process_receipt)
    if resource_reason(result):
        return dict(record, verdict='ERROR', reason=resource_reason(result))
    if result.rc in (X.Exit.CAP, X.Exit.FAULT):
        return dict(record, **X.outcome(result.rc, 'identity', input_role))
    try:
        semantic = json.loads(result.out) if result.rc == 0 else {}
        if not isinstance(semantic, dict):
            raise ValueError('invalid worker response')
        if semantic.get('export_sha256') != (expected_sha256 or digest(export)):
            raise ValueError('export changed')
        value = semantic['statement_closure_sha256']
        if not isinstance(value, str) or len(value) != 64 or any(c not in '0123456789abcdef' for c in value):
            raise ValueError('invalid semantic identity')
        if semantic.get('statement_closure_version') != IDENTITY_VERSION:
            raise ValueError('unknown semantic identity version')
    except (OSError, ValueError, KeyError, TypeError):
        return dict(record, verdict='ERROR', reason='statement-identity-unavailable')
    return dict(record, verdict='PASS', reason=None, **semantic)


@FS.guard_trusted_io
def reference_identity(env: L.LeanEnv, module: str, theorem: str, cwd: str,
                       lean_path: list[str], timeout: float) -> dict:
    """Bind a compiled reference using explicitly provisioned, pinned export tools.

    Temporary sealed exports live outside every compilation writer root. The returned
    identity binds bytes, never an environment path or an unelaborated statement string.
    """
    import tempfile

    tools = env.extra.get('acceptance_tools')
    try:
        if not isinstance(tools, Tools):
            root = env.extra.get('acceptance_tools_root')
            if not root:
                return dict(verdict='ERROR', reason='kernels-unavailable')
            tools = Tools.load(root)
        sandbox = L.current_sandbox() or env.extra.get('acceptance_sandbox') or Sandbox.production(tools.landrun)
        with tempfile.TemporaryDirectory(prefix='boole-reference-') as temporary:
            record = export_module(env, module, theorem, str(Path(temporary) / 'Challenge.ndjson'),
                                   tools, sandbox, cwd, lean_path, timeout, [],
                                   exposure=sandbox.exposed_writers, identity=True)
        if record['verdict'] != 'PASS':
            return dict(verdict='ERROR', reason=record['reason'])
        return dict(verdict='PASS', statement_closure_sha256=record['statement_closure_sha256'])
    except (OSError, ValueError, KeyError, TypeError):
        return dict(verdict='ERROR', reason='statement-identity-unavailable')


@FS.guard_trusted_io
def prepare_reference(env: L.LeanEnv, problem: dict, package: str, work: str, sealed: str,
                      tools: Tools, sandbox: Sandbox, timeout: float, axioms: list[str]) -> dict:
    """Build and seal a pristine Challenge before compiling any solution code."""
    from . import check as C, package as P

    if getattr(tools, 'lean_replay', ''):
        env = replace(env, extra=dict(env.extra, acceptance_tools=tools,
                                      kernel_replay_driver=tools.lean_replay))
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
    if post.get('error')=='checker-input-unavailable':
        return dict(verdict='ERROR',reason='checker-input-unavailable',compiled=compiled)
    if post.get('error')=='output-cap' or str(post.get('killed_by','')).startswith('output-cap'):
        return dict(verdict='ERROR',reason='output-cap',compiled=compiled)
    if str(post.get('killed_by','')).startswith('capture-failed'):
        return dict(verdict='ERROR',reason='capture-failed',compiled=compiled)
    if post.get('timeout') or post.get('memkill') or post.get('killed_by'):
        return dict(verdict='ERROR', reason='resource-unavailable', compiled=compiled)
    if post.get('replay') != 'ok':
        return dict(verdict='ERROR', reason='challenge-build-unavailable', compiled=compiled)
    result = export_module(env, 'ProofMod', problem['checker']['theorem_fqn'], sealed, tools, sandbox,
                           str(Path(work) / 'proof'), [str(Path(work) / 'build'), str(Path(work) / 'proof')],
                           timeout, axioms, identity=not P.candidate_derived(problem))
    if P.candidate_derived(problem):
        result.update(statement_closure_sha256=None,
                      statement_closure_reason="candidate-derived-identity-not-required")
    result['reference_type_sha256'] = post.get('type_sha256')
    result['compile_seconds'] = compiled.get('compile_secs', 0)
    result['pristine_compile_seconds'] = (sum(row['secs'] for row in compiled.get('imports', []))
                                         + compiled.get('compile_secs', 0) + post.get('secs', 0))
    result['stage_seconds'] = time.monotonic() - started
    return result


@FS.guard_trusted_io
def export_module(env: L.LeanEnv, module: str, theorem: str, output: str, tools: Tools, sandbox: Sandbox,
                  cwd: str, extra_lean_path: list[str], timeout: float, axioms: list[str], *,
                  exposure: dict | None = None, identity: bool = False, input_role: str = 'reference') -> dict:
    """Export the target's transitive type/value closure, including primitive and import dependencies."""
    prior = dict(exposure or {})
    stage = X.outcome(X.Exit.ACCEPT, 'export', input_role)['stage']
    if sandbox.mode == 'production' or sandbox.disabled_layer:
        sandbox = replace(sandbox, role='reference')
    target = Path(output)
    started = time.monotonic()
    if timeout <= 0:
        return dict(stage='closure-export', verdict='ERROR', reason='resource-unavailable')
    try:
        trusted_directory(target.parent, prior)
        trusted_directory(target.parent / 'processes', prior)
        if identity:
            trusted_directory(target.parent / (target.name + '-identity'), prior)
    except SandboxUnavailable:
        return dict(stage='closure-export', verdict='ERROR', reason='sandbox-unavailable', assurance=sandbox.mode)
    target.parent.mkdir(parents=True, exist_ok=True)
    task_env = L.process_env(env, extra_lean_path)
    # Lean commits package lookup to the first matching root, even an empty
    # directory. Explicitly precede all submission roots with pinned core/libs.
    task_env['LEAN_PATH'] = os.pathsep.join(dict.fromkeys(os.path.realpath(path) for path in
        [os.path.join(env.toolchain, 'lib', 'lean'), *env.lean_path, *extra_lean_path]))
    roots = list(dict.fromkeys([theorem, *axioms, *PRIMITIVES, *QUOTIENTS]))
    argv = [tools.exporter, module, '--', *roots]
    command = sandbox.command(argv, task_env, [env.toolchain, *env.lean_path, *extra_lean_path, cwd],
                              [task_env['TMPDIR']], prior_exposure=prior)
    run = CR.runner(str(target.parent / 'processes' / target.name), timeout)
    from . import bounded_capture as BC
    sink=BC.FileSink(str(target))
    with BC.capture(stdout=sink):
        result = run(command, task_env, cwd, timeout, 12000)
        # Adapt older injected runners through the same bounded sink contract.
        if not result.streams:sink.feed(result.out.encode('utf-8'))
    record = dict(stage=stage, assurance=sandbox.mode, seconds=result.secs,
                  peak_rss_mb=result.peak_rss_mb, rc=result.rc, boundary=result.boundary,
                  process_receipt=result.process_receipt, streams=result.streams)
    stopped = boundary_failure(result)
    if stopped and stopped['reason'] != 'output-cap':
        return dict(record, verdict=stopped['verdict'], reason=stopped['reason'])
    if sink.capture_failed or any(s.get('capture_failed') for s in result.streams.values()):
        return dict(record, verdict='ERROR', reason='capture-failed')
    if result.timeout or result.memkill or (result.killed_by and not result.killed_by.startswith('output-cap')):
        return dict(record, verdict='ERROR', reason=resource_reason(result))
    if sink.capped:
        return dict(record, **X.outcome(X.Exit.CAP, 'export', input_role))
    reason = resource_reason(result)
    if reason:
        return dict(record, verdict='ERROR', reason=reason)
    if result.rc in (X.Exit.CAP, X.Exit.FAULT, X.Exit.UNAVAILABLE):
        return dict(record, **X.outcome(result.rc, 'export', input_role))
    if result.rc != 0:
        return dict(record, verdict='ERROR', reason='export-unavailable', stdout=result.out[-2000:])
    target.chmod(0o444)
    exported_sha = digest(target)
    exported = dict(record, verdict='PASS', reason=None, path=str(target), sha256=exported_sha,
                    bytes=target.stat().st_size, roots=roots, theorem=theorem)
    if not identity:
        return exported
    remaining = timeout - (time.monotonic() - started)
    if remaining <= 0:
        return dict(record, verdict='ERROR', reason='resource-unavailable')
    semantic = statement_identity(str(target), theorem, str(target.parent / (target.name + '-identity')),
                                  remaining, expected_sha256=exported_sha, input_role=input_role)
    record['identity_worker'] = semantic
    exported['identity_worker'] = semantic
    if semantic['verdict'] != 'PASS':
        return dict(record, verdict=semantic['verdict'], reason=semantic['reason'])
    return dict(exported,
                **{k: semantic[k] for k in ('statement_closure_sha256', 'statement_closure_version')})


@FS.guard_trusted_io
def compare_exports(challenge: str, solution: str, theorem: str, tools: Tools, sandbox: Sandbox,
                    work: str, timeout: float, axioms: list[str], max_constants: int = 1000000,
                    nanoda_binary: str | None = None, *, fuel_budget=None, candidate_declarations=None) -> dict:
    """Run Comparator once; both its empty Lean kernel and built-in nanoda consume the sealed solution."""
    prior = dict(sandbox.exposed_writers)
    try:
        for path in (work, Path(work) / 'tmp', Path(work) / 'processes',
                     Path(challenge).parent, Path(solution).parent):
            trusted_directory(path, prior)
    except SandboxUnavailable:
        return dict(stage='comparator', verdict='ERROR', reason='sandbox-unavailable', assurance=sandbox.mode)
    if sandbox.mode == 'production' or sandbox.disabled_layer:
        sandbox = replace(sandbox, role='reference')
    directory = Path(work)
    directory.mkdir(parents=True, exist_ok=True)
    protected = [challenge, solution]
    if fuel_budget is not None:
        from .fuel_caps import Budget
        if not isinstance(fuel_budget, Budget):
            return dict(stage='comparator', verdict='ERROR', reason='environment-fuel-cap-unavailable')
        try:
            parent = Path(fuel_budget.state).resolve(strict=True).parent
            for root in prior:
                exposed = Path(root).resolve(strict=False)
                if parent == exposed or parent.is_relative_to(exposed):
                    raise SandboxUnavailable('sandbox-unavailable')
            # Identity checks also catch a renamed/aliased previously exposed
            # ancestor. A nested cache never becomes trusted by its new name.
            while parent != parent.parent:
                if FS.root_identity(parent) in prior.values():
                    raise SandboxUnavailable('sandbox-unavailable')
                parent = parent.parent
            if digest(fuel_budget.state) != fuel_budget.state_sha256:
                return dict(stage='comparator', verdict='ERROR', reason='environment-certified-unavailable')
        except (OSError, ValueError, RuntimeError, SandboxUnavailable):
            return dict(stage='comparator', verdict='ERROR', reason='environment-certified-unavailable',
                        assurance=sandbox.mode)
        # Parent inode checks plus before/after content hashes do not exclude a
        # concurrent writer swapping the state file between the reads. Provision
        # this root immutable; file-inode race exclusion is not claimed.
        protected.append(fuel_budget.state)
    before = {path: digest(path) for path in protected}
    lean_limits = dict(L.testnet_caps()['lean'])
    config = dict(challenge_module='Challenge', solution_module='ProofMod', theorem_names=[theorem],
                  permitted_axioms=axioms, enable_nanoda=True, max_exported_constants=max_constants,
                  lean_kernel=lean_limits, nanoda_kernel=dict(L.testnet_caps()['nanoda']))
    if fuel_budget is not None:
        config['lean_kernel']['aggregate_raw'] = fuel_budget.lean
        config['nanoda_kernel']['aggregate_work'] = fuel_budget.nanoda
        config['certified_base_olean'] = fuel_budget.state
        if candidate_declarations is not None:
            if (not isinstance(candidate_declarations, list)
                    or any(not isinstance(name, str) for name in candidate_declarations)
                    or len(set(candidate_declarations)) != len(candidate_declarations)):
                return dict(stage='comparator', verdict='ERROR', reason='environment-certified-unavailable')
            config['candidate_declarations'] = candidate_declarations
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
        'LEAN_STACK_SIZE_KB': str(lean_limits['worker_stack_bytes'] // 1024),
        'LEAN_NAT_MAX_SIZE': str(lean_limits['literal_bytes']),
    }
    comparator_status = directory / 'comparator.status.jsonl'
    with comparator_status.open('xb'):
        pass
    env['COMPARATOR_STATUS_FILE'] = str(comparator_status)
    status_directory = None
    if sandbox.mode == 'production':
        if not sandbox.inner_executable or not Path(sandbox.inner_executable).is_file():
            return dict(stage='comparator', verdict='ERROR', reason='sandbox-unavailable', assurance=sandbox.mode)
        status_directory = Path('/run/user') / str(os.getuid()) / 'boole-gate-inner' / uuid.uuid4().hex
        trusted_directory(status_directory, prior)
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
                              [tools.kernel_prefix, tools.exporter, tools.nanoda, *protected, str(directory)],
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
                  cleanup_errors=cleanup_errors,detail='')
    if fuel_budget is not None:
        record['fuel_cap'] = dict(reference=fuel_budget.reference, environment=fuel_budget.environment,
                                  units=dict(lean=fuel_budget.lean, nanoda=fuel_budget.nanoda),
                                  state_sha256=fuel_budget.state_sha256)
    if any(digest(path) != value for path, value in before.items()):
        return dict(record, verdict='ERROR', reason='export-integrity')
    if result.output_capped or result.killed_by.startswith(('output-cap','capture-failed')):
        return dict(record, verdict='ERROR', reason=resource_reason(result))
    if result.rc not in (0, 2, 3, 4, 70):
        # Only 0/2/3 reach the structured status validator. Plausible output cannot
        # turn a crash, signal or unknown exit status into a semantic rejection.
        observation = (result.boundary or {}).get('observation') or {}
        resource_events = observation.get('oom_kill_events') or observation.get('pids_max_events')
        guarded_resource_stop = resource_reason(result) is not None
        silent=(result.rc not in (None,0,2,70) and not guarded_resource_stop and not resource_events
                and not result.out.strip() and not result.stderr.strip())
        reason = ('resource-unavailable' if guarded_resource_stop or resource_events else
                  'silent-exit' if silent else 'sandbox-unavailable')
        record['detail']=('silent-exit:'+str(result.rc) if silent else
                          'signal:'+str(-result.rc) if result.rc is not None and result.rc<0 else '')
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
        if signals is None or any('setup_error' in row for row in signals):
            return dict(record, verdict='ERROR', reason='sandbox-unavailable')
        if (any(row.get('inner_exec') != 'ok' or type(row.get('exit')) is not int
                or not 0 <= row['exit'] < 128 for row in signals)
                or (result.rc not in (2, 3, 70) and not signals)):
            return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    # Kernel stdout contains attacker-controlled declaration names. Only the
    # Comparator's own private structured file establishes semantic statuses.
    try:
        raw = comparator_status.read_bytes()
        if len(raw) > 16384:
            raise ValueError('oversized comparator status')
        events = [json.loads(line) for line in raw.splitlines()]
        if not 1 <= len(events) <= 8 or any(not isinstance(e, dict) or type(e.get('version')) is not int
                or e['version'] != 1 for e in events):
            raise ValueError('invalid comparator status')
        if type(events[-1].get('rc')) is not int or events[-1] != dict(version=1, stage='final', rc=result.rc):
            raise ValueError('missing final comparator status')
        statuses = {}
        for event in events[:-1]:
            stage = event.get('stage')
            if stage in statuses:
                raise ValueError('duplicate comparator stage')
            if stage in ('lean', 'nanoda', 'quotient'):
                if set(event) != {'version', 'stage', 'result'} or event['result'] not in ('accept', 'reject'):
                    raise ValueError('invalid kernel status')
            elif stage in ('fuel', 'nanoda-fuel'):
                fields = {'debit', 'checked', 'calls', 'depth'} | ({'composed_depth'} if stage == 'nanoda-fuel' else set())
                kernel = 'nanoda' if stage == 'nanoda-fuel' else 'lean'
                if (set(event) != {'version', 'stage', 'kernel', *fields} or event['kernel'] != kernel
                        or any(type(event[key]) is not int or not 0 <= event[key] < 2**64 for key in fields)):
                    raise ValueError('invalid fuel record')
            elif stage in ('policy', 'error'):
                allowed = {'resource-cap', 'disallowed-axiom', 'comparator-statement-mismatch', 'certified-base-mismatch', 'tool-fault:replay'} if stage == 'policy' else {'export-integrity', 'kernel-unavailable', 'library-outside-certified-base'}
                if set(event) != {'version', 'stage', 'reason'} or event['reason'] not in allowed:
                    raise ValueError('invalid semantic status')
            else:
                raise ValueError('unknown comparator stage')
            statuses[stage] = event
    except (OSError, ValueError, TypeError, KeyError):
        return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    record['comparator_status'] = events
    lean_accepts = statuses.get('lean', {}).get('result') == 'accept'
    nanoda_accepts = statuses.get('nanoda', {}).get('result') == 'accept'
    record.update(lean_accepts=lean_accepts, nanoda_accepts=nanoda_accepts)
    if 'error' in statuses:
        reason = statuses['error']['reason']
        if reason == 'library-outside-certified-base' and result.rc != 70:
            return dict(record, verdict='ERROR', reason='sandbox-unavailable')
        if reason == 'kernel-unavailable':
            reason = 'environment-tool-unavailable:replay' if result.rc == 70 else 'sandbox-unavailable'
        return dict(record, verdict='ERROR', reason=reason)
    if 'policy' in statuses:
        valid_policy_exit = (result.rc == 2 and statuses['policy']['reason'] in
                             {'resource-cap', 'disallowed-axiom', 'comparator-statement-mismatch', 'certified-base-mismatch'}
                             or result.rc == 3 and statuses['policy']['reason'] == 'resource-cap'
                             or result.rc == 4 and statuses['policy']['reason'] == 'tool-fault:replay'
                             and 'nanoda-fuel' in statuses)
        return dict(record, verdict='INVALID' if valid_policy_exit else 'ERROR',
                    reason=statuses['policy']['reason'] if valid_policy_exit else 'sandbox-unavailable')
    if result.rc in (3, 4, 70):
        return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    if fuel_budget is not None and not {'fuel', 'nanoda-fuel'} <= statuses.keys():
        return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    if result.rc == 0:
        if (lean_accepts and nanoda_accepts and statuses.get('quotient', {}).get('result') == 'accept'
                and (not (tools.lean_replay or fuel_budget is not None) or {'fuel', 'nanoda-fuel'} <= statuses.keys())):
            return dict(record, verdict='PASS', reason=None)
        return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    if statuses.get('quotient', {}).get('result') == 'reject' and lean_accepts:
        return dict(record, verdict='INVALID', reason='kernel-rejection')
    if 'lean' not in statuses or 'nanoda' not in statuses or ((tools.lean_replay or fuel_budget is not None) and not {'fuel', 'nanoda-fuel'} <= statuses.keys()):
        return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    if lean_accepts and nanoda_accepts:
        return dict(record, verdict='ERROR', reason='sandbox-unavailable')
    if nanoda_accepts:
        if 'fuel' not in statuses:
            return dict(record, verdict='ERROR', reason='replay-unavailable')
        return dict(record, verdict='INVALID', reason='kernel-disagreement')
    if lean_accepts:
        return dict(record, verdict='INVALID', reason='kernel-disagreement')
    return dict(record, verdict='INVALID', reason='kernel-rejection')
