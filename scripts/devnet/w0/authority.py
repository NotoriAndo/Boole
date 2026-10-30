"""Read orchestrator grants under its lock; N may append lifecycle events only."""
import fcntl,json,os,stat,time
from pathlib import Path

class Refused(ValueError):
    pass

def decode(raw):
    def pairs(items):
        value={}
        for k,v in items:
            if k in value:raise Refused('duplicate authority key')
            value[k]=v
        return value
    return json.loads(raw,object_pairs_hook=pairs)

def read(path,cap=1048576):
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
    try:
        m=os.fstat(fd)
        if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_nlink!=1 or m.st_mode&0o077 or m.st_size>cap:
            raise Refused('private authority file required')
        raw=bytearray()
        while len(raw)<=cap:
            b=os.read(fd,min(65536,cap+1-len(raw)))
            if not b:break
            raw.extend(b)
        a=os.fstat(fd);n=os.stat(path,follow_symlinks=False)
        def stamp(v):return (v.st_dev,v.st_ino,v.st_mode,v.st_size,v.st_mtime_ns,v.st_ctime_ns)
        if len(raw)>cap or stamp(m)!=stamp(a) or stamp(m)!=stamp(n):raise Refused('authority changed while reading')
        return bytes(raw)
    finally:os.close(fd)

def validate_rows(value,grant,owner=None,require_active=True):
    if not isinstance(value,dict) or set(value)!={'version','rows'} or value['version']!=1 or not isinstance(value['rows'],list):
        raise Refused('window ledger schema')
    high=0;latest=None;known={};consumed={};released=set()
    target=(grant['request_id'],grant['generation'])
    for row in value['rows']:
        if not isinstance(row,dict):raise Refused('window row schema')
        if row.get('writer')=='orchestrator' and set(row)=={'writer','grant'}:
            g=row['grant']
            if not isinstance(g,dict) or set(g)!=set(grant) or type(g['generation']) is not int or g['generation']<=0 or not isinstance(g['request_id'],str):
                raise Refused('orchestrator grant schema')
            key=(g['request_id'],g['generation'])
            if g['state']=='granted':
                if g['generation']<=high:raise Refused('generation must strictly increase')
                high=g['generation'];known[key]=g;latest=g
            elif g['state'] in ('revoked','cancelled'):
                if known.get(key)!={**g,'state':'granted'}:raise Refused('foreign revocation')
                if latest and (latest['request_id'],latest['generation'])==key:latest=g
            else:raise Refused('unknown orchestrator state')
        elif row.get('writer')=='N' and set(row)=={'writer','event','request_id','generation','owner','unix'} and row['event'] in ('CONSUMED','RELEASED'):
            key=(row['request_id'],row['generation'])
            if key not in known or not isinstance(row['owner'],str) or not row['owner'] or type(row['unix']) is not int or row['unix']<0:
                raise Refused('foreign lifecycle event')
            if row['event']=='CONSUMED':
                if key in consumed or latest!=known[key]:raise Refused('window already consumed or inactive')
                consumed[key]=row['owner']
            elif consumed.get(key)!=row['owner'] or key in released:raise Refused('unowned release')
            else:released.add(key)
        else:raise Refused('only orchestrator grants and N lifecycle events are allowed')
    if known.get(target)!=grant or grant['state']!='granted':raise Refused('grant absent or foreign')
    if require_active and latest!=grant:raise Refused('grant superseded or revoked')
    if target in released:raise Refused('window already released')
    if target in consumed and consumed[target]!=owner:raise Refused('window already consumed')
    return consumed.get(target)

def ledger(path,lock_path,grant,owner=None,event=None):
    # The same lock path is part of the external authority, not a local quiet
    # lock invented by the runner. The orchestrator must use it for every write.
    lock_fd=os.open(lock_path,os.O_RDWR|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        m=os.fstat(lock_fd)
        if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or m.st_mode&0o077:raise Refused('window lock identity')
        fcntl.flock(lock_fd,fcntl.LOCK_EX if event else fcntl.LOCK_SH)
        value=decode(read(path));consumed=validate_rows(value,grant,owner,require_active=event!='RELEASED')
        if event is not None:
            if event not in ('CONSUMED','RELEASED') or not owner:raise Refused('N cannot write a grant')
            if (event=='CONSUMED' and consumed is not None) or (event=='RELEASED' and consumed!=owner):raise Refused('lifecycle order')
            value['rows'].append(dict(writer='N',event=event,request_id=grant['request_id'],generation=grant['generation'],owner=owner,unix=int(time.time())))
            atomic(path,(json.dumps(value,separators=(',',':'))+'\n').encode())
        return value
    finally:os.close(lock_fd)

def atomic(path,raw):
    p=Path(path);directory=os.open(p.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    stage=f'.{p.name}.{os.getpid()}.{time.monotonic_ns()}'
    fd=None
    try:
        fd=os.open(stage,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=directory)
        with os.fdopen(os.dup(fd),'wb') as f:f.write(raw);f.flush()
        os.fsync(fd);os.replace(stage,p.name,src_dir_fd=directory,dst_dir_fd=directory);os.fsync(directory)
    finally:
        if fd is not None:os.close(fd)
        try:os.unlink(stage,dir_fd=directory)
        except FileNotFoundError:pass
        os.close(directory)
