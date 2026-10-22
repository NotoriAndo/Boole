"""Final sandbox-review controls through the real public execution/compare paths."""
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
from dataclasses import replace
import unittest
from unittest.mock import patch
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import correctness_runtime as CR
from zk_registry import bounded_capture as BC
from zk_registry import closure as D
from zk_registry.sandbox import Sandbox
from zk_registry.sandbox import SandboxUnavailable
from test_zk_registry_aa_fix import comparator_status_program


def retain_control(test, observation):
    root = os.environ.get('BOOLE_TEST_CONTROL_RECEIPTS')
    if root:
        directory = Path(root) / 'aa-fix3'
        directory.mkdir(parents=True, exist_ok=True)
        with (directory / (test.id() + '.json')).open('x') as stream:
            json.dump(observation, stream, indent=2)


def launcher_module():
    source = Path(__file__).parent / 'zk_registry/sandbox_backend/launcher.py'
    spec = importlib.util.spec_from_file_location('aa_fix3_launcher', source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def elf_header(machine):
    return (b'\x7fELF\x02\x01\x01' + bytes(9) +
            struct.pack('<HHIQQQIHHHHHH', 2, machine, 1, 0, 0, 0, 0, 64, 56, 0, 64, 0, 0))


class ExecContent(unittest.TestCase):
    def test_content_inspection_never_follows_a_parent_symlink(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            outside = root / 'outside'
            outside.mkdir()
            program = outside / 'program'
            program.write_bytes(elf_header(183 if platform.machine() in ('arm64', 'aarch64') else 62))
            program.chmod(0o755)
            (root / 'escape').symlink_to(outside, target_is_directory=True)
            with self.assertRaises(launcher.SubmissionExecFailed):
                launcher.submission_executable([str(root / 'escape/program')], {})

    def test_shebang_interpreter_token_uses_kernel_whitespace_rules(self):
        launcher = launcher_module()
        for header in (b'#!/bin/sh\r\n', b'#!\v/bin/sh\n'):
            with self.subTest(header=header), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                program = root / 'bad-interpreter'
                program.write_bytes(header + b'exit 0\n')
                program.chmod(0o755)
                options = SimpleNamespace(authority='fixture', memory=1_000_000_000,
                                          pids=128, exec_origin='submission', cgroup=str(root))
                with patch.object(launcher, 'bundle', return_value=(root.parent / (root.name + '-trusted'), {})), \
                        patch.object(launcher, 'execution_preflight', return_value=None, create=True), \
                        patch.object(launcher, 'create_step', side_effect=AssertionError('payload launch attempted')):
                    with self.assertRaises(launcher.SubmissionExecFailed):
                        launcher.execute(options, ['--rw', str(root), '--', str(program)])

    def test_foreign_elf_is_rejected_before_any_payload_launch(self):
        """Kernel handlers cannot turn foreign submission content into an accepted executable."""
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            program = root / 'foreign'
            native = 183 if platform.machine() in ('arm64', 'aarch64') else 62
            program.write_bytes(elf_header(62 if native == 183 else 183))
            program.chmod(0o755)
            options = SimpleNamespace(authority='fixture', memory=1_000_000_000,
                                      pids=128, exec_origin='submission', cgroup=str(root))
            with patch.object(launcher, 'bundle', return_value=(root.parent / (root.name + '-trusted'), {})), \
                    patch.object(launcher, 'create_step', side_effect=AssertionError('payload launch attempted')):
                with self.assertRaises(launcher.SubmissionExecFailed):
                    launcher.execute(options, ['--rw', str(root), '--', str(program)])


class LiveExec(unittest.TestCase):
    def setUp(self):
        self.boundary = Sandbox.production(os.environ['BOOLE_TEST_SANDBOX'])
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)

    def execute_artifact(self, root, path):
        result = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                         readable_paths=['/usr', str(root)], writable_paths=[str(root)])(
            [str(path)], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        retain_control(self, {'rc': result.rc, 'boundary': result.boundary,
                              'process_receipt': getattr(result, 'process_receipt', None)})
        return result

    def test_valid_native_header_exec_errno_is_node_error(self):
        """ETXTBSY after a valid native ELF classification is a node ERROR, never INVALID."""
        root = Path(self.temp.name)
        program = root / 'valid-header-kernel-enoexec'
        shutil.copyfile('/usr/bin/true', program)
        program.chmod(0o755)
        with program.open('r+b'):
            result = self.execute_artifact(root, program)
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('ERROR', 'sandbox-unavailable'), result.boundary)

    def test_missing_submission_is_invalid(self):
        root = Path(self.temp.name)
        result = self.execute_artifact(root, root / 'missing')
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('INVALID', 'submission-exec-failed'), result.boundary)

    def test_foreign_elf_is_invalid(self):
        root = Path(self.temp.name)
        program = root / 'foreign'
        program.write_bytes(elf_header(62 if platform.machine() == 'aarch64' else 183))
        program.chmod(0o755)
        result = self.execute_artifact(root, program)
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('INVALID', 'submission-exec-failed'), result.boundary)

    def test_noexec_work_root_is_node_error(self):
        with tempfile.TemporaryDirectory(dir=os.environ['BOOLE_TEST_NOEXEC_ROOT']) as temp:
            root = Path(temp)
            program = root / 'true'
            shutil.copy2('/usr/bin/true', program)
            result = self.execute_artifact(root, program)
            self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                             ('ERROR', 'sandbox-unavailable'), result.boundary)

    def test_even_a_replaced_exec_probe_runs_inside_confinement(self):
        """Trusted fault injection substitutes the probe bytes, never a production proof."""
        root = Path(self.temp.name)
        work, fixture = root / 'work', root / 'fixture-gate'
        work.mkdir()
        fixture.mkdir()
        canary = root / 'outside-canary'
        canary.write_text('unchanged')
        privileged = work / 'privileged-canary'
        privileged.write_text('unchanged')
        program = root / 'injected-probe'
        program.write_text('#!/usr/bin/python3\nfrom pathlib import Path\nimport os\n'
            'if "BOOLE_PROBE_SECRET" in os.environ: raise SystemExit(98)\n'
            'for target in ' + repr([str(canary), str(privileged)]) + ':\n'
            ' try:\n  Path(target).write_text("escaped")\n'
            ' except OSError: pass\n else: raise SystemExit(99)\n'
            'print("PROBE-CONFINED-READONLY",flush=True)\n'
            'print("Lean default kernel accepts the solution",flush=True)\n')
        source = (Path(__file__).parent / 'zk_registry/sandbox_backend/isolate.c').read_text()
        needle = 'open("/usr/bin/true", O_RDONLY | O_NOFOLLOW | O_CLOEXEC)'
        self.assertEqual(source.count(needle), 1)
        injected = source.replace(needle, 'open(' + json.dumps(str(program)) + ', O_RDONLY | O_NOFOLLOW | O_CLOEXEC)')
        (root / 'injected.c').write_text(injected)
        compiled = subprocess.run(['/usr/bin/gcc', '-O2', '-static', '-Wall', '-Wextra', '-Werror',
            str(root / 'injected.c'), '-o', str(fixture / 'gate-isolate')], capture_output=True, text=True)
        self.assertEqual(compiled.returncode, 0, compiled.stderr)
        installed = Path(self.boundary.executable).parent
        for name in ['launcher.py', 'comparator-landrun', 'seccomp-' + platform.machine() + '.bpf']:
            shutil.copy2(installed / name, fixture / name)
        data = json.loads((installed / 'bundle.json').read_text())
        data['sandbox']['architectures'][platform.machine()]['isolate_sha256'] = hashlib.sha256(
            (fixture / 'gate-isolate').read_bytes()).hexdigest()
        (fixture / 'bundle.json').write_text(json.dumps(data))
        boundary = replace(self.boundary, executable=str(fixture / 'launcher.py'),
            authority=hashlib.sha256((fixture / 'bundle.json').read_bytes()).hexdigest())
        result = CR.runner(str(root / 'logs'), 20, sandbox=boundary,
            readable_paths=['/usr', str(root)], writable_paths=[str(work)])(
            ['/usr/bin/python3', '-c', 'from pathlib import Path;Path(' + repr(str(privileged)) +
             ').write_text("payload allowed");print("PAYLOAD-ONLY")'],
             {'PATH': '/usr/bin:/bin', 'BOOLE_PROBE_SECRET': 'must-not-reach-probe'}, str(work), 20, 256)
        self.assertEqual((result.rc, result.boundary['verdict']), (0, 'PASS'), result.boundary)
        self.assertEqual(result.out.strip(), 'PAYLOAD-ONLY')
        self.assertEqual(canary.read_text(), 'unchanged')
        self.assertEqual(privileged.read_text(), 'payload allowed')
        retain_control(self, {'assurance': 'test-fixture; not a proof', 'boundary': result.boundary,
            'stdout': result.out, 'canary_unchanged': True, 'probe_readonly_parent_writable': True,
            'probe_cannot_spoof_kernel_stdout': True,
            'fixture_source_sha256': hashlib.sha256(injected.encode()).hexdigest()})


