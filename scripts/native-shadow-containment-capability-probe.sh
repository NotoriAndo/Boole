#!/usr/bin/env bash
set -euo pipefail

# Phase 3B.1 capability probe only. This proves that the named Linux CI VM can
# provide the kernel building blocks required by the later native-shadow
# executor. It does not run a checker or declare containment GREEN.

die() {
  echo "native-shadow containment capability probe: $*" >&2
  exit 1
}

expect_value() {
  local path=$1
  local expected=$2
  local actual
  actual=$(<"$path")
  [[ "$actual" == "$expected" ]] || die "$path: expected '$expected', got '$actual'"
}

wait_for_line() {
  local path=$1
  local expected=$2
  local attempts=${3:-200}
  local i
  for ((i = 0; i < attempts; i++)); do
    if grep -qxF "$expected" "$path"; then
      return 0
    fi
    sleep 0.05
  done
  die "$path never reported '$expected'"
}

probe_dropped_identity() {
  local workspace=$1
  local ready=$2
  local release=$3
  local expected_uid=$4
  local expected_gid=$5
  local expected_leaf=$6
  local injection_mode=$7

  [[ $(id -u) == "$expected_uid" ]] || die "dropped child uid is not the probe uid"
  [[ $(id -g) == "$expected_gid" ]] || die "dropped child gid is not the probe gid"
  grep -Eq "^Uid:[[:space:]]*${expected_uid}([[:space:]]+${expected_uid}){3}[[:space:]]*$" \
    /proc/self/status || die "dropped child did not set all UID slots"
  grep -Eq "^Gid:[[:space:]]*${expected_gid}([[:space:]]+${expected_gid}){3}[[:space:]]*$" \
    /proc/self/status || die "dropped child did not set all GID slots"
  [[ $$ -eq 1 ]] || die "dropped child is not PID 1 in its private PID namespace"
  grep -Eq '^Groups:[[:space:]]*$' /proc/self/status \
    || die "dropped child retained supplementary groups"
  for field in CapInh CapPrm CapEff CapBnd CapAmb; do
    grep -Eq "^${field}:[[:space:]]*0{16}$" /proc/self/status \
      || die "dropped child retained ${field} capabilities"
  done
  grep -Eq '^NoNewPrivs:[[:space:]]*1$' /proc/self/status \
    || die "dropped child did not retain NoNewPrivs=1"

  local relative
  relative=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
  [[ "/sys/fs/cgroup${relative}" == "$expected_leaf" ]] \
    || die "dropped child escaped its assigned cgroup leaf"
  if printf '129\n' >"$expected_leaf/pids.max" 2>/dev/null; then
    die "dropped child could modify its cgroup controls"
  fi
  mkdir "$workspace/forbidden-mount"
  if mount -t tmpfs tmpfs "$workspace/forbidden-mount" 2>/dev/null; then
    die "dropped child retained mount authority"
  fi
  rmdir "$workspace/forbidden-mount"

  printf 'workspace-write-ok\n' >"$workspace/write-check"
  cat >"$workspace/exec-check" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test -f "$(dirname "$0")/write-check"
EOF
  chmod 0700 "$workspace/exec-check"
  "$workspace/exec-check"

  [[ "$injection_mode" != fail-before-ready ]] \
    || die "injected failure before readiness"
  [[ "$injection_mode" == normal ]] || die "unknown injection mode: $injection_mode"
  touch "$ready"
  local i
  for ((i = 0; i < 200; i++)); do
    [[ -e "$release" ]] && return 0
    sleep 0.05
  done
  die "privileged launcher never released the dropped namespace probe"
}

