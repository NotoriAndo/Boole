"""W0 v3: host ticks over stdin; only guest-written state uses the share."""
import errno,fcntl,hashlib,json,mmap,os,select,stat,subprocess,sys,time
from pathlib import Path
SIZE=4096
LIVE=b'BW0LIVE3'
SNAP=b'BW0SNAP3'
ACK=b'BW0ACKV3'
BOUND=3.0
STARTUP=10.0
METHODS=('held','fresh','fadvise','direct')

class Refused(ValueError):pass
class Torn(Refused):pass

def canonical(value):return json.dumps(value,sort_keys=True,separators=(',',':'),allow_nan=False).encode()
def grant_pin(grant):return hashlib.sha256(canonical(grant)).hexdigest()
def frame(magic,value):
    raw=canonical(value);seq=value['sequence']
    if type(seq) is not int or not 0<=seq<2**64 or len(raw)>SIZE-52:raise Refused('heartbeat sequence/size')
    body=magic+seq.to_bytes(8,'big')+len(raw).to_bytes(4,'big')+raw
    body+=b'\0'*(SIZE-32-len(body))
    return body+hashlib.sha256(body).digest()
def decode(raw,magic):
    if len(raw)!=SIZE or raw[:8]!=magic or hashlib.sha256(raw[:-32]).digest()!=raw[-32:]:raise Torn('heartbeat torn/checksum/size')
    size=int.from_bytes(raw[16:20],'big')
    if size>SIZE-52 or any(raw[20+size:-32]):raise Refused('heartbeat padding/length')
    def pairs(values):
        d={}
        for k,v in values:
            if k in d:raise Refused('duplicate heartbeat field')
            d[k]=v
        return d
    try:value=json.loads(raw[20:20+size],object_pairs_hook=pairs)
    except (ValueError,UnicodeError) as e:raise Refused('heartbeat JSON') from e
    if not isinstance(value,dict) or canonical(value)!=raw[20:20+size] or type(value.get('sequence')) is not int or value['sequence']!=int.from_bytes(raw[8:16],'big'):raise Refused('heartbeat canonical sequence')
    return value

def parent_fd(path):
    path=Path(path)
    if not path.is_absolute() or '..' in path.parts:raise Refused('absolute heartbeat path')
    fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
    try:
        for name in path.parts[1:-1]:
            child=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);os.close(fd);fd=child
        return fd
    except BaseException:os.close(fd);raise

def opened(path,flags,create=False):
    directory=parent_fd(path)
    try:return os.open(Path(path).name,flags|os.O_NOFOLLOW|os.O_CLOEXEC|os.O_NONBLOCK|(os.O_CREAT|os.O_EXCL if create else 0),0o600,dir_fd=directory)
    finally:os.close(directory)
def identity(fd,path,size=SIZE):
    m=os.fstat(fd);d=parent_fd(path)
    try:named=os.stat(Path(path).name,dir_fd=d,follow_symlinks=False)
    finally:os.close(d)
    if (not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1 or stat.S_IMODE(m.st_mode)!=0o600 or (size is not None and m.st_size!=size)
            or (named.st_dev,named.st_ino)!=(m.st_dev,m.st_ino)):raise Refused('heartbeat identity/owner/mode/size')
    return m

def write(fd,path,magic,value):
    raw=frame(magic,value);identity(fd,path,None)
    if os.pwrite(fd,raw,0)!=SIZE:raise Refused('short heartbeat publication')
    os.fsync(fd);identity(fd,path)
    return hashlib.sha256(raw).hexdigest()
def read(fd,path,magic,deadline=None):
    deadline=time.monotonic()+.2 if deadline is None else deadline;attempts=0
    while True:
        before=identity(fd,path);raw=os.pread(fd,SIZE+1,0);after=identity(fd,path);attempts+=1
        if (before.st_dev,before.st_ino)!=(after.st_dev,after.st_ino):raise Refused('heartbeat inode changed')
        try:value=decode(raw,magic)
        except Torn:
            if time.monotonic()>=deadline:raise
            time.sleep(min(.005,max(0,deadline-time.monotonic())));continue
        if time.monotonic()>deadline:raise Refused('heartbeat read deadline')
        return value,dict(inode=before.st_ino,device=before.st_dev,frame_sha256=hashlib.sha256(raw).hexdigest(),read_attempts=attempts)

def event_bytes(reader,**fields):
    raw=canonical(dict(version=3,reader=reader,pid=os.getpid(),read_unix_ns=time.time_ns(),read_monotonic_ns=time.monotonic_ns(),**fields))+b'\n'
    if len(raw)>4096:raise Refused('heartbeat evidence row bound')
    return raw

