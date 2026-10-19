"""Owned-PID resource guard for correctness generation and production-core checks.

The Lean checker, theorem policy and replay implementation are unchanged. This
runner supplies stricter process observation and cleanup for the local C pipeline.
"""

from __future__ import annotations

from datetime import datetime, timezone
from contextlib import contextmanager
import errno
import gzip
import hashlib
import json
import os
from .safe_fs import Path
from .safe_fs import directory_fd, submission_io
import re
import signal
import stat
import subprocess
import threading
import tempfile
import time
import uuid

from . import lean_runner as L


def processes() -> dict:
    """Observe PID, parent, RSS and start identity without retaining foreign command arguments."""
    output = subprocess.check_output(['/bin/ps', '-axo', 'pid=,ppid=,rss=,lstart='], text=True)
    result = {}
    for line in output.splitlines():
        fields = line.split(None, 7)
        if len(fields) == 8:
            result[int(fields[0])] = (int(fields[1]), int(fields[2]), ' '.join(fields[3:]))
    return result


def terminate_owned(child, owned: dict, observe=processes, grace_seconds: float = 0.25) -> list[str]:
    """Escalate tracked identities even after root exit; never signal an untracked or reused PID."""
    errors = []
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            table = observe()
        except Exception as ex:
            table = {}
            errors.append(type(ex).__name__)
        killed = set()
        for pid in sorted(owned, reverse=True):
            if pid in table and table[pid][2] == owned[pid]:
                try:
                    os.kill(pid, sig)
                    killed.add(pid)
                except ProcessLookupError:
                    pass
        if child.pid not in killed and child.poll() is None:
            try:
                child.terminate() if sig == signal.SIGTERM else child.kill()
            except ProcessLookupError:
                pass
        if sig == signal.SIGTERM:
            time.sleep(grace_seconds)
    return errors


def owner_is_dead(owner):
    """Prove death or PID reuse in the recorded namespace; unknown identity is retained."""
    if not isinstance(owner, dict):
        return False
    pid, start, namespace = owner.get('pid'), owner.get('start_time'), owner.get('pid_namespace')
    if (type(pid) is not int or pid <= 1 or type(start) is not str or not start.isdecimal()
            or type(namespace) is not str or not re.fullmatch(r'pid:\[\d+\]', namespace)):
        return False
    try:
        if os.readlink('/proc/self/ns/pid') != namespace:
            return False
        current_namespace = os.readlink('/proc/' + str(pid) + '/ns/pid')
        with open('/proc/' + str(pid) + '/stat') as stream:
            current_start = stream.read().rsplit(')', 1)[1].split()[19]
        return current_namespace == namespace and current_start != start
    except (ValueError, IndexError):
        return False
    except OSError:
        # ESRCH is meaningful only after establishing the same PID namespace.
        try:
            if os.readlink('/proc/self/ns/pid') != namespace:
                return False
        except OSError:
            return False
    try:
        os.kill(pid, 0)
    except OSError as ex:
        return ex.errno == errno.ESRCH
    return False


def owner_record():
    """Bind Linux ownership to a birth tick and PID namespace, never just a reused number."""
    pid = os.getpid()
    try:
        fd = os.open('/proc/' + str(pid) + '/stat', os.O_RDONLY | os.O_CLOEXEC)
        with os.fdopen(fd) as stream:
            start = stream.read().rsplit(')', 1)[1].split()[19]
        namespace = os.readlink('/proc/' + str(pid) + '/ns/pid')
    except OSError:
        start, namespace = None, None  # non-Linux fixtures cannot claim Linux birth evidence
    return dict(pid=pid, start_time=start, pid_namespace=namespace)


def archive_legacy_receipts(root, directory, files):
    """Make legacy evidence recoverable and durable before any purge."""
    archives = root / '.legacy-archives'
    archives.mkdir(mode=0o700, exist_ok=True)
    rows = []
    for file in files:
        before = file.lstat()
        if before.st_size > 32 * 1024**2:
            raise ValueError('legacy receipt too large; retained')
        raw = file.read_bytes()
        raw_sha = hashlib.sha256(raw).hexdigest()
        name = raw_sha + '.json.gz'
        archive = archives / name
        if not archive.exists():
            with archive.open('xb') as stream:
                stream.write(gzip.compress(raw, mtime=0));stream.flush();os.fsync(stream.fileno())
        compressed = archive.read_bytes()
        if gzip.decompress(compressed) != raw:
            raise ValueError('legacy archive round-trip failed')
        after = file.lstat()
        if (before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_ino, after.st_size, after.st_mtime_ns):
            raise ValueError('legacy receipt changed; retained')
        rows.append(dict(original=str(file), raw_sha256=raw_sha, bytes=len(raw), mtime_ns=before.st_mtime_ns,
                         archive=name, archive_sha256=hashlib.sha256(compressed).hexdigest(),
                         recovery='gzip -dc ARCHIVE > ORIGINAL; verify raw_sha256'))
    with (archives / (uuid.uuid4().hex + '.manifest.json')).open('x') as stream:
        json.dump(dict(directory=str(directory), files=rows), stream, sort_keys=True)
        stream.flush();os.fsync(stream.fileno())
    fd = directory_fd(archives)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


