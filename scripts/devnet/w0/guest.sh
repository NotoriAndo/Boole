#!/bin/bash
# Prepared only. Execute inside the separately granted, offline-prepared clone.
set -euo pipefail
umask 077
w0_source=$(realpath "${1:?reviewed export}")
w0_run=$(realpath "${2:?private run directory}")
w0_commit=${3:?expected commit}
w0_manifest=${4:?externally pinned manifest}
w0_mount=${5:?authority guest mount}
w0_stage=${6:-outer}
w0_deadline=${7:-0}
w0_budget="$w0_source/scripts/devnet/w0/budget.py"
w0_bound() { python3 -IB "$w0_budget" "$w0_deadline" "$1" "$2"; }
w0_home=${HOME:?explicit cached guest home}
[[ $(uname -s) == Linux && $(uname -m) == aarch64 && $(id -u) != 0 ]]
[[ $w0_source == "$w0_run"/source && $w0_run == "$w0_mount"/run-* ]]
[[ $w0_commit =~ ^[0-9a-f]{40}$ && $w0_manifest =~ ^[0-9a-f]{64}$ ]]
w0_runner="$w0_source/scripts/devnet/w0/runner.py"
w0_grant="$w0_run/host-grant.json"
if [[ $w0_stage == outer ]]; then
  w0_deadline=$(python3 -IB -c 'import time;print(time.monotonic()+1960)')
  exec /usr/bin/timeout --signal=TERM --kill-after=15s 1965s /usr/bin/env -i PATH=/usr/bin:/bin HOME="$w0_home" LC_ALL=C PYTHONDONTWRITEBYTECODE=1 \
    /usr/bin/nice -n 19 /bin/bash "$0" "$w0_source" "$w0_run" "$w0_commit" "$w0_manifest" "$w0_mount" scope "$w0_deadline"
fi
if [[ $w0_stage == scope ]]; then
python3 -IB - "$w0_source" "$w0_commit" "$w0_run" "$w0_manifest" <<'PY_CHECK'
import hashlib,json,os,stat,sys
from pathlib import Path
source,expected,run,pin=Path(sys.argv[1]),sys.argv[2],Path(sys.argv[3]),sys.argv[4]
raw=(source/'SOURCE.json').read_bytes();assert hashlib.sha256(raw).hexdigest()==pin
manifest=json.loads(raw);assert manifest['version']==1 and manifest['commit']==expected
actual=set()
for parent,dirs,files in os.walk(source,followlinks=False):
    for n in dirs:assert not (Path(parent)/n).is_symlink()
    for n in files:actual.add(str((Path(parent)/n).relative_to(source)))
assert actual==set(manifest['files'])|{'SOURCE.json'}
for name,pin in manifest['files'].items():
    p=source/name
    assert p.is_relative_to(source) and '..' not in Path(name).parts and not p.is_symlink()
    assert hashlib.sha256(p.read_bytes()).hexdigest()==pin
