"""Authenticated, per-run offline build inputs. Never installs into a user home."""
import hashlib,json,os,platform,re,stat,tarfile,time,tomllib
from pathlib import Path,PurePosixPath

class Refused(ValueError):pass

def canonical(value):return (json.dumps(value,sort_keys=True,separators=(',',':'))+'\n').encode()
def stamp(m):return (m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_nlink,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
def parent(path):
    path=Path(path)
    if not path.is_absolute() or '..' in path.parts:raise Refused('absolute input path required')
    fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
    try:
        for name in path.parts[1:-1]:
            child=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);os.close(fd);fd=child
        return fd
    except BaseException:os.close(fd);raise
def opened(path,private=False):
    path=Path(path);d=parent(path)
    try:fd=os.open(path.name,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,dir_fd=d)
    finally:os.close(d)
    try:
        m=os.fstat(fd)
        if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.geteuid() or (private and (m.st_nlink!=1 or stat.S_IMODE(m.st_mode)!=0o400)):raise Refused('build input kind/owner/mode/alias')
        return fd,m
    except BaseException:os.close(fd);raise
def stable(fd,path,before):
    d=parent(path)
    try:named=os.stat(Path(path).name,dir_fd=d,follow_symlinks=False)
    finally:os.close(d)
    if stamp(os.fstat(fd))!=stamp(before) or stamp(named)!=stamp(before):raise Refused('build input changed')
def digest_fd(fd,cap,deadline=None):
    os.lseek(fd,0,0);h=hashlib.sha256();size=0
    while True:
        if deadline is not None and time.monotonic()>deadline:raise Refused('build input preparation deadline')
        raw=os.read(fd,65536)
        if not raw:break
        size+=len(raw)
        if size>cap:raise Refused('build input byte cap')
        h.update(raw)
    return h.hexdigest(),size
def read(path,cap=4*1024**2):
    fd,m=opened(path)
    try:
        if m.st_size>cap:raise Refused('input metadata bound')
        raw=bytearray()
        while len(raw)<=cap:
            block=os.read(fd,min(65536,cap+1-len(raw)))
            if not block:break
            raw.extend(block)
        stable(fd,path,m)
        if len(raw)>cap:raise Refused('input metadata bound')
        return bytes(raw)
    finally:os.close(fd)
def private_dir(path):
    path=Path(path);d=parent(path/'placeholder');os.close(d);m=path.lstat()
    if not stat.S_ISDIR(m.st_mode) or m.st_uid!=os.geteuid() or stat.S_IMODE(m.st_mode)!=0o700:raise Refused('owned private input directory required')
def mkdir(path):
    path=Path(path);d=parent(path)
    try:os.mkdir(path.name,0o700,dir_fd=d);os.fsync(d)
    finally:os.close(d)
def create(path,raw,mode=0o400):
    path=Path(path);d=parent(path);fd=None
    try:
        fd=os.open(path.name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=d)
        with os.fdopen(os.dup(fd),'wb') as stream:stream.write(raw);stream.flush()
        os.fchmod(fd,mode);os.fsync(fd);os.fsync(d)
    finally:
        if fd is not None:os.close(fd)
        os.close(d)

def crate_set(lock):
    # Cargo resolves the entire workspace lock even when -p selects one member.
    packages=tomllib.loads(lock.decode())['package']
    crates={}
    for p in sorted(packages,key=lambda p:(p['name'],p['version'])):
        if not p.get('source'):continue
        if p['source']!='registry+https://github.com/rust-lang/crates.io-index':raise Refused('unsupported offline source')
        name=p['name']+'-'+p['version']
        if not re.fullmatch(r'[A-Za-z0-9_.+\-]+',name) or not re.fullmatch(r'[0-9a-f]{64}',p['checksum']):raise Refused('locked crate name/checksum')
        if name+'.crate' in crates:raise Refused('duplicate locked registry package')
        crates[name+'.crate']=p['checksum']
    return crates

