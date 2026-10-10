"""Deterministic guarded C-package generation: original model, accepted S, non-vacuity and fixed floor.

Only fixed mechanical tactics run here. No specification is authored and no solver
or hand-written correctness proof is attempted.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import time

from . import check as C
from . import correctness_gnark as CG
from . import correctness_runtime as Runtime
from . import gates as G
from . import gnark_r1cs as GR
from . import lean_runner as L
from . import package as P
from . import spec_problem as S


def put(path: Path, text: str) -> None:
    """Write a generated artifact without overwriting an earlier run."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x') as stream:
        stream.write(text)


def read(path: Path) -> dict:
    """Read pinned UTF-8 JSON input."""
    return json.loads(path.read_text())


def battery_completed(result, messages: list[dict]) -> bool:
    """Accept successful elaboration or recognized tactic failure, never an unclassified compiler/process failure."""
    if result.rc not in (0, 1) or result.timeout or result.memkill or result.killed_by:
        return False
    errors = L.errors(messages)
    if result.rc == 0:
        return not errors
    expected = ('unsolved goals', 'omega could not prove the goal', 'simp_all made no progress',
                'simp made no progress', 'Expected type must not contain free variables',
                'tactic', 'failed to synthesize', 'maximum number of heartbeats',
                '(deterministic) timeout', 'maximum recursion depth')
    forbidden = ('unknown identifier', 'unknown constant', 'unknown tactic', 'unexpected token',
                 'object file', 'invalid field', 'type mismatch', 'application type mismatch')
    return bool(errors) and all(any(marker in message.get('data', '') for marker in expected)
                                and not any(marker in message.get('data', '') for marker in forbidden)
                                for message in errors)


def copy(source: Path, target: Path) -> None:
    """Preserve exact original bytes; do not follow an unpinned symbolic link."""
    if source.is_symlink() or not source.is_file():
        raise ValueError('Not a regular pinned source: ' + str(source))
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        raise FileExistsError(target)
    shutil.copyfile(source, target)


def prepare(unit: dict, registry: Path, spec_directory: Path, source_file: Path,
            stage: Path, extra_witnesses: list[Path]) -> list[Path]:
    """Prepare pristine imports, original provenance and native solver witness bytes."""
    stage.mkdir(parents=True, exist_ok=False)
    spec = read(spec_directory / 'problem.json')
    if P.validate_problem(spec, str(spec_directory)) or spec['status'] != 'ACCEPTED-LOCAL':
        raise ValueError('Invalid accepted S package')
    if spec['family'] != unit['reference_operation']:
        raise ValueError('Accepted specification family mismatch')
    copy(registry / unit['model_file'], stage / unit['model_file'])
    copy(spec_directory / spec['specification']['file'], stage / 'Candidate.lean')
    copy(spec_directory / 'problem.json', stage / 'spec/problem.json')
    for record in spec['files']:
        copy(spec_directory / record['path'], stage / 'spec' / record['path'])
    for original, target in [('problem.json', 'problem.json'),
                             ('evidence/harness_result.json', 'evidence/harness_result.json'),
                             ('evidence/wrapper.go', 'evidence/wrapper.go')]:
        copy(registry / original, stage / 'reference' / target)
    copy(source_file, stage / 'reference/source.go')
    if P.sha256_file(str(source_file)) != unit['source_fullfile_sha256']:
        raise ValueError('Original source file changed')
    for name, text in CG.render_modules(unit).items():
        put(stage / name, text)
    put(stage / 'EvaluationSupport.lean', CG.evaluation_support((stage / unit['model_file']).read_text(),
                                                              unit['model_module']))
    result = GR.build(read(registry / 'evidence/harness_result.json'))
    witnesses = []
    for index, witness in enumerate(result.witnesses):
        path = stage / 'evidence/witnesses' / f'real_{index:03d}.txt'
        put(path, '\n'.join(map(str, witness)) + '\n')
        witnesses.append(path)
    # Production archives separate solver assignments from harness_result.json.
    # Keep those native assignment files verbatim rather than inferring "no witness"
    # from the deliberately omitted JSON samples field.
    existing = {P.sha256_file(str(path)) for path in witnesses}
    for index, original in enumerate(sorted((registry / 'evidence/witnesses').glob('real_*.txt'))):
        digest = P.sha256_file(str(original))
        if digest in existing:
            continue
        path = stage / 'evidence/witnesses' / f'archived_{index:03d}.txt'
        copy(original, path)
        if P.sha256_file(str(path)) != digest or P.sha256_file(str(original)) != digest:
            raise ValueError('Original native witness changed during copying')
        existing.add(digest)
        witnesses.append(path)
    for index, witness in enumerate(extra_witnesses):
        path = stage / 'evidence/witnesses' / f'extra_{index:03d}.txt'
        copy(witness, path)
        witnesses.append(path)
    return witnesses


