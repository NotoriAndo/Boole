"""Decomposition of TOO-LARGE gnark records into the functions they call.

For every TOO-LARGE record (wave or decomposition, up to :data:`MAX_DEPTH` levels) the direct callees of
its declaration (from the catalog: functions and methods called in its body) are resolved: a callee that
is a record of the wave (same catalog key) or an earlier decomposition record is only mapped; an exported
function or method of the repository module becomes a new record, instantiated, compiled, modelled and
gated exactly like a ledger row (``instantiation.decomposition`` names its parents and depth); unexported
functions (no wrapper can call them) and functions outside the module (gnark's frontend API, the Go
standard library) are recorded as edges only.  No compositional statement is made.
"""
from __future__ import annotations

import concurrent.futures as cf
import dataclasses
import hashlib
import json
import os

from zk_registry import gnark_det as D
from zk_registry import package as P

MAX_DEPTH = 4
MAX_CALLEES = 40


def split_key(key: str) -> tuple[str, str]:
    """``pkg/path.Name`` or ``pkg/path.Type.Method`` -> (package path, symbol)."""
    head, _, last = key.rpartition("/")
    first, _, symbol = last.partition(".")
    return (head + "/" + first if head else first), symbol


def run_decompose(cfg: D.WaveConfig, index_path: str, max_depth: int = MAX_DEPTH) -> tuple[list[dict], list[dict]]:
    with open(index_path, encoding="utf-8") as f:
        wave = [json.loads(x) for x in f if x.strip()]
    rows = {r["item_id"]: r for r in D.select_rows(cfg.ledger)}
    known: dict[str, dict] = {}                       # catalog key -> record
    for r in wave:
        k = r["evidence"].get("catalog_key")
        if k:
            known.setdefault(k, r)
    dcfg = dataclasses.replace(cfg, out=os.path.join(cfg.out, "decomposition"), work=cfg.work + "-decomp",
                               build=cfg.build + "-decomp", only=[])
    os.makedirs(dcfg.out, exist_ok=True)
    records: list[dict] = []
    edges: list[dict] = []
    parents = [(r, 0) for r in wave if r["status"] == "TOO-LARGE"]
    for depth in range(1, max_depth + 1):
        if not parents:
            break
        children: dict[str, dict] = {}                # key -> synthetic row
        child_parents: dict[str, list] = {}
        for prec, _ in parents:
            callees = (prec["evidence"].get("callees") or [])[:MAX_CALLEES]
            rid = prec["ids"]["repo"]
            index = _index(cfg, rid)
            for key in callees:
                e = {"parent": prec["package_id"], "callee": key, "depth": depth}
                if key in known:
                    e["resolution"] = "record of the wave" if known[key] in wave else "decomposition record"
                    e["child_package_id"] = known[key]["package_id"]
                elif key not in index:
                    e["resolution"] = "unresolved (not a function declared in the repository module)"
                else:
                    pkg, sym = split_key(key)
                    name = sym.rsplit(".", 1)[-1]
                    if not name[:1].isupper() or (("." in sym) and not sym.split(".")[0][:1].isupper()):
                        e["resolution"] = "unexported function of the module (no wrapper can call it)"
                    else:
                        e["resolution"] = "new decomposition record"
                        child_parents.setdefault(key, []).append(prec)
                        if key not in children:
                            fi = index[key]
                            src_row = rows.get(prec["ids"]["ledger_item_id"]) or {}
                            mdir = cfg.repos[rid].get("module_dir", "")
                            children[key] = {
                                "item_id": f"decomposition:{rid}:{key}", "repo": prec["ids"]["repo_url"],
                                "commit": prec["ids"]["commit"], "path": os.path.normpath(os.path.join(mdir, fi["file"])),
                                "symbol": sym, "framework": "gnark", "unit": "G1-gadget", "flags": [],
                                "coverage": "none", "census": [{"line": fi["line"], "pin": src_row.get("census", [{}])[0].get("pin", "")}],
                            }
                edges.append(e)
        if not children:
            break
        for key, row in children.items():
            ps = child_parents[key]
            row["_decomposition"] = {"parents": [{"package_id": p["package_id"], "call": key} for p in ps][:20],
                                     "n_parents": len(ps), "depth": depth,
                                     "content_sha256": hashlib.sha256(key.encode()).hexdigest(), "variants": 1,
                                     "instance_key": key}
        sh = D.setup_shared(dcfg, list(children.values()))
        new_parents = []
        with cf.ThreadPoolExecutor(max_workers=min(cfg.jobs, D.MAX_JOBS)) as pool:
            futs = {pool.submit(D.process, sh, row): key for key, row in children.items()}
            for fut in cf.as_completed(futs):
                rec = fut.result()
                key = futs[fut]
                tgt = sh.plans[children[key]["item_id"]][0]
                if tgt is not None and sh.repos.get(rec["ids"]["repo"]):
                    rec["evidence"]["content_sha256"] = D.content_key(sh.repos[rec["ids"]["repo"]], tgt) \
                        if tgt.get("found") else rec["evidence"].get("content_sha256")
                if rec["status"] in P.PACKAGED_STATUSES:
                    D.rewrite_problem(dcfg, rec)
                records.append(rec)
                known[key] = rec
                D.append_jsonl(os.path.join(dcfg.out, "PROGRESS.jsonl"), rec)
                if rec["status"] == "TOO-LARGE":
                    new_parents.append((rec, depth))
        parents = new_parents
    D.write_outputs(dcfg.out, records, [])
    D.write_index(os.path.join(dcfg.out, "EDGES.jsonl"), edges)
    return records, edges


_INDEX_CACHE: dict = {}


def _index(cfg: D.WaveConfig, rid: str) -> dict:
    if rid not in _INDEX_CACHE:
        path = os.path.join(cfg.build, D.repo_dir_name(rid), "catalog.json")
        with open(path, encoding="utf-8") as f:
            _INDEX_CACHE[rid] = json.load(f).get("index") or {}
    return _INDEX_CACHE[rid]
