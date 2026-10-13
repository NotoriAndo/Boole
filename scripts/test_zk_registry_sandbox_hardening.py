"""Production regression tests for the complete acceptance-gate containment boundary."""
import os
import platform
import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry.sandbox import Sandbox, SandboxUnavailable
from zk_registry import correctness_runtime as CR
from zk_registry import check as C
from zk_registry import lean_runner as L
import tempfile
from dataclasses import replace
import json
import socket


class ProductionPreflight(unittest.TestCase):
    """An old TCP-only launcher must not qualify as the hardened boundary."""

    def test_legacy_landrun_is_not_a_production_boundary(self):
        """Reject actual pinned legacy bytes even when the running kernel supports them."""
        legacy = os.environ.get('BOOLE_TEST_LEGACY_LANDRUN')
        if platform.system() != 'Linux' or not legacy:
            self.skipTest('live Linux legacy runtime is supplied by the production control runner')
        with self.assertRaisesRegex(SandboxUnavailable, 'sandbox-unavailable'):
            Sandbox.production(legacy)

    def test_pristine_tracked_profile_loads_without_an_overlay(self):
        """Load the actual provisioned proof tools through the unchanged tracked descriptor."""
        runtime = os.environ.get('BOOLE_TEST_SANDBOX')
        if platform.system() != 'Linux' or not runtime:
            self.skipTest('live Linux pinned runtime is supplied by the production control runner')
        from zk_registry import closure as D
        descriptor = Path(__file__).parent / 'zk_registry/data/acceptance_tools_v2.json'
        bundle = Path(runtime).parent / 'bundle.json'
        self.assertEqual(descriptor.read_bytes(), bundle.read_bytes())
        tools = D.Tools.load(str(Path(runtime).parent.parent))
        self.assertEqual(tools.landrun, runtime)

    def test_udp_socket_is_denied_in_a_live_production_step(self):
        """Require actual execution and EPERM, not a skip or an unavailable boundary."""
        runtime = os.environ.get('BOOLE_TEST_SANDBOX')
        if platform.system() != 'Linux' or not runtime:
            self.skipTest('live Linux pinned runtime is supplied by the production control runner')
        boundary = Sandbox.production(runtime)
        with tempfile.TemporaryDirectory() as temp:
            program = Path(temp) / 'udp.py'
            program.write_text('import socket,errno\ntry:\n socket.socket(socket.AF_INET,socket.SOCK_DGRAM)\n'
                               'except OSError as error:\n assert error.errno==errno.EPERM\n'
                               ' print("network-permission-denied")\nelse:\n raise AssertionError("UDP escaped")\n')
            run = CR.runner(str(Path(temp) / 'logs'), 20, sandbox=boundary,
                            readable_paths=['/usr', temp], writable_paths=[temp])
            result = run(['/usr/bin/python3', str(program)], {'PATH': '/usr/bin:/bin'}, temp, 20, 256)
            self.assertEqual(result.rc, 0, result.out)
            self.assertEqual(result.boundary['verdict'], 'PASS')
            self.assertIn('network-permission-denied', result.out)

    def test_reaped_double_fork_is_not_silently_accepted(self):
        """A fast orphan still invalidates the step even when no live process remains at exit."""
        runtime = os.environ.get('BOOLE_TEST_SANDBOX')
        if platform.system() != 'Linux' or not runtime:
            self.skipTest('live Linux pinned runtime is supplied by the production control runner')
        boundary = Sandbox.production(runtime)
        with tempfile.TemporaryDirectory() as temp:
            program = Path(temp) / 'orphan.py'
            program.write_text('import os,time\npid=os.fork()\nif pid==0:\n os.setsid()\n'
                               ' if os.fork()==0:\n  os._exit(0)\n os._exit(0)\n'
                               'os.waitpid(pid,0)\ntime.sleep(.05)\n')
            run = CR.runner(str(Path(temp) / 'logs'), 20, sandbox=boundary,
                            readable_paths=['/usr', temp], writable_paths=[temp])
            result = run(['/usr/bin/python3', str(program)], {'PATH': '/usr/bin:/bin'}, temp, 20, 256)
            self.assertEqual(result.boundary['verdict'], 'INVALID', result.out)
            self.assertEqual(result.boundary['reason'], 'owned-descendants-remained')
            self.assertGreater(result.boundary['observation']['orphans_reaped'], 0)


