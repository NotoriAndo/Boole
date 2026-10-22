"""One fail-closed Landrun boundary for all commands that execute submission code."""
from __future__ import annotations

from dataclasses import dataclass, field
from contextlib import contextmanager
from contextvars import ContextVar
import hashlib
import json
import os
from pathlib import Path
from .safe_fs import Path as SafePath, reject_symlinks, submission_io, root_identity, absolute, directory_fd, submission_error
import platform
import pwd
import subprocess
import shutil
import uuid

_PENDING = {}
_SUBMISSION_CODE = ContextVar('submission_controlled_code', default=False)


@contextmanager
def submission_code():
    token = _SUBMISSION_CODE.set(True)
    try:
        yield
    finally:
        _SUBMISSION_CODE.reset(token)


def registered_command(command):
    """Identify a boundary command allocated by this host process, not payload text."""
    if '--gate-status' not in command:
        return False
    index = command.index('--gate-status') + 1
    return index < len(command) and command[index] in _PENDING


def clean_receipt(status):
    """Remove only the exact per-step receipt paths allocated by this process."""
    for file in (SafePath(status), SafePath(status).with_suffix('.events.ndjson'),
                 SafePath(status).with_suffix('.preflight.log')):
        try:
            file.unlink(missing_ok=True)
        except OSError:
            pass


def capture_result(command, result):
    """Attach only receipts registered by this process's pinned Sandbox.command call."""
    if '--gate-status' not in command:
        return result
    status = command[command.index('--gate-status') + 1]
    pending = _PENDING.pop(status, None)
    if pending is None:
        return result
    authority, capabilities, mode, exposed, writers = pending
    if result.killed_by.startswith('output-cap:'):
        channel=result.killed_by.split(':',1)[1]
        if result.streams.get(channel,{}).get('kind')=='raw' and mode=='production':
            try:
                observed=normalized_boundary(json.loads(SafePath(status).read_text()))
                healthy=(observed.get('protocol')==1 and observed.get('assurance')==mode
                         and observed.get('descriptor_sha256')==authority and observed.get('verdict')=='PASS')
            except (OSError,ValueError,KeyError,TypeError):healthy=False
            if not healthy:
                result.killed_by='raw-output-guard:'+channel
                result.output_capped=False
    if result.timeout or result.memkill or result.killed_by:
        from .closure import resource_reason
        result.boundary = dict(verdict='ERROR', reason=resource_reason(result) or 'resource-unavailable', assurance=mode,
                               capabilities_snapshot=capabilities)
        clean_receipt(status)
        return result
    try:
        record = json.loads(SafePath(status).read_text())
        if record['protocol'] != 1 or record['assurance'] != mode:
            raise ValueError('invalid boundary receipt')
        if record['verdict'] == 'PASS' and record['descriptor_sha256'] != authority:
            raise ValueError('boundary authority mismatch')
        result.boundary = normalized_boundary(record)
        if record.get('observation') and not record['observation'].get('setup_failed', True):
            exposed.update(writers)
    except (OSError, ValueError, KeyError):
        result.boundary = dict(verdict='ERROR', reason='sandbox-unavailable', assurance=mode,
                               capabilities_snapshot=capabilities)
    finally:
        # CR.runner copies this observation into the returned/aggregate step report.
        clean_receipt(status)
    return result


def normalized_boundary(record):
    """Host resource observations cannot become mathematical rejections."""
    observation = (record or {}).get('observation') or {}
    if observation.get('oom_kill_events') or observation.get('pids_max_events'):
        return dict(record, verdict='ERROR', reason='resource-unavailable')
    return record


def boundary_failure(result):
    """Give wall/memory ERROR precedence over deterministic containment failures."""
    if result.timeout or result.memkill or result.killed_by:
        from .closure import resource_reason
        return {'verdict': 'ERROR', 'reason': resource_reason(result) or 'resource-unavailable'}
    boundary = normalized_boundary(result.boundary)
    if boundary and boundary.get('verdict') != 'PASS':
        return boundary
    return None


