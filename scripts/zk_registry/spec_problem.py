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


def authoring_prerequisites(problem: dict) -> list[str]:
    """Distinguish a transparent OPEN inventory from a fully bound authorship request."""
    files = {row['path']: row['sha256'] for row in problem['files']}
    missing = []
    if problem['standard'].get('authority_pending') is True:
        missing.append('authoritative-public-standard')
    for key in ('reference', 'mutants', 'protocol'):
        item = problem[key]
        if not item.get('file') or files.get(item.get('file')) != item['sha256']:
            missing.append(key + '-file-binding')
    if problem['reference'].get('executable_reference_binding') == 'pending':
        missing.append('independent-executable-reference')
    if problem['mutants'].get('fixed_protocol_only') is True:
        missing.append('function-specific-fixed-mutants')
    if not problem['parameters']:
        missing.append('parameter-scope')
    return missing


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
