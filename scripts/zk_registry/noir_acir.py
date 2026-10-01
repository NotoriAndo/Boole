"""Normalized ACIR programs (from ``boole-acir-tool decode``) and an independent Python evaluator.

The decoder prints the serde JSON of the ``acir`` crate of the compiler version that produced the
artifact.  Field elements, function inputs and memory operations are serialized differently across
versions; :func:`normalize_program` maps every supported shape to one internal form:

* field elements: 64-digit hex strings (up to v1.0.0-beta.2x) or 32 big-endian bytes (later);
* function inputs: ``{"input": {"Witness"|"Constant": x}, "num_bits": n}`` (old) or
  ``{"Witness"|"Constant": x}`` (new; ``RANGE`` / ``AND`` / ``XOR`` then carry ``num_bits``);
* memory operations: ``{"operation", "index", "value"}`` expressions with an optional ``predicate``
  (old) or ``{"read": <is write>, "index": w, "value": w}`` (new: the bool field is serde-renamed
  ``read`` but holds ``MemOpKind::Write``).

Internal opcodes (``op["kind"]``):

* ``assert_zero``  ``expr`` = Σ q·w_a·w_b + Σ q·w + q_c must be 0;
* ``range``        ``input`` < 2^``bits``;
* ``and`` / ``xor``  operands below 2^``bits`` and ``output`` = bitwise op of the operands;
* ``bb``           black box ``name`` with ``key`` (name + static parameters), ``inputs`` (witnesses or
  constants, in serde field order), ``outputs`` (witnesses) and optional ``predicate``: when the
  predicate is a witness, the call constrains only if it is non-zero;
* ``mem_init``     ``block`` initialized from witnesses ``init``;
* ``mem_op``       ``block``, ``write`` (expression or constant 0/1), ``index``, ``value`` expressions and
  optional ``predicate`` (old versions: a zero predicate skips the operation);
* ``brillig``      unconstrained call: its output witnesses are free (no constraint);
* ``call``         ACIR call to another function of the program (flattened by :func:`flatten`).

The evaluator implements the same semantics as the Lean model (``noir_lean_emit``), so G-FID compares
two independent implementations of one definition.
"""
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field

BN254 = 21888242871839275222246405745257275088548364400416034343698204186575808495617

BB_INTERPRETED = ("RANGE", "AND", "XOR")


class Unsupported(ValueError):
    """An opcode or shape without a model."""


def fe(v) -> int:
    """A field element in any serialized shape."""
    if isinstance(v, bool):
        return int(v)
    if isinstance(v, int):
        return v
    if isinstance(v, str):
        s = v[2:] if v.startswith("0x") else v
        return int(s, 16) if s else 0
    if isinstance(v, list) and all(isinstance(b, int) for b in v):
        return int.from_bytes(bytes(v), "big")
    raise Unsupported(f"field element shape {v!r:.80}")


@dataclass(frozen=True)
class Expr:
    mul: tuple            # ((q, a, b), ...)
    lin: tuple            # ((q, w), ...)
    const: int

    def witnesses(self) -> set[int]:
        return {a for _, a, _ in self.mul} | {b for _, _, b in self.mul} | {w for _, w in self.lin}

    def eval(self, w, p: int = BN254) -> int:
        return (sum(q * w[a] * w[b] for q, a, b in self.mul) + sum(q * w[i] for q, i in self.lin) + self.const) % p

    def as_const(self) -> int | None:
        return None if self.mul or self.lin else self.const

    def as_witness(self) -> int | None:
        if not self.mul and len(self.lin) == 1 and self.lin[0][0] == 1 and self.const == 0:
            return self.lin[0][1]
        return None

    def to_json(self) -> dict:
        return {"mul": [list(t) for t in self.mul], "lin": [list(t) for t in self.lin], "const": self.const}


def expr(d: dict, p: int = BN254) -> Expr:
    mul = tuple((fe(q) % p, int(a), int(b)) for q, a, b in d.get("mul_terms", []))
    lin = tuple((fe(q) % p, int(w)) for q, w in d.get("linear_combinations", []))
    return Expr(mul, lin, fe(d.get("q_c", 0)) % p)


