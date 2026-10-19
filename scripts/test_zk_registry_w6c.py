"""Solution reads retain prior submission attribution while sealed tools stay trusted."""
import inspect
from contextlib import contextmanager
import platform
import os
from pathlib import Path
import tempfile
import unittest
import uuid
from unittest.mock import patch
from zk_registry import closure as D, lean_runner as L, correctness_runtime as CR, safe_fs as FS
from zk_registry.sandbox import SandboxUnavailable
import test_zk_registry_w6b as w6b

class ExportAttribution(unittest.TestCase):
    fixture = w6b.TrustedExposure.fixture

    def export(self, root, work, boundary, *, exposed=True, sealed=None):
        env=L.LeanEnv('/usr', [], 'v4.34.1', {}, '', str(root/'environment'))
        tools=D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None)
        kwargs={'exposure':boundary.exposed_writers} if exposed and 'exposure' in inspect.signature(D.export_module).parameters else {}
        with patch.object(CR,'runner',return_value=lambda *args: L.RunResult(0,'fixture export',0,False)):
            return D.export_module(env,'ProofMod','Fixture.goal',str((sealed or root/'sealed')/('unit-'+uuid.uuid4().hex+'.ndjson')),
            tools,boundary,str(work/'proof'),[str(work/'proof'),str(work/'build')],10,[],**kwargs)

    def expose(self, root, work, boundary, run):
        (work/'proof').mkdir();(work/'build').mkdir()
        with patch.object(L,'run_process',run):
            L.run_submission(['/bin/sh','-c','true'],{'PATH':'/usr/bin:/bin'},str(work),10,256)
        self.assertTrue(boundary.exposed_writers)

    def test_solution_build_symlink_is_invalid(self):
        with self.fixture() as (root,work,boundary,run,finish):
            self.expose(root,work,boundary,run)
            (work/'build').rename(work/'real-build');(work/'build').symlink_to(work/'real-build')
            with self.assertRaises(L.BoundaryInvalid):self.export(root,work,boundary)

    def test_solution_proof_chmod_zero_is_invalid(self):
        with self.fixture() as (root,work,boundary,run,finish):
            self.expose(root,work,boundary,run)
            for mode in [0,0o400,0o600]:
                with self.subTest(mode=oct(mode)):
                    (work/'proof').chmod(mode)
                    try:
                        with self.assertRaises(L.BoundaryInvalid):self.export(root,work,boundary)
                    finally:(work/'proof').chmod(0o700)

    def test_pristine_challenge_ignores_submission_exposure(self):
        with self.fixture() as (root,work,boundary,run,finish):
            self.expose(root,work,boundary,run)
            (work/'build').rename(work/'real-build');(work/'build').symlink_to(work/'real-build')
            with self.assertRaises(SandboxUnavailable):self.export(root,work,boundary,exposed=False)

    def test_sealed_directory_alias_of_exposed_root_is_error(self):
        with self.fixture() as (root,work,boundary,run,finish):
            self.expose(root,work,boundary,run)
            with patch.object(CR,'runner',return_value=lambda *args:finish(args[0])):
                result=self.export(root,work,boundary,sealed=work)
                self.assertEqual((result['verdict'],result['reason']),('ERROR','sandbox-unavailable'))

    def test_comparator_directory_alias_of_exposed_root_is_error(self):
        with self.fixture() as (root,work,boundary,run,finish):
            self.expose(root,work,boundary,run)
            challenge=root/'Challenge';solution=root/'Solution';challenge.write_text('x');solution.write_text('x')
            tools=D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None)
            def comparator(command, env, *args):
                Path(env['COMPARATOR_BOUNDARY_STATUS']).write_text('{"inner_exec":"ok","exit":0}\n')
                return finish(command)
            with patch.object(CR,'runner',return_value=comparator):
                result=D.compare_exports(str(challenge),str(solution),'Fixture.goal',tools,boundary,str(work),10,[])
                self.assertEqual((result['verdict'],result['reason']),('ERROR','sandbox-unavailable'))

@unittest.skipUnless(platform.system() == 'Linux' and os.environ.get('BOOLE_TEST_SANDBOX'), 'Linux production boundary required')
class LiveExportAttribution(ExportAttribution):
    @contextmanager
    def fixture(self):
        from zk_registry.sandbox import Sandbox
        boundary=Sandbox.production(os.environ['BOOLE_TEST_SANDBOX'])
        with tempfile.TemporaryDirectory(dir=os.environ.get('BOOLE_TEST_CONTROL_RECEIPTS')) as temp:
            root=Path(temp).resolve();work=root/'work';work.mkdir()
            def run(argv,env,cwd,*args):
                command=boundary.command(argv,env,['/usr',str(root)],[str(work)],submission_controlled=True)
                return CR.runner(str(root/'exposure-processes'),10)(command,env,cwd,10,256)
            run.submission_boundary=boundary
            yield root,work,boundary,run,lambda command: L.RunResult(0,'',0,False)

if __name__=='__main__':unittest.main()
