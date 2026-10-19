"""Feed one hash-verified held source FD to the pinned compiler on stdin."""
import hashlib
import os
import stat
import sys
import tempfile


def main():
    source, expected, executable, *arguments = sys.argv[1:]
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    snapshot = tempfile.TemporaryFile()
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise ValueError('source-not-regular')
        digest = hashlib.sha256()
        while block := os.read(fd, 1024 * 1024):
            digest.update(block)
            snapshot.write(block)
        if digest.hexdigest() != expected:
            raise ValueError('source-sha256')
        snapshot.flush()
        snapshot.seek(0)
        os.dup2(snapshot.fileno(), 0)
    finally:
        os.close(fd)
    snapshot.close()
    os.execv(executable, [executable, *arguments])


if __name__ == '__main__':
    main()