def imports(unit: dict, stage: Path, build: Path, env: L.LeanEnv, timeout: int) -> list[dict]:
    """Compile only pristine pure imports in dependency order."""
    records = []
    for path in (unit['model_file'], 'Candidate.lean', 'Bindings.lean', 'Statements.lean'):
        result, messages = L.compile_module(env, str(stage), path, str(build), CG.LEAN_OPTIONS, timeout)
        records.append(dict(path=path, exit_code=result.rc, seconds=result.secs, peak_rss_mb=result.peak_rss_mb))
        if result.rc != 0 or result.timeout or result.memkill or result.killed_by or L.errors(messages):
            raise RuntimeError('Pure import failed: ' + path + ': ' + result.out[-1000:])
    return records


def evaluate(unit: dict, stage: Path, build: Path, witnesses: list[Path], env: L.LeanEnv,
             timeout: int) -> dict:
    """Evaluate all joint premises on actual original solver assignments, outside the statement import closure."""
    witness_pins = {str(path): P.sha256_file(str(path)) for path in witnesses}
    result, messages = L.compile_module(env, str(stage), 'EvaluationSupport.lean', str(build),
                                        CG.LEAN_OPTIONS, timeout)
    if result.rc != 0 or result.killed_by or result.timeout or result.memkill or L.errors(messages):
        return dict(completed=False, rows=[], error='Evaluation support failed', output=result.out[-2000:])
    result = L.run_lean(env, CG.LEAN_OPTIONS + ['--run', str(stage / 'Evaluation.lean')] + list(map(str, witnesses)),
                        str(stage), timeout, extra_lean_path=[str(build)])
    put(stage / 'evidence/evaluation.stdout', result.out)
    rows = []
    for line in result.out.splitlines():
        if line.startswith('{'):
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if 'witness' in row and 'Constraints' in row:
                if row['witness'] not in witness_pins:
                    continue
                row['witness_sha256'] = witness_pins[row['witness']]
                rows.append(row)
    completed = (result.rc == 0 and not result.timeout and not result.memkill and not result.killed_by
                 and len(rows) == len(witnesses)
                 and all(P.sha256_file(path) == digest for path, digest in witness_pins.items()))
    record = dict(completed=completed, rows=rows, exit_code=result.rc, reason=result.killed_by,
                  seconds=result.secs, peak_rss_mb=result.peak_rss_mb,
                  evaluation_sha256=P.sha256_file(str(stage / 'Evaluation.lean')),
                  support_sha256=P.sha256_file(str(stage / 'EvaluationSupport.lean')),
                  bindings_sha256=S.package_digest(unit), actual_solver_witnesses=len(witnesses))
    P.write_json(str(stage / 'evidence/evaluation.json'), record)
    return record


