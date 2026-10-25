"""Check the fixed positive corpus against architecture-independent fuel vectors.

This is a counter identity measurement, not package eligibility or admission.
Each input is hashed before execution. Only deterministic counters and typed
outcomes enter the golden; raw diagnostics and physical stack samples do not.
"""
from pathlib import Path
import argparse
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import tarfile

from .source_adoption import case_exact_path
from .build_fuel_tools import digest
from .lean_runner import testnet_caps


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False,
                      allow_nan=False).encode() + b'\n'


def vector_digest(path, kernel):
    hashed = hashlib.sha256()
    count = 0
    with path.open('rb') as stream:
        for line in stream:
            row = json.loads(line)
            if kernel == 'nanoda':
                counters = row['counters']
                row = dict(declaration_index=row['declaration_index'],
                           counters={key: counters[key] for key in
                                     ('calls', 'events', 'steps', 'work', 'depth',
                                      'composed_depth', 'remaining_depth')})
            hashed.update(canonical(row))
            count += 1
    return dict(sha256=hashed.hexdigest(), records=count)


def decoder_controls(binary, out):
    """Generate tiny fault controls; real read errors keep their IO type."""
    out.mkdir()
    malformed = out / 'malformed.ndjson'
    malformed.write_bytes(b'{')
    rows = []
    for label, source, expected in [('malformed', malformed, 70), ('missing', out / 'absent.ndjson', 70)]:
        status = out / (label + '.status.json')
        config = dict(export_file_path=str(source), num_threads=4, nat_extension=True,
                      string_extension=True, permitted_axioms=['Classical.choice', 'Quot.sound', 'propext'],
                      unpermitted_axiom_hard_error=True, gate_limits=testnet_caps()['nanoda'])
        path = out / (label + '.config.json')
        path.write_bytes(canonical(config))
        result = subprocess.run([str(binary), str(path)], capture_output=True, timeout=15,
                                env=dict(os.environ, NANODA_STATUS_FILE=str(status)))
        (out / (label + '.stdout')).write_bytes(result.stdout)
        (out / (label + '.stderr')).write_bytes(result.stderr)
        data = json.loads(status.read_text())
        if result.returncode != expected or data['code'] != expected or data['version'] != 1:
            raise ValueError('trusted decoder fault/read error classification mismatch')
        rows.append(dict(control=label, code=expected))
    (out / 'SUMMARY.json').write_bytes(canonical(dict(status='PASS', rows=rows)))


def one(source, out, lean, nanoda, *, base=None, select=None, measure=False):
    """Execute both actual drivers; no unavailable/crashed outcome is a golden."""
    out.mkdir()
    caps = testnet_caps()
    vectors = out / 'lean.vectors.jsonl'
    env = dict(os.environ, LEAN_STACK_SIZE_KB='262144', LEAN_NAT_MAX_SIZE='4096')
    measurements = {}
    if measure:
        env['COUNTER_TIMING_FILE'] = str(out / 'lean.timing.json')
        env['NANODA_TIMING_FILE'] = str(out / 'nanoda.timing.json')
    def execute(kernel, command):
        if measure:
            from . import correctness_runtime as CR
            result = CR.runner(str(out / (kernel + '.processes')), 1800)(
                command, env, str(out), 1800, 11000)
            (out / (kernel + '.stdout')).write_text(result.out)
            (out / (kernel + '.stderr')).write_text(result.stderr)
            measurements[kernel] = dict(seconds=result.secs, peak_rss_mib=result.peak_rss_mb,
                                        process_receipt=result.process_receipt)
            (out / 'MEASUREMENT.json').write_bytes(canonical(measurements))
            return result.rc
        with (out / (kernel + '.stdout')).open('wb') as stdout, (out / (kernel + '.stderr')).open('wb') as stderr:
            return subprocess.run(command, env=env, stdout=stdout, stderr=stderr, timeout=1800).returncode
    command = [str(lean), str(source), str(vectors), json.dumps(caps['lean'])]
    if base is not None:
        command.append(str(base))
    code = execute('lean', command)
    summary = json.loads((out / 'lean.stdout').read_text())
    if code not in (0, 3) or summary['code'] != code:
        raise ValueError('non-counter Lean outcome')
    lean_result = dict(summary=summary, vector=vector_digest(vectors, 'lean'))
    if base is not None and select is None:
        if code != 0 or summary['checked'] != summary['calls']:
            raise ValueError('incomplete own reference')
        select = []
        with vectors.open() as stream:
            for line in stream:
                select.extend(json.loads(line)['name_parts'])
        if len(select) != summary['checked']:
            raise ValueError('own selection does not match checked declarations')
        (out / 'OWN-NAMES.json').write_bytes(canonical(sorted(select)))
    config = dict(export_file_path=str(source), num_threads=4,
                  permitted_axioms=['Classical.choice', 'Quot.sound', 'propext'],
                  unpermitted_axiom_hard_error=True, nat_extension=True,
                  string_extension=True, print_success_message=False, gate_limits=caps['nanoda'])
    if select is not None:
        config['checked_declaration_names'] = select
    config_path = out / 'nanoda.config.json'
    config_path.write_text(json.dumps(config))
    status = out / 'nanoda.status.json'
    vectors = out / 'nanoda.vectors.jsonl'
    env.update(NANODA_STATUS_FILE=str(status), NANODA_VECTOR_FILE=str(vectors))
    code = execute('nanoda', [str(nanoda), str(config_path)])
    summary = json.loads(status.read_text())
    if (code not in (0, 3) or summary['code'] != code
            or summary['workers'] != 4):
        raise ValueError('non-counter nanoda outcome')
    kept = ('version', 'code', 'checked_declarations', 'aggregate_work', 'aggregate_steps',
            'maximum_depth', 'maximum_composed_depth', 'selected_declarations')
    nano_result = dict(summary={k: summary[k] for k in kept},
                       vector=vector_digest(vectors, 'nanoda'))
    # Retain every raw vector without leaving large uncompressed evidence trees.
    for path in out.glob('*.vectors.jsonl'):
        with path.open('rb') as src, gzip.GzipFile(filename=str(path) + '.gz', mode='wb', mtime=0) as dst:
            shutil.copyfileobj(src, dst)
        path.unlink()
    record = dict(lean=lean_result, nanoda=nano_result)
    (out / 'COUNTERS.json').write_bytes(canonical(record))
    return record