def delegated_cgroup():
    """Use a systemd-delegated domain, moving only the calling PID into its supervisor leaf."""
    if os.getuid() == 0:
        raise SandboxUnavailable('sandbox-unavailable')
    current = Path('/sys/fs/cgroup' + Path('/proc/self/cgroup').read_text().strip().split(':')[-1])
    root = current.parent if current.name == 'supervisor' else current
    if root.stat().st_uid != os.getuid() or not {'memory', 'pids'} <= set(
            (root / 'cgroup.controllers').read_text().split()):
        raise SandboxUnavailable('sandbox-unavailable')
    if current == root:
        if set((root / 'cgroup.procs').read_text().split()) != {str(os.getpid())}:
            raise SandboxUnavailable('sandbox-unavailable')
        supervisor = root / 'supervisor'
        supervisor.mkdir(mode=0o700, exist_ok=True)
        (supervisor / 'cgroup.procs').write_text(str(os.getpid()))
        (root / 'cgroup.subtree_control').write_text('+memory +pids')
    if not {'memory', 'pids'} <= set((root / 'cgroup.subtree_control').read_text().split()):
        raise SandboxUnavailable('sandbox-unavailable')
    return str(root)


class SandboxUnavailable(ValueError):
    """The required pinned production boundary is unavailable."""


class SandboxConfigurationError(SandboxUnavailable):
    """A named invalid node/profile configuration, rejected before launching anything."""


def validate_memory_profiles(descriptor, requested=None):
    """Validate every production profile against the same pinned whole-node envelope."""
    try:
        policy = descriptor['sandbox']['memory_envelope']
        node, reserve, minimum = (policy[k] for k in
                                  ('node_max_bytes', 'supervisor_reserve_bytes', 'minimum_payload_bytes'))
        if (any(type(x) is not int for x in (node, reserve, minimum))
                or node != 20_000_000_000 or reserve != 32 * 1024 * 1024 or minimum != 16 * 1024 * 1024):
            raise ValueError('invalid fixed envelope')
        values = [profile['payload_memory_max_bytes'] for profile in descriptor['binary_profiles'].values()
                  if profile.get('assurance') == 'production']
        if requested is not None:
            values.append(requested)
        for value in values:
            if type(value) is not int or value < minimum:
                raise SandboxConfigurationError('sandbox-configuration:invalid-payload-memory')
            if value > node - reserve:
                raise SandboxConfigurationError('sandbox-configuration:payload-memory-exceeds-node-envelope')
    except SandboxConfigurationError:
        raise
    except (KeyError, TypeError, ValueError):
        raise SandboxConfigurationError('sandbox-configuration:invalid-memory-envelope') from None


def stage_kind(argv):
    """Classify the trusted caller's executable stage, not submission stdout or environment."""
    name = os.path.basename(argv[0])
    if name == 'lean':
        return 'replay' if '--run' in argv else 'compile'
    if name == 'lean4export':
        return 'export'
    if name == 'comparator':
        return 'comparator'
    if (name in {'go', 'cargo', 'nargo', 'zokrates'} or name.startswith('circom')) and any(
            arg in argv for arg in ('build', 'test', 'compile', 'export', '--r1cs')):
        return 'native-build'
    return 'native-run'


