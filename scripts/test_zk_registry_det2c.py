"""Registry retry, quarantine and legacy replay contracts without installed tools."""
import copy, json, sys, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
from zk_registry import service as S, error_policy as E
from test_zk_registry_service import offline_problem
import test_zk_registry_service as fixtures

class RegistryPolicy(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        package,problem=offline_problem(self.tmp.name);self.ident=problem['package_id']
        self.answer=Path(self.tmp.name)/'answer';self.answer.mkdir();(self.answer/'Solution.lean').write_text('synthetic checker boundary\n')
        self.now=1000;self.verdict=dict(verdict='ERROR',reason='checker-unavailable')
        self.calls=0
        def verifier(*args): self.calls+=1;return copy.deepcopy(self.verdict)
        self.verify=verifier
        self.catalog=S.build_catalog([package])
        self.registry=S.Registry.create(str(Path(self.tmp.name)/'store'),self.catalog,reveal_window=10,verifier=verifier)
        self.registry.clock=lambda:self.now
    def commit(self,solver='a',time=1):
        return self.registry.commit(self.ident,solver,S.commitment(self.ident,solver,S.submission_digest(str(self.answer)),solver),time)['sequence']
    def reveal(self,seq,solver='a',time=2):
        return self.registry.reveal(seq,self.ident,solver,str(self.answer),solver,time)
    def context(self,host='A',worker='W2',escalated=False):
        return dict(host=host,worker=worker,profile='production',canary=True,envelope='escalated' if escalated else 'standard')
    def error(self,reason='resource-unavailable',host='A',worker='W1',escalated=False,group='T'):
        return dict(verdict='ERROR',reason=reason,attempt=dict(self.context(host,worker,escalated),stage='compile',reason=reason,detail='wall',resource_events=['wall'],**{'class':group}))
    def state(self,seq):return self.registry.status()['commits'][str(seq)]
    def test_checking_never_expires_and_holds_earliest_position(self):
        first=self.commit();second=self.commit('b',2)
        self.assertEqual(self.reveal(first,time=10)['outcome'],'CHECKING')
        self.verdict=fixtures.IntakeTests.verify(None,None,None)
        self.assertEqual(self.reveal(second,'b',11)['outcome'],'PENDING')
        self.registry.advance(20);self.assertEqual(self.state(first)['status'],'CHECKING');self.assertIsNone(self.state(first)['deadline'])
        self.now+=60
        self.registry.recheck(first,21,attempt=self.context())
        self.assertEqual(self.registry.show(self.ident)['winner'],first)
        self.assertEqual(self.state(second)['status'],'DUPLICATE')
    def test_three_transient_attempts_one_host_never_reject(self):
        seq=self.commit();self.verdict=self.error();self.reveal(seq)
        self.verdict=self.error(worker='W2',escalated=True)
        self.now+=60;self.registry.recheck(seq,3,attempt=self.context(escalated=True))
        self.verdict=self.error(worker='W3')
        self.now+=3600;self.registry.recheck(seq,4,attempt=self.context(worker='W3'))
        self.assertEqual(self.state(seq)['status'],'CHECKING')
        self.assertEqual(len(self.state(seq)['attempts']),3)
        alerts=[json.loads(x) for x in (self.registry.root/'alerts.jsonl').read_text().splitlines()]
        self.assertTrue(any(x['kind']=='ladder-exhausted' for x in alerts))
    def quarantine(self):
        seq=self.commit();self.verdict=self.error();self.reveal(seq)
        self.verdict=self.error(host='B',worker='W2',escalated=True)
        self.now+=60;self.registry.recheck(seq,3,attempt=self.context('B',escalated=True))
        self.assertEqual(self.state(seq)['status'],'QUARANTINED');return seq
    def test_independent_quarantine_replays_logged_attempts_without_classification(self):
        self.quarantine()
        def forbidden(*_):raise AssertionError('nonterminal evidence must not be rejudged')
        with patch.object(E,'classify',side_effect=forbidden):
            replica=self.registry.replay(str(Path(self.tmp.name)/'replica'),verifier=forbidden)
            self.assertEqual(S.canonical(replica.status()),S.canonical(self.registry.status()))
            self.assertEqual(replica.log.read_bytes(),self.registry.log.read_bytes())
    def test_reveal_is_consumed_and_does_not_call_checker_again(self):
        seq=self.commit();self.reveal(seq);calls=self.calls
        self.assertEqual(self.reveal(seq,time=3)['reason'],'already-revealed');self.assertEqual(self.calls,calls)
    def test_terminal_output_cap_releases_barrier_but_reference_cap_defers(self):
        for stage,group,expected in [('solution','S','REJECTED'),('reference','D-node','CHECKING')]:
            with self.subTest(stage=stage):
                seq=self.commit(stage,1 if stage=='solution' else 3)
                self.verdict=dict(verdict='ERROR',reason='output-cap',attempt=dict(self.context(),stage=stage,reason='output-cap',**{'class':group}))
                self.assertEqual(self.reveal(seq,stage,2 if stage=='solution' else 4)['outcome'],expected)
    def test_replay_retries_availability_then_matches_and_reports_persistent_unavailability(self):
        seq=self.commit();self.verdict=fixtures.IntakeTests.verify(None,None,None);self.reveal(seq)
        calls=[]
        def once(*args):
            calls.append(1)
            return dict(verdict='ERROR',reason='resource-unavailable') if len(calls)==1 else fixtures.IntakeTests.verify(*args)
        with patch.object(S.time,'sleep'):
            replica=self.registry.replay(str(Path(self.tmp.name)/'retry-replica'),verifier=once)
            self.assertEqual(replica.log.read_bytes(),self.registry.log.read_bytes())
            with self.assertRaisesRegex(S.RegistryError,'replay-unavailable'):
                self.registry.replay(str(Path(self.tmp.name)/'unavailable'),verifier=lambda *_:dict(verdict='ERROR',reason='checker-unavailable'))
        self.assertEqual(len(calls),2)
    def test_rules_bytes_are_verified_on_every_load(self):
        altered=Path(self.tmp.name)/'rules.json';altered.write_text('{}')
        with patch.object(E,'POLICY_PATH',altered):
            with self.assertRaisesRegex(S.RegistryError,'error-policy-mismatch'):self.registry.status()
    def test_backoff_worker_health_and_one_escalation_precede_event(self):
        seq=self.commit();self.verdict=self.error();self.reveal(seq);original=self.registry.log.read_bytes()
        with self.assertRaisesRegex(S.RegistryError,'retry-backoff'):self.registry.recheck(seq,3,attempt=self.context(escalated=True))
        self.assertEqual(original,self.registry.log.read_bytes())
        self.now+=60
        with self.assertRaisesRegex(S.RegistryError,'retry-worker'):self.registry.recheck(seq,3,attempt=self.context(worker='W1',escalated=True))
        self.verdict=self.error(worker='W2',escalated=True)
        self.registry.recheck(seq,3,attempt=self.context(escalated=True));self.now+=3600
        with self.assertRaisesRegex(S.RegistryError,'retry-escalation'):self.registry.recheck(seq,4,attempt=self.context(worker='W3',escalated=True))
    def test_quarantine_requires_fixed_gate_before_void_and_replay_needs_gate(self):
        seq=self.quarantine()
        with self.assertRaisesRegex(S.RegistryError,'fixed-gate'):self.registry.void(seq,4)
        gate='ab'*32;self.registry.gates[gate]=self.verify;self.now+=3600
        self.verdict=self.error(host='C',worker='W3')
        self.registry.recheck(seq,4,gate=gate,attempt=self.context('C','W3'))
        self.assertEqual(self.state(seq)['status'],'QUARANTINED')
        self.registry.void(seq,5);self.assertEqual(self.state(seq)['status'],'VOID')
        with self.assertRaisesRegex(S.RegistryError,'replay-gate-unavailable'):
            self.registry.replay(str(Path(self.tmp.name)/'missing-gate'),verifier=self.verify)
        replica=self.registry.replay(str(Path(self.tmp.name)/'with-gate'),verifier=self.verify,gates={gate:self.verify})
        self.assertEqual(replica.log.read_bytes(),self.registry.log.read_bytes())
    def test_actual_failed_canary_remains_node_error(self):
        seq=self.commit();self.verdict=self.error();self.reveal(seq)
        self.verdict=self.error(host='B',worker='W2',escalated=True)
        self.verdict['attempt']['canary']=False
        self.now+=60
        self.registry.recheck(seq,3,attempt=self.context('B',escalated=True))
        self.assertEqual(self.state(seq)['attempts'][-1]['class'],'D-node')
        self.assertEqual(self.state(seq)['status'],'CHECKING')
    def test_terminal_replay_retries_reference_availability(self):
        seq=self.commit();self.verdict=dict(verdict='ERROR',reason='output-cap');self.reveal(seq)
        calls=[]
        def verifier(*_):
            calls.append(1)
            return dict(verdict='ERROR',reason='output-cap',attempt=dict(stage='reference' if len(calls)==1 else 'solution',**{'class':'D-node' if len(calls)==1 else 'S'}))
        with patch.object(S.time,'sleep'):
            replica=self.registry.replay(str(Path(self.tmp.name)/'terminal-retry'),verifier=verifier)
        self.assertEqual(len(calls),2)
        self.assertEqual(replica.log.read_bytes(),self.registry.log.read_bytes())
    def test_worker_payloads_are_cleaned_but_diagnostics_and_integrity_exports_remain(self):
        def verifier(package, submission, work):
            root=Path(work)
            for name in ['build/obj.olean','exports/solution.ndjson']:
                target=root/name;target.parent.mkdir(exist_ok=True);target.write_bytes(b'payload')
            (root/'worker.stdout').write_bytes(b'diagnostic')
            return copy.deepcopy(self.verdict)
        self.registry.verifier=verifier
        for reason,keep_export in [('checker-unavailable',False),('export-integrity',True)]:
            self.verdict=dict(verdict='ERROR',reason=reason)
            seq=self.commit(reason,1 if not keep_export else 3);self.reveal(seq,reason,2 if not keep_export else 4)
            work=sorted((self.registry.root/'checks').iterdir(),key=lambda p:p.stat().st_mtime_ns)[-1]
            self.assertFalse((work/'build/obj.olean').exists())
            self.assertEqual((work/'exports/solution.ndjson').exists(),keep_export)
            self.assertEqual((work/'worker.stdout').read_bytes(),b'diagnostic')
    def test_independent_component_failures_without_resource_events_quarantine(self):
        seq=self.commit();self.verdict=self.error(reason='silent-exit',group='I')
        self.verdict['attempt']['resource_events']=[];self.reveal(seq)
        self.verdict=self.error(reason='containment_killed',host='B',worker='W2',group='I')
        self.verdict['attempt']['resource_events']=[];self.now+=60
        self.registry.recheck(seq,3,attempt=self.context('B'))
        self.assertEqual(self.state(seq)['status'],'QUARANTINED')
    def test_void_releases_only_the_original_barrier(self):
        first=self.quarantine();second=self.commit('b',4)
        self.verdict=fixtures.IntakeTests.verify(None,None,None)
        self.assertEqual(self.reveal(second,'b',5)['outcome'],'PENDING')
        gate='cd'*32;self.registry.gates[gate]=self.verify;self.now+=3600
        self.verdict=self.error(host='C',worker='W3')
        self.registry.recheck(first,6,gate=gate,attempt=self.context('C','W3'))
        self.assertNotIn('winner',self.registry.show(self.ident))
        self.registry.void(first,7)
        self.assertEqual(self.registry.show(self.ident)['winner'],second)
        self.assertEqual(self.state(first)['cause'],'void')
    def test_production_resource_receipts_match_selected_retry_envelope(self):
        from zk_registry import service_check as SC
        self.registry.verifier=SC.ProductionVerifier(dict(worker_id='W1',host_id='A',canary=True))
        def error(verifier,*_):return verifier.error('resource-unavailable',stage='compile',profile='production')
        with patch.object(SC.ProductionVerifier,'__call__',error):
            seq=self.commit();self.reveal(seq);self.now+=60
            self.registry.recheck(seq,3,attempt=self.context('B',escalated=True))
        last=self.state(seq)['attempts'][-1]
        self.assertNotEqual(last['class'],'D-node')
        self.assertEqual(last['envelope']['wall_s'],3600)
        self.assertEqual(self.state(seq)['status'],'QUARANTINED')
    def test_retry_backoff_starts_after_the_prior_attempt_finishes(self):
        def slow(*_):
            self.now+=90
            return dict(verdict='ERROR',reason='checker-unavailable')
        self.registry.verifier=slow;seq=self.commit();self.reveal(seq)
        before=self.registry.log.read_bytes()
        with self.assertRaisesRegex(S.RegistryError,'retry-backoff'):
            self.registry.recheck(seq,3,attempt=self.context())
        self.assertEqual(before,self.registry.log.read_bytes())
    def test_legacy_store_replays_byte_for_byte(self):
        legacy=S.Registry.create(str(Path(self.tmp.name)/'legacy'),self.catalog,reveal_window=10,verifier=self.verify,error_policy=None)
        seq=legacy.commit(self.ident,'a',S.commitment(self.ident,'a',S.submission_digest(str(self.answer)),'a'),1)['sequence']
        legacy.reveal(seq,self.ident,'a',str(self.answer),'a',2)
        self.assertEqual(legacy.status()['commits'][str(seq)]['status'],'COMMITTED')
        replica=legacy.replay(str(Path(self.tmp.name)/'legacy-replica'),verifier=lambda *_:self.fail('legacy ERROR must be preserved'))
        self.assertEqual(replica.log.read_bytes(),legacy.log.read_bytes());self.assertEqual(replica.status(),legacy.status())

if __name__=='__main__':unittest.main()
