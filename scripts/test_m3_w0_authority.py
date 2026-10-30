"""Host controls only. No VM or Linux execution."""
import importlib.util,json,os,tempfile,time,unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

def load(name):
    p=Path(__file__).parent/'devnet/w0'/f'{name}.py'
    spec=importlib.util.spec_from_file_location('w0_'+name,p)
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod
window=load('window');runner=load('runner')
CONFIG_SAFE=dict(ssh=dict(forwardAgent=False,forwardX11=False,forwardX11Trusted=False),copyToHost=[],portForwards=[dict(guestIPMustBeZero=False,guestIP='127.0.0.1',guestPortRange=[1,65535],hostIP='127.0.0.1',hostPortRange=[1,65535],proto='any',ignore=True)],propagateProxyEnv=False,networks=[])

class AuthorityTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.work=self.root/'work';self.work.mkdir(mode=0o700);(self.work/'w0').mkdir(mode=0o700)
        now=int(time.time());self.g=dict(state='granted',owner='N',step='W0',start_unix=now,end_unix=now+2700,request_id='slot-1',generation=1)
        self.ledger=self.root/'VM-WINDOWS.json';self.lock=self.root/'VM-WINDOWS.lock';self.lock.touch(mode=0o600)
        self.rows=[dict(writer='orchestrator',grant=self.g)];self.save()
        # A canonical stand-in: /bin is a symlink on merged-usr Linux, which the root check refuses.
        self.limactl=self.root/'limactl-false';self.limactl.write_text('#!/bin/sh\nexit 1\n');self.limactl.chmod(0o700)
        self.authority=dict(grant=self.g,vm='w0-fixture',lima_home=str(self.root/'vm-home'),option='ii',source_commit='a'*40,source_manifest_sha256='b'*64,quiet_acknowledged_by=['E','orchestrator'],work_root=str(self.work),share_root=str(self.work/'w0'),guest_mount='/mnt/w0',host_home=str(self.root),guest_home='/home/example',quiet_lock=str(self.root/'VM-QUIET.lock'),limactl=str(self.limactl),windows=str(self.ledger),windows_lock=str(self.lock))
        self.authority_path=self.root/'authority.json';self.authority_path.write_text(json.dumps(self.authority));self.authority_path.chmod(0o600)
        self.options=SimpleNamespace(grant=str(self.authority_path),work=str(self.work),leases=str(self.root/'leases.json'),activity=str(self.root/'activity.jsonl'))
        Path(self.options.activity).touch(mode=0o600)
        self.ops=window.Operations(self.options,self.authority,runner)
    def tearDown(self):
        if hasattr(self.ops,'close'):self.ops.close()
        self.tmp.cleanup()
    def save(self):self.ledger.write_text(json.dumps(dict(version=1,rows=self.rows)));self.ledger.chmod(0o600)
    def guard(self):
        with patch.object(window.shutil,'disk_usage',return_value=SimpleNamespace(free=500*2**30)):
            self.ops.guard()
    def test_guard_revokes_when_the_orchestrator_ledger_changes(self):
        self.guard()
        self.rows.append(dict(writer='orchestrator',grant=dict(self.g,state='revoked')));self.save()
        with self.assertRaises(window.Refused):self.guard()
    def test_stop_log_never_follows_a_guest_planted_symlink(self):
        self.ops.run.mkdir(mode=0o700)
        victim=self.root/'victim';victim.write_bytes(b'preserve')
        (self.ops.run/'vm-stop.log').symlink_to(victim)
        with patch.object(self.ops,'activity'),patch.object(self.ops,'guard'):
            try:self.ops.call('vm-stop',['/bin/true'],1,cleanup=True)
            except (OSError,window.Refused):pass
        self.assertEqual(victim.read_bytes(),b'preserve')
    def test_existing_vm_option_is_never_admitted(self):
        row=dict(status='Stopped',cpus=4,memory=4096*2**20,arch='aarch64',config=dict(mounts=[dict(location=str(self.work/'w0'),mountPoint='/mnt/w0',writable=True)]))
        self.assertFalse(window.validate_instance(row,self.work,'i'))
    def test_live_ticks_use_an_attached_stdin_pipe(self):
        self.ops.prepare_logs();self.ops.run.mkdir(mode=0o700);self.ops.live(True)
        read,write=os.pipe()
        guest=None
        try:
            self.ops.heartbeat.attach(write);self.ops.heartbeat.start()
            guest=window.heartbeat.Guest(self.ops.run,self.g,read)
            guest.step();self.ops.heartbeat.tick();guest.step()
            self.assertEqual(guest.sequence,2)
        finally:
            if guest:guest.close()
            os.close(read);os.close(write)

    def test_guest_command_stdin_drives_the_complete_close_handshake(self):
        import sys
        self.ops.prepare_logs();self.ops.run.mkdir(mode=0o700);self.ops.live(True)
        peer=self.root/'peer.py'
        peer.write_text("import importlib.util,json,sys\nfrom pathlib import Path\ns=importlib.util.spec_from_file_location('hb',sys.argv[1]);h=importlib.util.module_from_spec(s);s.loader.exec_module(h)\ng=h.Guest(Path(sys.argv[2]),json.loads(sys.argv[3]),0)\ntry:\n while not g.step(closing=g.sequence>=3):h.live_read(g.path,g.grant,int(h.time.time()))\nfinally:g.close()\n")
        args=[sys.executable,'-IB',str(peer),str(Path(window.heartbeat.__file__)),str(self.ops.run),json.dumps(self.g)]
        with patch.object(self.ops,'guard',side_effect=lambda:self.ops.live(True)):
            self.ops.call('guest-w0',args,5)
        self.assertTrue(self.ops.heartbeat.closed)
        self.assertGreaterEqual(self.ops.heartbeat.acknowledged,5)
        rows=[json.loads(x) for x in os.pread(self.ops.logs['live-host'],65536,0).splitlines()]
        self.assertEqual(rows[0]['transport'],'stdin')
        self.assertFalse((self.ops.run/'live-host.ndjson').exists())

    def test_w0_prepare_stages_inputs_from_unshared_authority_roots(self):
        self.ops.source.mkdir(parents=True);events=[]
        receipt=dict(version=1,files=394,crates=390,bytes=123,seconds=.1)
        with patch.object(self.ops,'guard'),patch.object(self.ops,'stage_source',side_effect=lambda:events.append('source')),patch.object(window.inputs,'stage',side_effect=lambda *a,**kw:(events.append('inputs') or receipt)) as stage:
            self.ops.prepare()
        self.assertEqual(events,['source','inputs'])
        self.assertEqual(stage.call_args.args,(self.ops.source,self.work/'w0-toolchain/dl-rust-1.95.0-aarch64-linux',self.root/'.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f',self.ops.run/'build-inputs'))
        self.assertTrue(stage.call_args.kwargs['deadline']>time.monotonic())
        self.assertEqual(json.loads(self.ledger.read_text())['rows'][-1]['event'],'CONSUMED')
        self.assertFalse((self.ops.run/'build-inputs.log').exists())
        self.assertEqual(json.loads(os.pread(self.ops.logs['build-inputs'],4096,0)),receipt)
    def test_mask_does_not_stage_or_extract_toolchain_inputs(self):
        self.ops.grant['step']='W0-MASK'
        with patch.object(window.inputs,'stage') as stage:self.ops.stage_build_inputs()
        stage.assert_not_called()
    def test_filesystem_probe_removes_its_fifo_before_strict_custody(self):
        import hashlib,re,subprocess,sys,stat
        source=self.work/'source';source.mkdir();raw=json.dumps(dict(version=1,commit='a'*40,files={})).encode();(source/'SOURCE.json').write_bytes(raw)
        run=self.work/'probe-run';run.mkdir(mode=0o700)
        script=(Path(__file__).parent/'devnet/w0/guest.sh').read_text()
        block=re.search(r"<<'PY_CHECK'\n(.*?)\nPY_CHECK",script,re.S).group(1)
        result=subprocess.run([sys.executable,'-IB','-',str(source),'a'*40,str(run),hashlib.sha256(raw).hexdigest()],input=block.encode(),capture_output=True,env={'PATH':'/usr/bin:/bin'},timeout=3)
        self.assertEqual(result.returncode,0,result.stderr.decode())
        self.assertTrue(all(stat.S_ISREG(p.lstat().st_mode) for p in (run/'fs-preflight').iterdir()))

    def test_udp_capture_classifies_name_and_logs_ps_and_lsof(self):
        self.ops.prepare_logs()
        vm=Path(self.authority['lima_home'])/self.authority['vm'];vm.mkdir(parents=True)
        (vm/'ha.pid').write_text(str(os.getpid()))
        samples=[b'COMMAND PID NODE NAME\nhostagent 1 UDP 127.0.0.1:65157\n',b'COMMAND PID NODE NAME\nhostagent 1 UDP *:5353\n',b'COMMAND PID NODE NAME\nhostagent 1 UDP [::1]:5353\n']
        def run(args,**kw):
            if args[0]=='/bin/ps':
                raw=f'{os.geteuid()} /usr/bin/limactl' if args[-1]=='uid=,comm=' else 'owned-birth'
                return SimpleNamespace(returncode=0,stdout=raw.encode(),stderr=b'')
            return SimpleNamespace(returncode=0,stdout=samples.pop(0),stderr=b'')
        with patch.object(self.ops,'guard'),patch.object(window.subprocess,'check_output',side_effect=lambda args,**kw:f'{os.geteuid()} /usr/bin/limactl' if args[-1]=='uid=,comm=' else 'owned-birth'),patch.object(window.subprocess,'run',side_effect=run),patch.object(window.time,'sleep'):
            self.ops.capture_udp()
        rows=[json.loads(x) for x in (self.work/'host-logs'/self.ops.run.name/'udp-sockets.log').read_text().splitlines()]
        self.assertEqual([r['non_loopback_udp_observed'] for r in rows],[False,True,False])
        self.assertEqual([r['loopback_udp_observed'] for r in rows],[True,False,True])
        events=[json.loads(x) for x in Path(self.options.activity).read_text().splitlines()]
        starts=[v for v in events if v['event']=='start']
        ends=[v for v in events if v['event']=='end']
        self.assertEqual(len(starts),11);self.assertEqual(len(ends),len(starts))
        self.assertTrue(all(v['argv'][0] in ('/bin/ps','/usr/sbin/lsof') for v in starts))
        self.assertEqual(sum(v['argv'][0]=='/usr/sbin/lsof' for v in starts),3)

    def test_broken_heartbeat_never_skips_the_owned_vm_stop(self):
        events=[];self.ops.heartbeat=SimpleNamespace(deactivate=lambda **kw:(_ for _ in ()).throw(ValueError('torn control')),close=lambda:None,stop_diagnostic=lambda:None)
        with patch.object(self.ops,'call',side_effect=lambda label,*a,**k:events.append(label)):
            self.ops.stop()
        self.assertEqual(events,['vm-stop'])

    def test_share_fifo_swap_cannot_block_vm_stop_or_host_evidence(self):
        import signal
        self.ops.prepare_logs();self.ops.run.mkdir(mode=0o700);self.ops.live(True)
        read,write=os.pipe()
        try:
            self.ops.heartbeat.attach(write);self.ops.heartbeat.start()
            path=self.ops.run/'live-host.ndjson'
            if path.exists():path.unlink()
            os.mkfifo(path,0o600)
            child=os.fork()
            if child==0:
                try:
                    with patch.object(self.ops,'call') as stop:
                        self.ops.stop()
                        assert stop.call_args.args[0]=='vm-stop'
                    os._exit(0)
                except BaseException:os._exit(1)
            end=time.monotonic()+1;status=None
            while time.monotonic()<end:
                pid,state=os.waitpid(child,os.WNOHANG)
                if pid:status=state;break
                time.sleep(.01)
            if status is None:os.kill(child,signal.SIGKILL);os.waitpid(child,0)
            self.assertIsNotNone(status,'VM stop blocked on guest FIFO')
            self.assertTrue(os.WIFEXITED(status) and os.WEXITSTATUS(status)==0)
        finally:os.close(read);os.close(write)
    def test_identity_never_follows_a_guest_replaced_parent(self):
        root=self.work/'state';root.mkdir();path=root/'file';fd=os.open(path,os.O_CREAT|os.O_RDWR,0o600)
        try:
            os.write(fd,b'X'*4096);root.rename(self.work/'old');root.symlink_to(self.work/'old',target_is_directory=True)
            with self.assertRaises((OSError,ValueError)):window.heartbeat.identity(fd,path)
        finally:os.close(fd)

    def test_guest_grant_check_honours_live_revocation(self):
        origin=self.work/'w0'/'host-grant.json';runner.write_new(origin,json.dumps(self.g).encode(),0o400)
        live=origin.with_name('live-grant.json')
        runner.write_new(live,runner.heartbeat.frame(runner.heartbeat.SNAP,dict(version=3,sequence=1,grant=self.g,active=True,tick_unix=int(time.time()),closing=False,observed_monotonic_ns=time.monotonic_ns())),0o600)
        runner.run(['check-grant-v1',str(origin)])
        live.unlink();runner.write_new(live,runner.heartbeat.frame(runner.heartbeat.SNAP,dict(version=3,sequence=1,grant=self.g,active=False,tick_unix=int(time.time()),closing=False,observed_monotonic_ns=time.monotonic_ns())),0o600)
        with self.assertRaises(runner.Refused):runner.run(['check-grant-v1',str(origin)])
    def test_a_revoked_consumed_window_can_record_its_confirmed_release(self):
        a=window.authority
        a.ledger(self.ledger,self.lock,self.g,'owned',event='CONSUMED')
        value=json.loads(self.ledger.read_text());value['rows'].append(dict(writer='orchestrator',grant=dict(self.g,state='revoked')))
        self.ledger.write_text(json.dumps(value))
        a.ledger(self.ledger,self.lock,self.g,'owned',event='RELEASED')
        self.assertEqual(json.loads(self.ledger.read_text())['rows'][-1]['event'],'RELEASED')
    def test_generations_replays_and_foreign_writers_are_refused(self):
        a=window.authority
        for extra in [dict(writer='orchestrator',grant=self.g),dict(writer='orchestrator',grant=dict(self.g,generation=0)),dict(writer='N',grant=dict(self.g,generation=2)),dict(writer='orchestrator',grant=dict(self.g,generation=2))]:
            self.rows=[dict(writer='orchestrator',grant=self.g),extra];self.save()
            with self.assertRaises(window.Refused):self.guard()
        self.rows=[dict(writer='orchestrator',grant=self.g)];self.save()
        a.ledger(self.ledger,self.lock,self.g,'owned',event='CONSUMED')
        for owner,event in [('owned','CONSUMED'),('foreign','RELEASED'),('owned','GRANTED')]:
            with self.assertRaises(window.Refused):a.ledger(self.ledger,self.lock,self.g,owner,event=event)
    def test_superseding_a_window_still_allows_its_owned_stop_record(self):
        a=window.authority;a.ledger(self.ledger,self.lock,self.g,'owned',event='CONSUMED')
        value=json.loads(self.ledger.read_text());value['rows'].append(dict(writer='orchestrator',grant=dict(self.g,generation=2,request_id='slot-2')))
        self.ledger.write_text(json.dumps(value))
        with self.assertRaises(window.Refused):self.guard()
        a.ledger(self.ledger,self.lock,self.g,'owned',event='RELEASED')
        self.assertEqual(json.loads(self.ledger.read_text())['rows'][-1]['request_id'],'slot-1')
    def test_authority_and_clone_home_are_never_inside_the_share(self):
        good=self.authority
        window.validate_roots(good,self.authority_path)
        for bad in [dict(good,lima_home=str(self.work/'w0'/'vm')),dict(good,option='i'),dict(good,share_root=str(self.work)),dict(good,windows=str(self.work/'windows.json'))]:
            with self.assertRaises(window.Refused):window.validate_roots(bad,self.authority_path)
    def test_precreated_held_log_is_not_redirected_even_after_rename(self):
        self.ops.prepare_logs()
        directory=self.work/'host-logs'/self.ops.run.name
        named=directory/'vm-stop.log';named.unlink()
        victim=self.root/'victim';victim.write_bytes(b'keep');named.symlink_to(victim)
        with patch.object(self.ops,'activity'),patch.object(self.ops,'guard'):
            self.ops.call('vm-stop',['/bin/echo','held-output'],1,cleanup=True)
        self.assertEqual(victim.read_bytes(),b'keep')
        self.assertEqual(os.pread(self.ops.logs['vm-stop'],100,0),b'held-output\n')
        for fd in self.ops.logs.values():os.close(fd)
    def test_guard_revocation_kills_and_reaps_the_owned_cli_group(self):
        self.ops.prepare_logs();self.ops.run.mkdir(mode=0o700);self.ops.live(True);children=[]
        original=window.subprocess.Popen
        def spawn(*args,**kwargs):
            child=original(*args,**kwargs);children.append(child);return child
        ticks=[0]
        def guard():
            ticks[0]+=1
            if ticks[0]>1:raise window.Refused('revoked')
        with patch.object(self.ops,'activity'),patch.object(self.ops,'guard',side_effect=guard),patch.object(window.subprocess,'Popen',side_effect=spawn):
            with self.assertRaises(window.Refused):self.ops.call('guest-w0',['/bin/sleep','30'],2)
        self.assertEqual(len(children),1);self.assertIsNotNone(children[0].returncode)
        for fd in self.ops.logs.values():os.close(fd)
    def test_absent_userns_sysctl_matches_the_tracked_launcher(self):
        p=load('preflight');path=self.root/'absent'
        row=p.userns_restriction(path)
        self.assertEqual(row,dict(present=False,value=None,ubuntu_restriction_configured=False,reason='kernel without the Ubuntu userns restriction'))
    def test_zero_userns_sysctl_is_refused_without_rewriting_it(self):
        p=load('preflight');path=self.root/'zero';path.write_bytes(b'0\n')
        with self.assertRaises(p.io.Refused):p.userns_restriction(path)
        self.assertEqual(path.read_bytes(),b'0\n')
    def test_one_userns_sysctl_is_accepted_without_rewriting_it(self):
        p=load('preflight');path=self.root/'one';path.write_bytes(b'1\n')
        row=p.userns_restriction(path)
        self.assertTrue(row['present']);self.assertEqual(row['value'],'1');self.assertTrue(row['ubuntu_restriction_configured'])
        self.assertEqual(path.read_bytes(),b'1\n')
    def test_mask_probe_mode_publishes_unknown_preconditions_without_a_new_gate(self):
        p=load('preflight');root=self.work/'inventory';root.mkdir(mode=0o700)
        with patch.object(p.platform,'system',return_value='Linux'):
            # No backend may execute: this host test replaces the observation collector.
            with patch.object(p,'observe_preconditions',create=True,return_value={'unknown':'not-installed'}) as observe:
                p.run(['mask-probes',str(root),'-'])
        observe.assert_called_once()
        self.assertEqual(json.loads((root/'preconditions-mask-only.json').read_text()),{'unknown':'not-installed'})
    def test_service_history_and_package_changes_are_not_ignored(self):
        p=load('preflight')
        rows={n:dict(UnitFileState='masked',ActiveState='inactive',is_active='inactive',is_active_exit=3,ActiveEnterTimestampMonotonic='0',ExecMainStartTimestampMonotonic='0',LastTriggerUSecMonotonic='0') for n in p.UNITS}
        self.assertTrue(p.services_safe(rows));p.same_packages({'pin':'a'},{'pin':'a'})
        for name,key,value in [('apt-daily.service','ActiveState','active'),('apt-daily.service','ExecMainStartTimestampMonotonic','1'),('apt-daily.timer','LastTriggerUSecMonotonic','1'),('snapd.service','UnitFileState','enabled')]:
            bad={n:dict(row) for n,row in rows.items()};bad[name][key]=value
            with self.assertRaises(p.io.Refused):p.services_safe(bad)
        with self.assertRaises(p.io.Refused):p.same_packages({'pin':'a'},{'pin':'b'})
    def test_mask_inventory_is_durable_before_the_channel_can_fail(self):
        import hashlib
        p=load('preflight');root=self.work/'mask';root.mkdir(mode=0o700)
        p.io.write_new(root/'host-grant.json',json.dumps(self.g).encode(),0o400)
        state=b'UnitFileState=masked\nActiveState=inactive\nActiveEnterTimestampMonotonic=0\nExecMainStartTimestampMonotonic=0\nLastTriggerUSecMonotonic=0\n'
        def command(args,**kw):
            self.assertNotIn('mask',args,'mask inventory must verify persisted masks')
            if 'is-active' in args:return SimpleNamespace(stdout=b'inactive\n',stderr=b'',returncode=3)
            return SimpleNamespace(stdout=state if 'show' in args else b'package pins',stderr=b'',returncode=0)
        binary=b'pinned-bwrap';pin=hashlib.sha256(binary).hexdigest()
        pins=iter((pin,'35613442bd82247d132b1102579edc5567d8047930d1b60fbb8bbba93c5abcd6'))
        with patch.object(p.platform,'system',return_value='Linux'),patch.object(p.platform,'machine',return_value='aarch64'),patch.dict(p.io.BWRAP,{'aarch64':pin}),patch.object(p.subprocess,'run',side_effect=command),patch.object(p.io,'read_file',return_value=binary),patch.object(p.hashlib,'sha256',side_effect=lambda raw:SimpleNamespace(hexdigest=lambda:next(pins))):
            # Host contract test: no Linux command or syscall executes.
            p.run(['mask-inventory',str(root),'-'])
        for name in ('services-mask-only.json','packages-mask-only.json'):
            self.assertTrue((root/name).is_file());self.assertEqual((root/name).stat().st_mode&0o777,0o400)
        with patch.object(p.platform,'system',return_value='Linux'),patch.object(p.io.heartbeat,'receive_frame',side_effect=p.io.heartbeat.Refused('stdin disconnected')):
            with self.assertRaisesRegex(ValueError,'stdin disconnected'):p.run(['mask-skew',str(root),'-'])
        self.assertTrue((root/'services-mask-only.json').exists())
        raw=p.io.heartbeat.frame(p.io.heartbeat.LIVE,dict(version=3,sequence=1,grant=self.g,active=True,tick_unix=int(time.time()),closing=False))
        with patch.object(p.platform,'system',return_value='Linux'),patch.object(p.io.heartbeat,'receive_frame',return_value=raw):p.run(['mask-skew',str(root),'-'])
        self.assertEqual((root/'live-initial.bin').read_bytes(),raw)
        self.assertEqual(json.loads((root/'clock-skew.json').read_text())['source'],'stdin')
        shell=(Path(__file__).parent/'devnet/w0/guest.sh').read_text();mask=shell[shell.index('if [[ $w0_step == W0-MASK ]]'):shell.index('  findmnt')]
        self.assertLess(mask.index('mask-inventory'),mask.index('mask-skew'))
        self.assertLess(mask.index('mask-skew'),mask.index('watch-live-v3'))

    def test_guard_waits_for_the_shared_ledger_lock(self):
        import concurrent.futures,fcntl,threading
        entered=threading.Event()
        def guard():entered.set();self.guard()
        with self.lock.open('r+') as lock,concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            fcntl.flock(lock,fcntl.LOCK_EX)
            future=pool.submit(guard);self.assertTrue(entered.wait(1))
            try:
                with self.assertRaises(concurrent.futures.TimeoutError):future.result(timeout=.05)
                self.rows.append(dict(writer='orchestrator',grant=dict(self.g,state='revoked')));self.save()
            finally:fcntl.flock(lock,fcntl.LOCK_UN)
            with self.assertRaises(window.Refused):future.result(timeout=2)
    def test_sigterm_still_stops_before_release(self):
        events=[]
        class Ops:
            def validate(self):pass
            def start(self):events.append('start')
            def guest(self):window.interrupted(window.signal.SIGTERM,None)
            def stop(self):events.append('stop')
            def stopped(self):events.append('stopped');return True
        class Lease:
            def acquire(self):return True
            def release(self):events.append('release')
        with patch.object(window.signal,'signal'):
            with self.assertRaises(window.Refused):window.execute(Ops(),Lease())
        self.assertEqual(events,['start','stop','stopped','release'])
    def test_failed_stop_observation_records_a_marker_and_retains_the_lease(self):
        events=[]
        class Ops:
            def validate(self):pass
            def start(self):pass
            def guest(self):pass
            def stop(self):pass
            def stopped(self):raise OSError('inventory unavailable')
            def unconfirmed(self):events.append('marker')
        class Lease:
            def acquire(self):return True
            def release(self):events.append('release')
        with self.assertRaises((window.Refused,OSError)):window.execute(Ops(),Lease())
        self.assertEqual(events,['marker'])
    def test_an_arbitrary_external_file_cannot_replace_the_adopted_ledger(self):
        with self.assertRaises(window.Refused):window.validate_roots(dict(self.authority,windows=str(self.root/'counterfeit.json')),self.authority_path)
    def test_a_fast_success_still_rechecks_authority_before_acceptance(self):
        self.ops.prepare_logs();count=[0]
        def guard():
            count[0]+=1
            if count[0]>1:raise window.Refused('revoked after fast exit')
        process=SimpleNamespace(returncode=0,poll=lambda:0)
        with patch.object(self.ops,'activity'),patch.object(self.ops,'guard',side_effect=guard),patch.object(window.subprocess,'Popen',return_value=process):
            with self.assertRaises(window.Refused):self.ops.call('vm-start',['fake'],1)
        for fd in self.ops.logs.values():os.close(fd)
    def test_shared_activity_cannot_be_redirected_into_the_guest_share(self):
        self.options.activity=str(self.work/'w0'/'guest-activity.jsonl')
        with self.assertRaises(window.Refused):self.ops.prepare_logs()
    def test_service_show_properties_do_not_substitute_for_is_active_evidence(self):
        p=load('preflight')
        rows={n:dict(UnitFileState='masked',ActiveState='inactive',ActiveEnterTimestampMonotonic='0',ExecMainStartTimestampMonotonic='0',LastTriggerUSecMonotonic='0') for n in p.UNITS}
        with self.assertRaises(p.io.Refused):p.services_safe(rows)
    def test_interruption_during_reservation_cleans_only_its_own_row(self):
        events=[]
        class Ops:
            def validate(self):pass
            def start(self):events.append('start')
        class Lease:
            def acquire(self):events.append('own-row-written');raise window.Refused('interrupted')
            def release(self):events.append('own-row-removed')
        with self.assertRaises(window.Refused):window.execute(Ops(),Lease())
        self.assertEqual(events,['own-row-written','own-row-removed'])
    def test_real_lima_transport_defaults_preserve_the_exact_share_boundary(self):
        mount=dict(location=str(self.work/'w0'),mountPoint='/mnt/w0',writable=True,sshfs=dict(followSymlinks=False),**{'9p':dict(securityModel='none')})
        row=dict(status='Stopped',cpus=4,memory=4096*2**20,arch='aarch64',vmType='vz',config=dict(CONFIG_SAFE,mountType='virtiofs',mounts=[mount]))
        self.assertTrue(window.validate_instance(row,self.work,'ii'))
        for bad in [dict(row,vmType='qemu'),dict(row,config=dict(CONFIG_SAFE,mountType='reverse-sshfs',mounts=[mount])),dict(row,config=dict(CONFIG_SAFE,mountType='virtiofs',mounts=[dict(mount,sshfs=dict(followSymlinks=True))])),dict(row,config=dict(CONFIG_SAFE,mountType='virtiofs',mounts=[mount,dict(mount,writable=False)]))]:
            self.assertFalse(window.validate_instance(bad,self.work,'ii'))
    def test_vm_disk_and_yaml_cannot_alias_a_share_or_the_original_disk(self):
        import hashlib
        vm=Path(self.authority['lima_home'])/self.authority['vm'];vm.mkdir(parents=True)
        disk=vm/'disk';disk.write_bytes(b'clone');(vm/'lima.yaml').write_bytes(b'config')
        self.ops.source.mkdir(parents=True);manifest=self.ops.source/'SOURCE.json';manifest.write_text(json.dumps(dict(commit=self.authority['source_commit'])));manifest.chmod(0o400)
        self.authority['source_manifest_sha256']=hashlib.sha256(manifest.read_bytes()).hexdigest()
        self.authority_path.write_text(json.dumps(self.authority));self.ops.original=self.authority_path.read_bytes()
        row=dict(status='Stopped',cpus=4,memory=4096*2**20,arch='aarch64',vmType='vz',config=dict(CONFIG_SAFE,mountType='virtiofs',mounts=[dict(location=str(self.work/'w0'),mountPoint='/mnt/w0',writable=True)]))
        with patch.object(self.ops,'info',return_value=row),patch.object(window.shutil,'disk_usage',return_value=SimpleNamespace(free=500*2**30)):
            self.ops.validate()
            pending=self.work/'host-runs';pending.mkdir(mode=0o700)
            (pending/'UNSEALED').write_bytes(b'previous run not retained')
            with self.assertRaisesRegex(window.Refused,'custody'):self.ops.validate()
            (pending/'UNSEALED').unlink()
            exposed=self.work/'w0'/'guest-disk';exposed.write_bytes(b'guest-writable');disk.unlink();disk.symlink_to(exposed)
            with self.assertRaises(window.Refused):self.ops.validate()
            disk.unlink();disk.write_bytes(b'clone');os.link(disk,self.root/'original-alias')
            with self.assertRaises(window.Refused):self.ops.validate()
    def test_lima_refuses_host_exfiltration_channels(self):
        config=dict(CONFIG_SAFE,mountType='virtiofs',mounts=[dict(location=str(self.work/'w0'),mountPoint='/mnt/w0',writable=True)],ssh=dict(forwardAgent=False,forwardX11=False,forwardX11Trusted=False),copyToHost=[],portForwards=[dict(guestIPMustBeZero=False,guestIP='127.0.0.1',guestPortRange=[1,65535],hostIP='127.0.0.1',hostPortRange=[1,65535],proto='any',ignore=True)],propagateProxyEnv=False,networks=[])
        row=dict(status='Stopped',cpus=4,memory=4096*2**20,arch='aarch64',vmType='vz',config=config)
        self.assertTrue(window.validate_instance(row,self.work,'ii'))
        for key,value in [('ssh',dict(forwardAgent=True)),('copyToHost',[dict(guest='/tmp/guest',host='/tmp/host')]),('portForwards',[]),('propagateProxyEnv',True),('networks',[dict(lima='shared')])]:
            with self.subTest(key=key):self.assertFalse(window.validate_instance(dict(row,config=dict(config,**{key:value})),self.work,'ii'))
    def test_stopped_run_is_detached_from_guest_before_final_hashes(self):
        run=self.work/'w0'/'run-seal';run.mkdir(mode=0o700)
        (run/'receipt').write_bytes(b'evidence');os.link(run/'receipt',self.work/'w0'/'alias')
        dest=self.work/'sealed';manifest=self.work/'hashes.json'
        window.custody.seal(run,dest,manifest)
        self.assertFalse(run.exists());self.assertTrue(json.loads(manifest.read_bytes())['complete'])
        (self.work/'w0'/'alias').write_bytes(b'changed')
        self.assertEqual((dest/'receipt').read_bytes(),b'evidence')
        dest.chmod(0o700)
    def test_failed_retention_keeps_truncation_and_per_file_errors(self):
        # Host custody refuses unsafe entries but always preserves the moved tree.
        run=self.work/'w0'/'run-unsafe';run.mkdir(mode=0o700);(run/'unsafe').symlink_to('/tmp/foreign')
        dest=self.work/'sealed';manifest=self.work/'hashes.json'
        with self.assertRaises(ValueError):window.custody.seal(run,dest,manifest)
        self.assertFalse(run.exists());self.assertIn('error',json.loads(manifest.read_bytes())['files']['unsafe']);dest.chmod(0o700)

    def test_guest_skew_has_a_bounded_allowance_and_mask_boot_is_short(self):
        runner.validate_grant(dict(self.g,step='W0-MASK',end_unix=self.g['start_unix']+900),int(time.time()))
        with self.assertRaises(runner.Refused):runner.validate_grant(dict(self.g,step='W0-MASK'),int(time.time()))
        origin=self.work/'w0'/'host-grant.json';runner.write_new(origin,json.dumps(self.g).encode(),0o400)
        live=origin.with_name('live-grant.json');now=int(time.time())
        for offset in (-33,30):
            if live.exists():live.unlink()
            runner.write_new(live,runner.heartbeat.frame(runner.heartbeat.SNAP,dict(version=3,sequence=1,grant=self.g,active=True,tick_unix=now+offset,closing=False,observed_monotonic_ns=time.monotonic_ns())),0o600)
            runner.validate_live(live,self.g,now)
        for offset in (-34,31):
            live.unlink();runner.write_new(live,runner.heartbeat.frame(runner.heartbeat.SNAP,dict(version=3,sequence=1,grant=self.g,active=True,tick_unix=now+offset,closing=False,observed_monotonic_ns=time.monotonic_ns())),0o600)
            with self.assertRaises(runner.Refused):runner.validate_live(live,self.g,now)

    def test_remaining_budget_includes_copy_build_and_test(self):
        budget=load('budget')
        self.assertEqual(budget.remaining(1980,600,300,now=0),600)
        self.assertEqual(budget.remaining(1980,1200,90,now=1500),390)
        with self.assertRaises(ValueError):budget.remaining(1980,1200,90,now=1890)
    def test_kernel_precheck_uses_file_bind_scratch_environment_and_private_fd(self):
        kernel=load('kernel_check');p=load('preflight')
        args=kernel.plan(Path('/sys/fs/cgroup/owned'),Path('/tmp/input'),Path('/tmp/scratch'),p.BWRAP_FLAGS)
        self.assertIn('--ro-bind',args);self.assertIn('--bind',args);self.assertIn('--chdir',args);self.assertIn('--setenv',args)
        self.assertIn('os.write(3',args[-1]);self.assertEqual(args[1:1+len(p.BWRAP_FLAGS)],p.BWRAP_FLAGS)
        with patch.object(kernel.platform,'system',return_value='Darwin'):
            with self.assertRaises(ValueError):kernel.run(Path('/tmp/never-created'),self.work,p.BWRAP_FLAGS)

    def test_bootstrap_checks_unshared_helper_bytes_before_import(self):
        import hashlib,subprocess,sys
        source=self.ops.source;helpers=source/'scripts/devnet/w0';helpers.mkdir(parents=True)
        files={}
        for name in ('window','authority','custody','runner','heartbeat'):
            raw=(Path(__file__).parent/'devnet/w0'/f'{name}.py').read_bytes();(helpers/f'{name}.py').write_bytes(raw)
            files[f'scripts/devnet/w0/{name}.py']=hashlib.sha256(raw).hexdigest()
        raw=json.dumps(dict(version=1,commit='a'*40,files=files)).encode();(source/'SOURCE.json').write_bytes(raw)
        self.authority['source_manifest_sha256']=hashlib.sha256(raw).hexdigest();self.authority_path.write_text(json.dumps(self.authority))
        victim=self.root/'imported'
        (helpers/'authority.py').write_text('from pathlib import Path;Path('+repr(str(victim))+').touch()')
        result=subprocess.run([sys.executable,'-IB',str(helpers/'window.py'),'run','--grant',str(self.authority_path)],env={'PATH':'/usr/bin:/bin'},capture_output=True,timeout=3)
        self.assertNotEqual(result.returncode,0);self.assertIn(b'bootstrap script pin',result.stderr);self.assertFalse(victim.exists())

    def test_source_access_time_is_not_a_content_change(self):
        source=self.work/'file';source.write_bytes(b'fixed');before=source.stat()
        def observation(atime,mtime):
            fields=list(before);fields[7]+=atime;fields[8]+=mtime
            return os.stat_result(fields,dict(st_atime_ns=before.st_atime_ns+atime*10**9,st_mtime_ns=before.st_mtime_ns+mtime*10**9,st_ctime_ns=before.st_ctime_ns))
        with patch.object(window.custody.os,'fstat',side_effect=[observation(0,0),observation(1,0)]):
            self.assertEqual(window.custody.read(source,10),b'fixed')
        with patch.object(window.custody.os,'fstat',side_effect=[observation(0,0),observation(0,1)]):
            with self.assertRaises(ValueError):window.custody.read(source,10)
    def test_unreadable_directory_never_produces_complete_custody(self):
        run=self.work/'w0'/'run-unreadable';run.mkdir(mode=0o700);child=run/'hidden';child.mkdir(mode=0o700);(child/'receipt').write_bytes(b'evidence');child.chmod(0)
        dest=self.work/'sealed';manifest=self.work/'hashes.json'
        try:
            with self.assertRaises((OSError,ValueError)):window.custody.seal(run,dest,manifest)
            self.assertFalse(json.loads(manifest.read_bytes())['complete'])
        finally:
            if dest.exists():dest.chmod(0o700);(dest/'hidden').chmod(0o700)

    def test_preboot_custody_marker_clears_only_after_successful_retention(self):
        self.ops.prepare_logs();self.ops.reserve_custody();self.ops.run.mkdir(mode=0o700)
        (self.ops.run/'receipt').write_bytes(b'owned evidence')
        self.assertTrue(self.ops.custody_marker.exists())
        self.ops.released()
        self.assertFalse(self.ops.custody_marker.exists());self.assertFalse(self.ops.run.exists())
        sealed=self.work/'host-runs'/self.ops.run.name
        self.assertTrue(json.loads((self.work/'host-logs'/self.ops.run.name/'run-hashes.json').read_bytes())['complete'])
        self.assertEqual((sealed/'receipt').read_bytes(),b'owned evidence');sealed.chmod(0o700)
    def test_failed_retention_keeps_preboot_marker_and_outside_share_evidence(self):
        self.ops.prepare_logs();self.ops.reserve_custody();self.ops.run.mkdir(mode=0o700)
        (self.ops.run/'unsafe').symlink_to('/tmp/foreign')
        with self.assertRaises(ValueError):self.ops.released()
        self.assertTrue(self.ops.custody_marker.exists());self.assertFalse(self.ops.run.exists())
        sealed=self.work/'host-runs'/self.ops.run.name;sealed.chmod(0o700)
        self.assertFalse(json.loads((self.work/'host-logs'/self.ops.run.name/'run-hashes.json').read_bytes())['complete'])

    def test_host_source_is_unshared_and_each_guest_copy_is_fresh(self):
        self.assertFalse(self.ops.source.is_relative_to(self.work/'w0'))
        self.ops.source.mkdir(parents=True)
        (self.ops.source/'payload').write_bytes(b'reviewed')
        import hashlib
        raw=json.dumps(dict(version=1,commit='a'*40,files={'payload':hashlib.sha256(b'reviewed').hexdigest()})).encode()
        (self.ops.source/'SOURCE.json').write_bytes(raw)
        self.authority['source_manifest_sha256']=hashlib.sha256(raw).hexdigest()
        self.ops.run.mkdir(mode=0o700)
        self.ops.stage_source()
        guest=self.ops.run/'source'
        self.assertEqual((guest/'payload').read_bytes(),b'reviewed')
        (guest/'payload').chmod(0o600);(guest/'payload').write_bytes(b'guest-tamper')
        self.assertEqual((self.ops.source/'payload').read_bytes(),b'reviewed')
        with self.assertRaises((OSError,window.Refused)):self.ops.stage_source()
        self.ops.run=self.work/'w0'/'run-next-2';self.ops.run.mkdir(mode=0o700);self.ops.stage_source()
        self.assertEqual((self.ops.run/'source/payload').read_bytes(),b'reviewed')

    def test_fake_limactl_uses_authority_home_and_checks_the_instance_directory(self):
        fake=self.root/'limactl';expected=Path(self.authority['lima_home'])/self.authority['vm']
        def install(directory):
            row=dict(name=self.authority['vm'],dir=str(directory),status='Stopped')
            fake.write_text('#!/usr/bin/python3\nimport os,sys,json\nif os.environ.get("LIMA_HOME")!='+repr(self.authority['lima_home'])+':sys.exit(3)\nprint('+repr(json.dumps(row))+')\n');fake.chmod(0o700)
        self.ops.lima=str(fake);install(expected)
        self.assertEqual(self.ops.info()['dir'],str(expected))
        install(self.root/'foreign')
        with self.assertRaises(window.Refused):self.ops.info()