def corpus(fixture, out, lean, nanoda):
    manifest_path = case_exact_path(fixture, 'manifest.json', ['manifest.json'])
    manifest = json.loads(manifest_path.read_text())
    families = {'identity', 'universe', 'pair', 'and', 'or', 'nat-rec', 'nat-theorem', 'list',
                'quotient', 'funext', 'inductive', 'literal', 'string', 'primitive', 'bitwise', 'nested'}
    if (manifest.get('version') != 2 or manifest.get('scope') != 'public-generated-Init/v1'
            or manifest['counts'] != {family: 2 for family in families} or len(manifest['units']) != 32):
        raise ValueError('fixed 32-unit public corpus required')
    prereg_path = case_exact_path(fixture, 'PREREGISTRATION.json', ['PREREGISTRATION.json'])
    if digest(prereg_path) != manifest['preregistration_sha256']:
        raise ValueError('public source preregistration integrity')
    prereg = json.loads(prereg_path.read_text())
    if (prereg['scope'] != manifest['scope'] or len(prereg['sources']) != 32
            or prereg['generator_sha256'] != manifest['generator_sha256']):
        raise ValueError('public source preregistration mismatch')
    if [(u['source_index'], u['family'], u['size']) for u in manifest['units']] != [
            (u['index'], u['family'], u['size']) for u in prereg['sources']]:
        raise ValueError('public export source mismatch')
    archive = out / 'corpus.tar.zst'
    with archive.open('xb') as dst:
        for part in manifest['parts']:
            if Path(part['path']).name != part['path']:
                raise ValueError('invalid corpus part')
            path = case_exact_path(fixture, part['path'], [p['path'] for p in manifest['parts']])
            if path.stat().st_size != part['bytes'] or digest(path) != part['sha256']:
                raise ValueError('corpus part integrity')
            with path.open('rb') as src:
                shutil.copyfileobj(src, dst)
    if archive.stat().st_size != manifest['compressed_bytes'] or digest(archive) != manifest['compressed_sha256']:
        raise ValueError('corpus integrity')
    rows = []
    with subprocess.Popen(['zstd', '-d', '-q', '--stdout', str(archive)], stdout=subprocess.PIPE) as child:
        with tarfile.open(fileobj=child.stdout, mode='r|') as stream:
            for index, member in enumerate(stream):
                if index >= len(manifest['units']):
                    raise ValueError('extra corpus member')
                unit = manifest['units'][index]
                if not member.isfile() or member.name != unit['member'] or member.size != unit['raw_bytes']:
                    raise ValueError('unexpected corpus member')
                source = out / 'current.ndjson'
                with stream.extractfile(member) as src, source.open('xb') as dst:
                    shutil.copyfileobj(src, dst)
                if digest(source) != unit['raw_sha256']:
                    raise ValueError('source integrity')
                result = one(source, out / ('unit-' + str(unit['source_index'])), lean, nanoda)
                rows.append(dict(source_index=unit['source_index'], source_sha256=unit['raw_sha256'],
                                 **result))
                (out / 'PROGRESS.json').write_bytes(canonical(dict(completed=len(rows), rows=rows)))
                print(json.dumps(dict(source_index=unit['source_index'], completed=len(rows),
                                      lean=result['lean']['summary']['code'],
                                      nanoda=result['nanoda']['summary']['code'])), flush=True)
                source.unlink()
        child.stdout.close()
        if child.wait() != 0 or len(rows) != len(manifest['units']):
            raise ValueError('incomplete corpus')
    archive.unlink()
    return dict(version=1, manifest_sha256=digest(fixture / 'manifest.json'),
                scope='public-generated-counter-identity/v2', rows=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--lean-counter', required=True, type=Path)
    parser.add_argument('--nanoda', required=True, type=Path)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--expected', type=Path)
    mode.add_argument('--record', type=Path, help='explicit development golden generation only')
    args = parser.parse_args()
    args.out.mkdir()
    (args.out / 'RUNNER.json').write_bytes(canonical(dict(
        harness_sha256=digest(Path(__file__)), lean_counter_sha256=digest(args.lean_counter),
        nanoda_sha256=digest(args.nanoda))))
    decoder_controls(args.nanoda.resolve(), args.out.resolve() / 'driver-controls')
    actual = corpus(args.fixture.resolve(), args.out.resolve(), args.lean_counter.resolve(), args.nanoda.resolve())
    (args.out / 'ACTUAL.json').write_bytes(canonical(actual))
    if args.expected:
        if actual != json.loads(args.expected.read_text()):
            raise SystemExit('counter vectors differ from aarch64 golden')
    else:
        with args.record.open('xb') as stream:
            stream.write(canonical(actual))
    print('PASS: all 32 public counter vectors and typed outcomes', flush=True)


if __name__ == '__main__':
    main()
