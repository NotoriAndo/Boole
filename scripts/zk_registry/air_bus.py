"""Bus models of the zkVM AIRs: the role of every interaction in the DET statement.

Every interaction an AIR records is classified by a :class:`Rule`:

* ``in``      the whole message is an input (the environment supplies it: a received state or memory value, a
              program / instruction lookup, a call the AIR serves);
* ``out``     the whole message is an output (a state or memory value the AIR produces, a call it makes);
* ``split``   some value fields are inputs and the others outputs (a call bus whose message carries both the
              request and the result); the multiplicity goes with the part the AIR controls;
* ``assume``  a lookup into a table another chip provides (byte, range, bitwise): the table fact is a hypothesis
              of the statement (``m ≠ 0 → Table values``) and the values are neither inputs nor outputs.

Facts the AIR itself is responsible for are never assumed: a table AIR's own received lookups, the result of
a call the AIR serves and the values it sends are not hypotheses.  Each table predicate exists twice, as Lean
source (emitted into the model) and as an independently written Python function (used by the gates and the
counterexample search); G-FID and G-NONVAC evaluate both on real rows.

Coverage (``coverage``) records public machine-checked artifacts that already answer DET or functional
correctness of an AIR at the same pin (census arm A of ZKVM-OBLIGATION-CENSUS-P0).
"""
from __future__ import annotations

import hashlib
import inspect
import re
from dataclasses import dataclass

from . import air_ir as IR
from . import air_lean as AL


@dataclass(frozen=True)
class Rule:
    role: str                              # in | out | split | assume
    reason: str
    table: str | None = None               # assume: Lean predicate name
    in_fields: tuple[int, ...] = ()        # split
    out_fields: tuple[int, ...] = ()       # split
    mult_with: str = "in"                  # split: the part whose message carries the multiplicity check


class BusModel:
    """Base class; a zkVM model implements :meth:`rule` and the tables."""
    zkvm = ""
    display = ""
    version = "1"
    tables: dict[str, str] = {}

    @property
    def name(self) -> str:
        return f"{self.zkvm}-bus-model/{self.version}"

    def sha256(self) -> str:
        return hashlib.sha256(inspect.getsource(type(self)).encode()).hexdigest()

    def rule(self, air: IR.Air, k: int, it: IR.Interaction) -> Rule:
        raise NotImplementedError

    def table_fn(self, table: str, values: list[int]) -> bool:
        raise NotImplementedError(table)

    def bus_label(self, it: IR.Interaction) -> str:
        return it.kind_name or (f"bus {it.bus}" if it.bus is not None else f"kind {it.kind}")

    def roles(self, air: IR.Air) -> tuple[AL.Roles, dict]:
        roles = AL.Roles(selectors=bool(air.uses()["selectors"]))
        iface = {"bus_model": {"name": self.name, "sha256": self.sha256()}, "inputs": [], "outputs": [],
                 "assumptions": [], "fixed_vars": len(IR.Layout.of(air).fixed())}
        for k, it in enumerate(air.interactions):
            r = self.rule(air, k, it)
            bus = self.bus_label(it)
            note = f"#{k} {it.direction} {bus}"
            rec = {"interaction": k, "direction": it.direction, "bus": bus, "role": r.role, "reason": r.reason}
            if r.role == "in":
                roles.inputs.append(AL.Message(k, it.mult, list(it.values), note))
                iface["inputs"].append(rec)
            elif r.role == "out":
                roles.outputs.append(AL.Message(k, it.mult, list(it.values), note))
                iface["outputs"].append(rec)
            elif r.role == "assume":
                roles.assumptions.append(AL.Assumption(k, r.table, it.mult, list(it.values), f"{note}: {r.table}"))
                iface["assumptions"].append(dict(rec, table=r.table))
            elif r.role == "split":
                roles.inputs.append(AL.Message(k, it.mult, [it.values[i] for i in r.in_fields],
                                               f"{note} fields {list(r.in_fields)}"))
                roles.outputs.append(AL.Message(k, it.mult, [it.values[i] for i in r.out_fields],
                                                f"{note} fields {list(r.out_fields)}"))
                iface["inputs"].append(dict(rec, fields=list(r.in_fields)))
                iface["outputs"].append(dict(rec, fields=list(r.out_fields)))
            else:
                raise ValueError(f"unknown role {r.role!r}")
        return roles, iface

    def assumption_texts(self, roles: AL.Roles, iface: dict) -> list[str]:
        used = sorted({a.table for a in roles.assumptions})
        out = []
        for t in used:
            n = sum(1 for a in roles.assumptions if a.table == t)
            out.append(f"Assumptions: {n} lookup(s) into `{t}` hold whenever their multiplicity is non-zero — "
                       f"{self.table_doc(t)} The table is provided and constrained by another chip, whose own "
                       "obligations are out of scope; bus balance (LogUp) is not modelled.")
        split = [m for m in iface["inputs"] if m["role"] == "split"]
        if split:
            out.append("Call messages carrying both a request and a result are split: the result fields of a call "
                       "this AIR serves are outputs, the result fields of a call it makes are inputs (the callee's "
                       f"answer); {len(split)} such message(s).")
        return out

    def table_doc(self, table: str) -> str:
        return ""

    def chip_type(self, name: str) -> str:
        """The Rust chip type of an AIR name (ledger rows are keyed ``<zkvm>:<ChipType>``)."""
        return name


