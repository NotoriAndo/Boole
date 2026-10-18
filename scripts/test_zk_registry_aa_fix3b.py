"""Review regressions: deterministic verdicts and retained private execution evidence."""
import json
import os
from pathlib import Path
import sys
import struct
import shutil
import errno
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch, Mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import correctness_runtime as CR, lean_runner as L, ratchet as RT
from zk_registry import closure as D
from zk_registry import check as C
from zk_registry.sandbox import SandboxUnavailable
from zk_registry.sandbox import Sandbox
from test_zk_registry_aa_fix3 import launcher_module, LiveExec, retain_control


class FailureReceipts(unittest.TestCase):
    def test_invalid_elaboration_writes_its_consumed_receipt(self):
        receipt = {'boundary': {'verdict': 'INVALID', 'reason': 'owned-descendants-remained'}, 'pid': 123}
        guard = lambda *args, **kwargs: None
        guard.step_reports = [receipt]
        env = SimpleNamespace(extra={'acceptance_sandbox': Sandbox.fixture()})
        run = L.RunResult(0, '', 0, False, boundary=receipt['boundary'], process_receipt=receipt)
        with tempfile.TemporaryDirectory() as temp:
            work = Path(temp) / 'elaborate'
            with patch.object(CR, 'runner', return_value=guard), \
                    patch.object(L, 'compile_module', return_value=(run, [])):
                with self.assertRaisesRegex(L.BoundaryInvalid, 'owned-descendants-remained'):
                    RT.elaborate(env, temp, ['Model.lean'], 'X.p', [], str(work))
            saved = json.loads((work / 'process-receipts.json').read_text())
            self.assertEqual(saved['process_receipts'], [receipt])

    def test_axiom_probe_keeps_receipts_on_continue_and_boundary_exception(self):
        from zk_registry import axiom_policy as A, gates as G
        for boundary in [None, {'verdict':'INVALID','reason':'owned-descendants-remained'}]:
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as temp:
                root=Path(temp);build=root/'build';build.mkdir()
                module=build/'Module.olean';module.write_bytes(b'synthetic pristine module')
                receipt={'observation':'consumed probe evidence'}
                guard=lambda *args, **kwargs: None
                guard.step_reports=[receipt]
                row=dict(module='Module',name='X',module_olean_sha256=D.digest(module))
                run=L.RunResult(1,'',0,False,boundary=boundary,process_receipt=receipt)
                with patch.object(CR,'runner',return_value=guard), patch.object(G,'run_replay',return_value=(run,{})):
                    if boundary:
                        with self.assertRaises(L.BoundaryInvalid):
                            A.resolve(SimpleNamespace(lean_path=[]),[row],str(build),Sandbox.fixture(),1)
                    else:
                        self.assertEqual(A.resolve(SimpleNamespace(lean_path=[]),[row],str(build),Sandbox.fixture(),1),{})
                self.assertEqual(json.loads((root/'n2-0/process-receipts.json').read_text())['process_receipts'],[receipt])

    def test_compile_exception_keeps_receipts(self):
        guard=lambda *args, **kwargs: None
        guard.step_reports=[{'observation':'compile evidence'}]
        with tempfile.TemporaryDirectory() as temp, patch.object(CR,'runner',return_value=guard), \
                patch.object(L,'production_environment_reason',return_value=None), \
                patch.object(C,'_compile_and_postcheck',side_effect=L.BoundaryInvalid('owned-descendants-remained')):
            with self.assertRaises(L.BoundaryInvalid):
                C.compile_in_sandbox(None,{},temp,'',temp,1,Sandbox.fixture())
            self.assertEqual(json.loads((Path(temp)/'process-receipts.json').read_text())['process_receipts'],guard.step_reports)


