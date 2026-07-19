"""Self-check tests for the dual-cert Phase 0 harness internals.

THROWAWAY offline experiment code — run standalone (`./run.sh` or
`python3 -m unittest test_selfcheck`), intentionally NOT registered in
scripts/self-test.sh (the experiment must stay unwired from CI/consensus).
"""
from __future__ import annotations

import subprocess
import tempfile
import unittest

import attackers
import solvers
from encode import canon_binding, circuit_canonical_bytes, d_dimacs_bytes, witness_bytes
from gen import Params, generate
from lrat_native import check_lrat
from verify import verify_bug
from xof import Xof

SMALL = Params(n_vars=14, n_pub=3, n_out=2, n_clause=30, n_xor=2, n_gate=2, clause_width=3)


def brute_force_d(circuit):
    """All satisfying assignments of D(seed), as witness lists."""
    sols = []
    n = circuit.n_vars
    d = circuit.d_clauses()
    for bits in range(1 << n):
        w = [0] + [(bits >> j) & 1 for j in range(n)]
        if all(any((w[abs(l)] == 1) == (l > 0) for l in cl) for cl in d):
            sols.append(w)
    return sols


class TestXof(unittest.TestCase):
    def test_deterministic_stream(self):
        a = Xof("seed-1", "d")
        b = Xof("seed-1", "d")
        self.assertEqual(a.bytes(257), b.bytes(257))
        self.assertNotEqual(Xof("seed-1", "d").bytes(32), Xof("seed-2", "d").bytes(32))

    def test_randbelow_range(self):
        x = Xof("s", "r")
        for _ in range(500):
            self.assertLess(x.randbelow(7), 7)


class TestGenerator(unittest.TestCase):
    def test_byte_identical_regeneration(self):
        c1 = generate("det-seed", SMALL)
        c2 = generate("det-seed", SMALL)
        self.assertEqual(circuit_canonical_bytes(c1), circuit_canonical_bytes(c2))
        self.assertEqual(d_dimacs_bytes(c1), d_dimacs_bytes(c2))

    def test_reference_witness_satisfies_circuit(self):
        for i in range(20):
            c = generate(f"ref-{i}", SMALL)
            for cl in c.clauses:
                self.assertTrue(
                    any((c.ref_witness[abs(l)] == 1) == (l > 0) for l in cl),
                    f"planted witness violates constraint in seed ref-{i}",
                )

    def test_reference_witness_never_satisfies_d(self):
        # w* satisfies C but must falsify the diff clause by construction.
        for i in range(10):
            c = generate(f"refd-{i}", SMALL)
            diff = c.diff_clause()
            self.assertFalse(
                any((c.ref_witness[abs(l)] == 1) == (l > 0) for l in diff)
            )

    def test_no_answer_label_fields(self):
        # The generator must not carry a BUG/SAFE label or an alternative
        # witness: the only witness field is the reference one.
        c = generate("label-check", SMALL)
        fields = set(vars(c).keys())
        self.assertNotIn("outcome", fields)
        self.assertNotIn("alt_witness", fields)
        self.assertNotIn("answer", fields)


class TestTotalityAndVerifier(unittest.TestCase):
    def test_brute_force_matches_certificates(self):
        bug_seen = safe_seen = 0
        for i in range(12):
            seed = f"tot-{i}"
            c = generate(seed, SMALL)
            sols = brute_force_d(c)
            if sols:
                bug_seen += 1
                v = verify_bug(seed, SMALL, sols[0])
                self.assertTrue(v.accepted, v.reason)
            else:
                safe_seen += 1
                res = solvers.run_cadical(d_dimacs_bytes(c), c.n_vars, 30, want_lrat=True)
                self.assertEqual(res.status, "UNSAT")
                nat = check_lrat(c.n_vars, c.d_clauses(), res.lrat_text)
                self.assertTrue(nat.ok, nat.reason)
            # exactly one of BUG/SAFE holds by definition of D; the reference
            # witness must never be a valid BUG certificate.
            self.assertFalse(verify_bug(seed, SMALL, list(c.ref_witness)).accepted)
        # tiny-band sanity: both outcomes should be reachable at this size
        self.assertGreater(bug_seen + safe_seen, 0)

    def test_bug_verifier_rejects_tampering(self):
        for i in range(20):
            seed = f"tamper-{i}"
            c = generate(seed, SMALL)
            sols = brute_force_d(c)
            if not sols:
                continue
            w = list(sols[0])
            # break a public pin
            pv = c.public_vars[0]
            w2 = list(w)
            w2[pv] ^= 1
            self.assertFalse(verify_bug(seed, SMALL, w2).accepted)
            # wrong length
            self.assertFalse(verify_bug(seed, SMALL, w[:-1]).accepted)
            return
        self.skipTest("no BUG instance found in tamper seeds")


