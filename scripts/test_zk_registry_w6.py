"""Gate follow-up regressions for deterministic content and node boundaries."""
import os
import json
from contextlib import nullcontext
from types import SimpleNamespace
from pathlib import Path
import struct
import platform
import errno
import tempfile
import unittest
from unittest.mock import patch

from test_zk_registry_aa_fix3 import launcher_module
import test_zk_registry_aa_fix3 as fix3
from test_zk_registry_aa_fix3b import elf
from test_zk_registry_ratchet import offline_problem
from zk_registry import acceptance, service_check as SC
from zk_registry import safe_fs as FS, lean_runner as L, ratchet_gnark as RG
from zk_registry import correctness_runtime as CR


class ExecutableContent(unittest.TestCase):
    def test_program_header_limit_is_fixed_across_node_page_sizes(self):
        launcher = launcher_module()
        load = struct.pack('<IIQQQQQQ', 1, 5, 0, 0x400000, 0, 120, 120, 4096)
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(launcher.platform, 'machine', return_value='aarch64'):
            program = Path(temp) / 'program'
            for page in (4096, 16384, 65536):
                with self.subTest(page=page), patch.object(launcher.os, 'sysconf', return_value=page):
                    program.write_bytes(elf([load] * 73))
                    program.chmod(0o755)
                    self.assertEqual(launcher.submission_executable([str(program)], {}), str(program))
                    program.write_bytes(elf([load] * 74))
                    with self.assertRaisesRegex(launcher.SubmissionExecFailed, 'program-headers'):
                        launcher.submission_executable([str(program)], {})

    def test_255_byte_shebang_without_newline_is_not_truncated(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            program = Path(temp) / 'program'
            program.write_bytes(b'#!' + b' ' * 246 + b'/bin/sh')
            program.chmod(0o755)
            self.assertEqual(launcher.submission_executable([str(program)], {}), str(program))


class RevealErrors(unittest.TestCase):
    def test_acceptance_read_error_has_fixed_reason_and_private_detail(self):
        with tempfile.TemporaryDirectory() as temp:
            package, problem = offline_problem(temp)
            answer = Path(temp) / 'answer'
            answer.mkdir()
            (answer / 'Solution.lean').write_text('test input')
            work = Path(temp) / 'work'
            work.mkdir()
            env = SimpleNamespace(lean_version=problem['env']['lean'], extra={})
            detail = 'node-private-materialization-path-' + temp
            with patch.object(SC.L, 'load_production_env', return_value=env), \
                    patch.object(acceptance, 'read', side_effect=OSError(detail)):
                result = SC.run({'lean_env': 'test'}, package, str(answer), str(work))
            self.assertEqual(result, {'verdict': 'ERROR', 'reason': 'acceptance-unavailable'})
            self.assertEqual(json.loads((work / 'error.json').read_text())['message'], detail)


class WriterClassification(unittest.TestCase):
    @staticmethod
    def inspect(launcher, work, program, exposed=()):
        options = SimpleNamespace(authority='fixture', memory=16 * 1024**2, pids=16,
                                  exec_origin='submission', exposed_writer=exposed)
        with patch.object(launcher, 'bundle', return_value=(work.parent / 'authority', {})):
            launcher.execute(options, ['--rwx', str(work), '--', str(program)])

    def test_walk_distinguishes_node_ancestors_from_submission_children(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            node = root / 'node'
            work = node / 'work'
            work.mkdir(parents=True)
            program = work / 'program'
            program.write_bytes(b'#!/bin/sh\n')
            program.chmod(0o755)
            (root / 'node-alias').symlink_to(node, target_is_directory=True)
            (work / 'alias').symlink_to(work, target_is_directory=True)
            with self.assertRaises(OSError):
                self.inspect(launcher, root / 'node-alias/work', root / 'node-alias/work/program')
            with self.assertRaises(launcher.SubmissionExecFailed):
                self.inspect(launcher, work, work / 'alias/program')

    def test_altered_root_is_invalid_only_after_exposure(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            work = Path(temp)
            work.chmod(0o500)
            with self.assertRaises(ValueError) as result:
                self.inspect(launcher, work, work / 'program')
            self.assertNotIsInstance(result.exception, launcher.SubmissionExecFailed)
            work.chmod(0o500)
            with self.assertRaises(launcher.SubmissionExecFailed):
                self.inspect(launcher, work, work / 'program', exposed={str(work): (work.stat().st_dev, work.stat().st_ino)})

    def test_path_search_skips_non_executable_first_match(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name, mode in [('first', 0o644), ('second', 0o755)]:
                directory = root / name
                directory.mkdir()
                program = directory / 'tool'
                program.write_bytes(b'#!/bin/sh\n')
                program.chmod(mode)
            result = launcher.submission_executable(['tool'], {'PATH': str(root / 'first') + ':' + str(root / 'second')})
            self.assertEqual(result, str(root / 'second/tool'))


class HostOutput(unittest.TestCase):
    def test_unknown_owner_record_is_retained(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for index, owner in enumerate([99999, {'pid': 99999}, {'pid': 99999, 'start_time': 1, 'pid_namespace': []}]):
                directory = root / str(index)
                directory.mkdir()
                (directory / '.owner.json').write_text(json.dumps(owner))
                (directory / 'receipt.json').write_text('retain')
            with patch.object(CR.os, 'kill', side_effect=ProcessLookupError(errno.ESRCH, 'absent')):
                CR.sweep_dead_receipts(CR.Path(root))
            self.assertEqual(len(list(root.glob('*/receipt.json'))), 3)

    def test_exposed_directory_mutation_failure_is_invalid(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            work = root / 'work'
            work.mkdir()
            work.chmod(0o500)
            try:
                with getattr(FS, 'submission_io', lambda roots: nullcontext())({str(work)}):
                    with self.assertRaisesRegex(L.BoundaryInvalid, 'submission-output-path'):
                        FS.makedirs(work / 'home', exist_ok=True)
                self.assertEqual(work.stat().st_mode & 0o700, 0o700)
            finally:
                work.chmod(0o700)

    def test_report_tampering_keeps_original_failure_and_private_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            work = root / 'work'
            work.mkdir()
            canary = root / 'canary'
            canary.write_text('retain')
            (work / 'process-receipts.json').symlink_to(canary)
            guard = SimpleNamespace(step_reports=[{'boundary': {'verdict': 'INVALID'}}])
            with getattr(FS, 'submission_io', lambda roots: nullcontext())({str(work)}), \
                    patch.object(CR, 'failed_receipt_directory', return_value=CR.Path(root / 'private')):
                with self.assertRaisesRegex(L.BoundaryInvalid, '^original-failure$'):
                    with CR.receipt_report(guard, str(work)):
                        raise L.BoundaryInvalid('original-failure')
            self.assertEqual(canary.read_text(), 'retain')
            self.assertEqual(len(guard.failed_receipt_reports), 1)
            saved = json.loads(Path(guard.failed_receipt_reports[0]).read_text())
            self.assertEqual(saved['process_receipts'], guard.step_reports)

    def test_gnark_submission_symlink_is_invalid_without_reading_target(self):
        with tempfile.TemporaryDirectory() as temp:
            work = Path(temp).resolve() / 'compile'
            work.mkdir()
            secret = work.parent / 'node-secret'
            secret.write_text('{"status":"ok"}')
            (work / 'harness.json').symlink_to(secret)
            context = getattr(FS, 'submission_io', lambda roots: nullcontext())({str(work)})
            with context, patch.object(RG, 'reference_text', return_value='wrapper'), \
                    patch.object(RG, 'build_tool', return_value={'binary': 'test'}), \
                    patch.object(RG, 'run_tool', return_value=SimpleNamespace(rc=0, secs=0, peak_rss_mb=0)):
                with self.assertRaisesRegex(L.BoundaryInvalid, 'submission-output-path'):
                    RG.compile_program({'reference': {'name': 'f', 'wrapper': {'id': 'w'}}},
                                       'snapshot', 'tools', str(work))
            self.assertEqual(secret.read_text(), '{"status":"ok"}')


@unittest.skipUnless(Path('/proc/self/ns/pid').exists(), 'Linux process identity required')
class ReceiptIdentity(unittest.TestCase):
    def test_legacy_pid_without_namespace_is_never_proven_dead(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            legacy = root / 'legacy'
            legacy.mkdir()
            receipt = legacy / '99999-1.json'
            receipt.write_text('legacy evidence')
            with patch.object(CR.os, 'kill', side_effect=ProcessLookupError(errno.ESRCH, 'not in this namespace')):
                CR.sweep_dead_receipts(CR.Path(root))
            self.assertEqual(receipt.read_text(), 'legacy evidence')

    def test_reused_pid_sweeps_only_matching_pid_namespace(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            owner = CR.owner_record()
            for name, namespace in [('reused', owner['pid_namespace']), ('foreign', 'pid:[1]')]:
                directory = root / name
                directory.mkdir()
                record = dict(owner, start_time='0', pid_namespace=namespace)
                (directory / '.owner.json').write_text(json.dumps(record))
                (directory / 'receipt.json').write_text('retained evidence')
            CR.sweep_dead_receipts(CR.Path(root))
            self.assertFalse((root / 'reused').exists())
            self.assertTrue((root / 'foreign/receipt.json').is_file())


@unittest.skipUnless(platform.system() == 'Linux', 'Linux production boundary required')
class LiveOutputs(unittest.TestCase):
    setUp = fix3.LiveExec.setUp

    def test_exposed_writer_with_node_ancestor_failure_stays_error(self):
        root = Path(self.temp.name).resolve()
        node = root / 'node'
        node.mkdir()
        work = node / 'work'
        work.mkdir()
        run = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                        readable_paths=['/usr', str(root)], writable_paths=[str(work)])
        with patch.object(L, 'run_process', run):
            exposed = L.run_submission(['/bin/sh', '-c', 'true'], {'PATH': '/usr/bin:/bin'}, str(work), 20, 256)
        self.assertEqual(exposed.boundary['verdict'], 'PASS')
        node.chmod(0o300)
        try:
            with self.assertRaises(SC.SandboxUnavailable):
                run(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, str(work), 20, 256)
        finally:
            node.chmod(0o700)
        fix3.retain_control(self, {'exposure': exposed.process_receipt, 'node_ancestor': 'ERROR'})

    def test_gnark_chmod_0300_output_is_invalid_and_recoverable(self):
        root = Path(self.temp.name).resolve()
        work = root / 'compile'
        work.mkdir()
        run = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                        readable_paths=['/usr', str(root)], writable_paths=[str(work)])
        receipts = []

        def harness(*args):
            with patch.object(L, 'run_process', run):
                result = L.run_submission(['/bin/sh', '-c', 'printf \'{"status":"ok"}\' > harness.json; chmod 0300 .'],
                                          {'PATH': '/usr/bin:/bin'}, str(work), 20, 256)
            self.assertEqual((result.rc, result.boundary['verdict']), (0, 'PASS'))
            receipts.append(result.process_receipt)
            return result

        context = getattr(FS, 'submission_io', lambda roots: nullcontext())(
            getattr(self.boundary, 'exposed_writers', {str(work)}))
        with context, \
                patch.object(RG, 'reference_text', return_value='wrapper'), \
                patch.object(RG, 'build_tool', return_value={'binary': 'test'}), \
                patch.object(RG, 'run_tool', side_effect=harness):
            with self.assertRaisesRegex(L.BoundaryInvalid, 'submission-output-path'):
                RG.compile_program({'reference': {'name': 'f', 'wrapper': {'id': 'w'}}},
                                   'snapshot', 'tools', str(work))
        self.assertEqual(work.stat().st_mode & 0o700, 0o700)
        self.assertEqual(json.loads((work / 'harness.json').read_text()), {'status': 'ok'})
        fix3.retain_control(self, {'process_receipts': receipts, 'outcome': 'INVALID',
                                  'reason': 'submission-output-path'})
