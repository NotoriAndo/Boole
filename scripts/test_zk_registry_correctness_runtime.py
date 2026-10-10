"""Owned-PID resource guard controls, using mocks only and never signalling real processes."""

import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import correctness_runtime as R, lean_runner as L


class RuntimeTests(unittest.TestCase):
    def test_one_checker_deadline_is_not_reset_for_each_module(self):
        """Once the aggregate deadline expires, another compiler process is not started."""
        original = L.run_process
        self.addCleanup(setattr, L, 'run_process', original)
        with tempfile.TemporaryDirectory() as root, patch.object(R.subprocess, 'Popen') as popen:
            R.install(root, total_timeout=0)
            first = L.run_process(['mock-first'], {}, root, 1800)
            second = L.run_process(['mock-second'], {}, root, 1800)
        self.assertEqual((first.rc, second.rc), (124, 124))
        self.assertEqual((first.killed_by, second.killed_by), ('checker-deadline', 'checker-deadline'))
        self.assertTrue(first.timeout and second.timeout)
        popen.assert_not_called()

    def test_only_recorded_pid_identities_can_receive_signals(self):
        """A foreign or reused PID is excluded even when it shares a recorded process number."""
        child = Mock(pid=900001, returncode=0)
        child.poll.return_value = 0
        owned = {900001: 'old-root', 900002: 'owned-descendant', 900003: 'reused'}
        table = {900001: (1, 0, 'old-root'), 900002: (900001, 0, 'owned-descendant'),
                 900003: (1, 0, 'new-foreign-identity'), 900004: (1, 0, 'foreign')}
        with patch.object(R.os, 'kill') as kill:
            self.assertEqual(R.terminate_owned(child, owned, observe=lambda: table, grace_seconds=0), [])
        self.assertEqual(kill.call_args_list,
                         [unittest.mock.call(pid, sig) for sig in (signal.SIGTERM, signal.SIGKILL)
                          for pid in (900002, 900001)])
        child.terminate.assert_not_called()
        child.kill.assert_not_called()

    def test_failed_observation_uses_only_the_unreaped_child_handle(self):
        """Without an identity table, no arbitrary recorded PID is signalled."""
        child = Mock(pid=900001)
        child.poll.return_value = None
        with patch.object(R.os, 'kill') as kill:
            errors = R.terminate_owned(child, {900002: 'unknown'},
                                       observe=Mock(side_effect=OSError('mock-only')), grace_seconds=0)
        self.assertEqual(errors, ['OSError', 'OSError'])
        kill.assert_not_called()
        child.terminate.assert_called_once()
        child.kill.assert_called_once()

    def test_unobserved_root_is_not_a_successful_result(self):
        """A fast exit whose own identity was never observed fails closed."""
        child = Mock(pid=900001, returncode=0)
        child.poll.return_value = 0
        child.communicate.return_value = (b'finished', None)
        original = L.run_process
        self.addCleanup(setattr, L, 'run_process', original)
        with tempfile.TemporaryDirectory() as root, patch.object(R.subprocess, 'Popen', return_value=child), \
                patch.object(R, 'processes', return_value={900002: (1, 0, 'foreign')}), \
                patch.object(R.os, 'kill') as kill:
            R.install(root)
            result = L.run_process(['mock'], {}, root, 1)
        self.assertEqual((result.rc, result.killed_by), (125, 'unobserved-owned-root'))
        kill.assert_not_called()

    def test_timeout_output_drain_is_bounded(self):
        """A descendant retaining the pipe cannot make timeout cleanup wait indefinitely."""
        child = Mock(pid=900001, returncode=0)
        child.poll.return_value = 0
        child.communicate.side_effect = subprocess.TimeoutExpired(['mock'], 1, output=b'partial')
        child.stdout = Mock()
        original = L.run_process
        self.addCleanup(setattr, L, 'run_process', original)
        table = {900001: (1, 0, 'owned-root')}
        with tempfile.TemporaryDirectory() as root, patch.object(R.subprocess, 'Popen', return_value=child), \
                patch.object(R, 'processes', return_value=table), \
                patch.object(R, 'terminate_owned', return_value=[]) as terminate:
            R.install(root)
            result = L.run_process(['mock'], {}, root, 1)
        self.assertEqual(result.killed_by, 'timeout')
        self.assertTrue(result.timeout)
        self.assertEqual(result.out, 'partial')
        self.assertEqual([item.kwargs['timeout'] for item in child.communicate.call_args_list], [1, 1])
        child.stdout.close.assert_called_once()
        terminate.assert_called_once()


if __name__ == '__main__':
    unittest.main()
