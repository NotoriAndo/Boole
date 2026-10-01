"""Extracted Plonky3-style AIR intermediate representation (``boole-air-ir/v1``) and an independent evaluator.

The Rust extractor (``air_harness/``) runs a zkVM's own symbolic builders on every AIR and writes one JSON
document per AIR.  Expressions are a hash-consed DAG: ``nodes`` is a list whose entries only refer to earlier
entries, so evaluation is one pass in list order.  Node forms::

    ["main", offset, column]      main trace cell of the local (offset 0) or next (offset 1) row
    ["prep", offset, column]      preprocessed trace cell
    ["pub", index]                public value
    ["first"] | ["last"] | ["trans"]   row selectors (is_first_row, is_last_row, is_transition)
    ["const", value]              canonical field element
    ["add", a, b] | ["sub", a, b] | ["mul", a, b] | ["neg", a]

``constraints`` are node ids of polynomials that must vanish (``when(..)`` guards and row selectors are
factors of the polynomial, exactly as the symbolic builder records them).  ``interactions`` are bus
messages: direction (``send`` / ``receive``), bus kind (numeric code and name) or bus index, scope, the
message value nodes and the multiplicity node.

The evaluator in this module shares no code with the Lean emitter; agreement of the two on real rows is
the fidelity evidence of the Lean model (gate G-FID).
"""
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field

FORMAT = "boole-air-ir/v1"
FIELDS = {"BabyBear": 2013265921, "KoalaBear": 2130706433}
LEAF_OPS = {"main", "prep", "pub", "first", "last", "trans", "const"}
BIN_OPS = {"add", "sub", "mul"}
SELECTORS = ("first", "last", "trans")


class IrError(ValueError):
    """The document is not a well-formed ``boole-air-ir/v1`` AIR."""


@dataclass
class Interaction:
    direction: str                  # "send" | "receive"
    kind: int | None                # bus kind code (SP1 InteractionKind, Pico LookupType); None for bus indices
    kind_name: str                  # kind name, or the bus name the configuration gives the index
    bus: int | None                 # bus index (OpenVM); None for kind-based buses
    scope: str | None               # "local" | "global" | None
    values: list[int]               # node ids
    mult: int                       # node id
    count_weight: int | None = None


@dataclass
class Air:
    zkvm: str
    release: str
    commit: str
    field_name: str
    p: int
    name: str
    rust_type: str
    group: str                      # "riscv" | "recursion" | "system" | "extension" ...
    index: int                      # position in the machine's AIR list
    width: int
    prep_width: int
    n_public: int
    nodes: list[list]
    constraints: list[int]
    interactions: list[Interaction]
    meta: dict = field(default_factory=dict)

    # ------------------------------------------------------------------ structure

    def uses(self) -> dict:
        """Which window parts the constraints and interactions reference."""
        u = {"main_next": False, "prep_next": False, "public": set(), "selectors": set(), "prep": False}
        for n in self.nodes:
            op = n[0]
            if op == "main" and n[1] == 1:
                u["main_next"] = True
            elif op == "prep":
                u["prep"] = True
                if n[1] == 1:
                    u["prep_next"] = True
            elif op == "pub":
                u["public"].add(n[1])
            elif op in SELECTORS:
                u["selectors"].add(op)
        return u

    def window_rows(self) -> int:
        u = self.uses()
        return 2 if (u["main_next"] or u["prep_next"]) else 1

    def degree(self) -> int:
        deg: list[int] = []
        for n in self.nodes:
            op = n[0]
            if op == "const":
                deg.append(0)
            elif op in ("main", "prep", "pub"):
                deg.append(1)
            elif op in SELECTORS:
                deg.append(1)
            elif op == "neg":
                deg.append(deg[n[1]])
            elif op == "mul":
                deg.append(deg[n[1]] + deg[n[2]])
            else:
                deg.append(max(deg[n[1]], deg[n[2]]))
        return max((deg[c] for c in self.constraints), default=0)

    def content_sha256(self) -> str:
        """Hash of everything that defines the statement (field, widths, nodes, constraints, interactions)."""
        body = {"p": self.p, "width": self.width, "prep_width": self.prep_width, "n_public": self.n_public,
                "nodes": self.nodes, "constraints": self.constraints,
                "interactions": [[i.direction, i.kind, i.kind_name, i.bus, i.scope, i.values, i.mult, i.count_weight]
                                 for i in self.interactions]}
        return hashlib.sha256(json.dumps(body, separators=(",", ":"), sort_keys=True).encode()).hexdigest()


