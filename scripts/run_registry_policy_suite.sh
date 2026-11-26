#!/usr/bin/env bash
# Run inside the provisioned offline Linux test VM, once per full checkout.
set -euo pipefail

if [[ $(uname -s) != Linux || $# != 2 || $1 != /* || ! $2 =~ ^[0-9a-f]{40}$ ]]; then
    echo "usage on Linux: bash run_registry_policy_suite.sh ABSOLUTE_CHECKOUT COMMIT" >&2
    exit 2
fi
cd -- "$1"
[[ $(git rev-parse --show-toplevel) == "$1" && $(git rev-parse HEAD) == "$2" ]]
[[ -z $(git status --porcelain=v1 --untracked-files=all) ]]
export BOOLE_REQUIRE_LEAN=1 RUST_TEST_THREADS=1 PYTHONDONTWRITEBYTECODE=1

# Include every registry module in this commit, even those outside the CI smoke selection.
/usr/bin/python3 -I -B - <<'PY'
from pathlib import Path
import subprocess
import sys

root = Path.cwd()
tracked = subprocess.check_output(
    ['git', 'ls-files', '-z', '--', 'scripts/test_zk_registry_*.py']
).decode('utf-8').split('\0')
modules = sorted(Path(name) for name in tracked if name)
if not modules:
    raise SystemExit('empty registry suite')
modules.extend(Path('scripts') / (name + '.py') for name in (
    'test_statement_identity', 'test_kernel_nonvac', 'test_m3_supervisor_vectors',
    'test_measuring_sticks',
))
for path in modules:
    if not path.is_file():
        raise SystemExit(f'incomplete VM checkout: {path}')

# A fresh interpreter per test module prevents globals or patches leaking to the next.
entry = '''
import sys, unittest
sys.path[:0] = sys.argv[1:3]
unittest.main(module=None, argv=['unittest', '-v', sys.argv[3]])
'''
failed = []
for path in modules:
    print(f'RUN {path}', flush=True)
    proc = subprocess.run([sys.executable, '-I', '-B', '-c', entry,
                           str(root / 'scripts'), str(root), path.stem])
    if proc.returncode:
        failed.append((str(path), proc.returncode))
print(f'Modules completed: {len(modules)}, unsuccessful: {len(failed)}', flush=True)
metadata = subprocess.run(['bash', 'scripts/setup_measuring_stick.sh', '--check-descriptors'])
if failed or metadata.returncode:
    raise SystemExit(f'failed modules {failed}; descriptor check exit {metadata.returncode}')
PY
