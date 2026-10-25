"""Gate-owned Lean compile driver. stdout stays Lean JSON; exit carries the outcome."""
from pathlib import Path
import re
import subprocess
import sys

# Isolated Python deliberately ignores PYTHONPATH; this directory is gate-owned.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from bounded_capture import MessageSink
from tool_exit import Exit


def main(argv):
    if not argv or '--json' not in argv:
        return Exit.UNAVAILABLE
    sink = MessageSink()
    try:
        with subprocess.Popen(argv, stdout=subprocess.PIPE) as child:
            while data := child.stdout.read1(65536):
                sink.feed(data)
                sys.stdout.buffer.write(data)
                sys.stdout.buffer.flush()
            sink.finish()
            rc = child.wait()
    except (OSError, ValueError):
        return Exit.UNAVAILABLE
    if rc < 0:
        return 128 - rc
    if rc in (124, 125) or rc >= 128:
        return rc
    if sink.capped or sink.capture_failed:
        return Exit.UNAVAILABLE
    if any(re.fullmatch(r'(?:nested\.)*runtime\.(?:maxHeartbeats|maxRecDepth)', kind)
           for kind in sink.summary['error_kinds']):
        return Exit.CAP
    if sink.summary['n_errors']:
        return Exit.REJECT
    return Exit.ACCEPT if rc == 0 else Exit.UNAVAILABLE


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
