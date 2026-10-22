"""Capture, classification and replay rules, without an installed toolchain."""
import hashlib
import importlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import check as C, lean_runner as L, gates as G, correctness_runtime as CR
from zk_registry import closure as D, sandbox as SB
try:
    from zk_registry import bounded_capture as B
except ImportError:
    B = None
try:
    from zk_registry import error_policy as E
except ImportError:
    E = None


class CaptureRules(unittest.TestCase):
    def test_raw_guard_counts_hashes_and_retention_in_both_runners(self):
        for owned in [False, True]:
            with self.subTest(owned=owned), tempfile.TemporaryDirectory() as root:
                run = CR.runner(root) if owned else L.run_process
                with B.capture(stdout=B.RawSink(4096, 4096, 1024*1024)):
                    result = run([sys.executable, '-c', 'import os; os.write(1,b"x"*(32*1024*1024))'],
                                 dict(os.environ), root, 10, poll_s=.02)
                self.assertEqual(result.killed_by, 'output-cap:stdout')
                self.assertLessEqual(len(result.out.encode()), 8192)
                stream = result.streams['stdout']
                self.assertGreater(stream['bytes'], 1024*1024)
                self.assertEqual(stream['sha256'], hashlib.sha256(b'x'*stream['bytes']).hexdigest())

    def test_stderr_is_separate_bounded_and_does_not_change_stdout(self):
        with tempfile.TemporaryDirectory() as root:
            result = L.run_process([sys.executable, '-c',
                'import os; os.write(2,b"e"*(10*1024*1024)); os.write(1,b"{}\\n")'], dict(os.environ), root, 10)
            self.assertEqual((result.rc, result.out, result.killed_by), (0, '{}\n', ''))
            self.assertEqual(result.streams['stderr']['bytes'], 10*1024*1024)
            self.assertTrue(result.streams['stderr']['truncated'])
            self.assertLessEqual(len(result.stderr.encode()), 1024*1024)

    def test_file_sink_contains_only_stdout_and_measures_exact_bytes(self):
        with tempfile.TemporaryDirectory() as root:
            target=Path(root)/'sealed'
            with B.capture(stdout=B.FileSink(str(target), 1024)):
                r=L.run_process([sys.executable,'-c','import os; os.write(1,b"{\\"x\\":1}\\n"); os.write(2,b"diagnostic\\n")'],dict(os.environ),root,5)
            self.assertEqual(target.read_bytes(), b'{"x":1}\n')
            self.assertEqual(r.streams['stdout']['bytes'], 8)
            self.assertFalse(r.output_capped)
            self.assertEqual(r.out, '')

    def test_file_and_small_sinks_stop_at_their_caps(self):
        with tempfile.TemporaryDirectory() as root:
            for sink in [B.FileSink(str(Path(root)/'sealed'),1024),B.SmallSink(1024)]:
                with B.capture(stdout=sink):
                    r=L.run_process([sys.executable,'-c','import os; os.write(1,b"x"*4096)'],dict(os.environ),root,5)
                self.assertTrue(r.output_capped)
                self.assertEqual(r.killed_by,'output-cap:stdout')
            self.assertLessEqual(Path(root,'sealed').stat().st_size,1024)

    def test_message_measure_excludes_paths_and_order(self):
        rows=[dict(severity='error',data='λ',fileName='/a',kind='runtime.maxHeartbeats'),
              dict(severity='warning',data='uses sorry',fileName='/b')]
        measures=[]
        for ordered in [rows,list(reversed(rows))]:
            sink=B.MessageSink()
            for row in ordered:sink.feed((json.dumps(row)+'\n').encode())
            sink.finish()
            measures.append((sink.summary['message_count'],sink.summary['data_bytes']))
            self.assertEqual(sink.summary['n_errors'],1)
            self.assertEqual(sink.summary['sorry_warnings'],1)
        self.assertEqual(measures,[(2,12),(2,12)])

    def test_message_cap_and_exact_error_count(self):
        for count,expected in [(20000,True),(10,False)]:
            sink=B.MessageSink(message_cap=16384,data_cap=16*1024*1024)
            payload=(json.dumps(dict(severity='error',data='bad'))+'\n').encode()
            for _ in range(count):sink.feed(payload)
            sink.finish()
            self.assertEqual(sink.capped,expected)
            if not expected:self.assertEqual(sink.summary['n_errors'],10)
            self.assertLessEqual(len(sink.messages),10)

    def test_bounded_log_does_not_truncate_message_measures(self):
        sink=B.MessageSink()
        sink.feed((json.dumps(dict(severity='information',data='x'*(2*1024*1024)))+'\n').encode())
        sink.finish()
        self.assertEqual(sink.summary['data_bytes'],2*1024*1024)
        self.assertFalse(sink.receipt()['truncated'])
        self.assertTrue(sink.receipt()['log_truncated'])
        self.assertLessEqual(len(sink.retained),1024*1024)

    def test_print_message_binding_is_preserved_while_draining(self):
        sink=B.MessageSink(print_name='N.t',print_line=4)
        for data,line in [("'N.t' depends on axioms: [bad]",2),("'N.t' does not depend on any axioms",4)]:
            sink.feed((json.dumps(dict(severity='information',data=data,pos=dict(line=line)))+'\n').encode())
        sink.finish()
        self.assertEqual(C.parse_printed_axioms(sink.messages,'N.t',4),[])

    def test_read_errors_are_explicit(self):
        class Bad:
            def read(self,size):raise OSError('read failed')
            def close(self):pass
        sink=B.RawSink(4,4,None)
        B.drain(Bad(),sink)
        self.assertTrue(sink.capture_failed)

    def test_output_cap_keeps_its_reason_at_every_boundary(self):
        result=L.RunResult(125,'',0,False,killed_by='output-cap:stdout')
        self.assertEqual(D.resource_reason(result),'output-cap')
        self.assertEqual(SB.boundary_failure(result)['reason'],'output-cap')
        with self.assertRaisesRegex(L.ResourceUnavailable,'^output-cap$'):L.enforce_boundary(result)

    def test_capture_failure_is_not_a_resource_limit(self):
        r=L.RunResult(125,'',0,False,killed_by='capture-failed:stderr')
        self.assertEqual(D.resource_reason(r),'capture-failed')


