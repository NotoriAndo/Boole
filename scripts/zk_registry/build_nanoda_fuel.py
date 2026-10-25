"""Build the pinned C6 nanoda from local git objects and offline Cargo sources."""
from pathlib import Path
import argparse
import copy
import json
import os
import shutil
import subprocess
import sys
from .build_fuel_tools import archive, digest

ROOT = Path(__file__).resolve().parent
REVISION = '0f88b5f382f0f3f08aed4af0dd6c0e48c3fd7a44'
MODULES = {'NanodaFuel.rs': 'gate_fuel.rs', 'NanodaDriver.rs': 'gate_driver.rs',
           'NanodaMain.rs': 'main.rs'}
SOURCE_FILES = (
    'src/debug_printer.rs', 'src/env.rs', 'src/expr.rs', 'src/gate_driver.rs',
    'src/gate_fuel.rs', 'src/inductive.rs', 'src/level.rs', 'src/lib.rs',
    'src/main.rs', 'src/name.rs', 'src/parser.rs', 'src/pretty_printer.rs',
    'src/quot.rs', 'src/tc.rs', 'src/tests.rs', 'src/tests/level.rs',
    'src/tests/name.rs', 'src/tests/natlit.rs', 'src/tests/pretty_printer.rs',
    'src/tests/util.rs', 'src/union_find.rs', 'src/unique_hasher.rs', 'src/util.rs')


def symbolic_identity(identity, build_root, registry):
    """Keep exact roots private; lengths preserve build-input provenance."""
    roots = sorted(((str(build_root), '<build-root>'),
                    (str(registry), '<cargo-registry>')), key=lambda item: len(item[0]), reverse=True)
    def scrub(value):
        if isinstance(value, dict):
            return {key: scrub(item) for key, item in value.items()}
        if isinstance(value, list):
            return [scrub(item) for item in value]
        if isinstance(value, str):
            for root, token in roots:
                value = value.replace(root, token)
        return value
    result = scrub(copy.deepcopy(identity))
    result['flags'] = ['--remap-path-prefix', '<build-root>/nanoda=/boole/nanoda',
                       '--remap-path-prefix', '<cargo-registry>=/boole/cargo-registry']
    result['source_build_path'] = '<build-root>/nanoda'
    result['root_utf8_bytes'] = {'<build-root>': len(str(build_root).encode('utf-8')),
                                '<cargo-registry>': len(str(registry).encode('utf-8'))}
    return result


