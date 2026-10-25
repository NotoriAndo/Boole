"""Byte- and case-exact adoption of an immutable tracked source snapshot."""
from contextlib import contextmanager
import builtins
import hashlib
import io
import os
from pathlib import Path, PurePosixPath
from unittest.mock import patch


def case_exact_path(root, relative, tracked):
    """Check lexical spelling before opening; resolve() can hide a case error."""
    parts = PurePosixPath(relative).parts
    if (relative not in tracked or not parts or PurePosixPath(relative).is_absolute()
            or any(part in ('', '.', '..') for part in parts)
            or '/'.join(parts) != relative):
        raise ValueError('case-exact tracked path required: ' + relative)
    path = Path(root)
    for part in parts:
        if part not in os.listdir(path):
            raise ValueError('case-exact directory entry required: ' + relative)
        path = path / part
        if path.is_symlink():
            raise ValueError('case-exact snapshot cannot contain symlinks: ' + relative)
    return path


def verify_snapshot(root, expected, opened_paths=()):
    """Verify tracked spelling and bytes, including the names consumers open."""
    root = Path(root)
    actual = {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()}
    if actual != set(expected):
        raise ValueError('case-exact snapshot file set mismatch')
    for relative, sha256 in expected.items():
        path = case_exact_path(root, relative, expected)
        if hashlib.sha256(path.read_bytes()).hexdigest() != sha256:
            raise ValueError('snapshot content mismatch: ' + relative)
    for relative in opened_paths:
        case_exact_path(root, relative, expected)
    return dict(files=len(expected), opened_paths=sorted(set(opened_paths)))


@contextmanager
def audit_reads(root, tracked):
    """Audit Python consumer reads under the snapshot, preserving lexical case.

    Native consumers must use paths already checked by case_exact_path. Outputs
    belong outside the adopted snapshot; write-only opens are not source reads.
    """
    root = os.path.abspath(root)
    observed = set()
    def guard(original):
        def open_checked(file, mode='r', *args, **kwargs):
            if isinstance(file, (str, bytes, os.PathLike)) and ('r' in mode or '+' in mode):
                absolute = os.path.abspath(os.fsdecode(file))
                if os.path.commonpath((root.casefold(), absolute.casefold())) == root.casefold():
                    if absolute[:len(root)] != root:
                        raise ValueError('case-exact snapshot root required: ' + absolute)
                    relative = os.path.relpath(absolute, root).replace(os.sep, '/')
                    case_exact_path(root, relative, tracked)
                    observed.add(relative)
            return original(file, mode, *args, **kwargs)
        return open_checked
    with patch.object(builtins, 'open', guard(builtins.open)), patch.object(io, 'open', guard(io.open)):
        yield observed