class RegisteredBinfmt(unittest.TestCase):
    def test_registered_handlers_make_production_unavailable(self):
        root = Path('/proc/sys/fs/binfmt_misc')
        self.assertFalse([p for p in root.iterdir() if p.name not in {'register', 'status'}])
        Sandbox.production(os.environ['BOOLE_TEST_SANDBOX'])
        name = 'boole-aa-fix3-control-' + str(os.getpid())
        target = root / name
        self.assertFalse(target.exists())
        rule = ':' + name + ':M::BOOLE-AA-FIX3::/usr/bin/false:'
        subprocess.run(['sudo', '-n', '/usr/bin/python3', '-I', '-c',
            'from pathlib import Path;import sys;Path("/proc/sys/fs/binfmt_misc/register").write_text(sys.argv[1])',
            rule], check=True)
        try:
            with self.assertRaises(SandboxUnavailable):
                Sandbox.production(os.environ['BOOLE_TEST_SANDBOX'])
        finally:
            subprocess.run(['sudo', '-n', '/usr/bin/python3', '-I', '-c',
                'from pathlib import Path;import sys;Path(sys.argv[1]).write_text("-1")', str(target)], check=True)
        self.assertFalse(target.exists())
        Sandbox.production(os.environ['BOOLE_TEST_SANDBOX'])
        retain_control(self, {'owned_handler_caused_preflight_ERROR': True,
                             'healthy_before_and_after': True, 'handler_removed': True})