def witness_expr(w: int) -> Expr:
    return Expr((), ((1, w),), 0)


def const_expr(c: int) -> Expr:
    return Expr((), (), c)


# ------------------------------------------------------------------------------------------ inputs

def function_input(v) -> tuple[tuple[str, int], int | None]:
    """(('w', index) | ('c', value), num_bits or None)."""
    if isinstance(v, dict) and "input" in v:
        inner, _ = function_input(v["input"])
        return inner, v.get("num_bits")
    if isinstance(v, dict) and len(v) == 1:
        (k, x), = v.items()
        if k == "Witness":
            return ("w", int(x)), None
        if k == "Constant":
            return ("c", fe(x)), None
    raise Unsupported(f"function input shape {v!r:.80}")


def is_function_input(v) -> bool:
    try:
        function_input(v)
        return True
    except Unsupported:
        return False


def _inputs_in(v) -> list:
    """Every function input inside a field value (lists and nested lists flattened)."""
    if is_function_input(v):
        return [function_input(v)[0]]
    if isinstance(v, list):
        out = []
        for x in v:
            out += _inputs_in(x)
        return out
    raise Unsupported(f"black-box input field {v!r:.80}")


def _witnesses_in(v) -> list[int]:
    if isinstance(v, bool):
        raise Unsupported("bool in output position")
    if isinstance(v, int):
        return [v]
    if isinstance(v, list):
        out = []
        for x in v:
            out += _witnesses_in(x)
        return out
    raise Unsupported(f"black-box output field {v!r:.80}")


def black_box(name: str, d: dict) -> dict:
    if name == "RANGE":
        inp, bits = function_input(d["input"])
        return {"kind": "range", "input": inp, "bits": int(d.get("num_bits", bits))}
    if name in ("AND", "XOR"):
        lhs, b1 = function_input(d["lhs"])
        rhs, b2 = function_input(d["rhs"])
        bits = d.get("num_bits", b1 if b1 is not None else b2)
        return {"kind": name.lower(), "lhs": lhs, "rhs": rhs, "bits": int(bits), "output": int(d["output"])}
    if name.startswith("BigInt"):
        raise Unsupported(f"black box {name} (bigint ids, not witnesses) has no model")
    inputs, outputs, static, predicate = [], [], {}, None
    for key in sorted(d):
        val = d[key]
        if key == "predicate":
            predicate = function_input(val)[0] if val is not None else None
        elif key in ("output", "outputs"):
            outputs += _witnesses_in(val)
        elif isinstance(val, (int, str)) and not isinstance(val, bool) and key not in ("inputs",):
            static[key] = val
        else:
            inputs += _inputs_in(val)
    bb_key = name + ("(" + ",".join(f"{k}={static[k]}" for k in sorted(static)) + ")" if static else "")
    return {"kind": "bb", "name": name, "key": bb_key, "inputs": inputs, "outputs": outputs, "predicate": predicate,
            "n_inputs": len(inputs)}


def _mem_op(d: dict) -> dict:
    op = d["op"]
    pred = d.get("predicate")
    predicate = expr(pred) if isinstance(pred, dict) else None
    if "operation" in op:
        return {"kind": "mem_op", "block": int(d["block_id"]), "write": expr(op["operation"]), "index": expr(op["index"]),
                "value": expr(op["value"]), "predicate": predicate}
    if "read" in op:      # serde-renamed field holding MemOpKind::Write as `true`
        return {"kind": "mem_op", "block": int(d["block_id"]), "write": const_expr(1 if op["read"] else 0),
                "index": witness_expr(int(op["index"])), "value": witness_expr(int(op["value"])), "predicate": predicate}
    raise Unsupported(f"memory op shape {op!r:.80}")


def _brillig_outputs(outs) -> list[int]:
    ws = []
    for o in outs:
        (k, v), = o.items()
        ws += _witnesses_in(v)
    return ws


