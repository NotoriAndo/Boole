"""Reproduce the v1.3 reset genesis; refuse a previously allocated verdict sink."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SINK = '0x42000000000000000000000000000000000000b3'

def genesis():
    old = json.loads((ROOT/'fixtures/devnet-m2/l2-genesis-v1.2.json').read_text())
    assert SINK.lower() not in {key.lower() for key in old['alloc']}, 'U5: sink already allocated'
    old['alloc'][SINK] = dict(balance='0x0', code='0x00')
    return old

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = ROOT/'fixtures/devnet-m3/l2-genesis-v1.3.json'
    data = json.dumps(genesis(), indent=2)+'\n'
    if args.check:
        assert path.read_text() == data, 'v1.3 genesis differs from the tracked generator'
    else:
        path.write_text(data)
    print('v1.3: U5 free address verified, STOP verdict sink installed')