class LiveCompare(unittest.TestCase):
    setUp = LiveExec.setUp

    def compare_control(self, suffix, *, acceptance_markers=True):
        """Trusted Comparator transport fixture; no theorem acceptance is claimed."""
        root = Path(self.temp.name)
        for name in ['Challenge.ndjson', 'Solution.ndjson']:
            (root / name).write_text('transport control, not a proof')
        program = root / 'comparator-control'
        program.write_text('#!/usr/bin/python3\n' + comparator_status_program() + 'import os,json\nfrom pathlib import Path\n'
            'Path(os.environ["COMPARATOR_BOUNDARY_STATUS"]).write_text('
            'json.dumps(dict(inner_exec="ok",exit=0))+"\\n")\n'
            + ('print("Lean default kernel accepts the solution",flush=True)\n'
               'print("nanoda kernel accepts the solution",flush=True)\n' if acceptance_markers else '') + suffix)
        program.chmod(0o755)
        tools = D.Tools(str(program), '/usr/bin/true', '/usr/bin/true', '/usr', self.boundary.executable)
        result = D.compare_exports(str(root / 'Challenge.ndjson'), str(root / 'Solution.ndjson'),
            'Control.goal', tools, self.boundary, str(root / 'compare'), 10, [])
        retain_control(self, result)
        return result

    def test_noncontract_comparator_exit_is_node_error(self):
        actual = self.compare_control('raise SystemExit(3)\n')
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'sandbox-unavailable'), actual)

    def test_failed_inner_marker_is_error_even_with_semantic_exit(self):
        actual = self.compare_control('Path(os.environ["COMPARATOR_BOUNDARY_STATUS"]).write_text('
            'json.dumps(dict(inner_exec="failed",exit=0))+"\\n")\nraise SystemExit(2)\n')
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'sandbox-unavailable'), actual)

    def test_malformed_inner_exit_is_transport_error(self):
        actual = self.compare_control('Path(os.environ["COMPARATOR_BOUNDARY_STATUS"]).write_text('
            'json.dumps(dict(inner_exec="ok",exit="bad"))+"\\n")\nraise SystemExit(2)\n')
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'sandbox-unavailable'), actual)

    def test_early_semantic_rejection_without_inner_launch_is_still_invalid(self):
        semantic = comparator_status_program([dict(stage='policy', reason='comparator-statement-mismatch')], rc=2)
        actual = self.compare_control('Path(os.environ["COMPARATOR_BOUNDARY_STATUS"]).write_text("")\n'
            + semantic + 'print("statement mismatch",flush=True)\nraise SystemExit(2)\n', acceptance_markers=False)
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('INVALID', 'comparator-statement-mismatch'), actual)

    def test_signal_killed_comparator_is_node_error(self):
        actual = self.compare_control('import signal\nos.kill(os.getpid(),signal.SIGTERM)\n')
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'resource-unavailable'), actual)

    def test_sigkill_without_resource_events_is_node_error(self):
        actual = self.compare_control('import signal\nos.kill(os.getpid(),signal.SIGKILL)\n')
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'resource-unavailable'), actual)

    def test_noncontract_exit_with_pids_events_is_resource_error(self):
        self.boundary = replace(self.boundary, pids_max=32)
        actual = self.compare_control('children=[]\ntry:\n while True:\n'
            '  pid=os.fork()\n  if pid==0: os._exit(0)\n  children.append(pid)\n'
            'except OSError: pass\nfor pid in children: os.waitpid(pid,0)\nraise SystemExit(3)\n')
        self.assertGreater(actual['boundary']['observation']['pids_max_events'], 0)
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'resource-unavailable'), actual)

    def test_comparator_oom_is_resource_error(self):
        self.boundary = replace(self.boundary, memory_max=64 * 1024 * 1024)
        actual = self.compare_control('data=bytearray(512*1024*1024)\n')
        self.assertGreater(actual['boundary']['observation']['oom_kill_events'], 0)
        self.assertEqual((actual['verdict'], actual['reason']),
                         ('ERROR', 'resource-unavailable'), actual)

    def test_cleanup_failure_cannot_replace_compare_result(self):
        with patch.object(D.Path, 'rmdir', side_effect=OSError('injected cleanup failure')):
            actual = self.compare_control('raise SystemExit(0)\n')
        self.assertEqual(actual['verdict'], 'PASS', actual)
        self.assertTrue(actual['cleanup_errors'])
        self.assertIn('injected cleanup failure', actual['cleanup_errors'][0])