def opcode(raw: dict) -> dict:
    (k, d), = raw.items()
    if k == "AssertZero":
        return {"kind": "assert_zero", "expr": expr(d)}
    if k == "BlackBoxFuncCall":
        (name, body), = d.items()
        return black_box(name, body)
    if k == "MemoryInit":
        return {"kind": "mem_init", "block": int(d["block_id"]), "init": [int(w) for w in d["init"]],
                "block_type": json.dumps(d.get("block_type"), sort_keys=True)}
    if k == "MemoryOp":
        return _mem_op(d)
    if k == "BrilligCall":
        pred = d.get("predicate")
        return {"kind": "brillig", "id": int(d["id"]), "outputs": _brillig_outputs(d.get("outputs", [])),
                "predicate": expr(pred) if isinstance(pred, dict) else None}
    if k == "Call":
        pred = d.get("predicate")
        return {"kind": "call", "id": int(d["id"]), "inputs": [int(w) for w in d["inputs"]],
                "outputs": [int(w) for w in d["outputs"]], "predicate": expr(pred) if isinstance(pred, dict) else None}
    raise Unsupported(f"opcode {k} has no model")


def _wset(v) -> list[int]:
    if isinstance(v, dict):         # PublicInputs(BTreeSet) may serialize as an object in some versions
        v = next(iter(v.values()), [])
    return sorted(int(x) for x in v)


@dataclass
class Function:
    opcodes: list
    private: list[int]
    public: list[int]
    returns: list[int]
    n_witnesses: int
    name: str = ""


@dataclass
class Program:
    functions: list[Function]
    abi: dict
    noir_version: str
    oracles: list[str] = field(default_factory=list)
    n_brillig: int = 0
    raw_sha256: str = ""

    @property
    def main(self) -> Function:
        return self.functions[0]


def _max_witness(ops: list[dict]) -> int:
    m = -1
    for op in ops:
        k = op["kind"]
        ws: set[int] = set()
        if k == "assert_zero":
            ws = op["expr"].witnesses()
        elif k == "range":
            ws = {op["input"][1]} if op["input"][0] == "w" else set()
        elif k in ("and", "xor"):
            ws = {x[1] for x in (op["lhs"], op["rhs"]) if x[0] == "w"} | {op["output"]}
        elif k == "bb":
            ws = {x[1] for x in op["inputs"] if x[0] == "w"} | set(op["outputs"])
            if op["predicate"] and op["predicate"][0] == "w":
                ws.add(op["predicate"][1])
        elif k == "mem_init":
            ws = set(op["init"])
        elif k == "mem_op":
            ws = op["write"].witnesses() | op["index"].witnesses() | op["value"].witnesses()
            if op["predicate"] is not None:
                ws |= op["predicate"].witnesses()
        elif k == "brillig":
            ws = set(op["outputs"])
        elif k == "call":
            ws = set(op["inputs"]) | set(op["outputs"])
        if ws:
            m = max(m, max(ws))
    return m


def normalize_function(d: dict) -> Function:
    ops = [opcode(o) for o in d["opcodes"]]
    private, public, returns = _wset(d.get("private_parameters", [])), _wset(d.get("public_parameters", [])), \
        _wset(d.get("return_values", []))
    top = max([_max_witness(ops)] + private + public + returns + [int(d.get("current_witness_index", -1))])
    return Function(ops, private, public, returns, top + 1, d.get("function_name", ""))


def normalize_program(decoded: dict) -> Program:
    raw = json.dumps(decoded["functions"], sort_keys=True, separators=(",", ":"))
    return Program([normalize_function(f) for f in decoded["functions"]], decoded.get("abi", {}),
                   str(decoded.get("noir_version", "")), list(decoded.get("oracles", [])),
                   len(decoded.get("unconstrained_functions", [])), hashlib.sha256(raw.encode()).hexdigest())


# ------------------------------------------------------------------------------------------ flattening

@dataclass
class Flat:
    """The main function with ACIR calls inlined: callee witnesses get fresh indices after the caller's."""
    opcodes: list
    inputs: list[int]
    outputs: list[int]
    n_witnesses: int
    call_sites: list = field(default_factory=list)   # (path, callee, offset) in execution order

    @property
    def size(self) -> int:
        return len(self.opcodes)


