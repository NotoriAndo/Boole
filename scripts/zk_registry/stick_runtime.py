"""Owned process guards and a shared 40 GB admission budget for stick rebuilds."""
from __future__ import annotations

from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import uuid

from . import correctness_runtime as CR

GUARD_BYTES = 19_000_000_000


def available_memory():
    if os.uname().sysname == 'Darwin':
        text = subprocess.check_output(['/usr/bin/vm_stat'], text=True)
        page = int(re.search(r'page size of (\d+)', text).group(1))
        return sum(map(int, re.findall(r'^Pages (?:free|inactive):\s+(\d+)', text, re.M))) * page
    text = Path('/proc/meminfo').read_text()
    fields = dict(re.findall(r'^(\w+):\s+(\d+)', text, re.M))
    return (int(fields['MemFree']) + int(fields['Inactive'])) * 1024


@contextmanager
def lease(root, deadline):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    token = uuid.uuid4().hex
    start = time.monotonic()
    acquired = False
    while time.time() < deadline:
        with (root / 'lock').open('a') as mutex:
            fcntl.flock(mutex, fcntl.LOCK_EX)
            state_path = root / 'leases.json'
            rows = json.loads(state_path.read_text()) if state_path.exists() else []
            live = []
            for row in rows:
                try:
                    os.kill(row['pid'], 0)
                    live.append(row)
                except ProcessLookupError:
                    with (root / 'released-dead.jsonl').open('a') as journal:
                        journal.write(json.dumps(row) + '\n')
                except PermissionError:
                    live.append(row)
            if (sum(row['bytes'] for row in live) + GUARD_BYTES <= 40_000_000_000
                    and available_memory() >= 6_000_000_000
                    and shutil.disk_usage(root).free >= 20_000_000_000):
                live.append(dict(token=token, pid=os.getpid(), bytes=GUARD_BYTES))
                state_path.write_text(json.dumps(live) + '\n')
                acquired = True
            else:
                state_path.write_text(json.dumps(live) + '\n')
        if acquired:
            break
        time.sleep(1)
    if not acquired:
        raise TimeoutError('resource-admission-deadline')
    try:
        yield time.monotonic() - start
    finally:
        with (root / 'lock').open('a') as mutex:
            fcntl.flock(mutex, fcntl.LOCK_EX)
            rows = json.loads((root / 'leases.json').read_text())
            (root / 'leases.json').write_text(json.dumps([row for row in rows if row['token'] != token]) + '\n')


def guarded_runner(logs, resources, deadline, seconds=1800):
    logs = Path(logs)
    logs.mkdir(parents=True, exist_ok=True)

    def run(argv, environment, cwd, label):
        attempt = 1
        while (logs / (label + '-' + str(attempt))).exists():
            attempt += 1
        out = logs / (label + '-' + str(attempt))
        out.mkdir()
        with lease(resources, deadline) as queue:
            remaining = min(seconds, deadline - time.time())
            if remaining <= 0:
                raise TimeoutError('build-deadline')
            result = CR.runner(str(out / 'processes'), remaining)(
                list(map(str, argv)), environment, str(cwd), remaining, GUARD_BYTES // 1024**2)
        (out / 'stdout').write_text(result.out)
        receipt = dict(argv=list(map(str, argv)), rc=result.rc, reason=result.killed_by,
                       active_seconds=result.secs, queue_seconds=queue, guard_bytes=GUARD_BYTES,
                       peak_rss_mib=result.peak_rss_mb)
        (out / 'RESULT.json').write_text(json.dumps(receipt, indent=2) + '\n')
        if result.rc or result.killed_by:
            raise RuntimeError('guarded-command-failed: ' + label + ' (' + str(result.killed_by or result.rc) + ')')
        return result

    return run
