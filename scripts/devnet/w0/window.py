#!/usr/bin/env python3
"""Future W0 window; host preparation never invokes run."""
import argparse,datetime,fcntl,hashlib,importlib.util,ipaddress,json,os,re,shutil,signal,stat,subprocess,time,sys,types
from pathlib import Path

# On a real launch, verify the unshared export before importing any sibling.
# The operator invokes this script from the pinned unshared export; no guest-
# writable script is ever imported or re-read after verification.
_VERIFIED={}
def bootstrap():
    args=sys.argv[1:]
    grant=Path(args[args.index('--grant')+1])
    def read(p,cap):
        fd=os.open(p,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        try:
            m=os.fstat(fd)
            if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1 or m.st_size>cap:raise ValueError('unsafe bootstrap file')
            with os.fdopen(os.dup(fd),'rb') as f:raw=f.read(cap+1)
            stamp=lambda v:(v.st_dev,v.st_ino,v.st_mode,v.st_uid,v.st_nlink,v.st_size,v.st_mtime_ns,v.st_ctime_ns)
            if len(raw)>cap or stamp(os.fstat(fd))!=stamp(m):raise ValueError('bootstrap file changed')
            return raw
        finally:os.close(fd)
    value=json.loads(read(grant,16384));root=Path(value['work_root'])/'host-source'/('source-'+value['source_commit'][:12])
    if root.resolve()!=root or root.is_relative_to(Path(value['share_root'])) or Path(__file__).resolve()!=root/'scripts/devnet/w0/window.py':raise ValueError('unshared reviewed export required')
    raw=read(root/'SOURCE.json',4*1024**2)
    if hashlib.sha256(raw).hexdigest()!=value['source_manifest_sha256']:raise ValueError('bootstrap manifest pin')
    manifest=json.loads(raw)
    if manifest['commit']!=value['source_commit']:raise ValueError('bootstrap commit pin')
    for name in ('window','authority','runner','custody','heartbeat','inputs'):
        key='scripts/devnet/w0/'+name+'.py';data=read(root/key,1048576)
        if hashlib.sha256(data).hexdigest()!=manifest['files'][key]:raise ValueError('bootstrap script pin')
        _VERIFIED[name]=data
    return root
_BOOT_ROOT=bootstrap() if __name__=='__main__' else None

def load(name,root=None):
    p=(Path(__file__).parent if root is None else Path(root))/f'{name}.py'
    if _VERIFIED:
        mod=types.ModuleType('w0_'+name);mod.__file__=str(p)
        if name=='runner':mod._W0_HEARTBEAT=heartbeat
        exec(compile(_VERIFIED[name],str(p),'exec'),mod.__dict__);return mod
    spec=importlib.util.spec_from_file_location('w0_'+name,p)
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod
authority=load('authority');custody=load('custody');heartbeat=load('heartbeat');inputs=load('inputs');Refused=authority.Refused

def admit(rows):
    if not isinstance(rows,list) or any(not isinstance(v,dict) or type(v.get('guard_mib')) is not int or v['guard_mib']<0 for v in rows):
        raise Refused('invalid shared leases')
    return sum(v['guard_mib'] for v in rows)+5120<=50000

def udp_names(raw):
    lines=raw.decode(errors='replace').splitlines();names=[];unknown=False
    if lines:
        header=lines[0].split()
        if 'NAME' not in header:unknown=True
        else:
            index=header.index('NAME')
            for line in lines[1:]:
                columns=line.split(None,index)
                if len(columns)<=index:unknown=True;continue
                name=columns[index]
                # NAME is the final lsof column; UDP's local endpoint precedes ->.
                local=name.split('->',1)[0]
                address=local.rsplit(':',1)[0].strip('[]')
                try:loopback=ipaddress.ip_address(address).is_loopback
                except ValueError:
                    if address=='*':loopback=False
                    else:unknown=True;loopback=None
                names.append(dict(name=name,loopback=loopback))
    return dict(udp_endpoints=names,loopback_udp_observed=any(v['loopback'] is True for v in names),non_loopback_udp_observed=any(v['loopback'] is False for v in names),unclassified_udp_observed=unknown)

def validate_instance(row,work,option,guest_mount='/mnt/w0'):
    config=row.get('config',{})
    if (option!='ii' or row.get('status')!='Stopped' or row.get('cpus')!=4 or row.get('memory')!=4096*2**20
            or row.get('arch')!='aarch64' or row.get('vmType')!='vz' or config.get('mountType')!='virtiofs' or config.get('additionalDisks') not in (None,[])):
        return False
    ssh=config.get('ssh',{})
    port=[dict(guestIPMustBeZero=False,guestIP='127.0.0.1',guestPortRange=[1,65535],hostIP='127.0.0.1',hostPortRange=[1,65535],proto='any',ignore=True)]
    if (not isinstance(ssh,dict) or any(ssh.get(k) is not False for k in ('forwardAgent','forwardX11','forwardX11Trusted'))
            or config.get('copyToHost') not in (None,[]) or config.get('portForwards')!=port
            or config.get('propagateProxyEnv') is not False or config.get('networks') not in (None,[])):return False
    mounts=config.get('mounts',[])
    if not isinstance(mounts,list) or len(mounts)!=1 or not isinstance(mounts[0],dict):return False
    mount=mounts[0]
    # Lima materializes defaults for inactive transports in every Info record.
    # Inspect the one share boundary, not dictionary equality with those fields.
    if set(mount)-{'location','mountPoint','writable','9p','sshfs'}:return False
    sshfs=mount.get('sshfs',{})
    return (isinstance(sshfs,dict) and sshfs.get('followSymlinks',False) is False
            and mount.get('location')==str(Path(work)/'w0') and mount.get('mountPoint')==guest_mount and mount.get('writable') is True)

def execute(ops,lease):
    ops.validate()
    started=False;held=False;attempting=True
    try:
        held=lease.acquire();attempting=False
        if not held:raise Refused('no capacity; no VM started and no reservation held')
        if hasattr(ops,'prepare'):ops.prepare()
        started=True
        ops.start()
        return ops.guest()
    finally:
        if started:
            stop_error=None
            try:ops.stop()
            except BaseException as error:stop_error=error
            confirmed=False
            try:confirmed=ops.stopped()
            except BaseException as error:
                if stop_error is None:stop_error=error
            if not confirmed:
                try:
                    if hasattr(ops,'unconfirmed'):ops.unconfirmed()
                except OSError as error:
                    if stop_error is None:stop_error=error
                raise Refused('VM_STOP_UNCONFIRMED: keep own reservation') from stop_error
            try:
                if hasattr(ops,'released'):ops.released()
            finally:lease.release()
            if stop_error:raise stop_error
        elif held or attempting:
            try:
                if hasattr(ops,'released'):ops.released()
            finally:lease.release()

class Lease:
    def __init__(self,path,name):
        self.path,self.name,self.pid=Path(path),name,os.getpid()
        self.birth=subprocess.check_output(['ps','-p',str(self.pid),'-o','lstart='],env={'PATH':'/usr/bin:/bin','LC_ALL':'C'},text=True).strip()
    def update(self,add):
        with os.fdopen(os.open(self.path.with_suffix('.lock'),os.O_RDWR|os.O_CREAT|os.O_NOFOLLOW|os.O_CLOEXEC,0o600),'r+') as lock:
            m=os.fstat(lock.fileno())
            if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1:raise Refused('shared lease lock identity')
            fcntl.flock(lock,fcntl.LOCK_EX)
            rows=authority.decode(self.path.read_bytes())
            own=lambda v:v.get('name')==self.name and v.get('pid')==self.pid and v.get('pid_birth')==self.birth
            if add:
                if any(own(v) for v in rows) or not admit(rows):return False
                rows.append(dict(session='N',task='M3D-B2-W0',name=self.name,pid=self.pid,pid_birth=self.birth,guard_mib=5120,start_utc=datetime.datetime.now(datetime.timezone.utc).isoformat()))
            else:rows=[v for v in rows if not own(v)]
            authority.atomic(self.path,(json.dumps(rows,separators=(',',':'))+'\n').encode())
        return True
    def acquire(self):return self.update(True)
    def release(self):self.update(False)

def validate_roots(value,grant_path):
    keys=('work_root','share_root','guest_mount','lima_home','host_home','guest_home','limactl','windows','windows_lock','quiet_lock')
    for key in keys:
        p=Path(value[key])
        if not p.is_absolute() or p==Path('/') or str(p)!=os.path.normpath(str(p)) or '..' in p.parts:
            raise Refused('canonical authority roots required')
    for key in ('work_root','share_root','lima_home','host_home','limactl','windows','windows_lock','quiet_lock'):
        p=Path(value[key])
        if p.resolve()!=p:raise Refused('host authority roots may not traverse symlinks')
    anchor=Path(grant_path).parent
    if any(Path(value[k])!=anchor/n for k,n in [('windows','VM-WINDOWS.json'),('windows_lock','VM-WINDOWS.lock'),('quiet_lock','VM-QUIET.lock')]):raise Refused('adopted ledger and locks must be beside the external authority')
    work=Path(value['work_root']);share=Path(value['share_root']);home=Path(value['lima_home'])
    if share!=work/'w0' or home.is_relative_to(share) or share.is_relative_to(home):raise Refused('VM home/disk must be outside every share')
    for p in [Path(grant_path),Path(value['windows']),Path(value['windows_lock']),Path(value['quiet_lock'])]:
        if p.is_relative_to(work):raise Refused('authority must remain outside work/share')
    if not value['guest_mount'].startswith('/mnt/') or not value['guest_home'].startswith('/home/') or Path(value['host_home']).is_relative_to(work) or Path(value['limactl']).is_relative_to(work):raise Refused('private host runtime and bounded guest roots required')
    if value['option']!='ii':raise Refused('only the selected offline clone is allowed')

class Operations:
    def __init__(self,options,value,runner):
        self.options,self.authority,self.runner=options,value,runner
        self.grant=value['grant'];self.original=Path(options.grant).read_bytes()
        self.work=Path(value['work_root']).resolve();self.device=self.work.stat().st_dev
        self.run=self.work/'w0'/('run-'+self.grant['request_id']+'-'+str(self.grant['generation']))
        self.source=self.work/'host-source'/('source-'+value['source_commit'][:12])
        self.env={'PATH':'/opt/homebrew/bin:/usr/bin:/bin','HOME':value['host_home'],'LIMA_HOME':value['lima_home'],'LC_ALL':'C','HTTP_PROXY':'http://127.0.0.1:9','HTTPS_PROXY':'http://127.0.0.1:9','NO_PROXY':'localhost,127.0.0.1,::1'}
        self.lima=value['limactl'];self.logs={};self.claimed=False;self.activity_fd=None
        self.heartbeat=None;self.custody_claimed=False;self.custody_marker=self.work/'host-runs'/'UNSEALED'
        self.owner=str(os.getpid())+'-'+str(time.monotonic_ns())
    def command(self,*args):return [self.lima,'--tty=false',*args]
    def info(self):
        raw=subprocess.check_output(self.command('list','--json',self.authority['vm']),env=self.env,text=True,timeout=10)
        rows=[authority.decode(line) for line in raw.splitlines() if line.strip()]
        if len(rows)!=1 or rows[0].get('name')!=self.authority['vm'] or rows[0].get('dir')!=str(Path(self.authority['lima_home'])/self.authority['vm']):raise Refused('exact existing instance required')
        return rows[0]
    def stopped(self):return self.info().get('status')=='Stopped'
    def validate(self):
        validate_roots(self.authority,self.options.grant)
        self.validate_journals()
        if self.custody_marker.exists() or self.custody_marker.is_symlink():raise Refused('previous custody incomplete; preserve and resolve before rebooking')
        if Path(self.options.work).resolve()!=self.work:raise Refused('authority work root mismatch')
        self.runner.validate_grant(self.grant,int(time.time()))
        if self.grant['end_unix']-time.time()<(600 if self.grant['step']=='W0-MASK' else 2600):raise Refused('full boot/build/cleanup window required')
        self.guard()
        if not validate_instance(self.info(),self.work,self.authority['option'],self.authority['guest_mount']):raise Refused('exact stopped clone/mounts required')
        vm=Path(self.authority['lima_home'])/self.authority['vm']
        if vm.resolve()!=vm or vm.is_symlink():raise Refused('canonical unshared VM directory required')
        for name in ['disk','lima.yaml']:
            p=vm/name
            try:m=p.lstat()
            except OSError as e:raise Refused('offline existing VM file unavailable') from e
            if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1 or m.st_mode&0o022 or p.resolve().parent!=vm or p.resolve().is_relative_to(self.work/'w0'):
                raise Refused('VM disk/config must be owned, unaliased and unshared')
        manifest=authority.read(self.source/'SOURCE.json',4*1024**2)
        if hashlib.sha256(manifest).hexdigest()!=self.authority['source_manifest_sha256'] or authority.decode(manifest)['commit']!=self.authority['source_commit']:
            raise Refused('reviewed source export missing')
        self.guard()
    def validate_journals(self):
        for name in ['activity','leases']:
            p=Path(getattr(self.options,name))
            if not p.is_absolute() or p.resolve()!=p or p.is_relative_to(self.work):raise Refused('shared host journals must stay outside work/share')
            fd=self.runner.parent_fd(p);os.close(fd)
            if p.exists():
                # Activity may be appended by another session. Validate the held
                # file identity, not a racing snapshot of unrelated event bytes.
                fd=os.open(p,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
                try:
                    m=os.fstat(fd)
                    if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1:raise Refused('shared host journal identity')
                finally:os.close(fd)
    def prepare_logs(self):
        self.validate_journals()
        self.activity_fd=os.open(self.options.activity,os.O_WRONLY|os.O_APPEND|os.O_NOFOLLOW|os.O_CLOEXEC)
        m=os.fstat(self.activity_fd)
        if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1:raise Refused('shared activity journal identity')
        directory=self.work/'host-logs'/self.run.name
        directory.parent.mkdir(mode=0o700,exist_ok=True);directory.mkdir(mode=0o700)
        self.runner.private_directory(directory)
        d=self.runner.parent_fd(directory/'placeholder')
        try:
            for name in ['vm-start','guest-w0','vm-stop','vm-stop-owned-force','VM_STOP_UNCONFIRMED','udp-sockets','live-host','build-inputs']:
                self.logs[name]=os.open(name+'.log',os.O_RDWR|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=d)
            os.fsync(d)
        finally:os.close(d)
    def stage_source(self):
        custody.stage(self.source,self.run/'source',self.authority['source_manifest_sha256'],self.authority['source_commit'])
    def stage_build_inputs(self):
        if self.grant['step']!='W0':return
        self.activity('start','stage-build-inputs',environment_names=sorted(self.env));receipt=None
        try:
            receipt=inputs.stage(self.source,self.work/'w0-toolchain'/'dl-rust-1.95.0-aarch64-linux',Path(self.authority['host_home'])/'.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f',self.run/'build-inputs',tick=self.guard,deadline=time.monotonic()+60)
            os.write(self.logs['build-inputs'],inputs.canonical(receipt));os.fsync(self.logs['build-inputs'])
        finally:self.activity('end','stage-build-inputs',success=receipt is not None)
    def reserve_custody(self):
        self.custody_marker.parent.mkdir(mode=0o700,exist_ok=True);self.runner.private_directory(self.custody_marker.parent)
        self.runner.write_new(self.custody_marker,self.owner.encode(),0o400);self.custody_claimed=True
        fd=os.open(self.custody_marker.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW);os.fsync(fd);os.close(fd)
    def prepare(self):
        self.guard();self.prepare_logs();self.reserve_custody()
        self.run.mkdir(mode=0o700);self.runner.private_directory(self.run)
        self.stage_source();self.stage_build_inputs()
        self.runner.write_new(self.run/'host-grant.json',(json.dumps(self.grant,separators=(',',':'))+'\n').encode(),0o400)
        self.heartbeat=heartbeat.Host(self.run,self.grant,self.logs['live-host'])
        authority.ledger(self.authority['windows'],self.authority['windows_lock'],self.grant,self.owner,event='CONSUMED');self.claimed=True
    def live(self,active):
        if active:
            if self.heartbeat is None:self.heartbeat=heartbeat.Host(self.run,self.grant,self.logs['live-host'])
            if self.heartbeat.started:self.heartbeat.tick()
        elif self.heartbeat is not None:self.heartbeat.deactivate()
    def guard(self,publish=True):
        authority.ledger(self.authority['windows'],self.authority['windows_lock'],self.grant,self.owner)
        if not self.work.exists() or self.work.stat().st_dev!=self.device:raise Refused('work device disconnected')
        if shutil.disk_usage(self.work).free<200*2**30 or shutil.disk_usage(Path(self.options.leases).parent).free<20*2**30:raise Refused('disk admission failed')
        if authority.read(self.options.grant,16384)!=self.original or time.time()>=self.grant['end_unix']-120:raise Refused('window revoked/expired')
        if publish and self.claimed and self.heartbeat is not None and self.heartbeat.started:self.live(True)
    def activity(self,event,label,**fields):
        if self.activity_fd is None:raise Refused('shared activity journal not opened before boot')
        with os.fdopen(os.dup(self.activity_fd),'a') as stream:
            fcntl.flock(stream,fcntl.LOCK_EX)
            try:
                if "argv" in fields:fields["argv"]=[re.sub(r"^([A-Z][A-Z0-9_]*)=.*",r"\1=<set>",v) for v in fields["argv"]]
                stream.write(json.dumps(dict(utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),session='N',task='M3D-B2-W0',event=event,label=label,nice=19,jobs=2,**fields))+'\n');stream.flush();os.fsync(stream.fileno())
            finally:fcntl.flock(stream,fcntl.LOCK_UN)
    def close(self):
        if self.heartbeat is not None:self.heartbeat.close()
        for fd in [*self.logs.values(),self.activity_fd]:
            if fd is not None:
                try:os.close(fd)
                except OSError:pass
        self.logs={};self.activity_fd=None
    def call(self,label,args,maximum,cleanup=False):
        if label not in self.logs:raise Refused('host log was not exclusively pre-created before boot')
        try:self.activity('start',label,argv=args,environment_names=sorted(self.env))
        except OSError:
            if not cleanup:raise
        process=None
        try:
            if not cleanup:self.guard()
            with os.fdopen(os.dup(self.logs[label]),'wb') as output:
                process=subprocess.Popen(['/usr/bin/nice','-n','19',*args],env=self.env,stdin=subprocess.PIPE if label=='guest-w0' else subprocess.DEVNULL,stdout=output,stderr=subprocess.STDOUT,start_new_session=True)
                if label=='guest-w0':
                    if self.heartbeat is None:raise Refused('guest heartbeat was not prepared')
                    self.heartbeat.attach(process.stdin.fileno(),diagnostic=self.grant['step']=='W0-MASK');self.heartbeat.start()
                deadline=time.monotonic()+maximum
                while process.poll() is None:
                    if not cleanup:self.guard()
                    if time.monotonic()>=deadline:raise Refused('bounded VM command timeout')
                    time.sleep(.2)
                if process.returncode:raise Refused('VM command failed: '+label)
                if not cleanup:
                    self.guard()
                    if label=='guest-w0':
                        if self.heartbeat is None:raise Refused('guest heartbeat was not started')
                        self.heartbeat.finish(self.guard)
        finally:
            if process is not None and process.poll() is None:
                os.killpg(process.pid,signal.SIGKILL);process.wait()
            if process is not None and getattr(process,'stdin',None) is not None:process.stdin.close()
            try:self.activity('end',label,exit_code=process.returncode if process else None)
            except OSError:
                if not cleanup:raise
    def capture_udp(self):
        # Read-only post-READY samples, before the guest command. The old log's
        # forwarding message alone cannot establish whether bind succeeded.
        pid_text=custody.read(Path(self.authority['lima_home'])/self.authority['vm']/'ha.pid',64).decode().strip()
        if not re.fullmatch(r'[1-9][0-9]{0,9}',pid_text):raise Refused('hostagent PID evidence missing')
        def sample(args):
            label='udp-'+Path(args[0]).name;result=None
            self.activity('start',label,argv=args,environment_names=sorted(self.env))
            try:
                result=subprocess.run(args,env=self.env,capture_output=True,timeout=2,check=False)
                if len(result.stdout)+len(result.stderr)>65536:raise Refused('UDP capture output bound')
                return result
            finally:self.activity('end',label,exit_code=result.returncode if result else None)
        def ps(field):
            result=sample(['/bin/ps','-p',pid_text,'-o',field])
            if result.returncode:raise Refused('hostagent ps capture unavailable')
            return result.stdout.decode().strip()
        def birth():return ps('lstart=')
        owner=ps('uid=,comm=').split(None,1)
        if len(owner)!=2 or owner[0]!=str(os.geteuid()) or Path(owner[1]).name!='limactl':raise Refused('hostagent process ownership/command mismatch')
        expected=birth()
        if not expected:raise Refused('hostagent not alive for UDP capture')
        for sample_index in range(3):
            self.guard()
            if birth()!=expected:raise Refused('hostagent PID identity changed')
            result=sample(['/usr/sbin/lsof','-nP','-a','-p',pid_text,'-iUDP'])
            if birth()!=expected or result.returncode not in (0,1) or len(result.stdout)+len(result.stderr)>65536:raise Refused('UDP capture unavailable/identity/bound')
            row=dict(version=2,sample=sample_index,utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),pid=int(pid_text),pid_birth=expected,exit_code=result.returncode,stdout=result.stdout.decode(errors='replace'),stderr=result.stderr.decode(errors='replace'),**udp_names(result.stdout),scope='hostagent at this instant; not proof of never bound')
            raw=(json.dumps(row,separators=(',',':'))+'\n').encode()
            with os.fdopen(os.dup(self.logs['udp-sockets']),'ab') as output:output.write(raw);output.flush();os.fsync(output.fileno())
            time.sleep(.1)
    def start(self):
        self.call('vm-start',self.command('start','--timeout=5m',self.authority['vm']),300)
        self.capture_udp()
    def guest(self):
        mount=self.authority['guest_mount'];run=mount+'/'+self.run.name;source=run+'/source'
        args=self.command('shell',self.authority['vm'],'--','/usr/bin/env','-i','PATH=/usr/bin:/bin','HOME='+self.authority['guest_home'],'LC_ALL=C','/usr/bin/nice','-n','19','/bin/bash',source+'/scripts/devnet/w0/guest.sh',source,run,self.authority['source_commit'],self.authority['source_manifest_sha256'],mount)
        self.call('guest-w0',args,min(1980,max(1,self.grant['end_unix']-time.time()-300)))
    def stop(self):
        try:
            try:
                if self.heartbeat is not None:self.heartbeat.deactivate(timeout=.2)
            except (OSError,ValueError):pass
        finally:
            # No share read, write, log open or flock may precede this finally.
            try:
                try:self.call('vm-stop',self.command('stop',self.authority['vm']),60,cleanup=True)
                except BaseException:self.call('vm-stop-owned-force',self.command('stop','--force',self.authority['vm']),30,cleanup=True)
            finally:
                if self.heartbeat is not None:self.heartbeat.stop_diagnostic()
    def unconfirmed(self):
        fd=self.logs.get('VM_STOP_UNCONFIRMED')
        if fd is not None:os.write(fd,(json.dumps(dict(vm=self.authority['vm'],owner=self.owner,request_id=self.grant['request_id']))+'\n').encode());os.fsync(fd)
    def released(self):
        try:
            if self.run.exists():
                parent=self.work/'host-runs';parent.mkdir(mode=0o700,exist_ok=True);self.runner.private_directory(parent)
                logs=self.work/'host-logs'/self.run.name
                custody.seal(self.run,parent/self.run.name,logs/'run-hashes.json')
            if self.custody_claimed:
                if authority.read(self.custody_marker,1024)!=self.owner.encode():raise Refused('custody marker identity changed')
                self.custody_marker.unlink();self.custody_claimed=False
                fd=os.open(self.custody_marker.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW);os.fsync(fd);os.close(fd)
        finally:
            try:
                for fd in self.logs.values():os.fchmod(fd,0o400);os.fsync(fd)
            finally:
                if self.claimed:authority.ledger(self.authority['windows'],self.authority['windows_lock'],self.grant,self.owner,event='RELEASED');self.claimed=False