def host_record(fd,**fields):
    # The caller pre-opens this unshared fd before boot. Never resolve a share name.
    m=os.fstat(fd)
    if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1 or m.st_size>128*1024**2:raise Refused('host evidence identity/bound')
    raw=event_bytes('host',**fields)
    if os.write(fd,raw)!=len(raw):raise Refused('host evidence short write')

def record(root,reader,**fields):
    if reader=='host':raise Refused('host evidence requires pre-opened unshared fd')
    path=Path(root)/'live-reads.ndjson';directory=parent_fd(path)
    try:fd=os.open(path.name,os.O_WRONLY|os.O_APPEND|os.O_CREAT|os.O_NOFOLLOW|os.O_CLOEXEC|os.O_NONBLOCK,0o600,dir_fd=directory)
    finally:os.close(directory)
    try:
        identity(fd,path,None);fcntl.flock(fd,fcntl.LOCK_EX)
        if os.fstat(fd).st_size>128*1024**2:raise Refused('heartbeat evidence bound')
        raw=event_bytes(reader,**fields)
        if os.write(fd,raw)!=len(raw):raise Refused('heartbeat evidence write')
        os.fsync(fd)
    finally:os.close(fd)

def valid_live(value,grant,now):
    if (set(value)!={'version','sequence','grant','active','tick_unix','closing'} or type(value['version']) is not int or value['version']!=3 or type(value['active']) is not bool or not value['active'] or type(value['closing']) is not bool
            or canonical(value['grant'])!=canonical(grant) or type(value['sequence']) is not int or value['sequence']<1 or type(value['tick_unix']) is not int or not now-33<=value['tick_unix']<=now+30):raise Refused('heartbeat revoked/stale/foreign')

def live_read(path,grant,now,reader='caller',fd=None):
    own=fd is None;meta=dict(inode=None,device=None);value={}
    try:
        if own:fd=opened(path,os.O_RDONLY)
        value,meta=read(fd,path,SNAP)
        stamp=value.pop('observed_monotonic_ns',None)
        valid_live(value,grant,now)
        mono=time.monotonic_ns()
        if type(stamp) is not int or not 0<=mono-stamp<=int(BOUND*10**9):raise Refused('guest snapshot stale/future')
        value['observed_monotonic_ns']=stamp
        record(Path(path).parent,reader,source='guest-stdin-snapshot',ok=True,sequence=value['sequence'],tick_unix=value['tick_unix'],**meta)
        return value,meta
    except (OSError,ValueError) as e:
        record(Path(path).parent,reader,source='guest-stdin-snapshot',ok=False,error=str(e)[:300],sequence=value.get('sequence'),tick_unix=value.get('tick_unix'),**meta);raise
    finally:
        if own and fd is not None:os.close(fd)

def send_frame(fd,raw,deadline):
    os.set_blocking(fd,False);offset=0
    while offset<len(raw):
        remaining=deadline-time.monotonic()
        if remaining<=0:raise Refused('stdin publication deadline')
        if not select.select([],[fd],[],min(.05,remaining))[1]:continue
        try:n=os.write(fd,raw[offset:])
        except BlockingIOError:continue
        if n<=0:raise Refused('stdin publication EOF')
        offset+=n

def receive_frame(fd,deadline):
    os.set_blocking(fd,False);raw=bytearray()
    while len(raw)<SIZE:
        remaining=deadline-time.monotonic()
        if remaining<=0:raise Refused('stdin heartbeat deadline')
        if not select.select([fd],[],[],min(.05,remaining))[0]:continue
        try:part=os.read(fd,SIZE-len(raw))
        except BlockingIOError:continue
        if not part:raise Refused('stdin heartbeat EOF/truncated')
        raw.extend(part)
    decode(raw,LIVE)
    return bytes(raw)

