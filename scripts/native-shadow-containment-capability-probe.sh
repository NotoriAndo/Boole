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

  [[ $(id -u) == 1 ]] || die "dropped child uid is not namespace uid 1"
  [[ $(id -g) == 1 ]] || die "dropped child gid is not namespace gid 1"
  grep -Eq '^Groups:[[:space:]]*$' /proc/self/status \
    || die "dropped child retained supplementary groups"
  for field in CapInh CapPrm CapEff CapBnd CapAmb; do
    grep -Eq "^${field}:[[:space:]]*0{16}$" /proc/self/status \
      || die "dropped child retained ${field} capabilities"
  done
  grep -Eq '^NoNewPrivs:[[:space:]]*1$' /proc/self/status \
    || die "dropped child did not retain NoNewPrivs=1"

  printf 'workspace-write-ok\n' >"$workspace/write-check"
  cat >"$workspace/exec-check" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test -f "$(dirname "$0")/write-check"
EOF
  chmod 0700 "$workspace/exec-check"
  "$workspace/exec-check"
}

probe_namespace() {
  local workspace=$1
  local ready=$2
  local release=$3
  local script_path=$4

  [[ $(id -u) == 0 ]] || die "user namespace did not map the caller to root"

  # mount --make-rprivate is the util-linux spelling of MS_REC|MS_PRIVATE.
  mount --make-rprivate /
  mount -t tmpfs \
    -o size=536870912,nr_inodes=8192,mode=0700,nosuid,nodev,uid=1,gid=1 \
    tmpfs "$workspace"
  mountpoint -q "$workspace" || die "tmpfs workspace mount is not active"
  local options
  options=$(findmnt -n -o OPTIONS --target "$workspace")
  [[ ",$options," == *,nosuid,* ]] || die "tmpfs is missing nosuid"
  [[ ",$options," == *,nodev,* ]] || die "tmpfs is missing nodev"
  [[ ",$options," != *,noexec,* ]] || die "tmpfs unexpectedly blocks compiled output execution"

  setpriv --reuid=1 --regid=1 --clear-groups \
    --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
    "$script_path" dropped "$workspace"

  touch "$ready"
  local i
  for ((i = 0; i < 200; i++)); do
    [[ -e "$release" ]] && break
    sleep 0.05
  done
  [[ -e "$release" ]] || die "parent never released namespace probe"
  umount "$workspace"
}

probe_user_and_mount_namespaces() (
  local script_path=$1
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

  probe_root=$(mktemp -d /tmp/boole-native-shadow-namespace.XXXXXX)
  chmod 0777 "$probe_root"
  workspace="$probe_root/workspace"
  ready="$probe_root/ready"
  release="$probe_root/release"
  mkdir "$workspace"

  unshare --user --map-auto --map-root-user --mount --pid --fork --kill-child \
    --mount-proc "$script_path" namespace "$workspace" "$ready" "$release" \
    "$script_path" &
  namespace_pid=$!

  local i
  for ((i = 0; i < 200; i++)); do
    [[ -e "$ready" ]] && break
    if ! kill -0 "$namespace_pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if [[ ! -e "$ready" ]]; then
    die "user/mount namespace probe failed before signaling readiness"
  fi

  if mountpoint -q "$workspace"; then
    die "private tmpfs leaked into the delegated parent's mount namespace"
  fi
  touch "$release"
  if ! wait "$namespace_pid"; then
    namespace_pid=''
    die "user/mount namespace probe failed during teardown"
  fi
  namespace_pid=''
  [[ ! -e "$workspace/write-check" ]] \
    || die "tmpfs contents leaked into the delegated parent's mount namespace"
)