class CompletedCapture:
    """Supply completed stub bytes without starting live pipe-drain threads."""
    apply = BC.Capture.apply

    def __init__(self, child, stop):
        self.stdout, self.stderr = BC.sinks()
        stdout, stderr = child.communicate()
        self.stdout.feed(stdout or b'')
        self.stderr.feed(stderr or b'')
        self.stdout.finish()
        self.stderr.finish()

    def join(self):
        pass


class ReceiptRetention(unittest.TestCase):
    def test_dead_runner_receipts_are_swept_and_live_runner_is_untouched(self):
        root = Path('/run/user') / str(os.getuid()) / 'boole-gate-receipts/processes'
        root.mkdir(parents=True, exist_ok=True)
        child = os.fork()
        if child == 0:
            os._exit(0)
        live_record = CR.owner_record()
        with open('/proc/' + str(child) + '/stat') as stream:
            child_start = stream.read().rsplit(')', 1)[1].split()[19]
        dead_record = dict(live_record, pid=child, start_time=child_start)
        os.waitpid(child, 0)
        with tempfile.TemporaryDirectory() as temp:
            live = root / (Path(temp).name + '-live')
            dead = root / (Path(temp).name + '-dead')
            for directory, owner in [(live, live_record), (dead, dead_record)]:
                directory.mkdir(mode=0o700)
                (directory / '.owner.json').write_text(json.dumps(owner))
                (directory / 'receipt.json').write_text('retained observation')
                self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
            stub = Mock(pid=900001, returncode=0)
            stub.communicate.return_value = (b'stub', None)
            stub.poll.return_value = 0
            with patch.object(CR.subprocess, 'Popen', return_value=stub), \
                    patch.object(CR, 'processes', return_value={900001: (os.getpid(), 1, 'stub')}), \
                    patch.object(BC, 'Capture', CompletedCapture), \
                    patch('zk_registry.sandbox.registered_command', return_value=True):
                CR.runner(temp)(['stub'], {}, temp, 1)
            self.assertFalse(dead.exists(), 'proven-dead runner receipts were not swept')
            self.assertEqual((live / 'receipt.json').read_text(), 'retained observation')
            retain_control(self, {'proven_dead_swept': True, 'live_owner_untouched': True})

    def test_ten_thousand_stubbed_steps_keep_no_process_receipt_backlog(self):
        """Copy each observation into its returned step report; no Lean or real child is run."""
        child = Mock(pid=900001, returncode=0)
        child.communicate.return_value = (b'stubbed step', None)
        child.poll.return_value = 0
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(CR.subprocess, 'Popen', return_value=child), \
                patch.object(CR, 'processes', return_value={900001: (os.getpid(), 1, 'test-owned-stub')}), \
                patch.object(BC, 'Capture', CompletedCapture), \
                patch('zk_registry.sandbox.registered_command', return_value=True):
            run = CR.runner(temp)
            started = time.monotonic()
            for _ in range(10000):
                report = run(['stubbed-control'], {}, temp, 1)
            seconds = time.monotonic() - started
            entries = list(Path(run.log_directory).glob('*.json'))
            self.assertLessEqual(len(entries), 1, 'temporary process receipts accumulate after step reports')
            self.assertEqual(len(run.step_reports), 10000)
            self.assertTrue(all(row['exit_code'] == 0 and not row['reason'] for row in run.step_reports))
            self.assertEqual(child.communicate.call_count, 10000)
            self.assertEqual(report.process_receipt['exit_code'], 0)
            self.assertEqual(report.process_receipt['argv'], ['stubbed-control'])
            stdout = report.process_receipt['streams']['stdout']
            self.assertEqual(stdout['bytes'], len(b'stubbed step'))
            self.assertEqual(stdout['sha256'], hashlib.sha256(b'stubbed step').hexdigest())
            self.assertFalse(stdout['capture_failed'])
            retain_control(self, {'stubbed_steps': 10000, 'remaining_temporary_receipts': len(entries),
                                 'seconds': seconds,
                                 'last_step_report': report.process_receipt, 'Lean_executed': False})


