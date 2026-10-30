import importlib.util,json,os,stat,tempfile,unittest,hashlib
from pathlib import Path
spec=importlib.util.spec_from_file_location('w0_runner',Path(__file__).parent/'devnet/w0/runner.py');runner=importlib.util.module_from_spec(spec);spec.loader.exec_module(runner)
class W0HostTests(unittest.TestCase):
 def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
 def tearDown(self):self.tmp.cleanup()
 def grant(self):return dict(state='granted',owner='N',step='W0',start_unix=100,end_unix=2800,request_id='window-1',generation=1)
 def test_grant_requires_current_generation_and_exact_window(self):
  good=self.grant();self.assertEqual(runner.validate_grant(good,101),good)
  for key,value in [('owner','E'),('state','requested'),('step','W1'),('generation',0),('generation',True),('end_unix',2801),('request_id','../escape')]:
   bad=dict(good,**{key:value})
   with self.assertRaises(runner.Refused):runner.validate_grant(bad,101)
  for now in [99,2671]:
   with self.assertRaises(runner.Refused):runner.validate_grant(good,now)
 def test_private_reads_refuse_aliases_links_and_extra_bytes(self):
  p=self.root/'input';p.write_bytes(b'ok');p.chmod(0o600);self.assertEqual(runner.read_private(p,2),b'ok')
  with self.assertRaises(runner.Refused):runner.read_private(p,1)
  q=self.root/'link';q.symlink_to(p)
  with self.assertRaises((runner.Refused,OSError)):runner.read_private(q,2)
  q.unlink();os.link(p,q)
  with self.assertRaises(runner.Refused):runner.read_private(p,2)
 def test_external_hardlinks_copy_via_new_fd_without_source_change(self):
  source=self.root/'source';source.write_bytes(b'compiled-static-fixture');alias=self.root/'alias';os.link(source,alias);before=source.stat();target=self.root/'probe'
  pin=runner.copy_external(source,target);self.assertEqual(pin,hashlib.sha256(source.read_bytes()).hexdigest());self.assertEqual(target.stat().st_nlink,1);self.assertEqual(source.stat().st_ctime_ns,before.st_ctime_ns);self.assertEqual(stat.S_IMODE(target.stat().st_mode),0o500)
  victim=self.root/'victim';victim.write_bytes(b'keep');symlink=self.root/'swap';symlink.symlink_to(victim)
  with self.assertRaises((runner.Refused,OSError)):runner.copy_external(source,symlink)
  self.assertEqual(victim.read_bytes(),b'keep')
 def test_bundle_paths_and_closed_fields_are_independently_bound(self):
  good=dict(version=1,backend=dict(version=1,probe=str(self.root/'probe'),comparator=str(self.root/'comparator'),nanoda=str(self.root/'nanoda'),probe_sha256='a'*64,bwrap='/usr/bin/bwrap',bwrap_sha256='cd6b143283b464baf078ab09ca19dadc5bcc2423b833c4f9dd7e83f9ba1b640b',cgroup='/sys/fs/cgroup/window',live_grant=str(self.root.parent/'live-grant.json'),grant_sha256='c'*64),grant=str(self.root/'grant.json'),grant_origin=str(self.root.parent/'host-grant.json'),lock=str(self.root/'window.lock'),source_sha256='b'*64)
  self.assertEqual(runner.parse_bundle(good,self.root,'aarch64'),good)
  for key,value in [('probe','/tmp/foreign'),('probe_sha256','0'*64),('bwrap','/bin/sh'),('cgroup','/sys/fs/cgroup')]:
   bad=dict(good,backend=dict(good['backend'],**{key:value}))
   with self.assertRaises(runner.Refused):runner.parse_bundle(bad,self.root,'aarch64')
  with self.assertRaises(runner.Refused):runner.parse_bundle(dict(good,unknown=True),self.root,'aarch64')

spec_window=importlib.util.spec_from_file_location('w0_window',Path(__file__).parent/'devnet/w0/window.py');window=importlib.util.module_from_spec(spec_window);spec_window.loader.exec_module(window)
class WindowTests(unittest.TestCase):
 def test_vm_share_is_reserved_in_full_and_never_over_cap(self):
  self.assertTrue(window.admit([dict(guard_mib=44880)]));self.assertFalse(window.admit([dict(guard_mib=44881)]))
 def test_release_is_after_a_confirmed_stop_even_when_guest_fails(self):
  events=[]
  class Ops:
   def validate(self):events.append('validate')
   def start(self):events.append('start')
   def guest(self):events.append('guest');raise RuntimeError('guest failure')
   def stop(self):events.append('stop')
   def stopped(self):events.append('stopped');return True
  class Lease:
   def acquire(self):events.append('acquire');return True
   def release(self):events.append('release')
  with self.assertRaises(RuntimeError):window.execute(Ops(),Lease())
  self.assertEqual(events,['validate','acquire','start','guest','stop','stopped','release'])
 def test_an_unconfirmed_stop_retains_the_vm_reservation(self):
  events=[]
  class Ops:
   def validate(self):pass
   def start(self):events.append('start')
   def guest(self):pass
   def stop(self):events.append('stop')
   def stopped(self):return False
  class Lease:
   def acquire(self):return True
   def release(self):events.append('release')
  with self.assertRaises(window.Refused):window.execute(Ops(),Lease())
  self.assertNotIn('release',events)

