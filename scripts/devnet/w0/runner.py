#!/usr/bin/python3 -IB
"""Pinned W0 fixture runner. No production permission and no non-Linux fallback."""
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import sys
import time

heartbeat=globals().get('_W0_HEARTBEAT')
if heartbeat is None:
    _spec=importlib.util.spec_from_file_location('w0_heartbeat',Path(__file__).with_name('heartbeat.py'))
    heartbeat=importlib.util.module_from_spec(_spec);_spec.loader.exec_module(heartbeat)
class Refused(ValueError):
    """Missing, foreign or changed operator-owned fixture inputs."""

# Bind the full workspace registry-set derivation along with Cargo.lock.
SOURCES = (
    'crates/boole-lean-runner/src/lib.rs',
    'crates/boole-lean-runner/src/w0_types.rs',
    'crates/boole-lean-runner/src/w0_linux.rs',
    'crates/boole-lean-runner/src/w0_linux_payload.rs',
    'crates/boole-gate-supervisor/src/real_w0.rs',
    'crates/boole-gate-supervisor/src/w0_fixture.rs',
    'crates/boole-gate-supervisor/src/bin/boole-w0-probe.rs',
    'scripts/devnet/w0/runner.py',
    'scripts/devnet/w0/guest.sh',
    'scripts/devnet/w0/window.py',
    'scripts/zk_registry/data/acceptance_tools_v2.json',
    'scripts/devnet/w0/authority.py', 'scripts/devnet/w0/preflight.py',
    'crates/boole-gate-supervisor/Cargo.toml', 'crates/boole-lean-runner/Cargo.toml',
    'Cargo.lock', 'rust-toolchain.toml',
    'scripts/devnet/w0/custody.py', 'scripts/devnet/w0/budget.py', 'scripts/devnet/w0/kernel_check.py', 'scripts/devnet/w0/heartbeat.py',
    'scripts/devnet/w0/inventory.py',
    'scripts/devnet/w0/inputs.py', 'scripts/devnet/w0/toolchain-v1.json',
)
BWRAP = {'aarch64': 'cd6b143283b464baf078ab09ca19dadc5bcc2423b833c4f9dd7e83f9ba1b640b',
         'x86_64': 'e318903862396f96de3df57264e0158682b952fd3fb53ac23d876413e7b30f71'}
CASES = ('SBX-network', 'SBX-filesystem', 'SBX-exec', 'SBX-capabilities', 'SBX-tasks',
         'LIFE-preparation', 'LIFE-deadlines', 'LIFE-tree', 'LIFE-parent-kill', 'LIFE-faults')

def exact(value, keys):
    if type(value) is not dict or set(value) != set(keys):
        raise Refused('closed fixture fields')
    return value

def integer(value):
    return type(value) is int and 0 <= value < 2**64

def validate_grant(value, now, skew=0):
    exact(value, ('state', 'owner', 'step', 'start_unix', 'end_unix', 'request_id', 'generation'))
    if (value['state'] != 'granted' or value['owner'] != 'N' or value['step'] not in ('W0','W0-MASK')
            or not integer(value['generation']) or not value['generation']
            or not isinstance(value['request_id'], str)
            or not re.fullmatch(r'[A-Za-z0-9_-]{1,96}', value['request_id'])
            or not integer(value['start_unix']) or not integer(value['end_unix'])
            or not 0 <= value['end_unix'] - value['start_unix'] <= (900 if value['step']=='W0-MASK' else 2700)
            or not (value['start_unix'] <= now+skew and now <= value['end_unix'] - 130+skew)):
        raise Refused('W0 grant is not current, owned and bounded')
    return value

def validate_live(path,grant,now):
    try:value,_=heartbeat.live_read(path,grant,now)
    except (OSError,ValueError) as e:raise Refused(str(e)) from e
    validate_grant(grant,now,skew=30)
    return value

def parent_fd(path):
    path = Path(path)
    if not path.is_absolute() or any(p in ('.', '..') for p in path.parts):
        raise Refused('absolute canonical fixture path required')
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for component in path.parts[1:-1]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                            dir_fd=fd)
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise

