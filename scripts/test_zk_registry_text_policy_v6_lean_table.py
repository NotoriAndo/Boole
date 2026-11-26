"""Recheck the reported text-policy verdicts without running Lean or the registry."""

from collections import Counter
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from measure_registry_policy_v6 import (
    CHECK, INPUTS, NUMERALS, POLICY5, POLICY6, ROOT, columns, pinned, pure_checker,
)


FIXTURE = ROOT / "fixtures/submission-policy/lean-v5-v6-table.json"
STATUSES = ("parsed-as-construct", "parse-error", "parsed-as-other")
PREFIX_COMMANDS = {"register_item": "register_tactic_tag", "declare_item": "declare_syntax_cat"}


def reported(scan, line, label):
    prefix = f"{label} in helper (line 1 of that part)"
    return any(text == prefix or text.startswith(prefix + " [")
               for text in scan([("helper", line)]))


class LeanTextPolicyTable(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.table = json.loads(FIXTURE.read_bytes())
        if cls.table["input_sha256"] != INPUTS:
            raise AssertionError("measurement pins differ from the text checker pins")
        checker = pinned(ROOT / CHECK, INPUTS[CHECK])
        cls.numerals = json.loads(pinned(ROOT / NUMERALS, INPUTS[NUMERALS]))
        policies = {
            name: json.loads(pinned(ROOT / path, INPUTS[path]))
            for name, path in (("v5", POLICY5), ("v6", POLICY6))
        }
        cls.scans = {name: pure_checker(checker, policy) for name, policy in policies.items()}
        cls.rule_columns = columns(policies["v6"])

    def test_contains_the_measured_cartesian_product(self):
        rows = self.table["rows"]
        policy_columns = self.table["columns"]
        self.assertEqual(len(rows), 40)
        self.assertEqual(rows, self.numerals)
        self.assertEqual(len(policy_columns), 55)
        labels = dict(self.rule_columns)
        command_columns = [(command, labels[placeholder]) for placeholder, command in PREFIX_COMMANDS.items()]
        self.assertEqual([(col["text"], col["label"]) for col in policy_columns],
                         self.rule_columns + command_columns)
        for column, placeholder in zip(policy_columns[-len(PREFIX_COMMANDS):], PREFIX_COMMANDS, strict=True):
            opener = self.table["opener_controls"][placeholder]
            self.assertEqual(column["text"] + column["tail"], opener["form"])
        self.assertEqual(set(self.table["cells"]), {row["name"] for row in rows})
        for row in rows:
            with self.subTest(row=row["name"]):
                self.assertEqual(len(self.table["cells"][row["name"]]), len(policy_columns))
        entries = [cell for row in self.table["cells"].values() for cell in row]
        self.assertEqual(len(entries), 2200)
        counts = Counter(cell["lean_status"] for cell in entries)
        totals = self.table["totals"]
        for status in STATUSES:
            self.assertEqual(counts[status], totals[f"lean_{status}"])

    def test_pinned_checkers_reproduce_each_recorded_verdict(self):
        for row, column, cell, text in self.cases():
            with self.subTest(row=row, word=column["text"]):
                for version, scan in self.scans.items():
                    self.assertIs(reported(scan, text, column["label"]), cell[f"{version}_caught"], version)
        labels = dict(self.rule_columns)
        for word, control in self.table["opener_controls"].items():
            with self.subTest(opener=word):
                for version, scan in self.scans.items():
                    self.assertIs(reported(scan, control["form"], labels[word]),
                                  control[f"{version}_caught"], version)

    def test_v6_catches_every_construct_identified_by_lean(self):
        for row, column, cell, text in self.cases():
            with self.subTest(row=row, word=column["text"]):
                self.assertIn(cell["lean_status"], STATUSES)
                if cell["lean_status"] == "parsed-as-construct":
                    self.assertIs(cell["v6_caught"], True, "Lean parsed this cell but v6 missed it")
                shifted = cell.get("shifted")
                if shifted:
                    self.assertTrue(shifted["labels"])
                    for label in shifted["labels"]:
                        self.assertTrue(reported(self.scans['v6'], text, label), f"missed shifted construct {label}")

    def cases(self):
        method = self.table["method"]
        for row in self.table["rows"]:
            for column, cell in zip(self.table["columns"], self.table["cells"][row["name"]], strict=True):
                if column["context"] == "tactic":
                    prefix = method["tactic_prefix"]
                else:
                    prefix = method["command_prefix"].replace("<row type>", row["type"])
                text = prefix + row["literal"] + column["text"] + column["tail"]
                yield row["name"], column, cell, text


if __name__ == "__main__":
    unittest.main()
