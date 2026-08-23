#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "native-shadow manager cgroup gate: $*" >&2
  exit 1
}

closed_local_replay_only=false
closed_local_replay_rootfs=''
closed_local_replay_manifest=''
if [[ $# -eq 3 && $1 == --closed-local-replay-rootfs ]]; then
  closed_local_replay_only=true
  closed_local_replay_rootfs=$(readlink -f -- "$2")
  closed_local_replay_manifest=$(readlink -f -- "$3")
  [[ -d "$closed_local_replay_rootfs" && ! -L "$2" ]] \
    || die "closed-local replay rootfs is not one exact nonsymlink directory"
  [[ -f "$closed_local_replay_manifest" && ! -L "$3" ]] \
    || die "closed-local replay manifest is not one exact nonsymlink file"
  for replay_path in "$closed_local_replay_rootfs" "$closed_local_replay_manifest"; do
    [[ "$replay_path" == /tmp/boole-native-shadow-rootfs-replay.*/* ]] \
      || die "closed-local replay input is outside the gate-owned scratch tree: $replay_path"
    [[ "$replay_path" != *:* && "$replay_path" != *$'\n'* ]] \
      || die "closed-local replay input cannot be represented safely in the systemd drop-in"
  done
elif [[ $# -ne 0 ]]; then
  die "usage: $0 [--closed-local-replay-rootfs ROOTFS CONTENT_MANIFEST]"
fi

[[ $(uname -s) == Linux ]] || die "this gate requires Linux"
[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"
for command_name in awk cargo cp find findmnt getent grep install journalctl paste python3 readlink sed \
  sha256sum sort stat systemctl systemd-run systemd-tmpfiles tee timeout tr; do
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
node_qualification_path=$launcher_directory/boole-native-shadow-node-qualification
node_replay_client_path=$launcher_directory/boole-native-shadow-node-replay-client
authority_stage=''
authority_share=''
authority_parent=''
authority_directory=''
checker_directory=''
fixture_directory=''
toolchain_parent=/opt/boole
toolchain_prefix=$toolchain_parent/native-checker-toolchain
toolchain_stage=''
opt_original_mode=''
runtime_parent=/run/boole
runtime_directory=$runtime_parent/native-shadow
socket_path="$runtime_directory/launcher.sock"
mode_path=$runtime_directory/manager-cgroup-gate-mode
recovery_release_path=$runtime_directory/startup-recovery-release
service_root=/sys/fs/cgroup/system.slice/$unit_name
manager_root=$service_root/manager
temp_root=${RUNNER_TEMP:-/tmp}
build_json=$(mktemp "$temp_root/boole-native-shadow-manager-build.XXXXXX")
node_build_json=$(mktemp "$temp_root/boole-native-shadow-node-build.XXXXXX")
replay_client_build_json=$(mktemp "$temp_root/boole-native-shadow-replay-client-build.XXXXXX")
log=$(mktemp "$temp_root/boole-native-shadow-manager.XXXXXX")
node_log=$(mktemp "$temp_root/boole-native-shadow-node.XXXXXX")
replay_client_log=$(mktemp "$temp_root/boole-native-shadow-replay-client.XXXXXX")
dropin_source=$(mktemp "$temp_root/boole-native-shadow-manager-dropin.XXXXXX")
node_unit=''
work_path=/work
mutation_backup=''
mutation_target=''

launcher_directory_created=false
runtime_parent_created=false
runtime_directory_created=false
unit_installed=false
unit_dropin_directory_created=false
unit_dropin_installed=false
launcher_installed=false
node_qualification_installed=false
node_replay_client_installed=false
toolchain_parent_created=false
toolchain_installed=false
opt_mode_changed=false
checker_installed=false
fixture_installed=false
work_created=false
declare -a installed_authorities=()

cleanup_gate() {
  set +e
  if [[ -n "$mutation_backup" && -n "$mutation_target" && -f "$mutation_backup" ]]; then
    sudo cp --preserve=all "$mutation_backup" "$mutation_target" >/dev/null 2>&1 || :
  fi
  [[ -z "$mutation_backup" ]] || sudo rm -f "$mutation_backup"
  if [[ -n "$node_unit" ]]; then
    sudo systemctl stop "${node_unit}.service" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "${node_unit}.service" >/dev/null 2>&1 || :
  fi
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
  [[ "$node_qualification_installed" == true ]] && sudo rm -f "$node_qualification_path"
  [[ "$node_replay_client_installed" == true ]] && sudo rm -f "$node_replay_client_path"
  [[ "$toolchain_installed" == true ]] && sudo rm -rf "$toolchain_prefix"
  [[ "$toolchain_parent_created" == true ]] && sudo rmdir "$toolchain_parent" >/dev/null 2>&1 || :
  [[ "$opt_mode_changed" == true ]] && sudo chmod "$opt_original_mode" /opt
  if [[ "$checker_installed" == true ]]; then
    sudo rm -f \
      "$checker_directory/checker.py" \
      "$checker_directory/policy.json" \
      "$checker_directory/RELEASE-MANIFEST.json"
    sudo rmdir "$checker_directory" >/dev/null 2>&1 || :
    sudo rmdir "$(dirname "$checker_directory")" >/dev/null 2>&1 || :
  fi
  if [[ "$fixture_installed" == true ]]; then
    sudo rm -f "$fixture_directory/task.json" "$fixture_directory/anchor.rs"
    sudo rmdir "$fixture_directory" >/dev/null 2>&1 || :
    sudo rmdir "$(dirname "$fixture_directory")" >/dev/null 2>&1 || :
  fi
  local basename
  for basename in "${installed_authorities[@]}"; do
    sudo rm -f "$authority_directory/$basename"
  done
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$mode_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$recovery_release_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$socket_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$runtime_directory/launcher.lock"
  [[ "$runtime_directory_created" == true ]] && sudo rmdir "$runtime_directory" >/dev/null 2>&1 || :
  [[ "$runtime_parent_created" == true ]] && sudo rmdir "$runtime_parent" >/dev/null 2>&1 || :
  [[ "$work_created" == true ]] && sudo rmdir "$work_path" >/dev/null 2>&1 || :
  if [[ -n "$authority_stage" ]]; then
    sudo rmdir "$authority_directory" >/dev/null 2>&1 || :
    sudo rmdir "$authority_parent" >/dev/null 2>&1 || :
    sudo rmdir "$authority_share" >/dev/null 2>&1 || :
    sudo rmdir "$authority_stage" >/dev/null 2>&1 || :
  fi
  [[ "$launcher_directory_created" == true ]] && sudo rmdir "$launcher_directory" >/dev/null 2>&1 || :
  [[ -z "$toolchain_stage" ]] || rm -rf "$toolchain_stage"
  rm -f "$build_json" "$node_build_json" "$replay_client_build_json" \
    "$log" "$node_log" "$replay_client_log" "$dropin_source"
}
trap cleanup_gate EXIT

for identity in boole-node boole-native-checker; do
  getent passwd "$identity" >/dev/null || die "missing service user: $identity"
  getent group "$identity" >/dev/null || die "missing service group: $identity"
done

load_state=$(systemctl show "$unit_name" --property=LoadState --value 2>/dev/null || :)
[[ "$load_state" == not-found ]] \
  || die "refusing to shadow pre-existing loaded unit: $unit_name ($load_state)"

for path in "$unit_path" "$unit_dropin_directory" "$launcher_path" \
  "$node_qualification_path" "$node_replay_client_path" "$runtime_directory" \
  "$service_root" "$toolchain_prefix"; do
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

cargo test --locked -p boole-node --lib \
  --no-run --message-format=json >"$node_build_json"

mapfile -t node_executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    target = item.get("target", {})
    if (
        item.get("reason") == "compiler-artifact"
        and target.get("name") == "boole_node"
        and "lib" in target.get("kind", [])
        and item.get("profile", {}).get("test") is True
        and item.get("executable")
    ):
        print(item["executable"])
' "$node_build_json"
)
[[ ${#node_executables[@]} -eq 1 ]] \
  || die "expected one boole-node lib test executable, got ${#node_executables[@]}"
node_qualification_source=${node_executables[0]}
[[ -x "$node_qualification_source" ]] || die "boole-node qualification test is not executable"

cargo test --locked -p boole-native-shadow-launcher \
  --features manager-cgroup-linux-gate \
  --test boole-native-shadow-closed-local-replay-client-linux \
  --no-run --message-format=json >"$replay_client_build_json"

mapfile -t replay_client_executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole-native-shadow-closed-local-replay-client-linux"
        and item.get("executable")
    ):
        print(item["executable"])
' "$replay_client_build_json"
)
[[ ${#replay_client_executables[@]} -eq 1 ]] \
  || die "expected one closed-local replay client, got ${#replay_client_executables[@]}"
node_replay_client_source=${replay_client_executables[0]}
[[ -x "$node_replay_client_source" ]] || die "closed-local replay client is not executable"

toolchain_stage=$(mktemp -d "$temp_root/boole-native-shadow-toolchain.XXXXXX")
./scripts/install-native-checker-toolchain.sh "$toolchain_stage"
[[ $(stat -c %U:%G /opt) == root:root ]] \
  || die "fixed /opt ancestor is not root-owned"
opt_original_mode=$(stat -c %a /opt)
if (( (8#$opt_original_mode & 8#022) != 0 )); then
  sudo chmod go-w /opt
  opt_mode_changed=true
fi
[[ $((8#$(stat -c %a /opt) & 8#022)) -eq 0 ]] \
  || die "fixed /opt ancestor remains group/other writable"
if [[ ! -d "$toolchain_parent" ]]; then
  sudo install -d -o root -g root -m 0755 "$toolchain_parent"
  toolchain_parent_created=true
fi
sudo install -d -o root -g root -m 0555 "$toolchain_prefix"
toolchain_installed=true
sudo cp -a "$toolchain_stage/." "$toolchain_prefix/"
sudo chown -R root:root "$toolchain_prefix"
sudo chmod 0555 "$toolchain_prefix" "$toolchain_prefix/bin"
[[ $(sudo stat -c %U:%G:%a "$toolchain_prefix") == root:root:555 ]] \
  || die "installed toolchain root metadata differs"
[[ $(sudo stat -c %U:%G:%a "$toolchain_prefix/bin") == root:root:555 ]] \
  || die "installed toolchain bin metadata differs"
for executable in rustc cargo; do
  [[ $(sudo stat -c %U:%G "$toolchain_prefix/bin/$executable") == root:root ]] \
    || die "installed $executable owner/group differs"
  [[ -x "$toolchain_prefix/bin/$executable" ]] \
    || die "installed $executable is not executable"
done

if [[ ! -d "$launcher_directory" ]]; then
  sudo install -d -o root -g root -m 0755 "$launcher_directory"
  launcher_directory_created=true
fi
[[ $(sudo stat -c %U:%G:%a "$launcher_directory") == root:root:755 ]] \
  || die "launcher staging directory does not match root:root:755"
sudo install -o root -g root -m 0755 "$harness" "$launcher_path"
launcher_installed=true
sudo install -o root -g root -m 0755 \
  "$node_qualification_source" "$node_qualification_path"
node_qualification_installed=true
sudo install -o root -g root -m 0755 \
  "$node_replay_client_source" "$node_replay_client_path"
node_replay_client_installed=true
[[ $(sha256sum "$harness" | awk '{ print $1 }') == $(sudo sha256sum "$launcher_path" | awk '{ print $1 }') ]] \
  || die "installed launcher bytes differ from reviewed harness"
[[ $(sudo stat -c %U:%G:%a "$launcher_path") == root:root:755 ]] \
  || die "installed launcher metadata does not match root:root:755"
[[ $(sha256sum "$node_qualification_source" | awk '{ print $1 }') == $(sudo sha256sum "$node_qualification_path" | awk '{ print $1 }') ]] \
  || die "installed boole-node qualification bytes differ from the reviewed test binary"
[[ $(sudo stat -c %U:%G:%a "$node_qualification_path") == root:root:755 ]] \
  || die "installed boole-node qualification metadata does not match root:root:755"
[[ $(sha256sum "$node_replay_client_source" | awk '{ print $1 }') == $(sudo sha256sum "$node_replay_client_path" | awk '{ print $1 }') ]] \
  || die "installed closed-local replay client bytes differ from the reviewed binary"
[[ $(sudo stat -c %U:%G:%a "$node_replay_client_path") == root:root:755 ]] \
  || die "installed closed-local replay client metadata does not match root:root:755"

authority_stage=$(sudo mktemp -d /run/boole-native-shadow-manager-authority.XXXXXX)
authority_share="$authority_stage/share"
authority_parent="$authority_share/boole"
authority_directory="$authority_parent/native-shadow"
checker_directory="$authority_directory/checkers/rust-tuple-struct-project-v1"
fixture_directory="$authority_directory/fixtures/a-rooted-native-mining-e2e-v1-real-history"
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
install_authority native/containment/native-shadow-local-execution-authority-v1.json local-execution-authority-v1.json
install_authority native/containment/native-shadow-closed-local-replay-grant-v1.json closed-local-replay-grant-v1.json
install_authority native/containment/native-shadow-closed-local-replay-registry-overlay-v1.json closed-local-replay-registry-overlay-v1.json
install_authority native/containment/native-shadow-closed-local-replay-execution-authority-v1.json closed-local-replay-execution-authority-v1.json
sudo install -d -o root -g root -m 0555 "$(dirname "$checker_directory")"
sudo install -d -o root -g root -m 0555 "$checker_directory"
checker_installed=true
sudo install -o root -g root -m 0444 \
  native/checker/rust-tuple-struct-project-v1/checker.py "$checker_directory/checker.py"
sudo install -o root -g root -m 0444 \
  native/checker/rust-tuple-struct-project-v1/policy.json "$checker_directory/policy.json"
sudo install -o root -g root -m 0444 \
  native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json \
  "$checker_directory/RELEASE-MANIFEST.json"
[[ $(sha256sum native/checker/rust-tuple-struct-project-v1/checker.py | awk '{ print $1 }') == $(sudo sha256sum "$checker_directory/checker.py" | awk '{ print $1 }') ]] \
  || die "installed checker bytes differ"
[[ $(sha256sum native/checker/rust-tuple-struct-project-v1/policy.json | awk '{ print $1 }') == $(sudo sha256sum "$checker_directory/policy.json" | awk '{ print $1 }') ]] \
  || die "installed checker policy bytes differ"
[[ $(sha256sum native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json | awk '{ print $1 }') == $(sudo sha256sum "$checker_directory/RELEASE-MANIFEST.json" | awk '{ print $1 }') ]] \
  || die "installed checker release manifest bytes differ"
sudo install -d -o root -g root -m 0555 "$(dirname "$fixture_directory")"
sudo install -d -o root -g root -m 0555 "$fixture_directory"
fixture_installed=true
sudo install -o root -g root -m 0444 \
  fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/task.json \
  "$fixture_directory/task.json"
sudo install -o root -g root -m 0444 \
  fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/anchor.rs \
  "$fixture_directory/anchor.rs"
for fixture_name in task.json anchor.rs; do
  [[ $(sha256sum "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/$fixture_name" | awk '{ print $1 }') == $(sudo sha256sum "$fixture_directory/$fixture_name" | awk '{ print $1 }') ]] \
    || die "installed replay fixture differs: $fixture_name"
done

if [[ ! -e "$work_path" && ! -L "$work_path" ]]; then
  sudo install -d -o root -g root -m 0555 "$work_path"
  work_created=true
fi
[[ -d "$work_path" && ! -L "$work_path" ]] \
  || die "fixed workspace mountpoint is not a nonsymlink directory"
[[ $(sudo stat -c %U:%G:%a "$work_path") == root:root:555 ]] \
  || die "fixed workspace mountpoint does not match root:root:0555"

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
if [[ "$closed_local_replay_only" == true ]]; then
  expected_dropin+=$'\n'"BindReadOnlyPaths=${closed_local_replay_rootfs}:/var/lib/boole/native-shadow/runtime-rootfs"
  expected_dropin+=$'\n'"BindReadOnlyPaths=${closed_local_replay_manifest}:/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json"
fi
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

journal_marker_count() {
  local marker=$1
  sudo journalctl --sync
  sudo journalctl --no-pager -o cat -u "$unit_name" \
    | grep -Fxc "$marker" || :
}

wait_for_marker_increment() {
  local marker=$1
  local before=$2
  local expected=$((before + 1))
  local observed=0
  local i
  for ((i = 0; i < 200; i++)); do
    observed=$(journal_marker_count "$marker")
    [[ "$observed" -eq "$expected" ]] && return 0
    if (( observed > expected )); then
      die "unit emitted marker more than once: $marker"
    fi
    sleep 0.05
  done
  sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
  die "unit marker count did not advance exactly once: $marker (before=$before last=$observed)"
}

wait_for_fixed_socket() {
  local metadata=''
  local i
  for ((i = 0; i < 200; i++)); do
    if sudo test -S "$socket_path"; then
      metadata=$(sudo stat -c %U:%G:%a "$socket_path")
      [[ "$metadata" == root:boole-node:660 ]] \
        || die "fixed qualification socket metadata differs: $metadata"
      return 0
    fi
    if [[ $(sudo systemctl show "$unit_name" --property=ActiveState --value 2>/dev/null || :) == failed ]]; then
      sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
      die "unit failed before the fixed qualification socket appeared"
    fi
    sleep 0.05
  done
  sudo systemctl show "$unit_name" --property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :
  sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
  die "fixed qualification socket did not appear"
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
  local marker_count_before
  set_mode "$mode"
  marker_count_before=$(journal_marker_count "$marker")
  sudo systemctl start boole-native-shadow-launcher.service
  wait_for_state inactive
  [[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
    || die "$mode rejection harness did not exit successfully"
  wait_for_marker_increment "$marker" "$marker_count_before"
  wait_for_cgroup_removal
}

run_closed_local_replay_gate() {
  local mutation_relative
  local mutation_expected_sha
  mutation_relative=$(jq -er \
    '[.entries[] | select(.kind == "file" and (.sizeBytes // 0) > 0 and (.sizeBytes // 0) <= 4096)] | sort_by(.logicalPath) | .[0].logicalPath | ltrimstr("/")' \
    "$closed_local_replay_manifest")
  mutation_expected_sha=$(jq -er --arg path "/$mutation_relative" \
    '.entries[] | select(.logicalPath == $path) | .sha256' \
    "$closed_local_replay_manifest")
  [[ "$mutation_relative" != /* && "$mutation_relative" != *..* && "$mutation_relative" != *$'\n'* ]] \
    || die "frozen mutation target is not one canonical relative path"
  [[ "$mutation_expected_sha" =~ ^[0-9a-f]{64}$ ]] \
    || die "frozen mutation target lacks one exact SHA-256"
  mutation_target="$closed_local_replay_rootfs/$mutation_relative"
  [[ -f "$mutation_target" && ! -L "$mutation_target" ]] \
    || die "frozen mutation target is not one regular file"
  [[ $(sha256sum "$mutation_target" | awk '{ print $1 }') == "$mutation_expected_sha" ]] \
    || die "frozen mutation target differs before the rejection gate"
  mutation_backup=$(mktemp "$temp_root/boole-native-shadow-rootfs-mutation-backup.XXXXXX")
  sudo cp --preserve=all "$mutation_target" "$mutation_backup"

  # First prove that startup verification is not stale authority.  The
  # listener opens over the exact tree, then one host-side byte changes before
  # Execute. The request-time full-tree rehash must close the connection before
  # a cgroup leaf or checker report can exist.
  set_mode closed-local-replay-three
  sudo systemctl start "$unit_name"
  local drift_invocation
  drift_invocation=$(unit_invocation_id)
  assert_manager_invariants >/dev/null
  wait_for_fixed_socket
  sudo python3 - "$mutation_target" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = bytearray(path.read_bytes())
if not data:
    raise SystemExit("mutation target unexpectedly empty")
data[0] ^= 1
path.write_bytes(data)
PY
  [[ $(sha256sum "$mutation_target" | awk '{ print $1 }') != "$mutation_expected_sha" ]] \
    || die "rootfs mutation did not change the selected file"

  local suffix
  suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
  suffix=${suffix//[^a-zA-Z0-9-]/-}
  node_unit="boole-native-shadow-node-replay-drift-${suffix}"
  set +e
  timeout --foreground --signal=TERM --kill-after=10s 120s \
    sudo systemd-run --quiet --pipe --wait --collect --unit="$node_unit" \
      --property=Type=exec --property=User=boole-node --property=Group=boole-node \
      --property=CapabilityBoundingSet= --property=AmbientCapabilities= \
      --property=NoNewPrivileges=yes --property=PrivateMounts=yes \
      --property=PrivateNetwork=yes --property=RestrictAddressFamilies=AF_UNIX \
      --property=ProtectSystem=strict \
      --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
      --property=WorkingDirectory=/ \
      "$node_replay_client_path" >"$replay_client_log" 2>&1
  local drift_client_status=$?
  set -e
  cat "$replay_client_log"
  [[ $drift_client_status -ne 0 && $drift_client_status -ne 124 ]] \
    || die "mutated rootfs did not fail one client promptly before checker execution"
  [[ $(grep -Fc 'native-shadow-closed-local-replay-report:' "$replay_client_log" || :) -eq 0 ]] \
    || die "request-time rootfs mutation produced a checker Report"
  [[ $(grep -Fc 'native-shadow-closed-local-replay-client-complete:' "$replay_client_log" || :) -eq 0 ]] \
    || die "request-time rootfs mutation completed the three-session matrix"
  sudo systemctl stop "$unit_name"
  wait_for_state inactive
  sudo journalctl --sync
  local drift_journal
  drift_journal=$(sudo journalctl --no-pager -o cat -u "$unit_name" \
    "_SYSTEMD_INVOCATION_ID=$drift_invocation")
  grep -F 'runtime rootfs replay identity drifted' <<<"$drift_journal" >/dev/null \
    || die "request-time rootfs mutation was not the launcher rejection reason"
  if sudo test -d "$service_root"; then
    [[ -z $(sudo find "$service_root" -mindepth 1 -maxdepth 1 -type d -name 'run-*' -print -quit) ]] \
      || die "request-time rootfs rejection created a checker cgroup leaf"
  fi
  sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
    || die "request-time rootfs rejection left the fixed socket behind"
  wait_for_cgroup_removal
  sudo systemctl reset-failed "$unit_name" >/dev/null 2>&1 || :
  local drift_load_state=''
  local drift_wait
  for ((drift_wait = 0; drift_wait < 200; drift_wait++)); do
    drift_load_state=$(sudo systemctl show "${node_unit}.service" \
      --property=LoadState --value 2>/dev/null || :)
    [[ "$drift_load_state" == not-found ]] && break
    sleep 0.05
  done
  [[ "$drift_load_state" == not-found ]] \
    || die "rootfs-drift transient client unit was not collected"
  node_unit=''

  sudo cp --preserve=all "$mutation_backup" "$mutation_target"
  [[ $(sha256sum "$mutation_target" | awk '{ print $1 }') == "$mutation_expected_sha" ]] \
    || die "frozen mutation target was not restored exactly"
  sudo rm -f "$mutation_backup"
  mutation_backup=''
  mutation_target=''
  : >"$replay_client_log"

  # Now start a fresh launcher over the restored exact tree and prove all
  # three checker-executing matrix rows through the real protocol.
  set_mode closed-local-replay-three
  sudo systemctl start "$unit_name"
  local launcher_invocation
  launcher_invocation=$(unit_invocation_id)
  assert_manager_invariants >/dev/null
  wait_for_fixed_socket

  node_unit="boole-native-shadow-node-replay-${suffix}"
  set +e
  timeout --foreground --signal=TERM --kill-after=10s 420s \
    sudo systemd-run --quiet --pipe --wait --collect --unit="$node_unit" \
      --property=Type=exec --property=User=boole-node --property=Group=boole-node \
      --property=CapabilityBoundingSet= --property=AmbientCapabilities= \
      --property=NoNewPrivileges=yes --property=PrivateMounts=yes \
      --property=PrivateNetwork=yes --property=RestrictAddressFamilies=AF_UNIX \
      --property=ProtectSystem=strict \
      --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
      --property=WorkingDirectory=/ \
      "$node_replay_client_path" >"$replay_client_log" 2>&1
  local client_status=$?
  set -e
  cat "$replay_client_log"
  [[ $client_status -eq 0 ]] || die "closed-local replay client failed or exceeded its outer deadline"

  local client_complete
  client_complete="native-shadow-closed-local-replay-client-complete:launcher_connections=3:empty_connections=0"
  [[ $(grep -Fxc "$client_complete" "$replay_client_log" || :) -eq 1 ]] \
    || die "closed-local replay client did not prove exactly three checker connections and zero empty connections"
  local -a expected_reports=(
    "native-shadow-closed-local-replay-report:accepted:accepted:accepted:cleanup=true"
    "native-shadow-closed-local-replay-report:tampered:deterministic_reject:compile_or_hidden_test_failed:cleanup=true"
    "native-shadow-closed-local-replay-report:constant:deterministic_reject:compile_or_hidden_test_failed:cleanup=true"
  )
  local report
  for report in "${expected_reports[@]}"; do
    [[ $(grep -Fxc "$report" "$replay_client_log" || :) -eq 1 ]] \
      || die "closed-local replay client did not validate one exact Report: $report"
  done
  [[ $(grep -Fc 'native-shadow-closed-local-replay-report:' "$replay_client_log" || :) -eq 3 ]] \
    || die "closed-local replay client observed a non-exact Report count"

  local client_pid
  client_pid=$(sed -n 's/^native-shadow-closed-local-replay-client-pid:\([1-9][0-9]*\)$/\1/p' \
    "$replay_client_log")
  [[ "$client_pid" =~ ^[1-9][0-9]*$ ]] \
    || die "closed-local replay client did not publish one exact process identity"
  sudo journalctl --sync
  local -a peer_pids=()
  mapfile -t peer_pids < <(
    sudo journalctl --no-pager -o cat -u "$unit_name" \
      "_SYSTEMD_INVOCATION_ID=$launcher_invocation" \
      | sed -n 's/^native-shadow-active-execution-peer:pid=\([1-9][0-9]*\)$/\1/p'
  )
  [[ ${#peer_pids[@]} -eq 3 ]] \
    || die "launcher did not observe exactly three SO_PEERCRED process identities"
  local peer_pid
  for peer_pid in "${peer_pids[@]}"; do
    [[ "$peer_pid" == "$client_pid" ]] \
      || die "launcher SO_PEERCRED PID differs from the one fixed client process"
  done

  wait_for_state inactive
  [[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
    || die "three-session replay launcher did not exit successfully"
  [[ $(sudo systemctl show "$unit_name" --property=NRestarts --value) == 0 ]] \
    || die "three-session replay launcher restarted unexpectedly"
  wait_for_marker native-shadow-closed-local-replay-three-complete "$launcher_invocation"
  sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
    || die "three-session replay left the fixed socket path behind"
  wait_for_cgroup_removal
  [[ -z $(sudo find /run/boole/native-shadow -maxdepth 1 -name 'rootfs-*' -print -quit) ]] \
    || die "three-session replay left a derived runtime-root path behind"
  [[ -z $(findmnt -rn -o TARGET | grep -F '/run/boole/native-shadow/rootfs-' || :) ]] \
    || die "three-session replay left a derived runtime-root mount behind"

  local node_load_state=''
  local i
  for ((i = 0; i < 200; i++)); do
    node_load_state=$(sudo systemctl show "${node_unit}.service" \
      --property=LoadState --value 2>/dev/null || :)
    [[ "$node_load_state" == not-found ]] && break
    sleep 0.05
  done
  [[ "$node_load_state" == not-found ]] \
    || die "closed-local replay transient client unit was not collected"
  node_unit=''
  echo "native-shadow closed-local replay three-session gate: PASS"
}

if [[ "$closed_local_replay_only" == true ]]; then
  run_closed_local_replay_gate
  exit 0
fi

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

listener_mode="qualification-one-shot"
set_mode "$listener_mode"
sudo systemctl start boole-native-shadow-launcher.service
qualification_invocation=$(unit_invocation_id)
assert_manager_invariants >/dev/null
wait_for_fixed_socket

suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
suffix=${suffix//[^a-zA-Z0-9-]/-}
node_unit="boole-native-shadow-node-qualification-${suffix}"
set +e
sudo systemd-run --quiet --pipe --wait --collect --unit="$node_unit" \
  --property=Type=exec --property=User=boole-node --property=Group=boole-node \
  --property=CapabilityBoundingSet= --property=AmbientCapabilities= \
  --property=NoNewPrivileges=yes --property=PrivateMounts=yes \
  --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
  --property=WorkingDirectory=/ \
  "$node_qualification_path" \
  native_shadow_qualification::tests::installed_launcher_round_trip_is_ready_only \
  --ignored --exact --nocapture >"$node_log" 2>&1
node_status=$?
set -e
cat "$node_log"
[[ $node_status -eq 0 ]] || die "boole-node installed qualification round trip failed"
node_marker_count=$(grep -Fxc native-shadow-node-qualification-ready-only "$node_log" || :)
[[ "$node_marker_count" -eq 1 ]] \
  || die "boole-node qualification test did not execute exactly once"
for ((i = 0; i < 200; i++)); do
  node_load_state=$(sudo systemctl show "${node_unit}.service" \
    --property=LoadState --value 2>/dev/null || :)
  [[ "$node_load_state" == not-found ]] && break
  sleep 0.05
done
[[ "$node_load_state" == not-found ]] \
  || die "boole-node qualification transient unit was not collected"
node_unit=''

wait_for_state inactive
[[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
  || die "one-shot qualification launcher did not exit successfully"
[[ $(sudo systemctl show "$unit_name" --property=NRestarts --value) == 0 ]] \
  || die "one-shot qualification launcher restarted unexpectedly"
wait_for_marker native-shadow-qualification-one-shot-complete "$qualification_invocation"
sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
  || die "one-shot qualification left the fixed socket path behind"
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
