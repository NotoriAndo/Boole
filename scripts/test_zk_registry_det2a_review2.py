"""Named availability reasons and complete reference diagnostics."""
import errno
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace as NS
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
from zk_registry import check as C,lean_runner as L,closure as D,bounded_capture as B,gates as G,sandbox as SB,error_policy as E
import test_zk_registry_check as fixtures
from test_zk_registry_package import make_package

def assert_worker_envelopes(test,matrix):
    """Run the full product inside two real worker pipes with the selected guards."""
    from zk_registry import service_check as SC,correctness_runtime as CR
    original=CR.runner
    observations=[]
    worker_code = r"""
import json,sys
from pathlib import Path
from zk_registry import check as C
config=Path(sys.argv[1]);work=config.parent
cases=json.loads(Path(sys.argv[2]).read_text());results=[]
for case in cases:
    invalid=list(case['invalid']);fail=list(case['fail']);error=[]
    C.apply_compile_result(case['result'],dict(reference_type_sha256='reference'),invalid,fail,error)
    actual=C.verdict_of(dict(invalid=invalid,fail=fail,error=error))
    assert actual==case['expected'],(case,actual)
    if case['resource']:assert actual=='ERROR',(case,actual)
    results.append(dict(verdict=actual,invalid=invalid,fail=fail,error=error))
(work/'matrix-results.json').write_text(json.dumps(results))
(work/'verdict.json').write_text(json.dumps(dict(verdict='PASS',assurance='production')))
"""
    with tempfile.TemporaryDirectory() as root:
        fixture=Path(root)/'matrix.json';fixture.write_text(json.dumps(matrix))
        for attempt in [1,2]:
            envelope=E.envelope(attempt,dict(reason='resource-unavailable'))
            work=Path(root)/str(attempt);work.mkdir();seen=[]
            def factory(processes,wall):
                test.assertEqual(wall,envelope['wall_s'])
                run=original(processes,wall)
                def execute(argv,env,cwd,timeout,rss,*args):
                    test.assertEqual((timeout,rss),(envelope['wall_s'],envelope['rss_mb']))
                    config_path=Path(argv[argv.index('--config')+1]);config=json.loads(config_path.read_text())
                    test.assertEqual({k:config[k] for k in ('wall_s','rss_mb','memory_max')},{k:envelope[k] for k in ('wall_s','rss_mb','memory_max')})
                    seen.append((timeout,rss))
                    return run([sys.executable,'-c',worker_code,str(config_path),str(fixture)],env,cwd,timeout,rss,*args)
                return execute
            with patch.object(SC,'delegated_cgroup'),patch.object(CR,'runner',side_effect=factory):
                result=SC.ProductionVerifier(envelope)('fixture-package','fixture-submission',str(work))
            test.assertEqual(result['verdict'],'PASS',result);test.assertEqual(len(seen),1)
            rows=json.loads((work/'matrix-results.json').read_text());test.assertEqual(len(rows),len(matrix))
            for row,case in zip(rows,matrix):
                if case['resource']:test.assertEqual(row['verdict'],'ERROR')
            observations.append(rows)
    test.assertEqual(observations[0],observations[1])