def specification(source):
    source=Path(source);lock=read(source/'Cargo.lock');raw=read(source/'scripts/devnet/w0/toolchain-v1.json',65536);pins=json.loads(raw)
    if (set(pins)!={'version','release','triple','channel_manifest_date','channel_manifest_sha256','components'} or pins['version']!=1 or pins['release']!='1.95.0' or pins['triple']!='aarch64-unknown-linux-gnu' or len(pins['components'])!=3 or {p['component'] for p in pins['components']}!={'rustc','cargo','rust-std'}):raise Refused('closed toolchain manifest')
    files={}
    for p in pins['components']:
        expected=p['component']+'-'+pins['release']+'-'+pins['triple']
        directory='rust-std-'+pins['triple'] if p['component']=='rust-std' else p['component']
        if set(p)!={'component','file','sha256','bytes','archive_root','directory'} or p['file']!=expected+'.tar.xz' or p['archive_root']!=expected or p['directory']!=directory or type(p['bytes']) is not int or not 0<p['bytes']<=128*1024**2 or not re.fullmatch('[0-9a-f]{64}',p['sha256']):raise Refused('toolchain component pin')
        files['toolchain/'+p['file']]=dict(sha256=p['sha256'],cap=p['bytes'],bytes=p['bytes'])
    crates=crate_set(lock)
    for name,pin in crates.items():files['crates/'+name]=dict(sha256=pin,cap=32*1024**2,bytes=None)
    header=canonical(dict(version=1,release=pins['release'],triple=pins['triple'],lock_sha256=hashlib.sha256(lock).hexdigest(),toolchain_manifest_sha256=hashlib.sha256(raw).hexdigest(),files={n:p['sha256'] for n,p in files.items()}))
    files['SET.json']=dict(sha256=hashlib.sha256(header).hexdigest(),cap=len(header),bytes=len(header))
    return dict(pins=pins,files=files,header=header,crates=crates)

def copy_bound(source,dest,pin,deadline=None):
    fd,m=opened(source);out=None;d=None
    try:
        actual,size=digest_fd(fd,pin['cap'],deadline);stable(fd,source,m)
        if actual!=pin['sha256'] or (pin['bytes'] is not None and size!=pin['bytes']):raise Refused('build input digest/size mismatch')
        d=parent(dest);out=os.open(Path(dest).name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=d)
        os.lseek(fd,0,0);h=hashlib.sha256();copied=0
        with os.fdopen(os.dup(out),'wb') as stream:
            while True:
                if deadline is not None and time.monotonic()>deadline:raise Refused('build input preparation deadline')
                raw=os.read(fd,65536)
                if not raw:break
                copied+=len(raw)
                if copied>pin['cap'] or copied>size:raise Refused('input grew during copy')
                h.update(raw);stream.write(raw)
            stream.flush()
        stable(fd,source,m)
        if copied!=size or h.hexdigest()!=actual:raise Refused('build input changed during copy')
        os.fchmod(out,0o400);os.fsync(out);os.fsync(d)
        return size
    finally:
        os.close(fd)
        if out is not None:os.close(out)
        if d is not None:os.close(d)

def stage(source,archives,cache,dest,tick=lambda:None,deadline=None):
    started=time.monotonic();spec=specification(source);dest=Path(dest);mkdir(dest);private_dir(dest);mkdir(dest/'toolchain');mkdir(dest/'crates');size=0
    for name,pin in spec['files'].items():
        if name=='SET.json':continue
        tick()
        root=Path(archives) if name.startswith('toolchain/') else Path(cache)
        size+=copy_bound(root/Path(name).name,dest/name,pin,deadline)
    create(dest/'SET.json',spec['header']);verify(dest,spec)
    return dict(version=1,files=len(spec['files']),crates=len(spec['crates']),bytes=size,seconds=time.monotonic()-started)