def _check_node(k: int, n, widths: dict) -> None:
    if not isinstance(n, list) or not n or not isinstance(n[0], str):
        raise IrError(f"node {k}: not a list with an operator")
    op = n[0]
    if op in ("main", "prep"):
        if len(n) != 3 or n[1] not in (0, 1) or not isinstance(n[2], int):
            raise IrError(f"node {k}: bad {op} variable {n}")
        lim = widths["main" if op == "main" else "prep"]
        if not 0 <= n[2] < lim:
            raise IrError(f"node {k}: {op} column {n[2]} outside width {lim}")
    elif op == "pub":
        if len(n) != 2 or not isinstance(n[1], int) or not 0 <= n[1] < widths["pub"]:
            raise IrError(f"node {k}: bad public value {n}")
    elif op in SELECTORS:
        if len(n) != 1:
            raise IrError(f"node {k}: selector with arguments")
    elif op == "const":
        if len(n) != 2 or not isinstance(n[1], int) or not 0 <= n[1] < widths["p"]:
            raise IrError(f"node {k}: constant not canonical {n}")
    elif op in BIN_OPS:
        if len(n) != 3 or not all(isinstance(x, int) and 0 <= x < k for x in n[1:]):
            raise IrError(f"node {k}: {op} operands must be earlier nodes")
    elif op == "neg":
        if len(n) != 2 or not isinstance(n[1], int) or not 0 <= n[1] < k:
            raise IrError(f"node {k}: neg operand must be an earlier node")
    else:
        raise IrError(f"node {k}: unknown operator {op!r}")


def from_json(doc: dict) -> Air:
    if doc.get("format") != FORMAT:
        raise IrError(f"format is {doc.get('format')!r}, expected {FORMAT}")
    fld = doc["field"]
    p = int(fld["p"])
    if FIELDS.get(fld["name"]) != p:
        raise IrError(f"unknown field {fld['name']} / {p}")
    air = doc["air"]
    widths = {"main": int(doc["width"]), "prep": int(doc["preprocessed_width"]), "pub": int(doc["num_public_values"]),
              "p": p}
    nodes = doc["nodes"]
    for k, n in enumerate(nodes):
        _check_node(k, n, widths)
    nn = len(nodes)
    cons = doc["constraints"]
    if not all(isinstance(c, int) and 0 <= c < nn for c in cons):
        raise IrError("constraint ids out of range")
    inters = []
    for j, it in enumerate(doc["interactions"]):
        if it["dir"] not in ("send", "receive"):
            raise IrError(f"interaction {j}: direction {it['dir']!r}")
        ids = list(it["values"]) + [it["mult"]]
        if not all(isinstance(c, int) and 0 <= c < nn for c in ids):
            raise IrError(f"interaction {j}: node ids out of range")
        inters.append(Interaction(it["dir"], it.get("kind"), it.get("kind_name") or "", it.get("bus"), it.get("scope"),
                                  list(it["values"]), it["mult"], it.get("count_weight")))
    return Air(doc["zkvm"], doc["release"], doc["commit"], fld["name"], p, air["name"], air.get("rust_type", air["name"]),
               air.get("group", ""), int(air.get("index", 0)), widths["main"], widths["prep"], widths["pub"], nodes,
               list(cons), inters, doc.get("meta", {}))


def load(path: str) -> Air:
    with open(path, encoding="utf-8") as f:
        return from_json(json.load(f))


# ------------------------------------------------------------------------------------------ windows

