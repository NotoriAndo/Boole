"""Read-only W0 observations, not containment evidence or new admission gates."""
import ctypes,hashlib,json,os,platform,re,resource,shutil,stat,subprocess,time
from pathlib import Path

PROFILE_PIN='35613442bd82247d132b1102579edc5567d8047930d1b60fbb8bbba93c5abcd6'
SYSCTL='/proc/sys/kernel/apparmor_restrict_unprivileged_userns'
TOOLCHAIN='1.95.0-aarch64-unknown-linux-gnu'

class Inspector:
    def __init__(self,seconds=60):
        if platform.system()!='Linux':raise ValueError('Linux observations only; no host syscall fallback')
        self.deadline=time.monotonic()+seconds
    def remaining(self):
        value=self.deadline-time.monotonic()
        if value<=0:raise TimeoutError('observation budget exhausted')
        return value
    def read(self,path,cap=65536):
        self.remaining();fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        try:
            m=os.fstat(fd)
            if not stat.S_ISREG(m.st_mode) or m.st_size>cap:raise ValueError('observation file kind/size')
            with os.fdopen(os.dup(fd),'rb') as stream:raw=stream.read(cap+1)
            if len(raw)>cap:raise ValueError('observation read bound')
            self.remaining();return raw
        finally:os.close(fd)
    def command(self,args,user_bus=False):
        env={'PATH':'/usr/bin:/bin','LC_ALL':'C','PYTHONDONTWRITEBYTECODE':'1','RUSTUP_AUTO_INSTALL':'0','CARGO_NET_OFFLINE':'true'}
        if user_bus:
            env.update(XDG_RUNTIME_DIR=f'/run/user/{os.geteuid()}',DBUS_SESSION_BUS_ADDRESS=f'unix:path=/run/user/{os.geteuid()}/bus')
        result=subprocess.run(args,env=env,stdin=subprocess.DEVNULL,capture_output=True,timeout=min(3,self.remaining()),check=False)
        if len(result.stdout)+len(result.stderr)>65536:raise ValueError('observation command output bound')
        return dict(exit_code=result.returncode,stdout=result.stdout.decode(errors='replace'),stderr=result.stderr.decode(errors='replace'),environment_names=sorted(env))
    def syscall(self,number,*args):
        self.remaining();libc=ctypes.CDLL(None,use_errno=True);libc.syscall.restype=ctypes.c_long;ctypes.set_errno(0)
        value=libc.syscall(ctypes.c_long(number),*args)
        return dict(value=value,errno=ctypes.get_errno() if value<0 else None)

def observation(call):
    try:return dict(status='observed',value=call())
    except FileNotFoundError:return dict(status='absent',value=None)
    except Exception as error:
        return dict(status='unavailable',value=None,error=type(error).__name__,errno=getattr(error,'errno',None))

def kernel_identity(inspector=None):
    i=inspector or Inspector(15)
    text=lambda path,cap=65536:observation(lambda:i.read(path,cap).decode(errors='replace').strip())
    row=dict(version=1,informational_only=True,release=observation(lambda:i.command(['/usr/bin/uname','-r'])),version_string=observation(lambda:i.command(['/usr/bin/uname','-v'])),lsm=text('/sys/kernel/security/lsm'),apparmor_enabled=text('/sys/module/apparmor/parameters/enabled'),user_max_user_namespaces=text('/proc/sys/user/max_user_namespaces',64),userns_sysctl=text(SYSCTL,32),landlock_abi=observation(lambda:i.syscall(444,ctypes.c_void_p(0),ctypes.c_size_t(0),ctypes.c_uint(1))))
    restriction=row['userns_sysctl']
    row['ubuntu_restriction_configured']=restriction['status']=='observed' and restriction['value']=='1'
    row['userns_rule']='kernel without the Ubuntu userns restriction' if restriction['status']=='absent' else 'present must equal 1; never changed by inventory'
    row['profile_file_sha256']=observation(lambda:hashlib.sha256(i.read('/etc/apparmor.d/boole-gate-bwrap')).hexdigest())
    row['profile_file_matches_pin']=row['profile_file_sha256']['value']==PROFILE_PIN
    profiles=text('/sys/kernel/security/apparmor/profiles')
    if profiles['status']=='unavailable':
        result=observation(lambda:i.command(['/usr/bin/sudo','-n','/bin/cat','/sys/kernel/security/apparmor/profiles']))
        if result['status']=='observed' and result['value']['exit_code']==0:profiles=dict(status='observed',value=result['value']['stdout'])
    loaded=[]
    if profiles['status']=='observed':
        loaded=[line.strip() for line in profiles['value'].splitlines() if line.startswith('boole-gate-bwrap (')]
    row['profile_loaded']=bool(loaded) if profiles['status']=='observed' else None
    row['profile_modes']=[v.removeprefix('boole-gate-bwrap (').removesuffix(')') for v in loaded]
    row['profile_query_status']=profiles['status']
    return row

