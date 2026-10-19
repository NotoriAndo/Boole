"""Reconstruct pinned stick sources without trusting a cache or a port worktree."""
from __future__ import annotations

import hashlib
import gzip
from contextlib import contextmanager
import shutil
import stat
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile
import urllib.request


class IntegrityError(ValueError):
    """The requested frozen reconstruction could not be established."""


def sha256(path):
    digest = hashlib.sha256()
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        os.close(fd)
        raise IntegrityError('expected-regular-file')
    with os.fdopen(fd, 'rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def map_digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def source_files():
    """The rebuild facility owns and hashes its assets independently of package generators."""
    root = Path(__file__).parent
    return sorted(['measuring_sticks.py', 'stick_runtime.py', 'exec_stick_source.py', 'safe_fs.py',
                   'lean/InspectStickTypes.lean', '../setup_measuring_stick.sh'] +
                  [str(path.relative_to(root)) for path in (root / 'data/sticks').rglob('*') if path.is_file()])


def source_info():
    root = Path(__file__).parent
    files = {name: sha256(root / name) for name in source_files()}
    return dict(sources_sha256=map_digest(files), files=files)


def relative(value):
    path = PurePosixPath(value)
    if not value or path.is_absolute() or '..' in path.parts or '\\' in value:
        raise IntegrityError('unsafe-relative-path')
    return Path(*path.parts)


def file_map(value):
    """Decode explicit path/hash rows without changing the frozen canonical map."""
    if isinstance(value, dict):
        result = dict(value)
    elif isinstance(value, list):
        result = {}
        for row in value:
            if not isinstance(row, dict) or set(row) != {'path', 'sha256'}:
                raise IntegrityError('file-row-schema')
            name = row['path']
            if not isinstance(name, str):
                raise IntegrityError('file-row-path')
            if name in result:
                raise IntegrityError('duplicate-file-path: ' + name)
            result[name] = row['sha256']
    else:
        raise IntegrityError('file-table-schema')
    for name, digest in result.items():
        if not isinstance(name, str) or not isinstance(digest, str):
            raise IntegrityError('expected-file-pin')
        relative(name)
        if not re.fullmatch('[a-f0-9]{64}', digest):
            raise IntegrityError('expected-file-pin')
    return result


def checked_file(root, name, expected):
    path = Path(root) / relative(name)
    if path.is_symlink() or not path.is_file() or sha256(path) != expected:
        raise IntegrityError('tracked-sha256: ' + name)
    return path


def validate_recipe(recipe):
    permitted = {'-j1','-j4','--tstack=65536','--tstack=400000','--json','-R','{package_root}',
                 '-o','{olean}','-i','{ilean}','{source}','-DautoImplicit=true','-DautoImplicit=false'}
    if any(argument not in permitted for argument in recipe['arguments']):
        raise IntegrityError('recipe-argument')


def load_descriptor(directory):
    directory = Path(directory)
    descriptor = json.loads((directory / 'descriptor.json').read_text())
    if descriptor.get('schema_version') != 1 or descriptor.get('lean') != 'v4.34.1':
        raise IntegrityError('descriptor-schema')
    if not re.fullmatch(r'[a-zA-Z0-9_-]+', descriptor.get('name', '')):
        raise IntegrityError('descriptor-name')
    names = set()
    for row in descriptor['upstreams']:
        if row['name'] in names or not re.fullmatch(r'[a-zA-Z0-9_-]+', row['name']):
            raise IntegrityError('upstream-name')
        names.add(row['name'])
        if not re.fullmatch('[a-f0-9]{40}', row['commit']) or not re.fullmatch('[a-f0-9]{64}', row['archive_sha256']):
            raise IntegrityError('upstream-pin')
        if not row['archive_url'].startswith('https://') or not row['licence'] or not row['notice']:
            raise IntegrityError('upstream-metadata')
        if not re.fullmatch('[a-f0-9]{64}', row.get('content_sha256', '')):
            raise IntegrityError('upstream-content-pin')
        if row['notice'] not in descriptor.get('tracked_files', {}):
            raise IntegrityError('notice-untracked')
    patches = descriptor['patches']
    if len({x['path'] for x in patches}) != len(patches):
        raise IntegrityError('patch-order-duplicates')
    for row in patches:
        checked_file(directory, row['path'], row['sha256'])
    expected = json.loads(checked_file(directory, descriptor['expected'], descriptor['expected_sha256']).read_text())
    if 'types_file' in expected:
        bundle = checked_file(directory, expected['types_file'], expected['types_file_sha256'])
        with gzip.open(bundle, 'rb') as stream:
            raw = stream.read(5 * 1024**2 + 1)
        if len(raw) > 5 * 1024**2:
            raise IntegrityError('type-list-size')
        expected['types'] = json.loads(raw)
        if map_digest(expected['types']) != expected['types_sha256']:
            raise IntegrityError('type-list-digest')
    for name, digest in descriptor.get('tracked_files', {}).items():
        checked_file(directory, name, digest)
    for name, digest in descriptor.get('base_files', {}).items():
        checked_file(directory.parent, name, digest)
    if 'definition_files' not in expected:
        sources = file_map(expected['source_files'])
        paths = sources if expected['definition_paths'] == 'all' else expected['definition_paths']
        expected['definition_files'] = {name: sources[name] for name in paths}
    for key in ['source_files', 'definition_files']:
        expected[key] = file_map(expected[key])
        for name, digest in expected[key].items():
            relative(name)
            if not re.fullmatch('[a-f0-9]{64}', digest):
                raise IntegrityError('expected-file-pin')
        if map_digest(expected[key]) != expected[key.removesuffix('_files') + '_digest']:
            raise IntegrityError(key.removesuffix('_files') + '-digest')
    if any(expected['source_files'].get(name) != digest for name, digest in expected['definition_files'].items()):
        raise IntegrityError('definition-files')
    if 'port_manifest' in expected:
        expected['port_manifest']['files'] = file_map(expected['port_manifest']['files'])
        if map_digest(expected['port_manifest']) != expected['port_digest']:
            raise IntegrityError('port-manifest-digest')
    if 'companion_artifacts' in expected:
        expected['companion_artifacts'] = file_map(expected['companion_artifacts'])
        permitted = {str(Path('build', *module.split('.'))) + suffix
                     for module in descriptor['build']['roots']
                     for suffix in ['.olean.private', '.olean.server', '.ir', '.ir.sig']}
        if not set(expected['companion_artifacts']).issubset(permitted):
            raise IntegrityError('companion-artifact-path')
    for recipe in descriptor.get('build', {}).get('module_recipes', {}).values():
        validate_recipe(recipe)
    expected['_build_modules'] = descriptor.get('build', {}).get('roots', [])
    return descriptor, expected


@contextmanager
def archive_stream(archive):
    """Stream gzip/xz/tar or zstd without delegating extraction to a system tar."""
    with Path(archive).open('rb') as probe:
        magic = probe.read(4)
    process = None
    if magic == bytes.fromhex('28b52ffd'):
        binary = shutil.which('zstd')
        if not binary:
            raise IntegrityError('zstd-required')
        process = subprocess.Popen([binary, '-dc', str(archive)], stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, env=dict(PATH=os.defpath, LANG='C'))
        source = process.stdout
    else:
        source = Path(archive).open('rb')
    try:
        with tarfile.open(fileobj=source, mode='r|*') as stream:
            yield stream
    except (tarfile.TarError, EOFError) as error:
        raise IntegrityError('archive-format') from error
    finally:
        source.close()
        if process is not None:
            code = process.wait(timeout=30)
            if code and code != -13:
                raise IntegrityError('archive-decompression')


def archive_members(stream):
    root, seen, total = None, set(), 0
    for member in stream:
        raw = PurePosixPath(member.name)
        if raw.is_absolute() or '..' in raw.parts or not raw.parts:
            raise IntegrityError('unsafe-archive-member')
        if not (member.isdir() or member.isfile() or member.issym()):
            raise IntegrityError('unsafe-archive-member')
        root = root or raw.parts[0]
        if raw.parts[0] != root:
            raise IntegrityError('archive-root')
        if len(raw.parts) == 1:
            if not member.isdir(): raise IntegrityError('archive-root-file')
            continue
        name = str(PurePosixPath(*raw.parts[1:]))
        if name in seen: raise IntegrityError('duplicate-archive-member')
        seen.add(name); total += member.size
        if len(seen) > 200000 or total > 8 * 1024**3 or member.size > 512 * 1024**2:
            raise IntegrityError('archive-size')
        yield name, member


def archive_content_inventory(archive):
    """Path-independent file bytes and link targets; archive metadata/modes are not identity."""
    entries = {}
    with archive_stream(archive) as stream:
        for name, member in archive_members(stream):
            if member.isfile():
                h = hashlib.sha256()
                with stream.extractfile(member) as source:
                    for block in iter(lambda: source.read(1024 * 1024), b''): h.update(block)
                entries[name] = dict(file_sha256=h.hexdigest(), executable=bool(member.mode & 0o111))
            elif member.issym():
                entries[name] = dict(symlink=member.linkname)
    return entries


def archive_content_digest(archive):
    return map_digest(archive_content_inventory(archive))


def extract_archive(archive, destination, *, allow_internal_links=False):
    """Write regular members first, links last, then validate the complete link graph."""
    destination = Path(destination).absolute()
    if destination.is_symlink():
        raise IntegrityError('unsafe-archive-destination')
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    links = []
    def parent_for(target):
        current = destination
        for part in target.relative_to(destination).parts[:-1]:
            current /= part
            if current.is_symlink(): raise IntegrityError('archive-linked-parent')
            current.mkdir(exist_ok=True)
        if not target.parent.resolve().is_relative_to(destination):
            raise IntegrityError('archive-parent-escape')
    with archive_stream(archive) as stream:
        for name, member in archive_members(stream):
            target = destination / relative(name)
            parent_for(target)
            if member.issym():
                if not allow_internal_links or Path(member.linkname).is_absolute():
                    raise IntegrityError('unsafe-archive-link')
                links.append((target, member.linkname))
            elif member.isdir():
                if target.is_symlink(): raise IntegrityError('archive-linked-parent')
                target.mkdir(exist_ok=True)
            else:
                with stream.extractfile(member) as source, target.open('xb') as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                target.chmod(member.mode & 0o755)
    for target, name in links:
        parent_for(target)
        if target.exists() or target.is_symlink(): raise IntegrityError('archive-link-conflict')
        target.symlink_to(name)
    for target, _ in links:
        try:
            if not target.resolve().is_relative_to(destination): raise IntegrityError('unsafe-archive-link')
        except (OSError, RuntimeError):
            raise IntegrityError('unsafe-archive-link') from None


def fetch_archive(row, cache, offline):
    cache = Path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    target = cache / (row['archive_sha256'] + '.tar.gz')
    if not target.exists():
        if offline:
            raise IntegrityError('archive-cache-miss: ' + row['name'])
        with tempfile.NamedTemporaryFile(dir=cache, delete=False) as output:
            partial = Path(output.name)
            try:
                with urllib.request.urlopen(row['archive_url'], timeout=120) as source:
                    while block := source.read(1024 * 1024):
                        output.write(block)
                output.close()
                if sha256(partial) != row['archive_sha256'] and (
                        not row.get('content_sha256') or archive_content_digest(partial) != row['content_sha256']):
                    raise IntegrityError('archive-sha256: ' + row['name'])
                partial.rename(target)
            finally:
                partial.unlink(missing_ok=True)
    if target.is_symlink():
        raise IntegrityError('archive-sha256: ' + row['name'])
    if row.get('content_sha256'):
        try:
            content = archive_content_digest(target)
        except IntegrityError:
            raise IntegrityError('archive-sha256: ' + row['name']) from None
        if content != row['content_sha256']:
            raise IntegrityError('archive-content-sha256: ' + row['name'])
    elif sha256(target) != row['archive_sha256']:
        raise IntegrityError('archive-sha256: ' + row['name'])
    return target


def verify_sources(root, expected):
    actual = {}
    for name, digest in expected['source_files'].items():
        path = Path(root) / relative(name)
        if path.is_symlink() or not path.is_file() or sha256(path) != digest:
            raise IntegrityError('source-sha256: ' + name)
        actual[name] = digest
    if map_digest(actual) != expected['source_digest']:
        raise IntegrityError('source-digest')
    definitions = expected['definition_files']
    if any(actual.get(name) != digest for name, digest in definitions.items()):
        raise IntegrityError('definition-files')
    if map_digest(definitions) != expected['definition_digest']:
        raise IntegrityError('definition-digest')
    return actual


def validate_patch(text, packages):
    """Distinguish unified-diff headers from removed Lean comments that begin with '--'."""
    lines = text.splitlines(); index = 0; old = new = 0; header = False
    while index < len(lines):
        line = lines[index]; index += 1
        if line.startswith('\\ No newline at end of file'): continue
        if old or new:
            if not line or line[0] not in ' +-': raise IntegrityError('patch-hunk')
            if line[0] in ' -': old -= 1
            if line[0] in ' +': new -= 1
            if old < 0 or new < 0: raise IntegrityError('patch-hunk')
            continue
        if line.startswith('--- '):
            if index >= len(lines) or not lines[index].startswith('+++ '): raise IntegrityError('patch-header')
            for target in [line[4:].split('\t',1)[0], lines[index][4:].split('\t',1)[0]]:
                if target != '/dev/null':
                    parts = relative(target).parts
                    if len(parts) < 3 or parts[0] not in {'a','b'} or parts[1] not in packages:
                        raise IntegrityError('patch-target')
            index += 1; header = True; continue
        match = re.fullmatch(r'@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@.*', line)
        if match and header:
            old = int(match[2]) if match[2] is not None else 1
            new = int(match[4]) if match[4] is not None else 1
            continue
        raise IntegrityError('patch-format')
    if old or new: raise IntegrityError('patch-truncated')


def materialize_sources(directory, destination, cache, *, offline=False):
    directory, destination = Path(directory), Path(destination)
    descriptor, expected = load_descriptor(directory)
    if destination.exists():
        raise IntegrityError('destination-exists')
    archives = [(row, fetch_archive(row, cache, offline)) for row in descriptor['upstreams']]
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.stick-', dir=destination.parent) as temp:
        stage = Path(temp) / 'sources'
        stage.mkdir()
        for row, archive in archives:
            extract_archive(archive, stage / row['name'])
        for rule in descriptor.get('rewrites', []):
            if (rule['token'], rule['replacement']) not in {('native_decide', 'decide'), ('assert', 'sailAssert')}:
                raise IntegrityError('rewrite-rule')
            manifest = checked_file(directory, rule['files'], rule['sha256'])
            for name in json.loads(manifest.read_text()):
                path = stage / relative(name)
                if path.is_symlink() or not path.is_file():
                    raise IntegrityError('rewrite-source')
                path.write_text(re.sub(r'\b' + rule['token'] + r'\b', rule['replacement'], path.read_text()))
        for row in descriptor['patches']:
            patch = directory / row['path']
            validate_patch(patch.read_text(), {x['name'] for x in descriptor['upstreams']})
            result = subprocess.run(['/usr/bin/patch', '--batch', '--forward', '--fuzz=0',
                                     '--no-backup-if-mismatch', '-p1', '-i', str(patch.resolve())],
                                    cwd=stage, capture_output=True, text=True, timeout=120,
                                    env=dict(PATH=os.defpath, LANG='C', LC_ALL='C'))
            if result.returncode:
                raise IntegrityError('patch-apply: ' + row['path'])
        verify_sources(stage, expected)
        stage.rename(destination)
    return descriptor, expected


def verify_loadable_inventory(directory, root, permitted, kind):
    def unreadable(error):
        raise IntegrityError(kind + '-import-inventory-read: ' + str(error.filename)) from error

    if not root.is_dir():
        raise IntegrityError(kind + '-import-root: ' + str(root))
    for parent, folders, files in os.walk(root, followlinks=False, onerror=unreadable):
        for name in folders:
            path = Path(parent) / name
            if path.is_symlink():
                raise IntegrityError('unexpected-' + kind + '-link: ' + str(path.relative_to(directory)))
        for name in files:
            if not name.endswith(('.olean', '.olean.server', '.olean.private', '.ir', '.ir.sig')):
                continue
            path = Path(parent) / name
            if path.is_symlink() or str(path.relative_to(directory)) not in permitted:
                raise IntegrityError('unexpected-' + kind + '-artifact: ' + str(path.relative_to(directory)))


def observed_inventory(directory, root, *, loadable_only):
    directory, root = Path(directory), Path(root)
    files, links = {}, {}
    def unreadable(error): raise IntegrityError('inventory-read: ' + str(error.filename))
    if root.is_symlink() or not root.is_dir(): raise IntegrityError('inventory-root')
    for parent, folders, names in os.walk(root, followlinks=False, onerror=unreadable):
        for name in [*folders, *names]:
            path = Path(parent) / name
            if path.is_symlink():
                if loadable_only or name in folders: raise IntegrityError('inventory-link')
                links[str(path.relative_to(directory))] = os.readlink(path)
        for name in names:
            path = Path(parent) / name
            if path.is_symlink(): continue
            if loadable_only and not name.endswith(('.olean','.olean.private','.olean.server','.ir','.ir.sig')): continue
            files[str(path.relative_to(directory))] = sha256(path)
    return files, links


def compare_inventory(actual, expected, label):
    if len(actual) != expected['count'] or map_digest(actual) != expected['sha256']:
        with tempfile.NamedTemporaryFile(mode='w', prefix='stick-observed-', suffix='.json', delete=False) as output:
            json.dump(dict(label=label, files=actual), output, sort_keys=True)
            receipt = output.name
        raise IntegrityError('inventory-digest: ' + label + '; observed map: ' + receipt)


def verify_import_inventory(directory, expected, profile):
    """Check every loadable and deny extras, including unreadable and linked subtrees."""
    directory = Path(directory)
    if 'library_inventories' in expected:
        roots = {'.lake/packages/' + name + '/.lake/build/lib/lean' for name in expected['packages']}
        if set(expected['library_inventories']) != roots:
            raise IntegrityError('library-inventory-roots')
        for root, pin in expected['library_inventories'].items():
            actual, _ = observed_inventory(directory, directory / relative(root), loadable_only=True)
            tool = expected.get('bootstrap_tool_inventory', {})
            if root == tool.get('root') and not os.path.lexists(directory / root / tool['subdirectory']):
                pin = tool['without_tools']
            compare_inventory(actual, pin, root)
        actual, links = observed_inventory(directory, directory / 'lean-v4.34.1', loadable_only=False)
        compare_inventory(actual, expected['toolchain_profiles'][profile], profile)
        if links != expected['toolchain_symlinks'][profile]: raise IntegrityError('toolchain-links')
        return
    permitted = set(file_map(expected['library_files'])) | set(file_map(expected['toolchain_profiles'][profile]))
    roots = [directory / 'lean-v4.34.1/lib/lean'] + [
        directory / '.lake/packages' / name / '.lake/build/lib/lean' for name in expected['packages']]
    for root in roots:
        verify_loadable_inventory(directory, root, permitted, 'base')


def verify_build_inventory(destination, expected):
    destination = Path(destination)
    build = destination / 'build'
    if build.is_symlink():
        raise IntegrityError('unexpected-build-link: build')
    permitted = {name for name in expected.get('port_manifest', {}).get('files', {}) if name.startswith('build/')}
    if 'port_manifest' not in expected:
        permitted.update(str(Path('build', *module.split('.'))) + suffix for module in expected.get('_build_modules', []) for suffix in ['.olean','.olean.private','.olean.server','.ir','.ir.sig'])
    # This generated import-only module is overwritten before every type observation.
    permitted.update(expected.get('companion_artifacts', {}))
    permitted.add('build/StickTypeImports.olean')
    verify_loadable_inventory(destination, build, permitted, 'build')


def frozen_artifacts(expected, module=None):
    files = {name: digest for name, digest in expected.get('port_manifest', {}).get('files', {}).items()
             if name.startswith('build/')}
    files.update(expected.get('companion_artifacts', {}))
    if module is not None:
        stem = str(Path('build', *module.split('.')))
        files = {name: digest for name, digest in files.items()
                 if name in {stem + suffix for suffix in ['.olean', '.olean.private', '.olean.server', '.ir', '.ir.sig']}}
    return files


def verify_frozen_artifacts(destination, expected, module=None):
    for name, digest in frozen_artifacts(expected, module).items():
        checked_file(destination, name, digest)


def verify_base(directory, metadata):
    """A read-only cache is accepted only by the tracked content manifest."""
    import platform
    directory = Path(directory).resolve()
    profile = platform.system() + '-' + platform.machine()
    expected = json.loads((Path(metadata) / 'expected.json').read_text())
    if profile not in expected['toolchain_profiles']:
        raise IntegrityError('unsupported-toolchain-profile: ' + profile)
    if 'library_files' in expected:
        for name, digest in {**file_map(expected['library_files']), **file_map(expected['toolchain_profiles'][profile])}.items():
            checked_file(directory, name, digest)
        for name, target in expected.get('toolchain_symlinks', {}).get(profile, {}).items():
            path = directory / relative(name)
            if not path.is_symlink() or os.readlink(path) != target:
                raise IntegrityError('toolchain-link: ' + name)
    verify_import_inventory(directory, expected, profile)
    for name in ['lake-manifest.json', 'lakefile.toml', 'lean-toolchain']:
        if (directory / name).read_bytes() != (Path(metadata) / name).read_bytes():
            raise IntegrityError('base-project: ' + name)
    return expected


def isolated_environment(base, destination, packages):
    base, destination = Path(base), Path(destination)
    home = destination / '.runtime'
    for name in ['home', 'tmp', 'cache', 'elan', 'lake']:
        (home / name).mkdir(parents=True, exist_ok=True)
    paths = [destination / 'build'] + [base / '.lake/packages' / name / '.lake/build/lib/lean' for name in packages]
    return dict(PATH=str(base / 'lean-v4.34.1/bin') + ':/usr/bin:/bin:/usr/sbin:/sbin',
                LEAN_SYSROOT=str(base / 'lean-v4.34.1'), LEAN_PATH=os.pathsep.join(map(str, paths)),
                HOME=str(home / 'home'), TMPDIR=str(home / 'tmp'), XDG_CACHE_HOME=str(home / 'cache'),
                MATHLIB_CACHE_DIR=str(home / 'cache/mathlib'), ELAN_HOME=str(home / 'elan'),
                LAKE_HOME=str(home / 'lake'), PYTHONDONTWRITEBYTECODE='1', LANG='C.UTF-8',
                GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null', GIT_TERMINAL_PROMPT='0')


def source_for(root, packages, module):
    suffix = Path(*module.split('.')).with_suffix('.lean')
    matches = [Path(root) / package / suffix for package in packages if (Path(root) / package / suffix).is_file()]
    if len(matches) != 1:
        raise IntegrityError('module-source: ' + module)
    return matches[0]


class BuildEnvironment(dict):
    """Carry current-run artifact bindings in memory, never trust a public resume receipt."""
    pass


def check_session(destination, environment):
    artifacts = getattr(environment, 'artifacts', None)
    if artifacts is None: raise IntegrityError('unverified-build-session')
    verify_loadable_inventory(Path(destination), Path(destination) / 'build', set(artifacts), 'session')
    for name, pin in artifacts.items(): checked_file(destination, name, pin)


def compile_stick(directory, destination, base, run, *, local_profile=None):
    """Public builds start empty; only an explicitly supplied private profile reuses frozen outputs."""
    descriptor, expected = load_descriptor(directory)
    destination = Path(destination).resolve()
    verify_sources(destination, expected)
    if local_profile is not None:
        profile = json.loads(Path(local_profile).read_text())
        identity = {k: expected[k] for k in ['source_digest','definition_digest']}
        if (profile['name'] != descriptor['name'] or profile['public_identity'] != identity
                or profile['types_sha256'] != map_digest(expected['types'])):
            raise IntegrityError('local-profile-identity')
        descriptor['build']['module_recipes'] = profile['module_recipes']
        expected.update(profile['expected'])
        expected['port_manifest']['files'] = file_map(expected['port_manifest']['files'])
        if map_digest(expected['port_manifest']) != expected['port_digest']:
            raise IntegrityError('local-profile-port-digest')
        expected['companion_artifacts'] = file_map(expected.get('companion_artifacts', {}))
    foundation = verify_base(base, Path(directory).parent / 'base')
    environment = BuildEnvironment(isolated_environment(base, destination, foundation['packages']))
    environment.artifacts = {}
    lean = str(Path(base).resolve() / 'lean-v4.34.1/bin/lean')
    build = destination / 'build'
    if build.is_symlink(): raise IntegrityError('unexpected-build-link')
    if local_profile is None and build.exists(): shutil.rmtree(build)
    build.mkdir(exist_ok=True)
    for suffix in ['.olean','.olean.private','.olean.server','.ir','.ir.sig']:
        (build / ('StickTypeImports' + suffix)).unlink(missing_ok=True)
    verify_build_inventory(destination, expected)
    completed = {}; progress = destination / 'BUILD-PROGRESS.jsonl'
    if local_profile is not None and progress.exists():
        for line in progress.read_text().splitlines():
            row = json.loads(line); completed[row['module']] = row
    for module in descriptor['build']['roots']:
        if local_profile is None: check_session(destination, environment)
        source = source_for(destination, descriptor['build']['source_packages'], module)
        source_hash = expected['source_files'][str(source.relative_to(destination))]
        recipe = descriptor['build']['module_recipes'][module]
        validate_recipe(recipe)
        feeder = Path(__file__).with_name('exec_stick_source.py')
        recipe_hash = map_digest(dict(recipe=recipe, source_feeder_sha256=sha256(feeder),
                                     transport='Lean4.34.1-stdin-default-setup-v1'))
        output = build / Path(*module.split('.')).with_suffix('.olean')
        frozen_hash = expected.get('port_manifest', {}).get('files', {}).get(str(output.relative_to(destination)))
        previous = completed.get(module)
        if (local_profile is not None and previous and previous['source_sha256'] == source_hash
                and previous.get('recipe_sha256') == recipe_hash and output.is_file()
                and sha256(output) == frozen_hash and previous['outputs'] and all(
                    (destination / name).is_file() and sha256(destination / name) == pin
                    for name, pin in {**previous['outputs'], **frozen_artifacts(expected,module)}.items())):
            environment.artifacts.update(frozen_artifacts(expected,module)); continue
        output.parent.mkdir(parents=True, exist_ok=True)
        package = next(name for name in descriptor['build']['source_packages'] if source.is_relative_to(destination / name))
        bindings = dict(package_root=str(destination / package), source=recipe['source_label'], olean=str(output),
                        ilean=str(output.with_suffix('.ilean')))
        arguments = [argument.format(**bindings) for argument in recipe['arguments']]
        cwd = destination / relative(recipe['cwd'])
        setup = destination / '.runtime/setup' / (module + '.json'); setup.parent.mkdir(parents=True, exist_ok=True)
        setup.write_text(json.dumps(dict(name=recipe['module_name'],isModule=False,importArts={},dynlibs=[],plugins=[],options={}))+'\n')
        import sys
        run([sys.executable, '-I', str(feeder), str(source), source_hash, lean, '--stdin', '--setup', str(setup),
             *arguments], environment, str(cwd), module.replace('.', '-'))
        verify_build_inventory(destination, expected)
        if frozen_hash is not None and sha256(output) != frozen_hash:
            raise IntegrityError('compiled-sha256: ' + module)
        verify_frozen_artifacts(destination, expected, module)
        files = [output.with_suffix(suffix) for suffix in ['.olean','.olean.private','.olean.server','.ir','.ir.sig']]
        outputs = {str(p.relative_to(destination)): sha256(p) for p in files if p.is_file()}
        environment.artifacts.update(outputs)
        if local_profile is None: check_session(destination, environment)
        with progress.open('a') as stream:
            stream.write(json.dumps(dict(module=module,source_sha256=source_hash,recipe_sha256=recipe_hash,
                                         outputs=outputs,public_fresh=local_profile is None),sort_keys=True)+'\n')
    verify_sources(destination, expected); verify_frozen_artifacts(destination, expected)
    check_session(destination, environment)
    if local_profile is not None:
        manifest = dict(expected['port_manifest']); actual = {name:sha256(destination/name) for name in manifest['files']}
        changed = [name for name,pin in manifest['files'].items() if actual[name]!=pin];manifest['files']=actual
        result=dict(expected=expected['port_digest'],actual=map_digest(manifest),mismatches=changed)
        (destination/'PORT-COMPARISON.json').write_text(json.dumps(result,indent=2)+'\n')
        if changed or result['actual']!=result['expected']: raise IntegrityError('port-digest')
    return environment


def verify_types(directory, destination, base, environment, run):
    descriptor, expected = load_descriptor(directory)
    destination = Path(destination).resolve()
    verify_build_inventory(destination, expected)
    check_session(destination, environment)
    source = destination / 'StickTypeImports.lean'
    source.write_text(''.join('import ' + name + '\n' for name in descriptor['build']['roots']))
    lean = Path(base).resolve() / 'lean-v4.34.1/bin/lean'
    run([lean, '-j1', '-o', destination / 'build/StickTypeImports.olean', source],
        environment, destination, 'type-imports')
    verify_build_inventory(destination, expected)
    names = destination / 'TYPE-NAMES.txt'
    names.write_text(''.join(row['name'] + '\n' for row in expected['types']))
    for suffix in ['.olean','.olean.private','.olean.server','.ir','.ir.sig']:
        path = destination / 'build' / ('StickTypeImports' + suffix)
        if path.is_file(): environment.artifacts[str(path.relative_to(destination))] = sha256(path)
    check_session(destination, environment)
    observed = destination / 'OBSERVED-TYPES.json'
    observer = Path(__file__).parent / 'lean/InspectStickTypes.lean'
    run([lean, '--run', observer, 'StickTypeImports', names, observed],
        environment, destination, 'type-observer')
    rows = json.loads(observed.read_text())
    wanted = {row['name']: (row.get('current_name', row['name']),row['canonical_type']) for row in expected['types']}
    actual = {row['name']: (row.get('current_name', row['name']),row['canonical_type']) for row in rows['rows']}
    mismatches = [name for name, value in wanted.items() if actual.get(name) != value]
    if rows['missing'] or actual != wanted or len(rows['rows']) != len(wanted):
        raise IntegrityError('canonical-type-comparison: ' + ', '.join(mismatches[:3]))
    result = dict(count=len(wanted), identical=True, observer_sha256=sha256(observer),
                  observed_sha256=sha256(observed), compiler_name_renames=rows['compiler_name_renames'])
    (destination / 'TYPE-COMPARISON.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def prepare_bootstrap_libraries(destination, expected):
    """A public cache may omit a search directory whose pinned inventory is empty.

    Cache-tool outputs are quarantined outside every Lean search root.
    This prepares owned bootstrap directories; read-only supplied bases still go
    directly through strict inventory verification. No link component is followed.
    """
    from . import safe_fs as FS
    roots = {'.lake/packages/' + name + '/.lake/build/lib/lean' for name in expected['packages']}
    if set(expected.get('library_inventories', {})) != roots:
        raise IntegrityError('library-inventory-roots')
    tool = expected.get('bootstrap_tool_inventory')
    if tool:
        if tool['root'] not in roots or tool['subdirectory'] != 'Cache':
            raise IntegrityError('bootstrap-tool-root')
        source = Path(destination) / relative(tool['root']) / 'Cache'
        if os.path.lexists(source):
            source_parent = FS.directory_fd(source.parent)
            target_parent = None
            try:
                info = os.stat(source.name, dir_fd=source_parent, follow_symlinks=False)
                if not stat.S_ISDIR(info.st_mode):
                    raise IntegrityError('bootstrap-tool-link')
                target_parent = FS.directory_fd(Path(destination) / '.bootstrap-tools/mathlib', create=True)
                try:
                    os.stat('Cache', dir_fd=target_parent, follow_symlinks=False)
                except FileNotFoundError:
                    pass
                else:
                    raise IntegrityError('bootstrap-tool-quarantine-exists')
                os.rename('Cache', 'Cache', src_dir_fd=source_parent, dst_dir_fd=target_parent)
            finally:
                os.close(source_parent)
                if target_parent is not None: os.close(target_parent)
    empty = dict(count=0, sha256=map_digest({}))
    for root, pin in expected.get('library_inventories', {}).items():
        if pin == empty:
            FS.makedirs(str(Path(destination) / relative(root)), exist_ok=True)


def bootstrap_base(metadata, destination, cache, run):
    """Build a fresh base from pinned public sources and verify the fetched cache."""
    import platform
    import shutil
    metadata, destination, cache = Path(metadata), Path(destination), Path(cache)
    expected = json.loads((metadata / 'expected.json').read_text())
    profile = platform.system() + '-' + platform.machine()
    if profile not in expected['compiler_archives']:
        raise IntegrityError('unsupported-toolchain-profile: ' + profile)
    if destination.exists():
        prepare_bootstrap_libraries(destination, expected)
        verify_base(destination, metadata)
        return destination
    destination.mkdir(parents=True)
    compiler = expected['compiler_archives'][profile]
    row = dict(name='Lean', archive_sha256=compiler['sha256'], archive_url=compiler['url'], content_sha256=compiler['content_sha256'])
    archive = fetch_archive(row, cache, False)
    toolchain = destination / 'lean-v4.34.1'
    toolchain.mkdir()
    extract_archive(archive, toolchain, allow_internal_links=True)
    for name in ['lake-manifest.json', 'lakefile.toml', 'lean-toolchain']:
        shutil.copyfile(metadata / name, destination / name)
    environment = isolated_environment(destination, destination, expected['packages'])
    environment.pop('LEAN_PATH')
    run([toolchain / 'bin/lake', 'exe', 'cache', 'get'], environment, destination, 'bootstrap-cache')
    prepare_bootstrap_libraries(destination, expected)
    verify_base(destination, metadata)
    return destination


def main(argv=None):
    import argparse
    import time
    from .stick_runtime import guarded_runner
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('name', nargs='?', choices=['evm', 'riscv-v2'])
    parser.add_argument('destination', nargs='?', type=Path)
    parser.add_argument('--cache', type=Path)
    parser.add_argument('--base-env', type=Path, help='read-only, content-verified base cache')
    parser.add_argument('--offline', action='store_true')
    parser.add_argument('--source-only', action='store_true', help='stop with SOURCE-VERIFIED, never a build PASS')
    parser.add_argument('--resume', action='store_true')
    parser.add_argument('--check-descriptors', action='store_true', help='lightweight CI; no build or network')
    parser.add_argument('--resource-root', type=Path)
    parser.add_argument('--deadline-epoch', type=float)
    parser.add_argument('--local-profile', type=Path, help='Optional private artifact/recipe binding; never public identity')
    args = parser.parse_args(argv)
    data = Path(__file__).parent / 'data/sticks'
    if args.check_descriptors:
        for name in ['evm', 'riscv-v2']:
            descriptor, expected = load_descriptor(data / name)
            if len(expected['types']) != {'evm':2145,'riscv-v2':72}[name]:
                raise IntegrityError('canonical-type-count')
            if len({row['name'] for row in expected['types']}) != len(expected['types']):
                raise IntegrityError('duplicate-canonical-type')
            if not descriptor['build']['roots'] or not descriptor['base_files']:
                raise IntegrityError('build-schema')
            if set(descriptor['build']['roots']) != set(descriptor['build']['module_recipes']):
                raise IntegrityError('module-recipes')
        print('Measuring-stick descriptors and ordered patch digests: PASS')
        return 0
    if not args.name or not args.destination:
        parser.error('name and destination are required')
    destination = args.destination.resolve()
    cache = (args.cache or destination.parent / '.stick-cache').resolve()
    descriptor, expected = load_descriptor(data / args.name)
    if args.resume:
        verify_sources(destination, expected)
    else:
        materialize_sources(data / args.name, destination, cache, offline=args.offline)
    if args.source_only:
        print('SOURCE-VERIFIED; compilation and canonical types have not been checked')
        return 0
    deadline = args.deadline_epoch or time.time() + 8 * 3600
    run = guarded_runner(destination / 'logs', args.resource_root or destination.parent / '.stick-resources', deadline)
    base = args.base_env.resolve() if args.base_env else destination.parent / '.stick-base-v4.34.1'
    if not args.base_env:
        if args.offline:
            raise IntegrityError('offline-build-requires-verified-base-env')
        bootstrap_base(data / 'base', base, cache, run)
    environment = compile_stick(data / args.name, destination, base, run, local_profile=args.local_profile)
    types = verify_types(data / args.name, destination, base, environment, run)
    record = dict(outcome='PASS',name=args.name,source_digest=expected['source_digest'],
                  definition_digest=expected['definition_digest'],canonical_type_list_sha256=expected['types_sha256'],
                  port_digest=json.loads((destination/'PORT-COMPARISON.json').read_text())['actual'] if args.local_profile else None,
                  canonical_types=types,descriptor_sha256=sha256(data / args.name / 'descriptor.json'),
                  base_expected_sha256=sha256(data / 'base/expected.json'),
                  builder_sources_sha256=source_info()['sources_sha256'],
                  foundation='content-verified read-only cache' if args.base_env else 'fresh public bootstrap',
                  environment=environment,assurance='build and type comparison; proof admission is separate')
    (destination / 'BUILD-RESULT.json').write_text(json.dumps(record,indent=2)+'\n')
    print(json.dumps({key:record[key] for key in ['outcome','name','definition_digest','port_digest']}))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (IntegrityError, OSError, ValueError, RuntimeError) as error:
        import sys
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
