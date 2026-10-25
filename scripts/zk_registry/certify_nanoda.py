"""Bounded-memory certification of every declaration in a complete trusted base.

Only offline base certification uses batches. Submission checking still uses the
single production four-worker driver and the package's aggregate allowance.
"""
from pathlib import Path
import gzip
import hashlib
import json
import os
import shutil

from . import correctness_runtime as CR
from . import lean_runner as L
from .counter_identity import canonical

BATCH_DECLARATIONS = 2048
COUNTERS = ('calls', 'events', 'steps', 'work', 'depth', 'composed_depth', 'remaining_depth')


def declaration_names(source):
    names = {0: []}
    declarations = []
    with source.open('rb') as stream:
        for line in stream:
            row = json.loads(line)
            if 'in' in row:
                if 'str' in row:
                    value = row['str']
                    names[row['in']] = names[value['pre']] + ['s' + value['str']]
                elif 'num' in row:
                    value = row['num']
                    names[row['in']] = names[value['pre']] + ['n' + str(value['i'])]
            elif 'inductive' in row:
                group = row['inductive']
                for kind in ('types', 'ctors', 'recs'):
                    declarations.extend(names[item['name']] for item in group[kind])
            else:
                for kind in ('axiom', 'def', 'thm', 'opaque', 'quot'):
                    if kind in row:
                        declarations.append(names[row[kind]['name']])
    if len({tuple(name) for name in declarations}) != len(declarations):
        raise ValueError('duplicate certification declaration')
    return declarations


def certify(source, out, binary):
    out.mkdir()
    names = declaration_names(source)
    limits = L.testnet_caps()['nanoda']
    hashed = hashlib.sha256()
    checked, debit, steps, depth, composed = 0, 0, 0, 0, 0
    coverage = set()
    (out / 'SELECTION.json').write_bytes(canonical(dict(
        batch_declarations=BATCH_DECLARATIONS, declarations=len(names),
        selection_sha256=hashlib.sha256(canonical(names)).hexdigest())))
    for offset in range(0, len(names), BATCH_DECLARATIONS):
        work = out / ('batch-' + str(offset // BATCH_DECLARATIONS))
        work.mkdir()
        selected = names[offset:offset + BATCH_DECLARATIONS]
        config = dict(export_file_path=str(source), num_threads=4, nat_extension=True,
                      string_extension=True, permitted_axioms=['Classical.choice', 'Quot.sound', 'propext'],
                      unpermitted_axiom_hard_error=True, print_success_message=False,
                      gate_limits=limits, checked_declaration_names=selected)
        path = work / 'config.json'
        path.write_bytes(canonical(config))
        status, vectors = work / 'status.json', work / 'vectors.jsonl'
        env = dict(os.environ, NANODA_STATUS_FILE=str(status), NANODA_VECTOR_FILE=str(vectors))
        result = CR.runner(str(work / 'processes'), 1800)([str(binary), str(path)], env, str(work), 1800, 11000)
        (work / 'stdout').write_text(result.out)
        (work / 'stderr').write_text(result.stderr)
        (work / 'MEASUREMENT.json').write_bytes(canonical(dict(seconds=result.secs, peak_rss_mib=result.peak_rss_mb,
                                                             process_receipt=result.process_receipt)))
        summary = json.loads(status.read_text())
        if (result.rc != 0 or summary['code'] != 0 or summary['workers'] != 4
                or len(summary['worker_stack_bytes']) != 4
                or min(summary['worker_stack_bytes']) < 67108864
                or summary['selected_declarations'] != len(selected)
                or summary['checked_declarations'] != len(selected)):
            raise ValueError('base certification batch unavailable or rejected')
        rows = []
        with vectors.open() as stream:
            for line in stream:
                row = json.loads(line)
                index = row['declaration_index']
                if type(index) is not int or index in coverage or not 0 <= index < len(names):
                    raise ValueError('base certification coverage mismatch')
                coverage.add(index)
                rows.append(dict(declaration_index=index, counters={k: row['counters'][k] for k in COUNTERS}))
        if {r['declaration_index'] for r in rows} != set(range(offset, offset + len(selected))):
            raise ValueError('base declaration order differs from native export order')
        for row in sorted(rows, key=lambda r: r['declaration_index']):
            hashed.update(canonical(row))
        checked += summary['checked_declarations']
        debit += summary['aggregate_work']
        steps += summary['aggregate_steps']
        depth = max(depth, summary['maximum_depth'])
        composed = max(composed, summary['maximum_composed_depth'])
        with vectors.open('rb') as src, gzip.GzipFile(filename=str(vectors) + '.gz', mode='wb', mtime=0) as dst:
            shutil.copyfileobj(src, dst)
        vectors.unlink()
        if debit > limits['aggregate_work']:
            raise ValueError('complete base aggregate cap')
    if checked != len(names) or coverage != set(range(len(names))):
        raise ValueError('incomplete base certification')
    summary = dict(version=1, code=0, workers=4, checked_declarations=checked,
                   selected_declarations=checked, aggregate_work=debit, aggregate_steps=steps,
                   maximum_depth=depth, maximum_composed_depth=composed)
    result = dict(summary=summary, vector=dict(sha256=hashed.hexdigest(), records=checked),
                  scope='complete-base-certification-batches/v1', batch_declarations=BATCH_DECLARATIONS)
    (out / 'SUMMARY.json').write_bytes(canonical(result))
    return result