class StageCaptureRules(unittest.TestCase):
    def test_export_cap_prevents_identity(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);script=root/'exporter'
            script.write_text('#!/usr/bin/python3\nimport os\nos.write(1,b"x"*8192)\nos.write(2,b"diagnostic\\n")\n');script.chmod(0o755)
            env=L.LeanEnv('/usr',[],'v4.34.1',{},'',str(root/'environment'))
            tools=D.Tools('/usr/bin/true',str(script),'/usr/bin/true','/usr',None)
            real_caps=B.caps()
            with patch.object(B,'caps',return_value=dict(real_caps,export_bytes=1024)),patch.object(D,'statement_identity') as identity:
                result=D.export_module(env,'ProofMod','T.t',str(root/'sealed/file'),tools,SB.Sandbox.fixture(),str(root),[],5,[],identity=True)
            self.assertEqual((result['verdict'],result['reason']),('ERROR','output-cap'))
            identity.assert_not_called()

    def test_oversized_worker_verdict_is_node_error(self):
        from zk_registry import service_check as SC
        with tempfile.TemporaryDirectory() as root:
            def worker(*args):
                (Path(root)/'verdict.json').write_text(json.dumps(dict(verdict='PASS',assurance='production',padding='x'*70000)))
                return L.RunResult(0,'',0,False)
            with patch.object(SC,'delegated_cgroup'),patch.object(CR,'runner',return_value=worker):
                result=SC.ProductionVerifier({})('package','submission',root)
            self.assertEqual((result['verdict'],result['reason']),('ERROR','checker-worker-error'))
            self.assertEqual(result['attempt']['detail'],'verdict-oversized')
            self.assertEqual(result['attempt']['class'],'D-node')

    def test_worker_internal_exception_always_has_an_error_report(self):
        from zk_registry import service_check as SC
        with tempfile.TemporaryDirectory() as root:
            config=Path(root)/'config.json';config.write_text('{}')
            with patch.object(SC,'run',side_effect=IndexError('internal')):
                result=SC.main(['--config',str(config),'--package','pkg','--submission','submission','--work',root])
            self.assertEqual(result,3)
            self.assertEqual(json.loads((Path(root)/'verdict.json').read_text())['reason'],'checker-internal-error')