class LiveMemory(unittest.TestCase):
    setUp = LiveExec.setUp

    def test_twenty_gb_payload_is_named_configuration_error_before_launch(self):
        boundary = replace(self.boundary, memory_max=20_000_000_000)
        with patch('zk_registry.sandbox.subprocess.run') as launch:
            with self.assertRaisesRegex(SandboxUnavailable, 'payload-memory-exceeds-node-envelope') as caught:
                boundary.command(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, ['/usr'], [self.temp.name])
            launch.assert_not_called()
        retain_control(self, {'configuration_error': str(caught.exception), 'launches': 0})

    def test_maximum_payload_and_supervisor_fit_twenty_gb(self):
        boundary = replace(self.boundary, memory_max=20_000_000_000 - 32 * 1024 * 1024)
        root = Path(self.temp.name)
        result = CR.runner(str(root / 'logs'), 10, sandbox=boundary,
                           readable_paths=['/usr'], writable_paths=[str(root)])(
            ['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, str(root), 10, 256)
        self.assertEqual(result.boundary['verdict'], 'PASS', result.boundary)
        self.assertEqual(result.boundary['whole_step_memory_max'], 20_000_000_000)
        retain_control(self, {'rc': result.rc, 'boundary': result.boundary,
                             'process_receipt': getattr(result, 'process_receipt', None)})


class MemoryConfiguration(unittest.TestCase):
    def test_oversized_payload_is_rejected_before_runtime_or_filesystem_access(self):
        boundary = Sandbox('production', '/unexecuted', 'fixture', '/unexecuted',
                           memory_max=20_000_000_000)
        with self.assertRaisesRegex(SandboxUnavailable, 'payload-memory-exceeds-node-envelope'):
            boundary.command(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, [], [])

    def test_every_production_profile_is_checked_by_shared_preflight(self):
        from zk_registry.sandbox import validate_memory_profiles
        data = json.loads((Path(__file__).parent / 'zk_registry/data/acceptance_tools_v2.json').read_text())
        validate_memory_profiles(data)
        for name, profile in data['binary_profiles'].items():
            if profile.get('assurance') != 'production':
                continue
            with self.subTest(profile=name):
                changed = json.loads(json.dumps(data))
                changed['binary_profiles'][name]['payload_memory_max_bytes'] = 20_000_000_000
                with self.assertRaisesRegex(SandboxUnavailable, 'payload-memory-exceeds-node-envelope'):
                    validate_memory_profiles(changed)

if __name__ == '__main__':
    unittest.main()
