"""Offline publication checks; diagnostics contain file/line, never leaked values."""
from pathlib import Path
import hashlib
import json
import re
import subprocess

FROZEN_HISTORICAL = {
    # Append-only historical record pinned by authority bindings.
    'native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json': '74b9507932b4eda97c89753f642bac579593b034b3e9eff24bb5b056c09079a6',
    # Append-only historical record pinned by authority bindings.
    'native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json': '2ff7a3a30513092495a2d8b67555b4e974ef75af47de08acfe8c049063549126',
}

# Patterns describe path shapes; concrete personal test paths are assembled at runtime.
HOME = re.compile(r'(?<![\w.-])/(?:Users|home)/([^\s/\\\'"<>`():;,=|\[\]{}]+)')
PLACEHOLDERS = frozenset({'example', 'other', 'runner', 'alice', 'operator'})
URL = re.compile(r'\b[A-Za-z][A-Za-z0-9+.-]{0,31}://[^\s<>\'"`]+')
MAC_TEMP = re.compile(r'/var/folders/[A-Za-z0-9_-]{2}/[A-Za-z0-9_-]{20,}/')
CLAUDE = re.compile(r'\.claude/([^\s/\\\'"<>`():;,=\[\]{}]+)')
# Shared system temp is not personal; session scratch roots still fail.
SESSION_TEMP = re.compile(r'/(?:private/)?tmp/claude-')
# A local-docs folder-name mention is not a published file; derived data has provenance checks.


def personal_home(text):
    homes = list(HOME.finditer(text))
    if not homes:
        return False
    urls = [(match.start(), match.end()) for match in URL.finditer(text)]
    return any(match[1] not in PLACEHOLDERS
               and not any(start <= match.start() < end for start, end in urls)
               for match in homes)


def path_leaks(raw, *, relative_path=None):
    if (relative_path in FROZEN_HISTORICAL
            and hashlib.sha256(raw).hexdigest() == FROZEN_HISTORICAL[relative_path]):
        return []
    if b'\0' in raw:  # Only tracked text is a publication path surface.
        return []
    found = []
    for number, line in enumerate(raw.splitlines(), 1):
        text = line.decode('utf-8', errors='replace')
        if (personal_home(text) or MAC_TEMP.search(text) or SESSION_TEMP.search(text)
                or any(match[1] != 'commands' for match in CLAUDE.finditer(text))):
            found.append(number)
    return found


def scan_tracked(root):
    root = Path(root)
    actual = subprocess.check_output(['git', 'rev-parse', '--show-toplevel'], cwd=root, text=True).strip()
    if Path(actual).resolve() != root.resolve():
        raise ValueError('publication scan requires the actual tracked checkout root')
    # Scan every tracked text file except the operator's tasks working notes.
    names = subprocess.check_output(['git', 'ls-files', '-z'], cwd=root).split(b'\0')
    return {name.decode(): lines for name in names if name
            if not name.startswith(b'tasks/')
            if (lines := path_leaks((root / name.decode()).read_bytes(), relative_path=name.decode()))}


def provenance_errors(table, root):
    from .build_fuel_tools import PATCH_ORDER, fuel_source_names
    from .build_nanoda_fuel import MODULES, SOURCE_FILES
    root = Path(root)
    def digest(relative):
        return hashlib.sha256((root / relative).read_bytes()).hexdigest()
    errors = []
    rebuilt = {'Darwin-arm64', 'Linux-aarch64'}
    for profile in sorted(rebuilt - set(table['binary_profiles'])):
        errors.append(profile + ':missing-profile')
    expected_patches = ['comparator-d03acab-' + name + '.patch' for name in PATCH_ORDER]
    driver_sources = {'src/' + destination: canonical for canonical, destination in MODULES.items()}
    for profile, tools in table['binary_profiles'].items():
        for tool in ('comparator', 'nanoda'):
            entry = tools[tool]
            p = entry.get('provenance')
            prefix = profile + ':' + tool + ':'
            if not isinstance(p, dict):
                if profile in rebuilt:
                    errors.append(prefix + 'missing-provenance')
                continue  # Historical unrebuilt x86 artifacts are not attestations.
            if p.get('binary_sha256') != entry['sha256']:
                errors.append(prefix + 'binary')
            if tool == 'nanoda':
                if set(p.get('modules', {})) != set(MODULES):
                    errors.append(prefix + 'module-set')
                if set(p.get('sources', {})) != set(SOURCE_FILES):
                    errors.append(prefix + 'source-set')
                for canonical, destination in MODULES.items():
                    expected = digest('rust/' + canonical)
                    if (p.get('modules', {}).get(canonical) != expected
                            or p.get('sources', {}).get('src/' + destination) != expected):
                        errors.append(prefix + canonical)
                for source in SOURCE_FILES:
                    relative = ('rust/' + driver_sources[source] if source in driver_sources else
                                '../../fixtures/nanoda-public-sources-v1/' + source)
                    if p.get('sources', {}).get(source) != digest(relative):
                        errors.append(prefix + source)
                for field, filename in (('cargo_toml_sha256', 'Cargo.toml'), ('cargo_lock_sha256', 'Cargo.lock')):
                    if p.get(field) != digest('../../fixtures/nanoda-public-sources-v1/' + filename):
                        errors.append(prefix + field)
                for key, filename in (('patch_sha256','nanoda-0f88b5f-fuel.patch'), ('own_patch_sha256','nanoda-0f88b5f-own.patch')):
                    if p.get(key) != digest('data/' + filename):
                        errors.append(prefix + key)
            else:
                if set(p.get('fuel_sources', {})) != set(fuel_source_names(True, True)):
                    errors.append(prefix + 'source-set')
                for source in fuel_source_names(True, True):
                    canonical = {'Main.lean':'ComparatorMain.lean', 'ZkReplay.lean':'GateReplay.lean'}.get(source, source)
                    if p.get('fuel_sources', {}).get(source) != digest('lean/' + canonical):
                        errors.append(prefix + source)
                if p.get('main_sha256') != p.get('fuel_sources', {}).get('Main.lean'):
                    errors.append(prefix + 'main')
                if p.get('patch_order') != expected_patches:
                    errors.append(prefix + 'patch-order')
                if set(p.get('patch_sha256', {})) != set(expected_patches):
                    errors.append(prefix + 'patch-set')
                for filename in expected_patches:
                    if p.get('patch_sha256', {}).get(filename) != digest('data/' + filename):
                        errors.append(prefix + filename)
    return errors
