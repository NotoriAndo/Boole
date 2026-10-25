"""Review regressions at the compile consumer and frozen replay binding."""
from pathlib import Path
from unittest.mock import patch
import json
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parent))
from zk_registry import check as C, lean_runner as L, gates as G, sandbox as S
from zk_registry import correctness_build as B, correctness_gnark as CG
from test_zk_registry_package import make_package
from test_zk_registry_check import fake_env

LEGACY='93177ff1b7ca2bd1d9c7955067701cd314e768cd075a2438b2a5b70119cde5ee'


def compiler(root,kind=None,body='sys.exit(1)'):
    binary=Path(root)/'bin/lean';binary.parent.mkdir()
    message='' if kind is None else 'print('+repr(json.dumps(dict(severity='error',data='omega could not prove the goal',kind=kind)))+')\n'
    binary.write_text('#!/usr/bin/python3\nimport sys,time\n'+message+body+'\n');binary.chmod(0o755)
    return L.LeanEnv(root,[],'v4.34.1',{},'',root)


class CompileReviewTests(unittest.TestCase):
    def test_isolated_caps_match_the_gate_caps(self):
        root=Path(L.__file__).parent
        self.assertEqual((root/'data/testnet_caps_v2.json').read_bytes(),
                         (root/'compile_support/data/testnet_caps_v2.json').read_bytes())

    def test_untyped_host_failure_is_error_through_compile_consumer(self):
        with tempfile.TemporaryDirectory() as root:
            env=compiler(root);problem=dict(checker=dict(theorem_fqn='T.t',files=[],lean_opts=[]))
            record=C.compile_in_sandbox(env,problem,root,'theorem t : True := by trivial',str(Path(root)/'work'),10,S.Sandbox.fixture())
            invalid,fail,error=[],[],[];C.apply_compile_result(record,{},invalid,fail,error)
            report=C._finish(dict(compile=record,stage='compile',invalid=invalid,fail=fail,error=error))
            self.assertEqual((report['verdict'],report.get('error_group')),('ERROR','D-node'))
            self.assertEqual(error,['environment-tool-unavailable:compile'])

    def test_typed_compile_cap_and_fault_reach_verdict_consumer(self):
        with tempfile.TemporaryDirectory() as root:
            env=compiler(root,'runtime.maxRecDepth');problem=dict(checker=dict(theorem_fqn='T.t',files=[],lean_opts=[]))
            record=C.compile_in_sandbox(env,problem,root,'theorem t : True := by trivial',str(Path(root)/'cap'),10,S.Sandbox.fixture())
            invalid,fail,error=[],[],[];C.apply_compile_result(record,{},invalid,fail,error)
            self.assertEqual((record['compile_exit'],invalid,fail,error),(3,['resource-cap'],[],[]))
            # A typed driver fault is supplied at the process boundary, then follows
            # the same compile-record and verdict path as the real cap above.
            with patch.object(L,'run_lean',return_value=L.RunResult(4,'',0,False,tool_exit=4)):
                record=C.compile_in_sandbox(env,problem,root,'theorem t : True := by trivial',str(Path(root)/'fault'),10,S.Sandbox.fixture())
            invalid,fail,error=[],[],[];C.apply_compile_result(record,{},invalid,fail,error)
            self.assertEqual((record['compile_exit'],invalid,fail,error),(4,['tool-fault:compile'],[],[]))

    def test_non_gate_battery_preserves_native_exit_and_open_classification(self):
        with tempfile.TemporaryDirectory() as root:
            env=compiler(root,'tactic.failure')
            result=L.run_lean(env,['--json','Battery.lean'],root,5)
            completed=B.battery_completed(result,L.complete_messages(result))
            attempts=[dict(index=i,closed=False,completed=completed) for i in range(1,8)]
            self.assertEqual((result.rc,result.tool_exit,CG.classify('NONVACUOUS',attempts,None)),(1,None,'OPEN'))

    def test_non_gate_rss_guard_measures_the_lean_process(self):
        with tempfile.TemporaryDirectory() as root:
            env=compiler(root,body='payload=bytearray(100*1024*1024);time.sleep(4)')
            result=L.run_lean(env,['--json','Memory.lean'],root,10,rss_limit_mb=40)
            self.assertTrue(result.memkill)
            self.assertGreater(result.peak_rss_mb,40)

    def test_compile_grant_is_only_the_dedicated_support_directory(self):
        with tempfile.TemporaryDirectory() as root:
            env=compiler(root,body='sys.exit(0)');boundary=S.Sandbox.fixture()
            with patch.object(S.Sandbox,'command',autospec=True,side_effect=lambda _,cmd,*a,**kw:cmd) as call:
                with L.sandbox_context(boundary,[],[]):L.run_lean(env,['--json','M.lean'],root,5)
            readable=call.call_args.args[3]
            self.assertNotIn(str(Path(L.__file__).parent),readable)
            support=Path(call.call_args.args[1][3]).parent
            self.assertIn(str(support),readable)
            self.assertTrue((support/'bounded_capture.py').is_file())


class FrozenReplayTests(unittest.TestCase):
    def test_pre_branch_replay_pin_reaches_the_original_text_verdict(self):
        with tempfile.TemporaryDirectory() as root:
            pkg,problem=make_package(root);problem['checker']['replay_tool_sha256']=LEGACY
            Path(pkg,'problem.json').write_text(json.dumps(problem));proof=Path(root,'Proof.lean');proof.write_text(Path(pkg,'Statement.lean').read_text())
            env=fake_env(root);report=C.check(pkg,str(proof),env,policy_fixture=env.pins())
            self.assertEqual(report['verdict'],'INVALID',report)
            self.assertEqual(report['error'],[])


if __name__=='__main__':unittest.main()