def kernel_support(i,kernel):
    release=kernel['release'];config={}
    if release['status']=='observed' and release['value']['exit_code']==0:
        name=release['value']['stdout'].strip()
        if re.fullmatch(r'[A-Za-z0-9_.+-]{1,128}',name):
            config=observation(lambda:i.read('/boot/config-'+name,1024**2).decode())
    wanted=('CONFIG_NAMESPACES','CONFIG_MULTIUSER','CONFIG_USER_NS','CONFIG_NET_NS','CONFIG_PID_NS','CONFIG_IPC_NS','CONFIG_UTS_NS','CONFIG_CGROUPS','CONFIG_CGROUP_PIDS','CONFIG_MEMCG','CONFIG_MEMCG_SWAP','CONFIG_CGROUP_FREEZER','CONFIG_SECCOMP','CONFIG_SECCOMP_FILTER','CONFIG_SECURITY_APPARMOR','CONFIG_SECURITY_LANDLOCK','CONFIG_TMPFS','CONFIG_PROC_FS')
    raw=config.get('value') or '';values=dict(line.split('=',1) for line in raw.splitlines() if '=' in line and not line.startswith('#'))
    # Invalid argument probes cannot create children, close fds or send a signal.
    calls={'clone3_invalid_size':(435,ctypes.c_void_p(0),ctypes.c_size_t(0)),
           'close_range_invalid_order':(436,ctypes.c_uint(2**32-1),ctypes.c_uint(0),ctypes.c_uint(0)),
           'pidfd_open_invalid_pid':(434,ctypes.c_int(-1),ctypes.c_uint(0)),
           'pidfd_signal_invalid_fd':(424,ctypes.c_int(-1),ctypes.c_int(0),ctypes.c_void_p(0),ctypes.c_uint(0))}
    limits=observation(lambda:{name:resource.getrlimit(getattr(resource,key)) for name,key in [('nofile','RLIMIT_NOFILE'),('nproc','RLIMIT_NPROC'),('address_space','RLIMIT_AS'),('stack','RLIMIT_STACK'),('core','RLIMIT_CORE')]})
    return dict(informational_only=True,process_limits=limits,namespace_handles={name:observation(lambda name=name:os.readlink('/proc/self/ns/'+name)) for name in ('user','net','pid','mnt','ipc','uts')},devices={name:observation(lambda name=name:stat.S_ISCHR(os.lstat('/dev/'+name).st_mode)) for name in ('null','urandom')},entropy_available=observation(lambda:i.read('/proc/sys/kernel/random/entropy_avail',64).decode().strip()),config_status=config.get('status','unavailable'),config={k:values.get(k) for k in wanted},syscall_availability={name:observation(lambda args=args:i.syscall(*args)) for name,args in calls.items()},seccomp_actions=observation(lambda:i.read('/proc/sys/kernel/seccomp/actions_avail',4096).decode().strip()),self_status=observation(lambda:'\n'.join(line for line in i.read('/proc/self/status').decode().splitlines() if line.startswith(('Seccomp:','Seccomp_filters:','NoNewPrivs:')))))

def registry(i,home,source):
    import tomllib  # Optional observation; older interpreters report unavailable.
    packages=tomllib.loads(i.read(source/'Cargo.lock',4*1024**2).decode())['package'];by_name={}
    for p in packages:by_name.setdefault(p['name'],[]).append(p)
    pending=list(by_name['boole-gate-supervisor']);seen={};ambiguous=[]
    while pending:
        p=pending.pop();key=(p['name'],p['version'])
        if key in seen:continue
        seen[key]=p
        for dep in p.get('dependencies',[]):
            parts=dep.split();matches=[q for q in by_name.get(parts[0],[]) if len(parts)==1 or q['version']==parts[1]]
            if len(matches)!=1:ambiguous.append(dep)
            pending.extend(matches)
    roots=home/'.cargo/registry';directories={}
    for kind in ('cache','src','index'):
        directories[kind]=[p for p in (roots/kind).iterdir() if p.is_dir() and not p.is_symlink()][:32] if (roots/kind).is_dir() else []
    rows=[]
    required=[p for p in sorted(seen.values(),key=lambda p:(p['name'],p['version'])) if p.get('source','').startswith('registry+')]
    for p in required:
        name,version=p['name'],p['version'];expected=p['checksum'];entry=dict(name=name,version=version,archive_verified=False,source_manifest_pin=False,index_row=False)
        try:
            i.remaining()
            for directory in directories['cache']:
                archive=directory/(name+'-'+version+'.crate')
                if archive.is_file():
                    # Locked archives are bounded, and full SHA-256 is checked, not mere presence.
                    entry['archive_verified']|=hashlib.sha256(i.read(archive,32*1024**2)).hexdigest()==expected
            for directory in directories['src']:
                path=directory/(name+'-'+version)/'.cargo-checksum.json'
                if path.is_file():entry['source_manifest_pin']|=json.loads(i.read(path,4*1024**2))['package']==expected
            suffix=name if len(name)<3 else ('3/'+name[0]+'/'+name if len(name)==3 else name[:2]+'/'+name[2:4]+'/'+name)
            if len(name)<3:suffix=str(len(name))+'/'+name
            for directory in directories['index']:
                for path in (directory/'.cache'/suffix,directory/suffix):
                    if not path.is_file():continue
                    raw=i.read(path,16*1024**2)
                    for record in re.split(b'[\x00\n]',raw):
                        if not record.startswith(b'{'):continue
                        v=json.loads(record)
                        if v.get('name')==name and v.get('vers')==version and v.get('cksum')==expected:entry['index_row']=True
        except Exception as error:entry['error']=type(error).__name__
        rows.append(entry)
    return dict(scope='Cargo.lock dependency closure of supervisor, including dev and platform union; conservative, not cargo resolution or source-tree verification',ambiguous_dependencies=ambiguous,required_packages=len(required),packages=rows,missing_archives=sum(not p['archive_verified'] for p in rows),missing_index_rows=sum(not p['index_row'] for p in rows),source_manifests_are_not_content_verification=True)