class Review(unittest.TestCase):
    def test_named_reasons_through_public_checker(self):
        cases=[('sandbox-configuration:invalid-payload-memory','sandbox-configuration:invalid-payload-memory'),
               ('pristine import Model.lean does not compile','challenge-build-unavailable'),
               ('pristine copy Model.lean changed','environment-mismatch'),
               ('checker package changed during verification','checker_release_mismatch')]
        with tempfile.TemporaryDirectory() as tmp:
            pkg,_=make_package(tmp);proof=Path(tmp)/'Proof.lean';proof.write_text(fixtures.submission(Path(pkg,'Statement.lean').read_text(),'  trivial\n'));env=fixtures.fake_env(tmp)
            for raw,token in cases:
                with self.subTest(raw=raw):
                    with patch.object(C,'compile_in_sandbox',return_value=dict(error=raw)):
                        r=C.check(pkg,str(proof),env,policy_fixture=env.pins())
                    self.assertEqual((r['verdict'],r['reason'],r['error_group']),('ERROR',token,'D-node'))
                    if raw!=token:self.assertEqual(r['detail'],raw)
            for kind in [ValueError,KeyError,TypeError,RuntimeError]:
                with self.subTest(kind=kind):
                    with patch.object(C,'compile_in_sandbox',side_effect=kind('private detail')):
                        r=C.check(pkg,str(proof),env,policy_fixture=env.pins())
                    self.assertEqual(r['reason'],'checker-internal-error')

    def test_replay_io_error_is_transient(self):
        with patch.object(G,'open',side_effect=OSError(errno.EIO,'fixture I/O error')):
            r=G.parse_replay('/replay.out')
        self.assertNotIn('malformed',r);self.assertEqual(E.classify(r['error']),'T')
        f=fixtures.VerdictMappingTests();result=f.ok_compile(post=dict(f.ok_post(),**r))
        self.assertEqual(f.verdict(result),([],[],['checker-input-unavailable']))

    def test_noir_structured_callers(self):
        from zk_registry import noir_det as N
        with tempfile.TemporaryDirectory() as tmp:
            sh=NS(env=None,cfg=NS(battery_heartbeats=100,battery_file_wall=5,battery_single_wall=5));info=dict(n_conjuncts=0,bb=False,memory=False)
            for capped in [False,True]:
                messages=[dict(severity='information',data='FID middle REJECT\nFID-DONE')]
                result=L.RunResult(0,'retained',0,False,messages=messages,output_capped=capped)
                with patch.object(N.NE,'emit_fid_runner',return_value='source'),patch.object(L,'run_lean',return_value=result):
                    if capped:
                        with self.assertRaisesRegex(L.ResourceUnavailable,'output-cap'):N.lean_verdicts(sh,tmp,'N',info,[],None,tmp)
                    else:self.assertEqual(N.lean_verdicts(sh,tmp,'N',info,[],None,tmp)[0]['middle'],'REJECT')
                with patch.object(N.NE,'emit_battery_forms',return_value='source'),patch.object(L,'run_lean',return_value=result),patch.object(G,'classify_battery',return_value={}) as classify:
                    if capped:
                        with self.assertRaisesRegex(L.ResourceUnavailable,'output-cap'):N.g_triv(sh,tmp,'N',info,tmp)
                    else:N.g_triv(sh,tmp,'N',info,tmp);self.assertEqual(classify.call_args.args[1],messages)

    def test_correctness_battery_uses_complete_messages(self):
        from zk_registry import correctness_build as CB
        for capped in [False,True]:
            with tempfile.TemporaryDirectory() as tmp:
                messages=[dict(severity='error',data='tactic failed',pos=dict(line=1))]
                r=L.RunResult(1,'retained',0,False,messages=messages,output_capped=capped)
                with patch.object(CB.CG,'theorem_template',return_value='  sorry'),patch.object(CB.CG,'qualify_battery',return_value='trivial'),patch.object(L,'run_lean',return_value=r),patch.object(CB,'battery_completed',return_value=False) as completed:
                    if capped:
                        with self.assertRaisesRegex(L.ResourceUnavailable,'output-cap'):CB.battery(dict(id='N'),'soundness',CB.Path(tmp),CB.Path(tmp),'sha',None,CB.Path(tmp))
                        completed.assert_not_called()
                    else:
                        result=CB.battery(dict(id='N'),'soundness',CB.Path(tmp),CB.Path(tmp),'sha',None,CB.Path(tmp))
                        self.assertEqual(completed.call_args.args[1],messages)
                        attempt=result['attempts'][0];self.assertIn('messages_file',attempt)
                        self.assertEqual(json.loads(Path(tmp,attempt['messages_file']).read_text().splitlines()[0]),messages[0])
                        from zk_registry import package as P
                        self.assertEqual(P._battery_messages(Path(tmp),attempt,attempt['source_file'],attempt['stdout_file']),messages)
                        with self.assertRaises(ValueError):P._battery_messages(Path(tmp),dict(attempt,messages_sha256='00'*32),attempt['source_file'],attempt['stdout_file'])

    def test_legacy_message_file_ignores_stderr_retention(self):
        from zk_registry import package as P
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);message=dict(severity='error',data='tactic failed')
            (root/'proof.stdout').write_text(json.dumps(message)+'\n')
            attempt=dict(streams=dict(stderr=dict(truncated=True),stdout=dict(truncated=False)))
            self.assertEqual(P._battery_messages(root,attempt,'proof.lean','proof.stdout'),[message])
            attempt['streams']['stdout']['truncated']=True
            with self.assertRaises(ValueError):P._battery_messages(root,attempt,'proof.lean','proof.stdout')

    def test_solution_export_cap_never_calls_comparator(self):
        from zk_registry import axiom_policy as A
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);pkg,_=make_package(tmp);metadata=json.loads(Path(pkg,'problem.json').read_text());proof=root/'Proof.lean';proof.write_text(fixtures.submission(Path(pkg,'Statement.lean').read_text(),'  trivial\n'));env=fixtures.fake_env(tmp)
            binary=root/'exporter';binary.write_text('#!/usr/bin/python3\nimport os\nos.write(1,b"x"*8192)\nos.write(2,b"diagnostic")\n');binary.chmod(0o755)
            def compile(*args):
                Path(args[4],'proof').mkdir(parents=True);Path(args[4],'build').mkdir()
                return fixtures.VerdictMappingTests().ok_compile(post=fixtures.VerdictMappingTests().ok_post(type_sha256=metadata['checker']['reference_type_sha256']))
            tools=D.Tools('/usr/bin/true',str(binary),'/usr/bin/true','/usr',None)
            with patch.object(D,'prepare_reference',return_value=dict(verdict='PASS',path=str(root/'Challenge'),statement_closure_sha256=None)),patch.object(A,'resolve'),patch.object(A,'validate',return_value=dict(verdict='PASS',allowed_axioms=sorted(C.ALLOWED_AXIOMS))),patch.object(C,'compile_in_sandbox',side_effect=compile),patch.object(B,'caps',return_value=dict(B.caps(),export_bytes=1024)),patch.object(D,'compare_exports') as comparator:
                result=C.check(pkg,str(proof),env,policy_fixture=env.pins(),tools=tools,sandbox=SB.Sandbox.fixture())
            self.assertEqual((result['verdict'],result['stage'],result['reason']),('INVALID','export','resource-cap'))
            self.assertEqual(result['class'],'REJECT');comparator.assert_not_called()

if __name__=='__main__':unittest.main()