class Host:
    def __init__(self,root,grant,record_fd):
        self.root=Path(root);self.grant=grant;self.pin=grant_pin(grant);self.record_fd=record_fd;self.sequence=0;self.started=False;self.closed=False;self.confirming=False;self.acknowledged=0;self.fds=[];self.channel=None;self.diag=None;self.diag_pending=b''
        self.ack_path=self.root/'live-ack.bin';self.diag_paths=[self.root/f'live-share-{m}.bin' for m in METHODS];self.diag_fds=[]
        try:
            self.ackfd=opened(self.ack_path,os.O_RDWR,True);self.fds.append(self.ackfd)
            write(self.ackfd,self.ack_path,ACK,dict(version=3,sequence=0,grant_sha256=self.pin,frame_sha256='0'*64,live_inode=0,tick_unix=0,read_unix_ns=0,closing=False))
            initial=dict(version=3,sequence=0,grant=grant,active=False,tick_unix=int(time.time()),closing=False)
            for path in self.diag_paths:
                fd=opened(path,os.O_RDWR,True);self.fds.append(fd);self.diag_fds.append(fd);write(fd,path,LIVE,initial)
        except BaseException:self.close();raise
    def attach(self,fd,diagnostic=False):
        if self.channel is not None:raise Refused('stdin already attached')
        self.channel=os.dup(fd);self.fds.append(self.channel);os.set_blocking(self.channel,False)
        if diagnostic:
            args=[sys.executable,'-IB',__file__,'share-writer-v3',str(self.root),*[str(fd) for fd in self.diag_fds]]
            self.diag=subprocess.Popen(args,stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,pass_fds=tuple(self.diag_fds),env={'PATH':'/usr/bin:/bin','LC_ALL':'C','PYTHONDONTWRITEBYTECODE':'1'})
            os.set_blocking(self.diag.stdin.fileno(),False)
    def _publish(self,active,closing=False,timeout=.2,log=True):
        if self.channel is None:raise Refused('stdin not attached')
        self.tick_unix=int(time.time());raw=frame(LIVE,dict(version=3,sequence=self.sequence,grant=self.grant,active=active,tick_unix=self.tick_unix,closing=closing))
        send_frame(self.channel,raw,time.monotonic()+timeout);self.sent=time.monotonic();self.digest=hashlib.sha256(raw).hexdigest()
        if log:host_record(self.record_fd,event='publish',transport='stdin',sequence=self.sequence,tick_unix=self.tick_unix,active=active,closing=closing,frame_sha256=self.digest)
        if log and self.diag is not None and self.diag.poll() is None:
            if len(self.diag_pending)+len(raw)<=2*SIZE:self.diag_pending+=raw
            else:host_record(self.record_fd,event='diagnostic-drop',sequence=self.sequence,diagnostic_only=True)
            try:
                n=os.write(self.diag.stdin.fileno(),self.diag_pending);self.diag_pending=self.diag_pending[n:]
            except (BlockingIOError,BrokenPipeError):pass
    def start(self):
        if self.started:raise Refused('heartbeat already started')
        self.started=True;self.sequence=1;self._publish(True)
    def tick(self,publish=True):
        if not self.started or self.closed:return
        deadline=self.sent+(STARTUP if self.sequence==1 else BOUND)
        ack,meta=read(self.ackfd,self.ack_path,ACK,min(deadline,time.monotonic()+.2))
        if time.monotonic()>deadline:raise Refused('heartbeat tick not observed before deadline')
        keys={'version','sequence','grant_sha256','frame_sha256','live_inode','tick_unix','read_unix_ns','closing'}
        if (set(ack)!=keys or type(ack['version']) is not int or ack['version']!=3 or ack['grant_sha256']!=self.pin or type(ack['closing']) is not bool or type(ack['sequence']) is not int or not 0<=ack['sequence']<=self.sequence):raise Refused('heartbeat acknowledgement binding')
        if ack['sequence']!=self.sequence:return
        if ack['frame_sha256']!=self.digest or ack['tick_unix']!=self.tick_unix or type(ack['live_inode']) is not int or ack['live_inode']<=0 or type(ack['read_unix_ns']) is not int or ack['read_unix_ns']<=0:raise Refused('heartbeat acknowledgement identity')
        if self.acknowledged!=self.sequence:
            host_record(self.record_fd,event='acknowledged',sequence=self.sequence,tick_unix=ack['tick_unix'],guest_inode=ack['live_inode'],guest_read_unix_ns=ack['read_unix_ns'],frame_sha256=self.digest,closing=ack['closing'],read_attempts=meta['read_attempts']);self.acknowledged=self.sequence
        if self.confirming:
            if not ack['closing']:raise Refused('heartbeat close acknowledgement missing')
            self.closed=True;return
        if ack['closing']:self.confirming=True;self.sequence+=1;self._publish(True,True)
        elif publish:self.sequence+=1;self._publish(True)
    def finish(self,guard=lambda:None):
        end=time.monotonic()+BOUND
        while not self.closed:
            guard();self.tick(False)
            if time.monotonic()>end:raise Refused('heartbeat final observation absent')
            time.sleep(.05)
    def deactivate(self,timeout=.2):
        if self.started:self.sequence+=1;self._publish(False,timeout=timeout,log=False)
    def stop_diagnostic(self):
        if self.diag is not None:
            if self.diag.poll() is None:self.diag.kill()
            self.diag.wait(timeout=.2);self.diag.stdin.close();self.diag=None
    def close(self):
        self.stop_diagnostic()
        for fd in self.fds:os.close(fd)
        self.fds=[]