class OutcomePrecedence(unittest.TestCase):
    """Availability failure always precedes deterministic descendant rejection."""

    def test_wall_error_precedes_a_descendant_observation(self):
        """Consumers must not turn a timed-out window into a closed INVALID result."""
        invalid, failed, errors = [], [], []
        C.apply_compile_result({'compile_timeout': True,
                                'boundary': {'verdict': 'INVALID', 'reason': 'owned-descendants-remained'}},
                               {}, invalid, failed, errors)
        self.assertEqual((invalid, failed, errors), ([], [], ['resource-unavailable']))

    def test_runtime_environment_uses_the_visible_canonical_sysroot(self):
        """The standard library must not refer to an alias missing from the private mount graph."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'real').mkdir()
            (root / 'alias').symlink_to(root / 'real', target_is_directory=True)
            env = L.LeanEnv(str(root / 'alias'), [], 'v4.34.1', {}, '', str(root / 'scratch'))
            actual = L.process_env(env)
            self.assertEqual(actual['LEAN_SYSROOT'], str((root / 'real').resolve()))

    def test_service_entrypoint_selects_the_hardened_pin(self):
        """The real service route must not select the retired ABI9-only launcher."""
        from unittest.mock import patch
        from zk_registry import acceptance as A, service_check as SC
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'problem.json').write_text(json.dumps(dict(schema_version='route-fixture',
                                                              env=dict(lean='v4.34.1'))))
            (root / 'Solution.lean').write_text('-- unexecuted route fixture\n')
            env = L.LeanEnv('/unused', [], 'v4.34.1', {}, '', str(root / 'scratch'))
            with patch.object(SC.P, 'validate_problem', return_value=[]), \
                 patch.object(SC.L, 'load_production_env', return_value=env), \
                 patch.object(A, 'read', return_value={}), \
                 patch('platform.machine', return_value='aarch64'), \
                 patch.object(SC.Sandbox, 'production',
                              side_effect=SandboxUnavailable('sandbox-unavailable')) as select:
                result = SC.run(dict(lean_env='/unexecuted', acceptance_tools_root=str(root / 'tools')),
                                temp, temp, str(root / 'work'))
            self.assertEqual(result, dict(verdict='ERROR', reason='sandbox-unavailable'))
            self.assertEqual(select.call_args.args, (str(root / 'tools/gate/launcher.py'),))

    def test_service_delegates_before_starting_its_checker_child(self):
        """A service parent and worker must not occupy the delegated domain root together."""
        from unittest.mock import patch
        from zk_registry import service_check as SC
        with tempfile.TemporaryDirectory() as temp:
            state = []
            (Path(temp) / 'verdict.json').write_text(json.dumps(dict(verdict='ERROR', reason='sandbox-unavailable')))

            def child(*args, **kwargs):
                self.assertEqual(state, ['delegated'])
                return L.RunResult(0, '', 0, False)

            with patch.object(SC, 'delegated_cgroup', side_effect=lambda: state.append('delegated'), create=True), \
                 patch.object(SC.CR, 'runner', return_value=child):
                result = SC.ProductionVerifier({})(temp, temp, temp)
            self.assertEqual(result['reason'], 'sandbox-unavailable')


class LiveConfinement(unittest.TestCase):
    """Fast, bounded Mathlib-free controls use the actual installed production boundary."""

    def setUp(self):
        """Provision no fixture fallback; an explicitly requested live run must really execute."""
        runtime = os.environ.get('BOOLE_TEST_SANDBOX')
        self.probe = os.environ.get('BOOLE_TEST_PROBE')
        if platform.system() != 'Linux' or not runtime or not self.probe:
            self.skipTest('live Linux runtime and compiled probe are supplied by the control runner')
        self.boundary = Sandbox.production(runtime)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.sequence = 0

    def run_probe(self, arguments, boundary=None):
        """Preserve stage receipt and diagnostic output for each real control."""
        self.sequence += 1
        directory = str(Path(self.temp.name) / str(self.sequence))
        Path(directory).mkdir()
        run = CR.runner(str(Path(directory) / 'logs'), 15, sandbox=boundary or self.boundary,
                        readable_paths=['/usr', self.temp.name], writable_paths=[self.temp.name])
        result = run([self.probe, *arguments], {'PATH': '/usr/bin:/bin'}, directory, 15, 256)
        self.assertIsNotNone(result.boundary, result.out)
        receipts = os.environ.get('BOOLE_TEST_CONTROL_RECEIPTS')
        if receipts:
            output = Path(receipts) / self._testMethodName
            output.mkdir(parents=True, exist_ok=True)
            with (output / (str(self.sequence) + '.json')).open('x') as stream:
                json.dump(dict(arguments=arguments, rc=result.rc, output=result.out,
                               boundary=result.boundary), stream, indent=2)
        return result

    def test_all_network_kinds_and_each_independent_layer(self):
        """Namespace-only and seccomp-only evidence stays explicitly test-fixture."""
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.addCleanup(listener.close)
        listener.bind(('0.0.0.0', 0))
        listener.listen(8)
        port = str(listener.getsockname()[1])
        udp_listener = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.addCleanup(udp_listener.close)
        udp_listener.bind(('127.0.0.1', 0))
        udp_listener.settimeout(.05)
        udp_port = str(udp_listener.getsockname()[1])
        vm_ip = os.environ.get('BOOLE_TEST_VM_IP')
        self.assertTrue(vm_ip, 'the control runner must supply its own non-loopback VM interface')
        cases = [('tcp', '127.0.0.1', port), ('tcp', vm_ip, port), ('tcp', '192.0.2.1', '9'),
                 ('tcp', '2001:db8::1', '9'), ('udp', '127.0.0.1', udp_port), ('raw', '127.0.0.1', port),
                 ('uring', '127.0.0.1', port)]
        layers = [('production', self.boundary),
                  ('namespace-only', replace(self.boundary, mode='test-fixture', disabled_layer='seccomp')),
                  ('seccomp-only', replace(self.boundary, mode='test-fixture', disabled_layer='net'))]
        for layer, boundary in layers:
            for arguments in cases:
                with self.subTest(layer=layer, kind=arguments[0], target=arguments[1]):
                    result = self.run_probe(list(arguments), boundary)
                    self.assertEqual(result.boundary['verdict'], 'PASS', result.boundary)
                    diagnostic = json.loads(result.out)
                    if layer == 'namespace-only' and arguments[0] == 'udp' and result.rc == 1:
                        # Linux may queue a datagram in the isolated namespace despite a down loopback.
                        self.assertEqual(diagnostic['sent_bytes'], 6)
                    else:
                        self.assertEqual(result.rc, 0, result.out)
                        self.assertTrue(diagnostic['blocked'])
                    if arguments[0] == 'udp':
                        with self.assertRaises(socket.timeout):
                            udp_listener.recvfrom(128)
                    self.assertEqual(result.boundary['assurance'], 'production' if layer == 'production'
                                     else 'test-fixture')

    def test_detached_and_double_fork_have_named_invalid_outcome(self):
        """Both live members and fast reaped orphans invalidate their own step."""
        for kind in ('detached', 'double-fork'):
            with self.subTest(kind=kind):
                result = self.run_probe([kind])
                self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                                 ('INVALID', 'owned-descendants-remained'))

    def test_pids_limit_is_invalid_but_reference_is_error(self):
        """The bounded fork bomb must hit a real pids controller, including reference precedence."""
        for role, verdict, reason in (('submission', 'INVALID', 'resource-cap'),
                                      ('reference', 'ERROR', 'resource-unavailable')):
            with self.subTest(role=role):
                result = self.run_probe(['fork-bomb'], replace(self.boundary, pids_max=8, role=role))
                self.assertEqual((result.boundary['verdict'], result.boundary['reason']), (verdict, reason))
                self.assertGreater(result.boundary['observation']['pids_max_events'], 0)

    def test_memory_oom_is_an_availability_error(self):
        """OOM cannot become a mathematical rejection or a closed success."""
        result = self.run_probe(['memory-hog'], replace(self.boundary, memory_max=64 * 1024 * 1024))
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']), ('ERROR', 'resource-unavailable'))
        self.assertGreater(result.boundary['observation']['oom_kill_events'], 0)

    def test_observer_descriptors_and_memory_are_private(self):
        """Pathless ptrace-class APIs cannot obtain the trusted PID-1's administrative handles."""
        for kind in ('parent-fd', 'parent-read', 'parent-write'):
            with self.subTest(kind=kind):
                result = self.run_probe([kind])
                self.assertEqual(result.rc, 0, result.out)
                self.assertEqual(json.loads(result.out), dict(blocked=True, errno=1))
                self.assertEqual(result.boundary['verdict'], 'PASS')
                self.assertEqual(result.boundary['observation']['pid_1_dumpable'], 0)

    def test_outside_write_and_readonly_mount_canary_are_inaccessible(self):
        """A real canary exists outside the explicitly allowed input paths."""
        canary = os.environ.get('BOOLE_TEST_CANARY')
        self.assertTrue(canary and Path(canary).is_file())
        for action in ('read', 'write'):
            with self.subTest(action=action):
                result = self.run_probe([action, canary])
                self.assertEqual(result.rc, 0, result.out)
                self.assertTrue(json.loads(result.out)['blocked'])

    def test_whole_host_readable_mount_is_refused(self):
        """Readonly root must not overwrite the private proc or device mounts."""
        with self.assertRaisesRegex(SandboxUnavailable, 'sandbox-unavailable'):
            self.boundary.command(['/usr/bin/true'], {}, ['/'], [])


if __name__ == '__main__':
    unittest.main()