# ------------------------------------------------------------------------------------------ registry

MODELS: dict[str, type] = {}


def register(cls: type) -> type:
    MODELS[cls.zkvm] = cls
    return cls


def model_for(zkvm: str) -> BusModel:
    return MODELS[zkvm]()


# ------------------------------------------------------------------------------------------ coverage

COVERAGE_SOURCES = {
    "sp1": {"name": "sp1-lean", "url": "https://github.com/succinctlabs/sp1-lean", "pin": "512e944a (extraction 9d249b8)",
            "scope": "per-row soundness of the chip against the Sail RISC-V model (functional correctness, which implies "
                     "DET), assuming bus/LogUp balance; supervisor-mode AIRs only"},
    "openvm": {"name": "openvm-fv", "url": "https://github.com/openvm-org/openvm-fv", "pin": "7523c23a (openvm v2.0.0)",
               "scope": "per-row equivalence against a Sail RV32 fork (functional correctness, which implies DET) under "
                        "assumption I1 (LogUp/bus soundness)"},
    "pico": {"name": "pico-fv", "url": "https://github.com/NethermindEth/pico-fv", "pin": "a7f9a097 (pico v2.0.0)",
             "scope": "RV64IM equivalence against Sail under the chip's row constraints, bus assumptions and program "
                      "well-formedness (functional correctness, which implies DET)"},
}
# Census arm A coverage lists (ARM-SUMMARY / NOTES at the census pins).  Values: (air name pattern, same code at
# the wave pin, note).
COVERED = {
    "sp1": {"names": ["Add", "Addi", "Addw", "AluX0", "Bitwise", "Branch", "DivRem", "Jal", "Jalr", "LoadByte",
                      "LoadDouble", "LoadHalf", "LoadWord", "LoadX0", "Lt", "Mul", "ShiftLeft", "ShiftRight",
                      "StoreByte", "StoreDouble", "StoreHalf", "StoreWord", "Sub", "Subw", "UType"],
            "same_code": True,
            "note": "extraction commit 9d249b8 (v6.2.2-20) vs v6.8.1: only derive/utility diffs in the chip crates "
                    "(census caveat 3); user-mode variants are not covered"},
    "openvm": {"names": ["Rv32BaseAluAir", "Rv32LessThanAir", "Rv32ShiftAir", "Rv32LoadStoreAir", "Rv32LoadSignExtendAir",
                         "Rv32BranchEqualAir", "Rv32BranchLessThanAir", "Rv32JalLuiAir", "Rv32JalrAir", "Rv32AuipcAir",
                         "Rv32MultiplicationAir", "Rv32MulHAir", "Rv32DivRemAir", "KeccakfOpAir", "KeccakfPermAir",
                         "XorinVmAir", "Sha2MainAir", "Sha2BlockHasherVmAir"],
               "same_code": True,
               "note": "openvm-fv targets v2.0.0; 0 primary circuit files change to v2.0.2 (census caveat 3); XorinVmAir "
                       "is covered for per-row well-formedness only"},
    "pico": {"names": ["AddChip", "AddwChip", "BitwiseChip", "CpuChip", "DivRemChip", "LtChip", "MemoryReadWriteChip",
                       "MulChip", "SLLChip", "ShiftRightChip", "SubChip", "SubwChip"],
             "same_code_names": ["AddwChip", "BitwiseChip", "MulChip", "SubChip", "SubwChip"],
             "note": "pico-fv pins v2.0.0; from v2.0.0 to v2.1.2 the chip directories of add, divrem, lt, sll, sr, "
                     "riscv_cpu and riscv_memory/read_write changed, so those proofs are not at the same code"},
}


