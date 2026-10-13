"""One fail-closed Landrun boundary for all commands that execute submission code."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd


class SandboxUnavailable(ValueError):
    """The required pinned production boundary is unavailable."""


@dataclass(frozen=True)
class Sandbox:
    mode: str
    executable: str | None = None

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
        expected = descriptor.get('landrun', {}).get('binary_sha256', {}).get(platform.machine())
        binary = Path(landrun_path)
        if not expected or not binary.is_file() or not os.access(binary, os.X_OK):
            raise SandboxUnavailable('sandbox-unavailable')
        if hashlib.sha256(binary.read_bytes()).hexdigest() != expected:
            raise SandboxUnavailable('sandbox-unavailable')
        return cls('production', str(binary.resolve()))

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
        if self.mode != 'production':
            return list(argv)
        owner_home = Path(pwd.getpwuid(os.getuid()).pw_dir)
        protected = (owner_home / '.elan', owner_home / '.cache/mathlib')
        outputs = sorted({str(Path(path).resolve()) for path in writable})
        for output in outputs:
            path = Path(output)
            if path == Path('/') or any(path.is_relative_to(root) or root.is_relative_to(path) for root in protected):
                raise SandboxUnavailable('sandbox-unavailable')
        command = [self.executable, '--add-exec', '--ldd']
        for path in sorted({*readable, '/usr', '/lib', '/lib64', '/dev/urandom'}):
            if Path(path).exists():
                command.extend(['--ro', str(Path(path).resolve())])
        if Path('/dev/null').exists():
            command.extend(['--rw', '/dev/null'])
        for path in outputs:
            command.extend(['--rwx', path])
        for key, value in sorted(env.items()):
            command.extend(['--env', key + '=' + value])
        return command + ['--'] + list(argv)
