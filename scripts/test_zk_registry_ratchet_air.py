#!/usr/bin/env python3
"""AIR ratchet problems: the proving-cost vector and its component-wise order, the bus-interface rule (role
inheritance, the negative-multiplicity rule, no new tables), the source rules of a candidate overlay, the build-tree
synchronization, admissibility of an extraction (machine, target, window, fixed layout), the problem package, the
generated statement and checker package, and the simulation screen's comparisons (rows, contributions, DET search).

The toy reference is an SP1-style row AIR ``Toy`` (position 1 of a three-AIR machine): columns ``a, b, c, t,
is_real``; ``t = a * b`` and ``c = t``; it receives ``(a, b)`` on the State bus, sends a byte lookup of ``a, b`` and
sends ``c``.  Candidates: ``equivalent`` (no ``t``: ``c = a * b``; fewer columns and constraints), ``nonequivalent``
(``c = a * b + 1``), ``underconstrained`` (``c`` free) and interface variants.  No Rust build, Lean or zkVM is needed.
``LiveTests`` re-runs the pipeline on a real problem when ``BOOLE_ZK_RATCHET_AIR_PROBLEM`` (a problem directory),
``BOOLE_ZK_RATCHET_AIR_CANDIDATE`` (an overlay that only edits a comment of the chip) and
``BOOLE_ZK_RATCHET_AIR_BUILD`` / ``BOOLE_ZK_RATCHET_AIR_REF_BUILD`` (build directories with ``repo/`` and
``target/``) are set; it is skipped otherwise."""
from __future__ import annotations

import collections
import json
import os
import shutil
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import air_bus as AB          # noqa: E402
from zk_registry import check as C             # noqa: E402
from zk_registry import lean_runner as L       # noqa: E402
from zk_registry import air_ir as IR           # noqa: E402
from zk_registry import air_lean as AL         # noqa: E402
from zk_registry import package as P           # noqa: E402
from zk_registry import ratchet as RT          # noqa: E402
from zk_registry import ratchet_air as RA      # noqa: E402

KB = IR.FIELDS["KoalaBear"]
H = "ab" * 32
ENV = {"lean": "v4.33.1", "mathlib": "c" * 40, "lake_manifest_sha256": H, "packages": {"mathlib": "c" * 40}}
FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "ratchet-air"
LIVE_LEAN = os.environ.get("BOOLE_ZK_RATCHET_LEAN_ENV")
LIVE_PROBLEM = os.environ.get("BOOLE_ZK_RATCHET_AIR_PROBLEM")
LIVE_CAND = os.environ.get("BOOLE_ZK_RATCHET_AIR_CANDIDATE")
LIVE_BUILD = os.environ.get("BOOLE_ZK_RATCHET_AIR_BUILD")
LIVE_REF_BUILD = os.environ.get("BOOLE_ZK_RATCHET_AIR_REF_BUILD")


class Dag:
    def __init__(self) -> None:
        self.nodes: list = []
        self.ids: dict = {}

    def node(self, *n) -> int:
        key = json.dumps(n)
        if key not in self.ids:
            self.ids[key] = len(self.nodes)
            self.nodes.append(list(n))
        return self.ids[key]

    def c(self, v: int) -> int:
        return self.node("const", v % KB)

    def v(self, col: int, row: int = 0) -> int:
        return self.node("main", row, col)


def inter(direction: str, kind: str, values: list, mult: int) -> dict:
    return {"dir": direction, "kind": {"State": 2, "Byte": 5, "Memory": 1}[kind], "kind_name": kind, "bus": None,
            "scope": "local", "values": values, "mult": mult, "count_weight": None}


def toy_doc(variant: str = "reference", index: int = 1, name: str = "Toy") -> dict:
    """The toy AIR and its candidate variants as boole-air-ir/v1 documents."""
    d = Dag()
    wide = variant in ("reference", "next-row")
    cols = {"a": 0, "b": 1, "c": 2, "t": 3, "is_real": 4} if wide else {"a": 0, "b": 1, "c": 2, "is_real": 3}
    a, b, c, r = d.v(cols["a"]), d.v(cols["b"]), d.v(cols["c"]), d.v(cols["is_real"])
    boolean = d.node("mul", r, d.node("sub", r, d.c(1)))
    ab = d.node("mul", a, b)
    cons = [boolean]
    if wide:
        t = d.v(cols["t"])
        cons += [d.node("sub", t, ab), d.node("sub", c, t)]
        if variant == "next-row":
            cons.append(d.node("mul", d.node("trans"), d.node("sub", d.v(cols["a"], 1), d.v(cols["a"], 1))))
    elif variant in ("equivalent", "two-lookups", "new-output", "renamed-bus", "fixed"):
        cons.append(d.node("sub", c, ab))
    elif variant == "nonequivalent":
        cons.append(d.node("sub", c, d.node("add", ab, d.c(1))))
    elif variant == "degree":
        cons.append(d.node("mul", r, d.node("sub", c, ab)))
    elif variant != "underconstrained":
        raise ValueError(variant)
    if variant == "fixed":
        cons.append(d.node("mul", d.node("first"), d.node("sub", c, c)))
    inters = [inter("receive", "State", [a, b], r),
              inter("send", "Byte", [d.c(3), d.c(0), a, b], r)]
    if variant == "two-lookups":
        inters.append(inter("send", "Byte", [d.c(3), d.c(0), c, d.c(0)], r))
    inters.append(inter("send", "Memory" if variant == "renamed-bus" else "State", [c], r))
    if variant == "new-output":
        inters.append(inter("send", "State", [a], r))
    return {"format": IR.FORMAT, "zkvm": "sp1", "release": "v6.8.1", "commit": "0" * 40,
            "field": {"name": "KoalaBear", "p": KB}, "air": {"name": name, "rust_type": name, "group": "riscv",
                                                            "index": index},
            "width": len(cols), "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": d.nodes,
            "constraints": cons, "interactions": inters}


