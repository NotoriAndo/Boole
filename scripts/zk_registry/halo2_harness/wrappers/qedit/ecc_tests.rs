//! Boole H1 wrapper for the qed-it/halo2 `EccChip` row (a whole chip; child module of `ecc::tests`; `cfg(test)`):
//! the repository's own chip test circuit `MyEccCircuit` (every `EccChip` instruction, with the ZSA additions, as
//! `tests::ecc_chip_with_zsa_additions` runs it, error cases off), K = 13.  It witnesses its own values: it has no
//! instance cells and no result cells; the run measures the chip's layout.

use super::MyEccCircuit;
use crate::boole_h1::{drive_cols, Spec};
use crate::utilities::lookup_range_check::PallasLookupRangeCheckConfig;

#[test]
fn boole_h1_qedit_ecc_chip() {
    drive_cols(
        &Spec { name: "Chip:EccChip", k: 13,
                call: "ecc::tests::MyEccCircuit::<PallasLookupRangeCheckConfig>::new(false, true) (every EccChip \
                       instruction, ZSA additions)",
                rule: "repo-test", provenance: "halo2_gadgets/src/ecc.rs tests::ecc_chip_with_zsa_additions (K = 13)",
                input_notes: &[] },
        |_rng, _s| (MyEccCircuit::<PallasLookupRangeCheckConfig>::new(false, true), std::vec![]),
    );
}
