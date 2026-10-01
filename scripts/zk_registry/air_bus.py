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

    def rule(self, air: IR.Air, k: int, it: IR.Interaction, negative: bool = False) -> Rule:
        """``negative``: the multiplicity takes a negated value (above p/2) on a real row."""
        raise NotImplementedError

    def table_fn(self, table: str, values: list[int]) -> bool:
        raise NotImplementedError(table)

    def bus_label(self, it: IR.Interaction) -> str:
        return it.kind_name or (f"bus {it.bus}" if it.bus is not None else f"kind {it.kind}")

    def roles(self, air: IR.Air, negative: frozenset = frozenset()) -> tuple[AL.Roles, dict]:
        """``negative``: interactions whose multiplicity is negated on a real row (reads sent with a negated
        count whose sign the expression does not show, e.g. a preprocessed column holding -1)."""
        roles = AL.Roles(selectors=bool(air.uses()["selectors"]))
        iface = {"bus_model": {"name": self.name, "sha256": self.sha256()}, "inputs": [], "outputs": [],
                 "assumptions": [], "fixed_vars": len(IR.Layout.of(air).fixed())}
        for k, it in enumerate(air.interactions):
            r = self.rule(air, k, it, k in negative)
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

    def chip_type_at(self, name: str, index: int) -> str:
        return self.chip_type(name)

    def label(self, index: int) -> str:
        """A human label for AIRs whose type name is shared (e.g. the modulus of a field-expression AIR)."""
        return ""

    def observe(self, airs: list) -> None:
        """Called once with every extracted AIR before any role is assigned (bus names from bus indices)."""


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

    def rule(self, air: IR.Air, k: int, it: IR.Interaction, negative: bool = False) -> Rule:
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


# ------------------------------------------------------------------------------------------ Pico v2.1.2

