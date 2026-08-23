#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "native-shadow manager cgroup gate: $*" >&2
  exit 1
}

[[ $(uname -s) == Linux ]] || die "this gate requires Linux"
[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"
for command_name in awk cargo find getent grep install journalctl paste python3 readlink sed \
  sha256sum sort stat systemctl systemd-tmpfiles tee tr; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done
[[ $(cat /proc/1/comm) == systemd ]] || die "PID 1 is not systemd"
[[ $(stat -fc %T /sys/fs/cgroup) == cgroup2fs ]] || die "/sys/fs/cgroup is not cgroup2fs"

unit_name=boole-native-shadow-launcher.service
unit_path=/run/systemd/system/$unit_name
launcher_directory=/usr/libexec/boole
launcher_path=$launcher_directory/boole-native-shadow-launcher
authority_parent=/usr/share/boole
authority_directory=$authority_parent/native-shadow
runtime_parent=/run/boole
runtime_directory=$runtime_parent/native-shadow
mode_path=$runtime_directory/manager-cgroup-gate-mode
service_root=/sys/fs/cgroup/system.slice/$unit_name
manager_root=$service_root/manager
temp_root=${RUNNER_TEMP:-/tmp}
build_json=$(mktemp "$temp_root/boole-native-shadow-manager-build.XXXXXX")
log=$(mktemp "$temp_root/boole-native-shadow-manager.XXXXXX")

launcher_directory_created=false
authority_parent_created=false
authority_directory_created=false
runtime_parent_created=false
runtime_directory_created=false
unit_installed=false
launcher_installed=false
declare -a installed_authorities=()

cleanup_gate() {
  set +e
  if [[ "$unit_installed" == true ]]; then
    sudo systemctl stop "$unit_name" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "$unit_name" >/dev/null 2>&1 || :
    sudo rm -f "$unit_path"
    sudo systemctl daemon-reload >/dev/null 2>&1 || :
  fi
  [[ "$launcher_installed" == true ]] && sudo rm -f "$launcher_path"
  local basename
  for basename in "${installed_authorities[@]}"; do
    sudo rm -f "$authority_directory/$basename"
  done
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$mode_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$runtime_directory/launcher.lock"
  [[ "$runtime_directory_created" == true ]] && sudo rmdir "$runtime_directory" >/dev/null 2>&1 || :
  [[ "$runtime_parent_created" == true ]] && sudo rmdir "$runtime_parent" >/dev/null 2>&1 || :
  [[ "$authority_directory_created" == true ]] && sudo rmdir "$authority_directory" >/dev/null 2>&1 || :
  [[ "$authority_parent_created" == true ]] && sudo rmdir "$authority_parent" >/dev/null 2>&1 || :
  [[ "$launcher_directory_created" == true ]] && sudo rmdir "$launcher_directory" >/dev/null 2>&1 || :
  rm -f "$build_json" "$log"
}
trap cleanup_gate EXIT

for identity in boole-node boole-native-checker; do
  getent passwd "$identity" >/dev/null || die "missing service user: $identity"
  getent group "$identity" >/dev/null || die "missing service group: $identity"
done

load_state=$(systemctl show "$unit_name" --property=LoadState --value 2>/dev/null || :)
[[ "$load_state" == not-found ]] \
  || die "refusing to shadow pre-existing loaded unit: $unit_name ($load_state)"

for path in "$unit_path" "$launcher_path" "$runtime_directory" "$service_root" \
  "$authority_directory/registry-v1.json" \
  "$authority_directory/execution-policy-v1.json" \
  "$authority_directory/toolchain-identity-v1.json"; do
  [[ ! -e "$path" && ! -L "$path" ]] || die "refusing to replace pre-existing path: $path"
done

cargo test --locked -p boole-native-shadow-launcher \
  --features manager-cgroup-linux-gate \
  --test boole-native-shadow-manager-cgroup-linux \
  --no-run --message-format=json >"$build_json"

mapfile -t executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole-native-shadow-manager-cgroup-linux"
        and item.get("executable")
    ):
        print(item["executable"])