class InventoryTests(unittest.TestCase):
    class Fake:
        def __init__(self):self.calls=[];self.files={}
        def remaining(self):return 60
        def read(self,path,cap=65536):
            value=self.files.get(str(path),FileNotFoundError())
            if isinstance(value,Exception):raise value
            return value
        def command(self,args,user_bus=False):
            self.calls.append(('command',args,user_bus))
            output='boole-gate-bwrap (unconfined)\n' if args[0]=='/usr/bin/sudo' else '7.1.0-fixture\n'
            return dict(exit_code=0,stdout=output,stderr='',environment_names=['PATH','LC_ALL'])
        def syscall(self,number,*args):
            self.calls.append(('syscall',number,[v.value for v in args]))
            return dict(value=8 if number==444 else -1,errno=None if number==444 else 22)
    def test_kernel_identity_records_absent_ubuntu_rule_and_loaded_profile(self):
        module=load('inventory');i=self.Fake()
        i.files={'/sys/kernel/security/lsm':b'landlock,apparmor', '/sys/module/apparmor/parameters/enabled':b'Y', '/proc/sys/user/max_user_namespaces':b'12345', '/etc/apparmor.d/boole-gate-bwrap':b'fixture-profile', '/sys/kernel/security/apparmor/profiles':PermissionError()}
        row=module.kernel_identity(i)
        self.assertEqual(row['userns_sysctl']['status'],'absent');self.assertFalse(row['ubuntu_restriction_configured'])
        self.assertIn('without the Ubuntu',row['userns_rule']);self.assertTrue(row['profile_loaded']);self.assertEqual(row['profile_modes'],['unconfined'])
        self.assertEqual(row['landlock_abi']['value']['value'],8);self.assertFalse(row['profile_file_matches_pin'])
    def test_unavailable_kernel_observations_and_zero_are_not_new_gates(self):
        module=load('inventory');i=self.Fake();i.files[module.SYSCTL]=b'0'
        row=module.kernel_identity(i)
        self.assertEqual(row['userns_sysctl']['value'],'0');self.assertFalse(row['ubuntu_restriction_configured'])
        self.assertIsNone(row['profile_loaded']);self.assertTrue(row['informational_only'])
    def test_syscall_availability_queries_cannot_create_close_or_signal(self):
        module=load('inventory');i=self.Fake();kernel=module.kernel_identity(i);module.kernel_support(i,kernel)
        calls={v[1]:v[2] for v in i.calls if v[0]=='syscall'}
        self.assertEqual(calls[435],[None,0]);self.assertGreater(calls[436][0],calls[436][1])
        self.assertEqual(calls[434],[-1,0]);self.assertEqual(calls[424],[-1,0,None,0])
        self.assertEqual(calls[444],[None,0,1])
    def test_process_limits_are_read_without_changing_them(self):
        module=load('inventory');i=self.Fake();kernel=module.kernel_identity(i)
        with patch.object(module.resource,'getrlimit',return_value=(32,64)),patch.object(module.resource,'setrlimit') as change:
            row=module.kernel_support(i,kernel)
        self.assertEqual(row['process_limits']['value']['nofile'],(32,64));change.assert_not_called()
    def test_failed_observation_keeps_remaining_categories_and_receipts(self):
        module=load('inventory');i=self.Fake();emitted=[]
        i.files['/proc/self/cgroup']=b'0::/fixture'
        with tempfile.TemporaryDirectory() as tmp:
            row=module.collect(Path(tmp),Path(tmp),Path(tmp),i,emit=lambda name,value:emitted.append(name))
        self.assertEqual(row['registry']['status'],'absent');self.assertEqual(row['storage']['status'],'observed')
        self.assertEqual(row['cgroup']['status'],'observed');self.assertTrue({'kernel','toolchain','registry','storage','cgroup'}<=set(emitted))
        self.assertTrue(row['budget']['no_build_or_case_executed'])
    def test_registry_reports_real_archive_and_index_pins_without_trusting_source_manifest(self):
        import hashlib
        module=load('inventory')
        class Reader(self.Fake):
            def read(self,path,cap=65536):return Path(path).read_bytes()
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);registry=root/'.cargo/registry';data=b'locked crate';pin=hashlib.sha256(data).hexdigest()
            (root/'Cargo.lock').write_text('version = 4\n[[package]]\nname="boole-gate-supervisor"\nversion="0.1.0"\ndependencies=["alpha"]\n[[package]]\nname="alpha"\nversion="1.0.0"\nsource="registry+fixture"\nchecksum="'+pin+'"\n')
            archive=registry/'cache/registry/alpha-1.0.0.crate';archive.parent.mkdir(parents=True);archive.write_bytes(data)
            index=registry/'index/registry/.cache/al/ph/alpha';index.parent.mkdir(parents=True);index.write_bytes(b'header\0'+json.dumps(dict(name='alpha',vers='1.0.0',cksum=pin)).encode()+b'\0')
            source=registry/'src/registry/alpha-1.0.0/.cargo-checksum.json';source.parent.mkdir(parents=True);source.write_text(json.dumps(dict(package=pin)))
            good=module.registry(Reader(),root,root);self.assertEqual(good['missing_archives'],0);self.assertEqual(good['missing_index_rows'],0)
            archive.write_bytes(b'damaged');bad=module.registry(Reader(),root,root)
            self.assertEqual(bad['missing_archives'],1);self.assertTrue(bad['packages'][0]['source_manifest_pin'])
            self.assertFalse(bad['packages'][0]['archive_verified']);self.assertEqual(archive.read_bytes(),b'damaged')

