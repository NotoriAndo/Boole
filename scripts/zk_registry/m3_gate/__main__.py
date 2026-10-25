"""Explicit Mac development invocation; authenticated inputs, one final channel."""
import argparse
from pathlib import Path
import sys
from . import adapter as A, codec as C


def read(path, cap, *, overflow=False):
    with Path(path).open('rb') as stream:
        raw = stream.read(cap+1)
    if len(raw) > cap and not overflow:
        raise ValueError('input exceeds framing cap')
    return raw


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('descriptor', 'gate-digest', 'bundle', 'package', 'inventory', 'context',
                 'reference', 'tools-root', 'counter-driver', 'work'):
        parser.add_argument('--'+name, required=True)
    parser.add_argument('--certified-root')
    parser.add_argument('--phase', choices=('first', 'retry', 'replay'), default='first')
    parser.add_argument('--public-counter', action='store_true')
    args = parser.parse_args()
    try:
        raw_descriptor = read(args.descriptor, 65536)
        descriptor = C.decode(raw_descriptor, args.gate_digest)
        context = C.loads(read(args.context, 65536))
        result = A.run(read(args.bundle, 134217728, overflow=True), read(args.package, 1048576),
            read(args.inventory, 67108864), context, raw_descriptor, args.gate_digest,
            args.reference, A.D.Tools.load(args.tools_root), args.counter_driver, args.work,
            certified_root=args.certified_root, phase=args.phase, public_counter=args.public_counter)
        encoded = C.canonical(result)
        A.decode_reply(encoded, descriptor, context)
        print(A.MARKER+encoded.decode())
        return 0
    except (OSError, ValueError, TypeError, KeyError, MemoryError, RecursionError):
        # The enclosing supervisor treats absence of an authenticated final reply
        # as unavailable. Never invent a digest for an undecodable descriptor.
        print('gate_m3 request/release unavailable', file=sys.stderr)
        return 70


if __name__ == '__main__':
    sys.exit(main())