probe_delegated_cgroup() {
  local report_path=$1
  local script_path=$2
  [[ ${EUID} -ne 0 ]] || die "delegated probe must run as the unprivileged service user"
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
  cleanup_leaf() {
    set +e
    if [[ -d "$leaf" ]]; then
      [[ -e "$leaf/cgroup.freeze" ]] && echo 0 >"$leaf/cgroup.freeze"
      [[ -e "$leaf/cgroup.kill" ]] && echo 1 >"$leaf/cgroup.kill"
      if [[ -n "$stopped_pid" ]]; then
        wait "$stopped_pid" 2>/dev/null
      fi
      rmdir "$leaf" 2>/dev/null
    fi
  }
  trap cleanup_leaf EXIT

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

  probe_user_and_mount_namespaces "$script_path"

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
  echo 0 >"$leaf/cgroup.freeze"
  rmdir "$leaf"
  trap - EXIT
}

probe_outer() {
  [[ $(uname -s) == Linux ]] || die "this capability gate requires Linux"
  [[ ${EUID} -eq 0 ]] || die "outer probe must be invoked through passwordless sudo"
  [[ $(stat -fc %T /sys/fs/cgroup) == cgroup2fs ]] \
    || die "/sys/fs/cgroup is not cgroup2fs"
  [[ $(cat /proc/1/comm) == systemd ]] || die "PID 1 is not systemd"
  command -v systemd-run >/dev/null || die "systemd-run is unavailable"
  command -v unshare >/dev/null || die "unshare is unavailable"
  command -v newuidmap >/dev/null || die "newuidmap is unavailable"
  command -v newgidmap >/dev/null || die "newgidmap is unavailable"
  command -v setpriv >/dev/null || die "setpriv is unavailable"

  local probe_user=${SUDO_USER:-}
  [[ -n "$probe_user" && "$probe_user" != root ]] \
    || die "sudo did not identify an unprivileged probe user"
  local probe_group
  probe_group=$(id -gn "$probe_user")
  grep -qE "^${probe_user}:" /etc/subuid \
    || die "$probe_user has no pre-provisioned subordinate UID range"
  grep -qE "^${probe_user}:" /etc/subgid \
    || die "$probe_user has no pre-provisioned subordinate GID range"

  local script_path
  script_path=$(readlink -f "$0")
  local report
  report=$(mktemp /tmp/boole-native-shadow-cgroup.XXXXXX)
  chown "$probe_user:$probe_group" "$report"
  local suffix=${GITHUB_RUN_ID:-$$}-${GITHUB_RUN_ATTEMPT:-0}
  suffix=${suffix//[^a-zA-Z0-9-]/-}
  local unit="boole-native-shadow-probe-${suffix}"

  set +e
  systemd-run --quiet --wait --collect --unit="$unit" \
    --property=Type=exec --property=Delegate=yes \
    --property="User=$probe_user" --property="Group=$probe_group" \
    "$script_path" delegated "$report" "$script_path"
  local service_status=$?
  set -e
  journalctl --no-pager -o cat -u "$unit" || :
  [[ $service_status -eq 0 ]] || die "delegated transient service failed"

  local delegated
  delegated=$(<"$report")
  [[ -n "$delegated" ]] || die "delegated service did not report its cgroup path"
  local i
  for ((i = 0; i < 200; i++)); do
    [[ ! -e "$delegated" ]] && break
    sleep 0.05
  done
  [[ ! -e "$delegated" ]] || die "transient service cgroup was not removed"
  rm -f "$report"
  echo "native-shadow containment capability probe: PASS"
}

case ${1:-outer} in
  outer)
    probe_outer
    ;;
  delegated)
    [[ $# -eq 3 ]] || die "delegated mode requires report path and script path"
    probe_delegated_cgroup "$2" "$3"
    ;;
  namespace)
    [[ $# -eq 5 ]] || die "namespace mode requires workspace, ready, release and script path"
    probe_namespace "$2" "$3" "$4" "$5"
    ;;
  dropped)
    [[ $# -eq 2 ]] || die "dropped mode requires workspace"
    probe_dropped_identity "$2"
    ;;
  *)
    die "unknown probe mode: $1"
    ;;
esac
