"""No-follow host I/O for files that a previous submission stage could replace."""
from __future__ import annotations

import errno
import os
from pathlib import Path as StandardPath
import shutil
import stat


def absolute(path):
    """Normalize syntax, but never resolve a submission-controlled symlink."""
    value = os.path.abspath(os.fspath(path))
    # macOS's OS-owned aliases are not submission directories.
    for alias in ('/tmp', '/var'):
        if os.uname().sysname == 'Darwin' and (value == alias or value.startswith(alias + '/')):
            value = '/private' + value
    return value


def directory_fd(path, create=False, mode=0o777):
    """Walk from an opened root using O_NOFOLLOW on every directory component."""
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for part in StandardPath(absolute(path)).parts[1:]:
            if create:
                try:
                    os.mkdir(part, mode, dir_fd=fd)
                except FileExistsError:
                    pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def open(path, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
    """Open a regular file without following the leaf or any directory symlink."""
    value = StandardPath(absolute(path))
    parent = directory_fd(value.parent)
    flags = os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    flags |= os.O_RDWR if '+' in mode else os.O_RDONLY if 'r' in mode else os.O_WRONLY
    if 'w' in mode:
        flags |= os.O_CREAT | os.O_TRUNC
    if 'x' in mode:
        flags |= os.O_CREAT | os.O_EXCL
    if 'a' in mode:
        flags |= os.O_CREAT | os.O_APPEND
    try:
        fd = os.open(value.name, flags, 0o600, dir_fd=parent)
    finally:
        os.close(parent)
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        os.close(fd)
        raise OSError(errno.EINVAL, 'host stage I/O requires a regular file', str(value))
    return os.fdopen(fd, mode, buffering, encoding, errors, newline)


class Path(type(StandardPath())):
    """A pathlib interface whose host reads and writes always use no-follow descriptors."""

    def open(self, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
        return open(self, mode, buffering, encoding, errors, newline)

    def mkdir(self, mode=0o777, parents=False, exist_ok=False):
        if parents:
            if self.exists() and not exist_ok:
                raise FileExistsError(str(self))
            os.close(directory_fd(self, create=True, mode=mode))
            return
        parent = directory_fd(self.parent)
        try:
            try:
                os.mkdir(self.name, mode, dir_fd=parent)
            except FileExistsError:
                if not exist_ok:
                    raise
            child = os.open(self.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
            os.close(child)
        finally:
            os.close(parent)

    def unlink(self, missing_ok=False):
        parent = directory_fd(self.parent)
        try:
            try:
                os.unlink(self.name, dir_fd=parent)
            except FileNotFoundError:
                if not missing_ok:
                    raise
        finally:
            os.close(parent)

    def chmod(self, mode, *, follow_symlinks=True):
        parent = directory_fd(self.parent)
        try:
            fd = os.open(self.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
            try:
                os.fchmod(fd, mode)
            finally:
                os.close(fd)
        finally:
            os.close(parent)


def copyfile(source, destination):
    """Copy using held no-follow source and destination descriptors."""
    with open(source, 'rb') as reader, open(destination, 'wb') as writer:
        shutil.copyfileobj(reader, writer)
    return destination


def copytree(source, destination, **options):
    """Reject symlinks in the tree and use no-follow file copies and destination traversal."""
    reject_symlinks(source)
    os.close(directory_fd(StandardPath(destination).parent, create=True))
    ignore = options.get('ignore')
    for directory, dirs, files in os.walk(source, followlinks=False):
        ignored = set(ignore(directory, dirs + files)) if ignore else set()
        dirs[:] = [name for name in dirs if name not in ignored]
        for name in [*dirs, *(name for name in files if name not in ignored)]:
            path = os.path.join(directory, name)
            if options.get('symlinks') and os.path.islink(path):
                continue  # Preserve the link itself, never read its target on the host.
            reject_symlinks(path)
    options['copy_function'] = copyfile
    return shutil.copytree(source, destination, **options)


def makedirs(path, mode=0o777, exist_ok=False):
    """Create directories through the same held no-follow traversal."""
    Path(path).mkdir(mode=mode, parents=True, exist_ok=exist_ok)


def reject_symlinks(path):
    """Validate an existing path without permitting any mutable symlink component."""
    value = StandardPath(absolute(path))
    parent = directory_fd(value.parent)
    try:
        metadata = os.stat(value.name, dir_fd=parent, follow_symlinks=False)
        if stat.S_ISLNK(metadata.st_mode):
            raise OSError(errno.ELOOP, 'submission-controlled symlink', str(value))
    finally:
        os.close(parent)