assert stat.S_IMODE(run.stat().st_mode)==0o700 and run.stat().st_uid==os.geteuid()
probe=run/'fs-preflight';probe.mkdir(mode=0o700)
with (probe/'a').open('xb') as f:f.write(b'W0-fsync');f.flush();os.fsync(f.fileno())
os.link(probe/'a',probe/'b');assert (probe/'a').stat().st_nlink==2
(probe/'b').unlink();(probe/'a').chmod(0o400)
try:fd=os.open(probe/'a',os.O_WRONLY)
except PermissionError:pass
else:os.close(fd);raise RuntimeError('read-only preflight failed')
with (probe/'a').open('rb') as f:assert f.read()==b'W0-fsync'
os.mkfifo(probe/'chunk.fifo',0o600)
try:assert stat.S_ISFIFO((probe/'chunk.fifo').lstat().st_mode)
finally:(probe/'chunk.fifo').unlink()
d=os.open(probe,os.O_RDONLY|os.O_DIRECTORY);os.fsync(d);os.close(d)
assert source.stat().st_dev==run.stat().st_dev
assert not os.statvfs(run).f_flag & getattr(os,'ST_NOEXEC',8)
PY_CHECK
  python3 -IB "$w0_runner" check-boot-v3 "$w0_grant"
  w0_step=$(python3 -IB -c 'import json,sys;print(json.load(open(sys.argv[1]))["step"])' "$w0_grant")
  if [[ $w0_step == W0-MASK ]]; then
    # Persist masks/packages and skew before a watcher can terminate this shell.
    timeout --signal=TERM --kill-after=5s "$(w0_bound 180 60)" python3 -IB "$w0_source/scripts/devnet/w0/preflight.py" mask-inventory "$w0_run" -
    timeout --signal=TERM --kill-after=5s "$(w0_bound 15 60)" python3 -IB "$w0_source/scripts/devnet/w0/preflight.py" mask-skew "$w0_run" -
    python3 -IB "$w0_runner" watch-live-v3 "$w0_run" "$$" <&0 &
    w0_watch=$!
    exec </dev/null
    trap 'exit 124' TERM INT HUP
    for ((w0_n=0;w0_n<100;w0_n++)); do [[ -f "$w0_run/live-watch.ready" ]] && break; sleep .05; done
    [[ -f "$w0_run/live-watch.ready" ]]
    python3 -IB "$w0_runner" check-grant-v1 "$w0_grant"
    # Read-only observations run after the first ACK, without extending its deadline.
    timeout --signal=TERM --kill-after=5s "$(w0_bound 90 60)" python3 -IB "$w0_source/scripts/devnet/w0/preflight.py" mask-probes "$w0_run" -
    # Diagnostic only: no cache method or diagnostic exit status decides a window.
    timeout --signal=TERM --kill-after=1s 4s python3 -IB "$w0_runner" diagnose-share-v3 "$w0_run" &
    w0_diag=$!
    if wait "$w0_diag"; then printf '0\n' > "$w0_run/share-diagnostic.exit"; else printf '%s\n' "$?" > "$w0_run/share-diagnostic.exit"; fi
    sleep 2
    python3 -IB "$w0_runner" request-close-v3 "$w0_run"
    for ((w0_n=0;w0_n<120;w0_n++)); do [[ -f "$w0_run/live-watch.closed" ]] && break; sleep .05; done
    [[ -f "$w0_run/live-watch.closed" ]];wait "$w0_watch"
    trap - EXIT TERM INT HUP
    exit 0
  fi
  findmnt --mountpoint "$w0_mount" -o TARGET,SOURCE,FSTYPE,OPTIONS > "$w0_run/mount.txt"
  w0_scope=$(python3 -IB - "$w0_grant" <<'PY_SCOPE'
import json,sys
g=json.load(open(sys.argv[1]));print('boole-w0-'+g['request_id']+'-'+str(g['generation']))
PY_SCOPE
)
  exec /usr/bin/env -i PATH=/usr/bin:/bin HOME="$w0_home" XDG_RUNTIME_DIR="/run/user/$(id -u)" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus" \
    systemd-run --user --scope --unit "$w0_scope" --quiet -p Delegate=yes -p MemoryMax=3584M -p TasksMax=512 \
    /usr/bin/env -i PATH=/usr/bin:/bin HOME="$w0_home" LC_ALL=C PYTHONDONTWRITEBYTECODE=1 /usr/bin/nice -n19 /bin/bash "$0" "$w0_source" "$w0_run" "$w0_commit" "$w0_manifest" "$w0_mount" manager "$w0_deadline"