def interrupted(_signal,_frame):
    for sig in [signal.SIGTERM,signal.SIGHUP,signal.SIGINT]:signal.signal(sig,signal.SIG_IGN)
    raise Refused('window interrupted; stop owned VM before release')

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('mode',choices=['run'])
    for name in ['grant','leases','quiet-lock','work','activity']:parser.add_argument('--'+name,required=True)
    options=parser.parse_args();value=authority.decode(authority.read(options.grant,16384))
    keys={'grant','vm','lima_home','option','source_commit','source_manifest_sha256','quiet_acknowledged_by','work_root','share_root','guest_mount','host_home','guest_home','limactl','windows','windows_lock','quiet_lock'}
    if set(value)!=keys or not re.fullmatch(r'[a-z0-9][a-z0-9-]{1,62}',value['vm']) or not re.fullmatch('[0-9a-f]{40}',value['source_commit']) or not re.fullmatch('[0-9a-f]{64}',value['source_manifest_sha256']) or set(value['quiet_acknowledged_by'])!={'E','orchestrator'}:raise Refused('closed external authority required')
    validate_roots(value,options.grant)
    if Path(options.quiet_lock)!=Path(value['quiet_lock']):raise Refused('shared quiet lock mismatch')
    source=Path(value['work_root'])/'host-source'/('source-'+value['source_commit'][:12])
    custody.verified(source,value['source_manifest_sha256'],value['source_commit'])
    runner=load('runner',source/'scripts/devnet/w0');runner.validate_grant(value['grant'],int(time.time()))
    # The quiet lock is also operator-created, outside all guest mounts.
    quiet=os.open(options.quiet_lock,os.O_RDWR|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        q=os.fstat(quiet)
        if not stat.S_ISREG(q.st_mode) or q.st_uid!=os.geteuid() or q.st_nlink!=1 or q.st_mode&0o077:raise Refused("private shared quiet lock required")
        fcntl.flock(quiet,fcntl.LOCK_EX|fcntl.LOCK_NB)
        for sig in [signal.SIGTERM,signal.SIGHUP,signal.SIGINT]:signal.signal(sig,interrupted)
        ops=Operations(options,value,runner)
        try:execute(ops,Lease(options.leases,'m3d-b2-w0-window-'+value['grant']['request_id']+'-'+str(value['grant']['generation'])))
        finally:ops.close()
    finally:os.close(quiet)

if __name__=='__main__':main()
