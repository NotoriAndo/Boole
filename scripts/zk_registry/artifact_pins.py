"""Portable fingerprints of executed toolchain and imported module bytes."""
import hashlib
import json
import os
from pathlib import Path
import re
from .safe_fs import open


def digest(path):
    value = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def tree(root, select):
    """Bind relative names and bytes, including missing/extra modules, in sorted order."""
    root = Path(root)
    if not root.is_dir():
        raise ValueError('artifact root unavailable')
    rows = []
    directories = []
    size = 0
    for directory, children, files in os.walk(root):
        directories.extend(Path(directory, child).relative_to(root).as_posix() for child in children)
        if any(Path(directory, child).is_symlink() for child in children):
            raise ValueError('symlinked artifact directory')
        for name in files:
            if select(name):
                path = Path(directory, name)
                target = path.resolve(strict=True)
                if not target.is_relative_to(root.resolve(strict=True)):
                    raise ValueError('artifact file escapes pinned root')
                rows.append([path.relative_to(root).as_posix(), digest(target)])
                size += target.stat().st_size
    encoded = json.dumps(sorted(rows), ensure_ascii=False, separators=(',', ':')).encode()
    # Empty package directories also affect Lean's first-root lookup rule.
    names = json.dumps(sorted(directories), ensure_ascii=False, separators=(',', ':')).encode()
    return dict(sha256=hashlib.sha256(encoded).hexdigest(), files=len(rows), bytes=size,
                directories_sha256=hashlib.sha256(names).hexdigest(), directories=len(directories))


def snapshot(env):
    # These roots come from trusted provisioning, like process_env's realpath.
    # Keep no-follow checks on every artifact below the canonical roots.
    prefix = Path(env.toolchain).resolve(strict=True)
    roots = [prefix/'lib/lean', *(Path(root).resolve(strict=True) for root in env.lean_path)]
    # Lean imports read both kernel/elaboration artifacts and interpreter IR.
    # Retain .olean.hash as a conservative additional pin, alongside server/private.
    modules = lambda name: name.endswith(('.olean', '.ir', '.ir.sig')) or '.olean.' in name
    native = lambda name: re.search(r'\.(?:so(?:\.\d+)*|dylib|dll)$', name) is not None
    return dict(lean_binary_sha256=digest(prefix/'bin/lean'),
                runtime_libraries=tree(prefix/'lib', native),
                olean_roots=[tree(root, modules) for root in roots])