@contextmanager
def receipt_report(guard, directory):
    """Persist consumed receipts even when a caller raises or skips a failed step."""
    try:
        yield
    finally:
        data = dict(process_receipts=guard.step_reports)
        try:
            with submission_io(None):
                Path(directory).mkdir(parents=True, exist_ok=True)
                (Path(directory) / 'process-receipts.json').write_text(json.dumps(data, sort_keys=True) + '\n')
        except OSError as ex:
            # A payload may chmod its root or plant a report symlink. Neither may
            # erase consumed evidence or replace the original boundary exception.
            protected = failed_receipt_directory()
            protected.mkdir(mode=0o700, parents=True, exist_ok=True)
            metadata = protected.lstat()
            if metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
                raise PermissionError('private receipt directory ownership/mode')
            target = protected / (uuid.uuid4().hex + '.json')
            with target.open('x') as stream:
                json.dump(dict(data, report_directory=str(directory), report_write_error=str(ex)), stream)
                stream.flush();os.fsync(stream.fileno())
            guard.failed_receipt_reports = [*getattr(guard, 'failed_receipt_reports', []), str(target)]


def failed_receipt_directory():
    """Production fallback is outside every allowed submission writer root."""
    if os.uname().sysname != 'Linux':
        return Path(tempfile.mkdtemp(prefix='boole-private-receipts-'))
    return Path('/run/user') / str(os.getuid()) / 'boole-gate-receipts/failed-reports'


def sweep_dead_receipts(root):
    """Sweep only owned, flat runner directories with an identifiable dead owner."""
    for directory in root.iterdir():
        try:
            entry = directory.lstat()
            if not stat.S_ISDIR(entry.st_mode) or entry.st_uid != os.getuid():
                continue
            files = list(directory.iterdir())
            if not files or any(not stat.S_ISREG(p.lstat().st_mode) or p.lstat().st_uid != os.getuid()
                                for p in files):
                continue
            owner = directory / '.owner.json'
            if owner in files:
                pid = json.loads(owner.read_text())
            else:
                # Filename-only legacy PIDs have no namespace/birth evidence; retain them.
                continue
            if not owner_is_dead(pid):
                continue
            for file in files:
                if not owner_is_dead(pid):
                    break
                file.unlink()
            else:
                parent = directory_fd(root)
                try:
                    os.rmdir(directory.name, dir_fd=parent)
                finally:
                    os.close(parent)
        except (OSError, ValueError, TypeError, EOFError):
            # An unknown or concurrently changed receipt is retained, never recursively removed.
            continue


