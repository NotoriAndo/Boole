"""Independent stdlib vectors for the staged M3a wire formats (no node or gate)."""
from pathlib import Path
import hashlib
import json
import sys

ROOT = Path(__file__).resolve().parents[2]


def sha(data):
    return hashlib.sha256(data).digest()


def uint(number, size):
    return number.to_bytes(size, 'big')


def minimal(number):
    return uint(number, (number.bit_length() + 7) // 8)


def rlp(value):
    if isinstance(value, list):
        body, base = b''.join(rlp(x) for x in value), 0xc0
    else:
        body, base = value, 0x80
        if len(body) == 1 and body[0] < 0x80:
            return body
    if len(body) < 56:
        return bytes([base + len(body)]) + body
    length = minimal(len(body))
    return bytes([base + 55 + len(length)]) + length + body


def deposit(domain, origin, index, recipient, mint, value, gas, data):
    source = sha(domain + origin + uint(index, 32))
    fields = [source, bytes.fromhex('deaddeaddeaddeaddeaddeaddeaddeaddead0001'),
              recipient, minimal(mint), minimal(value), minimal(gas), b'', data]
    return (b'\x7e' + rlp(fields)).hex()


def fork_list(kind, entries):
    entries = sorted(entries)
    assert (kind == 2) == bool(entries), 'only QUARANTINE has a nonempty list'
    assert all(a[0] < b[0] for a, b in zip(entries, entries[1:])), 'duplicate commit hash'
    assert all(len(ch) == 32 and disposition in (1, 2) for ch, disposition in entries)
    return b'BOOLE-M3/FORK-LIST/v1'+uint(len(entries), 4)+b''.join(ch+bytes([d]) for ch, d in entries)


fork_vectors = []
for kind in range(4):
    entries = [(sha(b'quarantine B'), 2), (sha(b'quarantine A'), 1)] if kind == 2 else []
    preimage = fork_list(kind, entries)
    epoch, gate = kind, sha(b'fork gate'+bytes([kind]))
    body = uint(epoch, 4)+gate+bytes([kind])+sha(preimage)
    assert len(body) == 69
    fork_vectors.append(dict(epoch=epoch, kind=kind, gate_rule_digest=gate.hex(),
        entries=[dict(commit_hash=ch.hex(), disposition=d) for ch,d in sorted(entries)],
        list_preimage=preimage.hex(), list_digest=sha(preimage).hex(),
        body=body.hex(), event=(b'\x08'+body).hex()))
# The independent generator also refuses every forbidden kind/list combination.
for kind, entries in [(0, [(bytes(32), 1)]), (1, [(bytes(32), 1)]), (3, [(bytes(32), 2)]),
                      (2, []), (2, [(bytes(32), 1), (bytes(32), 2)])]:
    try:
        fork_list(kind, entries)
    except AssertionError:
        pass
    else:
        raise AssertionError('invalid fork list accepted')

origin, height, first = bytes([3])*32, 7, 41
records = []
for i in range(129):
    raw = bytes([1 if i < 128 else 2]) + sha(b'candidate' + uint(i, 4))
    raw += bytes([1]) + uint(0, 2) + uint(2, 4) + uint(i % 11, 8)
    raw += sha(b'statement' + uint(i, 4))
    assert len(raw) == 80
    records.append(raw)
verdicts = []
for j in range(2):
    batch = records[j*128:(j+1)*128]
    data = b'm3vd' + uint(height, 8) + bytes([j, 2]) + uint(first+j*128, 8)
    data += uint(len(batch), 2) + b''.join(batch)
    verdicts.append(deposit(b'BOOLE-S1/VERDICT', origin, j,
                           bytes.fromhex('42000000000000000000000000000000000000b3'),
                           0, 0, 1_000_000, data))
statements = []
for lane in (1, 2):
    ext = b'' if lane == 1 else bytes([1, 2])+uint(3, 8)+uint(21, 8)+bytes([13])*32+bytes([14])*32
    preimage = b'BOOLE-M3/VSTMT/v1'+uint(720926, 8)
    preimage += bytes([1])*32+bytes([2])*32+bytes([3])*20
    preimage += b''.join(bytes([n])*32 for n in range(4, 9))
    preimage += bytes([1])+uint(0, 2)+uint(len(ext), 2)+ext
    assert len(preimage) == 274+len(ext)
    statements.append(dict(lane=lane, ext=ext.hex(), preimage=preimage.hex(), digest=sha(preimage).hex()))
declarations = [('zeta', 2, sha(b'normalized-definition')), ('alpha', 1, sha(b'normalized-theorem'))]
manifest = b'BOOLE-M3/MANIFEST/v1'+uint(len(declarations), 4)
for name, kind, digest in sorted(declarations):
    encoded = name.encode()
    manifest += uint(len(encoded), 2)+encoded+bytes([kind])+digest
reward = dict(reward_id=9, solver=(bytes([17])*20).hex(), amount=str(1000*10**18), gas=100_000)
reward['deposit'] = deposit(b'BOOLE-S1/REWARD', origin, reward['reward_id'], bytes.fromhex(reward['solver']),
                            int(reward['amount']), int(reward['amount']), reward['gas'], b'')
profiles = []
legacy = json.loads((ROOT/'fixtures/devnet-m2/config-golden-v1.1.json').read_text())
for index, row in enumerate(legacy['profiles']):
    # The frozen canonical JSON is an input, never rewritten. Parsing its byte
    # string preserves declaration order independently of the Rust serializer.
    config = json.loads(bytes.fromhex(row['canonical_json_hex']))
    slots = len(config['slot_table'])
    config['m3'] = dict(k=8 if index == 0 else 300, rate=1000, cost_fixed=100,
        fuel_slot=[10000]*slots,
        verdict_sink=list(bytes.fromhex('42000000000000000000000000000000000000b3')),
        q_reward=16, reward_gas=100000, maturity=64 if index == 0 else 50400,
        reward=[1000*10**18]*slots, mint_enabled=index == 0,
        notice=0 if index == 0 else 7200, open_licenses=[1], bridge=False,
        native_is_reward_unit=False,
        genesis_gate_rule_digest=list(sha(b'BOOLE-M2/BODY/v1BOOLE-M3/STUB/v1')))
    canonical = json.dumps(config, separators=(',', ':')).encode()
    digest = sha(b'BOOLE-M2/CONFIG/v2'+canonical)
    empty_root = sha(b'BOOLE-M2/ROOT/v3'+digest+uint(1, 8)+sha(uint(1, 8))+sha(b''))
    profiles.append(dict(name=row['name'], canonical_json_hex=canonical.hex(),
                         config_digest=digest.hex(), empty_root_1=empty_root.hex(),
                         policy_root_1=sha(b'BOOLE-M2/ROOT/v3'+digest+uint(1, 8)+sha(uint(1, 8))+
                             sha(b'\x08'+uint(1,4)+sha(b'policy gate')+b'\x03'+sha(fork_list(3,[])))).hex()))

# Independent R2/R8 settlement event stream for the public-fold scenario:
# pool at 1, draw at 4, reveal at 5, commits at 6/7, completion at 17,
# ACCEPT plus the earlier candidate's CANCEL at 19. No Rust output is read.
chain = uint(720926, 8)
package = b'generator-v1'
pool_root = sha(b'\0BOOLE-M2/POOL-LEAF/v2'+chain+b'\x02\0\0'+uint(len(package), 4)+sha(package)+bytes([9])*32)
seed = sha(b'BOOLE-M2/DRAW/v2'+chain+b'\0'+uint(1, 4)+sha(uint(4, 8)+b'randao'))
problem = sha(b'BOOLE-M2/PROBLEM/v2'+chain+pool_root+uint(0, 4)+seed+uint(0, 4))
base_config = legacy['profiles'][0]['config']
data = bytes([42])*8
part_root = sha(b'\0BOOLE-M2/PART/v1'+bytes([6])*20+bytes([7])*32+uint(0, 2)+sha(data))
body = b'\1'+problem+bytes([6])*20+bytes([7])*32+b'\1'
body += bytes(base_config['schema_id'])+bytes(base_config['base_root'])+b'\0\1\1'
body += uint(8, 8)+sha(data)+uint(8, 4)+uint(1, 2)+part_root+uint(0, 4)+uint(40, 2)
body += bytes([1, 0, 1, 0])+bytes(36)
commit = sha(b'BOOLE-M2/COMMIT/v2'+chain+problem+sha(b'BOOLE-M2/CANCEL/v1'+bytes([8])*32)+sha(b'BOOLE-M2/BODY/v1'+body))
record = b'\1'+commit+b'\1'+uint(0, 2)+uint(0, 4)+uint(0, 8)+sha(b'injected statement')
events = b'\x05'+uint(0, 8)+record+b'\x02'+problem+commit+b'\x06'
events += b'\x01'+uint(0, 8)+uint(0, 8)+bytes([7, 0x84])
events += b'\x02'+problem+commit+b'\x07'
events += bytes([3, 0])+uint(1, 4)+bytes([0xc5])+uint(1, 4)+uint(0, 4)+problem
events += b'\x06'+uint(0, 8)+problem+commit+bytes([6])*20+uint(1000*10**18, 16)+uint(23, 8)+b'\1'
events += b'\x07'+uint(1, 8)+problem+commit+b'\1'
settlement = dict(problem_id=problem.hex(), commit_hash=commit.hex(), event_hex=events.hex(), event_digest=sha(events).hex())
vector = dict(schema='boole-devnet-m3a-system-v1.3-draft',
              provenance=dict(generator='scripts/devnet/generate_m3_vectors.py', generator_sha256=sha(Path(__file__).read_bytes()).hex()),
              origin=origin.hex(), height=height, first_index=first,
              records=[x.hex() for x in records], verdict_deposits=verdicts, reward=reward,
              profiles=profiles, forks=fork_vectors, settlement=settlement, statements=statements, manifest=dict(declarations=[dict(name=n,kind=k,decl_hash=h.hex()) for n,k,h in declarations],
                                                 preimage=manifest.hex(), digest=sha(manifest).hex()))
target = ROOT/'fixtures/devnet-m3/system-v1.3.json'
encoded = json.dumps(vector, indent=2)+'\n'
if sys.argv[1:] == ['--check']:
    assert target.read_text() == encoded, 'M3 golden differs'
elif not sys.argv[1:]:
    target.parent.mkdir(exist_ok=True)
    target.write_text(encoded)
else:
    raise SystemExit('usage: generate_m3_vectors.py [--check]')