class Guest:
    def __init__(self,root,grant,input_fd):
        self.root=Path(root);self.grant=grant;self.input_fd=input_fd;self.path=self.root/'live-grant.json';self.ack_path=self.root/'live-ack.bin';self.fds=[]
        try:
            self.fd=opened(self.path,os.O_RDWR,True);self.fds.append(self.fd)
            self.ackfd=opened(self.ack_path,os.O_RDWR);self.fds.append(self.ackfd)
        except BaseException:self.close();raise
        self.sequence=0;self.digest=None;self.changed=time.monotonic();self.closing=False
    def step(self,closing=False,initial=None):
        self.closing|=closing
        raw=initial if initial is not None else receive_frame(self.input_fd,time.monotonic()+(STARTUP if self.sequence==0 else BOUND))
        value=decode(raw,LIVE);valid_live(value,self.grant,int(time.time()))
        if value['sequence']!=self.sequence+1:raise Refused('heartbeat sequence gap/regression/change')
        if value['closing'] and not self.closing:raise Refused('unexpected heartbeat close')
        snapshot=dict(value,observed_monotonic_ns=time.monotonic_ns());write(self.fd,self.path,SNAP,snapshot)
        self.sequence=value['sequence'];self.digest=hashlib.sha256(raw).hexdigest();self.changed=time.monotonic()
        record(self.root,'watch',source='stdin',ok=True,sequence=self.sequence,tick_unix=value['tick_unix'],inode=os.fstat(self.input_fd).st_ino,snapshot_inode=os.fstat(self.fd).st_ino,frame_sha256=self.digest)
        write(self.ackfd,self.ack_path,ACK,dict(version=3,sequence=self.sequence,grant_sha256=grant_pin(self.grant),frame_sha256=self.digest,live_inode=os.fstat(self.fd).st_ino,tick_unix=value['tick_unix'],read_unix_ns=time.time_ns(),closing=self.closing))
        return self.closing and value['closing']
    def invalidate(self):
        value=dict(version=3,sequence=self.sequence,grant=self.grant,active=False,tick_unix=int(time.time()),closing=False,observed_monotonic_ns=time.monotonic_ns())
        write(self.fd,self.path,SNAP,value)
    def close(self):
        for fd in self.fds:os.close(fd)
        self.fds=[]

def share_writer(root,fds):
    while True:
        try:raw=receive_frame(0,time.monotonic()+60)
        except Refused:return
        value=decode(raw,LIVE)
        for method,fd in zip(METHODS,fds):write(fd,Path(root)/f'live-share-{method}.bin',LIVE,value)

def diagnostic(root,seconds=3):
    # Separate per-method inodes keep fadvise from refreshing the held/fresh samples.
    root=Path(root);path=root/'share-diagnostic.ndjson';out=opened(path,os.O_WRONLY,True);held={}
    try:
        for method in METHODS:held[method]=opened(root/f'live-share-{method}.bin',os.O_RDONLY)
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            for method in METHODS:
                fd=held[method];temporary=None;row=dict(method=method,diagnostic_only=True,read_unix_ns=time.time_ns(),sequence=None)
                try:
                    p=root/f'live-share-{method}.bin'
                    if method=='fresh':temporary=opened(p,os.O_RDONLY);fd=temporary
                    elif method=='fadvise':
                        if not hasattr(os,'posix_fadvise'):raise OSError(errno.ENOTSUP,'fadvise unavailable')
                        os.posix_fadvise(fd,0,SIZE,os.POSIX_FADV_DONTNEED)
                    elif method=='direct':
                        if not hasattr(os,'O_DIRECT'):raise OSError(errno.ENOTSUP,'O_DIRECT unavailable')
                        temporary=opened(p,os.O_RDONLY|os.O_DIRECT);fd=temporary
                    identity(fd,p);row['inode']=os.fstat(fd).st_ino
                    if method=='direct':
                        with mmap.mmap(-1,SIZE) as aligned:
                            count=os.preadv(fd,[aligned],0);raw=aligned[:count]
                    else:raw=os.pread(fd,SIZE+1,0)
                    value=decode(raw,LIVE);row.update(ok=True,sequence=value['sequence'])
                except (OSError,ValueError) as e:row.update(ok=False,error=str(e)[:300],errno=getattr(e,'errno',None))
                finally:
                    if temporary is not None:os.close(temporary)
                os.write(out,canonical(row)+b'\n')
            time.sleep(.2)
        os.fsync(out)
    finally:
        for fd in held.values():os.close(fd)
        os.close(out)

if __name__=='__main__':
    if sys.argv[1:]==['vector-v3']:
        g=dict(state='granted',owner='N',step='W0',request_id='vector',generation=3,start_unix=100,end_unix=1000)
        sys.stdout.buffer.write(frame(SNAP,dict(version=3,sequence=7,grant=g,active=True,tick_unix=200,closing=False,observed_monotonic_ns=100000000)))
    elif len(sys.argv)==7 and sys.argv[1]=='share-writer-v3':share_writer(sys.argv[2],list(map(int,sys.argv[3:])))
    else:raise Refused('closed heartbeat command')
