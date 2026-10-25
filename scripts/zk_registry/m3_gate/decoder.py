"""Private typed transport-decoder evidence, never miner metadata or diagnostics."""
from dataclasses import dataclass
import os
import stat
from . import codec as C, front as F

MAX_BYTES = 8192
FIELDS = ('version', 'context_sha256', 'status', 'cause', 'raw_sha256', 'raw_bytes')
CAUSES = {'ok': frozenset({'none'}),
          'invalid-data': frozenset({'frame', 'checksum', 'digest'}),
          'unavailable': frozenset({'read', 'write', 'resource', 'tool', 'protocol'})}


@dataclass(frozen=True)
class Source:
    """A private node-controlled status file, read only after package authentication."""
    path: str


def read(source, context):
    try:
        if type(source) is bytes:
            raw = source
        elif type(source) is Source:
            with os.fdopen(os.open(source.path, os.O_RDONLY | os.O_NOFOLLOW), 'rb') as stream:
                before = os.fstat(stream.fileno())
                if not stat.S_ISREG(before.st_mode):
                    raise ValueError('decoder status file kind')
                raw = stream.read(MAX_BYTES + 1)
                after = os.fstat(stream.fileno())
                if (before.st_size, before.st_mtime_ns, before.st_ino) != (
                        after.st_size, after.st_mtime_ns, after.st_ino):
                    raise ValueError('decoder status changed')
        else:
            raise ValueError('decoder status source')
        if len(raw) > MAX_BYTES:
            raise ValueError('decoder status cap')
        value = C.loads(raw)
        C.fields(value, FIELDS)
        if (C.canonical(value) != raw or type(value['version']) is not int or value['version'] != 1
                or value['context_sha256'] != C.sha(C.canonical(context))):
            raise ValueError('decoder status binding/version')
        status, cause = value['status'], value['cause']
        if type(status) is not str or type(cause) is not str or cause not in CAUSES.get(status, ()):
            raise ValueError('decoder status/cause')
        if status == 'ok':
            C.digest(value['raw_sha256']); C.uint(value['raw_bytes'], True)
        elif value['raw_sha256'] is not None or value['raw_bytes'] is not None:
            raise ValueError('fabricated decoded output')
    except (OSError, ValueError, TypeError, KeyError, RecursionError):
        raise F.Refusal('node_fault', 'decoder channel unavailable or malformed') from None
    if status != 'ok':
        raise F.Refusal('front_invalid' if status == 'invalid-data' else 'node_fault',
                        'typed transport decoder')
    return value