' "$build_json"
)
[[ ${#executables[@]} -eq 1 ]] || die "expected one manager harness, got ${#executables[@]}"
harness=${executables[0]}
[[ -x "$harness" ]] || die "manager harness is not executable"

if [[ ! -d "$launcher_directory" ]]; then
  sudo install -d -o root -g root -m 0755 "$launcher_directory"
  launcher_directory_created=true
fi
sudo install -o root -g root -m 0755 "$harness" "$launcher_path"
launcher_installed=true
[[ $(sha256sum "$harness" | awk '{ print $1 }') == $(sudo sha256sum "$launcher_path" | awk '{ print $1 }') ]] \
  || die "installed launcher bytes differ from reviewed harness"
[[ $(sudo stat -c %U:%G:%a "$launcher_path") == root:root:755 ]] \
  || die "installed launcher metadata does not match root:root:755"

if [[ ! -d "$authority_parent" ]]; then
  sudo install -d -o root -g root -m 0755 "$authority_parent"
  authority_parent_created=true
fi
if [[ ! -d "$authority_directory" ]]; then
  sudo install -d -o root -g root -m 0555 "$authority_directory"
  authority_directory_created=true
fi

install_authority() {
  local source=$1
  local basename=$2
  sudo install -o root -g root -m 0444 "$source" "$authority_directory/$basename"
  installed_authorities+=("$basename")
  [[ $(sha256sum "$source" | awk '{ print $1 }') == $(sudo sha256sum "$authority_directory/$basename" | awk '{ print $1 }') ]] \
    || die "installed authority differs: $basename"
}
install_authority fixtures/native-shadow/registry-v1.json registry-v1.json
install_authority native/containment/native-shadow-execution-policy-v1.json execution-policy-v1.json
install_authority native/containment/native-shadow-toolchain-identity-v1.json toolchain-identity-v1.json

if [[ ! -d "$runtime_parent" ]]; then
  runtime_parent_created=true
fi
if [[ ! -d "$runtime_directory" ]]; then
  runtime_directory_created=true
fi
tmpfiles_path=$(readlink -f native/tmpfiles.d/boole-native-shadow.conf)
[[ -f "$tmpfiles_path" ]] || die "tracked tmpfiles input is unavailable"
sudo systemd-tmpfiles --create "$tmpfiles_path"
[[ $(stat -c %U:%G:%a "$runtime_directory") == root:boole-node:2750 ]] \
  || die "runtime directory does not match root:boole-node mode 2750"

sudo install -o root -g root -m 0644 native/systemd/boole-native-shadow-launcher.service "$unit_path"
unit_installed=true
[[ $(sha256sum native/systemd/boole-native-shadow-launcher.service | awk '{ print $1 }') == $(sudo sha256sum "$unit_path" | awk '{ print $1 }') ]] \
  || die "installed systemd unit differs from tracked bytes"
sudo systemctl daemon-reload

set_mode() {
  local mode=$1
  if [[ "$mode" == normal ]]; then
    sudo rm -f "$mode_path"
  else
    printf '%s\n' "$mode" | sudo tee "$mode_path" >/dev/null
    sudo chown root:root "$mode_path"
    sudo chmod 0600 "$mode_path"
  fi
}

wait_for_state() {
  local expected=$1
  local state=''
  local i
  for ((i = 0; i < 200; i++)); do
    state=$(sudo systemctl show "$unit_name" --property=ActiveState --value 2>/dev/null || :)
    [[ "$state" == "$expected" ]] && return 0
    if [[ "$state" == failed ]]; then
      sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
      die "unit entered failed state while waiting for $expected"
    fi
    sleep 0.05
  done
  die "unit did not reach $expected (last state: $state)"
}

wait_for_cgroup_removal() {
  local i
  for ((i = 0; i < 200; i++)); do
    [[ ! -e "$service_root" ]] && return 0
    sleep 0.05
  done
  die "service cgroup remained after stop"
}

single_numeric_id() {
  local path=$1
  local -a values=()
  mapfile -t values < <(awk 'NF == 1 { print $1; next } NF > 1 { print "__malformed__" }' "$path")
  [[ ${#values[@]} -eq 1 && ${values[0]} =~ ^[1-9][0-9]*$ ]] || return 1
  printf '%s\n' "${values[0]}"
}

assert_manager_invariants() {
  wait_for_state active
  local pid
  pid=$(sudo systemctl show "$unit_name" --property=MainPID --value)
  [[ "$pid" =~ ^[1-9][0-9]*$ ]] || die "unit has invalid MainPID: $pid"

  local i
  local manager_pid
  for ((i = 0; i < 200; i++)); do
    [[ -d "$manager_root" ]] || { sleep 0.05; continue; }
    [[ -z $(tr -d '[:space:]' <"$service_root/cgroup.procs") ]] || { sleep 0.05; continue; }
    manager_pid=$(single_numeric_id "$manager_root/cgroup.procs" || :)
    [[ "$manager_pid" == "$pid" ]] || { sleep 0.05; continue; }
    break
  done
  [[ $i -lt 200 ]] || die "manager cgroup did not reach the post-move state"

  local manager_tid
  manager_pid=$(single_numeric_id "$manager_root/cgroup.procs") \
    || die "manager cgroup.procs does not contain exactly one numeric ID"
  manager_tid=$(single_numeric_id "$manager_root/cgroup.threads") \
    || die "manager cgroup.threads does not contain exactly one numeric ID"
  [[ "$manager_pid" == "$pid" ]] \
    || die "manager cgroup does not contain exactly the MainPID process"
  [[ "$manager_tid" == "$pid" ]] \
    || die "manager cgroup does not contain exactly the MainPID thread"
  [[ $(stat -c %U:%G:%a "$manager_root") == root:root:700 ]] \
    || die "manager cgroup metadata does not match root:root:700"
  [[ -z $(tr -d '[:space:]' <"$manager_root/cgroup.subtree_control") ]] \
    || die "manager cgroup has residual subtree controllers"
  [[ $(tr -d '[:space:]' <"$manager_root/cgroup.type") == domain ]] \
    || die "manager cgroup type is not exact domain after move"
  [[ -z $(find "$manager_root" -mindepth 1 -maxdepth 1 -type d -print -quit) ]] \
    || die "manager cgroup contains a nested child"
  local controllers
  controllers=$(tr ' ' '\n' <"$service_root/cgroup.subtree_control" | sed '/^$/d' | sort | paste -sd' ' -)
  [[ "$controllers" == "cpu memory pids" ]] \
    || die "service subtree controllers differ: $controllers"
  [[ $(stat -fc %T "$service_root") == cgroup2fs ]] || die "service root is not cgroup2fs"
  [[ $(readlink -f "/proc/$pid/exe") == "$launcher_path" ]] \
    || die "MainPID does not execute the staged reviewed harness"
  local relative
  relative=$(awk -F: '$1 == "0" { print $3 }' "/proc/$pid/cgroup")
  [[ "$relative" == "/system.slice/$unit_name/manager" ]] \
    || die "MainPID cgroup path differs: $relative"
  printf '%s\n' "$pid"
}

unit_invocation_id() {
  local invocation_id
  invocation_id=$(sudo systemctl show "$unit_name" --property=InvocationID --value)
  [[ "$invocation_id" =~ ^[0-9a-f]{32}$ ]] \
    || die "unit has invalid InvocationID: $invocation_id"
  printf '%s\n' "$invocation_id"
}

run_expected_rejection() {
  local mode=$1
  local marker=$2
  set_mode "$mode"
  sudo systemctl start boole-native-shadow-launcher.service
  wait_for_state inactive
  [[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
    || die "$mode rejection harness did not exit successfully"
  sudo journalctl --no-pager -o cat -u "$unit_name" >"$log"
  [[ $(grep -Fxc "$marker" "$log" || :) -eq 1 ]] \
    || die "$mode rejection marker was not observed exactly once"
  wait_for_cgroup_removal
  sudo systemctl reset-failed "$unit_name"
}

run_expected_rejection nested-reject native-shadow-manager-nested-rejected
run_expected_rejection frozen-reject native-shadow-manager-frozen-rejected
run_expected_rejection multithread-reject native-shadow-manager-multithread-rejected

set_mode safe-reuse
sudo systemctl start boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal
sudo systemctl reset-failed "$unit_name"

set_mode normal
sudo systemctl start boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
first_invocation=$(unit_invocation_id)
sudo systemctl restart boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
second_invocation=$(unit_invocation_id)
[[ "$first_invocation" != "$second_invocation" ]] \
  || die "explicit restart reused the same InvocationID"
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal

echo "native-shadow manager cgroup gate: PASS"
