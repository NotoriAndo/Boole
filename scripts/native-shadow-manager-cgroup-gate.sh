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
unit_dropin_directory="/run/systemd/system/${unit_name}.d"
unit_dropin_path="$unit_dropin_directory/10-manager-gate-authority.conf"
launcher_directory=/usr/libexec/boole
launcher_path=$launcher_directory/boole-native-shadow-launcher
authority_stage=''
authority_share=''
authority_parent=''
authority_directory=''
runtime_parent=/run/boole
runtime_directory=$runtime_parent/native-shadow
mode_path=$runtime_directory/manager-cgroup-gate-mode
recovery_release_path=$runtime_directory/startup-recovery-release
service_root=/sys/fs/cgroup/system.slice/$unit_name
manager_root=$service_root/manager
temp_root=${RUNNER_TEMP:-/tmp}
build_json=$(mktemp "$temp_root/boole-native-shadow-manager-build.XXXXXX")
log=$(mktemp "$temp_root/boole-native-shadow-manager.XXXXXX")
dropin_source=$(mktemp "$temp_root/boole-native-shadow-manager-dropin.XXXXXX")

launcher_directory_created=false
runtime_parent_created=false
runtime_directory_created=false
unit_installed=false
unit_dropin_directory_created=false
unit_dropin_installed=false
launcher_installed=false
declare -a installed_authorities=()

cleanup_gate() {
  set +e
  if [[ "$unit_installed" == true ]]; then
    sudo systemctl stop "$unit_name" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "$unit_name" >/dev/null 2>&1 || :
    [[ "$unit_dropin_installed" == true ]] && sudo rm -f "$unit_dropin_path"
    [[ "$unit_dropin_directory_created" == true ]] \
      && sudo rmdir "$unit_dropin_directory" >/dev/null 2>&1 || :
    sudo rm -f "$unit_path"
    sudo systemctl daemon-reload >/dev/null 2>&1 || :
  fi
  [[ "$launcher_installed" == true ]] && sudo rm -f "$launcher_path"
  local basename
  for basename in "${installed_authorities[@]}"; do
    sudo rm -f "$authority_directory/$basename"
  done
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$mode_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$recovery_release_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$runtime_directory/launcher.lock"
  [[ "$runtime_directory_created" == true ]] && sudo rmdir "$runtime_directory" >/dev/null 2>&1 || :
  [[ "$runtime_parent_created" == true ]] && sudo rmdir "$runtime_parent" >/dev/null 2>&1 || :
  if [[ -n "$authority_stage" ]]; then
    sudo rmdir "$authority_directory" >/dev/null 2>&1 || :
    sudo rmdir "$authority_parent" >/dev/null 2>&1 || :
    sudo rmdir "$authority_share" >/dev/null 2>&1 || :
    sudo rmdir "$authority_stage" >/dev/null 2>&1 || :
  fi
  [[ "$launcher_directory_created" == true ]] && sudo rmdir "$launcher_directory" >/dev/null 2>&1 || :
  rm -f "$build_json" "$log" "$dropin_source"
}
trap cleanup_gate EXIT

for identity in boole-node boole-native-checker; do
  getent passwd "$identity" >/dev/null || die "missing service user: $identity"
  getent group "$identity" >/dev/null || die "missing service group: $identity"
done

load_state=$(systemctl show "$unit_name" --property=LoadState --value 2>/dev/null || :)
[[ "$load_state" == not-found ]] \
  || die "refusing to shadow pre-existing loaded unit: $unit_name ($load_state)"

for path in "$unit_path" "$unit_dropin_directory" "$launcher_path" "$runtime_directory" \
  "$service_root"; do
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

