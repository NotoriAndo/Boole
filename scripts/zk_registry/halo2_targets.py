"""Row plan of the halo2 DET generator: which ledger rows are built through which wrapper, and the terminal status
(with its reason) of every row that is not.

A row is planned as

* ``Wrapper(adapter, test)``: an adapter (one pinned repository build, see :data:`ADAPTERS`) and the wrapper test
  that instantiates the row's instruction / chip / circuit; or
* ``Static(status, reason)``: NOT-APPLICABLE (not a constraint system, or a configuration / witnessing step whose
  result is a prover-chosen value by design) or NO-INSTANTIATION (generic over the computation, or an instance
  needs artifacts the generator does not produce), decided from the row's unit, path and symbol by the rules
  below.  Every rule is a fixed function of the ledger row; nothing is decided per item by hand outside this file.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

from . import halo2_harness as HH


@dataclass
class Wrapper:
    adapter: str
    test: str                    # full test path in the crate's test binary
    name: str                    # target name the wrapper writes (export directory)


@dataclass
class Static:
    status: str
    reason: str


@dataclass
class Adapter:
    repo_id: str
    crate_dir: str               # crate with the wrappers, relative to the checkout
    package: str
    halo2_line: str
    proofs_dir: str              # instrumented halo2_proofs, relative to the checkout ("" = patched copy)
    toolchain: str               # installed rustup toolchain used
    toolchain_reason: str
    injections: list = field(default_factory=list)
    release: str = ""
    kind: str = "crate-tests"    # "crate-tests": wrappers are #[test]s of the crate; "vendored-bin": a minimal crate
    vendor: dict = field(default_factory=dict)
    patch: dict = field(default_factory=dict)    # crates.io halo2_proofs to instrument: {"crate", "version"}
    git_patch: dict = field(default_factory=dict)  # [patch] git entries to point at an instrumented checkout
    driver_field: str = ""       # replaces the driver's Pasta block for adapters over other fields


I = HH.Injection
ZCASH_GADGETS = Adapter(
    repo_id="zcash/halo2", crate_dir="halo2_gadgets", package="halo2_gadgets", halo2_line="zcash-0.3",
    proofs_dir="halo2_proofs", toolchain="1.60.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain.toml channel 1.60.0 (installed)", release="halo2_gadgets-0.5.0",
    injections=[
        I("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs", "src/lib.rs", "#[cfg(test)]\npub(crate) mod boole_h1;"),
        I("zcash_gadgets/ecc_chip.rs", "src/ecc/chip/boole_h1.rs", "src/ecc/chip.rs", "#[cfg(test)]\nmod boole_h1;"),
        I("zcash_gadgets/pow5.rs", "src/poseidon/pow5/boole_h1.rs", "src/poseidon/pow5.rs",
          "#[cfg(test)]\nmod boole_h1;"),
        I("zcash_gadgets/sinsemilla_tests.rs", "src/sinsemilla/tests/boole_h1.rs", "src/sinsemilla.rs", "mod boole_h1;",
          "pub(crate) mod tests {"),
        I("zcash_gadgets/merkle_tests.rs", "src/sinsemilla/merkle/tests/boole_h1.rs", "src/sinsemilla/merkle.rs",
          "mod boole_h1;", "pub mod tests {"),
        I("zcash_gadgets/cond_swap.rs", "src/utilities/cond_swap/boole_h1.rs", "src/utilities/cond_swap.rs",
          "#[cfg(test)]\nmod boole_h1;"),
        I("zcash_gadgets/lookup_range_check.rs", "src/utilities/lookup_range_check/boole_h1.rs",
          "src/utilities/lookup_range_check.rs", "#[cfg(test)]\nmod boole_h1;"),
    ])

DARKFI_FILES = [(f"repo:src/zk/gadget/{m}.rs", f"src/zk/gadget/{m}.rs") for m in
                ("arithmetic", "cond_select", "is_equal", "less_than", "native_range_check", "small_range_check")]
DARKFI_GADGETS = Adapter(
    repo_id="darkrenaissance/darkfi", crate_dir="", package="boole_h1_darkfi", halo2_line="zcash-0.3",
    proofs_dir="", toolchain="1.95.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain.toml channel stable (installed stable 1.95.0)", kind="vendored-bin",
    vendor={"fork": "parazyd/halo2", "fork_proofs": "halo2_proofs", "cargo_toml": "darkfi/Cargo.toml.in",
            "lockfile": "Cargo.lock", "bin": "boole-h1",
            "files": DARKFI_FILES + [("darkfi/lib.rs", "src/lib.rs"), ("darkfi/main.rs", "src/main.rs"),
                                     ("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs"),
                                     ("darkfi/chips.rs", "src/boole_h1_darkfi.rs")]})

ORCHARD = Adapter(
    repo_id="zcash/orchard", crate_dir="", package="orchard", halo2_line="zcash-0.3", proofs_dir="",
    toolchain="1.95.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain.toml channel 1.85.1 is not installed; installed stable 1.95.0 used (deviation)",
    patch={"crate": "halo2_proofs", "version": "0.3.5"},
    injections=[
        I("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs", "src/lib.rs", "#[cfg(test)]\npub(crate) mod boole_h1;"),
        I("orchard/add_chip.rs", "src/circuit/gadget/add_chip/boole_h1.rs", "src/circuit/gadget/add_chip.rs",
          "#[cfg(test)]\nmod boole_h1;"),
        I("orchard/circuit_tests.rs", "src/circuit/tests/boole_h1.rs", "src/circuit.rs", "mod boole_h1;",
          "\nmod tests {"),
    ])

QEDIT_HALO2 = Adapter(
    repo_id="qed-it/halo2", crate_dir="halo2_gadgets", package="halo2_gadgets", halo2_line="zcash-0.3",
    proofs_dir="halo2_proofs", toolchain="1.60.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain.toml channel 1.60.0 (installed)",
    injections=[
        I("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs", "src/lib.rs", "#[cfg(test)]\npub(crate) mod boole_h1;"),
        I("qedit/ecc_tests.rs", "src/ecc/tests/boole_h1.rs", "src/ecc.rs", "mod boole_h1;", "pub(crate) mod tests {"),
    ])
QEDIT_ORCHARD = Adapter(
    repo_id="qed-it/orchard", crate_dir="", package="orchard", halo2_line="zcash-0.3", proofs_dir="",
    toolchain="1.95.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain.toml channel 1.88 is not installed; installed stable 1.95.0 used (deviation)",
    git_patch={"fork": "zcash/halo2@828fab66a721", "url": "https://github.com/zcash/halo2",
               "crates": ["halo2_proofs", "halo2_gadgets"]},
    injections=[
        I("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs", "src/lib.rs", "#[cfg(test)]\npub(crate) mod boole_h1;"),
        I("qedit/vanilla_tests.rs", "src/circuit/circuit_vanilla/tests/boole_h1.rs", "src/circuit/circuit_vanilla.rs",
          "mod boole_h1;", "\nmod tests {"),
        I("qedit/zsa_tests.rs", "src/circuit/circuit_zsa/tests/boole_h1.rs", "src/circuit/circuit_zsa.rs",
          "mod boole_h1;", "\nmod tests {"),
    ])

SCROLL_POSEIDON = Adapter(
    repo_id="scroll-tech/poseidon-circuit", crate_dir="poseidon-circuit", package="poseidon-circuit",
    halo2_line="scroll-v1", proofs_dir="", toolchain="1.95.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain nightly-2024-07-07 is not installed; installed stable 1.95.0 used (deviation)",
    git_patch={"fork": "scroll-tech/halo2@2f5ee1040e3c", "url": "https://github.com/scroll-tech/halo2.git",
               "crates": ["halo2_proofs"], "spec": r'branch\s*=\s*"v1\.0"',
               "note": "the repository pins scroll-tech/halo2 by branch v1.0 and has no Cargo.lock; the generator uses "
                       "2f5ee104, the v1.0 commit scroll-tech/mpt-circuit locks (deviation: no pin at the repository)"},
    injections=[
        I("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs", "src/lib.rs", "#[cfg(test)]\npub(crate) mod boole_h1;"),
        I("scroll_poseidon/pow5.rs", "src/poseidon/pow5/boole_h1.rs", "src/poseidon/pow5.rs",
          "#[cfg(test)]\nmod boole_h1;"),
        I("scroll_poseidon/septidon.rs", "src/poseidon/septidon/instruction/boole_h1.rs",
          "src/poseidon/septidon/instruction.rs", "#[cfg(test)]\nmod boole_h1;"),
        I("scroll_poseidon/sponge_tests.rs", "src/hash/tests/boole_h1.rs", "src/hash.rs", "mod boole_h1;",
          "\nmod tests {"),
    ],
    driver_field="pub type Fp = halo2_proofs::halo2curves::bn256::Fr;")

SCROLL_MPT = Adapter(
    repo_id="scroll-tech/mpt-circuit", crate_dir="", package="boole_h1_mpt", halo2_line="scroll-v1", proofs_dir="",
    toolchain="1.95.0-aarch64-apple-darwin",
    toolchain_reason="rust-toolchain nightly-2023-10-27 is not installed; installed stable 1.95.0 used (deviation)",
    kind="vendored-bin", driver_field="pub type Fp = halo2_proofs::halo2curves::bn256::Fr;",
    vendor={"fork": "scroll-tech/halo2@2f5ee1040e3c", "fork_proofs": "halo2_proofs", "cargo_toml": "scroll_mpt/Cargo.toml.in",
            "lockfile": "Cargo.lock", "bin": "boole-h1",
            "files": [("repo:src/traces/existing_account_balance_update.json",
                       "src/traces/existing_account_balance_update.json"),
                      ("scroll_mpt/lib.rs", "src/lib.rs"), ("scroll_mpt/main.rs", "src/main.rs"),
                      ("zcash_gadgets/boole_h1.rs", "src/boole_h1.rs"), ("scroll_mpt/mpt.rs", "src/boole_h1_mpt.rs")]})

ADAPTERS = {"zcash-gadgets": ZCASH_GADGETS, "darkfi-gadgets": DARKFI_GADGETS, "orchard": ORCHARD,
            "qedit-halo2": QEDIT_HALO2, "qedit-orchard": QEDIT_ORCHARD, "scroll-poseidon": SCROLL_POSEIDON,
            "scroll-mpt": SCROLL_MPT}
_SCROLL_POSEIDON = {"Pow5Chip": "poseidon::pow5::boole_h1::boole_h1_scroll_pow5_chip",
                    "SeptidonChip": "poseidon::septidon::instruction::boole_h1::boole_h1_scroll_septidon_chip",
                    "SpongeChip": "hash::tests::boole_h1::boole_h1_scroll_sponge_chip"}
_DARKFI = {"Chip:ArithChip", "Chip:ConditionalSelectChip", "Chip:IsEqualChip", "Chip:AssertEqualChip",
           "Chip:LessThanChip", "Chip:NativeRangeCheckChip", "Chip:SmallRangeCheckChip"}

# halo2_gadgets 0.5.0 instruction rows -> wrapper test
_ZG = {
    "EccChip.add": "ecc::chip::boole_h1::boole_h1_ecc_add",
    "EccChip.add_incomplete": "ecc::chip::boole_h1::boole_h1_ecc_add_incomplete",
    "EccChip.constrain_equal": "ecc::chip::boole_h1::boole_h1_ecc_constrain_equal",
    "EccChip.extract_p": "ecc::chip::boole_h1::boole_h1_ecc_extract_p",
    "EccChip.mul": "ecc::chip::boole_h1::boole_h1_ecc_mul",
    "EccChip.mul_fixed": "ecc::chip::boole_h1::boole_h1_ecc_mul_fixed",
    "EccChip.mul_fixed_base_field_elem": "ecc::chip::boole_h1::boole_h1_ecc_mul_fixed_base_field_elem",
    "EccChip.mul_fixed_short": "ecc::chip::boole_h1::boole_h1_ecc_mul_fixed_short",
    "EccChip.mul_sign": "ecc::chip::boole_h1::boole_h1_ecc_mul_sign",
    "EccChip.scalar_fixed_from_signed_short": "ecc::chip::boole_h1::boole_h1_ecc_scalar_fixed_from_signed_short",
    "EccChip.scalar_var_from_base": "ecc::chip::boole_h1::boole_h1_ecc_scalar_var_from_base",
    "EccChip.witness_point_from_constant": "ecc::chip::boole_h1::boole_h1_ecc_witness_point_from_constant",
    "Pow5Chip.add_input": "poseidon::pow5::boole_h1::boole_h1_pow5_add_input",
    "Pow5Chip.get_output": "poseidon::pow5::boole_h1::boole_h1_pow5_get_output",
    "Pow5Chip.initial_state": "poseidon::pow5::boole_h1::boole_h1_pow5_initial_state",
    "Pow5Chip.permute": "poseidon::pow5::boole_h1::boole_h1_pow5_permute",
    "SinsemillaChip.extract": "sinsemilla::tests::boole_h1::boole_h1_sinsemilla_extract",
    "SinsemillaChip.hash_to_point": "sinsemilla::tests::boole_h1::boole_h1_sinsemilla_hash_to_point",
    "SinsemillaChip.hash_to_point_with_private_init":
        "sinsemilla::tests::boole_h1::boole_h1_sinsemilla_hash_to_point_with_private_init",
    "MerkleChip.extract": "sinsemilla::merkle::tests::boole_h1::boole_h1_merkle_extract",
    "MerkleChip.hash_layer": "sinsemilla::merkle::tests::boole_h1::boole_h1_merkle_hash_layer",
    "MerkleChip.hash_to_point": "sinsemilla::merkle::tests::boole_h1::boole_h1_merkle_hash_to_point",
    "MerkleChip.hash_to_point_with_private_init":
        "sinsemilla::merkle::tests::boole_h1::boole_h1_merkle_hash_to_point_with_private_init",
    "MerkleChip.mux": "sinsemilla::merkle::tests::boole_h1::boole_h1_merkle_mux",
    "MerkleChip.swap": "sinsemilla::merkle::tests::boole_h1::boole_h1_merkle_swap",
    "CondSwapChip.mux": "utilities::cond_swap::boole_h1::boole_h1_cond_swap_mux",
    "CondSwapChip.swap": "utilities::cond_swap::boole_h1::boole_h1_cond_swap_swap",
    "LookupRangeCheckConfig.short_range_check":
        "utilities::lookup_range_check::boole_h1::boole_h1_lookup_short_range_check",
    "LookupRangeCheck4_5BConfig.short_range_check":
        "utilities::lookup_range_check::boole_h1::boole_h1_lookup_4_5b_short_range_check",
}

WITNESSING = ("witness_point", "witness_point_non_id", "witness_scalar_var", "witness_scalar_fixed",
              "witness_message_piece")
CONFIGURATION = ("config", "configure", "load")

NA_WITNESS = ("witnessing instruction: its result is a prover-supplied value (a `Value` parameter, not a circuit cell), "
              "so no circuit input determines it; DET is false by design and is not a meaningful statement")
NA_CONFIG = ("configuration / table-loading function: it creates columns, gates or fixed table contents and has no "
             "circuit inputs or results (its constraints are part of every instruction package that uses it)")
NA_PROOF_SYSTEM = ("proof-system component (prover / verifier / transcript / commitment-scheme code), not a constraint "
                   "system: DET is a property of circuits")
NA_SECURITY = "security parameter of the proof system, not a constraint system"
NA_NATIVE_VERIFIER = ("native verifier entry point (out-of-circuit Rust or Solidity code that checks a proof), not a "
                      "constraint system")
NI_IN_CIRCUIT_VERIFIER = ("in-circuit SNARK verification (aggregation / recursion): an instance needs inner proofs, "
                          "their verifying keys and a KZG setup at the repository's large degree; the generator does "
                          "not produce proofs, so there is no instantiation")
NI_GENERIC_BUILDER = ("generic circuit builder / framework type: its constraints are defined by the caller's "
                      "computation (type parameters or a closure-built program), so there is no instantiation of "
                      "the row itself")
NI_PROGRAM_CIRCUIT = ("interpreter circuit: its constraints are defined by a program (a compiled zkas / Vamp-IR / ONNX "
                      "input) supplied at run time, not by the row's code")


def plan(row: dict) -> Wrapper | Static:
    project, unit, path, sym = row["project"], row["unit"], row["path"], row["symbol"]
    if project == "halo2-gadgets":
        method = sym.split(".", 1)[1] if "." in sym else sym
        if sym in _ZG:
            return Wrapper("zcash-gadgets", _ZG[sym], sym)
        if method in WITNESSING:
            if method == "witness_scalar_var":
                return Static("NOT-APPLICABLE", NA_WITNESS + "; also unimplemented at the pin (`todo!()`)")
            return Static("NOT-APPLICABLE", NA_WITNESS)
        if method in CONFIGURATION:
            return Static("NOT-APPLICABLE", NA_CONFIG)
        return Static("NO-INSTANTIATION", "no wrapper for this instruction")
    if unit == "P3-security-parameter":
        return Static("NOT-APPLICABLE", NA_SECURITY)
    if path.endswith(".sol"):
        return Static("NOT-APPLICABLE", NA_NATIVE_VERIFIER if unit == "P2-verifier-entry-point" else
                      "Solidity contract component (on-chain verifier / registry code), not a constraint system")
    if unit == "P2-verifier-entry-point":
        if "UniversalBatchVerifierChip::verify" in sym:
            return Static("NO-INSTANTIATION", NI_IN_CIRCUIT_VERIFIER)
        return Static("NOT-APPLICABLE", NA_NATIVE_VERIFIER)
    if unit == "P1-component":
        if re.search(r"aggregation|batch_verify|outer/universal|loader/halo2", sym):
            return Static("NO-INSTANTIATION", NI_IN_CIRCUIT_VERIFIER)
        return Static("NOT-APPLICABLE", NA_PROOF_SYSTEM)
    if unit == "CC-circuit-component":
        return _plan_cc(row)
    return Static("NO-INSTANTIATION", f"unit {unit} has no wrapper rule")


def _plan_cc(row: dict) -> Wrapper | Static:
    project, path, sym = row["project"], row["path"], row["symbol"]
    if re.search(r"Aggregation|UniversalBatchVerify|OuterCircuitWrapper|IvcCircuit", sym):
        return Static("NO-INSTANTIATION", NI_IN_CIRCUIT_VERIFIER)
    if re.search(r"CircuitBuilder|CsProxy|ComponentCircuitImpl|EthCircuitImpl|RlcKeccakCircuitImpl", sym):
        return Static("NO-INSTANTIATION", NI_GENERIC_BUILDER)
    if re.search(r"Halo2Module|ZkCircuit|GraphCircuit", sym):
        return Static("NO-INSTANTIATION", NI_PROGRAM_CIRCUIT)
    if project == "orchard" and sym == "AddChip":
        return Wrapper("orchard", "circuit::gadget::add_chip::boole_h1::boole_h1_orchard_add_chip", "AddChip")
    if project == "orchard" and sym == "Circuit":
        return Wrapper("orchard", "circuit::tests::boole_h1::boole_h1_orchard_circuit", "Circuit")
    if project == "qed-it/halo2" and sym == "Chip:EccChip":
        return Wrapper("qedit-halo2", "ecc::tests::boole_h1::boole_h1_qedit_ecc_chip", sym)
    if project == "qed-it/orchard" and sym == "Circuit:CircuitVanilla":
        return Wrapper("qedit-orchard", "circuit::circuit_vanilla::tests::boole_h1::boole_h1_qedit_circuit_vanilla", sym)
    if project == "qed-it/orchard" and sym == "Circuit:CircuitZsa":
        return Wrapper("qedit-orchard", "circuit::circuit_zsa::tests::boole_h1::boole_h1_qedit_circuit_zsa", sym)
    if project == "poseidon-circuit" and sym in _SCROLL_POSEIDON:
        return Wrapper("scroll-poseidon", _SCROLL_POSEIDON[sym], sym)
    if project == "mpt-circuit" and sym == "TestCircuit":
        return Wrapper("scroll-mpt", "TestCircuit", "TestCircuit")
    if project == "darkrenaissance/darkfi" and sym in _DARKFI:
        return Wrapper("darkfi-gadgets", sym, sym)
    for key, (status, reason) in CC_DECISIONS.items():
        if key == (project, sym):
            return Static(status, reason)
    return Static("NO-INSTANTIATION", "no wrapper for this circuit component")


# Circuit components whose repository builds against a halo2 line the H1 exporter (zcash 0.3 and scroll v1.0
# MockProver internals) has no adaptation for: not extracted in generator 1.0 (the locked fork is named).
def _no_exporter(fork: str) -> tuple:
    return ("COMPILE-FAIL", f"not extracted: the repository builds against {fork}, a halo2 line whose MockProver "
                            "internals the H1 exporter (zcash 0.3 line, scroll-tech v1.0 line) has no adaptation for")


CC_DECISIONS: dict = {
    ("scroll-tech/misc-precompiled-circuit", "Chip:ModExpChip"): _no_exporter("scroll-tech/halo2 develop@103ce21d"),
    ("scroll-tech/misc-precompiled-circuit", "Chip:RMD160Chip"): _no_exporter("scroll-tech/halo2 develop@103ce21d"),
    ("summa-solvency", "MstInclusionCircuit"): _no_exporter("summa-dev/halo2@8386d6e6"),
    ("intersectmbo/mithril", "Relation:StmCertificateCircuit"): _no_exporter("midnight-proofs 0.8.1 / midnight-circuits 7.2.2"),
    ("nebrazkp/upa", "Circuit:KeccakCircuit"): _no_exporter("axiom-crypto/halo2 (halo2-axiom) with halo2-lib v0.3.0-ce"),
    ("zkonduit/ezkl", "Circuit:HashCircuit"): _no_exporter("zkonduit/halo2 ac/conditional-compilation-icicle2@01c88842"),
}
for _c in ("DepositCircuit", "MerkleCircuit", "NewAccountCircuit", "WithdrawCircuit"):
    CC_DECISIONS[("cardinal-cryptography/blanksquare-monorepo", f"Circuit:{_c}")] = _no_exporter(
        "privacy-scaling-explorations/halo2 v0.3.0@73408a14")
