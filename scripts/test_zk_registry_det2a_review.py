"""Regression checks for complete stage output and deterministic availability."""
import io
import itertools
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace as NS
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import bounded_capture as B, lean_runner as L, check as C, gates as G
from zk_registry import closure as D, sandbox as SB, correctness_runtime as CR
from zk_registry import error_policy as E
from test_zk_registry_check import VerdictMappingTests, fake_env, submission
from test_zk_registry_package import make_package


class Review(unittest.TestCase):
    def test_reason_precedes_comparator_detail(self):
        for reason, group in dict.fromkeys(['resource-unavailable','sandbox-unavailable','checker-worker-error','checker-unavailable','acceptance-unavailable','checker-input-unavailable','checker-internal-error','capture-failed','capture_failed'],'T').items() | dict.fromkeys(['kernels-unavailable','environment-retired','environment-mismatch','environment-unpinned','export-integrity','verdict-oversized','trusted_helper_compile_unavailable','checker_release_mismatch','invalid-package','closure-unavailable','nonproduction-evidence','replay-gate-unavailable'],'D-node').items() | dict.fromkeys(['export-unavailable','silent-exit','primary_checker_unavailable','axiom_audit_unavailable','containment_killed','comparator-unavailable'],'I').items() | {'replay-unavailable':'D-input','output-cap':'S'}.items():
            self.assertEqual(E.classify(reason, 'comparator', 'silent-exit:1'), group)
        for stage in ['reference:replay', 'reference:export', 'challenge']:
            self.assertEqual(E.classify('output-cap', stage), 'D-node')

    def test_comparator_faults(self):
        for rc, events, changed, expected in [(1, {}, False, 'I'), (70, {}, False, 'T'),
                (1, {'oom_kill_events':1}, False, 'T'), (1, {'pids_max_events':1}, False, 'T'),
                (1, {}, True, 'D-node')]:
            with self.subTest(rc=rc,events=events,changed=changed), tempfile.TemporaryDirectory() as tmp:
                p=Path(tmp); a=p/'a'; b=p/'b';a.write_text('sealed');b.write_text('sealed')
                def run(*args):
                    if changed:b.write_text('changed')
                    return L.RunResult(rc,'',0,False,boundary={'observation':events})
                with patch.object(CR,'runner',return_value=run):
                    r=D.compare_exports(str(a),str(b),'N.t',D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None),SB.Sandbox.fixture(),str(p/'run'),5,[])
                self.assertEqual(E.classify(r['reason'],r['stage'],r['detail']),expected)
                if expected!='I':self.assertFalse(r['detail'].startswith('silent-exit:'))

    def test_final_unterminated_line_sets_stop(self):
        sink=B.MessageSink(data_cap=4); stops=[]
        B.drain(io.BytesIO(b'{"severity":"information","data":"12345"}'),sink,stops.append)
        self.assertEqual(stops,['output-cap:stdout'])
        with tempfile.TemporaryDirectory() as tmp,B.capture(stdout=B.MessageSink(data_cap=4)):
            result=L.run_process([sys.executable,"-c","import os;os.write(1,"+repr(b'{"data":"12345"}')+")"],dict(os.environ),tmp,5)
        self.assertEqual(result.killed_by,'output-cap:stdout');self.assertTrue(result.output_capped)

    def test_partial_line_and_data_bounds(self):
        sink=B.MessageSink();limit=96*1024*1024+4608
        for _ in range(limit//65536):sink.feed(b'x'*65536)
        sink.feed(b'x'*(limit%65536));self.assertFalse(sink.capped)
        sink.feed(b'x');self.assertTrue(sink.capped);self.assertEqual(sink.partial,b'')
        sink=B.MessageSink();sink.feed(json.dumps({'data':'x'*(16*1024*1024)}).encode()+b'\n')
        self.assertFalse(sink.capped)
        sink.feed(b'{"data":"x"}\n');self.assertTrue(sink.capped)

    def test_malformed_position_is_message_not_capture_failure(self):
        sink=B.MessageSink(retain_all=True)
        B.drain(io.BytesIO(b'{"severity":"information","data":"x","pos":17}\n'),sink)
        self.assertFalse(sink.capture_failed)
        self.assertEqual(L.errors(sink.messages)[0]['data'],'malformed Lean message')

    def test_compile_raw_line_still_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);lean=p/'bin/lean';lean.parent.mkdir();lean.write_text('#!/usr/bin/python3\nprint("raw diagnostic")\n');lean.chmod(0o755)
            env=L.LeanEnv(tmp,[],'v4.34.1',{},'',tmp)
            r,msgs=L.compile_module(env,tmp,'Model.lean',str(p/'build'),[],5)
            self.assertEqual(r.rc,0);self.assertTrue(L.errors(msgs))
            gate=G.g_elab(env,tmp,str(p/'build2'),'N',str(p/'gate'))
            self.assertEqual(gate.status,'FAIL')

    def test_replay_symlinks_have_same_malformed_outcome(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);results=[]
            for size in [1,130*1024*1024]:
                f=p/str(size)
                with f.open('wb') as stream:stream.truncate(size)
                link=p/('link'+str(size));link.symlink_to(f)
                results.append(G.parse_replay(str(link)))
            self.assertEqual(results[0],results[1]);self.assertIn('malformed',results[0])

    def test_export_seals_stdout_separately_from_stderr(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);binary=p/'exporter';binary.write_text('#!/usr/bin/python3\nimport os\nos.write(1,b"sealed stdout")\nos.write(2,b"diagnostic")\n');binary.chmod(0o755)
            result=D.export_module(L.LeanEnv('/usr',[],'v4.34.1',{},'',tmp),'M','N.t',str(p/'sealed/out'),D.Tools('/usr/bin/true',str(binary),'/usr/bin/true','/usr',None),SB.Sandbox.fixture(),tmp,[],5,[])
            self.assertEqual(result['verdict'],'PASS');self.assertEqual(Path(result['path']).read_bytes(),b'sealed stdout')

    def test_export_fallback_cap_and_reference_cap(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);env=L.LeanEnv('/usr',[],'v4.34.1',{},'',tmp)
            caps=dict(B.caps(),export_bytes=4)
            with patch.object(B,'caps',return_value=caps),patch.object(CR,'runner',return_value=lambda *a:L.RunResult(0,'12345',0,False)),patch.object(D,'statement_identity') as identity:
                r=D.export_module(env,'M','N.t',str(p/'sealed/out'),D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None),SB.Sandbox.fixture(),tmp,[],5,[],identity=True)
            self.assertEqual(r['reason'],'statement-resource-cap');identity.assert_not_called()
            package=p/'package';package.mkdir();(package/'Statement.lean').write_text('statement')
            compiled=dict(compile_rc=0,n_errors=0,post=dict(replay=None,killed_by='output-cap:stdout',error='output-cap'))
            with patch.object(C,'compile_in_sandbox',return_value=compiled):
                r=D.prepare_reference(env,dict(checker=dict(statement_file='Statement.lean')),str(package),str(p/'work'),str(p/'seal'),None,SB.Sandbox.fixture(),5,[])
            self.assertEqual(r['reason'],'output-cap')
            self.assertEqual(E.classify(r['reason'],'reference:replay'),'D-node')

    def test_capture_failure_precedence(self):
        f=VerdictMappingTests()
        for where,cap in itertools.product(['compile','post'],[False,True]):
            r=f.ok_compile(printed_axioms=['sorryAx'])
            if where=='compile':r['compile_killed_by']='capture-failed:stdout'
            else:r['post']['killed_by']='capture-failed:stderr'
            r['output_capped']=cap
            self.assertEqual(f.verdict(r),([],[],['output-cap' if cap else 'capture-failed']))

    def test_wrapper_input_and_compile_exception(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg,_=make_package(tmp);proof=Path(tmp)/'Proof.lean'
            proof.write_text(submission(Path(pkg,'Statement.lean').read_text(),'  trivial\n'))
            env=fake_env(tmp)
            for exc,reason in [(OSError('private path'),'checker-input-unavailable'),(IndexError('private text'),'checker-internal-error')]:
                with patch.object(C,'compile_in_sandbox',side_effect=exc):
                    r=C.check(pkg,str(proof),env,policy_fixture=env.pins())
                self.assertEqual(r['reason'],reason)
            r=C._finish(dict(error=['cannot read problem.json: /private/file'],invalid=[],fail=[]))
            self.assertEqual(r['reason'],'checker-input-unavailable');self.assertIn('/private/file',r['detail'])

    def test_worker_config_validation(self):
        from zk_registry.service_check import ProductionVerifier
        for field,values in [('wall_s',[0,True,1801]),('memory_max',[0,True,11534336001]),('host_id',['',None,1]),('worker_id',['',None,1])]:
            for value in values:
                with self.subTest(field=field,value=value),self.assertRaises(ValueError):ProductionVerifier({field:value})
        for wall,memory in [(1800,11534336000),(3600,19966445568)]:ProductionVerifier(dict(wall_s=wall,memory_max=memory))

    def test_structured_consumers_use_complete_messages(self):
        from zk_registry import halo2_det as H, gnark_det as K, air_det as A, circom_detmod as M
        with tempfile.TemporaryDirectory() as tmp:
            sh=NS(env=None);model=NS(r=NS(n_constraints=0),group_wires=lambda:[])
            calls=[(H.lean_eval,lambda:H.lean_eval(sh,tmp,'N',{},[],tmp),H.HE,'emit_fid_runner'),
                   (K.lean_eval,lambda:K.lean_eval(sh,tmp,'N',model,[],[],tmp),K.GE,'emit_fid_runner'),
                   (A.lean_eval,lambda:A.lean_eval(None,tmp,'text',tmp,'Eval'),None,None),
                   (M.lean_verdicts,lambda:M.lean_verdicts(None,tmp,'N',0,[],tmp),M,'emit_runner')]
            for fn,call,owner,emitter in calls:
                for capped in [False,True]:
                    with self.subTest(fn=fn.__module__,cap=capped):
                        r=L.RunResult(0,'retained window',0,False,output_capped=capped,killed_by='output-cap:stdout' if capped else '',messages=[dict(severity='information',data='FID middle REJECT\nFID-DONE')])
                        with patch.object(L,'run_lean',return_value=r):
                            from contextlib import nullcontext
                            with (patch.object(owner,emitter,return_value='text') if owner else nullcontext()):
                                if capped:
                                    with self.assertRaisesRegex(L.ResourceUnavailable,'^output-cap$'):call()
                                else:
                                    result=call()[0]
                                    if isinstance(result,dict):self.assertEqual(result['middle'],'REJECT')
                                    else:self.assertIn('FID middle REJECT',result)


    def test_batteries_and_air_kernel_use_complete_findings(self):
        from zk_registry import halo2_det as H, gnark_det as K, air_det as A
        from contextlib import ExitStack
        with tempfile.TemporaryDirectory() as tmp:
            cfg=NS(battery_heartbeats=100,battery_file_wall=10,battery_single_wall=10)
            sh=NS(env=None,cfg=cfg);model=NS(groups=[],r=NS(n_constraints=0))
            for module,call in [(H,lambda:H.g_triv(sh,tmp,'N',{},tmp)),(K,lambda:K.g_triv(sh,tmp,'N',model,tmp))]:
                for capped in [False,True]:
                    message=dict(severity='error',data='tactic failed',pos=dict(line=2))
                    r=L.RunResult(1,'retained',0,False,messages=[message],output_capped=capped)
                    with self.subTest(module=module.__name__,cap=capped),ExitStack() as stack:
                        stack.enter_context(patch.object(L,'run_lean',return_value=r))
                        stack.enter_context(patch.object(module.HE if module is H else module.GE,'emit_battery_forms',return_value='example : True := by\n trivial'))
                        classifier=stack.enter_context(patch.object(G,'classify_battery',return_value={}))
                        if capped:
                            with self.assertRaisesRegex(L.ResourceUnavailable,'output-cap'):call()
                            classifier.assert_not_called()
                        else:
                            call();self.assertTrue(classifier.called)
                            self.assertEqual(classifier.call_args.args[1],[message])
            for capped in [False,True]:
                r=L.RunResult(0,'',0,False,messages=[dict(severity='error',data='kernel failed',pos=dict(line=2))],output_capped=capped)
                with patch.object(L,'run_lean',return_value=r),patch.object(A.AL,'emit_kernel_checks',return_value=('source',{})):
                    if capped:
                        with self.assertRaisesRegex(L.ResourceUnavailable,'output-cap'):A.confirm_in_lean(sh,'N',{},tmp,[],[],tmp,kernel=True)
                        with self.assertRaisesRegex(L.ResourceUnavailable,'output-cap'):A.kernel_verdicts(sh,None,None,None,{},'N',tmp,[],tmp)
                    else:
                        self.assertFalse(A.confirm_in_lean(sh,'N',{},tmp,[],[],tmp,kernel=True)['lean_confirms'])
                        self.assertIn('kernel failed',A.kernel_verdicts(sh,None,None,None,{},'N',tmp,[],tmp)[3])

    def test_circom_stderr_and_guard(self):
        from zk_registry import circom_det as M
        from contextlib import ExitStack
        with tempfile.TemporaryDirectory() as tmp:
            cfg=NS(repo_dir=tmp,node='/usr/bin/true',compile_timeout_s=5,compile_rss_mb=100,compile_output_mb=1)
            sh=NS(cfg=cfg,files=[]);compiler=NS(kind='circom2',tag='fixture',command=lambda _:['/usr/bin/true'])
            for guard in ['', 'output-cap:stderr']:
                r=L.RunResult(1,'',0,False,stderr='error: expected diagnostic',killed_by=guard)
                with ExitStack() as stack:
                    for name,val in [('build_options',(None,[],None)),('main_free_copy',None),('uses_custom_templates_pragma',False)]:stack.enter_context(patch.object(M,name,return_value=val))
                    stack.enter_context(patch.object(M.I,'main_source',return_value='source'))
                    stack.enter_context(patch.object(L,'run_process',return_value=r))
                    result=M.compile_main(sh,tmp,'lib.circom','T',(),False,compiler=compiler)
                self.assertIn('stopped by the resource guard' if guard else 'expected diagnostic',result['error'])

    def test_noir_stderr_error(self):
        from zk_registry import noir_det as N
        with tempfile.TemporaryDirectory() as tmp:
            sh=NS(cfg=NS(compile_timeout=5),tc=NS(run_nargo=lambda *a:L.RunResult(1,'stdout only',0,False,stderr='error: expected diagnostic')))
            target=NS(crate='',crate_type='bin')
            with patch.object(N,'copy_crate'),patch.object(N,'scrub_text',side_effect=lambda s,t:t),patch.object(N,'dependency_note',side_effect=lambda t,e:e):
                r=N.compile_candidate(sh,target,'bin-main',{},tmp,'test')
            self.assertEqual(r.error,'expected diagnostic')

    def test_evaluation_never_parses_truncated_rows(self):
        from zk_registry import correctness_build as CB
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);w=p/'w';w.write_text('witness');(p/'Evaluation.lean').write_text('source');(p/'EvaluationSupport.lean').write_text('source')
            row=json.dumps(dict(witness=str(w),Constraints=True))+'\n'
            r=L.RunResult(0,row,0,False,streams={'stdout':{'truncated':True}})
            with patch.object(L,'compile_module',return_value=(L.RunResult(0,'',0,False),[])),patch.object(L,'run_lean',return_value=r),patch.object(CB.S,'package_digest',return_value='digest'):
                result=CB.evaluate({},CB.Path(tmp),CB.Path(tmp),[w],None,5)
            self.assertFalse(result['completed']);self.assertEqual(result['rows'],[])

    def test_reference_cap_never_calls_comparator(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg,_=make_package(tmp);proof=Path(tmp)/'Proof.lean';proof.write_text(submission(Path(pkg,'Statement.lean').read_text(),'  trivial\n'))
            env=fake_env(tmp)
            with patch.object(D,'prepare_reference',return_value=dict(verdict='ERROR',reason='output-cap')),patch.object(D,'compare_exports') as comparator:
                result=C.check(pkg,str(proof),env,policy_fixture=env.pins(),tools=object())
            comparator.assert_not_called();self.assertEqual((result['reason'],result['error_group']),('output-cap','D-node'))

if __name__=='__main__':unittest.main()
