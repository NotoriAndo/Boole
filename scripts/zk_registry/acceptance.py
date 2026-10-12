"""Catalog-bound v2 reference exports, separate from historical package schemas."""
from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
import re

from . import axiom_policy as A, closure as D

HASH = re.compile(r'^[0-9a-f]{64}$')
DATA = Path(__file__).parent / 'data'


@dataclass(frozen=True)
class RootBinding:
    """A trusted caller's validated catalog root, not a solver-supplied environment dictionary."""

    package_id: str
    manifest: dict
    manifest_sha256: str


def read(directory: str, problem: dict, *, required: bool = False,
         conformance_fixture: dict | None = None) -> RootBinding | None:
    """Verify the sidecar against metadata, tracked authorities and sealed Challenge bytes."""
    root = Path(directory).resolve()
    path = root / 'Acceptance.json'
    if not path.is_file():
        if required:
            raise ValueError('reference-export-unpinned')
        return None
    if path.is_symlink():
        raise ValueError('reference-binding-invalid')
    data = json.loads(path.read_text())
    fields = {'schema_version', 'package_id', 'metadata_sha256', 'environment_descriptor_sha256',
              'tools_descriptor_sha256', 'forbidden_version', 'challenge_export', 'reference_type_sha256',
              'measuring_stick_axioms', 'migration'}
    if not isinstance(data, dict) or set(data) != fields or data['schema_version'] != 2:
        raise ValueError('reference-binding-invalid')
    if data['package_id'] != problem['package_id'] or data['forbidden_version'] != 2:
        raise ValueError('reference-binding-invalid')
    if data['metadata_sha256'] != D.digest(root / 'problem.json'):
        raise ValueError('reference-binding-invalid')
    for key, file in (('environment_descriptor_sha256', 'acceptance_environment_v2.json'),
                      ('tools_descriptor_sha256', 'acceptance_tools_v2.json')):
        if data[key] != D.digest(DATA / file):
            raise ValueError('reference-binding-invalid')
    export = data['challenge_export']
    if not isinstance(export, dict) or set(export) != {'path', 'sha256'}:
        raise ValueError('reference-binding-invalid')
    if export['path'] != 'acceptance/Challenge.ndjson':
        raise ValueError('reference-binding-invalid')
    target = root / export['path']
    if target.is_symlink() or not target.is_file() or target.resolve().parent != root / 'acceptance':
        raise ValueError('reference-binding-invalid')
    if export['sha256'] != D.digest(target):
        raise ValueError('challenge-export-mismatch')
    if not isinstance(data['reference_type_sha256'], str) or not HASH.fullmatch(data['reference_type_sha256']):
        raise ValueError('reference-binding-invalid')
    if not isinstance(data['measuring_stick_axioms'], list):
        raise ValueError('n2-binding-invalid')
    policy = A.preflight(data['measuring_stick_axioms'], conformance_fixture=conformance_fixture)
    if policy['verdict'] != 'PASS':
        raise ValueError(policy['reason'])
    return RootBinding(problem['package_id'], data, D.digest(path))


def public(binding: RootBinding) -> dict:
    """Expose only reference and authority digests, not migration diagnostics or proof bytes."""
    return dict(schema_version=2, manifest_sha256=binding.manifest_sha256,
                challenge_export_sha256=binding.manifest['challenge_export']['sha256'],
                environment_descriptor_sha256=binding.manifest['environment_descriptor_sha256'],
                tools_descriptor_sha256=binding.manifest['tools_descriptor_sha256'], forbidden_version=2)
