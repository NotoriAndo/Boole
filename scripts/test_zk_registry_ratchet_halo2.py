"""Offline concrete-layout ratchet contracts, not statements about real projects."""

import sys
import copy
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import ratchet_halo2 as H
from zk_registry import halo2_ir as IR
import test_zk_registry_halo2 as Toy


class CostOrder(unittest.TestCase):
    def test_overlay_cannot_call_or_remove_native_harness(self):
        """A chip edit cannot access the exporter or remove the protected wrapper declaration."""
        with tempfile.TemporaryDirectory() as root:
            p = Path(root, 'repo/chip.rs')
            p.parent.mkdir()
            p.write_text('fn chip() {}\nmod boole_h1;\n')
            H.guard_harness_references(root, {'chip.rs': b'fn chip() { let x = 1; }\nmod boole_h1;\n'})
            for text in [b'fn chip() {}\n', b'fn chip() { boole_export::dump(); }\nmod boole_h1;\n']:
                with self.assertRaises(H.RT.Reject):
                    H.guard_harness_references(root, {'chip.rs': text})

    def test_sparse_zero_instance_does_not_change_model_layout(self):
        """Sparse zero instance encoding is harmless, but a changed fixed value is not."""
        a = Toy.adder_doc()
        b = copy.deepcopy(a)
        b['instance'] = b['instance'][1:]
        b = IR.parse(b)
        ma, mb = IR.flatten(a), IR.flatten(b)
        self.assertEqual(ma.sha256(), mb.sha256())
        self.assertEqual(H.layout_sha(a, ma), H.layout_sha(b, mb))
        b['fixed'][0][2] = Toy.hx(2)
        b = IR.parse(b)
        mb = IR.flatten(b)
        self.assertNotEqual(H.layout_sha(a, ma), H.layout_sha(b, mb))

    def test_malformed_histogram_and_domain_fail_closed(self):
        """Invalid domain sizes and cumulative degree records fail the cost contract."""
        a = dict(
            domain_rows=64,
            advice_columns=4,
            fixed_columns=2,
            gate_instances=10,
            copies=8,
            lookups=0,
            degree_ge={"2": 6, "3": 2},
            max_degree=3,
        )
        self.assertEqual(H.cost_errors(a), [])
        for b in [
            dict(a, domain_rows=63),
            dict(a, degree_ge={"NaN": 1}),
            dict(a, degree_ge={"2": 1, "3": 2}),
            dict(a, degree_ge={"2": 6}),
        ]:
            self.assertTrue(H.cost_errors(b))

    def test_no_scalar_tradeoff_can_hide_growth(self):
        """An informational priced sum never conceals growth of a vector component."""
        a = dict(
            domain_rows=64,
            advice_columns=4,
            fixed_columns=2,
            gate_instances=10,
            copies=8,
            lookups=0,
            degree_ge={"2": 6},
        )
        b = dict(a, advice_columns=3, lookups=1)
        self.assertFalse(H.cost_smaller(a, b))
        self.assertTrue(H.cost_smaller(a, dict(a, advice_columns=3)))
        self.assertFalse(H.cost_smaller(a, a))

    def test_degree_histogram_component_is_not_omitted(self):
        """A new higher-degree component prevents an otherwise smaller candidate."""
        a = dict(
            domain_rows=64,
            advice_columns=4,
            fixed_columns=2,
            gate_instances=10,
            copies=8,
            lookups=0,
            degree_ge={"2": 6},
        )
        self.assertFalse(H.cost_smaller(a, dict(a, gate_instances=9, degree_ge={"2": 6, "3": 1})))


if __name__ == "__main__":
    unittest.main()
