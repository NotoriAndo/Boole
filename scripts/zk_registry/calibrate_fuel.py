"""Measure checked own deltas; never infer admission from historical Lean timings.

Plans pin every provisioned state and source archive. Measurement output is not
an authority file: both OS results and envelope fitting must precede adoption.
"""
from pathlib import Path
import argparse
import gzip
import json
import os
import shutil
import subprocess

from . import closure as C
from . import fuel_caps as F
from . import lean_runner as L
from .counter_identity import canonical, one


def materialize(source, destination):
    archive = Path(source['archive'])
    if C.digest(archive) != source['archive_sha256']:
        raise ValueError('reference archive integrity')
    with destination.open('xb') as dst:
        if source['format'] == 'gzip':
            with gzip.open(archive, 'rb') as src:
                shutil.copyfileobj(src, dst)
        elif source['format'] == 'zstd':
            subprocess.run(['zstd', '-d', '-q', '--stdout', str(archive)], stdout=dst, check=True)
        else:
            raise ValueError('unsupported reference archive')
    if destination.stat().st_size != source['bytes'] or C.digest(destination) != source['sha256']:
        raise ValueError('reference bytes integrity')


def measure_reference(unit, directory, lean, nanoda):
    directory.mkdir()
    state = Path(unit['state'])
    if C.digest(state) != unit['state_sha256']:
        raise ValueError('certified state integrity')
    source = directory / 'reference.ndjson'
    materialize(unit['source'], source)
    row = dict(problem=unit['problem'], environment=unit['environment'],
               reference_export=unit['source']['sha256'], state_sha256=unit['state_sha256'])
    identity = C.statement_identity(str(source), unit['target'], str(directory / 'identity'), 1800,
                                    expected_sha256=unit['source']['sha256'])
    row['identity'] = identity
    if identity['verdict'] != 'PASS':
        row.update(status='EXCLUDED' if identity.get('tool_exit') in (2, 3, 4) else 'UNAVAILABLE',
                   reason=identity.get('reason'))
    else:
        try:
            counters = one(source, directory / 'kernels', lean, nanoda, base=state, measure=True)
        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
            row.update(status='UNQUALIFIED', reason=str(error))
            measurement = directory / 'kernels/MEASUREMENT.json'
            if measurement.exists():
                row['measurement'] = json.loads(measurement.read_text())
                codes = [r['process_receipt']['exit_code'] for r in row['measurement'].values()]
                if any(code in (2, 3) for code in codes):
                    row.update(status='EXCLUDED', reason='reference-kernel-cap' if 3 in codes else 'reference-kernel-rejection')
        else:
            row['counters'] = counters
            if counters['lean']['summary']['code'] or counters['nanoda']['summary']['code']:
                row.update(status='EXCLUDED', reason='reference-kernel-cap')
            elif (counters['lean']['summary']['checked'] == 0
                  or counters['lean']['summary']['checked'] != counters['nanoda']['summary']['checked_declarations']):
                row.update(status='EXCLUDED', reason='checked-own-set-mismatch')
            else:
                names = json.loads((directory / 'kernels/OWN-NAMES.json').read_text())
                units = dict(lean=counters['lean']['summary']['debit'],
                             nanoda=counters['nanoda']['summary']['aggregate_work'])
                reference = dict(path='certified-own-delta/v1', problem=unit['problem'],
                                 environment=unit['environment'], reference_export=unit['source']['sha256'],
                                 statement=identity['statement_closure_sha256'],
                                 own_declarations=F.digest(dict(names=names)), units=units,
                                 eligible=False)
                row.update(status='MEASURED', reference=reference,
                           cap_units={k: 4 * n for k, n in units.items()})
            row['measurement'] = json.loads((directory / 'kernels/MEASUREMENT.json').read_text())
            row['timing'] = {kernel: json.loads((directory / ('kernels/' + kernel + '.timing.json')).read_text())
                             for kernel in ('lean', 'nanoda')}
    if C.digest(state) != unit['state_sha256'] or C.digest(source) != unit['source']['sha256']:
        raise ValueError('measurement inputs changed')
    # Archives and exact hashes remain retained; remove only our materialization.
    source.unlink()
    (directory / 'RESULT.json').write_bytes(canonical(row))
    return row