def _base_type(rust_type: str) -> str:
    return re.sub(r"<.*$", "", rust_type).split("::")[-1].strip()


def coverage(zkvm: str, air_name: str, rust_type: str) -> dict:
    """``coverage`` record of an AIR: ``partial`` when a public artifact answers it at the same code (every public
    proof assumes bus balance, so none is ``full``); a proof of different code is listed with ``same_code: false``
    and does not count."""
    cov = COVERED.get(zkvm)
    src = COVERAGE_SOURCES.get(zkvm)
    if not cov:
        return {"status": "none", "sources": []}
    base = _base_type(rust_type)
    if zkvm == "sp1":
        hit = air_name in cov["names"]                       # supervisor variants carry the bare name
        same = hit
    elif zkvm == "openvm":
        hit = air_name in cov["names"] or base in cov["names"]
        same = hit
    else:
        hit = base in cov["names"] or air_name in cov["names"]
        same = hit and (base in cov["same_code_names"] or air_name in cov["same_code_names"])
    if not hit:
        return {"status": "none", "sources": []}
    s = dict(src, note=cov["note"], same_code=same)
    return {"status": "partial" if same else "none", "sources": [s],
            "note": ("answered at the same code, conditional on bus balance: issuance can exclude this item" if same else
                     "a public proof exists for an earlier version of this chip; not counted at this pin")}


# ------------------------------------------------------------------------------------------ SP1 v6.8.1

def _const_value(air: IR.Air, node: int) -> int | None:
    n = air.nodes[node]
    return n[1] if n[0] == "const" else None