probe_namespace() {
  local workspace=$1
  local ready=$2
  local release=$3
  local script_path=$4
  local probe_uid=$5
  local probe_gid=$6
  local expected_leaf=$7
  local injection_mode=$8

  [[ $(id -u) == 0 ]] || die "privileged launcher lost root before namespace setup"

  # mount --make-rprivate is the util-linux spelling of MS_REC|MS_PRIVATE.
  mount --make-rprivate /
  mount -t proc -o nosuid,nodev,noexec proc /proc
  mount -t tmpfs \
    -o "size=536870912,nr_inodes=8192,mode=0700,nosuid,nodev,uid=$probe_uid,gid=$probe_gid" \
    tmpfs "$workspace"
  mountpoint -q "$workspace" || die "tmpfs workspace mount is not active"
  local options
  options=$(findmnt -n -o OPTIONS --target "$workspace")
  [[ ",$options," == *,nosuid,* ]] || die "tmpfs is missing nosuid"
  [[ ",$options," == *,nodev,* ]] || die "tmpfs is missing nodev"
  [[ ",$options," != *,noexec,* ]] || die "tmpfs unexpectedly blocks compiled output execution"

  exec setpriv --reuid="$probe_uid" --regid="$probe_gid" --clear-groups \
    --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
    "$script_path" dropped "$workspace" "$ready" "$release" \
    "$probe_uid" "$probe_gid" "$expected_leaf" "$injection_mode"
}

probe_namespace_bootstrap() {
  kill -STOP $$
  exec unshare --mount --pid --fork --kill-child \
    --propagation unchanged "$@"
}

probe_mount_and_pid_namespaces() (
  local script_path=$1
  local leaf=$2
  local probe_uid=$3
  local probe_gid=$4
  local requested_root=$5
  local injection_mode=$6
  local probe_root=''
  local workspace=''
  local ready=''
  local release=''
  local namespace_pid=''

  cleanup_namespace_probe() {
    set +e
    if [[ -n "$release" ]]; then
      touch "$release" 2>/dev/null
    fi
    if [[ -n "$namespace_pid" ]]; then
      if kill -0 "$namespace_pid" 2>/dev/null; then
        kill "$namespace_pid" 2>/dev/null
      fi
      wait "$namespace_pid" 2>/dev/null
    fi
    if [[ -n "$workspace" ]]; then
      rm -f "$workspace/write-check" "$workspace/exec-check"
      rmdir "$workspace" 2>/dev/null
    fi
    [[ -n "$ready" ]] && rm -f "$ready"
    [[ -n "$release" ]] && rm -f "$release"
    [[ -n "$probe_root" ]] && rmdir "$probe_root" 2>/dev/null
  }
  trap cleanup_namespace_probe EXIT

  if [[ "$requested_root" == auto ]]; then
    probe_root=$(mktemp -d /tmp/boole-native-shadow-namespace.XXXXXX)
  else
    [[ ! -e "$requested_root" ]] || die "requested namespace temp root already exists"
    mkdir "$requested_root"
    probe_root=$requested_root
  fi
  chmod 0777 "$probe_root"
  workspace="$probe_root/workspace"
  ready="$probe_root/ready"
  release="$probe_root/release"
  mkdir "$workspace"

  "$script_path" namespace-bootstrap "$script_path" namespace \
    "$workspace" "$ready" "$release" "$script_path" "$probe_uid" \
    "$probe_gid" "$leaf" "$injection_mode" &
  namespace_pid=$!

  local i state=''
  for ((i = 0; i < 200; i++)); do
    if [[ -r "/proc/$namespace_pid/stat" ]]; then
      state=$(awk '{ print $3 }' "/proc/$namespace_pid/stat")
      [[ "$state" == T ]] && break
    fi
    sleep 0.05
  done
  [[ "$state" == T ]] || die "privileged launcher bootstrap did not stop before cgroup join"
  echo "$namespace_pid" >"$leaf/cgroup.procs"
  kill -CONT "$namespace_pid"

  for ((i = 0; i < 200; i++)); do
    [[ -e "$ready" ]] && break
    if ! kill -0 "$namespace_pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if [[ ! -e "$ready" ]]; then
    die "mount/PID namespace probe failed before signaling readiness"
  fi

  if mountpoint -q "$workspace"; then
    die "private tmpfs leaked into the delegated parent's mount namespace"
  fi
  touch "$release"
  if ! wait "$namespace_pid"; then
    namespace_pid=''
    die "mount/PID namespace probe failed during teardown"
  fi
  namespace_pid=''
  [[ ! -e "$workspace/write-check" ]] \
    || die "tmpfs contents leaked into the delegated parent's mount namespace"
)

