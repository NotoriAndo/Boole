"""Embed the canonical fuel driver in the isolated --run artifact."""
from pathlib import Path
import argparse

ROOT = Path(__file__).resolve().parent / 'lean'
BEGIN = '-- BEGIN GENERATED DET3 FUEL\n'
END = '-- END GENERATED DET3 FUEL\n'


def render():
    core = (ROOT / 'Det3Fuel.lean').read_text()
    core = core[core.index('namespace GateFuel'):]
    path = ROOT / 'GateReplay.lean'
    source = path.read_text()
    before, rest = source.split(BEGIN)
    _, after = rest.split(END)
    return path, before + BEGIN + core + END + after


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path, expected = render()
    if args.check:
        if path.read_text() != expected:
            raise SystemExit('fuel driver artifact differs from canonical source')
    else:
        path.write_text(expected)