fi
[[ $w0_stage == manager ]]
python3 -IB "$w0_runner" check-boot-v3 "$w0_grant"
w0_cgroup_rel=$(awk -F: '$1=="0"{print $3}' /proc/self/cgroup)
w0_cgroup="/sys/fs/cgroup$w0_cgroup_rel"
w0_scope=$(python3 -IB - "$w0_grant" <<'PY_SCOPE_VERIFY'
import json,sys
g=json.load(open(sys.argv[1]));print('boole-w0-'+g['request_id']+'-'+str(g['generation'])+'.scope')
PY_SCOPE_VERIFY
)
[[ $w0_cgroup == /sys/fs/cgroup/*/"$w0_scope" ]]
mkdir "$w0_cgroup/manager"
echo "$$" > "$w0_cgroup/manager/cgroup.procs"
echo '+memory +pids' > "$w0_cgroup/cgroup.subtree_control"
w0_packages=0
w0_cleanup() {
  local w0_status=$?
  trap - EXIT
  trap '' INT TERM HUP
  # pidfds pin the manager tasks; skip only this shell and the cleanup helper.
  python3 -IB - "$w0_cgroup/manager" "$$" <<'PY_MANAGER'
import os,signal,sys
from pathlib import Path
group=Path(sys.argv[1]);shell=int(sys.argv[2])
for text in (group/'cgroup.procs').read_text().split():
    pid=int(text)
    if pid in (shell,os.getpid()):continue
    try:
        fd=os.pidfd_open(pid)
        try:
            expected=str(group).removeprefix('/sys/fs/cgroup')
            if any(line=='0::'+expected for line in Path(f'/proc/{pid}/cgroup').read_text().splitlines()):signal.pidfd_send_signal(fd,signal.SIGKILL)
        finally:os.close(fd)
    except ProcessLookupError:pass
PY_MANAGER
  if [[ -n ${w0_job:-} ]]; then wait "$w0_job" 2>/dev/null || true; fi
  for w0_group in "$w0_cgroup"/w0-*; do
    [[ -d $w0_group && ! -L $w0_group ]] || continue
    echo 1 > "$w0_group/cgroup.kill" || w0_status=2
    for ((w0_n=0; w0_n<200; w0_n++)); do
      grep -q '^populated 0$' "$w0_group/cgroup.events" && break
      sleep 0.01
    done
    grep -q '^populated 0$' "$w0_group/cgroup.events" || w0_status=2
    rmdir "$w0_group" || w0_status=2
  done
  if [[ $w0_packages == 1 ]]; then
    python3 -IB "$w0_source/scripts/devnet/w0/preflight.py" after "$w0_run" "$w0_cgroup" || w0_status=2
  fi
  printf '{"scope_payloads_empty":%s,"guest_status":%d}\n' "$([[ $w0_status == 0 ]] && echo true || echo false)" "$w0_status" > "$w0_run/guest-release.json"
  exit "$w0_status"
}
trap w0_cleanup EXIT
trap 'exit 124' INT TERM HUP
python3 -IB "$w0_runner" watch-live-v3 "$w0_run" "$$" <&0 &
w0_watch=$!
exec </dev/null
for ((w0_n=0;w0_n<100;w0_n++)); do [[ -f "$w0_run/live-watch.ready" ]] && break; sleep .05; done
[[ -f "$w0_run/live-watch.ready" ]]
python3 -IB "$w0_runner" check-grant-v1 "$w0_grant"
mkdir -p "$w0_run/tmp" "$w0_run/home" "$w0_run/target" "$w0_run/receipts"
# Mask/check update services, exact tool pins, namespaces prerequisite, cgroups,
# FIFO and the real bwrap CLI before copying toolchains or building anything.
python3 -IB "$w0_source/scripts/devnet/w0/preflight.py" before "$w0_run" "$w0_cgroup"
w0_packages=1
# All archives and the exact lockfile closure were staged before boot. Recheck
# every input before extracting anything; use only this run's prefix and vendor.
[[ -f $(cc -print-file-name=libc.a) ]] || { echo 'NEEDED-OFFLINE: static glibc' >&2; exit 2; }
command -v readelf >/dev/null
timeout --signal=TERM --kill-after=5s "$(w0_bound 600 300)" python3 -IB "$w0_source/scripts/devnet/w0/inputs.py" install-v1 "$w0_source" "$w0_run" &
w0_job=$!;wait "$w0_job"
w0_bin="$w0_run/toolchain/bin"
w0_env=(/usr/bin/env -i PATH="$w0_bin:/usr/bin:/bin" HOME="$w0_run/home" LC_ALL=C RUSTUP_AUTO_INSTALL=0 CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1
  CARGO_HOME="$w0_run/cargo-home" RUSTUP_HOME="$w0_run/rustup-disabled" CARGO_TARGET_DIR="$w0_run/target" TMPDIR="$w0_run/tmp" RUSTC="$w0_bin/rustc" RUSTDOC="$w0_bin/rustdoc"
  PYTHONDONTWRITEBYTECODE=1 'RUSTFLAGS=-C target-feature=+crt-static -C codegen-units=1')
cd "$w0_source"
"${w0_env[@]}" timeout --signal=TERM --kill-after=5s "$(w0_bound 600 300)" nice -n19 "$w0_bin/cargo" build --locked --offline --features w0-fixture --target aarch64-unknown-linux-gnu -p boole-gate-supervisor --bin boole-w0-probe --test b2_w0 &
w0_job=$!;wait "$w0_job"
w0_probe="$w0_run/target/aarch64-unknown-linux-gnu/debug/boole-w0-probe"
readelf -l "$w0_probe" > "$w0_run/probe-elf.txt"
if grep -q INTERP "$w0_run/probe-elf.txt"; then echo 'static fixture required' >&2; exit 2; fi
python3 -IB "$w0_runner" prepare-v1 "$w0_source" "$w0_probe" "$w0_run/bundle" "$w0_grant" "$w0_cgroup"
"${w0_env[@]}" BOOLE_B2_W0_GRANT="$w0_run/bundle/grant.json" BOOLE_B2_W0_RUNNER="$w0_run/bundle/runner.py" BOOLE_B2_W0_RECEIPTS="$w0_run/receipts" \
  timeout --signal=TERM --kill-after=5s "$(w0_bound 1200 90)" nice -n19 "$w0_bin/cargo" test --locked --offline --features w0-fixture --target aarch64-unknown-linux-gnu -p boole-gate-supervisor --test b2_w0 -- --ignored --test-threads=1 --nocapture > "$w0_run/receipts/w0-tests.log" 2>&1 &
w0_job=$!;wait "$w0_job"

python3 -IB "$w0_runner" request-close-v3 "$w0_run"
for ((w0_n=0;w0_n<120;w0_n++)); do [[ -f "$w0_run/live-watch.closed" ]] && break; sleep .05; done
[[ -f "$w0_run/live-watch.closed" ]];wait "$w0_watch"