def stamp(entry):
    return (entry.st_dev, entry.st_ino, entry.st_mode, entry.st_uid, entry.st_nlink,
            entry.st_size, entry.st_mtime_ns, entry.st_ctime_ns)

def read_file(path, cap, private=False, atomic_snapshot=False):
    path = Path(path)
    directory = parent_fd(path)
    fd = None
    try:
        fd = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                     dir_fd=directory)
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_size > cap:
            raise Refused('fixture file kind/size')
        if private and (before.st_uid != os.geteuid() or before.st_nlink not in ((0,1) if atomic_snapshot else (1,))
                        or stat.S_IMODE(before.st_mode) not in (0o400, 0o600)):
            raise Refused('private fixture file owner/mode/alias')
        fcntl.fcntl(fd, fcntl.F_SETFL, fcntl.fcntl(fd, fcntl.F_GETFL) & ~os.O_NONBLOCK)
        data = bytearray()
        while len(data) <= cap:
            chunk = os.read(fd, min(65536, cap + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        named = os.stat(path.name, dir_fd=directory, follow_symlinks=False)
        after=os.fstat(fd)
        stable=lambda v:(v.st_dev,v.st_ino,v.st_mode,v.st_uid,v.st_size,v.st_mtime_ns)
        changed=(stable(after)!=stable(before) or after.st_nlink>1) if atomic_snapshot else (stamp(after)!=stamp(before) or stamp(named)!=stamp(before))
        if len(data)>cap or changed:
            raise Refused('fixture file changed')
        return bytes(data)
    finally:
        if fd is not None:
            os.close(fd)
        os.close(directory)

def read_private(path, cap):
    return read_file(path, cap, private=True)

def write_new(path, data, mode):
    path = Path(path)
    directory = parent_fd(path)
    fd = None
    try:
        fd = os.open(path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=directory)
        with os.fdopen(os.dup(fd), 'wb') as stream:
            stream.write(data)
            stream.flush()
        os.fchmod(fd, mode)
        os.fsync(fd)
        os.fsync(directory)
    finally:
        if fd is not None:
            os.close(fd)
        os.close(directory)

def copy_external(source, target):
    # Cargo's external Linux binary can have nlink=2. Import its bytes; never
    # chmod that source. Only the private store copy is required to be nlink=1.
    raw = read_file(source, 128 * 1024**2)
    write_new(target, raw, 0o500)
    return hashlib.sha256(raw).hexdigest()

def decode(raw):
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise Refused('duplicate fixture key')
            out[key] = value
        return out
    try:
        return json.loads(raw, object_pairs_hook=pairs)
    except (ValueError, UnicodeError) as error:
        raise Refused('fixture JSON') from error

def absolute(value):
    return (isinstance(value, str) and value.startswith('/') and value != '/'
            and value == os.path.normpath(value) and '\x00' not in value)

def pin(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) and value != '0' * 64

def parse_bundle(value, root, arch):
    exact(value, ('version', 'backend', 'grant', 'grant_origin', 'lock', 'source_sha256'))
    backend = exact(value['backend'], ('version', 'probe', 'comparator', 'nanoda',
                     'probe_sha256', 'bwrap', 'bwrap_sha256', 'cgroup', 'live_grant', 'grant_sha256'))
    if (type(value['version']) is not int or value['version'] != 1
            or type(backend['version']) is not int or backend['version'] != 1
            or arch not in BWRAP or backend['bwrap'] != '/usr/bin/bwrap'
            or backend['bwrap_sha256'] != BWRAP[arch]
            or backend['live_grant']!=str(root.parent/'live-grant.json') or not pin(backend['grant_sha256'])
            or not pin(backend['probe_sha256']) or not pin(value['source_sha256'])
            or not absolute(backend['cgroup'])
            or not backend['cgroup'].startswith('/sys/fs/cgroup/')
            or not all(backend[name] == str(root / name) for name in ('probe', 'comparator', 'nanoda'))
            or value['grant'] != str(root / 'grant.json') or value['lock'] != str(root / 'window.lock')
            or value['grant_origin'] != str(root.parent / 'host-grant.json')):
        raise Refused('fixture bundle identity/path/pins')
    return value

def source_digest(root):
    digest = hashlib.sha256(b'BOOLE-M3/W0-SOURCE/v1')
    for name in SOURCES:
        raw = read_file(Path(root) / name, 4 * 1024**2)
        encoded = name.encode()
        digest.update(len(encoded).to_bytes(4, 'big'))
        digest.update(encoded)
        digest.update(len(raw).to_bytes(8, 'big'))
        digest.update(raw)
    return digest.hexdigest()

def private_directory(path):
    path = Path(path)
    entry = path.lstat()
    if not stat.S_ISDIR(entry.st_mode) or entry.st_uid != os.geteuid() or stat.S_IMODE(entry.st_mode) != 0o700:
        raise Refused('private owned mode0700 directory required')
    directory = parent_fd(path / 'placeholder')
    os.close(directory)

def prepare(source, executable, root, grant_path, cgroup):
    if platform.system() != 'Linux':
        raise Refused('Linux fixture only; no host fallback')
    root = Path(root)
    if Path(grant_path) != root.parent / 'host-grant.json':
        raise Refused('separate externally granted origin required')
    grant_raw = read_private(grant_path, 16384)
    grant = validate_grant(decode(grant_raw), int(time.time()),skew=30)
    validate_live(Path(grant_path).with_name('live-grant.json'),grant,int(time.time()))
    expected = source_digest(source)
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    result = subprocess.run([str(executable), 'identity-v1'], env=env, capture_output=True,
                            timeout=5, check=True)
    if result.stdout != (expected + '\n').encode() or len(result.stderr) > 65536:
        raise Refused('compiled fixture source identity')
    root.mkdir(mode=0o700)
    private_directory(root)
    hashes = [copy_external(executable, root / name) for name in ('probe', 'comparator', 'nanoda')]
    if len(set(hashes)) != 1:
        raise Refused('fixture import changed')
    write_new(root / 'heartbeat.py', read_file(Path(source) / 'scripts/devnet/w0/heartbeat.py', 65536), 0o400)
    write_new(root / 'runner.py', read_file(Path(source) / 'scripts/devnet/w0/runner.py', 65536), 0o500)
    write_new(root / 'grant.json', grant_raw, 0o400)
    write_new(root / 'window.lock', json.dumps({'request_id': grant['request_id'],
              'generation': grant['generation']}, sort_keys=True).encode(), 0o600)
    arch = platform.machine()
    value = dict(version=1, backend=dict(version=1, probe=str(root / 'probe'),
                 comparator=str(root / 'comparator'), nanoda=str(root / 'nanoda'),
                 probe_sha256=hashes[0], bwrap='/usr/bin/bwrap', bwrap_sha256=BWRAP[arch],
                 cgroup=str(cgroup),live_grant=str(root.parent/'live-grant.json'),grant_sha256=hashlib.sha256(json.dumps(grant,sort_keys=True,separators=(',',':')).encode()).hexdigest()), grant=str(root / 'grant.json'),
                 grant_origin=str(root.parent / 'host-grant.json'), lock=str(root / 'window.lock'),
                 source_sha256=expected)
    parse_bundle(value, root, arch)
    write_new(root / 'bundle.json', (json.dumps(value, sort_keys=True, separators=(',', ':')) + '\n').encode(), 0o400)
    return value

def watch_live(root,parent):
    import signal,select
    if platform.system()!='Linux' or int(parent)!=os.getppid():raise Refused('owned Linux watcher parent required')
    fd=os.pidfd_open(int(parent))
    if int(parent)<=1 or int(parent)!=os.getppid():os.close(fd);raise Refused('watcher parent identity changed')
    guest=None
    try:
        grant=validate_grant(decode(read_private(Path(root)/'host-grant.json',16384)),int(time.time()),skew=30)
        guest=heartbeat.Guest(root,grant,0)
        initial_path=Path(root)/'live-initial.bin'
        initial=read_private(initial_path,heartbeat.SIZE) if initial_path.exists() else None
        guest.step(initial=initial)
        write_new(Path(root)/'live-watch.ready',b'v3',0o600)
        while True:
            if select.select([fd],[],[],0)[0]:raise Refused('watcher parent exited')
            validate_grant(grant,int(time.time()),skew=30)
            closing=False
            if (Path(root)/'live-watch.stop').exists():
                closing=read_private(Path(root)/'live-watch.stop',128)==heartbeat.grant_pin(grant).encode()
                if not closing:raise Refused('watcher close identity')
            if guest.step(closing):break
        write_new(Path(root)/'live-watch.closed',b'v3',0o600)
    except BaseException:
        if guest is not None:
            try:guest.invalidate()
            except (OSError,ValueError):pass
        try:signal.pidfd_send_signal(fd,signal.SIGTERM)
        except ProcessLookupError:pass
        raise
    finally:
        if guest is not None:guest.close()
        os.close(fd)

def run(argv):
    if len(argv)==3 and argv[0]=='watch-live-v3':
        watch_live(argv[1],argv[2]);return
    if len(argv)==2 and argv[0]=='check-boot-v3':
        validate_grant(decode(read_private(argv[1],16384)),int(time.time()),skew=30);return
    if len(argv) == 2 and argv[0] == 'check-grant-v1':
        grant=validate_grant(decode(read_private(argv[1],16384)),int(time.time()),skew=30)
        validate_live(Path(argv[1]).with_name('live-grant.json'),grant,int(time.time()))
        return
    if len(argv) == 2 and argv[0] == 'source-id-v1':
        print(source_digest(argv[1]))
        return
    if platform.system() != 'Linux':
        raise Refused('Linux fixture only; no host fallback')
    if len(argv)==2 and argv[0]=='diagnose-share-v3':
        heartbeat.diagnostic(Path(argv[1]));return
    if len(argv)==2 and argv[0]=='request-close-v3':
        root=Path(argv[1]);private_directory(root);grant=decode(read_private(root/'host-grant.json',16384))
        write_new(root/'live-watch.stop',heartbeat.grant_pin(grant).encode(),0o600);return
    if len(argv) == 6 and argv[0] == 'prepare-v1':
        prepare(*argv[1:])
        return
    if len(argv) != 4 or argv[0] != 'probe-v1' or argv[1] not in CASES:
        raise Refused('closed W0 runner invocation')
    root = Path(__file__).resolve().parent
    private_directory(root)
    scratch = Path(argv[3])
    private_directory(scratch)
    bundle = parse_bundle(decode(read_private(root / 'bundle.json', 16384)), root, platform.machine())
    grant_raw = read_private(bundle['grant'], 16384)
    if read_private(bundle['grant_origin'], 16384) != grant_raw:
        raise Refused('W0 grant revoked or changed')
    grant = validate_grant(decode(grant_raw), int(time.time()),skew=30)
    validate_live(Path(bundle['backend']['live_grant']),grant,int(time.time()))
    if grant['request_id'] != argv[2]:
        raise Refused('W0 request binding')
    for name in ('probe', 'comparator', 'nanoda'):
        if hashlib.sha256(read_file(bundle['backend'][name], 128 * 1024**2)).hexdigest() != bundle['backend']['probe_sha256']:
            raise Refused('W0 fixture executable pin')
    env = {'PATH': '/usr/bin:/bin', 'HOME': str(scratch), 'TMPDIR': str(scratch), 'LC_ALL': 'C'}
    os.execve(bundle['backend']['probe'], [bundle['backend']['probe'], *argv, str(root / 'bundle.json')], env)

if __name__ == '__main__':
    try:
        run(sys.argv[1:])
    except (Refused, OSError, subprocess.SubprocessError) as error:
        print('W0 fixture unavailable: ' + str(error), file=sys.stderr)
        raise SystemExit(2)
