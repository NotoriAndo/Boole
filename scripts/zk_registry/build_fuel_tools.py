"""Build the pinned C5 native tools from local source objects, without downloads."""
from pathlib import Path, PurePosixPath
import argparse
import hashlib
import io
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parent
COMPARATOR = 'd03acab154d269c06e60e4de7e4cc85deebff94b'
EXPORT = '076e8e57707e813375e8f9da8bf989799ace9680'
PATCH_ORDER = ('export-files', 'transport', 'status', 'fuel', 'nanoda-fuel', 'own')
BASE_FUEL_SOURCES = ('Main.lean', 'Det3Fuel.lean', 'ZkReplay.lean', 'kernel_fuel.cpp', 'CertifiedBase.lean')


def fuel_source_names(counter_replay=False, certified_export=False):
    return (BASE_FUEL_SOURCES + (('CounterReplay.lean',) if counter_replay else ())
            + (('CertifiedExport.lean',) if certified_export else ()))


def digest(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def archive(repository, revision, destination, env=None):
    """A clean pinned object tree, independent of the source worktree's edits."""
    env = dict(os.environ if env is None else env)
    for key in list(env):
        if key.startswith('GIT_CONFIG'):env.pop(key)
    env.update(GIT_CONFIG_GLOBAL='/dev/null',GIT_CONFIG_NOSYSTEM='1',GIT_TERMINAL_PROMPT='0',GIT_NO_LAZY_FETCH='1')
    raw = subprocess.check_output(['git', '-c', 'core.autocrlf=false', '-C', str(repository), 'archive', revision],env=env)
    destination.mkdir()
    with tarfile.open(fileobj=io.BytesIO(raw)) as source:
        for member in source:
            name = PurePosixPath(member.name)
            if name.is_absolute() or '..' in name.parts or not (member.isdir() or member.isfile()):
                raise ValueError('non-regular source archive entry')
            target = destination.joinpath(*name.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as src, target.open('xb') as dst:
                    shutil.copyfileobj(src, dst)
                target.chmod(member.mode & 0o777)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lean-prefix', required=True, type=Path)
    parser.add_argument('--comparator-source', required=True, type=Path)
    parser.add_argument('--export-source', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--nanoda', type=Path, help='pinned native binary for production library controls')
    parser.add_argument('--exporter', type=Path, help='pinned native exporter for production library controls')
    parser.add_argument('--cxx', type=Path)
    parser.add_argument('--cxx-headers', type=Path)
    parser.add_argument('--counter-replay', action='store_true',
                        help='also build the fixed-corpus counter identity probe')
    parser.add_argument('--counter-only', action='store_true',
                        help='build only the measurement probe, never production binaries')
    parser.add_argument('--certified-export', action='store_true',
                        help='also build the complete certified-state export tool')
    parser.add_argument('--cxx-abi', choices=('libc++', 'libstdc++'), default='libc++')
    args = parser.parse_args()
    if not args.counter_only and (args.nanoda is None or args.exporter is None):
        parser.error('production builds require --nanoda and --exporter for library controls')
    if args.cxx_abi != 'libc++' and (not args.counter_only or not args.cxx or args.cxx_headers):
        raise ValueError('libstdc++ is an explicit source-SDK counter-only profile')
    if args.counter_only:
        args.counter_replay = True
    prefix = args.lean_prefix.resolve(strict=True)
    out = args.out.resolve()
    if out.exists():
        raise ValueError('exclusive new build directory required')
    out.mkdir(parents=True)
    env = dict(os.environ, PATH=str(prefix / 'bin') + ':/usr/bin:/bin',
               HOME=str(out / 'home'), TMPDIR=str(out / 'tmp'),
               RUSTUP_AUTO_INSTALL='0', CARGO_NET_OFFLINE='true',
               ELAN_HOME=str(out / 'elan-unused'), XDG_CACHE_HOME=str(out / 'cache'),
               MATHLIB_CACHE_DIR=str(out / 'mathlib-cache'), GIT_CONFIG_GLOBAL='/dev/null',
               GIT_CONFIG_NOSYSTEM='1', GIT_TERMINAL_PROMPT='0',
               PYTHONDONTWRITEBYTECODE='1', LEAN_ABORT_ON_PANIC='1')
    for key in ('HOME', 'TMPDIR', 'ELAN_HOME', 'XDG_CACHE_HOME', 'MATHLIB_CACHE_DIR'):
        Path(env[key]).mkdir()
    version = subprocess.check_output([str(prefix / 'bin/lean'), '--version'], env=env, text=True)
    if not re.search(r'\bversion 4\.34\.1(?:,|\s)', version):
        raise ValueError('pinned Lean 4.34.1 required')
    from .generate_fuel_replay import render
    path, expected = render()
    if path.read_text() != expected:
        raise ValueError('generated replay differs from the canonical driver')
    headers = ROOT / 'lean/runtime'
    manifest = json.loads((headers / 'HEADERS.json').read_text())
    for relative, expected_hash in manifest['headers'].items():
        if digest(headers / 'include' / relative) != expected_hash:
            raise ValueError('runtime header pin mismatch')
    if digest(headers / 'LICENSE') != manifest['license_sha256']:
        raise ValueError('runtime license pin mismatch')
    source = out / 'comparator'
    dependency = out / 'lean4export'
    archive(args.comparator_source.resolve(strict=True), COMPARATOR, source, env)
    archive(args.export_source.resolve(strict=True), EXPORT, dependency, env)
    subprocess.run(['git', 'init', '--quiet', str(source)], env=env, check=True)
    patches = list(PATCH_ORDER)
    for patch in patches:
        subprocess.run(['git', 'apply', '--unidiff-zero', str(ROOT / 'data' /
                        ('comparator-d03acab-' + patch + '.patch'))], cwd=source, env=env, check=True)
    if (source / 'Main.lean').read_bytes() != (ROOT / 'lean/ComparatorMain.lean').read_bytes():
        raise ValueError('patched Main differs from committed canonical source')
    for project in (source, dependency):
        (project / 'lean-toolchain').write_text('leanprover/lean4:v4.34.1\n')
    for name in ('Det3Fuel.lean', 'ZkReplay.lean', 'kernel_fuel.cpp', 'CertifiedBase.lean'):
        shutil.copyfile(ROOT / 'lean' / ('GateReplay.lean' if name == 'ZkReplay.lean' else name), source / name)
    if args.counter_replay:
        shutil.copyfile(ROOT / 'lean/CounterReplay.lean', source / 'CounterReplay.lean')
    if args.certified_export:
        shutil.copyfile(ROOT / 'lean/CertifiedExport.lean', source / 'CertifiedExport.lean')
    obj = source / 'kernel_fuel.cpp.o'
    compiler = args.cxx or (Path('/usr/bin/clang++') if platform.system() == 'Darwin'
                            else prefix / 'bin/clang')
    flags = ['-x', 'c++', '-std=c++17', '-O3', '-fPIC', '-DNDEBUG',
             '-DLEAN_MULTI_THREAD', '-DLEAN_EXPORTING', '-DLEAN_BUILD_TYPE="Release"',
             '-I' + str(prefix / 'include'), '-I' + str(headers / 'include')]
    if args.cxx_abi == 'libc++':
        flags += ['-stdlib=libc++']
    if args.cxx_headers:
        flags += ['-nostdinc++', '-isystem', str(args.cxx_headers.resolve(strict=True)),
                  '-isystem', str(prefix / 'include/clang')]
    elif platform.system() == 'Linux' and args.cxx_abi == 'libc++':
        raise ValueError('matching libc++ ABI headers must be provisioned explicitly')
    compile_command = [str(compiler.resolve(strict=True)), *flags, '-c',
                       str(source / 'kernel_fuel.cpp'), '-o', str(obj)]
    subprocess.run(compile_command, env=env, check=True)
    probe_obj = source / 'kernel_fuel_tests.cpp.o'
    shutil.copyfile(ROOT / 'lean/kernel_fuel_tests.cpp', source / 'kernel_fuel_tests.cpp')
    shutil.copyfile(ROOT / 'lean/FuelTests.lean', source / 'FuelTests.lean')
    subprocess.run([str(compiler.resolve(strict=True)), *flags, '-c', str(source / 'kernel_fuel_tests.cpp'), '-o', str(probe_obj)], env=env, check=True)
    config = ('name = "Comparator"\ndefaultTargets = ["comparator", "zk_replay"]\n'
              'moreLinkArgs = [' + json.dumps(str(obj)) + ']\n'
              '[[lean_lib]]\nname = "Comparator"\n'
              '[[lean_lib]]\nname = "Det3Fuel"\n'
              '[[lean_lib]]\nname = "CertifiedBase"\n'
              '[[lean_exe]]\nname = "comparator"\nroot = "Main"\n'
              '[[lean_exe]]\nname = "zk_replay"\nroot = "ZkReplay"\n'
              '[[lean_exe]]\nname = \"gate_fuel_tests\"\nroot = \"FuelTests\"\n'
              'moreLinkArgs = [' + json.dumps(str(probe_obj)) + ']\n'
              '[[require]]\nname = "lean4export"\npath = "../lean4export"\n')
    if args.counter_replay:
        config += '[[lean_exe]]\nname = "counter_replay"\nroot = "CounterReplay"\n'
    if args.certified_export:
        config += '[[lean_exe]]\nname = "certified_export"\nroot = "CertifiedExport"\n'
    (source / 'lakefile.toml').write_text(config)
    (source / 'lake-manifest.json').write_text(json.dumps(dict(
        version='1.2.0', packagesDir='.lake/packages', name='Comparator', lakeDir='.lake',
        fixedToolchain=False, packages=[dict(type='path', scope='', name='lean4export',
        manifestFile='lake-manifest.json', inherited=False, dir='../lean4export',
        configFile='lakefile.toml')]), indent=2) + '\n')
    # Separate targets avoid concurrent full Lean frontend processes.
    targets = (('counter_replay',) if args.counter_only else
               ('comparator', 'zk_replay') + (('counter_replay',) if args.counter_replay else ()))
    if args.certified_export:
        targets += ('certified_export',)
    targets += ('gate_fuel_tests',)
    for target in targets:
        subprocess.run([str(prefix / 'bin/lake'), 'build', target], cwd=source, env=env, check=True)
    test_env = dict(env, LEAN_STACK_SIZE_KB='262144')
    unit = subprocess.run([str(source / '.lake/build/bin/gate_fuel_tests')], env=test_env, capture_output=True, text=True, timeout=180)
    (out / 'FUEL-UNIT-TESTS.json').write_text(json.dumps(dict(exit_code=unit.returncode, stdout=unit.stdout, stderr=unit.stderr), indent=2)+'\n')
    unit.check_returncode()
    if not args.counter_only:
        library = subprocess.run([sys.executable, '-B', str(ROOT / 'native_tests/library_material.py'),
            '--build', str(out), '--prefix', str(prefix), '--exporter', str(args.exporter.resolve(strict=True)),
            '--nanoda', str(args.nanoda.resolve(strict=True)), '--out', str(out / 'library-review')],
            env=test_env, capture_output=True, text=True, timeout=180)
        (out / 'LIBRARY-REVIEW-TESTS.json').write_text(json.dumps(dict(exit_code=library.returncode,
            stdout=library.stdout, stderr=library.stderr), indent=2)+'\n')
        library.check_returncode()
    sources = {name: digest(source / name) for name in
               fuel_source_names(args.counter_replay, args.certified_export)}
    result = dict(lean=version.strip(), comparator_revision=COMPARATOR, export_revision=EXPORT,
                  profile=('counter-identity-only-' + args.cxx_abi if args.counter_only else 'production-libc++'),
                  patches=patches, compile_command=compile_command, sources=sources,
                  runtime_headers=manifest, binaries={name: digest(source / '.lake/build/bin' / name)
                  for name in targets})
    (out / 'FUEL-BUILD.json').write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps(result['binaries'], sort_keys=True))


if __name__ == '__main__':
    main()
