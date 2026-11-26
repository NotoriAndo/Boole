"""Measure numeric token boundaries with inert declarations in pinned Lean 4.34.1."""

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory


LEAN_SHA256 = '1b370cfcbf44e80d1b004ab1b1ab9a4c73951f9f7c242140bcff9bc577576554'
CORPUS = Path(__file__).resolve().parents[1] / 'fixtures/submission-policy/numerals-v6.json'
DECLARATIONS = {
    'separated': '\ntheorem seen : True := True.intro\nend Probe\nexample : True := Probe.seen',
    'theorem': 'theorem seen : True := True.intro\nend Probe\nexample : True := Probe.seen',
    'def': 'def seen : Nat := 7\nend Probe\nexample : Probe.seen = 7 := rfl',
    'abbrev': 'abbrev seen : Nat := 7\nend Probe\nexample : Probe.seen = 7 := rfl',
    'end': 'end Probe\nexample : Probe.witness = 7 := rfl',
}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def compile_cell(lean, folder, source):
    # The absolute Lean executable derives its toolchain location from argv[0].
    proc = subprocess.run(['/usr/bin/env', '-i', str(lean), '--json', '--stdin'],
                          cwd=folder, input=source, text=True, encoding='utf-8',
                          capture_output=True, timeout=15)
    messages = [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
    errors = [m['data'] for m in messages if m['severity'] == 'error']
    if proc.stderr or (proc.returncode != 0) != bool(errors):
        raise ValueError(f'unclassified compiler exit {proc.returncode}: {proc.stderr}')
    if not errors:
        status = 'elaborated'
    elif any(s in msg for msg in errors for s in
             ('unexpected', 'expected token', 'missing exponent digits', 'invalid digit')):
        status = 'syntax-error'
    else:
        status = 'other-error'
    return {'status': status, 'exit': proc.returncode, 'messages': [
        {'severity': m['severity'], 'line': m['pos']['line'],
         'column': m['pos']['column'], 'message': m['data']} for m in messages]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lean', type=Path, required=True)
    parser.add_argument('--hold', type=Path, required=True)
    parser.add_argument('--hold-schema', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not all(p.is_absolute() for p in (args.lean, args.hold, args.output)):
        parser.error('absolute paths required')
    if args.output.exists() or digest(args.lean) != LEAN_SHA256 or os.getpriority(os.PRIO_PROCESS, 0) != 19:
        parser.error('fresh output, pinned compiler and nice 19 required')
    rows = json.loads(CORPUS.read_bytes())
    results = {}
    counts = {marker: Counter() for marker in DECLARATIONS}
    with TemporaryDirectory(dir=args.output.parent) as folder:
        for row in rows:
            cases = results[row['name']] = {}
            header = 'namespace Probe\ndef witness : Nat := 7\ndef subject : ' + row['type'] + ' := '
            for marker, declaration in DECLARATIONS.items():
                hold = json.loads(args.hold.read_bytes())
                if hold.get('schema') != args.hold_schema or hold.get('hold') is not False:
                    raise SystemExit('hold is not clear')
                outcome = compile_cell(args.lean, folder, header + row['literal'] + declaration + '\n')
                cases[marker] = outcome
                counts[marker][outcome['status']] += 1
    if digest(args.lean) != LEAN_SHA256:
        raise ValueError('compiler pin changed')
    if not all(results['decimal'][m]['status'] == 'elaborated' for m in ('theorem', 'def', 'abbrev', 'separated')):
        raise ValueError('decimal reference declarations did not elaborate')
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump({'lean_sha256': LEAN_SHA256, 'corpus_sha256': digest(CORPUS), 'columns': counts,
                   'rows': results, 'row_counts': {
                       name: Counter(v['status'] for v in cases.values()) for name, cases in results.items()}},
                  stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write('\n')


if __name__ == '__main__':
    main()