def save_local_identity(identity, sidecar):
    sidecar = Path(sidecar).resolve()
    ignored = subprocess.run(['git', 'check-ignore', '--quiet', '--', str(sidecar)],
                             cwd=ROOT, capture_output=True)
    if ignored.returncode != 0:
        raise ValueError('exact build roots require an explicitly ignored sidecar')
    sidecar.parent.mkdir(parents=True, exist_ok=True)
    with sidecar.open('x') as stream:
        os.chmod(sidecar, 0o600)
        json.dump(identity, stream, indent=2, sort_keys=True)
        stream.write('\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nanoda-source', required=True, type=Path)
    parser.add_argument('--cargo', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--local-sidecar', required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists():
        raise ValueError('exclusive new output directory required')
    out.mkdir(parents=True)
    source = out / 'nanoda'
    cargo = args.cargo.resolve(strict=True)
    rustc = cargo.with_name('rustc').resolve(strict=True)
    if cargo.name != 'cargo' or rustc.name != 'rustc' or os.path.samefile(cargo, rustc):
        raise ValueError('explicit native Cargo and rustc executables required; do not pass rustup proxies')
    cache = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))) / 'registry'
    cache = cache.resolve(strict=True)
    cargo_home = out / 'cargo-home'
    cargo_home.mkdir()
    (cargo_home / 'registry').symlink_to(cache, target_is_directory=True)
    for name in ('home', 'tmp'):
        (out / name).mkdir()
    flags = ['--remap-path-prefix', str(source) + '=/boole/nanoda',
             '--remap-path-prefix', str(cache) + '=/boole/cargo-registry']
    # No inherited compiler flags, wrappers, Cargo profile overrides or user
    # Cargo configuration. Cargo runs from / with an explicit manifest path.
    env = dict(PATH=str(cargo.parent) + ':/usr/bin:/bin', HOME=str(out / 'home'),
               TMPDIR=str(out / 'tmp'), CARGO_HOME=str(cargo_home),
               RUSTUP_AUTO_INSTALL='0', CARGO_NET_OFFLINE='true', CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0',
               CARGO_TARGET_DIR=str(out / 'target'), RUSTC=str(rustc),
               CARGO_ENCODED_RUSTFLAGS='\x1f'.join(flags),
               PYTHONDONTWRITEBYTECODE='1', GIT_CONFIG_GLOBAL='/dev/null',
               GIT_CONFIG_NOSYSTEM='1', GIT_TERMINAL_PROMPT='0', LANG='C', LC_ALL='C')
    identity = dict(rustc_vV=subprocess.check_output([str(rustc), '-vV'], env=env, text=True),
                    rustc_sha256=digest(rustc), cargo_sha256=digest(cargo),
                    flags=flags, cargo_config_isolated=True, inherited_flags_ignored=True,
                    cargo_cwd='/', source_build_path=str(source))
    save_local_identity(identity, args.local_sidecar)
    identity = symbolic_identity(identity, out, cache)
    archive(args.nanoda_source.resolve(strict=True), REVISION, source, env)
    subprocess.run(['git', 'init', '--quiet', str(source)], env=env, check=True)
    patch = ROOT / 'data/nanoda-0f88b5f-fuel.patch'
    subprocess.run(['git', 'apply', '--unidiff-zero', str(patch)], cwd=source, env=env, check=True)
    own_patch = ROOT / 'data/nanoda-0f88b5f-own.patch'
    subprocess.run(['git', 'apply', '--unidiff-zero', str(own_patch)], cwd=source, env=env, check=True)
    for canonical, destination in MODULES.items():
        shutil.copyfile(ROOT / 'rust' / canonical, source / 'src' / destination)
    public_sources = ROOT.parents[1] / 'fixtures/nanoda-public-sources-v1'
    for expected in [public_sources/'Cargo.toml', public_sources/'Cargo.lock',
                     *sorted((public_sources/'src').rglob('*.rs'))]:
        relative = expected.relative_to(public_sources)
        if digest(source / relative) != digest(expected):
            raise ValueError('patched public nanoda source differs: ' + str(relative))
    if {str(path.relative_to(source)) for path in (source/'src').rglob('*.rs')} != set(SOURCE_FILES):
        raise ValueError('nanoda source inventory differs from the builder list')
    common = ['--manifest-path', str(source / 'Cargo.toml'), '--release', '--locked', '--offline']
    subprocess.run([str(cargo), 'build', *common, '--bin', 'nanoda_bin'], cwd='/', env=env, check=True)
    units = subprocess.run([str(cargo), 'test', *common, '--lib', '--bin', 'nanoda_bin',
                            'gate_', '--', '--test-threads=1'], cwd='/', env=env,
                           capture_output=True, text=True, timeout=180)
    (out / 'NANODA-UNIT-TESTS.json').write_text(json.dumps(dict(exit_code=units.returncode,
        stdout=units.stdout, stderr=units.stderr), indent=2)+'\n')
    units.check_returncode()
    binary = out / 'target/release/nanoda_bin'
    review = subprocess.run([sys.executable, '-B', str(ROOT / 'native_tests/nanoda_review.py'),
        '--binary', str(binary), '--caps', str(ROOT / 'data/testnet_caps_v2.json'),
        '--out', str(out / 'native-review'), '-v'], cwd='/', env=env, capture_output=True,
        text=True, timeout=180)
    (out / 'NANODA-REVIEW-TESTS.json').write_text(json.dumps(dict(exit_code=review.returncode,
        stdout=review.stdout, stderr=review.stderr), indent=2)+'\n')
    review.check_returncode()
    assert digest(cargo)==identity['cargo_sha256'] and digest(rustc)==identity['rustc_sha256']
    result = dict(revision=REVISION, patch_sha256=digest(patch), own_patch_sha256=digest(own_patch), **identity,
                  modules={name: digest(ROOT / 'rust' / name) for name in MODULES},
                  sources={name: digest(source / name) for name in SOURCE_FILES},
                  cargo_toml_sha256=digest(source / 'Cargo.toml'),
                  cargo_lock_sha256=digest(source / 'Cargo.lock'),
                  binary_sha256=digest(binary))
    (out / 'NANODA-FUEL-BUILD.json').write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps(dict(binary_sha256=result['binary_sha256'])))


if __name__ == '__main__':
    main()