class MessageIntegrationRules(unittest.TestCase):
    def test_compile_counts_all_errors_and_never_uses_capped_findings(self):
        for count in [10,20000]:
            with self.subTest(count=count),tempfile.TemporaryDirectory() as root:
                binary=Path(root)/'bin/lean';binary.parent.mkdir()
                binary.write_text('#!/usr/bin/python3\nimport json,sys\nfor _ in range('+str(count)+'): print(json.dumps(dict(severity="error",data="bad")))\nsys.exit(1)\n');binary.chmod(0o755)
                env=L.LeanEnv(root,[],'v4.34.1',{},'',root)
                problem=dict(checker=dict(theorem_fqn='T.t',files=[],lean_opts=[]))
                r=C.compile_in_sandbox(env,problem,root,'theorem t : True := by trivial',str(Path(root)/'work'),10,SB.Sandbox.fixture())
                invalid,fail,error=[],[],[]
                C.apply_compile_result(r,{},invalid,fail,error)
                if count==10:
                    self.assertEqual(r['n_errors'],10)
                    self.assertTrue(fail)
                else:
                    self.assertEqual(error,['output-cap'])
                    self.assertEqual((invalid,fail),([],[]))

    def test_raw_guard_requires_a_healthy_production_boundary(self):
        with tempfile.TemporaryDirectory() as root:
            for healthy in [False,True]:
                status=Path(root)/('status-'+str(healthy))
                if healthy:status.write_text(json.dumps(dict(protocol=1,assurance='production',descriptor_sha256='authority',verdict='PASS',observation=dict(setup_failed=False))))
                SB._PENDING[str(status)]=('authority',{},'production',{}, {})
                r=L.RunResult(-9,'',0,False,killed_by='output-cap:stderr',output_capped=True,streams=dict(stderr=dict(kind='raw')))
                r=SB.capture_result(['launcher','--gate-status',str(status)],r)
                self.assertEqual(D.resource_reason(r),'output-cap' if healthy else 'resource-unavailable')

    def test_reference_cap_is_an_error_and_battery_cap_is_not_pass(self):
        r=L.RunResult(125,'',0,False,killed_by='output-cap:stdout',output_capped=True)
        for second in [False,True]:
            values=[(L.RunResult(0,'',0,False),[]),(r,[])] if second else [(r,[])]
            with tempfile.TemporaryDirectory() as root,patch.object(L,'compile_module',side_effect=values):
                result=G.g_elab(None,root,root,'N',root)
            self.assertEqual(result.status,'ERROR')
            self.assertEqual(result.detail['reason'],'output-cap')
        result=G._triv_gate({},[dict(error='output-cap')],{},'fixture')
        self.assertEqual(result.status,'ERROR')


class ArtifactRules(unittest.TestCase):
    def test_attempt_cleanup_keeps_logs_and_honors_keep_and_integrity(self):
        for keep,integrity in [(False,False),(False,True),(True,False)]:
            with self.subTest(keep=keep,integrity=integrity),tempfile.TemporaryDirectory() as root:
                build=Path(root)/'build';sealed=Path(root)/'sealed'
                def attempt(*args,**kwargs):
                    (build/'proof').mkdir(parents=True);sealed.mkdir()
                    (build/'proof/compile.log').write_text('diagnostic')
                    (build/'proof/ProofMod.olean').write_bytes(b'compiled')
                    (sealed/'Solution.ndjson').write_bytes(b'export')
                    C._record_artifact(str(build),'build');C._record_artifact(str(sealed),'export')
                    return dict(verdict='ERROR',error=['export-integrity' if integrity else 'resource-unavailable'])
                with patch.object(C,'_check',side_effect=attempt,create=True):
                    result=C.check('package','proof',None,keep=keep)
                self.assertEqual(build.exists(),keep)
                self.assertEqual(sealed.exists(),keep or integrity)
                if not keep:
                    logs=Path(result['diagnostics_dir'])
                    self.assertTrue(any(p.read_text()=='diagnostic' for p in logs.glob('*.log')))
                    self.assertIn('artifact_digests',result)


