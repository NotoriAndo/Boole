"""No-follow source staging and stopped-VM evidence custody (host only)."""
import hashlib,json,os,stat,time
from pathlib import Path

def stamp(m):
    return (m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_nlink,m.st_size,m.st_mtime_ns,m.st_ctime_ns)

def read(path,cap):
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
    try:
        before=os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_uid!=os.geteuid() or before.st_nlink!=1 or before.st_size>cap:raise ValueError('owned single-link regular bounded source required')
        with os.fdopen(os.dup(fd),'rb') as f:raw=f.read(cap+1)
        if len(raw)>cap or stamp(os.fstat(fd))!=stamp(before):raise ValueError('source changed while reading')
        return raw
    finally:os.close(fd)

def inventory(source):
    source=Path(source)
    if source.resolve()!=source:raise ValueError('source directory alias')
    actual=set()
    for parent,dirs,files in os.walk(source,followlinks=False):
        for name in dirs:
            p=Path(parent)/name
            if p.is_symlink():raise ValueError('source directory symlink')
        for name in files:actual.add(str((Path(parent)/name).relative_to(source)))
        if len(actual)>10000:raise ValueError('source file bound')
    return actual

def verified(source,pin,commit):
    raw=read(source/'SOURCE.json',4*1024**2)
    if hashlib.sha256(raw).hexdigest()!=pin:raise ValueError('source manifest pin')
    m=json.loads(raw)
    if set(m) not in ({'version','commit','files'},{'version','commit','tree','files'}) or m['version']!=1 or m['commit']!=commit or not isinstance(m['files'],dict):raise ValueError('source manifest identity')
    if inventory(source)!=set(m['files'])|{'SOURCE.json'}:raise ValueError('source manifest file set')
    for name,digest in m['files'].items():
        p=Path(name)
        if p.is_absolute() or '..' in p.parts or str(p)!=name:raise ValueError('source relative name')
        if hashlib.sha256(read(source/name,128*1024**2)).hexdigest()!=digest:raise ValueError('source file digest')
    return raw,m

def stage(source,destination,pin,commit):
    raw,m=verified(source,pin,commit)
    destination.mkdir(mode=0o700)
    for name,digest in [*m['files'].items(),('SOURCE.json',pin)]:
        data=read(source/name,128*1024**2)
        if hashlib.sha256(data).hexdigest()!=digest:raise ValueError('source changed before guest copy')
        p=destination/name;p.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
        fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600)
        with os.fdopen(fd,'wb') as f:f.write(data);f.flush();os.fchmod(f.fileno(),0o500 if (source/name).stat().st_mode&0o111 else 0o400);os.fsync(f.fileno())
    verified(destination,pin,commit)
    for parent,_,_ in os.walk(destination,topdown=False):
        fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW);os.fsync(fd);os.close(fd)

def seal(run,destination,manifest_path):
    """After confirmed stop, remove guest access, detach aliases, then seal.

    Unsafe/unreadable entries and resource limits fail qualification while
    preserving the moved tree and an explicitly incomplete hash manifest.
    """
    if run.resolve()!=run or destination.exists() or destination.is_symlink() or destination.parent.resolve()!=destination.parent:raise ValueError('custody alias or collision')
    os.rename(run,destination)
    rows={};errors=[];deadline=time.monotonic()+90;total=0;directories=[];pending=[destination];nodes=0
    def error(e):
        if len(errors)<1000:errors.append(str(e)[:512])
    while pending:
        parent=pending.pop();directories.append(parent)
        try:
            if time.monotonic()>deadline or nodes>100000:raise ValueError('custody time/file bound')
            if len(parent.relative_to(destination).parts)>64:raise ValueError('custody depth bound')
            with os.scandir(parent) as entries:
                for entry in entries:
                    nodes+=1
                    if time.monotonic()>deadline or nodes>100000:raise ValueError('custody time/file bound')
                    p=Path(entry.path)
                    if entry.is_dir(follow_symlinks=False):pending.append(p);continue
                    key=str(p.relative_to(destination));row={};rows[key]=row
                    try:
                        fd=os.open(p,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
                        try:
                            before=os.fstat(fd)
                            if not stat.S_ISREG(before.st_mode) or before.st_uid!=os.geteuid():raise ValueError('unsafe custody entry')
                            tmp=p.with_name(p.name+'.seal-stage');h=hashlib.sha256();size=0
                            out=os.open(tmp,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600)
                            with os.fdopen(out,'wb') as f:
                                while True:
                                    if time.monotonic()>deadline:raise ValueError('custody deadline')
                                    b=os.read(fd,65536)
                                    if not b:break
                                    size+=len(b);total+=len(b)
                                    if total>64*1024**3:raise ValueError('custody byte bound')
                                    f.write(b);h.update(b)
                                f.flush();os.fchmod(f.fileno(),0o500 if before.st_mode&0o111 else 0o400);os.fsync(f.fileno())
                            if stamp(os.fstat(fd))!=stamp(before):raise ValueError('evidence changed after stop')
                            os.replace(tmp,p);row.update(sha256=h.hexdigest(),bytes=size)
                        finally:os.close(fd)
                    except (OSError,ValueError) as e:row['error']=str(e)[:512];error(e)
        except (OSError,ValueError) as e:error(e)
        if time.monotonic()>deadline or nodes>100000:
            error('custody traversal incomplete');break
    for parent in reversed(directories):
        if time.monotonic()>deadline:error('custody sync deadline');break
        try:
            fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
            try:os.fsync(fd);os.fchmod(fd,0o500)
            finally:os.close(fd)
        except OSError as e:error(e)
    payload=json.dumps(dict(version=1,files=rows,errors=errors,complete=not errors),sort_keys=True,separators=(',',':')).encode()+b'\n'
    fd=os.open(manifest_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o400)
    with os.fdopen(fd,'wb') as f:f.write(payload);f.flush();os.fsync(f.fileno())
    for parent in (destination.parent,manifest_path.parent):
        fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW);os.fsync(fd);os.close(fd)
    if errors:raise ValueError('custody incomplete; evidence preserved outside share')