def certify(unit, directory, exporter, lean, nanoda, batch_nanoda=False):
    directory.mkdir()
    state = Path(unit['state'])
    if C.digest(state) != unit['state_sha256']:
        raise ValueError('certified state integrity')
    source = directory / 'complete.ndjson'
    env = dict(os.environ, LEAN_STACK_SIZE_KB='262144', LEAN_NAT_MAX_SIZE='4096')
    with source.open('xb') as dst, (directory / 'export.stderr').open('wb') as stderr:
        subprocess.run([str(exporter), str(state)], env=env, stdout=dst, stderr=stderr,
                       timeout=1800, check=True)
    record = dict(environment=unit['environment'], state_sha256=unit['state_sha256'],
                  full_export_sha256=C.digest(source), full_export_bytes=source.stat().st_size,
                  lean_counter_sha256=C.digest(lean), nanoda_sha256=C.digest(nanoda),
                  exporter_sha256=C.digest(exporter), limits=L.testnet_caps())
    parent = None
    if 'parent_certificate' in unit:
        pin = unit['parent_certificate']
        certificate = Path(pin['path'])
        if C.digest(certificate) != pin['sha256']:
            raise ValueError('parent certificate integrity')
        certified = json.loads(certificate.read_text())
        parent = Path(unit['parent_state'])
        if (certified['status'] != 'DUAL-CERTIFIED'
                or certified['state_sha256'] != unit['parent_state_sha256']
                or C.digest(parent) != unit['parent_state_sha256']):
            raise ValueError('parent is not dual certified')
        record['parent_certificate_sha256'] = pin['sha256']
    try:
        if batch_nanoda and parent is None:
            # C5 full replay and its quotient check run once. The independent
            # Nano certificate covers every declaration in bounded-memory
            # four-worker processes, with original indices and exact coverage.
            full_dir = directory / 'full'
            full_dir.mkdir()
            vectors = full_dir / 'lean.vectors.jsonl'
            from . import correctness_runtime as CR
            from .counter_identity import vector_digest
            result = CR.runner(str(full_dir / 'lean.processes'), 1800)(
                [str(lean), str(source), str(vectors), json.dumps(L.testnet_caps()['lean'])],
                env, str(full_dir), 1800, 11000)
            (full_dir / 'lean.stdout').write_text(result.out)
            (full_dir / 'lean.stderr').write_text(result.stderr)
            (full_dir / 'MEASUREMENT.json').write_bytes(canonical(dict(lean=dict(
                seconds=result.secs, peak_rss_mib=result.peak_rss_mb, process_receipt=result.process_receipt))))
            summary = json.loads(result.out)
            if result.rc != 0 or summary['code'] != 0:
                raise ValueError('base Lean replay or quotient check failed')
            from .certify_nanoda import certify as certify_all
            full = dict(lean=dict(summary=summary, vector=vector_digest(vectors, 'lean')),
                        nanoda=certify_all(source, full_dir / 'nanoda-batches', nanoda))
            with vectors.open('rb') as src, gzip.GzipFile(filename=str(vectors) + '.gz', mode='wb', mtime=0) as dst:
                shutil.copyfileobj(src, dst)
            vectors.unlink()
        else:
            full = one(source, directory / 'full', lean, nanoda, base=parent, measure=True)
        record['full'] = full
        if full['lean']['summary']['code'] or full['nanoda']['summary']['code']:
            record.update(status='UNQUALIFIED', reason='base-kernel-cap')
        else:
            empty = one(source, directory / 'identity', lean, nanoda, base=state, measure=True)
            record['identity'] = empty
            if (empty['lean']['summary']['code'] or empty['nanoda']['summary']['code']
                    or empty['lean']['summary']['checked'] or empty['nanoda']['summary']['checked_declarations']):
                raise ValueError('complete certified state identity mismatch')
            record['status'] = 'DUAL-CERTIFIED'
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        record.update(status='UNQUALIFIED', reason=str(error))
    if C.digest(state) != unit['state_sha256']:
        raise ValueError('certified state changed')
    (directory / 'CERTIFICATE.json').write_bytes(canonical(record))
    with source.open('rb') as src, (directory / 'complete.ndjson.zst').open('xb') as dst:
        subprocess.run(['zstd', '-q', '-3', '-T1', '--stdout'], stdin=src, stdout=dst, check=True)
    source.unlink()
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('certify', 'references'))
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--lean-counter', type=Path, required=True)
    parser.add_argument('--nanoda', type=Path, required=True)
    parser.add_argument('--certified-export', type=Path)
    parser.add_argument('--batch-nanoda', action='store_true',
                        help='offline complete-base certification only; no submission path change')
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    args.out.mkdir()
    rows = []
    (args.out / 'RUNNER.json').write_bytes(canonical(dict(
        plan_sha256=C.digest(args.plan), harness_sha256=C.digest(Path(__file__)),
        counter_harness_sha256=C.digest(Path(__file__).with_name('counter_identity.py')))))
    for index, unit in enumerate(plan['units']):
        directory = args.out / ('unit-' + str(index))
        restored = None
        if 'state_source' in unit:
            # Restore a single exact own copy; retain the original compressed
            # certificate and never write beneath another session's paths.
            restored = args.out / ('state-' + str(index) + '.olean')
            materialize(unit['state_source'], restored)
            unit = dict(unit, state=str(restored.resolve()))
        try:
            if args.mode == 'certify':
                row = certify(unit, directory, args.certified_export.resolve(), args.lean_counter.resolve(), args.nanoda.resolve(), args.batch_nanoda)
            else:
                row = measure_reference(unit, directory, args.lean_counter.resolve(), args.nanoda.resolve())
        finally:
            if restored is not None:
                restored.unlink()
        rows.append(row)
        (args.out / 'SUMMARY.json').write_bytes(canonical(dict(complete=index + 1, total=len(plan['units']), rows=rows)))
        print(json.dumps(dict(sequence=index, status=row['status'])), flush=True)


if __name__ == '__main__':
    main()
