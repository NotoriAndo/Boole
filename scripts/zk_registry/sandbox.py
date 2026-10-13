"""One fail-closed Landrun boundary for all commands that execute submission code."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd
import subprocess
import uuid

_PENDING = {}


def capture_result(command, result):
    """Attach only receipts registered by this process's pinned Sandbox.command call."""
    if '--gate-status' not in command:
        return result
    status = command[command.index('--gate-status') + 1]
    pending = _PENDING.pop(status, None)
    if pending is None:
        return result
    authority, capabilities, mode = pending
    if result.timeout or result.memkill or result.killed_by:
        result.boundary = dict(verdict='ERROR', reason='resource-unavailable', assurance=mode,
                               capabilities_snapshot=capabilities)
        return result
    try:
        record = json.loads(Path(status).read_text())
        if record['protocol'] != 1 or record['assurance'] != mode:
            raise ValueError('invalid boundary receipt')
        if record['verdict'] == 'PASS' and record['descriptor_sha256'] != authority:
            raise ValueError('boundary authority mismatch')
        result.boundary = record
    except (OSError, ValueError, KeyError):
        result.boundary = dict(verdict='ERROR', reason='sandbox-unavailable', assurance=mode,
                               capabilities_snapshot=capabilities)
    return result


def boundary_failure(result):
    """Give wall/memory ERROR precedence over deterministic containment failures."""
    if result.timeout or result.memkill or result.killed_by:
        return {'verdict': 'ERROR', 'reason': 'resource-unavailable'}
    if result.boundary and result.boundary.get('verdict') != 'PASS':
        return result.boundary
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


@dataclass(frozen=True)
class Sandbox:
    mode: str
    executable: str | None = None
    authority: str | None = None
    cgroup: str | None = None
    capabilities: dict | None = None
    inner_executable: str | None = None
    memory_max: int = 11000 * 1024 * 1024
    pids_max: int = 128
    role: str = 'submission'
    disabled_layer: str | None = None

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
        try:
            data_path = Path(__file__).parent / 'data' / 'acceptance_tools_v2.json'
            authority = hashlib.sha256(data_path.read_bytes()).hexdigest()
            if json.loads(data_path.read_text()) != descriptor:
                raise ValueError('production authority override')
            candidate = cls('production', str(binary.resolve()), authority, delegated_cgroup(),
                            inner_executable=str(binary.resolve().parent / 'comparator-landrun'))
            probe = candidate.command(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, ['/usr'], ['/dev/null'])
            from .lean_runner import RunResult
            process = subprocess.run(probe, env={'PATH': '/usr/bin:/bin'}, capture_output=True,
                                     text=True, timeout=15)
            Path(probe[probe.index('--gate-status') + 1]).with_suffix('.preflight.log').write_text(
                process.stdout + process.stderr)
            result = capture_result(probe, RunResult(process.returncode, process.stdout, 0, False))
            if not result.boundary or result.boundary['verdict'] != 'PASS' or result.rc:
                raise ValueError('capability preflight failed')
            return cls('production', candidate.executable, authority, candidate.cgroup, result.boundary,
                       candidate.inner_executable)
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

    def command(self, argv: list[str], env: dict, readable: list[str], writable: list[str]) -> list[str]:
        """Preserve a strict filesystem/network boundary, with no best-effort fallback."""
        if self.mode != 'production' and self.disabled_layer is None:
            return list(argv)
        owner_home = Path(pwd.getpwuid(os.getuid()).pw_dir)
        receipts = Path('/run/user') / str(os.getuid()) / 'boole-gate-receipts'
        receipts.mkdir(mode=0o700, parents=True, exist_ok=True)
        protected = (owner_home / '.elan', owner_home / '.cache/mathlib', receipts,
                     Path(self.executable).parent, Path(self.cgroup))
        outputs = sorted({str(Path(path).resolve()) for path in writable})
        for reader in readable:
            path = Path(reader).resolve()
            if path in {Path('/'), Path('/proc'), Path('/sys'), Path('/dev')}:
                raise SandboxUnavailable('sandbox-unavailable')
        for output in outputs:
            path = Path(output)
            if path == Path('/') or any(path.is_relative_to(root) or root.is_relative_to(path) for root in protected):
                raise SandboxUnavailable('sandbox-unavailable')
        status = str(receipts / (uuid.uuid4().hex + '.json'))
        _PENDING[status] = self.authority, self.capabilities, self.mode
        command = [self.executable, '--gate-authority', self.authority, '--gate-cgroup', self.cgroup,
                   '--gate-status', status, '--gate-memory', str(self.memory_max), '--gate-pids', str(self.pids_max),
                   '--gate-role', self.role, '--gate-mode', self.mode]
        if self.disabled_layer:
            command += ['--gate-fixture-no-' + self.disabled_layer]
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
