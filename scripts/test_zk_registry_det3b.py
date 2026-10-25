"""Fuel-driver boundary controls; real kernel controls are run separately."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).parent))
from zk_registry import closure as C,lean_runner as L,gates as G
from zk_registry.sandbox import Sandbox


class FuelBoundary(unittest.TestCase):
    def test_declared_lean_fuel_reaches_the_trusted_kernel_driver(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);binary=root/'comparator'
            binary.write_text('''#!/usr/bin/python3
import json,os,sys
from pathlib import Path
config=json.loads(Path(sys.argv[1]).read_text())
cap=config.get('lean_kernel',{}).get('max_heartbeats')==1
events=([dict(stage='policy',reason='resource-cap')] if cap else
        [dict(stage=stage,result='accept') for stage in ('lean','nanoda','quotient')])
code=3 if cap else 0
events.append(dict(stage='final',rc=code))
Path(os.environ['COMPARATOR_STATUS_FILE']).write_text(''.join(json.dumps(dict(version=1,**event))+'\\n' for event in events))
sys.exit(code)
''');binary.chmod(0o755)
            challenge=root/'Challenge';solution=root/'Solution'
            challenge.write_bytes(b'fixture');solution.write_bytes(b'fixture')
            tools=C.Tools(str(binary),'/usr/bin/true','/usr/bin/true','/usr',None)
            caps=L.testnet_caps();caps['lean']['max_heartbeats']=1
            with patch.object(L,'testnet_caps',return_value=caps):
                result=C.compare_exports(str(challenge),str(solution),'T.t',tools,Sandbox.fixture(),
                    str(root/'work'),5,[])
            self.assertEqual((result['verdict'],result['reason']),('INVALID','resource-cap'))


class NativeReplayBoundary(unittest.TestCase):
    def test_canonical_driver_matches_the_isolated_artifact(self):
        from zk_registry.generate_fuel_replay import render
        path, expected = render()
        self.assertEqual(path.read_text(), expected)

    def test_only_the_pinned_native_driver_receives_replay_inputs(self):
        from zk_registry import artifact_pins
        import platform
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);driver=root/'zk_replay'
            driver.write_text('#!/bin/sh\nexit 0\n');driver.chmod(0o755)
            env=L.LeanEnv('/usr',[],'v4.34.1',{},'',str(root/'scratch'),
                          dict(kernel_replay_driver=str(driver)))
            authority=json.loads((Path(C.__file__).parent/'data/acceptance_tools_v2.json').read_text())
            expected=authority['binary_profiles']['Darwin-arm64']['lean_replay']['sha256']
            calls=[]
            def run(argv, task_env, cwd, wall, rss):
                calls.append((argv,task_env));return L.RunResult(0,'',0,False)
            args=['--tstack=262144','--run',G.GATE_REPLAY_TOOL,'M.olean','T.t','out','{}']
            with patch.object(platform,'system',return_value='Darwin'),patch.object(platform,'machine',return_value='arm64'),patch.object(artifact_pins,'digest',return_value=expected),patch.object(L,'run_process',side_effect=run):
                result=L.run_lean(env,args,str(root),5)
            self.assertEqual(result.rc,0)
            self.assertEqual(calls[0][0],[str(driver),'M.olean','T.t','out','{}'])
            self.assertEqual(calls[0][1]['LEAN_STACK_SIZE_KB'],'262144')
            with patch.object(artifact_pins,'digest',return_value='bad'),patch.object(L,'run_process') as launched:
                result=L.run_lean(env,args,str(root),5)
            self.assertEqual(result.rc,70);launched.assert_not_called()

    def test_other_trusted_run_keeps_its_existing_primitive_settings(self):
        with tempfile.TemporaryDirectory() as root:
            env=L.LeanEnv('/usr',[],'v4.34.1',{},'',root)
            calls=[]
            def run(argv, task_env, cwd, wall, rss):
                calls.append(task_env);return L.RunResult(0,'',0,False)
            with patch.object(L,'run_process',side_effect=run):
                result=L.run_lean(env,['--run','WitnessEval.lean','ZkReplay.lean'],root,5)
            self.assertEqual(result.rc,0)
            self.assertNotIn('LEAN_NAT_MAX_SIZE',calls[0])

    def test_missing_native_driver_is_unavailable(self):
        with tempfile.TemporaryDirectory() as root:
            env=L.LeanEnv('/usr',[],'v4.34.1',{},'',root)
            with patch.object(L,'run_process') as launched:
                result=L.run_lean(env,['--run',G.GATE_REPLAY_TOOL,'M.olean','T','out','{}'],root,5)
            self.assertEqual(result.rc,70);launched.assert_not_called()


class ComparatorTypedStatus(unittest.TestCase):
    def test_incomplete_boundary_is_an_availability_error(self):
        from zk_registry import correctness_runtime as CR
        for code in (0, 2, 3, 70):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary);left=root/'left';right=root/'right'
                left.write_text('sealed');right.write_text('sealed')
                result=L.RunResult(code,'',0,False,boundary={'observation':{}})
                with patch.object(CR,'runner',return_value=lambda *args:result):
                    row=C.compare_exports(str(left),str(right),'T.t',
                        C.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None),
                        Sandbox.fixture(),str(root/'work'),5,[])
                self.assertEqual((row['verdict'],row['reason']),('ERROR','sandbox-unavailable'))

    def compare(self, code, events):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);binary=root/'comparator'
            rows=[dict(version=1,**event) for event in events]+[dict(version=1,stage='final',rc=code)]
            binary.write_text('#!/usr/bin/python3\nimport os,sys\nfrom pathlib import Path\nPath(os.environ["COMPARATOR_STATUS_FILE"]).write_text('+repr(''.join(json.dumps(row)+'\n' for row in rows))+')\nsys.exit('+str(code)+')\n')
            binary.chmod(0o755)
            left=root/'left';right=root/'right';left.write_text('fixture');right.write_text('fixture')
            tools=C.Tools(str(binary),'/usr/bin/true','/usr/bin/true','/usr',None)
            return C.compare_exports(str(left),str(right),'T.t',tools,Sandbox.fixture(),str(root/'work'),5,[])

    def test_unavailable_requires_its_matching_trusted_error_record(self):
        for code,expected in [(70,'environment-tool-unavailable:replay'),(2,'sandbox-unavailable'),(0,'sandbox-unavailable')]:
            with self.subTest(code=code):
                row=self.compare(code,[dict(stage='error',reason='kernel-unavailable')])
                self.assertEqual((row['verdict'],row['reason']),('ERROR',expected))

    def test_transport_code_cannot_turn_a_negative_into_a_semantic_rejection(self):
        fuel=dict(stage='fuel',kernel='lean',debit=64,checked=1,calls=1,depth=1)
        negative=[dict(stage='nanoda',result='accept'),dict(stage='lean',result='reject')]
        for code,events,verdict,reason in [(2,negative,'ERROR','replay-unavailable'),
                (2,[fuel,*negative],'INVALID','kernel-disagreement'),
                (70,[fuel,*negative],'ERROR','sandbox-unavailable')]:
            with self.subTest(code=code,typed=fuel in events):
                row=self.compare(code,events)
                self.assertEqual((row['verdict'],row['reason']),(verdict,reason))

    def test_cap_code_requires_the_cap_record(self):
        for reason,expected in [('resource-cap','INVALID'),('disallowed-axiom','ERROR')]:
            with self.subTest(reason=reason):
                row=self.compare(3,[dict(stage='policy',reason=reason)])
                self.assertEqual(row['verdict'],expected)


if __name__=='__main__':unittest.main()
