"""Cheap, closed guest prerequisites; never called by host preparation."""
import ctypes,hashlib,importlib.util,json,os,platform,subprocess,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('w0_io',Path(__file__).with_name('runner.py'))
io=importlib.util.module_from_spec(spec);spec.loader.exec_module(io)
UNITS=('apt-daily.service','apt-daily.timer','apt-daily-upgrade.service','apt-daily-upgrade.timer','unattended-upgrades.service','snapd.service','snapd.socket','snapd.snap-repair.service','snapd.snap-repair.timer')
BWRAP_FLAGS=['--unshare-user','--unshare-net','--unshare-pid','--unshare-ipc','--unshare-uts','--disable-userns','--assert-userns-disabled','--as-pid-1','--die-with-parent','--new-session','--uid','0','--gid','0','--cap-drop','ALL','--cap-add','CAP_SETPCAP','--clearenv','--proc','/proc','--dev','/dev','--tmpfs','/tmp']

def services_safe(rows):
    for name in UNITS:
        row=rows.get(name,{})
        times=['ActiveEnterTimestampMonotonic']
        if name.endswith('.service'):times.append('ExecMainStartTimestampMonotonic')
        if name.endswith('.timer'):times.append('LastTriggerUSecMonotonic')
        if row.get('is_active')!=row.get('ActiveState') or row.get('is_active_exit')!=3 or row.get('UnitFileState')!='masked' or row.get('ActiveState') not in ('inactive','failed') or any(row.get(n)!='0' for n in times):
            raise io.Refused('update service active or ran since boot: '+name)
    return True

def same_packages(before,after):
    if before!=after:raise io.Refused('bwrap/AppArmor version or pin changed during window')

def userns_restriction(path='/proc/sys/kernel/apparmor_restrict_unprivileged_userns'):
    try:value=io.read_file(path,32).strip()
    except FileNotFoundError:return dict(present=False,value=None,ubuntu_restriction_configured=False,reason='kernel without the Ubuntu userns restriction')
    if value!=b'1':raise io.Refused('AppArmor user namespace policy')
    return dict(present=True,value=value.decode(),ubuntu_restriction_configured=True)

def inventory_module():
    spec=importlib.util.spec_from_file_location('w0_inventory',Path(__file__).with_name('inventory.py'))
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module

def observe_preconditions(run):
    def emit(name,row):io.write_new(run/f'inventory-{name}.json',json.dumps(row,sort_keys=True).encode(),0o400)
    return inventory_module().collect(Path(os.environ['HOME']),Path(__file__).resolve().parents[3],run,emit=emit)

