"""Trusted cheap Linux prerequisite probe, before compilation; no host fallback."""
import ctypes,os,platform,signal,subprocess,time
from pathlib import Path

def plan(group,source,scratch,flags):
    args=['/usr/bin/bwrap',*flags]
    for p in ('/usr','/bin','/lib','/lib64'):
        if Path(p).exists():args+=['--ro-bind',p,p]
    script="import os;assert os.getcwd()=='/scratch';assert os.environ['W0_PRECHECK']=='yes';f=open('/input','rb');assert f.read()==b'W0-input';f.close();\ntry:open('/input','wb')\nexcept OSError:pass\nelse:raise RuntimeError('writable input')\nopen('/scratch/ok','xb').close();os.write(3,b'W0-fd3')"
    return [*args,'--ro-bind',str(source),'/input','--bind',str(scratch),'/scratch','--chdir','/scratch','--setenv','W0_PRECHECK','yes','/usr/bin/python3','-I','-B','-c',script]

def run(group,root,flags):
    if platform.system()!='Linux' or os.geteuid()==0:raise ValueError('unprivileged Linux prerequisite probe only')
    libc=ctypes.CDLL(None,use_errno=True);libc.syscall.restype=ctypes.c_long
    class CloneArgs(ctypes.Structure):_fields_=[(n,ctypes.c_uint64) for n in ('flags','pidfd','child_tid','parent_tid','exit_signal','stack','stack_size','tls','set_tid','set_tid_size','cgroup')]
    source=root/'precheck-input';scratch=root/'precheck-scratch';scratch.mkdir(mode=0o700)
    with source.open('xb') as f:f.write(b'W0-input');f.flush();os.fsync(f.fileno())
    source.chmod(0o400)
    group.mkdir();child=None;pidfd=ctypes.c_int(-1);rd=wr=None
    try:
        for k,v in [('memory.max','268435456'),('memory.swap.max','0'),('memory.oom.group','1'),('pids.max','32')]:
            (group/k).write_text(v)
            if (group/k).read_text().strip()!=v:raise ValueError('cgroup setting readback')
        move=os.open(group/'cgroup.procs',os.O_WRONLY|os.O_CLOEXEC)
        rd,wr=os.pipe2(os.O_CLOEXEC|os.O_NONBLOCK)
        args=plan(group,source,scratch,flags);parent=os.getpid()
        spec=CloneArgs(flags=0x1000,pidfd=ctypes.addressof(pidfd),exit_signal=signal.SIGCHLD)
        try:child=libc.syscall(435,ctypes.byref(spec),ctypes.sizeof(spec))
        except BaseException:os.close(move);raise
        if child<0:os.close(move);raise OSError(ctypes.get_errno(),'clone3 CLONE_PIDFD required')
        if child==0:
            try:
                if libc.prctl(1,signal.SIGKILL,0,0,0)!=0 or os.getppid()!=parent:os._exit(125)
                number=str(os.getpid()).encode()
                if os.write(move,number)!=len(number):os._exit(125)
                os.close(move)
                if '0::'+str(group).removeprefix('/sys/fs/cgroup') not in Path('/proc/self/cgroup').read_text().splitlines():os._exit(125)
                os.dup2(wr,3,inheritable=True)
                if libc.syscall(436,4,ctypes.c_uint(-1),0)!=0:os._exit(125)
                os.execve(args[0],args,{'PATH':'/usr/bin:/bin','LC_ALL':'C'})
            except BaseException:os._exit(125)
        os.close(move);os.close(wr);wr=None
        deadline=time.monotonic()+10
        while True:
            p,status=os.waitpid(child,os.WNOHANG)
            if p:child=None;break
            if time.monotonic()>=deadline:raise TimeoutError('kernel precheck deadline')
            time.sleep(.01)
        if not os.WIFEXITED(status) or os.WEXITSTATUS(status)!=0 or os.read(rd,64)!=b'W0-fd3' or not (scratch/'ok').is_file():raise ValueError('kernel precheck child/file-bind/chdir/setenv/fd3 failure')
    finally:
        if child is not None and child>0:
            signal.pidfd_send_signal(pidfd.value,signal.SIGKILL);os.waitpid(child,0)
        for fd in (rd,wr,pidfd.value if pidfd.value>=0 else None):
            if fd is not None:os.close(fd)
        if group.exists():
            (group/'cgroup.kill').write_text('1');deadline=time.monotonic()+2
            while 'populated 0' not in (group/'cgroup.events').read_text():
                if time.monotonic()>deadline:raise ValueError('precheck cgroup did not drain')
                time.sleep(.01)
            group.rmdir()
