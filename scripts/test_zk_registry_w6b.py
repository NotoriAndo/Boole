"""Node and trusted-tool failures must remain availability errors."""
from dataclasses import replace
from contextlib import contextmanager
import errno
import json
import os
from pathlib import Path
import platform
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from test_zk_registry_aa_fix3 import launcher_module
import test_zk_registry_aa_fix3 as fix3
from zk_registry import safe_fs as FS, lean_runner as L, correctness_runtime as CR
from zk_registry.sandbox import Sandbox, SandboxUnavailable
from zk_registry import sandbox as SB, closure as D
import test_zk_registry_aa_fix3b as fix3b


class NodeParents(unittest.TestCase):
    def test_recreated_root_has_no_inherited_exposure(self):
        launcher = launcher_module()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve() / 'work'
            root.mkdir()
            old = root.stat()
            exposed = {str(root): (old.st_dev, old.st_ino)}
            root.rename(root.with_name('old-work'))
            root.mkdir()
            root.chmod(0o500)
            try:
                with FS.submission_io(exposed), self.assertRaises(OSError):
                    FS.makedirs(root / 'child')
                with self.assertRaises(ValueError) as error:
                    launcher.prepare_writer_roots([str(root)], exposed=exposed)
                self.assertNotIsInstance(error.exception, launcher.SubmissionExecFailed)
            finally:
                root.chmod(0o700)

    def test_missing_output_and_eperm_stay_node_errors(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            with FS.submission_io({str(root)}):
                with self.assertRaises(FileNotFoundError):
                    FS.Path(root / 'missing').read_bytes()
                original = FS.os.open

                def denied(path, *args, **kwargs):
                    if str(path) == 'lsm-denied':
                        raise PermissionError(errno.EPERM, 'node LSM denial')
                    return original(path, *args, **kwargs)
                with patch.object(FS.os, 'open', side_effect=denied), self.assertRaises(PermissionError):
                    FS.Path(root / 'lsm-denied').read_bytes()

    def test_root_lookup_in_unsearchable_parent_is_node_error(self):
        launcher = launcher_module()
        for mode in (0o600, 0o400, 0o300):
            with self.subTest(mode=oct(mode)), tempfile.TemporaryDirectory() as temp:
                node = Path(temp).resolve() / 'node'
                node.mkdir()
                root = node / 'work'
                root.mkdir()
                identity = root.stat()
                exposed = {str(root): (identity.st_dev, identity.st_ino)}
                node.chmod(mode)
                try:
                    with FS.submission_io(exposed):
                        with self.assertRaises(OSError):
                            FS.reject_symlinks(root)
                    with self.assertRaises(OSError):
                        launcher.prepare_writer_roots([str(root)], exposed=exposed)
                finally:
                    node.chmod(0o700)


class TrustedExposure(unittest.TestCase):
    @contextmanager
    def fixture(self, role='submission'):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            work = root / 'work'
            work.mkdir()
            standard, safe = SB.Path, D.Path

            def mapped(kind):
                def factory(*parts):
                    parts = (str(root / 'run'), *parts[1:]) if str(parts[0]) == '/run/user' else parts
                    return kind(*parts)
                return factory

            boundary = Sandbox('production', '/usr/bin/true', 'fixture', str(root / 'cgroup'),
                               inner_executable='/usr/bin/true', role=role)

            def finish(command, *args):
                status = Path(command[command.index('--gate-status') + 1])
                status.write_text(json.dumps({'protocol': 1, 'assurance': 'production', 'verdict': 'PASS',
                    'descriptor_sha256': 'fixture', 'observation': {'setup_failed': False}}))
                return SB.capture_result(command, L.RunResult(0, 'Lean default kernel accepts the solution\n'
                    'nanoda kernel accepts the solution\n', 0, False))

            def run(argv, env, cwd, *args):
                return finish(boundary.command(argv, env, ['/usr', str(root)], [str(work)]))
            run.submission_boundary = boundary
            with patch.object(SB, 'Path', side_effect=mapped(standard)), patch.object(D, 'Path', side_effect=mapped(safe)):
                yield root, work, boundary, run, finish

    def test_trusted_tool_never_exposes_its_writer_root(self):
        with self.fixture() as (root, work, boundary, run, finish):
            run(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, str(work), 10, 256)
            self.assertFalse(boundary.exposed_writers)

    def test_reference_role_never_exposes_even_submission_marked_call(self):
        with self.fixture('reference') as (root, work, boundary, run, finish):
            with patch.object(L, 'run_process', run):
                L.run_submission(['/bin/sh', '-c', 'true'], {'PATH': '/usr/bin:/bin'}, str(work), 10, 256)
            self.assertFalse(boundary.exposed_writers)

    def test_submission_marked_call_retains_attribution(self):
        with self.fixture() as (root, work, boundary, run, finish):
            with patch.object(L, 'run_process', run):
                L.run_submission(['/bin/sh', '-c', 'true'], {'PATH': '/usr/bin:/bin'}, str(work), 10, 256)
            self.assertTrue(boundary.exposed_writers)
            (work / 'output').symlink_to(root / 'node-only')
            with FS.submission_io(boundary.exposed_writers), self.assertRaises(L.BoundaryInvalid):
                FS.Path(work / 'output').read_bytes()

    def test_replace_does_not_share_submission_exposure(self):
        boundary = Sandbox.fixture()
        reference = replace(boundary, role='reference')
        self.assertIsNot(reference.exposed_writers, boundary.exposed_writers)

    def test_missing_comparator_inner_status_remains_error(self):
        with self.fixture() as (root, work, boundary, run, finish):
            challenge, solution = root / 'Challenge.ndjson', root / 'ProofMod.ndjson'
            challenge.write_text('fixture export')
            solution.write_text('fixture export')
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', '/usr/bin/true')

            def comparator(command, env, *args):
                Path(env['COMPARATOR_BOUNDARY_STATUS']).unlink()
                return finish(command)

            with FS.submission_io(boundary.exposed_writers), patch.object(CR, 'runner', return_value=comparator):
                result = D.compare_exports(str(challenge), str(solution), 'Fixture.goal', tools, boundary,
                                           str(root / 'compare'), 10, [])
            self.assertEqual((result['verdict'], result['reason']), ('ERROR', 'sandbox-unavailable'))

    def test_trusted_status_symlink_error_ignores_outer_submission_context(self):
        with self.fixture() as (root, work, boundary, run, finish):
            metadata = root.stat()
            if isinstance(boundary.exposed_writers, dict):
                boundary.exposed_writers[str(root)] = (metadata.st_dev, metadata.st_ino)
            else:
                boundary.exposed_writers.add(str(root))
            challenge, solution = root / 'Challenge.ndjson', root / 'ProofMod.ndjson'
            challenge.write_text('fixture export')
            solution.write_text('fixture export')
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', '/usr/bin/true')

            def comparator(command, env, *args):
                status = Path(env['COMPARATOR_BOUNDARY_STATUS'])
                status.unlink()
                status.symlink_to(root / 'node-only')
                return finish(command)

            with FS.submission_io(boundary.exposed_writers), patch.object(CR, 'runner', return_value=comparator):
                result = D.compare_exports(str(challenge), str(solution), 'Fixture.goal', tools, boundary,
                                           str(root / 'compare'), 10, [])
            self.assertEqual((result['verdict'], result['reason']), ('ERROR', 'sandbox-unavailable'))


class BinfmtWiring(unittest.TestCase):
    def test_unguarded_alternative_cleanup_commands_are_rejected(self):
        original = Path.read_text
        path = Path(__file__).resolve().parents[1] / '.github/workflows/acceptance-gate-linux.yml'
        source = path.read_text()
        commands = [
            'for e in /proc/sys/fs/binfmt_*/[!rs]*; do printf -- -1 | sudo -n dd of="$e"; done',
            'sudo update-binfmts --disable',
            'sudo systemctl stop systemd-binfmt.service',
        ]
        for command in commands:
            changed = source + '\n      - name: Unguarded cleanup\n        run: ' + command + '\n'
            def content(value, *args, **kwargs):
                return changed if value == path else original(value, *args, **kwargs)
            with self.subTest(command=command), patch.object(Path, 'read_text', content):
                with self.assertRaises(AssertionError):
                    fix3b.CIWiring('test_disposable_runner_clears_binfmt_before_check_only_setup').test_disposable_runner_clears_binfmt_before_check_only_setup()


@unittest.skipUnless(platform.system() == 'Linux', 'Linux production boundary required')
class LiveNodeParents(unittest.TestCase):
    setUp = fix3.LiveExec.setUp

    def test_unsearchable_node_parent_is_error_after_submission_exposure(self):
        observations = []
        for mode in (0o600, 0o400, 0o300):
            with self.subTest(mode=oct(mode)):
                node = Path(self.temp.name).resolve() / oct(mode)
                node.mkdir()
                root = node / 'work'
                root.mkdir()
                run = CR.runner(str(Path(self.temp.name) / ('logs-' + oct(mode))), 20, sandbox=self.boundary,
                                readable_paths=['/usr', self.temp.name], writable_paths=[str(root)])
                with patch.object(L, 'run_process', run):
                    exposed = L.run_submission(['/bin/sh', '-c', 'true'], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
                self.assertEqual(exposed.boundary['verdict'], 'PASS')
                node.chmod(mode)
                try:
                    with self.assertRaises(SandboxUnavailable):
                        run(['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
                finally:
                    node.chmod(0o700)
                observations.append({'parent_mode': oct(mode), 'outcome': 'ERROR', 'exposure': exposed.process_receipt})
        fix3.retain_control(self, {'cases': observations})


@unittest.skipUnless(platform.system() == 'Linux' and os.environ.get('BOOLE_TEST_SANDBOX'),
                     'requires the live pinned Linux boundary')
class LiveTrustedChildren(unittest.TestCase):
    def test_trusted_reference_descendants_are_availability_errors(self):
        boundary = replace(Sandbox.production(os.environ['BOOLE_TEST_SANDBOX']), role='reference')
        programs = [
            'import os,time\npid=os.fork()\nif pid==0:\n os.setsid()\n'
            ' if os.fork()==0:\n  os._exit(0)\n os._exit(0)\n'
            'os.waitpid(pid,0)\ntime.sleep(.05)\n',
            'import os,time\nif os.fork()==0:\n time.sleep(30)\nelse:\n time.sleep(.05)\n',
        ]
        for index, source in enumerate(programs):
            with self.subTest(kind=index), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                program = root / 'trusted-tool.py'
                program.write_text(source)
                run = CR.runner(str(root / 'logs'), 20, sandbox=boundary,
                                readable_paths=['/usr', temp], writable_paths=[temp])
                result = run(['/usr/bin/python3', str(program)], {'PATH': '/usr/bin:/bin'}, temp, 20, 256)
                self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                                 ('ERROR', 'sandbox-unavailable'), result.out)
