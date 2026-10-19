"""Statement identity is semantic closure, independent of export intern IDs and proof bodies."""

import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from zk_registry import statement_identity as I
from zk_registry import package as P


def fixture(body=1):
    rows = [dict(meta=dict(format=dict(version="3.1.0")))]
    for index, name in enumerate(["True", "False", "meaning", "goal"], 1):
        rows.append({"in": index, "str": dict(pre=0, str=name)})
    rows += [{"ie": 0, "sort": 0}]
    rows += [{"ie": i, "const": dict(name=i, us=[])} for i in [1, 2, 3]]
    rows += [dict(axiom=dict(name=i, levelParams=[], type=0, isUnsafe=False)) for i in [1, 2]]
    rows += [
        dict(
            def_=dict(
                name=3, levelParams=[], type=0, value=body, hints="opaque", safety="safe", all=[3]
            )
        )
    ]
    rows[-1]["def"] = rows[-1].pop("def_")
    rows += [dict(thm=dict(name=4, levelParams=[], type=3, value=1, all=[4]))]
    return rows


class ClosureIdentity(unittest.TestCase):
    def test_every_package_schema_can_bind_a_semantic_identity(self):
        schemas = list((Path(P.__file__).parent / "schema").glob("*problem.schema.json"))
        self.assertEqual(len(schemas), 10)
        for schema in schemas:
            with self.subTest(schema=schema.name):
                prop = json.loads(schema.read_text())["properties"]["statement_closure_sha256"]
                self.assertEqual(set(prop["type"]), {"string", "null"})
                self.assertEqual(prop["pattern"], "^[0-9a-f]{64}$")

    def identity(self, rows):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "owned.ndjson"
            path.write_text("".join(json.dumps(row) + "\n" for row in rows))
            return I.identity(path, "goal")["statement_closure_sha256"]

    def test_same_name_and_type_different_definition_do_not_collide(self):
        records = []
        with tempfile.TemporaryDirectory() as temp:
            for body in [1, 2]:
                path = Path(temp) / str(body)
                path.write_text("".join(json.dumps(row) + "\n" for row in fixture(body)))
                records.append(I.identity(path, "goal"))
        self.assertEqual(records[0]["root_type_sha256"], records[1]["root_type_sha256"])
        self.assertNotEqual(
            records[0]["statement_closure_sha256"], records[1]["statement_closure_sha256"]
        )

    def test_byte_identical_rebuild_is_identical(self):
        self.assertEqual(self.identity(fixture()), self.identity(copy.deepcopy(fixture())))

    def test_theorem_proof_and_unreachable_definition_do_not_affect_identity(self):
        first = fixture()
        second = copy.deepcopy(first)
        second[-1]["thm"]["value"] = 2
        second.append({"ie": 9, "sort": 0})
        second[-4]["axiom"]["type"] = 9
        self.assertEqual(self.identity(first), self.identity(second))

    def test_intern_name_ids_are_not_identity(self):
        first = fixture()
        second = copy.deepcopy(first)
        for row in second:
            if "in" in row:
                row["in"] += 20
            if "const" in row:
                row["const"]["name"] += 20
            for tag in ["axiom", "def", "thm"]:
                if tag in row:
                    row[tag]["name"] += 20
                    if "all" in row[tag]:
                        row[tag]["all"] = [i + 20 for i in row[tag]["all"]]
        self.assertEqual(self.identity(first), self.identity(second))

    def test_dangling_semantic_dependency_fails_closed(self):
        rows = fixture()
        rows = [r for r in rows if r.get("axiom", {}).get("name") != 1]
        with self.assertRaises(ValueError):
            self.identity(rows)

    def test_transport_is_validated_even_outside_semantic_closure(self):
        for mutation in ["proof-ref", "hints", "unreachable-expr", "empty-inductive", "meta"]:
            rows = fixture()
            if mutation == "proof-ref":
                rows[-1]["thm"]["value"] = 999
            elif mutation == "hints":
                rows[-2]["def"]["hints"] = {"bogus": -1}
            elif mutation == "unreachable-expr":
                rows.append({"ie": 99, "sort": 999})
            elif mutation == "empty-inductive":
                rows.append({"inductive": {"types": [], "ctors": [], "recs": []}})
            elif mutation == "meta":
                rows[0]["meta"] = []
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.identity(rows)

    def test_nested_recursor_external_constructor_is_in_closure(self):
        rows = fixture()
        for index, name in [(5, "External"), (6, "make"), (7, "Nested"), (8, "rec")]:
            rows.append({"in": index, "str": dict(pre=0, str=name)})

        def typ(name, ctors, nested=0):
            return dict(
                name=name,
                levelParams=[],
                type=0,
                numParams=0,
                numIndices=0,
                all=[name],
                ctors=ctors,
                numNested=nested,
                isRec=False,
                isReflexive=False,
                isUnsafe=False,
            )

        rows.append(
            {
                "inductive": dict(
                    types=[typ(5, [6])],
                    ctors=[
                        dict(
                            name=6,
                            levelParams=[],
                            type=0,
                            induct=5,
                            cidx=0,
                            numParams=0,
                            numFields=0,
                            isUnsafe=False,
                        )
                    ],
                    recs=[],
                )
            }
        )
        rows.append(
            {
                "inductive": dict(
                    types=[typ(7, [], 1)],
                    ctors=[],
                    recs=[
                        dict(
                            name=8,
                            levelParams=[],
                            type=0,
                            all=[7],
                            numParams=0,
                            numIndices=0,
                            numMotives=1,
                            numMinors=0,
                            rules=[dict(ctor=6, nfields=0, rhs=0)],
                            k=False,
                            isUnsafe=False,
                        )
                    ],
                )
            }
        )
        rows.append({"ie": 4, "const": dict(name=7, us=[])})
        next(r["thm"] for r in rows if "thm" in r)["type"] = 4
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "nested.ndjson"
            path.write_text("".join(json.dumps(r) + "\n" for r in rows))
            self.assertEqual(I.identity(path, "goal")["semantic_constants"], 4)

    def test_duplicate_json_keys_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "bad.ndjson"
            path.write_text('{"ie":0,"ie":1,"sort":0}\n')
            with self.assertRaises(ValueError):
                I.identity(path, "goal")