authority_stage=$(sudo mktemp -d /run/boole-native-shadow-manager-authority.XXXXXX)
authority_share="$authority_stage/share"
authority_parent="$authority_share/boole"
authority_directory="$authority_parent/native-shadow"
sudo chmod 0700 "$authority_stage"
sudo install -d -o root -g root -m 0755 "$authority_share"
sudo install -d -o root -g root -m 0755 "$authority_parent"
sudo install -d -o root -g root -m 0555 "$authority_directory"

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
sudo install -d -o root -g root -m 0755 "$unit_dropin_directory"
unit_dropin_directory_created=true
expected_dropin=$'[Service]\n'"BindReadOnlyPaths=${authority_share}:/usr/share"
printf '%s\n' "$expected_dropin" >"$dropin_source"
unit_dropin_installed=true
sudo install -o root -g root -m 0644 "$dropin_source" "$unit_dropin_path"
[[ $(sudo stat -c %U:%G:%a "$unit_dropin_path") == root:root:644 ]] \
  || die "authority bind drop-in metadata does not match root:root:644"
[[ $(sudo cat "$unit_dropin_path") == "$expected_dropin" ]] \
  || die "authority bind drop-in differs from the exact two-line contract"
sudo systemctl daemon-reload
[[ $(sudo systemctl show "$unit_name" --property=FragmentPath --value) == "$unit_path" ]] \
  || die "systemd did not load the exact tracked unit fragment"
[[ $(sudo systemctl show "$unit_name" --property=DropInPaths --value) == "$unit_dropin_path" ]] \
  || die "systemd did not load exactly the gate-owned authority drop-in"

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
    sudo test ! -e "$service_root" && return 0
    sleep 0.05
  done
  die "service cgroup remained after stop"
}