class InstanceTests(unittest.TestCase):
 def test_only_the_selected_ssd_share_can_be_writable(self):
  work=Path('/work');mount=dict(location=str(work/'w0'),mountPoint='/mnt/w0',writable=True)
  good=dict(status='Stopped',cpus=4,memory=4096*2**20,arch='aarch64',vmType='vz',config=dict(mountType='virtiofs',mounts=[mount],ssh=dict(forwardAgent=False,forwardX11=False,forwardX11Trusted=False),copyToHost=[],portForwards=[dict(guestIPMustBeZero=False,guestIP='127.0.0.1',guestPortRange=[1,65535],hostIP='127.0.0.1',hostPortRange=[1,65535],proto='any',ignore=True)],propagateProxyEnv=False,networks=[]))
  self.assertTrue(window.validate_instance(good,work,'ii'))
  bad=dict(good,config=dict(mounts=[mount,dict(location='/unrelated',mountPoint='/mnt/unrelated',writable=True)]))
  self.assertFalse(window.validate_instance(bad,work,'ii'))

class MoreWindowControls(unittest.TestCase):
 def test_missing_capacity_never_starts_or_releases_an_unheld_vm(self):
  events=[]
  class Ops:
   def validate(self):events.append('validate')
   def start(self):events.append('start')
  class Lease:
   def acquire(self):events.append('acquire');return False
   def release(self):events.append('release')
  with self.assertRaises(window.Refused):window.execute(Ops(),Lease())
  self.assertEqual(events,['validate','acquire'])
 def test_both_instance_options_enforce_cold_and_readonly_shares(self):
  s=Path('/work');rw=dict(location=str(s/'w0'),mountPoint='/mnt/w0',writable=True);cold=dict(location='/archive-ro',mountPoint='/archive-ro',writable=False)
  good=dict(status='Stopped',cpus=4,memory=4096*2**20,arch='aarch64',vmType='vz',config=dict(mountType='virtiofs',mounts=[rw,cold]))
  self.assertFalse(window.validate_instance(good,s,'i'));self.assertFalse(window.validate_instance(good,s,'ii'))
  for key,val in [('status','Running'),('cpus',8),('memory',32*2**30),('arch','x86_64')]:self.assertFalse(window.validate_instance(dict(good,**{key:val}),s,'i'))
  self.assertFalse(window.validate_instance(dict(good,config=dict(mounts=[rw,dict(cold,writable=True)])),s,'i'))
 def test_duplicate_json_and_nonlinux_execution_never_reach_a_probe(self):
  with self.assertRaises(runner.Refused):runner.decode(b'{"generation":0,"generation":1}')
  from unittest.mock import patch
  with patch.object(runner.platform,'system',return_value='Darwin'),patch.object(runner.os,'execve') as execute:
   with self.assertRaises(runner.Refused):runner.run(['probe-v1','SBX-network','window-1','/tmp/unused'])
   execute.assert_not_called()

class BundleFlowTests(unittest.TestCase):
 setUp=W0HostTests.setUp
 tearDown=W0HostTests.tearDown
 grant=W0HostTests.grant
 def test_materialized_bundle_fixed_exec_and_argv_are_bound_without_running_linux(self):
  from unittest.mock import patch
  from types import SimpleNamespace
  import time
  source=Path(__file__).resolve().parent.parent
  grant=self.grant();grant['start_unix']=int(time.time());grant['end_unix']=grant['start_unix']+2700
  origin=self.root/'host-grant.json';origin.write_text(json.dumps(grant));origin.chmod(0o600)
  runner.write_new(self.root/'live-grant.json',runner.heartbeat.frame(runner.heartbeat.SNAP,dict(version=3,sequence=1,grant=grant,active=True,tick_unix=int(time.time()),closing=False,observed_monotonic_ns=time.monotonic_ns())),0o600)
  binary=self.root/'compiled';binary.write_bytes(b'unit-fixture-only');binary.chmod(0o500)
  bundle=self.root/'bundle';expected=(runner.source_digest(source)+'\n').encode()
  with patch.object(runner.platform,'system',return_value='Linux'),patch.object(runner.platform,'machine',return_value='aarch64'),patch.object(runner.subprocess,'run',return_value=SimpleNamespace(stdout=expected,stderr=b'')) as build_identity:
   value=runner.prepare(source,binary,bundle,origin,'/sys/fs/cgroup/window')
   self.assertEqual(build_identity.call_args.args[0],[str(binary),'identity-v1'])
  self.assertEqual(runner.decode(runner.read_private(bundle/'bundle.json',16384)),value)
  scratch=self.root/'scratch';scratch.mkdir(mode=0o700)
  with patch.object(runner,'__file__',str(bundle/'runner.py')),patch.object(runner.platform,'system',return_value='Linux'),patch.object(runner.platform,'machine',return_value='aarch64'),patch.object(runner.os,'execve') as execute:
   runner.run(['probe-v1','SBX-network',grant['request_id'],str(scratch)])
   self.assertEqual(execute.call_count,1);args=execute.call_args.args
   self.assertEqual(args[0],str(bundle/'probe'));self.assertEqual(args[1][-1],str(bundle/'bundle.json'));self.assertEqual(set(args[2]),{'PATH','HOME','TMPDIR','LC_ALL'})
   changed=dict(grant,state='revoked');origin.write_text(json.dumps(changed))
   with self.assertRaises(runner.Refused):runner.run(['probe-v1','SBX-network',grant['request_id'],str(scratch)])
   self.assertEqual(execute.call_count,1)
 def test_guest_embedded_python_blocks_are_syntax_checked_only(self):
  import ast,re
  script=(Path(__file__).parent/'devnet/w0/guest.sh').read_text()
  count=0
  for match in re.finditer(r"<<'([A-Z_]+)'\n(.*?)\n\1",script,re.S):
   ast.parse(match.group(2));count+=1
  self.assertEqual(count,4)
if __name__=='__main__':unittest.main()