def reference(unit: dict, kind: str, stage: Path, build: Path, env: L.LeanEnv, timeout: int) -> dict:
    """Record an elaborated reference theorem type with its sole intentional reference hole."""
    path = stage / (kind + '-reference.lean')
    put(path, CG.theorem_template(unit, kind))
    result, messages = L.compile_module(env, str(stage), path.name, str(build), CG.LEAN_OPTIONS, timeout)
    warnings = [message for message in messages if message.get('severity') == 'warning']
    if (result.rc != 0 or result.timeout or result.memkill or result.killed_by or L.errors(messages)
            or len(warnings) != 1 or 'sorry' not in warnings[0].get('data', '')):
        raise RuntimeError('Reference type elaboration failed: ' + result.out[-1000:])
    name = 'Soundness' if kind == 'soundness' else 'Completeness'
    fqn = 'MProof.' + unit['id'] + name + '.solution'
    replay_result, replay = G.run_replay(env, str(build / (kind + '-reference.olean')), fqn,
                                       str(stage / 'evidence' / (kind + '-reference.type')),
                                       CG.LEAN_OPTIONS, [str(build)], timeout)
    if (replay_result.rc != 0 or replay_result.timeout or replay_result.memkill or replay_result.killed_by
            or replay.get('replay') != 'ok' or not isinstance(replay.get('target'), dict)
            or replay['target']['kind'] != 'theorem' or 'sorryAx' not in replay['axioms']
            or set(replay['axioms']) - C.ALLOWED_AXIOMS - {'sorryAx'}):
        raise RuntimeError('Reference kernel/type recording failed')
    return dict(reference_type_sha256=replay['type_sha256'], theorem_fqn=fqn, replay='ok',
                hole='reference-only', source_sha256=P.sha256_file(str(path)))


def pure_files(unit: dict, stage: Path, template: str) -> list[dict]:
    """Pin the exact checker import closure; evaluator modules never appear as imports."""
    files = [dict(path=path, role='import', module=path[:-5].replace('/', '.'),
                  sha256=P.sha256_file(str(stage / path)))
             for path in (unit['model_file'], 'Candidate.lean', 'Bindings.lean', 'Statements.lean')]
    files.append(dict(path='ProofStatement.lean', role='statement', sha256=hashlib.sha256(template.encode()).hexdigest()))
    return files


def check_closure(unit: dict, kind: str, stage: Path, text: str, type_sha: str,
                  env: L.LeanEnv, work: Path, timeout: int) -> dict:
    """Check an unchanged fixed-battery closure with production text, axioms, replay and exact type checks."""
    template = CG.theorem_template(unit, kind)
    invalid, auxiliary, body = C.text_check(template, text, 'solution')
    invalid += C.forbidden_scan([('helpers', auxiliary), ('proof', body)])
    fail, error = [], []
    checker = CG.checker_record(unit, kind, pure_files(unit, stage, template), type_sha)
    if invalid:
        return dict(verdict='INVALID', invalid=invalid, fail=fail, error=error)
    compiled = C.compile_and_postcheck(env, dict(checker=checker), str(stage), text, str(work), timeout)
    if 'error' in compiled:
        error.append(compiled['error'])
    else:
        C.apply_compile_result(compiled, checker, invalid, fail, error)
    record = dict(invalid=invalid, fail=fail, error=error, compile=compiled,
                  proof_sha256=hashlib.sha256(text.encode()).hexdigest())
    record['verdict'] = C.verdict_of(record)
    return record