def verify(root,spec):
    started=time.monotonic();root=Path(root);private_dir(root);actual=set();directories={'toolchain','crates'}
    for base,dirs,files in os.walk(root,followlinks=False):
        for name in dirs:
            path=Path(base)/name;relative=path.relative_to(root).as_posix()
            if relative not in directories:raise Refused('unexpected staged input directory')
            private_dir(path)
        for name in files:
            path=Path(base)/name;m=path.lstat()
            if not stat.S_ISREG(m.st_mode) or m.st_nlink!=1 or m.st_uid!=os.geteuid() or stat.S_IMODE(m.st_mode)!=0o400:raise Refused('staged input kind/owner/mode/alias')
            actual.add(path.relative_to(root).as_posix())
    if actual!=set(spec['files']):raise Refused('staged input set differs from pinned registry set')
    size=0
    for name,pin in spec['files'].items():
        fd,m=opened(root/name,True)
        try:
            value,count=digest_fd(fd,pin['cap']);stable(fd,root/name,m)
            if value!=pin['sha256'] or (pin['bytes'] is not None and count!=pin['bytes']):raise Refused('staged input digest/size mismatch')
            size+=count
        finally:os.close(fd)
    return dict(version=1,files=len(actual),crates=len(spec['crates']),bytes=size,seconds=time.monotonic()-started)

def authenticated(path,pin,private=False):
    fd,m=opened(path,private)
    try:
        value,count=digest_fd(fd,pin['cap']);stable(fd,path,m)
        if value!=pin['sha256'] or (pin['bytes'] is not None and count!=pin['bytes']):raise Refused('archive digest/size mismatch before extraction')
        os.lseek(fd,0,0);return fd,m
    except BaseException:os.close(fd);raise

def directories(path,root):
    relative=Path(path).relative_to(root);current=Path(root)
    for part in relative.parts:
        current/=part
        try:mkdir(current)
        except FileExistsError:private_dir(current)

def unpack(fd,root,prefix,skip_manifest=False,distribution_root=None):
    """Strict regular-file subset; no tarfile extraction or link-following API."""
    root=Path(root);prefix=tuple(prefix);hashes={};total=0;members=0
    with os.fdopen(os.dup(fd),'rb') as stream,tarfile.open(fileobj=stream,mode='r:*') as archive:
        for member in archive:
            members+=1
            path=PurePosixPath(member.name)
            if (members>200000 or path.is_absolute() or '\\' in member.name or any(p in ('.','..') for p in path.parts) or not(member.isfile() or member.isdir())):raise Refused('archive path/type/member bound')
            if tuple(path.parts[:len(prefix)])!=prefix:
                if distribution_root is not None and path.parts and path.parts[0]==distribution_root:continue
                raise Refused('archive root differs from pinned package')
            parts=path.parts[len(prefix):]
            if not parts:
                if not member.isdir():raise Refused('archive root is not a directory')
                continue
            relative=PurePosixPath(*parts).as_posix()
            if skip_manifest and relative=='manifest.in':continue
            if relative=='.cargo-checksum.json':raise Refused('archive supplies a reserved checksum manifest')
            dest=root.joinpath(*parts)
            if member.isdir():directories(dest,root);continue
            if member.size<0 or member.size>512*1024**2 or total+member.size>2*1024**3:raise Refused('archive expansion bound')
            total+=member.size;directories(dest.parent,root);d=parent(dest);out=None
            try:
                out=os.open(dest.name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=d);h=hashlib.sha256();left=member.size
                with archive.extractfile(member) as source,os.fdopen(os.dup(out),'wb') as target:
                    while left:
                        block=source.read(min(left,65536))
                        if not block:raise Refused('short archive member')
                        target.write(block);h.update(block);left-=len(block)
                    target.flush()
                os.fchmod(out,0o444|(member.mode&0o111));os.fsync(out);os.fsync(d);hashes[relative]=h.hexdigest()
            finally:
                if out is not None:os.close(out)
                os.close(d)
    return dict(files=hashes,bytes=total,members=members)