def _negative_weights(air: IR.Air, node: int) -> bool:
    """The affine multiplicity is a sum of columns with negative weights (a read sent with a negated count)."""
    p = air.p
    stack, consts, seen_var = [node], [], False
    while stack:
        n = air.nodes[stack.pop()]
        if n[0] == "add":
            stack += [n[1], n[2]]
        elif n[0] == "mul" and air.nodes[n[1]][0] == "const" and air.nodes[n[2]][0] in ("main", "prep"):
            consts.append(air.nodes[n[1]][1])
            seen_var = True
        elif n[0] == "const":
            if n[1]:
                consts.append(n[1])
        else:
            return False
    return seen_var and all(c > p // 2 for c in consts)


@register
class PicoModel(BusModel):
    """Pico v2.1.2 `LookupType` buses (vm/src/machine/lookup.rs); `looking` is recorded as send, `looked` as receive.

    Direction conventions at the pin (vm/src/machine/builder/{riscv_memory,lookup}.rs, chips/chips/*): a memory
    access looks the previous (chunk, clk, address, value) and is looked with the current one; the CPU's call to
    MemoryReadWrite and every ALU call carry the request and the result in one message; byte lookups go to the
    ByteChip table; program lookups supply the instruction; Global messages carry their direction in the
    is_send / is_receive fields; SHA round chains and syscalls are looked by the AIR that serves them.  Recursion
    AIRs use a write-once memory bus (writes looked by readers; a read can also be sent with a negated count)."""
    zkvm = "pico"
    display = "Pico"
    tables = {"picoByte": "\n".join([
        "/-- Pico byte lookup table (`ByteChip`, `ByteOpcode` AND 0, OR 1, XOR 2, SLL 3, ShrCarry 4, LTU 5, MSB 6,",
        "U8Range 7, U16Range 8, BitRange 9; vm/src/chips/chips/byte): `[op, a1, a2, b, c]` is a table row iff, with",
        "bytes `b, c` and `s = c % 8`: `[0, b &&& c, 0, b, c]`, `[1, b ||| c, 0, b, c]`, `[2, b ^^^ c, 0, b, c]`,",
        "`[3, (b * 2 ^ s) % 256, 0, b, c]`, `[4, b / 2 ^ s, b % 2 ^ s, b, c]`, `[5, b < c, 0, b, c]`, `[6, b / 128, 0, b, 0]`,",
        "`[7, 0, 0, b, c]`; or `[8, v, 0, 0, 0]` with `v < 2 ^ 16`; or `[9, v, 0, bits, 0]` with `bits ≤ 16`,",
        "`v < 2 ^ bits`. -/",
        "def picoByte (v : List F) : Bool :=",
        "  match v with",
        "  | [op, a1, a2, b, c] =>",
        "    let o := op.val; let x := a1.val; let y := a2.val; let bb := b.val; let cc := c.val",
        "    let byte := decide (bb < 256 ∧ cc < 256)",
        "    if o = 0 then byte && decide (y = 0 ∧ x = Nat.land bb cc)",
        "    else if o = 1 then byte && decide (y = 0 ∧ x = Nat.lor bb cc)",
        "    else if o = 2 then byte && decide (y = 0 ∧ x = Nat.xor bb cc)",
        "    else if o = 3 then byte && decide (y = 0 ∧ x = (bb * 2 ^ (cc % 8)) % 256)",
        "    else if o = 4 then byte && decide (x = bb / 2 ^ (cc % 8) ∧ y = bb % 2 ^ (cc % 8))",
        "    else if o = 5 then byte && decide (y = 0 ∧ x = (if bb < cc then 1 else 0))",
        "    else if o = 6 then decide (bb < 256 ∧ cc = 0 ∧ y = 0 ∧ x = bb / 128)",
        "    else if o = 7 then byte && decide (x = 0 ∧ y = 0)",
        "    else if o = 8 then decide (x < 65536 ∧ y = 0 ∧ bb = 0 ∧ cc = 0)",
        "    else if o = 9 then decide (bb ≤ 16 ∧ x < 2 ^ bb ∧ y = 0 ∧ cc = 0)",
        "    else false",
        "  | _ => false"])}

    def table_doc(self, table: str) -> str:
        return ("the Pico ByteChip table: bytewise AND, OR, XOR, shift-left, shift-right with carry, unsigned "
                "less-than and most significant bit on bytes, u8 and u16 ranges, and `v < 2^bits` for `bits ≤ 16`.")

    def table_fn(self, table: str, values: list[int]) -> bool:
        if table != "picoByte" or len(values) != 5:
            return False
        o, x, y, b, c = values
        byte = b < 256 and c < 256
        s = c % 8
        if o == 0:
            return byte and y == 0 and x == b & c
        if o == 1:
            return byte and y == 0 and x == b | c
        if o == 2:
            return byte and y == 0 and x == b ^ c
        if o == 3:
            return byte and y == 0 and x == (b << s) & 0xFF
        if o == 4:
            return byte and x == b >> s and y == b & ((1 << s) - 1)
        if o == 5:
            return byte and y == 0 and x == int(b < c)
        if o == 6:
            return b < 256 and c == 0 and y == 0 and x == b >> 7
        if o == 7:
            return byte and x == 0 and y == 0
        if o == 8:
            return x < 65536 and y == 0 and b == 0 and c == 0
        if o == 9:
            return b <= 16 and x < (1 << b) and y == 0 and c == 0
        return False

    def rule(self, air: IR.Air, k: int, it: IR.Interaction, negative: bool = False) -> Rule:
        kind, send, n = it.kind_name, it.direction == "send", len(it.values)
        if air.group.startswith("recursion"):
            if kind == "Memory":
                if not send:
                    return Rule("in", "recursion memory read (looked)")
                if _negative_weights(air, it.mult):
                    return Rule("in", "recursion memory read sent with a negated count")
                if negative:
                    return Rule("in", "recursion memory read: the multiplicity column holds -1 on real rows")
                return Rule("out", "recursion memory write (multiplicity = number of reads)")
            return Rule("out" if send else "in", f"recursion {kind}")
        if kind == "Memory" and n == 9:
            return (Rule("in", "access: the previous (chunk, clk, address, value) is supplied by memory") if send else
                    Rule("out", "access: the current (chunk, clk, address, value) this AIR produces"))
        if kind == "Memory" and n == 27:
            req = (0,) + tuple(range(5, 27))
            if send:
                return Rule("split", "CPU call to MemoryReadWrite: request out; `a` (load result or store value) in",
                            in_fields=(1, 2, 3, 4), out_fields=req)
            return Rule("in", "MemoryReadWrite serves the CPU call; `a` is the load result or the store value "
                              "depending on the opcode, so the whole message is an input (conservative)")
        if kind == "Byte":
            return (Rule("assume", "lookup into the ByteChip table", table="picoByte") if send else
                    Rule("in", "table AIR: preprocessed row values; the multiplicity is the other chips' demand"))
        if kind == "Program":
            return Rule("in", "program table: the instruction at pc (preprocessed values on the table side)")
        if kind == "Alu" and n == 13:
            req, res = (0, 5, 6, 7, 8, 9, 10, 11, 12), (1, 2, 3, 4)
            if send:
                return Rule("split", "ALU call made: request [opcode, b, c] out, result a in", in_fields=res,
                            out_fields=req, mult_with="out")
            return Rule("split", "ALU call served: request [opcode, b, c] in, result a out", in_fields=req,
                        out_fields=res)
        if kind == "Poseidon2" and n == 32:
            first, second = tuple(range(16)), tuple(range(16, 32))
            if send:
                return Rule("split", "Poseidon2 call made: input state out, output state in", in_fields=second,
                            out_fields=first, mult_with="out")
            return Rule("split", "Poseidon2 call served: input state in, output state out", in_fields=first,
                        out_fields=second)
        if kind == "Global" and n == 11:
            if not send:
                return Rule("in", "global bus messages are consumed by the Global chip")
            is_send, is_recv = _const_value(air, it.values[8]), _const_value(air, it.values[9])
            if is_send == 1 and is_recv == 0:
                return Rule("out", "global bus message sent to other chunks (is_send = 1)")
            if is_recv == 1 and is_send == 0:
                return Rule("in", "global bus message received from other chunks (is_receive = 1)")
            return Rule("in", "global bus message with non-constant direction flags (conservatively an input)")
        return (Rule("out", f"{kind}: produced by this AIR (looking)") if send else
                Rule("in", f"{kind}: consumed or served by this AIR (looked)"))

    def chip_type(self, name: str) -> str:
        n = re.sub(r"^Recursion", "", name)
        if re.fullmatch(r"(Bn254|Bls12381|Secp256k1|Secp256r1)AddAssign", n):
            return "WeierstrassAddAssignChip"
        if re.fullmatch(r"(Bn254|Bls12381|Secp256k1|Secp256r1)DoubleAssign", n):
            return "WeierstrassDoubleAssignChip"
        if re.fullmatch(r"(Bn254|Bls12381|Secp256k1|Secp256r1)Decompress", n):
            return "WeierstrassDecompressChip"
        m = re.fullmatch(r"(Bn254|Bls381|Secp256k1)(FpOp|Fp2AddSub|Fp2Mul)", n)
        if m:
            return m.group(2) + "Chip"
        special = {"Cpu": "CpuChip", "Program": "ProgramChip", "Byte": "ByteChip", "Global": "GlobalChip",
                   "MemoryLocal": "MemoryLocalChip", "MemoryReadWrite": "MemoryReadWriteChip",
                   "MemoryInitialize": "MemoryInitializeFinalizeChip", "MemoryFinalize": "MemoryInitializeFinalizeChip",
                   "ShiftLeft": "SLLChip", "ShiftRight": "ShiftRightChip", "LessThan": "LtChip", "DivRem": "DivRemChip",
                   "KeccakPermute": "KeccakPermuteChip", "SyscallRiscv": "SyscallChip", "SyscallPrecompile": "SyscallChip",
                   "RiscvKoalaBearPoseidon2": "Poseidon2Chip", "Uint256MulMod": "Uint256MulChip",
                   "KoalaBearPoseidon2": "Poseidon2Chip"}
        if n in special:
            return special[n]
        return n if n.endswith("Chip") else n + "Chip"


# ------------------------------------------------------------------------------------------ OpenVM v2.0.2

# Labels of the FieldExpr / ModularIsEqual AIRs of SdkVmConfig::standard() by air id: the vkey order is the
# reverse of the extension insertion order (modular: bn254 Fp, bn254 Fr, secp256k1 Fp, Fr, p256 Fp, Fr,
# bls12-381 Fp, Fr, each [add/sub, mul/div, is-equal]; fp2: Bn254Fp2, Bls12_381Fp2 [add/sub, mul/div]; ecc: bn254,
# secp256k1, p256, bls12-381 [add-ne, double]); checked against the limb sizes (48-limb AIRs only for bls12-381 Fp).
OPENVM_STANDARD_LABELS = {
    4: ("bls12-381 G1 double", "WeierstrassAir"), 5: ("bls12-381 G1 add-ne", "WeierstrassAir"),
    6: ("p256 double", "WeierstrassAir"), 7: ("p256 add-ne", "WeierstrassAir"),
    8: ("secp256k1 double", "WeierstrassAir"), 9: ("secp256k1 add-ne", "WeierstrassAir"),
    10: ("bn254 G1 double", "WeierstrassAir"), 11: ("bn254 G1 add-ne", "WeierstrassAir"),
    12: ("Bls12_381Fp2 mul/div", "Fp2Air"), 13: ("Bls12_381Fp2 add/sub", "Fp2Air"),
    14: ("Bn254Fp2 mul/div", "Fp2Air"), 15: ("Bn254Fp2 add/sub", "Fp2Air"),
}
for _k, _m in enumerate(["bls12-381 Fr", "bls12-381 Fp", "p256 Fr", "p256 Fp", "secp256k1 Fr", "secp256k1 Fp",
                         "bn254 Fr", "bn254 Fp"]):
    OPENVM_STANDARD_LABELS[16 + 3 * _k] = (f"{_m} is-equal", "ModularIsEqualAir")
    OPENVM_STANDARD_LABELS[17 + 3 * _k] = (f"{_m} mul/div", "ModularAir")
    OPENVM_STANDARD_LABELS[18 + 3 * _k] = (f"{_m} add/sub", "ModularAir")
OPENVM_ALIASES = [
    (r"Rv32BaseAluAdapterAir, BaseAluCoreAir<4, 8>", "Rv32BaseAluAir"),
    (r"Rv32BaseAluAdapterAir, LessThanCoreAir<4, 8>", "Rv32LessThanAir"),
    (r"Rv32BaseAluAdapterAir, ShiftCoreAir<4, 8>", "Rv32ShiftAir"),
    (r"Rv32LoadStoreAdapterAir, LoadStoreCoreAir<4>", "Rv32LoadStoreAir"),
    (r"Rv32LoadStoreAdapterAir, LoadSignExtendCoreAir<4, 8>", "Rv32LoadSignExtendAir"),
    (r"Rv32BranchAdapterAir, BranchEqualCoreAir<4>", "Rv32BranchEqualAir"),
    (r"Rv32BranchAdapterAir, BranchLessThanCoreAir<4, 8>", "Rv32BranchLessThanAir"),
    (r"Rv32CondRdWriteAdapterAir, Rv32JalLuiCoreAir", "Rv32JalLuiAir"),
    (r"Rv32JalrAdapterAir, Rv32JalrCoreAir", "Rv32JalrAir"),
    (r"Rv32RdWriteAdapterAir, Rv32AuipcCoreAir", "Rv32AuipcAir"),
    (r"Rv32MultAdapterAir, MultiplicationCoreAir<4, 8>", "Rv32MultiplicationAir"),
    (r"Rv32MultAdapterAir, MulHCoreAir<4, 8>", "Rv32MulHAir"),
    (r"Rv32MultAdapterAir, DivRemCoreAir<4, 8>", "Rv32DivRemAir"),
    (r"BaseAluCoreAir<32, 8>", "Rv32BaseAlu256Air"),
    (r"LessThanCoreAir<32, 8>", "Rv32LessThan256Air"),
    (r"BranchEqualCoreAir<32>", "Rv32BranchEqual256Air"),
    (r"BranchLessThanCoreAir<32, 8>", "Rv32BranchLessThan256Air"),
    (r"MultiplicationCoreAir<32, 8>", "Rv32Multiplication256Air"),
    (r"ShiftCoreAir<32, 8>", "Rv32Shift256Air"),
]


@register
class OpenVmModel(BusModel):
    """OpenVM v2.0.2 buses of `SdkVmConfig::standard()` (stark-backend v2.0.1 interactions: a receive is a negated
    count).  Bus indices are named from the AIRs that own them (`observe`): the program, variable-range, bitwise,
    range-tuple and Poseidon2-compression tables, the connector's execution bus, the boundary's memory and merkle
    buses, the keccak state bus and the SHA-2 buses.

    Direction conventions at the pin (crates/vm/src/arch/execution.rs, system/memory/offline_checker,
    crates/circuits/primitives): an executor row receives its (pc, timestamp) and sends the next one; a memory access
    receives the previous (address space, pointer, data, timestamp) and sends the new one; range, bitwise and
    range-tuple checks are lookups into tables; program lookups supply the instruction; Poseidon2 compression
    messages carry the input and the output; the keccak state bus carries the state before (tag 0) and after
    (tag 1) the permutation."""
    zkvm = "openvm"
    display = "OpenVM"
    RANGE_MAX_BITS = 17
    RANGE_TUPLE_SIZES = (256, 8192)
    tables = {
        "ovmVarRange": "\n".join([
            "/-- OpenVM variable range checker table (`VariableRangeCheckerAir`, range_max_bits 17): `[v, bits]` is a",
            "row iff `bits ≤ 17` and `v < 2 ^ bits`. -/",
            "def ovmVarRange (v : List F) : Bool :=",
            "  match v with",
            "  | [x, b] => decide (b.val ≤ 17 ∧ x.val < 2 ^ b.val)",
            "  | _ => false"]),
        "ovmBitwise": "\n".join([
            "/-- OpenVM bitwise operation lookup table (`BitwiseOperationLookupAir<8>`): `[x, y, z, op]` is a row iff `x, y`",
            "are bytes and either `op = 0, z = 0` (range pair) or `op = 1, z = x ^^^ y` (xor). -/",
            "def ovmBitwise (v : List F) : Bool :=",
            "  match v with",
            "  | [x, y, z, op] =>",
            "    decide (x.val < 256 ∧ y.val < 256 ∧ ((op.val = 0 ∧ z.val = 0) ∨ (op.val = 1 ∧ z.val = Nat.xor x.val y.val)))",
            "  | _ => false"]),
        "ovmRangeTuple": "\n".join([
            "/-- OpenVM range tuple checker table (`RangeTupleCheckerAir<2>`, sizes [256, 8192]): `[x, y]` is a row iff",
            "`x < 256` and `y < 8192`. -/",
            "def ovmRangeTuple (v : List F) : Bool :=",
            "  match v with",
            "  | [x, y] => decide (x.val < 256 ∧ y.val < 8192)",
            "  | _ => false"]),
    }

    def __init__(self) -> None:
        self.bus_names: dict[int, str] = {}

    def observe(self, airs: list) -> None:
        # bus indices are per circuit: only the application VM's AIRs are named (the leaf aggregation circuit has
        # its own bus index manager; its buses keep the direction-literal rule)
        airs = [a for a in airs if a.group == "app-vm"]
        owners = {"ProgramAir": "Program", "VariableRangeCheckerAir": "VarRange", "BitwiseOperationLookupAir": "Bitwise",
                  "RangeTupleCheckerAir": "RangeTuple", "Poseidon2PeripheryAir": "Poseidon2", "KeccakfPermAir":
                  "KeccakState"}
        names: dict[int, str] = {}
        by_air = {a.name: a for a in airs}
        for a in airs:
            base = _base_type(a.name)
            if base in owners and len({it.bus for it in a.interactions}) == 1:
                names[a.interactions[0].bus] = owners[base]
        for a in airs:
            base = _base_type(a.name)
            buses = {it.bus for it in a.interactions}
            if base == "VmConnectorAir":
                rest = [b for b in buses if names.get(b) != "Program"]
                if len(rest) == 1:
                    names[rest[0]] = "Execution"
            if base == "MemoryMerkleAir":
                for b in buses:
                    if b not in names:
                        names[b] = "Merkle"
        for a in airs:
            base = _base_type(a.name)
            buses = {it.bus for it in a.interactions}
            if base == "PersistentBoundaryAir":
                for b in buses:
                    if b not in names:
                        names[b] = "Memory"
            if base == "Sha2BlockHasherVmAir":
                main_buses = set()
                for m in airs:
                    if _base_type(m.name) == "Sha2MainAir":
                        main_buses |= {it.bus for it in m.interactions}
                for b in buses:
                    if b not in names:
                        names[b] = "Sha2" if b in main_buses else "Sha2Private"
        del by_air
        self.bus_names = names

    def bus_label(self, it: IR.Interaction) -> str:
        return f"{self.bus_names.get(it.bus, 'bus')} {it.bus}"

    def roles(self, air: IR.Air, negative: frozenset = frozenset()) -> tuple[AL.Roles, dict]:
        if air.group == "app-vm":
            return super().roles(air, negative)
        # another circuit (the leaf aggregation circuit): its bus indices are not the application VM's, so no bus
        # is named and every interaction follows the direction-literal rule
        return BusModel.roles(OpenVmModel(), air, negative)

    def table_doc(self, table: str) -> str:
        return {"ovmVarRange": "the OpenVM variable range checker table: `v < 2^bits` for `bits ≤ 17`.",
                "ovmBitwise": "the OpenVM bitwise lookup table: byte pairs, with their xor for op 1.",
                "ovmRangeTuple": "the OpenVM range tuple checker table: `x < 256` and `y < 8192`."}[table]

    def table_fn(self, table: str, values: list[int]) -> bool:
        if table == "ovmVarRange" and len(values) == 2:
            x, b = values
            return b <= self.RANGE_MAX_BITS and x < (1 << b)
        if table == "ovmBitwise" and len(values) == 4:
            x, y, z, op = values
            return x < 256 and y < 256 and ((op == 0 and z == 0) or (op == 1 and z == x ^ y))
        if table == "ovmRangeTuple" and len(values) == 2:
            return values[0] < self.RANGE_TUPLE_SIZES[0] and values[1] < self.RANGE_TUPLE_SIZES[1]
        return False

    def rule(self, air: IR.Air, k: int, it: IR.Interaction, negative: bool = False) -> Rule:
        bus = self.bus_names.get(it.bus, "")
        send = it.direction == "send"
        n = len(it.values)
        if bus == "VarRange":
            return (Rule("assume", "lookup into the variable range checker table", table="ovmVarRange") if send else
                    Rule("in", "table AIR: row values; the multiplicity is the other chips' demand"))
        if bus == "Bitwise":
            return (Rule("assume", "lookup into the bitwise operation table", table="ovmBitwise") if send else
                    Rule("in", "table AIR: row values; the multiplicity is the other chips' demand"))
        if bus == "RangeTuple":
            return (Rule("assume", "lookup into the range tuple checker table", table="ovmRangeTuple") if send else
                    Rule("in", "table AIR: row values; the multiplicity is the other chips' demand"))
        if bus == "Program":
            return Rule("in", "program table: the instruction at pc")
        if bus == "Poseidon2" and n == 24:
            req, res = tuple(range(16)), tuple(range(16, 24))
            if send:
                return Rule("split", "Poseidon2 compression call made: input out, digest in", in_fields=res,
                            out_fields=req, mult_with="out")
            return Rule("split", "Poseidon2 compression call served: input in, digest out", in_fields=req,
                        out_fields=res)
        if bus == "KeccakState" and n >= 1:
            tag = _const_value(air, it.values[0])
            if tag in (0, 1):
                before = tag == 0
                if send:
                    return (Rule("out", "keccak-f call made: the state before the permutation (tag 0)") if before else
                            Rule("in", "keccak-f call made: the state after the permutation (tag 1) is the answer"))
                return (Rule("in", "keccak-f call served: the state before the permutation (tag 0)") if before else
                        Rule("out", "keccak-f call served: the state after the permutation (tag 1)"))
        if send and negative:
            return Rule("in", f"{bus or 'bus'}: the multiplicity column is negative on real rows (a receive)")
        return (Rule("out", f"{bus or 'bus'}: sent (produced) by this AIR") if send else
                Rule("in", f"{bus or 'bus'}: received (consumed) by this AIR"))

    def label(self, index: int) -> str:
        return OPENVM_STANDARD_LABELS.get(index, ("", ""))[0]

    def chip_type_at(self, name: str, index: int) -> str:
        if index in OPENVM_STANDARD_LABELS and ("FieldExpressionCoreAir" in name or "ModularIsEqualCoreAir" in name):
            return OPENVM_STANDARD_LABELS[index][1]
        return self.chip_type(name)

    def chip_type(self, name: str) -> str:
        for pat, alias in OPENVM_ALIASES:
            if pat in name:
                return alias
        base = _base_type(name)
        return base
