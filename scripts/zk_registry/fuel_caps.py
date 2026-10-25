"""Digest-bound budgets for measured references on a certified own-delta path."""
from __future__ import annotations

import hashlib
import json
import re
import platform
from dataclasses import dataclass
from .safe_fs import Path

KERNELS = {'lean', 'nanoda'}
STRUCTURAL = dict(lean=262144000000000, nanoda=34359738368000000)


class FuelCapError(ValueError):
    pass


def digest(record: dict) -> str:
    return hashlib.sha256(json.dumps(record, sort_keys=True, separators=(',', ':'),
                                     ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def resolve(record: dict, authority: dict) -> dict:
    """Resolve only catalogued reference budgets.

    ``own_declarations`` binds the calibration reference's checked declaration
    manifest. A candidate may use a different own delta within the same budget.
    """
    try:
        if (not isinstance(authority, dict) or type(authority['version']) is not int
                or authority['version'] != 1 or not isinstance(authority['factors'], dict)
                or set(authority['factors']) != KERNELS
                or any(type(n) is not int or n != 4 for n in authority['factors'].values())):
            raise FuelCapError('unsupported-fuel-authority')
        if not isinstance(record, dict) or set(record) != {'cap', 'sha256'}:
            raise FuelCapError('invalid-fuel-binding')
        cap = record['cap']
        if (not isinstance(cap, dict)
                or set(cap) != {'version', 'reference', 'problem', 'environment', 'statement',
                                'own_declarations', 'units'}
                or type(cap['version']) is not int or cap['version'] != 1
                or not _hash(record['sha256']) or digest(cap) != record['sha256']):
            raise FuelCapError('invalid-fuel-binding')
        for field in ('reference', 'environment', 'statement', 'own_declarations'):
            if not _hash(cap[field]):
                raise FuelCapError('invalid-fuel-binding')
        if not isinstance(cap['problem'], str) or not 0 < len(cap['problem'].encode()) <= 1024:
            raise FuelCapError('invalid-fuel-binding')
        reference = authority['references'].get(cap['reference'])
        if not isinstance(reference, dict):
            raise FuelCapError('unmeasured-reference')
        if (reference.get('path') != 'certified-own-delta/v1'
                or reference.get('eligible') is not True
                or digest(reference) != cap['reference']):
            raise FuelCapError('ineligible-reference')
        for field in ('problem', 'environment', 'statement', 'own_declarations'):
            if reference[field] != cap[field]:
                raise FuelCapError('reference-binding-mismatch')
        if not _hash(reference['reference_export']):
            raise FuelCapError('invalid-reference')
        for units in (cap['units'], reference['units']):
            if (not isinstance(units, dict) or set(units) != KERNELS
                    or any(type(n) is not int or not 0 < n < 2**64 for n in units.values())):
                raise FuelCapError('invalid-fuel-units')
        for kernel in KERNELS:
            value = reference['units'][kernel] * authority['factors'][kernel]
            if value != cap['units'][kernel] or value > STRUCTURAL[kernel]:
                raise FuelCapError('invalid-fourfold-cap')
        return dict(units=dict(cap['units']), problem=cap['problem'], environment=cap['environment'],
                    statement=cap['statement'], own_declarations=cap['own_declarations'],
                    reference=cap['reference'], reference_export=reference['reference_export'])
    except FuelCapError:
        raise
    except (KeyError, TypeError, AttributeError, OverflowError, UnicodeError, ValueError) as error:
        raise FuelCapError('invalid-fuel-binding') from error


def _hash(value) -> bool:
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value) is not None


@dataclass(frozen=True)
class Budget:
    lean: int
    nanoda: int
    environment: str
    state: str
    state_sha256: str
    reference: str


def load_authority() -> dict:
    """The descriptor pins executable admission data; the operator supplies no caps."""
    from . import lean_runner as L
    root = Path(__file__).parent / 'data'
    entry = L.production_descriptor()['fuel_cap_authority']
    if entry['file'] != 'fuel_cap_authority_v1.json':
        raise FuelCapError('fuel-authority-unavailable')
    path = root / entry['file']
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != entry['sha256']:
        raise FuelCapError('fuel-authority-unavailable')
    authority = json.loads(raw)
    if authority['factors'] != entry['factors']:
        raise FuelCapError('fuel-authority-unavailable')
    return authority


def bind(problem: dict, statement: str, root: str | None) -> Budget:
    """Bind the actual checked problem and provisioned immutable certified state."""
    from .closure import digest as file_digest
    try:
        authority = load_authority()
        resolved = resolve(problem['fuel_cap'], authority)
        if resolved['problem'] != problem['package_id'] or resolved['statement'] != statement:
            raise FuelCapError('fuel-problem-mismatch')
        environment = authority['environments'][resolved['environment']]
        if (environment['certified_kernels'] != ['lean', 'nanoda']
                or environment['eligible'] is not True or root is None):
            raise FuelCapError('certified-environment-unavailable')
        profile = environment['profiles'][platform.system() + '-' + platform.machine()]
        if not _hash(profile['state_sha256']) or not _hash(profile['certificate_sha256']):
            raise FuelCapError('certified-environment-unavailable')
        # The operator chooses the provisioned root. The authority fixes its
        # environment suffix and state digest; no submission path chooses it.
        state = Path(root).resolve(strict=True) / resolved['environment'] / 'state.olean'
        if not state.is_file() or file_digest(state) != profile['state_sha256']:
            raise FuelCapError('certified-environment-unavailable')
        return Budget(resolved['units']['lean'], resolved['units']['nanoda'],
                      resolved['environment'], str(state.resolve(strict=True)),
                      profile['state_sha256'], resolved['reference'])
    except FuelCapError:
        raise
    except (KeyError, TypeError, AttributeError, OSError, ValueError, RuntimeError) as error:
        raise FuelCapError('fuel-cap-unavailable') from error
