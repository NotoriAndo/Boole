"""Prepare a fresh node-local v1.3 stub profile from an authenticated build artifact.

No node is started, old recording changed, dependency installed or network used.
The descriptor commits the executable SHA-256 for this host profile; distribute
these exact descriptor/config bytes and the same binary to every replica.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def sha(data):
    return hashlib.sha256(data).digest()

def prepare(binary, output):
    binary = binary.resolve(strict=True)
    executable = sha(binary.read_bytes())
    system = {'Darwin': 'darwin', 'Linux': 'linux'}[platform.system()]
    arch = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    host = system+'-'+arch
    implementation = hashlib.sha256(b'BOOLE-M3/ADAPTER-SOURCE/v1')
    for name in ['lib.rs','stub.rs','process.rs','codec.rs','raw_frame.rs','rebuild.rs','supervisor.rs','cache.rs']:
        source = (ROOT/'crates/boole-gate-supervisor/src'/name).read_bytes()
        implementation.update(len(name).to_bytes(2, 'big'))
        implementation.update(name.encode())
        implementation.update(len(source).to_bytes(8, 'big'))
        implementation.update(source)
    descriptors = []
    for fixed in (False, True):
        d = dict(adapter='boole-devnet-stub-v1', revision=1, fixed=fixed,
                 generator=True, profiles=[host],
                 kernel_set_digest=list(sha(b'BOOLE-M3/STUB-KERNELS/v1')),
                 binaries={host: list(executable)}, implementation_sha256=list(implementation.digest()))
        canonical = json.dumps(d, separators=(',', ':')).encode()
        descriptors.append((d, sha(b'BOOLE-M3/GATE-DESCRIPTOR/v1'+canonical)))
    legacy = json.loads((ROOT/'fixtures/devnet-m2/config-golden-v1.1.json').read_text())
    config = legacy['profiles'][0]['config']
    slots = len(config['slot_table'])
    config['m3'] = dict(k=8, rate=1000, cost_fixed=100, fuel_slot=[10000]*slots,
        verdict_sink=list(bytes.fromhex('42000000000000000000000000000000000000b3')),
        q_reward=16, reward_gas=100000, maturity=64, reward=[1000*10**18]*slots,
        mint_enabled=True, notice=0, open_licenses=[1], bridge=False,
        native_is_reward_unit=False, genesis_gate_rule_digest=list(descriptors[0][1]))
    output.mkdir(exist_ok=False)
    def write(name, value):
        (output/name).write_text(json.dumps(value, indent=2)+'\n')
    write('m3-config.json', config)
    write('gates.json', [dict(descriptor=d, binary=str(binary), binary_sha256=list(executable))
                         for d, _ in descriptors])
    write('forks.json', dict(rows=[dict(epoch=0, height=0, gate_rule_digest=list(descriptors[0][1]),
                                      kind='GENESIS', list=[])]))
    write('artifact.json', dict(schema='boole-m3-stub-install-v1', host=host,
          binary_sha256=executable.hex(), gate_rule_digests=[h.hex() for _, h in descriptors]))
    (output/'artifacts').mkdir()
    for text in [b'BOOLE-DEVNET/BASE/v1', b'BOOLE-DEVNET/EVM/v1', b'BOOLE-DEVNET/RISCV/v1']:
        (output/'artifacts'/('0x'+sha(text).hex())).write_bytes(text)
    return dict(host=host, gate_rule_digest=descriptors[0][1].hex(), output=str(output))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gate-binary', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.gate_binary, args.out)))
