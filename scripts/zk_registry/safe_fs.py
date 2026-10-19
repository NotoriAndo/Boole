"""No-follow host I/O for files that a previous submission stage could replace."""
from __future__ import annotations

import errno
import os
from pathlib import Path as StandardPath
import shutil
import stat
from contextlib import contextmanager
from contextvars import ContextVar
from functools import wraps

_WRITERS = ContextVar('submission_exposed_writers', default=None)


@contextmanager
def submission_io(exposed):
    """Scope host I/O classification to roots actually exposed by earlier payloads."""
    token = _WRITERS.set(exposure_records(exposed))
    try:
        yield
    finally:
        _WRITERS.reset(token)


def guard_submission_io(function):
    @wraps(function)
    def guarded(*args, **kwargs):
        with submission_io(set()):
            return function(*args, **kwargs)
    return guarded


def guard_trusted_io(function):
    @wraps(function)
    def guarded(*args, **kwargs):
        with submission_io(None):
            return function(*args, **kwargs)
    return guarded


def set_submission_writers(exposed):
    _WRITERS.set(exposure_records(exposed))


def root_identity(path):
    """A root lookup failure belongs to its node-owned parent, never to the submission."""
    value = StandardPath(absolute(path))
    parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for part in value.parts[1:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
            os.close(parent)
            parent = child
        fd = os.open(value.name, getattr(os, 'O_PATH', os.O_RDONLY) | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                     dir_fd=parent)
        try:
            metadata = os.fstat(fd)
            return metadata.st_dev, metadata.st_ino
        finally:
            os.close(fd)
    finally:
        os.close(parent)


def exposure_records(exposed):
    if exposed is None or isinstance(exposed, dict):
        return exposed
    records = {}
    for root in exposed:
        try:
            records[absolute(root)] = root_identity(root)
        except OSError:
            pass
    return records


def restore_root(fd, metadata):
    """The kernel-owned fd path denotes the held inode, never a submission-controlled link."""
    if metadata.st_uid != os.geteuid():
        return
    try:
        mode = stat.S_IMODE(metadata.st_mode) | 0o700
        if hasattr(os, 'O_PATH'):
            os.chmod('/proc/self/fd/' + str(fd), mode)
        else:
            os.fchmod(fd, mode)
    except OSError:
        pass


def open_directory_child(parent, part, path):
    roots = _WRITERS.get() or {}
    expected = roots.get(str(path))
    if expected is None:
        return os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
    # O_PATH bypasses R's own access mode, but still requires search on its parent.
    held = os.open(part, getattr(os, 'O_PATH', os.O_RDONLY) | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                   dir_fd=parent)
    try:
        metadata = os.fstat(held)
        if ((metadata.st_dev, metadata.st_ino) == tuple(expected) and metadata.st_uid == os.geteuid()
                and metadata.st_mode & 0o700 != 0o700):
            restore_root(held, metadata)
            from .lean_runner import BoundaryInvalid
            raise BoundaryInvalid('submission-output-path')
        return os.open('.', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC, dir_fd=held)
    finally:
        os.close(held)


def submission_error(path, error, exposed=None):
    """Only content/path failures in a previously exposed tree belong to the submission."""
    roots = _WRITERS.get() if exposed is None else exposure_records(exposed)
    value = StandardPath(absolute(path))
    if error.errno not in {errno.EACCES, errno.ELOOP, errno.ENOTDIR, errno.EINVAL}:
        return
    if not roots:
        return
    # Restore only an owned directory's access, without following any symlink, for trusted cleanup.
    matched = False
    for root, expected in tuple(roots.items()):
        if value == StandardPath(root) or not value.is_relative_to(StandardPath(root)):
            continue
        parent = None
        try:
            target = StandardPath(absolute(root))
            parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
            for component in target.parts[1:-1]:
                child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                                dir_fd=parent)
                os.close(parent)
                parent = child
            held = os.open(target.name, getattr(os, 'O_PATH', os.O_RDONLY) | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                           dir_fd=parent)
            try:
                metadata = os.fstat(held)
                if (metadata.st_dev, metadata.st_ino) == tuple(expected) and metadata.st_uid == os.geteuid():
                    matched = True
                    restore_root(held, metadata)
            finally:
                os.close(held)
        except OSError:
            pass
        finally:
            if parent is not None:
                os.close(parent)
    if matched:
        from .lean_runner import BoundaryInvalid
        raise BoundaryInvalid('submission-output-path') from None


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
        current = StandardPath('/')
        for part in StandardPath(absolute(path)).parts[1:]:
            current /= part
            if create:
                try:
                    os.mkdir(part, mode, dir_fd=fd)
                except FileExistsError:
                    pass
                except OSError as error:
                    submission_error(current, error)
                    raise
            try:
                child = open_directory_child(fd, part, current)
            except OSError as error:
                submission_error(current, error)
                raise
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
    except OSError as error:
        submission_error(value, error)
        raise
    finally:
        os.close(parent)
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        os.close(fd)
        error = OSError(errno.EINVAL, 'host stage I/O requires a regular file', str(value))
        submission_error(value, error)
        raise error
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
            child = open_directory_child(parent, self.name, StandardPath(absolute(self)))
            os.close(child)
        except OSError as error:
            submission_error(self, error)
            raise
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
        except OSError as error:
            submission_error(self, error)
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
        except OSError as error:
            submission_error(self, error)
            raise
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
    try:
        return shutil.copytree(source, destination, **options)
    except OSError as error:
        if error.filename and os.path.isabs(error.filename):
            submission_error(error.filename, error)
        raise


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
    except OSError as error:
        submission_error(value, error)
        raise
    finally:
        os.close(parent)