def runner(log_directory: str, total_timeout: float | None = None, *, sandbox=None,
           readable_paths: list[str] | None = None, writable_paths: list[str] | None = None):
    """Construct the existing whole-tree owned runner without changing process-wide state."""
    logs = Path(log_directory)
    lock = threading.Lock()
    sequence = 0
    active = 0
    step_reports = []
    deadline = time.monotonic() + total_timeout if total_timeout is not None else None

    def run_process(cmd, env, cwd, timeout, rss_limit_mb=None, watch=None, poll_s=0.25):
        nonlocal active
        with lock:
            active += 1
        try:
            return execute(cmd, env, cwd, timeout, rss_limit_mb, watch, poll_s)
        finally:
            with lock:
                active -= 1
                if not active and getattr(run_process, 'trusted_logs', False):
                    try:
                        owner = logs / '.owner.json'
                        if list(logs.iterdir()) == [owner] and json.loads(owner.read_text()) == owner_record():
                            owner.unlink()
                            parent = directory_fd(logs.parent)
                            try:
                                os.rmdir(logs.name, dir_fd=parent)
                            finally:
                                os.close(parent)
                            run_process.trusted_logs = False
                    except (OSError, ValueError):
                        pass  # unknown evidence is retained for the dead-owner sweep

    def execute(cmd, env, cwd, timeout, rss_limit_mb=None, watch=None, poll_s=0.25):
        nonlocal sequence, logs
        if sandbox is not None:
            cmd = sandbox.command(cmd, env, readable_paths or [], writable_paths or [])
        from .sandbox import registered_command
        with lock:
            if registered_command(cmd) and not getattr(run_process, 'trusted_logs', False):
                root = Path('/run/user') / str(os.getuid()) / 'boole-gate-receipts/processes'
                root.mkdir(mode=0o700, parents=True, exist_ok=True)
                sweep_dead_receipts(root)
                logs = root / (str(os.getpid()) + '-' + uuid.uuid4().hex)
                logs.mkdir(mode=0o700)
                with (logs / '.owner.json').open('x') as stream:
                    json.dump(owner_record(), stream)
                run_process.trusted_logs = True
            logs.mkdir(mode=0o700, parents=True, exist_ok=True)
            run_process.log_directory = str(logs)
            sequence += 1
            label = str(os.getpid()) + '-' + str(sequence)
        started = datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z')
        epoch = time.monotonic()
        if deadline is not None:
            remaining = deadline - epoch
            if remaining <= 0:
                record = dict(argv=cmd, cwd=cwd, start_utc=started, end_utc=started,
                              exit_code=124, child_exit_code=None, reason='checker-deadline',
                              peak_rss_mb=0, rss_limit_mb=rss_limit_mb or 12000,
                              owned_identities={}, cleanup_observation_errors=[], child_started=False)
                result = L.RunResult(124, '', 0, True, False, 0, 'checker-deadline')
                return copy_step_report(label, record, result)
            timeout = min(timeout, remaining)
        child = subprocess.Popen(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        owned, state, stop = {}, dict(reason='', peak=0, errors=[]), threading.Event()
        try:
            table = processes()
            if child.pid in table:
                owned[child.pid] = table[child.pid][2]
        except Exception as ex:
            state['reason'] = 'process-watch-error:' + type(ex).__name__
            state['errors'] += terminate_owned(child, owned)

        def guard():
            while not stop.wait(min(poll_s, 0.25)):
                try:
                    table = processes()
                    if child.pid in table:
                        owned.setdefault(child.pid, table[child.pid][2])
                    changed = True
                    while changed:
                        changed = False
                        for pid, (parent, _, identity) in table.items():
                            if (parent in owned and parent in table and table[parent][2] == owned[parent]
                                    and pid not in owned):
                                owned[pid] = identity
                                changed = True
                    rss = sum(entry[1] for pid, entry in table.items() if owned.get(pid) == entry[2]) / 1024
                    state['peak'] = max(state['peak'], rss)
                    reason = (watch() if watch else '') or ('rss-limit' if rss > (rss_limit_mb or 12000) else '')
                except Exception as ex:
                    reason = 'process-watch-error:' + type(ex).__name__
                if reason:
                    state['reason'] = reason
                    state['errors'] += terminate_owned(child, owned)
                    return

        thread = threading.Thread(target=guard, daemon=True)
        thread.start()
        try:
            output, _ = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            state['reason'] = ('checker-deadline'
                               if deadline is not None and time.monotonic() >= deadline else 'timeout')
            state['errors'] += terminate_owned(child, owned)
            try:
                output, _ = child.communicate(timeout=1)
            except subprocess.TimeoutExpired as ex:
                output = ex.output or b''
                child.stdout.close()
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=1)
                state['errors'].append('owned-or-unobserved descendant retained output pipe')
        except BaseException:
            terminate_owned(child, owned)
            raise
        finally:
            stop.set()
            thread.join(timeout=2)
            try:
                table = processes()
                remaining = [pid for pid in owned if pid != child.pid and pid in table and table[pid][2] == owned[pid]]
                if remaining:
                    state['reason'] = state['reason'] or 'owned-descendants-remained'
                    state['errors'] += terminate_owned(child, owned)
            except Exception as ex:
                state['reason'] = state['reason'] or 'cleanup-watch-error:' + type(ex).__name__
                state['errors'] += terminate_owned(child, owned)
        if not owned:
            state['reason'] = state['reason'] or 'unobserved-owned-root'
        code = 125 if state['reason'] and child.returncode == 0 else child.returncode
        result = L.RunResult(code, output.decode('utf-8', 'replace'), round(time.monotonic() - epoch, 3),
                             state['reason'] in ('timeout', 'checker-deadline'), state['reason'] == 'rss-limit',
                             int(state['peak']), state['reason'])
        from .sandbox import capture_result
        result = capture_result(cmd, result)
        record = dict(argv=cmd, cwd=cwd, start_utc=started,
                      end_utc=datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'),
                      exit_code=code, child_exit_code=child.returncode, reason=state['reason'],
                      peak_rss_mb=state['peak'], rss_limit_mb=rss_limit_mb or 12000,
                      owned_identities=owned, cleanup_observation_errors=state['errors'], boundary=result.boundary)
        return copy_step_report(label, record, result)

    def copy_step_report(label, record, result):
        """Keep the observation in the returned/aggregate report before removing its temporary copy."""
        file = logs / (label + '.json')
        with file.open('x') as stream:
            json.dump(record, stream, indent=2)
        result.process_receipt = json.loads(file.read_text())
        with lock:
            step_reports.append(result.process_receipt)
        if getattr(run_process, 'trusted_logs', False):
            file.unlink()
        return result

    run_process.submission_boundary = sandbox
    run_process.log_directory = str(logs)
    run_process.step_reports = step_reports
    return run_process


def install(log_directory: str, total_timeout: float | None = None, **boundary) -> None:
    """Install the bounded runner for existing single-worker production callers."""
    L.run_process = runner(log_directory, total_timeout, **boundary)