class ReplayRules(unittest.TestCase):
    def parse(self,raw):
        with tempfile.TemporaryDirectory() as root:
            p=Path(root)/'replay.out';p.write_bytes(raw);return G.parse_replay(str(p))

    def test_exact_record_grammar(self):
        bad=[b'CONST\tx\n',b'AXIOM\n',b'UNKNOWN\tx\n',b'REPLAY\tbad\n',b'TARGET\ttheorem\n',
             b'CONST\tx\ttheorem\tyes\tfalse\n',b'TARGET-MISSING\tx\n',b'AXIOM\tx\textra\n',b'REPLAY\tok']
        for raw in bad:
            with self.subTest(raw=raw):
                r=self.parse(raw)
                self.assertNotEqual(r['replay'],'ok')
                self.assertIn('malformed',r)

    def test_unicode_separators_remain_inside_fields(self):
        r=self.parse('CONST\ta\u2028b\ttheorem\tfalse\tfalse\nREPLAY\tok\nTYPE\tx\ty\n'.encode())
        self.assertEqual(r['consts'][0]['name'],'a\u2028b')
        self.assertEqual(r['type_str'],'x\ty')
        self.assertEqual(r['replay'],'ok')

    def test_replay_size_cap_is_checked_before_reading(self):
        with tempfile.TemporaryDirectory() as root:
            p=Path(root)/'replay.out'
            with p.open('wb') as f:f.truncate(130*1024*1024)
            self.assertEqual(G.parse_replay(str(p))['error'],'output-cap')

    def test_usage_and_unexpected_errors_exit_three_and_write_report(self):
        with tempfile.TemporaryDirectory() as root:
            report=Path(root)/'report.json'
            self.assertEqual(C.main(['--report',str(report)]),3)
            self.assertEqual(json.loads(report.read_text())['verdict'],'ERROR')
            with patch.object(L,'load_env',side_effect=IndexError('internal')):
                self.assertEqual(C.main(['pkg','proof','--lean-env','env','--report',str(report)]),3)
            self.assertEqual(json.loads(report.read_text())['reason'],'checker-internal-error')

    def test_silent_exit_has_distinct_reason(self):
        invalid,fail,error=[],[],[]
        C.apply_compile_result(dict(compile_rc=1,n_errors=0),{},invalid,fail,error)
        self.assertEqual(error,['silent-exit'])


class ClassificationRules(unittest.TestCase):
    def test_reason_groups_and_default(self):
        expected={'resource-unavailable':'T','sandbox-unavailable':'T','capture-failed':'T',
            'checker-internal-error':'T','environment-retired':'D-node','export-integrity':'D-node',
            'checker_release_mismatch':'D-node','replay-unavailable':'D-input','export-unavailable':'I',
            'silent-exit':'I','primary_checker_unavailable':'I','output-cap':'S','unknown':'T'}
        for reason,group in expected.items():
            with self.subTest(reason=reason):self.assertEqual(E.classify(reason,'compile',''),group)
        self.assertEqual(E.classify('output-cap','reference:export',''),'D-node')
        self.assertEqual(E.classify('replay-unavailable','replay','malformed-record'),'I')
        self.assertEqual(E.classify('checker-worker-error','worker','verdict-oversized'),'D-node')
        self.assertEqual(E.classify('sandbox-unavailable','comparator','silent-exit:101'),'T')
        for reason in ['pin-mismatch','replay-tool-mismatch','checker package changed']:
            self.assertEqual(E.classify(reason,'worker',''),'D-node')

    def test_retry_envelopes_change_only_resources(self):
        first=dict(reason='resource-unavailable',resource_events=['wall'])
        a=E.envelope(1,first);b=E.envelope(2,first)
        self.assertEqual((a['wall_s'],b['wall_s']),(1800,3600))
        self.assertEqual({k for k in a if a[k]!=b[k]}, {'wall_s','rss_mb','memory_max'})
        self.assertEqual(a['output_caps'],b['output_caps'])

    def test_quarantine_needs_two_healthy_independent_hosts(self):
        a=dict(index=1,host='A',worker='a',stage='export',reason='export-unavailable',**{'class':'I'},canary=True,envelope='standard',resource_events=[],profile='production')
        b=dict(a,index=2,host='B',worker='b',envelope='escalated')
        self.assertFalse(E.quarantine_trigger([a,dict(b,host='A')]))
        self.assertTrue(E.quarantine_trigger([a,b]))
        self.assertFalse(E.quarantine_trigger([a,dict(b,canary=False)]))
        self.assertFalse(E.quarantine_trigger([dict(a,reason='environment-mismatch',**{'class':'D-node'}),dict(b,reason='environment-mismatch',**{'class':'D-node'})]))


if __name__=='__main__':unittest.main()
