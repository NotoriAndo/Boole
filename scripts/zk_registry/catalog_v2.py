"""Re-pin validated immutable packages into a separate acceptance-v2 catalog root."""
from __future__ import annotations

import copy
import json
from zk_registry import safe_fs as FS
from .safe_fs import Path
import shutil

from . import acceptance as A, check as C, closure as D, lean_runner as L, package as P
from . import service as S


def repin_package(source: str, destination: str, reference: dict, *, old_reference_type_sha256: str) -> dict:
    """Copy original artifacts, promote only judge pins, and bind the pristine reference export."""
    old, new = Path(source).resolve(), Path(destination).resolve()
    if new.exists() or new.is_relative_to(old):
        raise ValueError('re-pin requires a separate new destination')
    if reference.get('verdict') != 'PASS' or reference.get('reference_type_sha256') != old_reference_type_sha256:
        raise ValueError('unchanged passing reference required')
    if D.digest(reference['path']) != reference['sha256']:
        raise ValueError('reference export digest differs')
    problem = json.loads((old / 'problem.json').read_text())
    errors = P.validate_problem(problem, str(old))
    if errors:
        raise ValueError('invalid original package: ' + '; '.join(errors[:3]))
    old_metadata = D.digest(old / 'problem.json')
    old_env = copy.deepcopy(problem['env'])
    pins = L.production_descriptor()
    new.mkdir(parents=True)
    for row in S.file_manifest(str(old), package=True):
        path = new / row['path']
        path.parent.mkdir(parents=True, exist_ok=True)
        FS.copyfile(old / row['path'], path)
        if D.digest(path) != row['sha256']:
            raise ValueError('source changed while re-pinning')
    problem['env'].update({key: pins[key] for key in ('lean', 'mathlib', 'lake_manifest_sha256', 'packages')})
    if 'checker' in problem:
        problem['checker'].update(forbidden_tokens=list(C.FORBIDDEN_LABELS),
                                  reference_type_sha256=reference['reference_type_sha256'])
        if problem['schema_version'] == P.CORRECTNESS_GNARK_SCHEMA_VERSION:
            problem['checker']['forbidden_version'] = C.FORBIDDEN_VERSION
    P.write_json(str(new / 'problem.json'), problem)
    errors = P.validate_problem(problem, str(new))
    if errors:
        raise ValueError('invalid promoted package: ' + '; '.join(errors[:3]))
    export = new / 'acceptance/Challenge.ndjson'
    export.parent.mkdir()
    FS.copyfile(reference['path'], export)
    manifest = dict(schema_version=2, package_id=problem['package_id'], metadata_sha256=D.digest(new / 'problem.json'),
                    environment_descriptor_sha256=D.digest(A.DATA / 'acceptance_environment_v2.json'),
                    tools_descriptor_sha256=D.digest(A.DATA / 'acceptance_tools_v2.json'),
                    forbidden_version=C.FORBIDDEN_VERSION,
                    challenge_export=dict(path='acceptance/Challenge.ndjson', sha256=D.digest(export)),
                    reference_type_sha256=reference['reference_type_sha256'], measuring_stick_axioms=[],
                    migration=dict(old_metadata_sha256=old_metadata, old_environment=old_env,
                                   old_reference_type_sha256=old_reference_type_sha256,
                                   floor='carried over, not rerun', battery='carried over, not rerun',
                                   assurance=reference.get('assurance')))
    with (new / 'Acceptance.json').open('x') as stream:
        json.dump(manifest, stream, indent=2, sort_keys=True)
    A.read(str(new), problem, required=True)
    if D.digest(old / 'problem.json') != old_metadata:
        raise ValueError('original metadata changed')
    return manifest