def run(argv):
    if platform.system()!='Linux' or os.geteuid()==0 or len(argv)!=3 or argv[0] not in ('before','after','mask-inventory','mask-skew','mask-probes'):
        raise io.Refused('unprivileged Linux preflight invocation required')
    mode,run,cgroup=argv[0],Path(argv[1]),Path(argv[2]);io.private_directory(run)
    if mode=='mask-probes':
        io.write_new(run/'preconditions-mask-only.json',json.dumps(observe_preconditions(run),sort_keys=True).encode(),0o400);return
    if mode=='mask-skew':
        rows=io.decode(io.read_private(run/'services-mask-only.json',65536))
        grant=io.validate_grant(io.decode(io.read_private(run/'host-grant.json',16384)),int(__import__('time').time()),skew=30)
        raw=io.heartbeat.receive_frame(0,__import__('time').monotonic()+io.heartbeat.STARTUP)
        live=io.heartbeat.decode(raw,io.heartbeat.LIVE);io.heartbeat.valid_live(live,grant,int(__import__('time').time()))
        skew=__import__('time').time()-live['tick_unix']
        io.write_new(run/'clock-skew.json',json.dumps(dict(guest_minus_host_seconds=skew,bound_seconds=30,source='stdin',sequence=live['sequence'])).encode(),0o400)
        if abs(skew)>30:raise io.Refused('mask clock skew')
        services_safe(rows)
        io.write_new(run/'live-initial.bin',raw,0o600)
        return
    env={'PATH':'/usr/bin:/bin','HOME':str(run/'home'),'LC_ALL':'C','PYTHONDONTWRITEBYTECODE':'1'}
    def command(args):
        v=subprocess.run(args,env=env,stdin=subprocess.DEVNULL,capture_output=True,timeout=10,check=True)
        if len(v.stdout)+len(v.stderr)>65536:raise io.Refused('preflight output bound')
        return v.stdout
    if mode=='before':command(['/usr/bin/sudo','-n','/usr/bin/systemctl','mask',*UNITS])
    rows={}
    for name in UNITS:
        raw=command(['/usr/bin/systemctl','show',name,'--property=UnitFileState','--property=ActiveState','--property=ExecMainStartTimestampMonotonic','--property=ActiveEnterTimestampMonotonic','--property=LastTriggerUSecMonotonic'])
        rows[name]=dict(line.split('=',1) for line in raw.decode().splitlines() if '=' in line)
        active=subprocess.run(['/usr/bin/systemctl','is-active',name],env=env,stdin=subprocess.DEVNULL,capture_output=True,timeout=10,check=False)
        if len(active.stdout)+len(active.stderr)>65536:raise io.Refused('is-active output bound')
        rows[name].update(is_active=active.stdout.decode().strip(),is_active_exit=active.returncode,is_active_stderr=active.stderr.decode())
    io.write_new(run/f'services-{"mask-only" if mode=="mask-inventory" else mode}.json',json.dumps(rows,sort_keys=True).encode(),0o400)
    # Inventory is durable before liveness can fail; validation follows its pins.
    pins={'packages':command(['/usr/bin/dpkg-query','-W','-f=${Package} ${Version}\n','bubblewrap','apparmor']).decode(),
          'bwrap':hashlib.sha256(io.read_file('/usr/bin/bwrap',4*1024**2)).hexdigest(),
          'apparmor':hashlib.sha256(io.read_file('/etc/apparmor.d/boole-gate-bwrap',65536)).hexdigest()}
    io.write_new(run/f'packages-{"mask-only" if mode=="mask-inventory" else mode}.json',json.dumps(pins,sort_keys=True).encode(),0o400)
    if pins['bwrap']!=io.BWRAP[platform.machine()] or pins['apparmor']!='35613442bd82247d132b1102579edc5567d8047930d1b60fbb8bbba93c5abcd6':raise io.Refused('retained isolation pins missing')
    services_safe(rows)
    if mode=='mask-inventory':return
    io.write_new(run/f'kernel-identity-{mode}.json',json.dumps(inventory_module().kernel_identity(),sort_keys=True).encode(),0o400)
    if mode=='after':same_packages(io.decode(io.read_private(run/'packages-before.json',65536)),pins)
    else:
        userns_restriction()
        abi=ctypes.CDLL(None,use_errno=True).syscall(444,0,0,1)
        if abi<4:raise io.Refused('Landlock ABI>=4 required')
        if command(['/usr/bin/findmnt','-n','-o','FSTYPE','-T',str(cgroup)]).strip()!=b'cgroup2':raise io.Refused('cgroup-v2 required')
        if not {'memory','pids'}<=set((cgroup/'cgroup.subtree_control').read_text().split()) or (cgroup/'cgroup.procs').read_text().strip():raise io.Refused('empty delegated cgroup root required')
        version=command(['/usr/bin/bwrap','--version'])
        # Trusted runtime probe only, not an untrusted profile or relaxation.
        spec=importlib.util.spec_from_file_location('w0_kernel_check',Path(__file__).with_name('kernel_check.py'))
        kernel=importlib.util.module_from_spec(spec);spec.loader.exec_module(kernel)
        kernel.run(cgroup/'preflight',run,BWRAP_FLAGS)
        io.write_new(run/'bwrap-precheck.txt',version,0o400)
        io.write_new(run/'landlock-abi.json',json.dumps({'ABI':abi}).encode(),0o400)

if __name__=='__main__':
    if sys.argv[1:]==['flags-v1']:print(json.dumps(BWRAP_FLAGS,separators=(',',':')))
    else:run(sys.argv[1:])
