"""Release compatibility and recorded intake policy contracts."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
from zk_registry import check as C, service as S
from test_zk_registry_service import IntakeTests as Fixtures

class Review(unittest.TestCase):
    def test_recorded_policy_controls_legacy_replay(self):
        fixture=Fixtures();fixture.setUp();self.addCleanup(fixture.doCleanups)
        registry=fixture.registry
        events,state=registry._load()
        payload=copy.deepcopy(events[0]['payload']);payload.pop('submission_policy')
        registry.log.unlink();registry._append('catalog',payload,0,[])
        text='-- #check legacy annotation\n'
        (fixture.submission/'Solution.lean').write_text(text)
        fixture.submission_sha=S.submission_digest(str(fixture.submission))
        calls=[]
        def verifier(pkg,answer,work):
            hits=C.forbidden_scan([('proof',(Path(answer)/'Solution.lean').read_text())]);calls.append(hits)
            if hits:return dict(verdict='INVALID',reason='forbidden-text')
            return fixture.verify(pkg,answer,work)
        registry.verifier=verifier
        seq=fixture.commit()['sequence'];registry.reveal(seq,fixture.problem_id,'alice',str(fixture.submission),'first',2)
        self.assertEqual(registry.status()['problems'][fixture.problem_id]['status'],'ACCEPTED')
        replica=registry.replay(str(Path(fixture.scratch.name)/'copy'),verifier=verifier)
        self.assertEqual(calls,[[],[]]);self.assertEqual(registry.log.read_bytes(),replica.log.read_bytes())
        self.assertEqual(S.canonical(registry.status()),S.canonical(replica.status()))
        self.assertTrue(C.forbidden_scan([('proof',text)]))

    def test_recorded_policy_sha_is_verified_before_checker(self):
        fixture=Fixtures();fixture.setUp();self.addCleanup(fixture.doCleanups)
        registry=fixture.registry;_,state=registry._load();state['submission_policy']['sha256']='00'*32
        manifest=registry._capture(str(fixture.submission))
        with patch.object(registry,'verifier') as verifier:
            with self.assertRaisesRegex(ValueError,'policy'):registry._check(state,fixture.problem_id,manifest)
            verifier.assert_not_called()

    def test_rules_read_policy_fields(self):
        source='set_option trace.foo true\nexample «x\u2028y» : True := trivial\n'
        changed=copy.deepcopy(C.TEXT_POLICY);changed['output_options']['forbidden_roots']=[];changed['forbidden_name_characters']=[]
        with patch.object(C,'TEXT_POLICY',changed):self.assertEqual(C.forbidden_scan([('proof',source)]),[])

    def test_appended_print_line_is_outside_submission_scan(self):
        from zk_registry import lean_runner as L, sandbox as SB
        with tempfile.TemporaryDirectory() as root:
            env=L.LeanEnv('/unused',[],'v4.34.1',{},'',root)
            problem=dict(checker=dict(theorem_fqn='N.t',files=[],lean_opts=[]))
            with patch.object(L,'run_lean',return_value=L.RunResult(0,'',0,False)):
                result=C.compile_in_sandbox(env,problem,root,'theorem t : True := trivial\n',str(Path(root)/'work'),5,SB.Sandbox.fixture())
            written=Path(root,'work/proof/ProofMod.lean').read_text()
            self.assertIn('#print axioms N.t',written)
            self.assertTrue(C.forbidden_scan([('proof',written)]))
            self.assertEqual(result['n_errors'],0)
        import test_zk_registry_check as F
        from test_zk_registry_package import make_package
        with tempfile.TemporaryDirectory() as root:
            pkg,_=make_package(root);proof=Path(root)/'Proof.lean'
            proof.write_text(F.submission(Path(pkg,'Statement.lean').read_text(),'  trivial\n'))
            env=F.fake_env(root)
            with patch.object(L,'run_lean',return_value=L.RunResult(0,'',0,False)) as run:
                report=C.check(pkg,str(proof),env,keep=True,policy_fixture=env.pins())
            self.assertTrue(run.called)
            self.assertEqual(report['verdict'],'FAIL',report)
            self.assertTrue(any('output missing' in reason for reason in report['fail']))
            self.assertIn('#print axioms',Path(report['check_dir'],'proof/ProofMod.lean').read_text())


if __name__=='__main__':unittest.main()
