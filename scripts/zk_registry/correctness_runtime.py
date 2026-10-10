"""Owned-PID resource guard for correctness generation and production-core checks.

The Lean checker, theorem policy and replay implementation are unchanged. This
runner supplies stricter process observation and cleanup for the local C pipeline.
"""

from __future__ import annotations

from datetime import datetime, timezone
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
import time

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


def install(log_directory: str, total_timeout: float | None = None) -> None:
    """Install a bounded runner, retaining process evidence while using the unchanged checker core."""
    logs = Path(log_directory)
    logs.mkdir(parents=True, exist_ok=True)
    lock = threading.Lock()
    sequence = 0
    deadline = time.monotonic() + total_timeout if total_timeout is not None else None

    def run_process(cmd, env, cwd, timeout, rss_limit_mb=None, watch=None, poll_s=0.25):
        nonlocal sequence
        with lock:
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
                with (logs / (label + '.json')).open('x') as stream:
                    json.dump(record, stream, indent=2)
                return L.RunResult(124, '', 0, True, False, 0, 'checker-deadline')
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
        record = dict(argv=cmd, cwd=cwd, start_utc=started,
                      end_utc=datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'),
                      exit_code=code, child_exit_code=child.returncode, reason=state['reason'],
                      peak_rss_mb=state['peak'], rss_limit_mb=rss_limit_mb or 12000,
                      owned_identities=owned, cleanup_observation_errors=state['errors'])
        with (logs / (label + '.json')).open('x') as stream:
            json.dump(record, stream, indent=2)
        return L.RunResult(code, output.decode('utf-8', 'replace'), round(time.monotonic() - epoch, 3),
                           state['reason'] in ('timeout', 'checker-deadline'), state['reason'] == 'rss-limit',
                           int(state['peak']), state['reason'])

    L.run_process = run_process
