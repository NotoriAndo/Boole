#!/usr/bin/env python3
"""Fast production-boundary and forbidden-policy controls; no Lean or Mathlib installation."""
from __future__ import annotations

import ctypes
import errno
import os
from pathlib import Path
import socket
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
source_root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(source_root / 'scripts'))
from zk_registry import check as C
from zk_registry import correctness_runtime as CR
from zk_registry.sandbox import Sandbox, SandboxUnavailable


class LinuxProductionControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        """Require the tracked binary and actual kernel support, never best-effort or passthrough."""
        cls.boundary = Sandbox.production(os.environ.get('ZK_GATE_LANDRUN'))
        abi = ctypes.CDLL(None, use_errno=True).syscall(444, 0, 0, 1)
        if abi < 4:
            raise RuntimeError('ERROR sandbox-unavailable: Landlock ABI >=4 is mandatory')

    def setUp(self):
        """Allocate only inside the caller's explicit work root."""
        root = Path(os.environ['ZK_GATE_WORK_ROOT']).resolve()
        root.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=root)
        self.work = Path(self.directory.name)

    def tearDown(self):
        """Remove only this test's temporary directory."""
        self.directory.cleanup()

    def run_code(self, source):
        """Execute the control through the exact production command constructor."""
        env = {'PATH': '/usr/bin:/bin', 'HOME': str(self.work), 'TMPDIR': str(self.work),
               'LANG': 'C.UTF-8', 'PYTHONDONTWRITEBYTECODE': '1'}
        argv = self.boundary.command([sys.executable, '-c', source], env, [], [str(self.work)])
        result = CR.runner(str(self.work / 'logs'), 10)(argv, env, str(self.work), 10, 256)
        self.assertEqual(result.boundary['verdict'], 'PASS', result.boundary)
        return result

    def test_valid_command_executes_in_production(self):
        result = self.run_code('print("production-control")')
        self.assertEqual(result.rc, 0, result.out)
        self.assertEqual(result.out.strip(), 'production-control')
        self.assertEqual(self.boundary.mode, 'production')

    def test_inside_write_allowed_and_outside_write_denied(self):
        inside = self.work / 'inside'
        outside = self.work.parent / (self.work.name + '-outside')
        source = ('from pathlib import Path\nPath(' + repr(str(inside)) + ').write_text("allowed")\n'
                  + 'try:\n Path(' + repr(str(outside)) + ').write_text("forbidden")\n'
                  + 'except OSError as e:\n print(e.errno)\n')
        result = self.run_code(source)
        self.assertEqual(result.rc, 0, result.out)
        self.assertIn(int(result.out.strip()), (errno.EACCES, errno.ENOENT))
        self.assertEqual(inside.read_text(), 'allowed')
        self.assertFalse(outside.exists())

    def test_tcp_connect_to_own_loopback_listener_is_denied(self):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen(1)
            port = listener.getsockname()[1]
            source = ('import socket\ntry:\n s=socket.socket()\n s.connect(("127.0.0.1",'
                      + str(port) + '))\n print("unexpected connection")\n'
                      + 'except OSError as e:\n print(e.errno)\n')
            result = self.run_code(source)
        self.assertEqual(result.rc, 0, result.out)
        self.assertEqual(result.out.strip(), str(errno.EPERM))

    def test_missing_binary_and_protected_write_roots_fail_closed(self):
        with self.assertRaisesRegex(SandboxUnavailable, '^sandbox-unavailable$'):
            Sandbox.production('/not-installed')
        with self.assertRaisesRegex(SandboxUnavailable, '^sandbox-unavailable$'):
            self.boundary.command([sys.executable, '-c', 'pass'], {}, [], ['/'])

    def test_allowlist_and_code_defining_commands_are_enforced(self):
        allowed = '@[simp] theorem helper : True := trivial\nattribute [local ext] helper'
        self.assertEqual(C.forbidden_scan([('helper declarations', allowed)]), [])
        for command in ('elab', 'elab_rules', 'simproc', 'dsimproc', 'macro', 'macro_rules',
                        'syntax', 'run_tac', 'register_custom_handler', 'declare_custom_elaborator'):
            with self.subTest(command=command):
                self.assertTrue(C.forbidden_scan([('helper declarations', command + ' anything')]))
        for source in ('@[custom_handler]', '@[simp, custom_handler]', 'native_decide', '#eval 1'):
            with self.subTest(source=source):
                self.assertTrue(C.forbidden_scan([('proof', source)]))


if __name__ == '__main__':
    unittest.main()
