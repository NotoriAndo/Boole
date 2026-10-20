"""Digest-bound N2 axioms, gated solely by tracked measuring-stick conformance."""
from __future__ import annotations

import json
from .safe_fs import Path
import re
import time

BASE_AXIOMS = {'propext', 'Classical.choice', 'Quot.sound'}
HASH = re.compile(r'^[0-9a-f]{64}$')


class BindingUnavailable(ValueError):
    """A pristine binding probe was interrupted rather than disproving the binding."""


def native_evaluator_axiom(name: str) -> bool:
    """N1 is unconditional, including generated per-use compiled-evaluator axioms."""
    return bool(re.search(r'(^|\.)ofReduce\w*|(^|\.)trustCompiler\b|_native\.(?:native_decide|decide)\.ax_', name))


def conformance() -> dict:
    """Read only the fixed tracked authority file; never load fixtures or operator overrides."""
    return json.loads((Path(__file__).parent / 'data/measuring_stick_conformance_v2.json').read_text())


def validate(manifest: list[dict], actual: dict[str, dict], *, conformance_fixture: dict | None = None) -> dict:
    """Validate names, types and declaring-module bytes; fixtures enter only through this argument."""
    result = dict(verdict='PASS', reason=None, allowed_axioms=sorted(BASE_AXIOMS),
                  assurance='test-fixture' if conformance_fixture is not None else 'production')
    names = list(actual) + [row.get('name', '') for row in manifest if isinstance(row, dict)]
    if any(not isinstance(name, str) for name in names):
        return dict(result, verdict='INVALID', reason='n2-binding-invalid')
    if any(native_evaluator_axiom(name) for name in names):
        return dict(result, verdict='INVALID', reason='native-evaluator-axiom')
    data = conformance() if conformance_fixture is None else conformance_fixture
    allowed = set(BASE_AXIOMS)
    for row in manifest:
        fields = {'name', 'type_sha256', 'module', 'module_olean_sha256', 'measuring_stick'}
        if not isinstance(row, dict) or set(row) != fields:
            return dict(result, verdict='INVALID', reason='n2-binding-invalid')
        name = row['name']
        if any(not isinstance(row[key], str) or not row[key] for key in ('module', 'measuring_stick')):
            return dict(result, verdict='INVALID', reason='n2-binding-invalid')
        if not isinstance(name, str) or not name or name in allowed:
            return dict(result, verdict='INVALID', reason='n2-binding-invalid')
        if any(not isinstance(row[key], str) or not HASH.fullmatch(row[key])
               for key in ('type_sha256', 'module_olean_sha256')):
            return dict(result, verdict='INVALID', reason='n2-binding-invalid')
        qualified = data.get('measuring_sticks', {}).get(row['measuring_stick'], {})
        if qualified.get('status') != 'PASS':
            return dict(result, verdict='INVALID', reason='conformance-required')
        binding = actual.get(name)
        if binding is None or any(binding.get(key) != row[key]
                                  for key in ('type_sha256', 'module', 'module_olean_sha256')):
            return dict(result, verdict='INVALID', reason='n2-binding-mismatch')
        if qualified.get('modules', {}).get(row['module']) != row['module_olean_sha256']:
            return dict(result, verdict='INVALID', reason='n2-binding-mismatch')
        if binding.get('declared_in_module') is not True:
            return dict(result, verdict='INVALID', reason='n2-binding-mismatch')
        allowed.add(name)
    unexpected = sorted(set(actual) - allowed)
    if unexpected:
        return dict(result, verdict='INVALID', reason='disallowed-axiom')
    return dict(result, allowed_axioms=sorted(allowed))


def preflight(manifest: list[dict], *, conformance_fixture: dict | None = None) -> dict:
    """Check the authority and manifest shape before resolving pristine declarations."""
    proposed = {row['name']: dict(row, declared_in_module=True) for row in manifest
                if isinstance(row, dict) and isinstance(row.get('name'), str)}
    result = validate(manifest, proposed, conformance_fixture=conformance_fixture)
    result['provisional'] = True
    return result


def resolve(env, manifest: list[dict], build: str, sandbox, timeout: float) -> dict[str, dict]:
    """Read type and declaration provenance only from digest-matched pristine olean files."""
    from . import gates as G, lean_runner as L, closure as D, correctness_runtime as CR

    bindings = {}
    started = time.monotonic()
    for index, row in enumerate(manifest):
        module = row['module']
        if not re.fullmatch(r'[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*', module):
            continue
        relative = module.replace('.', '/') + '.olean'
        candidates = [Path(root, relative) for root in [build, *env.lean_path]]
        selected = next((path for path in candidates if path.is_file()
                         and D.digest(path) == row['module_olean_sha256']), None)
        if selected is None:
            continue
        directory = Path(build).parent / ('n2-' + str(index))
        directory.mkdir(parents=True, exist_ok=True)
        remaining = max(0.001, timeout - (time.monotonic() - started))
        guard = CR.runner(str(directory / 'processes'), remaining)
        with CR.receipt_report(guard, directory), \
                L.sandbox_context(sandbox, [str(selected), build, *env.lean_path, str(Path(G.REPLAY_TOOL).parent)],
                               [str(directory)], guard):
            run, post = G.run_replay(env, str(selected), row['name'], str(directory / 'replay.out'),
                                    [], [build], remaining)
        from .sandbox import boundary_failure
        stopped = boundary_failure(run)
        if stopped:
            if stopped['verdict'] == 'INVALID':
                raise L.BoundaryInvalid(stopped['reason'])
            raise BindingUnavailable(stopped['reason'])
        if D.resource_reason(run):
            raise BindingUnavailable('resource-unavailable')
        if run.rc != 0 or post.get('replay') != 'ok':
            continue
        own = any(item.get('kind') == 'axiom' and item.get('name') == row['name'] for item in post.get('consts', []))
        bindings[row['name']] = dict(module=module, module_olean_sha256=D.digest(selected),
                                    type_sha256=post.get('type_sha256'), declared_in_module=own,
                                    process_receipt=run.process_receipt)
    return bindings
