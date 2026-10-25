"""Trusted native nanoda outcomes must survive the Comparator boundary."""
import json
from pathlib import Path
import tempfile
import unittest
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent))

from zk_registry import closure as C
from zk_registry.sandbox import Sandbox


class NativeBuildIdentity(unittest.TestCase):
    def test_cargo_proxy_is_rejected_before_any_tool_executes(self):
        from unittest.mock import patch
        from zk_registry import build_nanoda_fuel as builder
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            proxy = root / 'rustup'
            proxy.write_bytes(b'generated proxy; must never execute')
            (root / 'cargo').symlink_to(proxy)
            (root / 'rustc').symlink_to(proxy)
            argv = ['builder', '--nanoda-source', str(root), '--cargo',
                    str(root / 'cargo'), '--out', str(root / 'out'),
                    '--local-sidecar', str(root / 'identity.json')]
            with patch.object(sys, 'argv', argv), patch.object(
                    builder.subprocess, 'check_output',
                    side_effect=AssertionError('tool executed before identity validation')):
                with self.assertRaisesRegex(ValueError, 'native Cargo'):
                    builder.main()


class NanodaStatus(unittest.TestCase):
    def compare(self, code, events):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / 'comparator'
            rows = [dict(version=1, **event) for event in events]
            rows.append(dict(version=1, stage='final', rc=code))
            payload = ''.join(json.dumps(row) + '\n' for row in rows)
            binary.write_text('#!/usr/bin/python3\nimport os,sys\nfrom pathlib import Path\n'
                              'Path(os.environ["COMPARATOR_STATUS_FILE"]).write_text('
                              + repr(payload) + ')\nsys.exit(' + str(code) + ')\n')
            binary.chmod(0o755)
            left, right = root / 'left', root / 'right'
            left.write_bytes(b'sealed')
            right.write_bytes(b'sealed')
            tools = C.Tools(str(binary), '/usr/bin/true', '/usr/bin/true', '/usr',
                            None, lean_replay='pinned-native-replay')
            return C.compare_exports(str(left), str(right), 'T.t', tools,
                                     Sandbox.fixture(), str(root / 'work'), 5, [])

    @staticmethod
    def debit():
        return dict(stage='nanoda-fuel', kernel='nanoda', debit=64, checked=1,
                    calls=2, depth=1, composed_depth=2)

    def test_both_native_debits_are_required_for_acceptance(self):
        accepted = [dict(stage=s, result='accept') for s in ('lean', 'nanoda', 'quotient')]
        lean = dict(stage='fuel', kernel='lean', debit=64, checked=1, calls=1, depth=1)
        for events, verdict in [([lean, *accepted], 'ERROR'),
                                ([self.debit(), lean, *accepted], 'PASS')]:
            with self.subTest(verdict=verdict):
                self.assertEqual(self.compare(0, events)['verdict'], verdict)

    def test_native_cap_and_fault_need_matching_typed_records(self):
        for code, reason in [(3, 'resource-cap'), (4, 'tool-fault:replay')]:
            row = self.compare(code, [self.debit(), dict(stage='policy', reason=reason)])
            self.assertEqual((row['verdict'], row['reason']), ('INVALID', reason))
        row = self.compare(4, [dict(stage='policy', reason='tool-fault:replay')])
        self.assertEqual(row['verdict'], 'ERROR')

    def test_malformed_native_debits_fail_closed(self):
        for field, bad in [('kernel', 'lean'), ('debit', True), ('calls', -1),
                           ('depth', 2**64), ('composed_depth', '2')]:
            debit = self.debit()
            debit[field] = bad
            with self.subTest(field=field):
                row = self.compare(3, [debit, dict(stage='policy', reason='resource-cap')])
                self.assertEqual(row['verdict'], 'ERROR')

    def test_transport_exit_never_establishes_a_cap(self):
        row = self.compare(70, [self.debit(), dict(stage='policy', reason='resource-cap')])
        self.assertEqual(row['verdict'], 'ERROR')


if __name__ == '__main__':
    unittest.main()
