"""M2 wire-layout fixtures with A1-generated verdicts; no contract changes.

Wire hashes follow devnet-commit-reveal-v1 and the v1.3 draft. The Rust
consumer posts these exact bodies through the public fold before rebuilding.
"""
import copy
import hashlib
import tempfile
from pathlib import Path
from unittest.mock import patch
from test_zk_registry_m3_gate import descriptor
from zk_registry.m3_gate import adapter as A, codec as C


def digest(*parts):
    return hashlib.sha256(b''.join(parts)).digest()


def verdict_record(reply, descriptor, context):
    """Independent v1.3 VSTMT and first-verdict record encoding."""
    outcome = {'ACCEPT': 1, 'REJECT': 2, 'VOID': 3}[reply['outcome']]
    ext = bytes(reply['ext'])
    statement = (b'BOOLE-M3/VSTMT/v1' + context['chain_id'].to_bytes(8, 'big')
        + b''.join(bytes.fromhex(context[k]) for k in
                   ('problem_id', 'commit_hash', 'solver', 'env_id', 'export_digest'))
        + bytes.fromhex(reply['statement_id']) + bytes.fromhex(reply['gate_rule_digest'])
        + bytes.fromhex(descriptor['kernel_set_digest']) + bytes([outcome])
        + reply['reason'].to_bytes(2, 'big') + len(ext).to_bytes(2, 'big') + ext)
    return (b'\1' + bytes.fromhex(context['commit_hash']) + bytes([outcome])
        + reply['reason'].to_bytes(2, 'big') + bytes(4)
        + reply['fuel'].to_bytes(8, 'big') + digest(statement)).hex()


def open_layout_vectors():
    chain = (720926).to_bytes(8, 'big')
    schema = digest(b'EENV2-canonical-closure-v2')
    base = digest(b'BOOLE-DEVNET/BASE/v1')
    rows = []
    models = [('', b''), ('one', b'M'), ('forty', b'M' * 40),
              ('sixty-four', b'M' * 64), ('space', b' '), ('tilde', b'~'),
              ('printable-boundaries', b' ~' * 32)]
    for k, lane in [(0, 1), (1, 1), (4, 1), (4, 2)]:
        packages = [bytes([16 + i]) * 32 for i in range(k)]
        required = packages[:]
        env = digest(b'BOOLE-M2/ENV/v1', schema, base, bytes([k]), *packages).hex()
        inventory = C.canonical(dict(version=1, environment=env, base={}, library={}))
        package = C.canonical(dict(version=1, mode='full', problem='fixture', env_id=env,
            target=[['str', 'Fixture']], statement='a' * 64, reference_sha256='b' * 64,
            inventory_sha256=C.sha(inventory), fuel_cap=None))
        raw = b'broken-transport'
        root = digest(b'\0BOOLE-M2/POOL-LEAF/v2', chain, bytes([1, k]), *required,
                      b'\0', len(package).to_bytes(4, 'big'), digest(package), bytes([9]) * 32)
        mix = digest((4).to_bytes(8, 'big'), b'randao')
        seed = digest(b'BOOLE-M2/DRAW/v2', chain, b'\0', (1).to_bytes(4, 'big'), mix)
        problem = digest(b'BOOLE-M2/PROBLEM/v2', chain, root, bytes(4), seed, bytes(4))
        part = digest(b'\0BOOLE-M2/PART/v1', bytes([6]) * 20, bytes([7]) * 32,
                      bytes(2), digest(raw))
        for suffix, model in models:
            meta = bytes([1, len(model)]) + model + bytes([1, 0]) + bytes(36)
            body = (b'\1' + problem + bytes([6]) * 20 + bytes([7]) * 32 + b'\1'
                    + schema + base + bytes([k]) + b''.join(packages) + bytes([1, lane])
                    + len(raw).to_bytes(8, 'big') + digest(raw) + len(raw).to_bytes(4, 'big')
                    + b'\0\1' + part + bytes(4) + len(meta).to_bytes(2, 'big') + meta)
            variants = [(suffix or 'empty', body, True)]
            if suffix == '':
                q = 151 + 32 * k
                for name, at, size in [('deps', q + 80, 64), ('ext', q + 82, 1)]:
                    bad = bytearray(body); bad[at:at+2] = b'\0\1'
                    bad[q+86:q+86] = bytes(size)
                    variants.append((name, bytes(bad), False))
                for name, value in [('below-printable', 31), ('above-printable', 127)]:
                    bad = bytearray(body); bad[q+84:q+86] = (41).to_bytes(2, 'big')
                    bad[q+87] = 1; bad[q+88:q+88] = bytes([value])
                    variants.append((name, bytes(bad), False))
                bad = bytearray(body); bad[q+84:q+86] = (105).to_bytes(2, 'big')
                bad[q+87] = 65; bad[q+88:q+88] = b'M' * 65
                variants.append(('model-65', bytes(bad), False))
                variants += [('truncated', body[:-1], False), ('trailing', body+b'\0', False)]
            for name, wire, accepted in variants:
                commit = digest(b'BOOLE-M2/COMMIT/v2', chain, problem,
                    digest(b'BOOLE-M2/CANCEL/v1', bytes([8]) * 32), digest(b'BOOLE-M2/BODY/v1', wire))
                context = dict(chain_id=720926, problem_id=problem.hex(), commit_hash=commit.hex(),
                    solver=(bytes([6])*20).hex(), env_id=env, export_digest=C.sha(raw),
                    package_sha256=C.sha(package), leaf_kind=1, lane=lane, slot_fuel=10000)
                d = copy.deepcopy(descriptor()); d['artifact_table_sha256'] = C.sha(inventory)
                reply = None
                if accepted:
                    with tempfile.TemporaryDirectory() as tmp, patch.object(A, 'verify_tools'):
                        reply = A.run(raw, package, inventory, context, C.encode(d), C.identity(d),
                                      None, None, None, Path(tmp)/'work', public_counter=True)
                    A.decode_reply(C.canonical(reply), d, context)
                rows.append(dict(name=f'k{k}-lane{lane}-{name}', accepted=accepted,
                    model_hex=model.hex(), packages=[p.hex() for p in packages],
                    required=[p.hex() for p in required], lane=lane, body_hex=wire.hex(),
                    package_hex=package.hex(), bundle_hex=raw.hex(), inventory_hex=inventory.hex(),
                    context=context, descriptor_hex=C.encode(d).hex(),
                    reply_hex=None if reply is None else C.canonical(reply).hex(),
                    record_hex=verdict_record(reply, d, context)
                        if accepted and lane == 1 else None))
    import json
    return (json.dumps(dict(cases=rows), indent=2)+'\n').encode()