@dataclass(frozen=True)
class Sandbox:
    mode: str
    executable: str | None = None
    authority: str | None = None
    cgroup: str | None = None
    capabilities: dict | None = None
    inner_executable: str | None = None
    memory_max: int = 11000 * 1024 * 1024
    pids_max: int | None = None
    role: str = 'submission'
    disabled_layer: str | None = None
    exposed_writers: dict[str, tuple[int, int]] = field(default_factory=dict, compare=False, repr=False)

    def __post_init__(self):
        # replace() must never share mutable attribution state; trusted roles inherit none.
        object.__setattr__(self, 'exposed_writers', dict(self.exposed_writers) if self.role == 'submission' else {})

    @classmethod
    def production(cls, landrun_path: str | None = None, system: str | None = None,
                   descriptor: dict | None = None) -> Sandbox:
        """Require Linux and the pinned binary; never discover it from an environment variable."""
        if (system or platform.system()) != 'Linux' or not landrun_path:
            raise SandboxUnavailable('sandbox-unavailable')
        if descriptor is None:
            path = Path(__file__).parent / 'data' / 'acceptance_tools_v2.json'
            try:
                descriptor = json.loads(path.read_text())
            except (OSError, ValueError):
                raise SandboxUnavailable('sandbox-unavailable') from None
        hardened = descriptor.get('sandbox', {})
        if hardened.get('version') != 1:
            raise SandboxUnavailable('sandbox-unavailable')
        expected = hardened.get('launcher_sha256')
        binary = Path(landrun_path)
        if not expected or not binary.is_file() or not os.access(binary, os.X_OK):
            raise SandboxUnavailable('sandbox-unavailable')
        if hashlib.sha256(binary.read_bytes()).hexdigest() != expected:
            raise SandboxUnavailable('sandbox-unavailable')
        validate_memory_profiles(descriptor)
        profile = descriptor['binary_profiles'].get('Linux-' + platform.machine())
        if profile is None or profile.get('assurance') != 'production':
            raise SandboxUnavailable('sandbox-unavailable')
        try:
            data_path = Path(__file__).parent / 'data' / 'acceptance_tools_v2.json'
            authority = hashlib.sha256(data_path.read_bytes()).hexdigest()
            if json.loads(data_path.read_text()) != descriptor:
                raise ValueError('production authority override')
            candidate = cls('production', str(binary.resolve()), authority, delegated_cgroup(),
                            inner_executable=str(binary.resolve().parent / 'comparator-landrun'),
                            memory_max=profile['payload_memory_max_bytes'])
            probe = candidate.command(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, ['/usr'], ['/dev/null'])
            from .lean_runner import RunResult
            process = subprocess.run(probe, env={'PATH': '/usr/bin:/bin'}, capture_output=True,
                                     text=True, timeout=15)
            SafePath(probe[probe.index('--gate-status') + 1]).with_suffix('.preflight.log').write_text(
                process.stdout + process.stderr)
            result = capture_result(probe, RunResult(process.returncode, process.stdout, 0, False))
            if not result.boundary or result.boundary['verdict'] != 'PASS' or result.rc:
                raise ValueError('capability preflight failed')
            return cls('production', candidate.executable, authority, candidate.cgroup, result.boundary,
                       candidate.inner_executable, memory_max=candidate.memory_max)
        except SandboxConfigurationError:
            raise
        except (OSError, ValueError, KeyError, subprocess.SubprocessError):
            raise SandboxUnavailable('sandbox-unavailable') from None

    @classmethod
    def development(cls) -> Sandbox:
        """Use the explicit macOS-only test passthrough, never production evidence."""
        if platform.system() != 'Darwin':
            raise SandboxUnavailable('sandbox-unavailable')
        return cls('dev-unsandboxed')

    @classmethod
    def fixture(cls) -> Sandbox:
        """Allow argument-injected fast fixtures on any test platform."""
        return cls('test-fixture')

    def command(self, argv: list[str], env: dict, readable: list[str], writable: list[str], *,
                submission_controlled: bool | None = None, prior_exposure: dict | None = None) -> list[str]:
        """Preserve a strict filesystem/network boundary, with no best-effort fallback."""
        if self.mode != 'production' and self.disabled_layer is None:
            return list(argv)
        # Read attribution can survive a trusted tool transition; only controlled payloads
        # can add exposure, through the original writer map stored in _PENDING.
        read_exposure = dict(self.exposed_writers if prior_exposure is None else prior_exposure)
        data = json.loads((Path(__file__).parent / 'data/acceptance_tools_v2.json').read_text())
        validate_memory_profiles(data, self.memory_max)
        owner_home = Path(pwd.getpwuid(os.getuid()).pw_dir)
        receipts = Path('/run/user') / str(os.getuid()) / 'boole-gate-receipts'
        receipts.mkdir(mode=0o700, parents=True, exist_ok=True)
        protected = (owner_home / '.elan', owner_home / '.cache/mathlib', receipts,
                     Path(self.executable).parent, Path(self.cgroup))
        outputs = sorted({os.path.abspath(path) for path in writable})
        executable = shutil.which(argv[0], path=env.get('PATH', '/usr/bin:/bin'))
        requested = executable or (os.path.abspath(argv[0]) if '/' in argv[0] else None)
        artifact = bool(requested and any(Path(requested).is_relative_to(Path(root)) for root in outputs
                                         if root != '/dev/null'))
        controlled = self.role == 'submission' and (artifact or (
            _SUBMISSION_CODE.get() if submission_controlled is None else submission_controlled))
        identities = {}
        if controlled:
            try:
                identities = {absolute(path): root_identity(path) for path in outputs if path != '/dev/null'}
            except OSError:
                raise SandboxUnavailable('sandbox-unavailable') from None
        defaults = [str(Path(self.executable).parent), '/usr', '/lib', '/lib64',
                    '/etc/alternatives', '/dev/urandom']
        allowed = []
        for raw in [*readable, *defaults]:
            try:
                if Path(raw).exists() and str(Path(raw).resolve()) == os.path.abspath(raw):
                    allowed.append(Path(raw))
            except OSError as error:
                submission_error(raw, error, read_exposure)
                raise SandboxUnavailable('sandbox-unavailable') from None
        for raw in outputs:
            try:
                with submission_io(read_exposure):
                    reject_symlinks(raw)
            except OSError:
                raise SandboxUnavailable('sandbox-unavailable') from None
        for reader in readable:
            if artifact and os.path.abspath(reader) == requested:
                continue  # the pinned launcher classifies this submission path before any execution
            try:
                with submission_io(read_exposure):
                    reject_symlinks(reader)
                    if Path(reader).is_dir():
                        held = directory_fd(reader)
                        try:
                            try:
                                probe = os.open('.', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC, dir_fd=held)
                            except OSError as error:
                                submission_error(reader, error)
                                raise
                            os.close(probe)
                        finally:
                            os.close(held)
                path = Path(reader).resolve()
            except OSError:
                raise SandboxUnavailable('sandbox-unavailable') from None
            if not any(path == root or path.is_relative_to(root) for root in allowed):
                raise SandboxUnavailable('sandbox-unavailable')
            if path in {Path('/'), Path('/proc'), Path('/sys'), Path('/dev')}:
                raise SandboxUnavailable('sandbox-unavailable')
        for output in outputs:
            path = Path(output)
            if path == Path('/') or any(path.is_relative_to(root) or root.is_relative_to(path) for root in protected):
                raise SandboxUnavailable('sandbox-unavailable')
        if executable and not artifact and not any(Path(executable).is_relative_to(Path(root)) for root in ('/usr', '/bin', '/sbin')):
            try:
                reject_symlinks(executable)
            except OSError:
                raise SandboxUnavailable('sandbox-unavailable') from None
        status = str(receipts / (uuid.uuid4().hex + '.json'))
        _PENDING[status] = self.authority, self.capabilities, self.mode, self.exposed_writers, identities
        stage = stage_kind(argv)
        pids = self.pids_max if self.pids_max is not None else data['sandbox']['parallelism']['pids_max'][stage]
        command = [self.executable, '--gate-authority', self.authority, '--gate-cgroup', self.cgroup,
                   '--gate-status', status, '--gate-memory', str(self.memory_max), '--gate-pids', str(pids),
                   '--gate-stage', stage,
                   '--gate-exec-origin', 'submission' if artifact else 'trusted',
                   '--gate-role', self.role, '--gate-mode', self.mode]
        if self.disabled_layer:
            command += ['--gate-fixture-no-' + self.disabled_layer]
        for root, identity in sorted(read_exposure.items()):
            command += ['--gate-exposed-writer', json.dumps([root, *identity], separators=(',', ':'))]
        command += ['--add-exec', '--ldd']
        for path in sorted({*readable, str(Path(self.executable).parent), '/usr', '/lib', '/lib64',
                            '/etc/alternatives', '/dev/urandom'}):
            if Path(path).exists():
                command.extend(['--ro', str(Path(path).resolve())])
        if Path('/dev/null').exists():
            command.extend(['--rw', '/dev/null'])
        for path in outputs:
            command.extend(['--rwx', path])
        for key, value in sorted(env.items()):
            command.extend(['--env', key + '=' + value])
        return command + ['--'] + list(argv)