cleanup_cgroup_leaf_strict() {
  local leaf_path=$1
  [[ ! -e "$leaf_path" ]] && return 0
  [[ -d "$leaf_path" ]] || return 1
  if [[ -e "$leaf_path/cgroup.freeze" ]]; then
    echo 0 >"$leaf_path/cgroup.freeze" || return 1
  fi
  [[ -w "$leaf_path/cgroup.kill" ]] || return 1
  echo 1 >"$leaf_path/cgroup.kill" || return 1
  local i empty=0
  for ((i = 0; i < 200; i++)); do
    if grep -qxF 'populated 0' "$leaf_path/cgroup.events" 2>/dev/null; then
      empty=1
      break
    fi
    sleep 0.05
  done
  [[ $empty -eq 1 ]] || return 1
  rmdir "$leaf_path"
}

cleanup_cgroup_leaf_best_effort() {
  cleanup_cgroup_leaf_strict "$1" >/dev/null 2>&1 || :
}

probe_cleanup_failure_injection() {
  local delegated=$1
  local failure_leaf="$delegated/failure-injection"
  mkdir "$failure_leaf"
  sleep 300 &
  local failure_pid=$!
  echo "$failure_pid" >"$failure_leaf/cgroup.procs"

  set +e
  (
    trap "cleanup_cgroup_leaf_strict '$failure_leaf' || exit 74" EXIT
    exit 73
  )
  local injected_status=$?
  set -e

  local i state=''
  for ((i = 0; i < 200; i++)); do
    if ! kill -0 "$failure_pid" 2>/dev/null; then
      state=gone
      break
    fi
    if [[ -r "/proc/$failure_pid/stat" ]]; then
      state=$(awk '{ print $3 }' "/proc/$failure_pid/stat")
      [[ "$state" == Z ]] && break
    fi
    sleep 0.05
  done
  if [[ "$state" != gone && "$state" != Z ]]; then
    kill "$failure_pid" 2>/dev/null || :
    wait "$failure_pid" 2>/dev/null || :
    die "cleanup failure injection left a live child"
  fi
  wait "$failure_pid" 2>/dev/null || :
  [[ $injected_status -eq 73 ]] || die "cleanup failure injection lost its exit status"
  [[ ! -e "$failure_leaf" ]] || die "cleanup failure injection left a cgroup leaf"
}