def battery(unit: dict, kind: str, stage: Path, build: Path, type_sha: str,
            env: L.LeanEnv, work: Path) -> dict:
    """Run exactly seven frozen forms, at 200k heartbeats, within one unreset 600-second floor window."""
    started = time.monotonic()
    attempts, closure, verdict = [], None, None
    template = CG.theorem_template(unit, kind)
    goal = unit['id'] + ('.Soundness' if kind == 'soundness' else '.Completeness')
    for index, form in enumerate(CG.BATTERY_FORMS, 1):
        remaining = 600 - (time.monotonic() - started)
        if remaining <= 0:
            break
        tactic = 'unfold ' + goal + '\n  ' + CG.qualify_battery(unit, kind, form)
        text = template.replace('  sorry', '  ' + tactic)
        path = stage / 'evidence/battery' / f'{kind}-{index}.lean'
        put(path, text)
        result = L.run_lean(env, ['--tstack=32768', '-DmaxRecDepth=100000', '-DmaxHeartbeats=200000',
                                '-DsynthInstance.maxSize=100000', '--json', str(path)], str(stage), remaining,
                            extra_lean_path=[str(build)])
        put(path.with_suffix('.stdout'), result.out)
        messages = L.parse_messages(result.out)
        completed = battery_completed(result, messages)
        closed = result.rc == 0 and completed and not L.errors(messages)
        attempts.append(dict(index=index, form=CG.qualify_battery(unit, kind, form), completed=completed, closed=closed,
                             exit_code=result.rc, reason=result.killed_by, seconds=result.secs,
                             peak_rss_mb=result.peak_rss_mb, source_sha256=P.sha256_file(str(path)),
                             stdout_sha256=P.sha256_file(str(path.with_suffix('.stdout'))),
                             source_file=str(path.relative_to(stage)),
                             stdout_file=str(path.with_suffix('.stdout').relative_to(stage))))
        if closed:
            closure = text
            checked = check_closure(unit, kind, stage, text, type_sha, env, work / f'closure-{kind}', 1800)
            P.write_json(str(stage / 'evidence' / (kind + '-closure-check.json')), checked)
            verdict = checked['verdict']
            break
    return dict(forms=CG.BATTERY_FORMS, attempts=attempts, checker_verdict=verdict,
                timeout_seconds_per_property=600, heartbeats_per_form=200000,
                seconds=round(time.monotonic() - started, 3), new_solver_proof_attempts=0,
                closure_sha256=hashlib.sha256(closure.encode()).hexdigest() if closure else None)


def package_property(unit: dict, kind: str, spec: dict, stage: Path, destination: Path,
                     selected: dict, evaluation: dict, floor: dict, type_sha: str,
                     env: L.LeanEnv, created_utc: str) -> dict:
    """Write a self-contained C package only after all qualification gates pass."""
    destination.mkdir(parents=True, exist_ok=False)
    template = CG.theorem_template(unit, kind)
    files = pure_files(unit, stage, template)
    for record in files[:-1]:
        copy(stage / record['path'], destination / record['path'])
    put(destination / 'ProofStatement.lean', template)
    witness_file = 'evidence/nonvacuity-witness.txt'
    witness = Path(selected['witness'])
    if P.sha256_file(str(witness)) != selected['witness_sha256']:
        raise ValueError('Witness bytes changed after Lean evaluation')
    copy(witness, destination / witness_file)
    witness_sha = P.sha256_file(str(witness))
    if witness_sha != selected['witness_sha256'] or P.sha256_file(str(destination / witness_file)) != witness_sha:
        raise ValueError('Witness bytes changed while packaging')
    nv_data = dict(completed=True, rows=[selected], binding_sha256=S.package_digest(unit), witness_sha256=witness_sha,
                   evaluator=dict(evaluation_sha256=evaluation['evaluation_sha256'],
                                  support_sha256=evaluation['support_sha256'], exit_code=evaluation['exit_code'],
                                  reason=evaluation['reason']), actual_solver_assignment=True)
    P.write_json(str(destination / 'evidence/nonvacuity.json'), nv_data)
    P.write_json(str(destination / 'evidence/battery.json'), floor)
    for original in sorted(stage.rglob('*')):
        if not original.is_file():
            continue
        rel = original.relative_to(stage)
        if (rel.parts[0] in ('reference', 'spec') or original.name in ('Evaluation.lean', 'EvaluationSupport.lean')
                or rel.parts[:2] == ('evidence', 'battery')
                or original.name in ('evaluation.stdout', kind + '-closure-check.json')):
            copy(original, destination / rel)
    for path in sorted(destination.rglob('*')):
        if path.is_file() and str(path.relative_to(destination)) not in {row['path'] for row in files}:
            files.append(dict(path=str(path.relative_to(destination)), role='evidence', sha256=P.sha256_file(str(path))))
    nv = dict(status='NONVACUOUS', file='evidence/nonvacuity.json',
              sha256=P.sha256_file(str(destination / 'evidence/nonvacuity.json')),
              witness_file=witness_file, witness_sha256=witness_sha)
    bound_floor = dict(floor, file='evidence/battery.json',
                       sha256=P.sha256_file(str(destination / 'evidence/battery.json')))
    record = CG.make_record(unit, spec, kind, files, type_sha, nv, bound_floor, env.pins(), created_utc)
    errors = P.validate_problem(record, str(destination))
    if errors:
        raise ValueError('Generated C package failed validation: ' + '; '.join(errors[:8]))
    P.write_json(str(destination / 'problem.json'), record)
    return record