def other_doc(index: int, name: str) -> dict:
    d = Dag()
    x = d.v(0)
    con = d.node("mul", x, d.node("sub", x, d.c(1)))
    return {"format": IR.FORMAT, "zkvm": "sp1", "release": "v6.8.1", "commit": "0" * 40,
            "field": {"name": "KoalaBear", "p": KB}, "air": {"name": name, "rust_type": name, "group": "riscv",
                                                            "index": index},
            "width": 1, "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": d.nodes,
            "constraints": [con], "interactions": []}


def air_of(doc: dict) -> IR.Air:
    return IR.from_json(json.loads(json.dumps(doc)))


def write_json(path: str, obj) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f)


def machine_dir(root: str, toy: dict, other: dict | None = None) -> str:
    """A fake extraction directory: manifest.jsonl and airs/<index>.json of a three-AIR machine."""
    docs = [other or other_doc(0, "Other0"), toy, other_doc(2, "Other2")]
    os.makedirs(os.path.join(root, "airs"), exist_ok=True)
    lines = []
    for k, doc in enumerate(docs):
        write_json(os.path.join(root, "airs", f"{k}.json"), doc)
        lines.append(json.dumps({"index": k, "name": doc["air"]["name"], "rust_type": doc["air"]["name"],
                                 "group": "riscv", "status": "extracted", "detail": ""}))
    with open(os.path.join(root, "manifest.jsonl"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    return root


PAIRS = [(0, 0), (1, 1), (2, 3), (15, 17), (255, 255), (128, 7), (9, 200), (77, 3)]


def rows_doc(variant: str, pairs=PAIRS, height: int = 16, source: str = "core record 0") -> dict:
    main = {}
    for i in range(height):
        if i < len(pairs):
            a, b = pairs[i]
            c = (a * b + (1 if variant == "nonequivalent" else 0)) % KB
            row = [a, b, c, a * b, 1] if variant == "reference" else [a, b, c, 1]
        else:
            row = [0] * (5 if variant == "reference" else 4)
            if variant == "nonequivalent":
                row[2] = 1                                      # c = a * b + 1 holds on padding rows too
        main[str(i)] = row
    return {"format": "boole-air-rows/v1", "air_index": 1, "name": "Toy", "source": source, "height": height,
            "width": len(main["0"]), "preprocessed_width": 0, "public_values": [], "main": main, "prep": {}}


def registry_package(root: str, env: dict | None = None) -> str:
    """A registry AIR DET package of the toy reference, built with the wave's emitters and bus model."""
    doc = toy_doc("reference")
    air = air_of(doc)
    bus = AB.model_for("sp1")
    roles, iface = bus.roles(air)
    pkg = os.path.join(root, "registry", "001.Toy")
    ir_path = os.path.join(pkg, "evidence", "air.ir.json")
    write_json(ir_path, doc)
    ns = P.lean_namespace("sp1-v6.8.1", "001.Toy")
    gen = {"name": "boole-zk-registry-air-det", "version": "1.0", "sources_sha256": H}
    builder = "a symbolic builder (fixture)"
    meta = {"zkvm_name": "SP1", "release": "v6.8.1", "generator": f"{gen['name']} v{gen['version']}",
            "repo_url": "https://example.invalid/sp1", "extractor": builder, "ir_sha256": P.sha256_file(ir_path)}
    model, _ = AL.emit_model(ns, meta, air, IR.Layout.of(air), roles, bus.tables)
    stmt = AL.emit_statement(ns, meta, air)
    mrel = AL.model_relpath(ns)
    RT.put(os.path.join(pkg, mrel), model)
    RT.put(os.path.join(pkg, "Statement.lean"), stmt)
    prob = {"package_id": "sp1-v6.8.1/001.Toy", "property": {"template": "DET"}, "status": "OPEN",
            "ids": {"zkvm": "sp1", "release": "v6.8.1", "repo": "fixture/sp1", "repo_url": meta["repo_url"],
                    "commit": "0" * 40, "air_name": "Toy", "air_index": 1, "rust_type": "Toy", "group": "riscv"},
            "air": {"window_rows": 1, "size_policy": {"within": True}, "extractor": {"builder": builder},
                    "ir_sha256": meta["ir_sha256"], "content_sha256": air.content_sha256()},
            "statement": {"model_file": mrel, "model_module": AL.model_module(ns), "theorem_fqn": ns + ".det"},
            "checker": {"statement_file": "Statement.lean", "lean_opts": ["-DautoImplicit=false"],
                        "files": [{"path": "Statement.lean", "role": "statement", "sha256": RT.sha(stmt.encode())},
                                  {"path": mrel, "role": "import", "module": AL.model_module(ns),
                                   "sha256": RT.sha(model.encode())}]},
            "env": dict(env or ENV), "interface": iface, "gates": {"G-FID": {"status": "PASS"}}, "generator": gen,
            "coverage": {"status": "none"}}
    write_json(os.path.join(pkg, "problem.json"), prob)
    return pkg


def snapshot(root: str) -> tuple[str, str]:
    tree = os.path.join(root, "tree")
    RT.put(os.path.join(tree, "crates/core/machine/src/toy.rs"), "// toy chip\npub fn eval() {}\n")
    RT.put(os.path.join(tree, "crates/core/machine/src/other.rs"), "pub fn other() {}\n")
    RT.put(os.path.join(tree, "crates/core/machine/Cargo.toml"), "[package]\nname = \"m\"\n")
    RT.put(os.path.join(tree, "boole-air-extract/src/main.rs"), "fn main() {}\n")
    RT.put(os.path.join(tree, "README.md"), "readme\n")
    os.symlink("README.md", os.path.join(tree, "LINK.md"))
    snap = os.path.join(root, "snapshots", "fixture@0")
    RA.make_snapshot(tree, snap)
    return snap, "fixture@0"


def build_problem(root: str, env: dict | None = None) -> tuple[str, dict]:
    reg = registry_package(root, env)
    ext = machine_dir(os.path.join(root, "extract"), toy_doc("reference"))
    shutil.copyfile(os.path.join(reg, "evidence", "air.ir.json"), os.path.join(ext, "airs", "1.json"))
    write_json(os.path.join(ext, "rows-all", "1", "0000.json"), rows_doc("reference"))
    write_json(os.path.join(ext, "rows-all", "1", "0001.json"), rows_doc("reference", PAIRS[::-1], 8, "core record 1"))
    snap, sid = snapshot(root)
    proof = os.path.join(root, "Solution.lean")
    RT.put(proof, "-- fixture DET proof\n")
    spec = RA.reference_spec(reg)
    det = RT.det_record("mech-p3", spec, proof, {"verdict": "PASS", "peak_rss_mb": 7000, "secs": 12.0},
                        "fixture", location="det-proofs/x/Solution.lean")
    brec = RA.build_record("sp1", "stable-aarch64-apple-darwin", "stable", "rustc 1.98.0 (fixture)", H,
                           "a symbolic builder (fixture)")
    out = os.path.join(root, "problems", "sp1", "001.Toy")
    prob = RA.build_problem(reg, det, ext, snap, sid, brec, out, {"aliases": []})
    return out, prob


def equivalent_package(root: str, pdir: str, prob: dict, env=None) -> tuple[str, dict, str]:
    """The checker package of the equivalent candidate (``<root>/cand/pkg``) and its statement text."""
    d = machine_dir(os.path.join(root, "candext"), toy_doc("equivalent"))
    ev = RA.evaluate(prob, pdir, d)
    rep = {"candidate": {"overlay_sha256": H, "files": ["crates/core/machine/src/toy.rs"], **ev["report"]}}
    out = os.path.join(root, "cand")
    RA.package_candidate(prob, pdir, rep, ev, out, env)
    return os.path.join(out, "pkg"), rep, Path(out, "pkg", "Statement.lean").read_text(encoding="utf-8")


def fixture_solution(statement: str) -> str:
    """The committed fixture proof of the toy equivalence (its tactic block) in the generated statement."""
    body = (FIX / "equivalent.proof.lean").read_text(encoding="utf-8")
    return statement.replace("\n  sorry\n", "\n" + body, 1)


# ------------------------------------------------------------------------------------------ metric

class MetricTests(unittest.TestCase):
    def test_toy_costs(self) -> None:
        bus = AB.model_for("sp1")
        ref = RA.cost_of(air_of(toy_doc("reference")), bus)
        self.assertEqual((ref["main_columns"], ref["interactions"], ref["constraints"], ref["constraints_deg_ge"],
                          ref["interaction_degree"], ref["max_degree"], ref["lookups"], ref["priced"]),
                         (5, 3, 3, {"2": 2}, 1, 2, 1, 11))
        eq = RA.cost_of(air_of(toy_doc("equivalent")), bus)
        self.assertEqual((eq["main_columns"], eq["constraints"], eq["constraints_deg_ge"], eq["priced"]),
                         (4, 2, {"2": 2}, 9))
        s = RA.score(ref, eq)
        self.assertTrue(s["smaller"])
        self.assertEqual(s["larger_components"], [])
        self.assertEqual(s["smaller_components"], ["main_columns", "constraints"])

    def test_componentwise_order(self) -> None:
        base = {"main_columns": 10, "interactions": 5, "constraints": 6, "constraints_deg_ge": {"2": 4, "3": 2},
                "interaction_degree": 1, "max_degree": 3}

        def c(**kw):
            x = json.loads(json.dumps(base))
            x.update(kw)
            x["priced"] = P.air_priced(x)
            return x
        b = c()
        self.assertFalse(P.air_cost_smaller(b, c()))                                    # equal is not smaller
        self.assertTrue(P.air_cost_smaller(b, c(main_columns=9)))
        # trading columns for a constraint of higher degree never counts
        self.assertFalse(P.air_cost_smaller(b, c(main_columns=8, constraints_deg_ge={"2": 4, "3": 2, "4": 1},
                                                 max_degree=4)))
        # lowering a degree counts (cumulative counts), raising one does not
        self.assertTrue(P.air_cost_smaller(b, c(constraints_deg_ge={"2": 4, "3": 1})))
        self.assertFalse(P.air_cost_smaller(b, c(constraints_deg_ge={"2": 5, "3": 2})))
        # fewer columns but more interactions (a lookup instead of a column) does not count
        self.assertFalse(P.air_cost_smaller(b, c(main_columns=9, interactions=6)))
        self.assertFalse(P.air_cost_smaller(b, c(main_columns=9, interaction_degree=2)))
        self.assertTrue(P.air_cost_smaller(b, c(interaction_degree=0)))
        larger, smaller = P.air_cost_compare(b, c(main_columns=8, interactions=6))
        self.assertEqual((larger, smaller), (["interactions"], ["main_columns"]))

    def test_cost_record_consistency_is_validated(self) -> None:
        good = RA.cost_of(air_of(toy_doc("reference")), AB.model_for("sp1"))
        self.assertEqual(P._air_cost_errors(good, "record"), [])
        bad = dict(good, priced=good["priced"] + 1)
        self.assertTrue(P._air_cost_errors(bad, "record"))
        bad = dict(good, constraints_deg_ge={"2": 1, "3": 2}, max_degree=3)
        self.assertTrue(P._air_cost_errors(bad, "record"))


# ------------------------------------------------------------------------------------------ interface

class InterfaceTests(unittest.TestCase):
    def prob(self) -> dict:
        air = air_of(toy_doc("reference"))
        bus = AB.model_for("sp1")
        _, iface = bus.roles(air)
        return {"reference": {"interface": iface, "negative": RA.reference_negative(air, iface, bus)}}

    def test_same_interface_and_fewer_or_more_lookups(self) -> None:
        bus = AB.model_for("sp1")
        prob = self.prob()
        ref = air_of(toy_doc("reference"))
        roles, iface = RA.candidate_roles(prob, ref, air_of(toy_doc("equivalent")), bus)
        self.assertEqual((len(roles.inputs), len(roles.outputs), len(roles.assumptions)), (1, 1, 1))
        roles, _ = RA.candidate_roles(prob, ref, air_of(toy_doc("two-lookups")), bus)
        self.assertEqual(len(roles.assumptions), 2)          # another lookup into a table the reference uses

    def test_interface_changes_are_rejected(self) -> None:
        bus = AB.model_for("sp1")
        prob = self.prob()
        ref = air_of(toy_doc("reference"))
        with self.assertRaisesRegex(RA.Reject, "non-lookup interactions"):
            RA.candidate_roles(prob, ref, air_of(toy_doc("new-output")), bus)
        with self.assertRaisesRegex(RA.Reject, "shape"):
            RA.candidate_roles(prob, ref, air_of(toy_doc("renamed-bus")), bus)

    def test_no_new_tables(self) -> None:
        bus = AB.model_for("sp1")
        doc = toy_doc("equivalent")
        doc["interactions"] = [it for it in doc["interactions"] if it["kind_name"] != "Byte"]
        ref = air_of(doc)
        _, iface = bus.roles(ref)
        prob = {"reference": {"interface": iface, "negative": []}}
        with self.assertRaisesRegex(RA.Reject, "tables the reference does not use"):
            RA.candidate_roles(prob, ref, air_of(toy_doc("equivalent")), bus)

    def test_negative_multiplicity_rule_is_inherited(self) -> None:
        """OpenVM: a send whose multiplicity is negated on real rows is a receive; the candidate's matching message
        keeps that role without rows."""
        d = Dag()
        x, m = d.v(0), d.v(1)
        doc = {"format": IR.FORMAT, "zkvm": "openvm", "release": "v2.0.2", "commit": "0" * 40,
               "field": {"name": "BabyBear", "p": IR.FIELDS["BabyBear"]},
               "air": {"name": "T", "rust_type": "T", "group": "app-vm", "index": 0}, "width": 2,
               "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": d.nodes,
               "constraints": [], "interactions": [{"dir": "send", "kind": None, "kind_name": "", "bus": 9,
                                                    "scope": None, "values": [x], "mult": m, "count_weight": 1}]}
        air = air_of(doc)
        bus = AB.model_for("openvm")
        roles, iface = bus.roles(air, frozenset({0}))
        self.assertEqual(iface["inputs"][0]["role"], "in")
        neg = RA.reference_negative(air, iface, bus)
        self.assertEqual(neg, [0])
        croles, ciface = RA.candidate_roles({"reference": {"interface": iface, "negative": neg}}, air, air, bus)
        self.assertEqual((len(croles.inputs), len(croles.outputs), ciface["negative"]), (1, 0, [0]))
        with self.assertRaisesRegex(RA.Reject, "inputs differ|outputs differ"):
            RA.candidate_roles({"reference": {"interface": iface, "negative": []}}, air, air, bus)


# ------------------------------------------------------------------------------------------ source rules

class SourceRuleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp(prefix="ratchet-air-src-")
        self.snap, _ = snapshot(self.tmp)
        self.prob = {"snapshot": {"manifest_sha256": P.sha256_file(os.path.join(self.snap, "MANIFEST.sha256"))},
                     "build": RA.build_record("sp1", "stable", "stable", "rustc", H, "x")}

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def cand(self, files: dict) -> str:
        d = tempfile.mkdtemp(dir=self.tmp)
        for rel, text in files.items():
            RT.put(os.path.join(d, rel), text)
        return d

    def test_overlay_is_the_changed_files(self) -> None:
        d = self.cand({"crates/core/machine/src/toy.rs": "// toy chip, faster\npub fn eval() {}\n",
                       "crates/core/machine/src/other.rs": "pub fn other() {}\n",        # unchanged: ignored
                       "crates/core/machine/src/new_helper.rs": "pub fn h() {}\n"})
        ov = RA.overlay_of(self.prob, self.snap, d)
        self.assertEqual(sorted(ov), ["crates/core/machine/src/new_helper.rs", "crates/core/machine/src/toy.rs"])
        self.assertEqual(len(RA.overlay_digest(ov)), 64)

    def test_rejections(self) -> None:
        cases = [({"crates/core/machine/src/other.rs": "pub fn other() {}\n"}, "changes no file"),
                 ({"crates/core/machine/Cargo.toml": "[package]\nname = \"x\"\n"}, "protected"),
                 ({"boole-air-extract/src/main.rs": "fn main() { }\n"}, "protected"),
                 ({"crates/core/machine/src/toy.txt": "x\n"}, "only Rust"),
                 ({"crates/core/executor/src/x.rs": "pub fn x() {}\n"}, "editable roots"),
                 ({"crates/core/machine/src/toy.rs": "pub fn eval() { unsafe { } }\n"}, "unsafe"),
                 ({"crates/core/machine/src/toy.rs": "#[cfg(test)]\npub fn eval() {}\n"}, "cfg"),
                 ({"crates/core/machine/src/toy.rs": "pub fn eval() { let _ = std::any::TypeId::of::<u8>(); }\n"},
                  "TypeId|std::any"),
                 ({"crates/core/machine/src/toy.rs": "pub fn eval() { let _ = std::env::var(\"X\"); }\n"}, "std::env")]
        for files, pat in cases:
            with self.subTest(files=list(files)):
                with self.assertRaisesRegex(RA.Reject, pat):
                    RA.overlay_of(self.prob, self.snap, self.cand(files))

    def test_forbidden_words_in_comments_and_unchanged_lines_are_fine(self) -> None:
        d = self.cand({"crates/core/machine/src/toy.rs": "// toy chip\npub fn eval() {} // not unsafe here\n"})
        self.assertEqual(list(RA.overlay_of(self.prob, self.snap, d)), ["crates/core/machine/src/toy.rs"])

    def test_wildcard_roots(self) -> None:
        self.assertTrue(RA._under("extensions/rv32im/circuit/src/mul/core.rs", "extensions/*/circuit/src/"))
        self.assertFalse(RA._under("extensions/rv32im/transpiler/src/a.rs", "extensions/*/circuit/src/"))
        self.assertFalse(RA._under("extensions/a/b/circuit/src/a.rs", "extensions/*/circuit/src/"))

    def test_sync_tree_writes_only_changes_and_links(self) -> None:
        man = RA.manifest_of(self.snap)
        dest = os.path.join(self.tmp, "build", "repo")
        r = RA.sync_tree(self.snap, man, {}, dest)
        self.assertEqual(r["written"], len(man))
        self.assertEqual(os.readlink(os.path.join(dest, "LINK.md")), "README.md")
        toy = os.path.join(dest, "crates/core/machine/src/toy.rs")
        other = os.path.join(dest, "crates/core/machine/src/other.rs")
        t_other = os.stat(other).st_mtime_ns
        time.sleep(0.01)
        ov = {"crates/core/machine/src/toy.rs": b"// edited\n", "crates/core/machine/src/extra.rs": b"// new\n"}
        r = RA.sync_tree(self.snap, man, ov, dest)
        self.assertEqual(r, {"written": 2, "removed": 0})
        self.assertEqual(Path(toy).read_text(encoding="utf-8"), "// edited\n")
        self.assertEqual(os.stat(other).st_mtime_ns, t_other)          # untouched files keep their time
        r = RA.sync_tree(self.snap, man, {}, dest)                     # back to the snapshot
        self.assertEqual(r, {"written": 1, "removed": 1})
        self.assertFalse(os.path.exists(os.path.join(dest, "crates/core/machine/src/extra.rs")))

    def test_snapshot_links_are_pinned(self) -> None:
        os.remove(os.path.join(self.snap, "repo", "LINK.md"))
        os.symlink("Cargo.toml", os.path.join(self.snap, "repo", "LINK.md"))
        with self.assertRaisesRegex(RuntimeError, "symbolic links"):
            RA.manifest_of(self.snap)


# ------------------------------------------------------------------------------------------ problem and admissibility

class ProblemTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.mkdtemp(prefix="ratchet-air-prob-")
        self.pdir, self.prob = build_problem(self.tmp)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp)

    def test_open_problem_validates(self) -> None:
        prob = self.prob
        self.assertEqual(P.validate_problem(prob, self.pdir), [])
        self.assertFalse(P.has_statement(prob))
        self.assertEqual(prob["record"]["priced"], 11)
        self.assertEqual(prob["reference"]["messages"], dict(prob["reference"]["messages"], traces=2, rows=24))
        self.assertEqual(prob["reference"]["negative"], [])
        self.assertEqual(prob["simulate"]["max_height"], RA.MAX_HEIGHT)
        self.assertEqual(prob["reference"]["layout"]["fixed"], [])
        msgs = RT.read_json(os.path.join(self.pdir, "reference", "messages.json"))
        self.assertEqual([t["n_outputs"] for t in msgs["traces"]], [8, 8])
        bad = dict(prob, record=dict(prob["record"], priced=1))
        self.assertTrue(P.validate_problem(bad, self.pdir))
        os.remove(os.path.join(self.pdir, "reference", "machine.json"))
        self.assertTrue(any("missing" in e for e in P.validate_problem(prob, self.pdir)))

    def test_ineligible_references(self) -> None:
        reg = os.path.join(self.tmp, "registry", "001.Toy")
        spec = RA.reference_spec(reg)
        self.assertIsNone(RA.static_exclusion(spec))
        self.assertEqual(RA.static_exclusion(dict(spec, window_rows=2))[0], "two-row")
        self.assertEqual(RA.static_exclusion(dict(spec, gates={"G-FID": {"status": "FAIL"}}))[0], "no-rows")
        with self.assertRaises(RT.NotEligible) as cm:
            RA.build_problem(reg, None, os.path.join(self.tmp, "extract"), "", "", {}, os.path.join(self.tmp, "x"))
        self.assertEqual(cm.exception.code, "det")
        other = machine_dir(os.path.join(self.tmp, "extract2"), toy_doc("equivalent"))
        det = self.prob["det"]
        with self.assertRaises(RT.NotEligible) as cm:
            RA.build_problem(reg, det, other, "", "", {}, os.path.join(self.tmp, "y"))
        self.assertEqual(cm.exception.code, "rebuild")

    def evaluate(self, toy: dict, other: dict | None = None) -> dict:
        d = machine_dir(tempfile.mkdtemp(dir=self.tmp), toy, other)
        return RA.evaluate(self.prob, self.pdir, d)

    def test_evaluate_candidates(self) -> None:
        ev = self.evaluate(toy_doc("equivalent"))
        self.assertTrue(ev["report"]["smaller"])
        self.assertEqual(ev["report"]["cost"]["main_columns"], 4)
        ev = self.evaluate(toy_doc("reference"))
        self.assertFalse(ev["report"]["smaller"])
        self.assertEqual(ev["report"]["reduction_pct"], 0.0)
        ev = self.evaluate(toy_doc("degree"))                  # fewer columns, but a degree-3 constraint
        self.assertFalse(ev["report"]["smaller"])
        self.assertIn("constraints_deg_ge[3]", ev["report"]["larger_components"])

    def test_admissibility_of_the_extraction(self) -> None:
        cases = [((toy_doc("equivalent"), other_doc(0, "Changed")), "other AIR"),
                 ((toy_doc("equivalent", name="Renamed"),), "the target is"),
                 ((toy_doc("next-row"),), "one-row"),
                 ((toy_doc("fixed"),), "fixed variables"),
                 ((toy_doc("new-output"),), "bus interface")]
        for args, pat in cases:
            with self.subTest(pat=pat):
                with self.assertRaisesRegex(RA.Reject, pat):
                    self.evaluate(*args)

    def test_statement_and_checker_package(self) -> None:
        ev = self.evaluate(toy_doc("equivalent"))
        rep = {"candidate": {"overlay_sha256": H, "files": ["crates/core/machine/src/toy.rs"], **ev["report"]}}
        out = os.path.join(self.tmp, "cand")
        RA.package_candidate(self.prob, self.pdir, rep, ev, out, None)
        stmt = Path(os.path.join(out, "pkg", "Statement.lean")).read_text(encoding="utf-8")
        base, ref_ns, cand_ns = RA.namespaces(self.prob)
        self.assertIn(f"import {ref_ns}.Model", stmt)
        self.assertIn(f"import {cand_ns}.Model", stmt)
        self.assertIn("theorem equiv [Fact (Nat.Prime " + ref_ns + ".p)]", stmt)
        self.assertIn(f"∀ (f : List {ref_ns}.F) (x y : List {ref_ns}.Msg),", stmt)
        self.assertIn(f"{ref_ns}.Constraints w ∧ {ref_ns}.Assumptions w ∧ {ref_ns}.Fixed.map w = f ∧", stmt)
        self.assertIn("Cand.BusEq (Cand.In w) x ∧ Cand.BusEq (Cand.Out w) y) := by", stmt)
        self.assertEqual(stmt.count("sorry"), 1)
        self.assertIn(" ↔\n", stmt)
        cmodel = Path(out, "pkg", AL.model_relpath(cand_ns)).read_text(encoding="utf-8")
        self.assertIn("# Ratchet candidate model: SP1 v6.8.1 AIR `Toy`", cmodel)
        self.assertIn("abbrev nVars : ℕ := 4", cmodel)
        files = RA.checker_files(self.prob, stmt, cmodel)
        cprob = RA.candidate_problem(self.prob, rep["candidate"], files, stmt, H,
                                     RA.statement_assumptions(self.prob, ev["iface"], ev["bus"]))
        self.assertEqual(P.validate_problem(cprob, os.path.join(out, "pkg")), [])
        self.assertTrue(P.has_statement(cprob))
        lie = json.loads(json.dumps(cprob))
        lie["candidate"]["smaller"] = False
        self.assertIn("candidate.smaller disagrees with the record", P.validate_problem(lie))

    def test_committed_fixture_proof_follows_the_text_rules(self) -> None:
        _, _, stmt = equivalent_package(self.tmp, self.pdir, self.prob)
        sol = fixture_solution(stmt)
        probs, aux, body = C.text_check(stmt, sol, "equiv")
        self.assertEqual((probs, aux.strip()), ([], ""))
        self.assertEqual(C.forbidden_scan([("proof", body)]), [])
        self.assertNotIn("sorry", body)

    def test_battery_det_solution_follows_the_text_rules(self) -> None:
        reg = os.path.join(self.tmp, "registry", "001.Toy")
        stmt = Path(os.path.join(reg, "Statement.lean")).read_text(encoding="utf-8")
        sol = RA.battery_det_solution(stmt, RA.model_summary(reg), "triv_V3_grind")
        self.assertNotIn("sorry", sol)
        self.assertIn("    grind", sol)
        self.assertIn("intro w₁ w₂ h₁ h₂ ha₁ ha₂ hfix hin", sol)
        with self.assertRaises(ValueError):
            RA.battery_det_solution(stmt, RA.model_summary(reg), "triv_V1_exactQ")


# ------------------------------------------------------------------------------------------ screen

class ScreenTests(unittest.TestCase):
    def summaries(self, variant: str, docs=None) -> list[dict]:
        air = air_of(toy_doc(variant))
        bus = AB.model_for("sp1")
        roles, _ = bus.roles(air)
        docs = docs or [rows_doc(variant if variant != "equivalent" else "candidate")]
        return [RA.trace_summary(air, roles, bus, d, keep=4) for d in docs]

    def test_equivalent_candidate_passes(self) -> None:
        ref = self.summaries("reference")
        self.assertEqual((ref[0]["rows"], sum(ref[0]["outputs"].values()), ref[0]["constraint_failures"]),
                         (16, 8, 0))
        kinds, ex = RA.compare_runs(ref, self.summaries("equivalent"), "seed 0")
        self.assertEqual((dict(kinds), ex), ({}, []))

    def test_different_outputs_are_caught(self) -> None:
        kinds, ex = RA.compare_runs(self.summaries("reference"), self.summaries("nonequivalent",
                                    [rows_doc("nonequivalent")]), "seed 0")
        self.assertEqual(dict(kinds), {"different output contributions": 1})
        self.assertIn("message 0", ex[0]["detail"])

    def test_rows_violating_the_candidate_are_caught(self) -> None:
        kinds, _ = RA.compare_runs(self.summaries("reference"), self.summaries("nonequivalent",
                                   [rows_doc("candidate")]), "seed 0")
        self.assertEqual(dict(kinds), {"candidate row violates the candidate constraints": 1})
        bad = rows_doc("candidate")
        bad["main"]["0"] = [300, 1, 300, 1]                       # a = 300 is not a byte: the lookup fails
        kinds, _ = RA.compare_runs(self.summaries("reference"), self.summaries("equivalent", [bad]), "seed 0")
        self.assertEqual(dict(kinds), {"candidate row violates a table lookup or selector assumption": 1})

    def test_missing_traces_are_caught(self) -> None:
        kinds, _ = RA.compare_runs(self.summaries("reference"),
                                   self.summaries("equivalent", [rows_doc("candidate", source="core record 9")]), "s")
        self.assertEqual(kinds["trace missing"], 2)

    def test_reference_messages_round_trip(self) -> None:
        ref = self.summaries("reference")
        rec = RA.messages_record(ref)
        again = [RA._load_summary(json.loads(json.dumps(RA._dump_summary(s)))) for s in ref]
        self.assertEqual(RA.messages_record(again), rec)
        self.assertEqual(RA.compare_runs(ref, again, "s")[0], collections.Counter())

    def test_det_search_refutes_an_underconstrained_candidate(self) -> None:
        bus = AB.model_for("sp1")
        for variant, want in (("underconstrained", "counterexample"), ("equivalent", "no-counterexample")):
            air = air_of(toy_doc(variant))
            roles, _ = bus.roles(air)
            s = RA.trace_summary(air, roles, bus, rows_doc("candidate"), keep=8)
            res = RA.det_search(air, roles, bus, s["windows"], "fixture", 5.0)
            self.assertEqual(res["status"], want, variant)

    def test_det_screen_is_informational(self) -> None:
        bus = AB.model_for("sp1")
        for variant, want in (("equivalent", "DETERMINED"), ("underconstrained", "STUCK")):
            air = air_of(toy_doc(variant))
            roles, _ = bus.roles(air)
            self.assertEqual(RA.det_screen(air, roles)["propagation"], want)

    def test_screen_limits(self) -> None:
        self.assertEqual(RA.screen_limits(air_of(toy_doc("reference"))), (RA.MAX_TRACES, RA.MAX_HEIGHT))
        self.assertEqual(RA.screen_limits(air_of(dict(toy_doc("reference"), width=22))), (2, 1 << 16))
        self.assertEqual(RA.screen_limits(air_of(dict(toy_doc("reference"), width=660))), (3, 2048))


@unittest.skipUnless(LIVE_LEAN, "set BOOLE_ZK_RATCHET_LEAN_ENV (lean_runner environment JSON) for the Lean checks")
class LiveLeanTests(unittest.TestCase):
    def test_toy_statement_elaborates_and_the_checker_judges_the_fixture_proof(self) -> None:
        env = L.load_env(LIVE_LEAN)
        L.set_lean_limits(slots=1, rss_mb=RT.DET_PROOF_LIMITS["rss_mb"])
        pins = env.pins()
        with tempfile.TemporaryDirectory(prefix="ratchet-air-lean-") as root:
            pdir, prob = build_problem(root, dict(ENV, lean=pins["lean"], mathlib=pins["mathlib"],
                                                  lake_manifest_sha256=pins["lake_manifest_sha256"]))
            pkg, _, stmt = equivalent_package(root, pdir, prob, env)
            self.assertTrue(P.has_statement(RT.read_json(os.path.join(pkg, "problem.json"))))
            sol = os.path.join(root, "Solution.lean")
            RT.put(sol, fixture_solution(stmt))
            rep = C.check(pkg, sol, env, None, RT.DET_PROOF_LIMITS["timeout_s"], False, os.path.join(root, "chk"))
            self.assertEqual(rep["verdict"], "PASS", rep.get("fail") or rep.get("error") or rep.get("invalid"))
            RT.put(sol, stmt)
            rep = C.check(pkg, sol, env, None, RT.DET_PROOF_LIMITS["timeout_s"], False, os.path.join(root, "chk2"))
            self.assertEqual(rep["verdict"], "INVALID")


@unittest.skipUnless(LIVE_PROBLEM and LIVE_CAND and LIVE_BUILD and LIVE_REF_BUILD,
                     "set BOOLE_ZK_RATCHET_AIR_PROBLEM / _CANDIDATE / _BUILD / _REF_BUILD for the live pipeline")
class LiveTests(unittest.TestCase):
    def test_comment_only_candidate_is_admissible_equal_and_passes_the_screen(self) -> None:
        with tempfile.TemporaryDirectory(prefix="ratchet-air-live-") as out:
            rep = RA.simulate(LIVE_PROBLEM, LIVE_CAND, out, LIVE_BUILD, LIVE_REF_BUILD, 1)
            c = rep["candidate"]
            self.assertFalse(c["smaller"])
            self.assertEqual(c["larger_components"], [])
            self.assertEqual(rep["simulate"]["verdict"], "PASS", rep["simulate"]["examples"][:3])


if __name__ == "__main__":
    unittest.main()