@register
class Sp1Model(BusModel):
    """SP1 v6.8.1 (hypercube): `InteractionKind` buses of crates/hypercube/src/lookup/interaction.rs.

    Direction conventions at the pin (crates/core/machine/src/air/memory.rs, adapter/state.rs, bytes/, range/,
    program/, global/, syscall/): a memory or page-permission access *sends* the previous (timestamp, address,
    value) and *receives* the current one; every other state or call bus receives what the AIR consumes and
    sends what it produces; byte and range lookups are sent to the ByteChip / RangeChip tables; program,
    instruction-fetch and instruction-decode lookups supply the instruction.  Recursion AIRs use a write-once
    memory bus (the writer sends, readers receive)."""
    zkvm = "sp1"
    display = "SP1"
    tables = {"sp1Byte": "\n".join([
        "/-- SP1 byte and range lookup tables (`ByteChip`, `RangeChip`; `ByteOpcode` AND 0, OR 1, XOR 2, U8Range 3,",
        "LTU 4, MSB 5, Range 6; crates/core/machine/src/bytes, range): `[op, a, b, c]` is a table row iff",
        "`[0, b &&& c, b, c]`, `[1, b ||| c, b, c]`, `[2, b ^^^ c, b, c]`, `[3, 0, b, c]`, `[4, b < c, b, c]` with bytes",
        "`b, c`; `[5, b / 128, b, 0]` with a byte `b`; or `[6, a, bits, 0]` with `bits ≤ 16` and `a < 2 ^ bits`. -/",
        "def sp1Byte (v : List F) : Bool :=",
        "  match v with",
        "  | [op, a, b, c] =>",
        "    let o := op.val; let x := a.val; let y := b.val; let z := c.val",
        "    if o = 0 then decide (y < 256 ∧ z < 256 ∧ x = Nat.land y z)",
        "    else if o = 1 then decide (y < 256 ∧ z < 256 ∧ x = Nat.lor y z)",
        "    else if o = 2 then decide (y < 256 ∧ z < 256 ∧ x = Nat.xor y z)",
        "    else if o = 3 then decide (y < 256 ∧ z < 256 ∧ x = 0)",
        "    else if o = 4 then decide (y < 256 ∧ z < 256 ∧ x = (if y < z then 1 else 0))",
        "    else if o = 5 then decide (y < 256 ∧ z = 0 ∧ x = y / 128)",
        "    else if o = 6 then decide (y ≤ 16 ∧ x < 2 ^ y ∧ z = 0)",
        "    else false",
        "  | _ => false"])}

    def table_doc(self, table: str) -> str:
        return ("the SP1 ByteChip / RangeChip table: bytewise AND, OR, XOR, u8 range, unsigned less-than and most "
                "significant bit on bytes, and `a < 2^bits` for `bits ≤ 16`.")

    def table_fn(self, table: str, values: list[int]) -> bool:
        if table != "sp1Byte" or len(values) != 4:
            return False
        o, x, y, z = values
        byte = y < 256 and z < 256
        if o == 0:
            return byte and x == y & z
        if o == 1:
            return byte and x == y | z
        if o == 2:
            return byte and x == y ^ z
        if o == 3:
            return byte and x == 0
        if o == 4:
            return byte and x == int(y < z)
        if o == 5:
            return y < 256 and z == 0 and x == y >> 7
        if o == 6:
            return y <= 16 and x < (1 << y) and z == 0
        return False

    def rule(self, air: IR.Air, k: int, it: IR.Interaction) -> Rule:
        kind, send = it.kind_name, it.direction == "send"
        if air.group.startswith("recursion"):
            return Rule("out" if send else "in", "recursion write-once memory: the writer sends, readers receive")
        if kind in ("Memory", "PageProtAccess"):
            return (Rule("in", "access: the previous (timestamp, address, value) is supplied by memory") if send else
                    Rule("out", "access: the current (timestamp, address, value) this AIR produces"))
        if kind == "Byte":
            return (Rule("assume", "lookup into the ByteChip / RangeChip table", table="sp1Byte") if send else
                    Rule("in", "table AIR: preprocessed row values; the multiplicity is the other chips' demand"))
        if kind == "Program":
            return Rule("in", "program table: the instruction at pc (preprocessed values on the table side)")
        if kind == "InstructionDecode":
            if send:
                return Rule("in", "decode-table lookup: the decoded instruction is supplied by the decode table")
            n = len(it.values)
            return Rule("split", "decode table: serves the decoding of an instruction word",
                        in_fields=(0, 1), out_fields=tuple(range(2, n)))
        if kind == "InstructionFetch":
            if send:
                return Rule("in", "instruction-fetch lookup: the fetched instruction is supplied by the fetch chip")
            n = len(it.values)
            ins = (0, 1, 2, n - 2, n - 1)
            return Rule("split", "instruction fetch: serves (pc, clk) -> instruction", in_fields=ins,
                        out_fields=tuple(i for i in range(n) if i not in ins))
        if kind == "Global" and send and len(it.values) == 11:
            is_send, is_recv = _const_value(air, it.values[8]), _const_value(air, it.values[9])
            if is_send == 1 and is_recv == 0:
                return Rule("out", "global bus message sent to other shards (is_send = 1)")
            if is_recv == 1 and is_send == 0:
                return Rule("in", "global bus message received from other shards (is_receive = 1)")
            return Rule("in", "global bus message with non-constant direction flags (conservatively an input)")
        return (Rule("out", f"{kind}: produced by this AIR") if send else Rule("in", f"{kind}: consumed by this AIR"))

    def chip_type(self, name: str) -> str:
        n = re.sub(r"^Recursion(Wrap)?", "", name)
        n = re.sub(r"User$", "", n)
        special = {"Program": "ProgramChip", "Byte": "ByteChip", "Range": "RangeChip", "Global": "GlobalChip",
                   "MemoryGlobalInit": "MemoryGlobalChip", "MemoryGlobalFinalize": "MemoryGlobalChip",
                   "PageProtGlobalInit": "PageProtGlobalChip", "PageProtGlobalFinalize": "PageProtGlobalChip",
                   "SyscallCore": "SyscallChip", "SyscallPrecompile": "SyscallChip", "Mprotect": "MProtectChip",
                   "KeccakPermute": "KeccakPermuteChip", "KeccakPermuteControl": "KeccakPermuteControlChip",
                   "Uint256MulMod": "Uint256MulChip", "Poseidon2WideDeg3": "Poseidon2WideChip",
                   "ExtFeltConvert": "ConvertChip"}
        if n in special:
            return special[n]
        if re.fullmatch(r"(Secp256k1|Secp256r1|Bn254|Bls12381)AddAssign", n):
            return "WeierstrassAddAssignChip"
        if re.fullmatch(r"(Secp256k1|Secp256r1|Bn254|Bls12381)DoubleAssign", n):
            return "WeierstrassDoubleAssignChip"
        m = re.fullmatch(r"(Bls12381|Bn254)(FpOp|Fp2AddSub|Fp2Mul)Assign", n)
        if m:
            return {"FpOp": "FpOpChip", "Fp2AddSub": "Fp2AddSubAssignChip", "Fp2Mul": "Fp2MulAssignChip"}[m.group(2)]
        return n + "Chip"