class ContentClassification(unittest.TestCase):
    def test_submission_walk_failures_are_content_invalid(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / 'target'
            target.mkdir()
            program = target / 'program'
            program.write_bytes(b'#!/bin/sh\nexit 0\n')
            program.chmod(0o755)
            (root / 'alias').symlink_to(target, target_is_directory=True)
            (root / 'leaf').symlink_to(program)
            for path in [root / 'alias/program', root / 'leaf', program / 'bad']:
                with self.subTest(path=path), self.assertRaises(launcher.SubmissionExecFailed):
                    launcher.submission_executable([str(path)], {})
            for mode in [0o411, 0o111, 0o644]:
                program.chmod(mode)
                with self.subTest(mode=oct(mode)), self.assertRaises(launcher.SubmissionExecFailed):
                    launcher.submission_executable([str(program)], {})
            program.chmod(0o755)
            self.assertEqual(launcher.submission_executable([str(program)], {}), str(program))
            for error in [errno.ELOOP,errno.ENOTDIR,errno.EACCES]:
                with patch.object(launcher,'open_submission',side_effect=OSError(error,'controlled walk failure')), \
                        self.subTest(errno=error), self.assertRaises(launcher.SubmissionExecFailed):
                    launcher.submission_executable([str(program)], {})


class CIWiring(unittest.TestCase):
    def test_disposable_runner_clears_binfmt_before_check_only_setup(self):
        workflow = (Path(__file__).resolve().parents[1] / '.github/workflows/acceptance-gate-linux.yml').read_text()
        self.assertLess(workflow.index('name: Clear binfmt handlers on the disposable runner'),
                        workflow.index('name: Build and require the pinned hardened boundary'))
        clearing = workflow.split('name: Clear binfmt handlers on the disposable runner', 1)[1].split('\n      - name:', 1)[0]
        self.assertIn('tee "$entry"', clearing)
        self.assertIn('cat "$entry"', clearing)
        setup = (Path(__file__).parent / 'setup_zk_registry_sandbox.sh').read_text()
        self.assertNotIn('tee /proc/sys/fs/binfmt_misc/status', setup)


def elf(program_headers, payload=b'', **fields):
    header = dict(kind=2, machine=183, count=len(program_headers), entry_size=56)
    header.update(fields)
    return (b'\x7fELF\x02\x01\x01' + bytes(9) +
            struct.pack('<HHIQQQIHHHHHH', header['kind'], header['machine'], 1, 0, 64, 0, 0,
                        64, header['entry_size'], header['count'], 0, 0, 0) + b''.join(program_headers) + payload)


class KernelContent(unittest.TestCase):
    def test_kernel_rejected_headers_are_content_invalid(self):
        launcher = launcher_module()
        load = struct.pack('<IIQQQQQQ', 1, 5, 0, 0x400000, 0, 120, 120, 4096)
        bad = [elf([load], kind=1), elf([load], kind=4), elf([load + b'\0'], entry_size=57),
               elf([], count=0), elf([load] * 1171)]
        bad += [b'#!' + b' ' * 300 + b'/bin/sh\n']
        for note in [b'', bytes(16), struct.pack('<III4s', 4, 16, 5, b'GNU\0') +
                     struct.pack('<IIQ', 0xc0000000, 8, 1)]:
            ph = struct.pack('<IIQQQQQQ', 0x6474e553, 4, 120, 0, 0, len(note), len(note), 8)
            bad.append(elf([ph], note))
        with tempfile.TemporaryDirectory() as temp, patch.object(launcher.platform, 'machine', return_value='aarch64'):
            path = Path(temp) / 'program'
            for index, data in enumerate(bad):
                path.write_bytes(data); path.chmod(0o755)
                with self.subTest(index=index), self.assertRaises(launcher.SubmissionExecFailed):
                    launcher.submission_executable([str(path)], {})
            good_note = struct.pack('<III4sIIII', 4, 16, 5, b'GNU\0', 0xc0000000, 4, 1, 0)
            good_prop = struct.pack('<IIQQQQQQ', 0x6474e553, 4, 120, 0, 0, len(good_note), len(good_note), 8)
            for data in [elf([load]), elf([good_prop], good_note), b'#!/bin/sh ' + b'x' * 500]:
                path.write_bytes(data)
                self.assertEqual(launcher.submission_executable([str(path)], {}), str(path))


class WriterRoots(unittest.TestCase):
    def test_altered_writer_permissions_are_restored_and_submission_invalid(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'work'
            root.mkdir(mode=0o700)
            root.chmod(0o500)
            with self.assertRaises(launcher.SubmissionExecFailed):
                launcher.prepare_writer_roots([str(root)])
            self.assertEqual(root.stat().st_mode & 0o700, 0o700)
            launcher.prepare_writer_roots([str(root)])

    def test_only_owned_regular_reserved_probe_leftovers_are_removed(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            leftover = root / '.boole-exec-probe-0123456789abcdef'
            leftover.write_bytes(b'regenerable probe');leftover.chmod(0o700)
            source = root / '.boole-exec-probe-source.lean';source.write_text('retain')
            link = root / '.boole-exec-probe-fedcba9876543210';link.symlink_to(source)
            launcher.prepare_writer_roots([str(root)])
            self.assertFalse(leftover.exists())
            self.assertTrue(link.is_symlink())
            self.assertEqual(source.read_text(), 'retain')


class ComparatorContract(unittest.TestCase):
    def test_exit_zero_with_mismatched_output_is_never_invalid(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            a, b = root / 'Challenge.ndjson', root / 'Proof.ndjson'
            a.write_text('fixture');b.write_text('fixture')
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', '')
            for index, text in enumerate(['resource-cap: constants', 'Lean default kernel accepts the solution',
                                          'disallowed axiom', 'statement mismatch', '']):
                guard = lambda *args: L.RunResult(0, text, 0, False)
                with self.subTest(text=text), patch.object(CR, 'runner', return_value=guard):
                    result = D.compare_exports(str(a), str(b), 'X.p', tools, Sandbox.fixture(),
                                               str(root / str(index)), 1, [])
                    self.assertEqual((result['verdict'], result['reason']), ('ERROR', 'sandbox-unavailable'))


class ReceiptLifecycle(unittest.TestCase):
    def test_legacy_receipts_are_archived_before_purge(self):
        import gzip, hashlib
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            dead = root / 'legacy';dead.mkdir()
            raw = b'{"retained":"legacy evidence"}\n'
            (dead / '99999-1.json').write_bytes(raw)
            with patch.object(CR, 'owner_is_dead', return_value=True):
                CR.sweep_dead_receipts(CR.Path(root))
            manifests = list(root.glob('.legacy-archives/*.manifest.json'))
            self.assertEqual(len(manifests), 1)
            receipt = json.loads(manifests[0].read_text())['files'][0]
            self.assertEqual(gzip.decompress((root / '.legacy-archives' / receipt['archive']).read_bytes()), raw)
            self.assertEqual(receipt['raw_sha256'], hashlib.sha256(raw).hexdigest())
            self.assertFalse(dead.exists())

    def test_completed_runner_releases_empty_owned_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            original = CR.Path
            def mapped(value):
                return original(root / 'run') if str(value) == '/run/user' else original(value)
            child = Mock(pid=900001, returncode=0)
            child.communicate.return_value=(b'stub', None);child.poll.return_value=0
            owners=[]
            def launch(*args, **kwargs):
                owners.append(json.loads(Path(run.log_directory, '.owner.json').read_text()))
                return child
            with patch.object(CR, 'Path', side_effect=mapped), \
                    patch.object(CR, 'owner_record', create=True,
                                 return_value=dict(pid=os.getpid(), start_time='fixture', pid_namespace='fixture')), \
                    patch.object(CR.subprocess, 'Popen', side_effect=launch), \
                    patch.object(CR, 'processes', return_value={900001:(os.getpid(),1,'stub')}), \
                    patch('zk_registry.sandbox.registered_command', return_value=True):
                for _ in range(5):
                    run=CR.runner(str(root/'logs'));run(['stub'],{},str(root),1)
            directories=list((root/'run'/str(os.getuid())/'boole-gate-receipts/processes').iterdir())
            self.assertEqual(directories, [])
            self.assertTrue(all(row.get('start_time') and 'pid_namespace' in row for row in owners))

    def test_sweep_retains_symlinks_and_foreign_uid(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);directory=root/'dead';directory.mkdir()
            receipt=directory/'99999-1.json';receipt.write_text('retain')
            (root/'alias').symlink_to(directory,target_is_directory=True)
            with patch.object(CR,'owner_is_dead',return_value=True), \
                    patch.object(CR.os,'getuid',return_value=os.getuid()+1):
                CR.sweep_dead_receipts(CR.Path(root))
            self.assertEqual(receipt.read_text(),'retain')
            receipt.unlink();receipt.symlink_to(root/'alias')
            with patch.object(CR,'owner_is_dead',return_value=True):
                CR.sweep_dead_receipts(CR.Path(root))
            self.assertTrue(receipt.is_symlink())
            self.assertTrue((root/'alias').is_symlink())


class ProductionContract(unittest.TestCase):
    def test_worker_writes_receipts_only_to_private_report(self):
        from zk_registry import service_check as SC, service as S
        receipt={'pid':123,'start_utc':'changing'}
        guard=lambda *args: None
        guard.step_reports=[receipt]
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);config=root/'config.json';config.write_text('{}')
            argv=['service_check','--config',str(config),'--package',temp,'--submission',temp,'--work',temp]
            with patch.object(sys,'argv',argv), patch.object(SC,'run',return_value=dict(
                    verdict='PASS',assurance='production',process_receipts=[receipt])), \
                    patch.object(L,'run_process',guard):
                self.assertEqual(SC.main(),0)
            verdict=S.read_json(root/'verdict.json')
            self.assertEqual(set(verdict),{'verdict','assurance'})
            self.assertEqual(S.read_json(root/'process-receipts.json')['process_receipts'],[receipt])


class LiveReview(unittest.TestCase):
    setUp=LiveExec.setUp

    def run_work(self,root,work,program):
        result=CR.runner(str(root/'logs'),20,sandbox=self.boundary,
                        readable_paths=['/usr',str(root)],writable_paths=[str(work)])(
                            [str(program)],{'PATH':'/usr/bin:/bin'},str(work),20,256)
        return result

    def test_writer_content_failures_invalid_but_trusted_symlink_is_error(self):
        root=Path(self.temp.name);work=root/'work';work.mkdir()
        program=work/'program';shutil.copyfile('/usr/bin/true',program);program.chmod(0o755)
        (work/'alias').symlink_to(work,target_is_directory=True)
        (work/'leaf').symlink_to(program)
        paths=[work/'leaf',work/'alias/program',program/'child']
        observations=[]
        for path in paths:
            result=self.run_work(root,work,path)
            self.assertEqual((result.boundary['verdict'],result.boundary['reason']),('INVALID','submission-exec-failed'))
            observations.append(result.boundary)
        for mode in [0o411,0o111]:
            program.chmod(mode);result=self.run_work(root,work,program)
            self.assertEqual((result.boundary['verdict'],result.boundary['reason']),('INVALID','submission-exec-failed'))
            observations.append(result.boundary)
        program.chmod(0o755)
        (root/'trusted-alias').symlink_to(program)
        with self.assertRaises(SandboxUnavailable):self.run_work(root,work,root/'trusted-alias')
        trusted=root/'trusted-program';shutil.copyfile('/usr/bin/true',trusted);trusted.chmod(0o755)
        (root/'trusted-parent').symlink_to(root,target_is_directory=True)
        outside=[root/'trusted-parent/trusted-program',trusted/'child',root/'missing-trusted']
        trusted_observations=[]
        for path in outside:
            try:actual=self.run_work(root,work,path).boundary
            except SandboxUnavailable:actual=dict(verdict='ERROR',reason='sandbox-unavailable')
            self.assertEqual((actual['verdict'],actual['reason']),('ERROR','sandbox-unavailable'))
            trusted_observations.append(actual)
        for mode in [0o411,0o111]:
            trusted.chmod(mode);actual=self.run_work(root,work,trusted).boundary
            self.assertEqual((actual['verdict'],actual['reason']),('ERROR','sandbox-unavailable'))
            trusted_observations.append(actual)
        retain_control(self,dict(inside_writer=observations,outside_writer=trusted_observations))

    def test_altered_writer_root_invalid_then_next_launch_healthy(self):
        root=Path(self.temp.name);work=root/'work';work.mkdir();work.chmod(0o500)
        first=self.run_work(root,work,Path('/usr/bin/true'))
        self.assertEqual((first.boundary['verdict'],first.boundary['reason']),('INVALID','submission-exec-failed'))
        self.assertEqual(work.stat().st_mode & 0o700,0o700)
        second=self.run_work(root,work,Path('/usr/bin/true'))
        self.assertEqual((second.rc,second.boundary['verdict']),(0,'PASS'))
        retain_control(self,dict(altered=first.boundary,next_healthy=second.boundary))


class NamedConfiguration(unittest.TestCase):
    def test_named_configuration_failure_survives_compile_adapter(self):
        reason='sandbox-configuration:payload-memory-exceeds-node-envelope'
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(Sandbox,'production',side_effect=SandboxUnavailable(reason)):
            result=C.compile_in_sandbox(None,{},temp,'',temp,1)
            self.assertEqual(result['error'],reason)


if __name__ == '__main__':
    unittest.main()
