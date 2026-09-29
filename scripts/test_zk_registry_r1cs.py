#!/usr/bin/env python3
"""R1CS / .sym reader and oracle tests on a real circom 2.2.3 fixture (no circom or node needed)."""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import r1cs as R  # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-circuit"
P = R.BN254_SCALAR


def toy():
    return R.read_r1cs(str(FIX / "toy.r1cs")), R.read_sym(str(FIX / "toy.sym"))


def toy_witness() -> list[int]:
    return [int(v) for v in json.loads((FIX / "witness.json").read_text())["witness"]]


class R1csReaderTests(unittest.TestCase):
    def test_header_and_constraints_of_the_fixture(self) -> None:
        r, _ = toy()
        self.assertEqual(r.prime, P)
        self.assertEqual(r.prime_name, "bn128")
        self.assertEqual((r.n_wires, r.n_pub_out, r.n_pub_in, r.n_prv_in), (7, 2, 0, 3))
        self.assertEqual(r.n_constraints, 2)
        self.assertEqual(len(r.wire_to_label), 7)

    def test_encode_round_trips_byte_for_byte(self) -> None:
        data = (FIX / "toy.r1cs").read_bytes()
        self.assertEqual(R.encode_r1cs(R.parse_r1cs(data)), data)

    def test_malformed_files_are_rejected(self) -> None:
        data = (FIX / "toy.r1cs").read_bytes()
        for bad in (b"xxxx" + data[4:], data[:-3], data + b"\0", data[:4] + b"\x02" + data[5:]):
            with self.assertRaises(R.R1csFormatError):
                R.parse_r1cs(bad)
        r = R.parse_r1cs(data)
        r.constraints[0][0].append((99, 1))
        with self.assertRaises(R.R1csFormatError):
            R.parse_r1cs(R.encode_r1cs(r))

    def test_main_io_wires_follow_the_header_and_sym_names(self) -> None:
        r, syms = toy()
        io = R.main_io_wires(r, syms)
        self.assertEqual(io.outputs, [1, 2])
        self.assertEqual(io.inputs, [3, 4, 5])
        self.assertEqual(io.output_names, ["main.c", "main.d"])
        self.assertEqual(io.input_names, ["main.a", "main.b[0]", "main.b[1]"])
        self.assertEqual(R.wire_names(syms, r.n_wires)[0], "one")
        self.assertEqual(R.signal_base("main.b[1][2]"), "main.b")

    def test_main_io_wires_rejects_a_subcomponent_signal_in_the_io_range(self) -> None:
        r, syms = toy()
        bad = [R.SymEntry(e.label, e.wire, e.component, "main.sub.x" if e.wire == 3 else e.name) for e in syms]
        with self.assertRaises(R.R1csFormatError):
            R.main_io_wires(r, bad)


class OracleTests(unittest.TestCase):
    def test_real_generator_witness_satisfies_the_fixture(self) -> None:
        r, _ = toy()
        w = toy_witness()
        self.assertTrue(R.satisfies(r, w))
        self.assertEqual(R.violated(r, w), [])

    def test_oracle_rejects_bad_constant_wire_unreduced_values_and_violations(self) -> None:
        r, _ = toy()
        w = toy_witness()
        self.assertFalse(R.satisfies(r, [2] + w[1:]))
        self.assertFalse(R.satisfies(r, w[:1] + [w[1] + P] + w[2:]))
        bad = list(w)
        bad[1] = (bad[1] + 1) % P          # c no longer equals t + 2 b[1] - 1
        self.assertFalse(R.satisfies(r, bad))
        self.assertEqual(R.violated(r, bad), [1])
        with self.assertRaises(ValueError):
            R.violated(r, w[:-1])

    def test_unconstrained_output_is_visible_in_constraint_sites(self) -> None:
        r, _ = toy()
        sites = R.constraint_sites(r)
        self.assertEqual(sites[2], set())      # main.d is assigned with <-- only
        self.assertTrue(sites[1])


if __name__ == "__main__":
    unittest.main()
