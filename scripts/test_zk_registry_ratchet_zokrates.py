"""Offline boundaries for the ZoKrates ratchet pipeline (synthetic source only)."""
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import ratchet_zokrates as K
from zk_registry import ratchet as RT, r1cs as R


class Boundaries(unittest.TestCase):
    def test_rebuild_matches_polynomials_not_factor_spelling(self):
        def r(c):return R.R1cs(17,32,4,1,1,0,4,c)
        a=r([([],[],[(1,1)]), ([(2,2)],[(3,1)],[(1,2)])])
        b=r([([(0,1)],[],[(1,1)]), ([(3,1)],[(2,1)],[(1,1)])])
        self.assertEqual(K._constraint_key(a),K._constraint_key(b))
        c=r([([],[],[(1,1)]), ([(2,1)],[(2,1)],[(1,1)])])
        self.assertNotEqual(K._constraint_key(a),K._constraint_key(c))

    def test_nonlinear_metric_is_shared_and_linear_is_free(self):
        r = R.R1cs(R.BN254_SCALAR, 32, 4, 1, 1, 0, 4,
                   [([(2, 1)], [(2, 1)], [(1, 1)]), ([(0, 1)], [(3, 1)], [(2, 1)])])
        self.assertEqual(K.counts_of(r), RT.counts_of(r))
        self.assertTrue(K.smaller(dict(nonlinear=2, linear=0, total=2), dict(nonlinear=1, linear=99, total=100)))

    def test_include_escape_and_missing_import_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            Path(d, "repo").mkdir()
            Path(d, "stdlib").mkdir()
            main = Path(d, "repo/main.zok")
            main.write_text('import "../../outside" as outside;\ndef main(field x) -> field { return x; }')
            with self.assertRaises(K.Reject):
                K.source_closure(main, Path(d))
            main.write_text('import "missing" as f;\ndef main(field x) -> field { return f(x); }')
            with self.assertRaises(K.Reject):
                K.source_closure(main, Path(d))

    def test_signature_mismatch_rejects_public_private_or_order_change(self):
        a = dict(prime="17", input_names=["main.x", "main.y"], output_names=["main.ret0"], input_types=["field", "u32"],
                 n_pub_in=1, n_prv_in=1)
        self.assertIsNone(K.io_mismatch(a, a))
        self.assertIsNotNone(K.io_mismatch(a, dict(a, n_pub_in=2, n_prv_in=0)))
        self.assertIsNotNone(K.io_mismatch(a, dict(a, input_names=list(reversed(a["input_names"])))))


if __name__ == "__main__":
    unittest.main()