def _shift_input(x, off):
    return (x[0], x[1] + off) if x[0] == "w" else x


def _shift_expr(e: Expr, off: int) -> Expr:
    return Expr(tuple((q, a + off, b + off) for q, a, b in e.mul), tuple((q, w + off) for q, w in e.lin), e.const)


def _shift(op: dict, off: int, block_off: int) -> dict:
    k = op["kind"]
    o = dict(op)
    if k == "assert_zero":
        o["expr"] = _shift_expr(op["expr"], off)
    elif k == "range":
        o["input"] = _shift_input(op["input"], off)
    elif k in ("and", "xor"):
        o.update(lhs=_shift_input(op["lhs"], off), rhs=_shift_input(op["rhs"], off), output=op["output"] + off)
    elif k == "bb":
        o.update(inputs=[_shift_input(x, off) for x in op["inputs"]], outputs=[w + off for w in op["outputs"]],
                 predicate=_shift_input(op["predicate"], off) if op["predicate"] else None)
    elif k == "mem_init":
        o.update(block=op["block"] + block_off, init=[w + off for w in op["init"]])
    elif k == "mem_op":
        o.update(block=op["block"] + block_off, write=_shift_expr(op["write"], off), index=_shift_expr(op["index"], off),
                 value=_shift_expr(op["value"], off),
                 predicate=_shift_expr(op["predicate"], off) if op["predicate"] is not None else None)
    elif k == "brillig":
        o["outputs"] = [w + off for w in op["outputs"]]
    elif k == "call":
        o.update(inputs=[w + off for w in op["inputs"]], outputs=[w + off for w in op["outputs"]])
    return o


def flatten(prog: Program, max_ops: int | None = None) -> Flat:
    """Inline every ACIR call of the main function (recursively).  A call's callee parameters are
    ``Witness(0..n)`` of the callee (ACVM's call convention), its return witnesses are matched in order to
    the call outputs.  Calls with a predicate that is not the constant 1 are not supported."""
    out: list = []
    sites: list = []
    state = {"next": prog.main.n_witnesses, "blocks": 0}

    def inline(fi: int, off: int, path: tuple) -> None:
        f = prog.functions[fi]
        block_off = state["blocks"]
        blocks = {op["block"] for op in f.opcodes if op["kind"] in ("mem_init", "mem_op")}
        state["blocks"] += (max(blocks) + 1) if blocks else 0
        seq = 0
        for k, op in enumerate(f.opcodes):
            if op["kind"] != "call":
                sop = _shift(op, off, block_off)
                sop["src"] = (fi, k, off)            # function, opcode index, witness offset (for bbeval)
                out.append(sop)
                if max_ops is not None and len(out) > max_ops:
                    raise OverflowError("flattened program exceeds the size bound")
                continue
            pred = op["predicate"]
            if pred is not None and pred.as_const() != 1:
                raise Unsupported("ACIR call with a predicate other than the constant 1")
            callee = prog.functions[op["id"]]
            coff = state["next"]
            state["next"] += callee.n_witnesses
            sub = path + ((fi, k, seq),)
            seq += 1
            sites.append((sub, op["id"], coff))
            for i, w in enumerate(op["inputs"]):
                out.append({"kind": "assert_zero", "expr": Expr((), ((1, w + off), (BN254 - 1, i + coff)), 0),
                            "origin": "call-input"})
            inline(op["id"], coff, sub)
            for w_out, w_ret in zip(op["outputs"], callee.returns):
                out.append({"kind": "assert_zero", "expr": Expr((), ((1, w_out + off), (BN254 - 1, w_ret + coff)), 0),
                            "origin": "call-output"})

    inline(0, 0, ())
    inputs = sorted(set(prog.main.private) | set(prog.main.public))
    return Flat(out, inputs, list(prog.main.returns), state["next"], sites)