single_numeric_id() {
  local path=$1
  local -a values=()
  mapfile -t values < <(sudo awk 'NF == 1 { print $1; next } NF > 1 { print "__malformed__" }' "$path")
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
    sudo test -d "$manager_root" || { sleep 0.05; continue; }
    [[ -z $(sudo cat "$service_root/cgroup.procs" | tr -d '[:space:]') ]] \
      || { sleep 0.05; continue; }
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
  [[ $(sudo stat -c %U:%G:%a "$manager_root") == root:root:700 ]] \
    || die "manager cgroup metadata does not match root:root:700"
  [[ -z $(sudo cat "$manager_root/cgroup.subtree_control" | tr -d '[:space:]') ]] \
    || die "manager cgroup has residual subtree controllers"
  [[ $(sudo cat "$manager_root/cgroup.type" | tr -d '[:space:]') == domain ]] \
    || die "manager cgroup type is not exact domain after move"
  [[ -z $(sudo find "$manager_root" -mindepth 1 -maxdepth 1 -type d -print -quit) ]] \
    || die "manager cgroup contains a nested child"
  local controllers
  controllers=$(sudo cat "$service_root/cgroup.subtree_control" | tr ' ' '\n' | sed '/^$/d' | sort | paste -sd' ' -)
  [[ "$controllers" == "cpu memory pids" ]] \
    || die "service subtree controllers differ: $controllers"
  [[ $(sudo stat -fc %T "$service_root") == cgroup2fs ]] || die "service root is not cgroup2fs"
  [[ $(sudo readlink -f "/proc/$pid/exe") == "$launcher_path" ]] \
    || die "MainPID does not execute the staged reviewed harness"
  local relative
  relative=$(sudo awk -F: '$1 == "0" { print $3 }' "/proc/$pid/cgroup")
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

wait_for_marker() {
  local marker=$1
  local invocation_id=$2
  local i
  for ((i = 0; i < 200; i++)); do
    if sudo journalctl --no-pager -o cat -u "$unit_name" \
      "_SYSTEMD_INVOCATION_ID=$invocation_id" | grep -Fqx "$marker"; then
      return 0
    fi
    sleep 0.05
  done
  sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
  die "unit did not emit marker: $marker"
}

wait_for_leaf_event() {
  local leaf=$1
  local key=$2
  local expected=$3
  local i
  local observed=''
  for ((i = 0; i < 200; i++)); do
    observed=$(sudo awk -v key="$key" '$1 == key && NF == 2 { print $2 }' "$leaf/cgroup.events")
    [[ "$observed" == "$expected" ]] && return 0
    sleep 0.05
  done
  die "leaf event $key did not reach $expected (last: $observed)"
}

create_run_leaf() {
  local leaf=$1
  sudo mkdir "$leaf"
  sudo chmod 0700 "$leaf"
  [[ $(sudo stat -c %U:%G:%a "$leaf") == root:root:700 ]] \
    || die "run leaf metadata does not match root:root:700"
}

start_process_tree() {
  local leaf=$1
  sudo python3 -c '
import os
import signal
import sys

leaf = sys.argv[1]
with open(os.path.join(leaf, "cgroup.procs"), "w", encoding="ascii") as stream:
    stream.write(f"{os.getpid()}\n")
child = os.fork()
signal.pause()
' "$leaf" &
  tree_supervisor_pid=$!
}

wait_for_leaf_process_count() {
  local leaf=$1
  local expected=$2
  local i
  local observed=0
  for ((i = 0; i < 200; i++)); do
    observed=$(sudo awk 'NF == 1 { count += 1 } END { print count + 0 }' "$leaf/cgroup.procs")
    [[ "$observed" == "$expected" ]] && return 0
    sleep 0.05
  done
  die "leaf process count did not reach $expected (last: $observed)"
}

pid_start_time() {
  local pid=$1
  python3 -c '
import pathlib
import sys

try:
    text = pathlib.Path(f"/proc/{sys.argv[1]}/stat").read_text(encoding="ascii")
except FileNotFoundError:
    print("missing")
    raise SystemExit(0)
closing = text.rfind(")")
fields = text[closing + 2:].split()
if closing < 0 or len(fields) <= 19:
    raise SystemExit("malformed proc stat")
print(fields[19])
' "$pid"
}

wait_for_original_process_exit() {
  local pid=$1
  local original_start_time=$2
  local current_start_time=''
  local i
  for ((i = 0; i < 200; i++)); do
    current_start_time=$(pid_start_time "$pid") \
      || die "could not identify recovered process: $pid"
    if [[ "$current_start_time" == missing || "$current_start_time" != "$original_start_time" ]]; then
      return 0
    fi
    sleep 0.05
  done
  die "original recovered process remains visible: $pid"
}

wait_for_background_job() {
  local pid=$1
  local i
  for ((i = 0; i < 200; i++)); do
    if ! jobs -pr | grep -Fqx "$pid"; then
      wait "$pid" 2>/dev/null || :
      return 0
    fi
    sleep 0.05
  done
  die "background fixture job did not exit: $pid"
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
}

run_expected_rejection nested-reject native-shadow-manager-nested-rejected
run_expected_rejection frozen-reject native-shadow-manager-frozen-rejected
run_expected_rejection multithread-reject native-shadow-manager-multithread-rejected

startup_recovery_mode="startup-recovery"
set_mode "$startup_recovery_mode"
sudo rm -f "$recovery_release_path"
sudo systemctl start boole-native-shadow-launcher.service
startup_recovery_invocation=$(unit_invocation_id)
wait_for_marker native-shadow-startup-recovery-prepared "$startup_recovery_invocation"
assert_manager_invariants >/dev/null

leaf_a="$service_root/run-0000000000000000000000000000000000000000000000000000000000000001"
leaf_b="$service_root/run-0000000000000000000000000000000000000000000000000000000000000002"
leaf_c="$service_root/run-0000000000000000000000000000000000000000000000000000000000000003"
create_run_leaf "$leaf_a"
create_run_leaf "$leaf_b"
create_run_leaf "$leaf_c"
start_process_tree "$leaf_a"
recovered_tree_a=$tree_supervisor_pid
start_process_tree "$leaf_b"
recovered_tree_b=$tree_supervisor_pid
wait_for_leaf_process_count "$leaf_a" 2
wait_for_leaf_process_count "$leaf_b" 2
mapfile -t recovered_cgroup_pids < <(
  {
    sudo cat "$leaf_a/cgroup.procs"
    sudo cat "$leaf_b/cgroup.procs"
  } | sort -n
)
[[ ${#recovered_cgroup_pids[@]} -eq 4 ]] \
  || die "startup recovery fixture does not contain four cgroup processes"
declare -a recovered_pid_starttimes=()
for recovered_pid in "${recovered_cgroup_pids[@]}"; do
  recovered_start_time=$(pid_start_time "$recovered_pid") \
    || die "could not identify recovery fixture process: $recovered_pid"
  [[ "$recovered_start_time" != missing ]] \
    || die "recovery fixture process disappeared before release: $recovered_pid"
  recovered_pid_starttimes+=("$recovered_start_time")
done
printf '1\n' | sudo tee "$leaf_b/cgroup.freeze" >/dev/null
wait_for_leaf_event "$leaf_b" frozen 1
sudo touch "$recovery_release_path"
wait_for_marker native-shadow-startup-recovery-complete:3 "$startup_recovery_invocation"
for recovered_index in "${!recovered_cgroup_pids[@]}"; do
  wait_for_original_process_exit \
    "${recovered_cgroup_pids[$recovered_index]}" \
    "${recovered_pid_starttimes[$recovered_index]}"
done
wait_for_background_job "$recovered_tree_a"
wait_for_background_job "$recovered_tree_b"
[[ -z $(sudo find "$service_root" -mindepth 1 -maxdepth 1 -type d ! -name manager -print -quit) ]] \
  || die "startup recovery left a non-manager cgroup child"
assert_manager_invariants >/dev/null
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal

inventory_reject_mode="startup-inventory-reject"
set_mode "$inventory_reject_mode"
sudo rm -f "$recovery_release_path"
sudo systemctl start boole-native-shadow-launcher.service
inventory_reject_invocation=$(unit_invocation_id)
wait_for_marker native-shadow-startup-inventory-prepared "$inventory_reject_invocation"
assert_manager_invariants >/dev/null
leaf="$service_root/run-0000000000000000000000000000000000000000000000000000000000000001"
create_run_leaf "$leaf"
start_process_tree "$leaf"
reject_tree=$tree_supervisor_pid
wait_for_leaf_process_count "$leaf" 2
sudo mkdir "$service_root/zzz-unexpected"
frozen=$(sudo awk '$1 == "frozen" && NF == 2 { print $2 }' "$leaf/cgroup.events")
populated=$(sudo awk '$1 == "populated" && NF == 2 { print $2 }' "$leaf/cgroup.events")
[[ "$frozen" == 0 && "$populated" == 1 ]] \
  || die "inventory reject fixture did not begin live and unfrozen"
before_procs=$(sudo sort -n "$leaf/cgroup.procs")
before_threads=$(sudo sort -n "$leaf/cgroup.threads")
sudo touch "$recovery_release_path"
wait_for_marker native-shadow-startup-inventory-untouched "$inventory_reject_invocation"
after_procs=$(sudo sort -n "$leaf/cgroup.procs")
after_threads=$(sudo sort -n "$leaf/cgroup.threads")
[[ "$before_procs" == "$after_procs" && "$before_threads" == "$after_threads" ]] \
  || die "inventory rejection changed the valid leaf process membership"
frozen=$(sudo awk '$1 == "frozen" && NF == 2 { print $2 }' "$leaf/cgroup.events")
populated=$(sudo awk '$1 == "populated" && NF == 2 { print $2 }' "$leaf/cgroup.events")
[[ "$frozen" == 0 && "$populated" == 1 ]] \
  || die "inventory rejection changed valid leaf events"
sudo test -d "$service_root/zzz-unexpected" \
  || die "inventory rejection removed the unexpected child"
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_background_job "$reject_tree"
wait_for_cgroup_removal

set_mode safe-reuse
sudo systemctl start boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal

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