class InputSetTests(unittest.TestCase):
    def setUp(self):
        import hashlib,tarfile,io
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name).resolve();self.module=load('inputs')
        self.source=self.root/'source';self.archives=self.root/'archives';self.cache=self.root/'cache';self.stage=self.root/'staged'
        for p in (self.source/'scripts/devnet/w0',self.archives,self.cache):p.mkdir(parents=True)
        pins=json.loads((Path(__file__).parent/'devnet/w0/toolchain-v1.json').read_text())
        for row in pins['components']:
            raw=row['component'].encode();path=self.archives/row['file'];path.write_bytes(raw);path.chmod(0o400)
            row.update(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
        (self.source/'scripts/devnet/w0/toolchain-v1.json').write_text(json.dumps(pins))
        buffer=io.BytesIO()
        with tarfile.open(fileobj=buffer,mode='w:gz') as archive:
            raw=b'[package]\nname="alpha"\nversion="1.0.0"\nedition="2021"\n'
            for name,data in [('alpha-1.0.0/Cargo.toml',raw),('alpha-1.0.0/src/lib.rs',b'pub fn value()->u8 { 1 }')]:
                info=tarfile.TarInfo(name);info.size=len(data);info.mode=0o644;archive.addfile(info,io.BytesIO(data))
        raw=buffer.getvalue();(self.cache/'alpha-1.0.0.crate').write_bytes(raw)
        pin=hashlib.sha256(raw).hexdigest()
        (self.source/'Cargo.lock').write_text('version=4\n[[package]]\nname="boole-gate-supervisor"\nversion="0.1.0"\ndependencies=["alpha"]\n[[package]]\nname="alpha"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="'+pin+'"\n')
        self.spec=self.module.specification(self.source)
        self.receipt=self.module.stage(self.source,self.archives,self.cache,self.stage)
        self.crate=self.stage/'crates/alpha-1.0.0.crate'
    def tearDown(self):self.temp.cleanup()
    def refused(self):
        with self.assertRaises(self.module.Refused):self.module.verify(self.stage,self.spec)
        self.assertFalse((self.root/'prefix').exists(),'rejected inputs reached extraction')
    def mutate(self,path):
        raw=path.read_bytes();path.chmod(0o600);path.write_bytes(bytes([raw[0]^1])+raw[1:]);path.chmod(0o400)
    def test_tampered_toolchain_is_refused_before_extraction(self):
        self.mutate(next((self.stage/'toolchain').iterdir()));self.refused()
    def test_tampered_crate_is_refused_before_extraction(self):
        self.mutate(self.crate);self.refused()
    def test_missing_crate_is_refused_before_extraction(self):
        self.crate.unlink();self.refused()
    def test_extra_input_is_refused_before_extraction(self):
        self.module.create(self.stage/'unexpected',b'extra');self.refused()
    def test_symlink_input_is_refused_before_extraction(self):
        self.crate.unlink();self.crate.symlink_to(self.cache/'alpha-1.0.0.crate');self.refused()
    def test_fifo_input_is_refused_before_extraction_without_blocking(self):
        self.crate.unlink();os.mkfifo(self.crate,0o400);self.refused()
    def test_install_verifies_the_entire_set_before_any_extraction(self):
        self.stage.rename(self.root/'build-inputs');self.stage=self.root/'build-inputs';self.crate=self.stage/'crates/alpha-1.0.0.crate';self.mutate(self.crate)
        with patch.object(self.module.platform,'system',return_value='Linux'),patch.object(self.module,'unpack') as extract:
            with self.assertRaises(self.module.Refused):self.module.install_guest(self.source,self.root)
        extract.assert_not_called();self.assertFalse((self.root/'toolchain').exists())
    def test_directory_source_has_real_package_and_file_checksums(self):
        import hashlib
        out=self.root/'vendor';row=self.module.vendor_crates(self.spec['crates'],self.stage/'crates',out,True)
        manifest=json.loads((out/'alpha-1.0.0/.cargo-checksum.json').read_text())
        self.assertEqual(row['crates'],1);self.assertEqual(manifest['package'],self.spec['crates']['alpha-1.0.0.crate'])
        self.assertEqual(manifest['files']['src/lib.rs'],hashlib.sha256((out/'alpha-1.0.0/src/lib.rs').read_bytes()).hexdigest())
    def test_tar_traversal_and_special_members_do_not_escape_vendor(self):
        import tarfile,io
        for index,kind in enumerate(('path','symlink','fifo')):
            path=self.root/f'unsafe-{index}.crate'
            with tarfile.open(path,'w:gz') as archive:
                info=tarfile.TarInfo('alpha-1.0.0/../../escape' if kind=='path' else 'alpha-1.0.0/member')
                if kind=='symlink':info.type=tarfile.SYMTYPE;info.linkname='../../escape'
                elif kind=='fifo':info.type=tarfile.FIFOTYPE
                else:info.size=1
                archive.addfile(info,io.BytesIO(b'x') if info.isfile() else None)
            out=self.root/f'out-{index}';out.mkdir(mode=0o700);fd=os.open(path,os.O_RDONLY)
            try:
                with self.assertRaises(self.module.Refused):self.module.unpack(fd,out,('alpha-1.0.0',))
            finally:os.close(fd)
        self.assertFalse((self.root/'escape').exists())
    def test_native_cargo_accepts_vendor_and_refuses_changed_source(self):
        import subprocess,shutil
        vendor=self.root/'vendor';self.module.vendor_crates(self.spec['crates'],self.stage/'crates',vendor,True)
        home=self.root/'cargo-home';self.module.configure_cargo(home,vendor)
        (self.source/'Cargo.toml').write_text('[package]\nname="boole-gate-supervisor"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nalpha="=1.0.0"\n[lib]\npath="root.rs"\n[workspace]\n')
        (self.source/'root.rs').write_text('pub fn value()->u8 { alpha::value() }')
        env={k:os.environ[k] for k in ('PATH','HOME','TMPDIR','RUSTC','RUSTDOC','RUSTUP_HOME') if k in os.environ}
        env.update(CARGO_HOME=str(home),CARGO_TARGET_DIR=str(self.root/'target'),CARGO_NET_OFFLINE='true',RUSTUP_AUTO_INSTALL='0',CARGO_BUILD_JOBS='2')
        cargo=shutil.which('cargo');self.assertIsNotNone(cargo,'native Cargo is required for directory-source control')
        def run():return subprocess.run([cargo,'check','--locked','--offline'],cwd=self.source,env=env,capture_output=True,timeout=60)
        first=run();self.assertEqual(first.returncode,0,first.stderr.decode())
        path=vendor/'alpha-1.0.0/src/lib.rs';path.chmod(0o644);path.write_bytes(b'pub fn value()->u8 { 2 }')
        env['CARGO_TARGET_DIR']=str(self.root/'target-recheck')  # Force Cargo to consume the altered vendor source.
        second=run();self.assertNotEqual(second.returncode,0);self.assertIn(b'checksum',second.stderr.lower())
    def test_native_workspace_resolution_requires_the_full_staged_lock_set(self):
        import hashlib,tarfile,io,subprocess,shutil,tomllib
        buffer=io.BytesIO()
        with tarfile.open(fileobj=buffer,mode='w:gz') as archive:
            for name,raw in [('beta-1.0.0/Cargo.toml',b'[package]\nname="beta"\nversion="1.0.0"\nedition="2021"\n'),('beta-1.0.0/src/lib.rs',b'pub fn value()->u8 { 2 }')]:
                info=tarfile.TarInfo(name);info.size=len(raw);info.mode=0o644;archive.addfile(info,io.BytesIO(raw))
        raw=buffer.getvalue();(self.cache/'beta-1.0.0.crate').write_bytes(raw)
        with (self.source/'Cargo.lock').open('a') as stream:
            stream.write('[[package]]\nname="boole-node"\nversion="0.1.0"\ndependencies=["beta"]\n[[package]]\nname="beta"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="'+hashlib.sha256(raw).hexdigest()+'"\n')
        (self.source/'Cargo.toml').write_text('[package]\nname="boole-gate-supervisor"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nalpha="=1.0.0"\n[lib]\npath="root.rs"\n[workspace]\nmembers=["node"]\nresolver="2"\n')
        (self.source/'root.rs').write_text('pub fn value()->u8 { alpha::value() }')
        (self.source/'node/src').mkdir(parents=True)
        (self.source/'node/Cargo.toml').write_text('[package]\nname="boole-node"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nbeta="=1.0.0"\n')
        (self.source/'node/src/lib.rs').write_text('pub fn value()->u8 { beta::value() }')
        spec=self.module.specification(self.source)
        expected={p['name']+'-'+p['version']+'.crate':p['checksum'] for p in tomllib.loads((self.source/'Cargo.lock').read_text())['package'] if p.get('source')}
        self.assertEqual(spec['crates'],expected,'the unselected workspace member still participates in Cargo resolution')
        staged=self.root/'workspace-inputs';self.module.stage(self.source,self.archives,self.cache,staged)
        self.assertEqual({p.name for p in (staged/'crates').iterdir()},set(expected))
        vendor=self.root/'workspace-vendor';self.module.vendor_crates(spec['crates'],staged/'crates',vendor,True)
        home=self.root/'workspace-cargo-home';self.module.configure_cargo(home,vendor)
        env={k:os.environ[k] for k in ('PATH','HOME','TMPDIR','RUSTC','RUSTDOC','RUSTUP_HOME') if k in os.environ}
        env.update(CARGO_HOME=str(home),CARGO_TARGET_DIR=str(self.root/'workspace-target'),CARGO_NET_OFFLINE='true',RUSTUP_AUTO_INSTALL='0',CARGO_BUILD_JOBS='2')
        cargo=shutil.which('cargo');self.assertIsNotNone(cargo)
        def build():return subprocess.run([cargo,'build','--locked','--offline','-p','boole-gate-supervisor'],cwd=self.source,env=env,capture_output=True,timeout=60)
        # Neither archive cache nor an existing Cargo home supplies this build.
        self.cache.rename(self.root/'unused-original-cache')
        green=build();self.assertEqual(green.returncode,0,green.stderr.decode())
        (staged/'crates/beta-1.0.0.crate').unlink()
        with self.assertRaises(self.module.Refused):self.module.verify(staged,spec)
        # Resolution negative control only: deliberately make an incomplete vendor
        # from the remaining staged archives. Production refuses it above.
        remaining={n:p for n,p in expected.items() if n!='beta-1.0.0.crate'}
        negative=self.root/'missing-vendor';self.module.vendor_crates(remaining,staged/'crates',negative,True)
        missing_home=self.root/'missing-cargo-home';self.module.configure_cargo(missing_home,negative)
        env.update(CARGO_HOME=str(missing_home),CARGO_TARGET_DIR=str(self.root/'missing-target'))
        red=build();self.assertNotEqual(red.returncode,0);self.assertIn(b'no matching package named `beta`',red.stderr)
    def test_component_payload_layout_skips_installers_and_metadata(self):
        import tarfile,io
        path=self.root/'synthetic-component.tar.xz'
        with tarfile.open(path,'w:xz') as archive:
            for name,raw,mode in [('release/install.sh',b'never execute',0o755),('release/rustc/manifest.in',b'installer metadata',0o644),('release/rustc/bin/rustc',b'synthetic, not executable code',0o755),('release/rustc/lib/payload',b'library',0o644)]:
                info=tarfile.TarInfo(name);info.size=len(raw);info.mode=mode;archive.addfile(info,io.BytesIO(raw))
        prefix=self.root/'prefix';prefix.mkdir(mode=0o700);fd=os.open(path,os.O_RDONLY)
        try:row=self.module.unpack(fd,prefix,('release','rustc'),True,'release')
        finally:os.close(fd)
        self.assertEqual(set(row['files']),{'bin/rustc','lib/payload'})
        self.assertEqual((prefix/'bin/rustc').stat().st_mode&0o777,0o555)
        self.assertFalse((prefix/'install.sh').exists());self.assertFalse((prefix/'manifest.in').exists())
    def test_complete_input_staging_is_exclusive_and_source_preserving(self):
        before=(self.cache/'alpha-1.0.0.crate').read_bytes()
        self.assertEqual(self.receipt['crates'],1);self.assertEqual(self.receipt['files'],5)
        self.module.verify(self.stage,self.spec)
        self.assertEqual((self.cache/'alpha-1.0.0.crate').read_bytes(),before)
        self.assertEqual(self.crate.stat().st_nlink,1);self.assertEqual(self.crate.stat().st_mode&0o777,0o400)
        with self.assertRaises(FileExistsError):self.module.stage(self.source,self.archives,self.cache,self.stage)

class HeartbeatTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.base=Path(self.tmp.name).resolve();self.base.chmod(0o700)
        self.root=self.base/'share';self.root.mkdir(mode=0o700)
        now=int(time.time());self.grant=dict(state='granted',owner='N',step='W0',start_unix=now,end_unix=now+2700,request_id='tick-test',generation=3)
        self.hb=load('heartbeat');self.log=self.base/'host.ndjson';self.logfd=os.open(self.log,os.O_CREAT|os.O_RDWR|os.O_NONBLOCK,0o600)
        self.host=self.hb.Host(self.root,self.grant,self.logfd);self.readfd,self.writefd=os.pipe();self.host.attach(self.writefd);self.host.start();self.guest=self.hb.Guest(self.root,self.grant,self.readfd)
    def tearDown(self):
        self.guest.close();self.host.close();os.close(self.readfd);os.close(self.writefd);os.close(self.logfd);self.tmp.cleanup()
    def test_every_stdin_tick_is_observed_even_when_all_host_share_copies_are_frozen(self):
        inode=self.guest.path.stat().st_ino
        for _ in range(20):
            self.guest.step();self.host.tick();self.assertEqual(self.guest.path.stat().st_ino,inode)
        self.guest.step(True);self.host.tick();self.assertTrue(self.guest.step(True));self.host.finish()
        self.assertTrue(self.host.closed);self.assertEqual(self.host.sequence,self.host.acknowledged)
        events=[json.loads(x) for x in self.log.read_text().splitlines()]
        self.assertEqual([x['sequence'] for x in events if x['event']=='publish' and x['active']],[x['sequence'] for x in events if x['event']=='acknowledged'])
        for fd in self.host.diag_fds:self.assertEqual(self.hb.decode(os.pread(fd,self.hb.SIZE,0),self.hb.LIVE)['sequence'],0)
        value,_=self.hb.live_read(self.guest.path,self.grant,int(time.time()));self.assertEqual(value['sequence'],self.guest.sequence)
    def test_unobserved_or_late_valid_ack_is_refused_and_silent_pipe_is_bounded(self):
        with patch.object(self.hb.time,'monotonic',return_value=self.host.sent+11):
            with self.assertRaisesRegex(ValueError,'deadline'):self.host.tick()
        self.guest.step()
        with patch.object(self.hb.time,'monotonic',return_value=self.host.sent+11):
            with self.assertRaisesRegex(ValueError,'deadline'):self.host.tick()
        with self.assertRaisesRegex(ValueError,'deadline'):self.hb.receive_frame(self.readfd,time.monotonic()+.02)
    def test_fragmented_stream_eof_and_corrupt_frames_fail_closed(self):
        raw=self.hb.receive_frame(self.readfd,time.monotonic()+1)
        for bad in [raw[:100],raw[:-1]+bytes([raw[-1]^1])]:
            with self.assertRaises(ValueError):self.hb.decode(bad,self.hb.LIVE)
        import threading
        r,w=os.pipe()
        def send():
            for i in range(0,len(raw),37):os.write(w,raw[i:i+37])
            os.close(w)
        t=threading.Thread(target=send);t.start()
        try:
            self.assertEqual(self.hb.receive_frame(r,time.monotonic()+1),raw)
            with self.assertRaisesRegex(ValueError,'EOF'):self.hb.receive_frame(r,time.monotonic()+1)
        finally:t.join();os.close(r)
    def test_forged_ack_and_sequence_gaps_are_not_adopted(self):
        self.host.tick();self.assertEqual(self.host.sequence,1)
        self.guest.step();ack,_=self.hb.read(self.host.ackfd,self.host.ack_path,self.hb.ACK)
        ack['frame_sha256']='0'*64;self.hb.write(self.host.ackfd,self.host.ack_path,self.hb.ACK,ack)
        with self.assertRaisesRegex(ValueError,'identity'):self.host.tick()
        value=dict(version=3,sequence=3,grant=self.grant,active=True,tick_unix=int(time.time()),closing=False)
        os.write(self.writefd,self.hb.frame(self.hb.LIVE,value))
        with self.assertRaisesRegex(ValueError,'sequence'):self.guest.step()
    def test_foreign_frames_and_guest_snapshot_staleness_are_refused(self):
        self.guest.step();value,_=self.hb.read(self.guest.fd,self.guest.path,self.hb.SNAP)
        with patch.object(self.hb.time,'monotonic_ns',return_value=value['observed_monotonic_ns']+3000000001):
            with self.assertRaisesRegex(ValueError,'stale'):self.hb.live_read(self.guest.path,self.grant,int(time.time()))
        value.pop('observed_monotonic_ns');value['sequence']=2;value['grant']=dict(self.grant,generation=4)
        os.write(self.writefd,self.hb.frame(self.hb.LIVE,value))
        with self.assertRaisesRegex(ValueError,'foreign'):self.guest.step()
    def test_replayed_inactive_and_replaced_snapshot_never_pass(self):
        raw=self.hb.receive_frame(self.readfd,time.monotonic()+1);value=self.hb.decode(raw,self.hb.LIVE)
        self.guest.step(initial=raw)
        value['tick_unix']+=1;os.write(self.writefd,self.hb.frame(self.hb.LIVE,value))
        with self.assertRaisesRegex(ValueError,'sequence'):self.guest.step()
        value.update(sequence=2,active=False);os.write(self.writefd,self.hb.frame(self.hb.LIVE,value))
        with self.assertRaisesRegex(ValueError,'revoked'):self.guest.step()
        snapshot=self.guest.path.read_bytes();self.guest.path.unlink()
        with self.assertRaises(OSError):self.hb.read(self.guest.fd,self.guest.path,self.hb.SNAP)
        self.guest.path.write_bytes(snapshot);self.guest.path.chmod(0o600)
        with self.assertRaisesRegex(ValueError,'identity'):self.hb.read(self.guest.fd,self.guest.path,self.hb.SNAP)
    def test_diagnostic_writer_is_separate_and_its_exit_does_not_stop_ticks(self):
        root=self.base/'diagnostic-share';root.mkdir(mode=0o700);read,write=os.pipe()
        host=self.hb.Host(root,self.grant,self.logfd);guest=None
        try:
            host.attach(write,diagnostic=True);host.start();guest=self.hb.Guest(root,self.grant,read)
            guest.step();host.tick();end=time.monotonic()+2
            while True:
                values=[self.hb.read(fd,path,self.hb.LIVE)[0]['sequence'] for fd,path in zip(host.diag_fds,host.diag_paths)]
                if values==[2]*4:break
                self.assertLess(time.monotonic(),end,'diagnostic writer did not sample host publication');time.sleep(.01)
            host.stop_diagnostic();guest.step();host.tick();guest.step();self.assertEqual(guest.sequence,3)
        finally:
            if guest:guest.close()
            host.close();os.close(read);os.close(write)
    def test_torn_shared_reads_retry_only_within_their_deadline(self):
        self.guest.step();raw=os.pread(self.host.ackfd,self.hb.SIZE,0);bad=raw[:-1]+bytes([raw[-1]^1])
        with patch.object(self.hb.os,'pread',side_effect=[bad,raw]):
            _,meta=self.hb.read(self.host.ackfd,self.host.ack_path,self.hb.ACK,time.monotonic()+.1)
        self.assertEqual(meta['read_attempts'],2)
        with patch.object(self.hb.os,'pread',return_value=bad):
            with self.assertRaises(self.hb.Torn):self.hb.read(self.host.ackfd,self.host.ack_path,self.hb.ACK,time.monotonic()+.02)
    def test_full_stdin_pipe_cannot_make_deactivation_unbounded(self):
        while True:
            try:os.write(self.host.channel,b'X'*4096)
            except BlockingIOError:break
        start=time.monotonic()
        with self.assertRaisesRegex(ValueError,'deadline'):self.host.deactivate(timeout=.02)
        self.assertLess(time.monotonic()-start,.3)
    def test_cache_diagnostics_are_nondecisive_and_include_every_method(self):
        self.hb.diagnostic(self.root,seconds=.02)
        rows=[json.loads(x) for x in (self.root/'share-diagnostic.ndjson').read_text().splitlines()]
        self.assertEqual({x['method'] for x in rows},set(self.hb.METHODS));self.assertTrue(all(x['diagnostic_only'] for x in rows))
        self.guest.step();self.host.tick();self.guest.step();self.assertEqual(self.guest.sequence,2)

if __name__=='__main__':unittest.main()