def flat_witness(flat: Flat, main: dict[int, int], calls: list[dict]) -> list[int] | None:
    """A full assignment of the flattened program from an execution record (main witness map plus the
    call records of the tool, matched to call sites by their path).  Missing witnesses (never assigned by
    the solver) are 0."""
    w = [0] * flat.n_witnesses
    for i, v in main.items():
        if i < len(w):
            w[i] = v
    by_path = {tuple(tuple(x) for x in c["path"]): c for c in calls}
    for path, callee, off in flat.call_sites:
        c = by_path.get(path)
        if c is None:
            return None
        for i, v in c["witness"].items():
            w[off + i] = v
    return w


# ------------------------------------------------------------------------------------------ evaluation

def input_value(x, w) -> int:
    return x[1] if x[0] == "c" else w[x[1]]


class Interp:
    """Black-box interpretation: a table from (key, inputs) to outputs (the real function values, from
    executions and ``bbeval``).  A missing entry yields ``None`` (the call cannot be satisfied)."""

    def __init__(self, table: dict | None = None):
        self.table = dict(table or {})

    def __call__(self, key: str, ins: tuple) -> tuple | None:
        return self.table.get((key, tuple(ins)))

    def add(self, key: str, ins, outs) -> None:
        self.table[(key, tuple(ins))] = tuple(outs)


def check(ops: list[dict], w, interp=None, p: int = BN254, first_failure: bool = True):
    """(ok, index of the first violated opcode or None).  ``w`` is indexable by witness."""
    mem: dict[int, list[int]] = {}
    for k, op in enumerate(ops):
        kind = op["kind"]
        ok = True
        if kind == "assert_zero":
            ok = op["expr"].eval(w, p) == 0
        elif kind == "range":
            ok = input_value(op["input"], w) < (1 << op["bits"])
        elif kind in ("and", "xor"):
            a, b = input_value(op["lhs"], w), input_value(op["rhs"], w)
            bound = 1 << op["bits"]
            r = (a & b) if kind == "and" else (a ^ b)
            ok = a < bound and b < bound and w[op["output"]] == r
        elif kind == "bb":
            pred = op["predicate"]
            if pred is None or input_value(pred, w) % p != 0:
                ins = tuple(input_value(x, w) for x in op["inputs"])
                outs = interp(op["key"], ins) if interp is not None else None
                ok = outs is not None and tuple(w[o] for o in op["outputs"]) == tuple(outs)
        elif kind == "mem_init":
            mem[op["block"]] = [w[i] for i in op["init"]]
        elif kind == "mem_op":
            pred = op["predicate"]
            if pred is None or pred.eval(w, p) != 0:
                block = mem.get(op["block"])
                wr, idx = op["write"].eval(w, p), op["index"].eval(w, p)
                if block is None or idx >= len(block) or wr not in (0, 1):
                    ok = False
                elif wr == 1:
                    block[idx] = op["value"].eval(w, p)
                else:
                    ok = op["value"].eval(w, p) == block[idx]
        elif kind == "brillig":
            ok = True
        else:
            raise Unsupported(f"cannot evaluate {kind}")
        if not ok:
            return False, k
    return True, None


def bb_calls(ops: list[dict], w) -> list[tuple[str, tuple, tuple, int]]:
    """(key, inputs, outputs, opcode index) of every active black-box call under assignment ``w``."""
    out = []
    for k, op in enumerate(ops):
        if op["kind"] != "bb":
            continue
        pred = op["predicate"]
        if pred is not None and input_value(pred, w) % BN254 == 0:
            continue
        out.append((op["key"], tuple(input_value(x, w) for x in op["inputs"]), tuple(w[o] for o in op["outputs"]), k))
    return out


def stats(ops: list[dict]) -> dict:
    c: dict[str, int] = {}
    for op in ops:
        name = op["kind"] if op["kind"] != "bb" else "bb:" + op["name"]
        c[name] = c.get(name, 0) + 1
    return dict(sorted(c.items()))


def bb_keys(ops: list[dict]) -> list[str]:
    """Distinct black-box keys in order of first use; the Lean model numbers them 0, 1, ..."""
    seen: list[str] = []
    for op in ops:
        if op["kind"] == "bb" and op["key"] not in seen:
            seen.append(op["key"])
    return seen
