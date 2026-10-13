#!/usr/bin/python3
"""Pinned production boundary: namespaces, native seccomp, delegated cgroups and PID-1 evidence."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import shutil
import sys
import uuid
import time


def digest(path):
    """Hash a small executed boundary component before every invocation."""
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def bundle(authority):
    """Require the exact pristine tracked descriptor and every architecture component."""
    root = Path(__file__).resolve().parent
    data_path = root / 'bundle.json'
    if digest(data_path) != authority:
        raise ValueError('descriptor-integrity')
    data = json.loads(data_path.read_text())['sandbox']
    profile = data['architectures'][platform.machine()]
    paths = {'launcher': Path(__file__).resolve(), 'isolate': root / 'gate-isolate',
             'inner_launcher': root / 'comparator-landrun',
             'filter': root / ('seccomp-' + platform.machine() + '.bpf'), 'bwrap': Path('/usr/bin/bwrap'),
             'apparmor_profile': Path('/etc/apparmor.d/boole-gate-bwrap')}
    for name, file in paths.items():
        expected = data[name + '_sha256'] if name in {'launcher', 'inner_launcher'} else profile[name + '_sha256']
        if digest(file) != expected or file.stat().st_mode & 0o6000:
            raise ValueError('component-integrity:' + name)
        if name in {'launcher', 'isolate', 'inner_launcher', 'bwrap'} and not os.access(file, os.X_OK):
            raise ValueError('component-not-executable:' + name)
    if paths['apparmor_profile'].stat().st_uid != 0 or paths['bwrap'].stat().st_uid != 0:
        raise ValueError('untrusted-system-component')
    return root, paths


def landrun_arguments(arguments):
    """Accept the existing read/write/environment CLI without accepting fallback switches."""
    readers, writers, environment = [], [], {}
    index = 0
    while index < len(arguments):
        flag = arguments[index]
        index += 1
        if flag == '--':
            break
        if flag in {'--add-exec', '--ldd'}:
            continue
        if flag not in {'--ro', '--rw', '--rwx', '--env'} or index >= len(arguments):
            raise ValueError('unsupported-launcher-argument')
        value = arguments[index]
        index += 1
        if flag == '--env':
            key, separator, text = value.partition('=')
            if not separator or not key:
                raise ValueError('invalid-environment')
            environment[key] = text
        elif flag == '--ro':
            readers.append(str(Path(value).resolve()))
        else:
            writers.append(str(Path(value).resolve()))
    if index >= len(arguments):
        raise ValueError('missing-command')
    return readers, writers, environment, arguments[index:]


def create_step(root, memory, pids):
    """Create an owned domain and separate init/payload groups; the payload cannot see their handles."""
    if root.stat().st_uid != os.getuid() or os.getuid() == 0:
        raise ValueError('cgroup-not-delegated')
    if not {'memory', 'pids'} <= set((root / 'cgroup.subtree_control').read_text().split()):
        raise ValueError('cgroup-controllers-unavailable')
    step = root / ('step-' + uuid.uuid4().hex)
    step.mkdir(mode=0o700)
    (step / 'memory.max').write_text(str(memory + 32 * 1024 * 1024))
    (step / 'pids.max').write_text(str(pids + 8))
    (step / 'cgroup.subtree_control').write_text('+memory +pids')
    init, payload = step / 'init', step / 'payload'
    init.mkdir()
    payload.mkdir()
    (payload / 'memory.max').write_text(str(memory))
    if (payload / 'memory.swap.max').exists():
        (payload / 'memory.swap.max').write_text('0')
    (payload / 'memory.oom.group').write_text('1')
    (payload / 'pids.max').write_text(str(pids))
    return step, init, payload


def cleanup_step(step, groups, timeout=2.0):
    """Wait for asynchronous cgroup.kill with a bound; never replace the payload verdict."""
    errors = []
    try:
        if 'populated 1' in (step / 'cgroup.events').read_text():
            (step / 'cgroup.kill').write_text('1')
        deadline, delay = time.monotonic() + timeout, .001
        while 'populated 1' in (step / 'cgroup.events').read_text():
            if time.monotonic() >= deadline:
                errors.append('population-timeout')
                break
            time.sleep(min(delay, max(0, deadline - time.monotonic())))
            delay = min(.02, delay * 2)
        if not errors:
            for group in groups:
                try:
                    group.rmdir()
                except OSError as error:
                    errors.append('rmdir:' + group.name + ':' + str(error.errno))
    except OSError as error:
        errors.append('cleanup-io:' + str(error.errno))
    return dict(complete=not errors, errors=errors)


def execute(options, arguments):
    """Run one actual payload and preserve deterministic descendant/resource precedence."""
    root, paths = bundle(options.authority)
    sysctl = Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns')
    if sysctl.exists() and sysctl.read_text().strip() != '1':
        raise ValueError('apparmor-userns-restriction-disabled')
    if not 1 <= options.pids <= 1024 or not 16 * 1024 * 1024 <= options.memory <= 20000000000 - 32 * 1024**2:
        raise ValueError('step-limits-outside-node-envelope')
    readers, writers, environment, payload_command = landrun_arguments(arguments)
    environment.update(GOMAXPROCS='4', CARGO_BUILD_JOBS='4', RAYON_NUM_THREADS='4', RUST_TEST_THREADS='4')
    separator = arguments.index('--')
    arguments = [*arguments[:separator], *sum((['--env', key + '=' + value] for key, value in
                                              environment.items()), []), *arguments[separator:]]
    executable = shutil.which(payload_command[0], path=environment.get('PATH', '/usr/bin:/bin'))
    if executable is None:
        raise ValueError('executable-unavailable')
    executable = str(Path(executable).resolve())
    readers.append(executable)
    separator = arguments.index('--')
    arguments = [*arguments[:separator + 1], executable, *payload_command[1:]]
    if any(Path(reader) in {Path('/'), Path('/proc'), Path('/sys'), Path('/dev')} for reader in readers):
        raise ValueError('boundary-read-exposure')
    for output in writers:
        directory = Path(output)
        if directory == Path('/') or root.is_relative_to(directory) or directory.is_relative_to(root):
            raise ValueError('boundary-write-exposure')
    step, init, payload = create_step(Path(options.cgroup), options.memory, options.pids)
    handles = []
    events_file = Path(options.status).with_suffix('.events.ndjson')
    report_fd = os.open(events_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    record = {}
    try:
        for file, mode in ((payload / 'cgroup.procs', os.O_WRONLY), (payload / 'cgroup.freeze', os.O_WRONLY),
                           (payload / 'cgroup.events', os.O_RDONLY), (payload / 'cgroup.procs', os.O_RDONLY),
                           (payload / 'memory.events', os.O_RDONLY), (payload / 'pids.events', os.O_RDONLY),
                           (payload / 'cgroup.kill', os.O_WRONLY)):
            handles.append(os.open(file, mode))
        init_fd = os.open(init / 'cgroup.procs', os.O_WRONLY)
        handles.append(report_fd)
        bwrap = [str(paths['bwrap']), '--unshare-user', '--unshare-pid', '--unshare-ipc', '--unshare-uts',
                 '--disable-userns', '--assert-userns-disabled', '--die-with-parent', '--new-session',
                 '--as-pid-1', '--uid', '0', '--gid', '0', '--cap-drop', 'ALL',
                 '--cap-add', 'CAP_SETPCAP', '--clearenv']
        if not options.no_seccomp:
            filter_fd = os.open(paths['filter'], os.O_RDONLY)
            handles.append(filter_fd)
            bwrap += ['--seccomp', str(filter_fd)]
        mounts = ['--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp']
        for file in sorted(set(readers + [str(root)]), key=lambda name: (len(Path(name).parts), name)):
            if file in {'/dev/null', '/dev/random', '/dev/urandom', '/dev/zero'}:
                continue
            mounts += ['--ro-bind', file, file]
        for file in sorted(set(writers), key=lambda name: (len(Path(name).parts), name)):
            if file == '/dev/null':
                continue
            mounts += ['--bind', file, file]
        for link, destination in (('/bin', 'usr/bin'), ('/lib', 'usr/lib'), ('/lib64', 'usr/lib64')):
            if not any(Path(link).is_relative_to(Path(reader)) for reader in readers):
                mounts += ['--symlink', destination, link]
        mounts += ['--dir', os.getcwd(), '--chdir', os.getcwd()]
        bwrap += mounts
        for key, value in sorted(environment.items()):
            bwrap += ['--setenv', key, value]
        bwrap += [str(paths['isolate']), 'init', *map(str, handles[:8]), '0' if options.no_net else '1', '--',
                  str(paths['isolate']), 'landlock', *arguments]
        if not options.no_net:
            bwrap = [str(paths['bwrap']), '--unshare-user', '--unshare-net', '--unshare-pid', '--as-pid-1',
                     '--uid', '0', '--gid', '0',
                     '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--cap-add', 'ALL',
                     '--clearenv', '--setenv', 'PATH', '/usr/bin:/bin',
                     *mounts, str(paths['isolate']), 'network', *bwrap]
        command = [str(paths['isolate']), 'join', str(init_fd), '--', *bwrap]
        child = subprocess.Popen(command, pass_fds=tuple(handles + [init_fd]),
                                 env={'PATH': '/usr/bin:/bin', 'LANG': 'C'}, close_fds=True)
        os.close(init_fd)
        os.close(report_fd)
        handles.remove(report_fd)
        code = child.wait()
        observation = None
        events = []
        counts = {'orphan': 0, 'frozen-member': 0}
        with events_file.open() as stream:
            for line in stream:
                event = json.loads(line)
                events.append(event)
                if event['event'] == 'final':
                    observation = event
                else:
                    counts[event['event']] += 1
        if observation is None:
            raise ValueError('pid-1-receipt-unavailable')
        if (counts['orphan'] != observation['orphan_events_recorded']
                or counts['orphan'] != min(256, observation['orphans_reaped'])
                or counts['frozen-member'] != observation['frozen_members']):
            raise ValueError('pid-1-event-integrity')
        if observation['oom_kill_events']:
            verdict, reason = 'ERROR', 'resource-unavailable'
        elif observation['pids_max_events']:
            verdict, reason = ('ERROR', 'resource-unavailable') if options.role == 'reference' else (
                'INVALID', 'resource-cap')
        elif observation['orphans_reaped'] or observation['frozen_members']:
            verdict, reason = 'INVALID', 'owned-descendants-remained'
        elif observation['setup_failed']:
            verdict, reason = 'ERROR', 'sandbox-unavailable'
        else:
            verdict, reason = 'PASS', None
        host_namespaces = {name: os.readlink('/proc/self/ns/' + name) for name in ('user', 'net', 'pid', 'mnt', 'ipc')}
        if (observation['landlock_abi'] < 4 or observation['apparmor_label'] != 'boole-gate-bwrap (unconfined)'
                or observation['pid_1_dumpable'] != 0
                or observation['cpu_affinity_count'] != 4
                or observation['seccomp_mode'] != (0 if options.no_seccomp else 2)
                or any(observation['namespaces'][name] == value for name, value in host_namespaces.items()
                       if name != 'net' or not options.no_net)
                or (not options.no_net and observation['loopback_up'])
                or any(observation[name] for name in ('caps_effective', 'caps_ambient', 'caps_bounding'))):
            verdict, reason = 'ERROR', 'sandbox-unavailable'
        sysctl = Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns')
        if 'populated 1' in (step / 'cgroup.events').read_text():
            (step / 'cgroup.kill').write_text('1')
            observation['outer_members_had_to_be_killed'] = True
            if verdict != 'ERROR':
                verdict, reason = 'INVALID', 'owned-descendants-remained'
        peak = payload / 'pids.peak'
        if not peak.is_file():
            raise ValueError('pids-peak-unavailable')
        record.update(protocol=1, verdict=verdict, reason=reason, child_exit=observation['main_exit'],
                    assurance=options.mode, utc=datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'),
                    kernel=platform.release(), apparmor_userns_sysctl=sysctl.read_text().strip() if sysctl.exists() else None,
                    apparmor_profile_sha256=digest(paths['apparmor_profile']), seccomp_sha256=digest(paths['filter']),
                    cgroup_controllers=['memory', 'pids'], memory_max=options.memory,
                    whole_step_memory_max=options.memory + 32 * 1024**2, pids_max=options.pids,
                    descriptor_sha256=options.authority, observation=observation,
                    stage=options.stage,
                    peak_tasks=int(peak.read_text()),
                    pid_1_events_path=str(events_file), pid_1_events_sha256=digest(events_file),
                    pid_1_events=events,
                    pid_1_events_bytes=events_file.stat().st_size)
        return record
    finally:
        for fd in handles:
            try:
                os.close(fd)
            except OSError:
                pass
        record['cleanup'] = cleanup_step(step, (payload, init, step))


def main():
    """Keep boundary status off payload stdout and always report missing layers as availability errors."""
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--gate-authority', dest='authority', required=True)
    parser.add_argument('--gate-cgroup', dest='cgroup', required=True)
    parser.add_argument('--gate-status', dest='status', required=True)
    parser.add_argument('--gate-memory', dest='memory', type=int, default=11000 * 1024 * 1024)
    parser.add_argument('--gate-pids', dest='pids', type=int, default=128)
    parser.add_argument('--gate-stage', dest='stage', default='native-run')
    parser.add_argument('--gate-role', dest='role', choices=('submission', 'reference'), default='submission')
    parser.add_argument('--gate-mode', dest='mode', choices=('production', 'test-fixture'), default='production')
    parser.add_argument('--gate-fixture-no-net', dest='no_net', action='store_true')
    parser.add_argument('--gate-fixture-no-seccomp', dest='no_seccomp', action='store_true')
    options, arguments = parser.parse_known_args()
    try:
        if options.mode == 'production' and (options.no_net or options.no_seccomp):
            raise ValueError('fixture-switch-in-production')
        result = execute(options, arguments)
    except Exception as ex:
        print('BOOLE_SANDBOX_UNAVAILABLE:' + type(ex).__name__ + ':' + str(ex), file=sys.stderr)
        result = dict(protocol=1, verdict='ERROR', reason='sandbox-unavailable', assurance=options.mode,
                      detail=type(ex).__name__ + ':' + str(ex))
        sysctl = Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns')
        profile = Path('/etc/apparmor.d/boole-gate-bwrap')
        result.update(kernel=platform.release(),
                      apparmor_userns_sysctl=sysctl.read_text().strip() if sysctl.exists() else None,
                      apparmor_profile_sha256=digest(profile) if profile.is_file() else None,
                      apparmor_profile_state='not-attested')
    fd = os.open(options.status, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(result, stream, sort_keys=True, indent=2)
        stream.write('\n')
        stream.flush()
    return result.get('child_exit', 1) if result['verdict'] == 'PASS' else 125


if __name__ == '__main__':
    raise SystemExit(main())