def generate_unit(registry: str, parameters: dict, spec_directory: str, source_file: str,
                  ident: str, stage: str, output: str, work: str, env: L.LeanEnv,
                  created_utc: str, extra_witnesses: list[str] | None = None) -> dict:
    """Build both properties for one exact original unit; never author specs or attempt human proofs."""
    unit = CG.bind_registry(registry, parameters, ident)
    unit['source_fullfile_sha256'] = P.sha256_file(source_file)
    unit['parameters_sha256'] = S.package_digest(parameters)
    stage, work = Path(stage), Path(work)
    witnesses = prepare(unit, Path(registry), Path(spec_directory), Path(source_file), stage,
                        list(map(Path, extra_witnesses or [])))
    P.write_json(str(stage / 'reference/parameters.json'), parameters)
    if not witnesses:
        return dict(unit=unit, excluded='NO-REAL-WITNESS', properties=[])
    build = work / 'build'
    records = imports(unit, stage, build, env, 1800)
    evaluation = evaluate(unit, stage, build, witnesses, env, 1800)
    spec = read(Path(spec_directory) / 'problem.json')
    properties = []
    for kind in ('soundness', 'completeness'):
        status = CG.nonvacuity_status(evaluation['rows'], kind, evaluation['completed'])
        if status != 'NONVACUOUS':
            properties.append(dict(kind=kind, status=status, reason='joint-premise gate failed'))
            continue
        keys = ['valid_length', 'Constraints', 'Assumptions', 'CanonicalInputs']
        keys += ['SpecDomain'] if kind == 'soundness' else ['AcceptedData', 'BindInputs']
        selected = next(row for row in evaluation['rows'] if all(row.get(key) is True for key in keys))
        ref = reference(unit, kind, stage, build, env, 1800)
        floor = battery(unit, kind, stage, build, ref['reference_type_sha256'], env, work)
        status = CG.classify('NONVACUOUS', floor['attempts'], floor['checker_verdict'])
        if status not in ('OPEN', 'FACT'):
            properties.append(dict(kind=kind, status=status, reason='mechanical gate incomplete or closure unaccepted'))
            continue
        destination = Path(output) / (ident + '.' + kind)
        record = package_property(unit, kind, spec, stage, destination, selected, evaluation, floor,
                                  ref['reference_type_sha256'], env, created_utc)
        properties.append(dict(kind=kind, status=status, directory=str(destination), package_id=record['package_id'],
                               trivial=record['trivial'], original_model_sha256=unit['model_sha256']))
    result = dict(unit=unit, imports=records, evaluation=evaluation, properties=properties, proof_attempts=0)
    P.write_json(str(stage / 'RESULT.json'), result)
    return result


def main(argv=None) -> int:
    """Build a single pinned gnark unit against an existing accepted S package."""
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    for name in ('registry', 'parameters', 'spec', 'source-file', 'ident', 'stage', 'out', 'work', 'lean-env', 'utc'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--witness', action='append', default=[])
    args = parser.parse_args(argv)
    Runtime.install(str(Path(args.work) / 'processes'))
    L.set_lean_limits(slots=1, rss_mb=12000)
    result = generate_unit(args.registry, read(Path(args.parameters)), args.spec, args.source_file, args.ident,
                           args.stage, args.out, args.work, L.load_env(args.lean_env), args.utc, args.witness)
    print(json.dumps(dict(properties=result['properties'], proof_attempts=0)))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
