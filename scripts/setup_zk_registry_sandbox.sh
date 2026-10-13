#!/usr/bin/env bash
# Provision the tracked boundary; proof/kernel tools have a separate consumer pin set.
set -euo pipefail
source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
state="${1:?absolute task-owned runtime directory required}"
[[ "$state" == /* && "$state" != / ]]
mkdir -p "$state/gate" "$state/downloads" "$state/home" "$state/tmp"
export HOME="$state/home" TMPDIR="$state/tmp"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0
export npm_config_userconfig=/dev/null PIP_CONFIG_FILE=/dev/null GOENV=off
export PYTHONDONTWRITEBYTECODE=1
arch="$(uname -m)"
case "$arch" in
  aarch64) compiler=/usr/bin/gcc ;;
  x86_64) compiler=/usr/bin/x86_64-linux-gnu-gcc ;;
  *) printf 'ERROR sandbox-unavailable: unsupported architecture\n' >&2; exit 1 ;;
esac
descriptor="$source_root/scripts/zk_registry/data/acceptance_tools_v2.json"
backend="$source_root/scripts/zk_registry/sandbox_backend"
cd "$state/downloads"
apt-get download bubblewrap=0.9.0-1ubuntu0.3 libseccomp-dev=2.5.5-1ubuntu3.1
python3 - "$descriptor" "$arch" "$state/downloads" <<'PY'
import hashlib
import json
from pathlib import Path
import sys
profile = json.loads(Path(sys.argv[1]).read_text())['sandbox']['architectures'][sys.argv[2]]
directory = Path(sys.argv[3])
for package, key in (('bubblewrap', 'bubblewrap_deb_sha256'), ('libseccomp-dev', 'libseccomp_dev_deb_sha256')):
    files = list(directory.glob(package + '_*.deb'))
    assert len(files) == 1
    assert hashlib.sha256(files[0].read_bytes()).hexdigest() == profile[key]
PY
sudo -n env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin DEBIAN_FRONTEND=noninteractive APT_CONFIG=/dev/null \
  apt-get install -y --no-install-recommends "$state"/downloads/bubblewrap_*.deb \
  "$state"/downloads/libseccomp-dev_*.deb
if [[ "$arch" == x86_64 ]]; then
  sudo -n env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin DEBIAN_FRONTEND=noninteractive APT_CONFIG=/dev/null \
    apt-get install -y --no-install-recommends gcc-x86-64-linux-gnu=4:13.2.0-7ubuntu1 \
    libc6-dev-amd64-cross=2.39-0ubuntu8cross1
  "$compiler" --sysroot=/usr/x86_64-linux-gnu -O2 -static -Wall -Wextra -Werror \
    "$backend/isolate.c" -o "$state/gate/gate-isolate"
else
  "$compiler" -O2 -static -Wall -Wextra -Werror "$backend/isolate.c" -o "$state/gate/gate-isolate"
fi
gcc -O2 -Wall -Wextra -Werror "$backend/filter.c" -lseccomp -o "$state/gate/filter-gen"
"$state/gate/filter-gen" "$arch" "$state/gate/seccomp-$arch.bpf"
"$compiler" -O2 -static -Wall -Wextra -Werror "$backend/probes.c" -o "$state/control-probe"
cp "$backend/launcher.py" "$state/gate/launcher.py"
cp "$backend/comparator-landrun" "$state/gate/comparator-landrun"
cp "$descriptor" "$state/gate/bundle.json"
chmod 755 "$state/gate/launcher.py" "$state/gate/comparator-landrun"
sudo -n install -o root -g root -m 644 "$backend/apparmor-bwrap" /etc/apparmor.d/boole-gate-bwrap
sudo -n apparmor_parser -r /etc/apparmor.d/boole-gate-bwrap
python3 - "$descriptor" "$arch" "$state/gate" <<'PY'
import hashlib
import json
from pathlib import Path
import sys
data = json.loads(Path(sys.argv[1]).read_text())['sandbox']
profile = data['architectures'][sys.argv[2]]
root = Path(sys.argv[3])
files = {'launcher': root / 'launcher.py', 'inner_launcher': root / 'comparator-landrun',
         'isolate': root / 'gate-isolate', 'filter': root / ('seccomp-' + sys.argv[2] + '.bpf'),
         'bwrap': Path('/usr/bin/bwrap'), 'apparmor_profile': Path('/etc/apparmor.d/boole-gate-bwrap')}
for name, path in files.items():
    expected = data[name + '_sha256'] if name in {'launcher', 'inner_launcher'} else profile[name + '_sha256']
    assert hashlib.sha256(path.read_bytes()).hexdigest() == expected, name
    assert not path.stat().st_mode & 0o6000, name
sysctl = Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns')
assert not sysctl.exists() or sysctl.read_text().strip() == '1'
print('pinned-sandbox-installed; live capability preflight is still mandatory')
PY