class WorkerAvailability(unittest.TestCase):
    def test_worker_failures_are_errors_not_submission_rejections(self):
        from unittest.mock import patch
        from zk_registry import closure as D, lean_runner as L

        with tempfile.TemporaryDirectory() as temp:
            export = Path(temp) / "owned.ndjson"
            export.write_text("".join(json.dumps(row) + "\n" for row in fixture()))
            valid = I.identity(export, "goal")
            cases = [
                ("crash", L.RunResult(2, "", 0, False)),
                ("memory", L.RunResult(-9, "", 0, False, memkill=True)),
                ("observation", L.RunResult(1, "", 0, False, killed_by="watch-unavailable")),
                ("array", L.RunResult(0, "[]", 0, False)),
                ("bad-json", L.RunResult(0, "{", 0, False)),
            ]
            for field, value in [
                ("statement_closure_sha256", "NOTHEX"),
                ("export_sha256", "0" * 64),
                ("statement_closure_version", "unknown"),
            ]:
                bad = dict(valid)
                bad[field] = value
                cases.append((field, L.RunResult(0, json.dumps(bad), 0, False)))
            for label, result in cases:
                with self.subTest(label=label), patch.object(
                    D.CR, "runner", return_value=lambda *a, **k: result
                ):
                    self.assertEqual(
                        D.statement_identity(str(export), "goal", str(Path(temp) / label), 10)[
                            "verdict"
                        ],
                        "ERROR",
                    )
            for exc in [OSError("start unavailable"), ValueError("invalid worker")]:
                with patch.object(D.CR, "runner", side_effect=exc):
                    self.assertEqual(
                        D.statement_identity(str(export), "goal", str(Path(temp) / "start"), 10)[
                            "verdict"
                        ],
                        "ERROR",
                    )
            with patch.object(
                D.CR,
                "runner",
                return_value=lambda *a, **k: L.RunResult(0, json.dumps(valid), 0, False),
            ):
                self.assertEqual(
                    D.statement_identity(str(export), "goal", str(Path(temp) / "valid"), 10)[
                        "verdict"
                    ],
                    "PASS",
                )


if __name__ == "__main__":
    unittest.main()