def vendor_crates(crates,cache,dest,private=False):
    """Directory source authenticated by the lock's package and per-file digests."""
    started=time.monotonic();cache,dest=Path(cache),Path(dest)
    # Authenticate the entire set before creating any output directory.
    for name,pin in crates.items():
        fd,_=authenticated(cache/name,dict(sha256=pin,cap=32*1024**2,bytes=None),private);os.close(fd)
    mkdir(dest);total=files=0
    for name,pin in crates.items():
        fd,m=authenticated(cache/name,dict(sha256=pin,cap=32*1024**2,bytes=None),private)
        try:
            package=dest/name[:-6];mkdir(package);row=unpack(fd,package,(name[:-6],));stable(fd,cache/name,m)
            create(package/'.cargo-checksum.json',canonical(dict(package=pin,files=row['files'])))
            total+=row['bytes'];files+=len(row['files'])
            if total>2*1024**3:raise Refused('vendored closure expansion bound')
        finally:os.close(fd)
    return dict(crates=len(crates),files=files,bytes=total,seconds=time.monotonic()-started)

def configure_cargo(home,vendor):
    home=Path(home);mkdir(home)
    raw=('[net]\noffline = true\n[source.crates-io]\nreplace-with = "w0-vendor"\n[source.w0-vendor]\ndirectory = '+json.dumps(str(Path(vendor)))+'\n').encode()
    create(home/'config.toml',raw)

def install_guest(source,run):
    if platform.system()!='Linux':raise Refused('Linux guest installation only; no host toolchain extraction')
    import subprocess
    started=time.monotonic();source,run=Path(source),Path(run);spec=specification(source);inputs=run/'build-inputs'
    verified=verify(inputs,spec)  # Every rejection above precedes every extraction.
    prefix=run/'toolchain';mkdir(prefix);bytes_written=0;begin=time.monotonic()
    for component in spec['pins']['components']:
        name='toolchain/'+component['file'];fd,m=authenticated(inputs/name,spec['files'][name],True)
        try:
            # Distribution metadata is outside the payload directory. Install no
            # script and run no installer or ldconfig; merge just the pinned payload.
            row=unpack(fd,prefix,(component['archive_root'],component['directory']),True,component['archive_root'])
            if not row['files']:raise Refused('empty pinned component payload')
            bytes_written+=row['bytes'];stable(fd,inputs/name,m)
        finally:os.close(fd)
    toolchain_seconds=time.monotonic()-begin
    vendored=vendor_crates(spec['crates'],inputs/'crates',run/'vendor',True);configure_cargo(run/'cargo-home',run/'vendor')
    env=dict(PATH=str(prefix/'bin')+':/usr/bin:/bin',HOME=str(run/'home'),CARGO_HOME=str(run/'cargo-home'),RUSTUP_AUTO_INSTALL='0',CARGO_NET_OFFLINE='true',LC_ALL='C')
    versions={}
    for tool in ('rustc','cargo'):
        result=subprocess.run([str(prefix/'bin'/tool),'-Vv'],env=env,stdin=subprocess.DEVNULL,capture_output=True,timeout=5,check=True)
        if len(result.stdout)+len(result.stderr)>65536 or not result.stdout.startswith((tool+' 1.95.0 ').encode()):raise Refused('run-local compiler version')
        versions[tool]=result.stdout.decode()
    if 'host: '+spec['pins']['triple'] not in versions['rustc']:raise Refused('run-local compiler triple')
    receipt=dict(version=1,verified=verified,toolchain_bytes=bytes_written,toolchain_seconds=toolchain_seconds,vendored=vendored,seconds=time.monotonic()-started,versions=versions,environment_names=sorted(env),no_installer_or_rustup=True)
    create(run/'build-inputs-install.json',canonical(receipt));return receipt

if __name__=='__main__':
    import sys
    if len(sys.argv)!=4 or sys.argv[1]!='install-v1':raise Refused('closed guest input invocation')
    install_guest(Path(sys.argv[2]),Path(sys.argv[3]))
