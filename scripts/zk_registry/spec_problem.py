"""Specification authorship requests and locally accepted, evidence-bound specifications.

S packages are not proof-checker packages. Local D/M/C acceptance does not implement a
public challenge window, reward, or issuance service.
"""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path

from . import package as P

SCHEMA_VERSION = 'zk-registry-spec-problem/v1'
GENERATOR_NAME = 'boole-zk-registry-spec'


def generator_files() -> list[str]:
    """Registry-owned specification generator sources, including its schema."""
    return ['spec_problem.py', 'schema/spec_problem.schema.json', 'data/spec_history_m_2026_10_10.json']


def generator_info() -> dict:
    """Hash the specification pipeline and shared serialization/admission code."""
    paths = generator_files() + ['package.py', 'jsonschema_lite.py']
    digest = hashlib.sha256()
    for name in sorted(paths):
        digest.update(name.encode() + b'\0' + P.sha256_file(str(Path(__file__).parent / name)).encode() + b'\n')
    return dict(name=GENERATOR_NAME, version='1.0', sources_sha256=digest.hexdigest())


def package_digest(problem: dict) -> str:
    """Digest the complete canonical metadata, including its pinned file manifest."""
    raw = json.dumps(problem, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
    return hashlib.sha256(raw).hexdigest()


def request_identity(request: dict) -> str:
    """Identify a spec request independently of submission status and creation time."""
    bound = {key: request[key] for key in ('family', 'standard', 'parameters', 'reference', 'mutants', 'protocol')}
    return 'S/' + request['family'] + '/' + package_digest(bound)


def record(request: dict, accepted: dict | None = None) -> dict:
    """Create OPEN or ACCEPTED-LOCAL metadata; never synthesize a missing specification."""
    problem = copy.deepcopy(request)
    problem.update(schema_version=SCHEMA_VERSION, kind='S', package_id=request_identity(request),
                   status='ACCEPTED-LOCAL' if accepted is not None else 'OPEN',
                   public_challenge=dict(status='pending', implemented=False),
                   generator=dict(name=GENERATOR_NAME, version='1.0'))
    if accepted is not None:
        problem['specification'] = copy.deepcopy(accepted)
    return problem


def authoring_prerequisites(problem: dict, directory: str | None = None) -> list[str]:
    """Distinguish a transparent OPEN inventory from a fully bound authorship request."""
    files = {row['path']: row['sha256'] for row in problem['files']}
    missing = []
    if problem['standard'].get('authority_pending') is True:
        missing.append('authoritative-public-standard')
    is_open = problem['status'] == 'OPEN'
    if is_open:
        standard = problem['standard']
        if not standard.get('file') or files.get(standard.get('file')) != standard['sha256']:
            missing.append('standard-file-binding')
        parameters = problem['parameters']
        if (not parameters.get('file') or not parameters.get('sha256')
                or files.get(parameters.get('file')) != parameters['sha256']):
            missing.append('parameter-scope-file-binding')
        if parameters.get('scope_complete') is not True:
            missing.append('parameter-scope')
    for key in ('reference', 'mutants', 'protocol'):
        item = problem[key]
        if not item.get('file') or files.get(item.get('file')) != item['sha256']:
            missing.append(key + '-file-binding')
    if (problem['reference'].get('executable_reference_binding') == 'pending'
            or (is_open and problem['reference'].get('executable') is not True)):
        missing.append('independent-executable-reference')
    mutants = problem['mutants']
    if (mutants.get('fixed_protocol_only') is True
            or (is_open and (mutants.get('family') != problem['family']
                            or not mutants.get('code') or not mutants.get('code_sha256')
                            or files.get(mutants.get('code')) != mutants['code_sha256']))):
        missing.append('function-specific-fixed-mutants')
    if not problem['parameters'] and 'parameter-scope' not in missing:
        missing.append('parameter-scope')
    if is_open and directory is not None:
        missing += authoring_content_errors(problem, directory)
    return missing


def authoring_content_errors(problem: dict, directory: str) -> list[str]:
    """Check actual OPEN parameter/mutant scope, not merely ready-looking metadata flags."""
    if P._bound_file_errors(problem['files'], directory):
        return ['request-file-content']
    root, errors = Path(directory), []
    files = {row['path']: row['sha256'] for row in problem['files']}
    try:
        data = json.loads((root / problem['parameters']['file']).read_text())
        units = data.get('units') if isinstance(data, dict) else None
        valid = (isinstance(units, list) and bool(units) and type(data.get('count')) is int
                 and data['count'] == len(units) and isinstance(data.get('function'), str) and bool(data['function']))
        if valid:
            for unit in units:
                if not isinstance(unit, dict):
                    valid = False
                    break
                parameter = unit.get('field_parameters')
                if (not isinstance(parameter, dict) or not isinstance(parameter.get('modulus'), str)
                        or not parameter['modulus'].isdigit() or int(parameter['modulus']) <= 1
                        or type(parameter.get('bits_per_limb')) is not int or parameter['bits_per_limb'] < 1
                        or type(parameter.get('nb_limbs')) is not int or parameter['nb_limbs'] < 1
                        or not isinstance(unit.get('reference_domain'), str) or not unit['reference_domain']
                        or not isinstance(unit.get('reference_observation'), str) or not unit['reference_observation']
                        or unit.get('reference_family') != data['function']
                        or not isinstance(unit.get('original_metadata_file'), str)
                        or not isinstance(unit.get('original_metadata_sha256'), str)
                        or files.get(unit.get('original_metadata_file')) != unit.get('original_metadata_sha256')
                        or not isinstance(unit.get('wrapper_file'), str)
                        or not isinstance(unit.get('wrapper_sha256'), str)
                        or files.get(unit.get('wrapper_file')) != unit.get('wrapper_sha256')):
                    valid = False
                    break
        if not valid:
            errors.append('parameter-scope-content')
    except (OSError, ValueError, KeyError, TypeError):
        errors.append('parameter-scope-content')
    try:
        data = json.loads((root / problem['mutants']['file']).read_text())
        rows = data.get('mutants') if isinstance(data, dict) else None
        if (not isinstance(rows, list) or data.get('family') != problem['family']
                or type(data.get('count')) is not int or data['count'] != problem['mutants']['count']
                or len(rows) != data['count']
                or any(not isinstance(row, dict) or type(row.get('index')) is not int
                       or row['index'] != index or row.get('family') != data.get('reference_family')
                       or not isinstance(row.get('detail'), str) or not row['detail']
                       for index, row in enumerate(rows, 1))):
            errors.append('function-specific-mutant-content')
    except (OSError, ValueError, KeyError, TypeError):
        errors.append('function-specific-mutant-content')
    return errors


def historical_binding_errors(problem: dict) -> list[str]:
    """Accept only the exact retained, approved Task M history; this is not a new-spec gate service."""
    history = json.loads((Path(__file__).parent / 'data/spec_history_m_2026_10_10.json').read_text())
    expected = history['specifications'].get(problem['family'])
    if expected is None:
        return ['specification family is not in the approved local history']
    accepted = problem['specification']
    observed = dict(specification_sha256=accepted['sha256'],
                    gate_sha256={key: gate['sha256'] for key, gate in accepted['gate_evidence'].items()},
                    reference_sha256=problem['reference']['sha256'], mutants_sha256=problem['mutants']['sha256'],
                    protocol_sha256=problem['protocol']['sha256'], tested_parameters=accepted['tested_parameters'])
    return [] if observed == expected else ['specification or gate fingerprints differ from approved local history']


def write_package(directory: str, problem: dict) -> None:
    """Validate a prepared file closure before writing its canonical package record."""
    errors = P.validate_problem(problem, directory)
    if errors:
        raise ValueError('Invalid specification package: ' + '; '.join(errors[:8]))
    path = Path(directory) / 'problem.json'
    if path.exists():
        raise FileExistsError(path)
    P.write_json(str(path), problem)