def collect(home,source,run,inspector=None,emit=lambda name,row:None):
    i=inspector or Inspector(60);home,source,run=map(Path,(home,source,run));out=dict(version=1,informational_only=True)
    def add(name,call):
        out[name]=observation(call);emit(name,out[name])
    add('kernel',lambda:kernel_identity(i))
    add('kernel_support',lambda:kernel_support(i,out['kernel']['value']))
    tools=('bash','python3','realpath','uname','id','env','nice','timeout','findmnt','awk','mkdir','cp','cc','readelf','sudo','systemctl','systemd-run','grep','sleep','rmdir','ar','as','ld','make','cmake','pkg-config','perl','du')
    add('tools',lambda:dict(presence={name:shutil.which(name,path='/usr/bin:/bin') is not None for name in tools},python=observation(lambda:i.command(['/usr/bin/python3','-VV']))))
    toolchain=home/'.rustup/toolchains'/TOOLCHAIN
    def static_libc():
        row=i.command(['/usr/bin/cc','-print-file-name=libc.a'])
        path=Path(row['stdout'].strip())
        return dict(command=row,absolute=path.is_absolute(),library_exists=path.is_absolute() and path.is_file())
    add('toolchain',lambda:dict(rustc=observation(lambda:i.command([str(toolchain/'bin/rustc'),'-Vv'])),cargo=observation(lambda:i.command([str(toolchain/'bin/cargo'),'-Vv'])),native_std=any((toolchain/'lib/rustlib/aarch64-unknown-linux-gnu/lib').glob('libstd-*.rlib')),static_libc=observation(static_libc),readelf=observation(lambda:i.command(['/usr/bin/readelf','--version'])),copy_sizes=observation(lambda:i.command(['/usr/bin/du','-sk',str(toolchain),str(home/'.cargo/registry')]))))
    add('registry',lambda:registry(i,home,source))
    def filesystem(path):
        m=os.statvfs(path)
        return dict(free_bytes=m.f_bavail*m.f_frsize,total_bytes=m.f_blocks*m.f_frsize,noexec=bool(m.f_flag&getattr(os,'ST_NOEXEC',8)),mount=i.command(['/usr/bin/findmnt','-n','-o','FSTYPE,OPTIONS','-T',str(path)]))
    add('storage',lambda:dict(work=filesystem(run),guest_home=filesystem(home),slow_d4_tmpfs=observation(lambda:filesystem(Path('/dev/shm')))))
    def cgroup():
        rel=next(line.split(':',2)[2] for line in i.read('/proc/self/cgroup').decode().splitlines() if line.startswith('0::'))
        root=Path('/sys/fs/cgroup');path=root/rel.lstrip('/')
        rows={}
        for key in ('cgroup.controllers','cgroup.subtree_control','cgroup.type','memory.max','memory.swap.max','pids.max'):
            rows[key]=observation(lambda key=key:i.read(path/key,4096).decode().strip())
        return dict(scope='current mask process group, not the future W0 delegate; read-only, no group or controller write',mount=i.command(['/usr/bin/findmnt','-n','-o','FSTYPE','-T',str(root)]),own_group=rows,user_manager=observation(lambda:i.command(['/usr/bin/systemctl','--user','show','--property=Version','--property=ControlGroup'],user_bus=True)))
    add('cgroup',cgroup)
    out['budget']=dict(collection_seconds=60,per_command_seconds=3,guest_build_and_copy_max_seconds=600,guest_total_seconds=1965,guest_test_max_seconds=1200,guest_test_cleanup_reserve_seconds=90,no_build_or_case_executed=True)
    return out