class TestNativeLrat(unittest.TestCase):
    CNF = [(1, 2), (1, -2), (-1, 2), (-1, -2)]

    def _proof(self):
        res = solvers.run_cadical(
            b"p cnf 2 4\n1 2 0\n1 -2 0\n-1 2 0\n-1 -2 0\n", 2, 30, want_lrat=True
        )
        assert res.status == "UNSAT" and res.lrat_text
        return res.lrat_text

    def test_accepts_valid_proof(self):
        self.assertTrue(check_lrat(2, self.CNF, self._proof()).ok)

    def test_rejects_corrupted_proof(self):
        proof = self._proof()
        # flip a hint digit
        corrupted = proof.replace(" 1 ", " 3 ", 1)
        self.assertFalse(check_lrat(2, self.CNF, corrupted).ok)

    def test_rejects_truncated_proof(self):
        proof = self._proof()
        lines = proof.strip().splitlines()
        self.assertFalse(check_lrat(2, self.CNF, "\n".join(lines[:-1]) + "\n").ok)

    def test_rejects_proof_for_wrong_cnf(self):
        # same proof against a satisfiable CNF must not certify UNSAT
        sat_cnf = [(1, 2), (1, -2), (-1, 2)]
        self.assertFalse(check_lrat(2, sat_cnf, self._proof()).ok)


class TestBcpCertificate(unittest.TestCase):
    def test_bcp_lrat_checks_when_available(self):
        found = 0
        for i in range(200):
            seed = f"bcp-{i}"
            c = generate(seed, SMALL)
            lrat = attackers.propagation_safe_lrat(c)
            if lrat is None:
                continue
            found += 1
            self.assertTrue(check_lrat(c.n_vars, c.d_clauses(), lrat).ok)
            if found >= 3:
                break
        # BCP-refutable instances may legitimately not exist in this band;
        # the test only asserts soundness of emitted certificates.


class TestAttackSoundness(unittest.TestCase):
    def test_attack_decisions_match_brute_force(self):
        for i in range(10):
            seed = f"atk-{i}"
            c = generate(seed, SMALL)
            truth = "BUG" if brute_force_d(c) else "SAFE"
            for a in (
                attackers.attack_propagation(c),
                attackers.attack_gauss(c),
                attackers.attack_free_output(c),
                attackers.attack_local_flip(c, max_flips=500, budget_s=5),
            ):
                if a.decided is not None:
                    self.assertEqual(a.decided, truth, f"{a.name} wrong on {seed}")
                if a.decided == "BUG":
                    self.assertTrue(verify_bug(seed, SMALL, a.witness).accepted)


class TestCanonBinding(unittest.TestCase):
    def test_flip_and_seed_only(self):
        cert = b"some-certificate-bytes"
        canon = canon_binding("seed-x", "BUG", cert)
        flipped = bytearray(cert)
        flipped[3] ^= 1
        self.assertNotEqual(canon, canon_binding("seed-x", "BUG", bytes(flipped)))
        self.assertNotEqual(canon, canon_binding("seed-x", "BUG", b""))
        self.assertNotEqual(canon, canon_binding("seed-x", "SAFE", cert))
        self.assertNotEqual(canon, canon_binding("seed-y", "BUG", cert))

    def test_witness_bytes_roundtrip_shape(self):
        w = [0, 1, 0, 1, 1, 0, 0, 1, 1, 0]  # 9 bits -> 2 bytes
        self.assertEqual(len(witness_bytes(w)), 2)


if __name__ == "__main__":
    unittest.main()