probe_privileged_launcher() {
  local report_path=$1
  local script_path=$2
  local probe_uid=$3
  local probe_gid=$4
  local injection_mode=$5
  local probe_root=$6
  [[ ${EUID} -eq 0 ]] || die "privileged launcher probe did not start as root"
  local field
  for field in CapPrm CapEff CapBnd; do
    grep -Eq "^${field}:[[:space:]]*00000000002001c0$" /proc/self/status \
      || die "privileged launcher has unexpected ${field}"
  done
  for field in CapInh CapAmb; do
    grep -Eq "^${field}:[[:space:]]*0{16}$" /proc/self/status \
      || die "privileged launcher has unexpected ${field}"
  done
  [[ $(stat -fc %T /sys/fs/cgroup) == cgroup2fs ]] \
    || die "/sys/fs/cgroup is not a unified cgroup v2 hierarchy"

  local relative
  relative=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
  [[ -n "$relative" ]] || die "cannot resolve the delegated cgroup path"
  local delegated="/sys/fs/cgroup${relative}"
  printf '%s\n' "$delegated" >"$report_path"
  [[ -w "$delegated/cgroup.procs" ]] \
    || die "systemd Delegate=yes did not grant cgroup.procs write access"
  [[ -w "$delegated/cgroup.subtree_control" ]] \
    || die "systemd Delegate=yes did not grant subtree-control write access"

  local required
  for required in cpu memory pids; do
    grep -qw "$required" "$delegated/cgroup.controllers" \
      || die "delegated cgroup is missing the $required controller"
  done

  local manager="$delegated/manager"
  local leaf="$delegated/probe"
  mkdir "$manager"
  echo "$$" >"$manager/cgroup.procs"
  [[ -z "$(<"$delegated/cgroup.procs")" ]] \
    || die "delegated parent still has internal processes"
  echo '+cpu +memory +pids' >"$delegated/cgroup.subtree_control"
  for required in cpu memory pids; do
    grep -qw "$required" "$delegated/cgroup.subtree_control" \
      || die "failed to enable the $required controller"
  done

  mkdir "$leaf"
  local stopped_pid=''
  trap "cleanup_cgroup_leaf_best_effort '$leaf'" EXIT

  echo 128 >"$leaf/pids.max"
  echo 2147483648 >"$leaf/memory.max"
  echo 0 >"$leaf/memory.swap.max"
  echo 1 >"$leaf/memory.oom.group"
  echo 'max 100000' >"$leaf/cpu.max"
  expect_value "$leaf/pids.max" 128
  expect_value "$leaf/memory.max" 2147483648
  expect_value "$leaf/memory.swap.max" 0
  expect_value "$leaf/memory.oom.group" 1
  expect_value "$leaf/cpu.max" 'max 100000'
  [[ -e "$leaf/memory.events" && -e "$leaf/pids.events" && -e "$leaf/cpu.stat" ]] \
    || die "delegated leaf is missing required event/accounting files"
  [[ -w "$leaf/cgroup.freeze" && -w "$leaf/cgroup.kill" ]] \
    || die "kernel lacks writable cgroup.freeze or cgroup.kill"

  probe_mount_and_pid_namespaces "$script_path" "$leaf" "$probe_uid" "$probe_gid" \
    "$probe_root" "$injection_mode"

  bash -c 'kill -STOP $$; exec sleep 300' &
  stopped_pid=$!
  local i state=''
  for ((i = 0; i < 200; i++)); do
    if [[ -r "/proc/$stopped_pid/stat" ]]; then
      state=$(awk '{ print $3 }' "/proc/$stopped_pid/stat")
      [[ "$state" == T ]] && break
    fi
    sleep 0.05
  done
  [[ "$state" == T ]] || die "probe child did not stop before cgroup assignment"
  echo "$stopped_pid" >"$leaf/cgroup.procs"
  echo 1 >"$leaf/cgroup.freeze"
  wait_for_line "$leaf/cgroup.events" 'frozen 1'
  echo 1 >"$leaf/cgroup.kill"
  set +e
  wait "$stopped_pid"
  local killed_status=$?
  set -e
  stopped_pid=''
  [[ $killed_status -ne 0 ]] || die "cgroup.kill did not terminate the probe child"
  wait_for_line "$leaf/cgroup.events" 'populated 0'
  cleanup_cgroup_leaf_strict "$leaf" || die "strict cgroup leaf cleanup failed"
  trap - EXIT
  probe_cleanup_failure_injection "$delegated"
}

