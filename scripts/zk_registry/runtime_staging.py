"""Exclusive, descriptor-relative staging into a real caller-owned runtime root."""
from pathlib import Path
import hashlib
import os
import stat


def stage_binary(root, relative, source, expected_sha256):
    root=Path(root).absolute();name=Path(relative)
    if root.resolve(strict=True)!=root or name.is_absolute() or not name.parts or any(p in ('.','..') for p in name.parts):
        raise ValueError('staging requires a real owned root and a contained relative path')
    directory=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    created=False
    try:
        if os.fstat(directory).st_uid!=os.getuid():raise ValueError('runtime root is not owned')
        for part in name.parts[:-1]:
            try:os.mkdir(part,0o755,dir_fd=directory)
            except FileExistsError:pass
            child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=directory)
            if os.fstat(child).st_uid!=os.getuid():
                os.close(child);raise ValueError('runtime parent is not owned')
            os.close(directory);directory=child
        leaf=name.parts[-1]
        output=os.open(leaf,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o755,dir_fd=directory)
        created=True;digest=hashlib.sha256()
        with os.fdopen(output,'wb') as dst,open(source,'rb') as src:
            if not stat.S_ISREG(os.fstat(src.fileno()).st_mode):raise ValueError('binary source is not regular')
            for block in iter(lambda:src.read(1024*1024),b''):
                digest.update(block);dst.write(block)
            dst.flush();os.fsync(dst.fileno())
        if digest.hexdigest()!=expected_sha256:raise ValueError('staged binary digest mismatch')
        return root/name
    except BaseException:
        if created:os.unlink(name.parts[-1],dir_fd=directory)
        raise
    finally:os.close(directory)
