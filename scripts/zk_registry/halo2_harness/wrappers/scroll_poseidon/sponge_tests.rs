//! Boole H1 wrapper for scroll-tech/poseidon-circuit `SpongeChip` (chip row; child module of `hash::tests`;
//! `cfg(test)`): the repository's own test circuit `TestCircuit::<Pow5Chip<Fr, 3, 2>>` over the hash table of
//! `tests::poseidon_hash_circuit_impl` (messages [1, 2] and [2, 3]), TEST_STEP 32, K = 8.  It loads its own table
//! (no instance cells, no result cells); the run measures the chip's layout.

use halo2_proofs::halo2curves::bn256::Fr;

use super::{PoseidonHashTable, TestCircuit};
use crate::boole_h1::{drive_cols, Spec};
use crate::poseidon::Pow5Chip;

#[test]
fn boole_h1_scroll_sponge_chip() {
    drive_cols(
        &Spec { name: "SpongeChip", k: 8,
                call: "hash::tests::TestCircuit::<Pow5Chip<Fr, 3, 2>> on PoseidonHashTable { inputs: [[1, 2], [2, 3]] }",
                rule: "repo-test", provenance: "poseidon-circuit/src/hash.rs tests::poseidon_hash_circuit_impl (K = 8)",
                input_notes: &[] },
        |_rng, _s| {
            let table = PoseidonHashTable {
                inputs: std::vec![[Fr::from(1u64), Fr::from(2u64)], [Fr::from(2u64), Fr::from(3u64)]],
                ..Default::default()
            };
            (TestCircuit::<Pow5Chip<Fr, 3, 2>>::new(table), std::vec![])
        },
    );
}