probe_outer() {
  [[ $(uname -s) == Linux ]] || die "this capability gate requires Linux"
  [[ ${EUID} -eq 0 ]] || die "outer probe must be invoked through passwordless sudo"
  [[ $(stat -fc %T /sys/fs/cgroup) == cgroup2fs ]] \
    || die "/sys/fs/cgroup is not cgroup2fs"
  [[ $(cat /proc/1/comm) == systemd ]] || die "PID 1 is not systemd"
  command -v systemd-run >/dev/null || die "systemd-run is unavailable"
  command -v unshare >/dev/null || die "unshare is unavailable"
  command -v setpriv >/dev/null || die "setpriv is unavailable"

  local probe_user=nobody
  id "$probe_user" >/dev/null 2>&1 || die "probe-only nobody identity is unavailable"
  local probe_uid probe_gid
  probe_uid=$(id -u "$probe_user")
  probe_gid=$(id -g "$probe_user")
  [[ "$probe_uid" -ne 0 && "$probe_gid" -ne 0 ]] \
    || die "probe-only containment identity must be unprivileged"

  local script_path launcher_path='' failure_report='' report=''
  script_path=$(readlink -f "$0")
  local suffix=${GITHUB_RUN_ID:-$$}-${GITHUB_RUN_ATTEMPT:-0}
  suffix=${suffix//[^a-zA-Z0-9-]/-}

  cleanup_outer_probe() {
    set +e
    [[ -n "$failure_report" ]] && rm -f "$failure_report"
    [[ -n "$report" ]] && rm -f "$report"
    [[ -n "$launcher_path" ]] && rm -f "$launcher_path"
  }

  # The privileged service deliberately lacks CAP_DAC_OVERRIDE. GitHub's
  # runner-home traversal permissions therefore cannot be part of the launch
  # contract. Stage the exact reviewed bytes in root-owned /run before
  # capability bounding, and make that copy the recursive launcher path.
  launcher_path=$(mktemp /run/boole-native-shadow-launcher.XXXXXX)
  trap cleanup_outer_probe EXIT
  install -o root -g root -m 0555 "$script_path" "$launcher_path"
  [[ "$(sha256sum "$launcher_path" | awk '{ print $1 }')" == "$(sha256sum "$script_path" | awk '{ print $1 }')" ]] \
    || die "staged privileged launcher bytes do not match the reviewed probe"

  local failure_root failure_unit failure_status failure_delegated
  failure_report=$(mktemp /tmp/boole-native-shadow-failure-cgroup.XXXXXX)
  failure_root="/tmp/boole-native-shadow-expected-failure-${suffix}"
  [[ ! -e "$failure_root" ]] || die "expected-failure namespace temp root already exists"
  failure_unit="boole-native-shadow-expected-failure-${suffix}"

  set +e
  systemd-run --quiet --wait --collect --unit="$failure_unit" \
    --property=Type=exec --property=Delegate=yes \
    --property=User=root --property=Group=root \
    --property='CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN' \
    "$launcher_path" privileged-launcher "$failure_report" "$launcher_path" \
    "$probe_uid" "$probe_gid" fail-before-ready "$failure_root"
  failure_status=$?
  set -e
  journalctl --no-pager -o cat -u "$failure_unit" || :
  [[ $failure_status -ne 0 ]] \
    || die "expected-failure transient service unexpectedly succeeded"
  failure_delegated=$(<"$failure_report")
  [[ -n "$failure_delegated" ]] \
    || die "expected-failure service did not report its cgroup path"
  local i
  for ((i = 0; i < 200; i++)); do
    [[ ! -e "$failure_delegated" ]] && break
    sleep 0.05
  done
  [[ ! -e "$failure_delegated" ]] \
    || die "expected-failure transient cgroup was not removed"
  [[ ! -e "$failure_root" ]] \
    || die "expected-failure namespace temp tree was not removed"
  rm -f "$failure_report"
  failure_report=''

  report=$(mktemp /tmp/boole-native-shadow-cgroup.XXXXXX)
  local unit="boole-native-shadow-privileged-launcher-${suffix}"

  set +e
  systemd-run --quiet --wait --collect --unit="$unit" \
    --property=Type=exec --property=Delegate=yes \
    --property=User=root --property=Group=root \
    --property='CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN' \
    "$launcher_path" privileged-launcher "$report" "$launcher_path" \
    "$probe_uid" "$probe_gid" normal auto
  local service_status=$?
  set -e
  journalctl --no-pager -o cat -u "$unit" || :
  [[ $service_status -eq 0 ]] || die "privileged launcher transient service failed"

  local delegated
  delegated=$(<"$report")
  [[ -n "$delegated" ]] || die "delegated service did not report its cgroup path"
  for ((i = 0; i < 200; i++)); do
    [[ ! -e "$delegated" ]] && break
    sleep 0.05
  done
  [[ ! -e "$delegated" ]] || die "transient service cgroup was not removed"
  rm -f "$report"
  report=''
  rm -f "$launcher_path"
  launcher_path=''
  trap - EXIT
  echo "native-shadow containment capability probe: PASS"
}

case ${1:-outer} in
  outer)
    probe_outer
    ;;
  privileged-launcher)
    [[ $# -eq 7 ]] \
      || die "privileged-launcher mode requires report, script, uid, gid, mode and temp root"
    probe_privileged_launcher "$2" "$3" "$4" "$5" "$6" "$7"
    ;;
  namespace-bootstrap)
    [[ $# -ge 2 ]] || die "namespace-bootstrap mode requires a command"
    shift
    probe_namespace_bootstrap "$@"
    ;;
  namespace)
    [[ $# -eq 9 ]] \
      || die "namespace mode requires workspace, ready, release, script, uid, gid, leaf and mode"
    probe_namespace "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9"
    ;;
  dropped)
    [[ $# -eq 8 ]] \
      || die "dropped mode requires workspace, ready, release, uid, gid, leaf and mode"
    probe_dropped_identity "$2" "$3" "$4" "$5" "$6" "$7" "$8"
    ;;
  *)
    die "unknown probe mode: $1"
    ;;
esac