@dataclass
class Layout:
    """Flat variable vector of one window (the Lean model's ``w : Fin nVars → F``)."""
    width: int
    prep_width: int
    main_next: bool
    prep_next: bool
    public: list[int]
    selectors: list[str]

    @staticmethod
    def of(air: Air) -> "Layout":
        u = air.uses()
        return Layout(air.width, air.prep_width if u["prep"] else 0, u["main_next"], u["prep_next"] and u["prep"],
                      sorted(u["public"]), [s for s in SELECTORS if s in u["selectors"]])

    def offsets(self) -> dict:
        o, k = {}, 0
        o["main0"] = k
        k += self.width
        if self.main_next:
            o["main1"] = k
            k += self.width
        o["prep0"] = k
        k += self.prep_width
        if self.prep_next:
            o["prep1"] = k
            k += self.prep_width
        o["pub"] = k
        k += len(self.public)
        o["sel"] = k
        k += len(self.selectors)
        o["n"] = k
        return o

    @property
    def n_vars(self) -> int:
        return self.offsets()["n"]

    def var(self, node: list) -> int:
        o = self.offsets()
        op = node[0]
        if op == "main":
            if node[1] == 1 and not self.main_next:
                raise IrError("next-row main cell in a one-row layout")
            return o[f"main{node[1]}"] + node[2]
        if op == "prep":
            return o[f"prep{node[1]}"] + node[2]
        if op == "pub":
            return o["pub"] + self.public.index(node[1])
        if op in SELECTORS:
            return o["sel"] + self.selectors.index(op)
        raise IrError(f"not a variable: {node}")

    def fixed(self) -> list[int]:
        """Variables both assignments share: preprocessed cells, public values and row selectors."""
        o = self.offsets()
        return list(range(o["prep0"], o["n"]))

    def names(self) -> list[str]:
        out = [f"main[{i}]" for i in range(self.width)]
        if self.main_next:
            out += [f"main[{i}] (next row)" for i in range(self.width)]
        out += [f"preprocessed[{i}]" for i in range(self.prep_width)]
        if self.prep_next:
            out += [f"preprocessed[{i}] (next row)" for i in range(self.prep_width)]
        out += [f"public[{i}]" for i in self.public]
        out += [{"first": "is_first_row", "last": "is_last_row", "trans": "is_transition"}[s] for s in self.selectors]
        return out

    def selector_values(self, row: int, height: int) -> dict[str, int]:
        """Normalized selectors of row ``row`` in a trace of ``height`` rows."""
        return {"first": int(row == 0), "last": int(row == height - 1), "trans": int(row != height - 1)}

    def window(self, main_rows: list[list[int]], prep_rows: list[list[int]] | None, public: list[int],
               row: int, height: int) -> list[int]:
        """Flat assignment for the window starting at ``row`` (the next row wraps around, as in the prover)."""
        nxt = (row + 1) % height
        w = list(main_rows[row])
        if self.main_next:
            w += list(main_rows[nxt])
        if self.prep_width:
            w += list(prep_rows[row])
            if self.prep_next:
                w += list(prep_rows[nxt])
        w += [public[i] for i in self.public]
        sv = self.selector_values(row, height)
        w += [sv[s] for s in self.selectors]
        if len(w) != self.n_vars:
            raise IrError(f"window has {len(w)} values, layout needs {self.n_vars}")
        return w


def eval_nodes(air: Air, layout: Layout, w: list[int]) -> list[int]:
    """Value of every node under the flat assignment ``w`` (one pass in node order)."""
    p = air.p
    val: list[int] = []
    for n in air.nodes:
        op = n[0]
        if op == "const":
            val.append(n[1])
        elif op in ("main", "prep", "pub") or op in SELECTORS:
            val.append(w[layout.var(n)] % p)
        elif op == "add":
            val.append((val[n[1]] + val[n[2]]) % p)
        elif op == "sub":
            val.append((val[n[1]] - val[n[2]]) % p)
        elif op == "mul":
            val.append(val[n[1]] * val[n[2]] % p)
        elif op == "neg":
            val.append((-val[n[1]]) % p)
        else:
            raise IrError(f"unknown operator {op!r}")
    return val


def failing_constraints(air: Air, layout: Layout, w: list[int], vals: list[int] | None = None) -> list[int]:
    vals = vals if vals is not None else eval_nodes(air, layout, w)
    return [k for k, c in enumerate(air.constraints) if vals[c] != 0]


def satisfies(air: Air, layout: Layout, w: list[int]) -> bool:
    return not failing_constraints(air, layout, w)


def message(air: Air, it: Interaction, vals: list[int]) -> tuple[int, list[int]]:
    return vals[it.mult], [vals[v] for v in it.values]


def tree_size(air: Air, node: int, cap: int = 10**9) -> int:
    """Size of the node when printed as a tree (shared subterms counted at every use), capped."""
    sizes: dict[int, int] = {}
    for k in range(node + 1):
        n = air.nodes[k]
        op = n[0]
        if op in LEAF_OPS:
            s = 1
        elif op == "neg":
            s = 1 + sizes[n[1]]
        else:
            s = 1 + sizes[n[1]] + sizes[n[2]]
        sizes[k] = min(s, cap)
    return sizes[node]


def tree_sizes(air: Air, cap: int = 10**9) -> list[int]:
    sizes: list[int] = []
    for n in air.nodes:
        op = n[0]
        if op in LEAF_OPS:
            s = 1
        elif op == "neg":
            s = 1 + sizes[n[1]]
        else:
            s = 1 + sizes[n[1]] + sizes[n[2]]
        sizes.append(min(s, cap))
    return sizes
