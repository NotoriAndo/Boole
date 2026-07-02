#!/usr/bin/env python3
"""RED-able contract tests for the T0 reuse-signal offline experiment.

Pins the premise-DAG construction and reuse-signal metrics on a synthetic
mini-corpus with KNOWN reuse (see `local-docs/todo/thesis-realization-roadmap.md`,
"## T0 -- reuse-signal offline experiment"). The RED/GREEN cycle here is for
the TOOL (does it compute the DAG/metrics correctly on a fixture with a known
answer), not for the hypothesis ("is reuse a real signal on Boole's own
corpus" is answered by the experiment's report, not by a test assertion).
"""
from __future__ import annotations

import importlib.util
import json
import math
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = ROOT / "scripts" / "reuse_signal_experiment.py"


def load_experiment():
    spec = importlib.util.spec_from_file_location("reuse_signal_experiment", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _write(root: Path, relative: str, content: str) -> Path:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return path


def _build_known_reuse_corpus(root: Path) -> Path:
    """lemma A is a premise of B and C => reuse(A)=2, reuse(B)=0, reuse(C)=0."""
    corpus = root / "corpus"
    _write(corpus, "lemma_a.lean", "lemma A : True := trivial\n")
    _write(
        corpus,
        "lemma_b.lean",
        "lemma B : True := by\n  have h := A\n  trivial\n",
    )
    _write(
        corpus,
        "lemma_c.lean",
        "lemma C : True := by\n  have h := A\n  trivial\n",
    )
    return corpus


def _build_self_reference_corpus(root: Path) -> Path:
    corpus = root / "corpus"
    _write(
        corpus,
        "lemma_d.lean",
        "theorem D : True := by\n  have h := D\n  trivial\n",
    )
    return corpus


def _build_repeated_reference_corpus(root: Path) -> Path:
    corpus = root / "corpus"
    _write(corpus, "lemma_a.lean", "lemma A : True := trivial\n")
    _write(
        corpus,
        "lemma_e.lean",
        "lemma E : True := by\n  have h1 := A\n  have h2 := A\n  trivial\n",
    )
    return corpus


def _build_concentrated_corpus(root: Path) -> Path:
    """5 core declarations each reused by 9 leaves => a dominant head (n=50)."""
    corpus = root / "corpus"
    for i in range(5):
        _write(corpus, f"core_{i}.lean", f"lemma core{i} : True := trivial\n")
    for i in range(45):
        core_name = f"core{i % 5}"
        _write(
            corpus,
            f"leaf_{i}.lean",
            f"lemma leaf{i} : True := by\n  have h := {core_name}\n  trivial\n",
        )
    return corpus


def _build_uniform_ring_corpus(root: Path) -> Path:
    """50 declarations, each referenced by exactly one other => flat/no head."""
    corpus = root / "corpus"
    for i in range(50):
        target = f"ring{(i + 1) % 50}"
        _write(
            corpus,
            f"ring_{i}.lean",
            f"lemma ring{i} : True := by\n  have h := {target}\n  trivial\n",
        )
    return corpus


def _build_comment_and_string_mention_corpus(root: Path) -> Path:
    """A name mentioned ONLY inside a `-- line comment`, a `/- block
    comment -/` (including a NESTED block comment), a `/-- doc comment -/`,
    or a `"string literal"` must never be treated as a reference."""
    corpus = root / "corpus"
    _write(
        corpus,
        "mentions.lean",
        "theorem mentionedOnly : True := trivial\n"
        "\n"
        "theorem lineCommentCaller : True := trivial\n"
        "-- see mentionedOnly for details, not a real call\n"
        "\n"
        "theorem blockCommentCaller : True := trivial\n"
        "/- block comment referencing mentionedOnly -/\n"
        "\n"
        "theorem docCommentCaller : True := trivial\n"
        "/-- doc comment referencing mentionedOnly -/\n"
        "\n"
        "def stringCaller : String :=\n"
        '  "mentionedOnly appears in a string literal, not real code"\n'
        "\n"
        "theorem nestedBlockCommentCaller : True := trivial\n"
        "/- outer /- inner -/ mentions mentionedOnly here, still outer -/\n",
    )
    return corpus


def _build_doc_comment_misattribution_corpus(root: Path) -> Path:
    """`prevDecl`'s body must EXCLUDE names that appear only in the doc
    comment belonging to the NEXT declaration, `afterDoc`."""
    corpus = root / "corpus"
    _write(
        corpus,
        "doc_leak.lean",
        "theorem prevDecl : True := trivial\n"
        "\n"
        "/-- References `targetHelper` in prose, not code. -/\n"
        "theorem afterDoc : True := trivial\n"
        "\n"
        "theorem targetHelper : True := trivial\n",
    )
    return corpus


def _build_phantom_declaration_corpus(root: Path) -> Path:
    """A `theorem ...` line INSIDE a `/-- ... -/` doc comment must not be
    parsed as a real declaration, and must not truncate the preceding real
    declaration's body (which would drop its genuine reference)."""
    corpus = root / "corpus"
    _write(
        corpus,
        "d1.lean",
        "theorem realPrev : True := by\n"
        "  have h := realDependency\n"
        "  trivial\n"
        "\n"
        "/--\n"
        "theorem phantomInDoc : this text describes an example, not a real decl\n"
        "-/\n"
        "theorem realNext : True := trivial\n"
        "\n"
        "theorem realDependency : True := trivial\n",
    )
    return corpus


def _build_namespace_collision_corpus(root: Path) -> Path:
    """Two declarations sharing the bare name `dup` in different
    `namespace X ... end X` blocks must survive as distinct nodes, each
    resolving its own in-namespace bare reference to itself."""
    corpus = root / "corpus"
    _write(
        corpus,
        "ns.lean",
        "namespace NSX\n"
        "theorem dup : True := trivial\n"
        "theorem usesX : True := by\n"
        "  have h := dup\n"
        "  trivial\n"
        "end NSX\n"
        "\n"
        "namespace NSY\n"
        "theorem dup : True := trivial\n"
        "theorem usesY : True := by\n"
        "  have h := dup\n"
        "  trivial\n"
        "end NSY\n",
    )
    return corpus


def _build_dotted_reference_corpus(root: Path) -> Path:
    """`List.map f xs` must not count as a reference to an unrelated
    top-level corpus declaration named `map`."""
    corpus = root / "corpus"
    _write(
        corpus,
        "b2.lean",
        "theorem map : True := trivial\n"
        "\n"
        "theorem usesListMap : True := by\n"
        "  have h := List.map f xs\n"
        "  trivial\n",
    )
    return corpus


def _build_prime_boundary_corpus(root: Path) -> Path:
    """A body referencing only `foo'_aux` must not create an edge to the
    distinct, shorter declaration `foo'`."""
    corpus = root / "corpus"
    _write(
        corpus,
        "b3.lean",
        "theorem foo' : True := trivial\n"
        "\n"
        "theorem usesFooAux : True := by\n"
        "  have h := foo'_aux\n"
        "  trivial\n"
        "\n"
        "theorem foo'_aux : True := trivial\n",
    )
    return corpus


def _build_unicode_subscript_corpus(root: Path) -> Path:
    """`eval₂` (a mathlib-style subscript name) must be captured with
    its FULL name, and a reference to it must resolve to it, never to the
    distinct declaration `eval`."""
    corpus = root / "corpus"
    _write(
        corpus,
        "b4.lean",
        "theorem eval : True := trivial\n"
        "\n"
        "theorem eval₂ : True := trivial\n"
        "\n"
        "theorem usesEvalSubscript : True := by\n"
        "  have h := eval₂\n"
        "  trivial\n",
    )
    return corpus


def _reference_gini(values: list[int]) -> float:
    """Independent reference implementation (population Gini via mean absolute
    difference) used to cross-check the module's concentration metric without
    importing the module's own formula."""
    n = len(values)
    if n == 0:
        return 0.0
    total = sum(values)
    if total == 0:
        return 0.0
    mean = total / n
    diff_sum = sum(abs(a - b) for a in values for b in values)
    return diff_sum / (2 * n * n * mean)


def _reference_entropy_bits(values: list[int]) -> float:
    total = sum(values)
    if total <= 0:
        return 0.0
    entropy = 0.0
    for v in values:
        if v > 0:
            p = v / total
            entropy -= p * math.log2(p)
    return entropy


class DeclarationEnumerationTests(unittest.TestCase):
    def test_enumerates_declarations_across_corpus_files(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            files = module.find_lean_files([corpus])
            self.assertEqual(len(files), 3)
            declarations = module.parse_declarations(files)
            self.assertEqual(set(declarations.keys()), {"A", "B", "C"})

    def test_declaration_enumeration_across_multiple_corpus_dirs(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dir_one = root / "one"
            dir_two = root / "two"
            _write(dir_one, "a.lean", "lemma A : True := trivial\n")
            _write(dir_two, "b.lean", "lemma B : True := by\n  have h := A\n  trivial\n")
            files = module.find_lean_files([dir_one, dir_two])
            declarations = module.parse_declarations(files)
            self.assertEqual(set(declarations.keys()), {"A", "B"})


class PremiseDagTests(unittest.TestCase):
    def test_premise_dag_edges_reflect_known_reuse(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["A"], set())
            self.assertEqual(premise_dag["B"], {"A"})
            self.assertEqual(premise_dag["C"], {"A"})

    def test_self_reference_is_excluded_from_premises(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_self_reference_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["D"], set())
            reuse_counts = module.compute_reuse_counts(premise_dag)
            self.assertEqual(reuse_counts["D"], 0)
            self.assertEqual(module.total_premise_edges(premise_dag), 0)

    def test_repeated_reference_counts_once_as_single_dag_edge(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_repeated_reference_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["E"], {"A"})
            self.assertEqual(module.total_premise_edges(premise_dag), 1)
            reuse_counts = module.compute_reuse_counts(premise_dag)
            self.assertEqual(reuse_counts["A"], 1)


class ReuseCountTests(unittest.TestCase):
    def test_reuse_counts_are_in_degree_of_premise_dag(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            reuse_counts = module.compute_reuse_counts(premise_dag)
            self.assertEqual(reuse_counts, {"A": 2, "B": 0, "C": 0})
            self.assertEqual(module.total_premise_edges(premise_dag), 2)


class SummaryMetricsTests(unittest.TestCase):
    def test_histogram_and_never_reused_fraction_on_known_corpus(self) -> None:
        module = load_experiment()
        reuse_counts = {"A": 2, "B": 0, "C": 0}
        histogram = module.reuse_histogram(reuse_counts)
        self.assertEqual(histogram, {0: 2, 2: 1})
        self.assertAlmostEqual(module.never_reused_fraction(reuse_counts), 2 / 3)

    def test_histogram_and_never_reused_fraction_on_concentrated_corpus(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_concentrated_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            reuse_counts = module.compute_reuse_counts(premise_dag)
            self.assertEqual(len(reuse_counts), 50)
            self.assertEqual(module.total_premise_edges(premise_dag), 45)
            histogram = module.reuse_histogram(reuse_counts)
            self.assertEqual(histogram, {0: 45, 9: 5})
            self.assertAlmostEqual(module.never_reused_fraction(reuse_counts), 45 / 50)

    def test_top_reused_declarations_ranking(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_concentrated_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            reuse_counts = module.compute_reuse_counts(premise_dag)
            top = module.top_reused(reuse_counts, limit=10)
            self.assertEqual(len(top), 10)
            top_names = {name for name, _count in top[:5]}
            self.assertEqual(top_names, {"core0", "core1", "core2", "core3", "core4"})
            for _name, count in top[:5]:
                self.assertEqual(count, 9)
            for _name, count in top[5:]:
                self.assertEqual(count, 0)


class ConcentrationMetricsTests(unittest.TestCase):
    def test_gini_and_entropy_match_reference_implementation_on_concentrated_corpus(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_concentrated_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            reuse_counts = module.compute_reuse_counts(premise_dag)
            values = list(reuse_counts.values())

            gini = module.gini_coefficient(values)
            entropy = module.shannon_entropy_bits(values)

            self.assertAlmostEqual(gini, _reference_gini(values), places=9)
            self.assertAlmostEqual(entropy, _reference_entropy_bits(values), places=9)
            # A 5-of-50 dominant head must read as high inequality.
            self.assertGreater(gini, 0.7)

    def test_gini_and_entropy_match_reference_implementation_on_uniform_corpus(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_uniform_ring_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            reuse_counts = module.compute_reuse_counts(premise_dag)
            values = list(reuse_counts.values())

            gini = module.gini_coefficient(values)
            entropy = module.shannon_entropy_bits(values)

            self.assertAlmostEqual(gini, _reference_gini(values), places=9)
            self.assertAlmostEqual(entropy, _reference_entropy_bits(values), places=9)
            # A perfectly uniform in-degree-1 ring must read as (near-)zero inequality.
            self.assertAlmostEqual(gini, 0.0, places=9)

    def test_gini_and_entropy_are_bounded_and_defined_on_empty_input(self) -> None:
        module = load_experiment()
        self.assertEqual(module.gini_coefficient([]), 0.0)
        self.assertEqual(module.shannon_entropy_bits([]), 0.0)
        self.assertEqual(module.gini_coefficient([0, 0, 0]), 0.0)
        self.assertEqual(module.shannon_entropy_bits([0, 0, 0]), 0.0)


class VerdictTests(unittest.TestCase):
    def test_verdict_corpus_too_small_below_threshold(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            report = module.run_experiment([corpus])
            self.assertEqual(report["declarationCount"], 3)
            self.assertLess(report["declarationCount"], module.THRESHOLD_DECLARATIONS)
            self.assertEqual(report["verdict"], "corpus-too-small")

    def test_verdict_signal_on_concentrated_synthetic_corpus_above_threshold(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_concentrated_corpus(Path(tmp))
            report = module.run_experiment([corpus])
            self.assertEqual(report["declarationCount"], 50)
            self.assertGreaterEqual(report["declarationCount"], module.THRESHOLD_DECLARATIONS)
            self.assertEqual(report["verdict"], "signal")

    def test_verdict_flat_on_uniform_synthetic_corpus_above_threshold(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_uniform_ring_corpus(Path(tmp))
            report = module.run_experiment([corpus])
            self.assertEqual(report["declarationCount"], 50)
            self.assertGreaterEqual(report["declarationCount"], module.THRESHOLD_DECLARATIONS)
            self.assertEqual(report["verdict"], "flat")

    def test_threshold_constant_is_fifty(self) -> None:
        # Pinned per the T0 spec: below ~50 declarations, concentration
        # statistics are not meaningful (see module docstring/comment).
        module = load_experiment()
        self.assertEqual(module.THRESHOLD_DECLARATIONS, 50)


class RunExperimentReportTests(unittest.TestCase):
    def test_report_contains_all_required_fields_on_known_corpus(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            report = module.run_experiment([corpus])

            self.assertEqual(report["declarationCount"], 3)
            self.assertEqual(sorted(report["declarations"]), ["A", "B", "C"])
            self.assertEqual(report["totalPremiseEdges"], 2)
            self.assertEqual(report["reuseCounts"], {"A": 2, "B": 0, "C": 0})
            self.assertEqual(report["histogram"], {0: 2, 2: 1})
            self.assertAlmostEqual(report["neverReusedFraction"], 2 / 3)
            self.assertEqual(report["neverReusedCount"], 2)
            self.assertIn("giniCoefficient", report)
            self.assertIn("shannonEntropyBits", report)
            self.assertIn("topReused", report)
            self.assertEqual(report["topReused"][0], ("A", 2))
            self.assertEqual(report["verdict"], "corpus-too-small")
            self.assertIn("verdictCriteria", report)
            self.assertEqual(report["verdictCriteria"]["thresholdDeclarations"], 50)


class CliTests(unittest.TestCase):
    def test_cli_json_output_matches_library_report(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            proc = subprocess.run(
                ["python3", str(SCRIPT_PATH), "--corpus-dir", str(corpus), "--json"],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            payload = json.loads(proc.stdout)
            self.assertEqual(payload["declarationCount"], 3)
            self.assertEqual(payload["totalPremiseEdges"], 2)
            self.assertEqual(payload["verdict"], "corpus-too-small")
            self.assertEqual(payload["reuseCounts"], {"A": 2, "B": 0, "C": 0})

    def test_cli_human_readable_output_contains_verdict_and_key_metrics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_known_reuse_corpus(Path(tmp))
            proc = subprocess.run(
                ["python3", str(SCRIPT_PATH), "--corpus-dir", str(corpus)],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertNotIn("{", proc.stdout.split("\n")[0])
            self.assertIn("declarations", proc.stdout.lower())
            self.assertIn("verdict", proc.stdout.lower())
            self.assertIn("corpus-too-small", proc.stdout)

    def test_cli_requires_at_least_one_corpus_dir(self) -> None:
        proc = subprocess.run(
            ["python3", str(SCRIPT_PATH)],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("--corpus-dir", proc.stderr)

    def test_cli_accepts_repeated_corpus_dir_flags(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dir_one = root / "one"
            dir_two = root / "two"
            _write(dir_one, "a.lean", "lemma A : True := trivial\n")
            _write(dir_two, "b.lean", "lemma B : True := by\n  have h := A\n  trivial\n")
            proc = subprocess.run(
                [
                    "python3",
                    str(SCRIPT_PATH),
                    "--corpus-dir",
                    str(dir_one),
                    "--corpus-dir",
                    str(dir_two),
                    "--json",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            payload = json.loads(proc.stdout)
            self.assertEqual(payload["declarationCount"], 2)
            self.assertEqual(payload["reuseCounts"], {"A": 1, "B": 0})


# --------------------------------------------------------------------------
# v1.1 adversarial-review change set (see reuse_signal_experiment.py module
# docstring, "v1.1 changes from v1"). RED-first: these pin behavior the v1
# parser gets wrong.
# --------------------------------------------------------------------------


class CommentAndStringStrippingTests(unittest.TestCase):
    def test_comment_and_string_mentions_are_never_references(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_comment_and_string_mention_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["lineCommentCaller"], set())
            self.assertEqual(premise_dag["blockCommentCaller"], set())
            self.assertEqual(premise_dag["docCommentCaller"], set())
            self.assertEqual(premise_dag["stringCaller"], set())
            self.assertEqual(premise_dag["nestedBlockCommentCaller"], set())
            self.assertEqual(module.compute_reuse_counts(premise_dag)["mentionedOnly"], 0)


class DocCommentAttributionTests(unittest.TestCase):
    def test_doc_comment_is_not_attributed_to_preceding_declaration(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_doc_comment_misattribution_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["prevDecl"], set())
            self.assertEqual(premise_dag["afterDoc"], set())
            self.assertEqual(module.compute_reuse_counts(premise_dag)["targetHelper"], 0)


class PhantomDeclarationTests(unittest.TestCase):
    def test_fake_keyword_inside_doc_comment_does_not_create_declaration(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_phantom_declaration_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            self.assertNotIn("phantomInDoc", declarations)
            self.assertEqual(set(declarations.keys()), {"realPrev", "realNext", "realDependency"})
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["realPrev"], {"realDependency"})


class NamespaceQualificationTests(unittest.TestCase):
    def test_same_bare_name_in_different_namespaces_are_distinct_nodes(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_namespace_collision_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            self.assertEqual(
                set(declarations.keys()),
                {"NSX.dup", "NSX.usesX", "NSY.dup", "NSY.usesY"},
            )
            report = module.run_experiment([corpus])
            self.assertEqual(report["declarationCount"], 4)

            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["NSX.usesX"], {"NSX.dup"})
            self.assertEqual(premise_dag["NSY.usesY"], {"NSY.dup"})

            reuse_counts = module.compute_reuse_counts(premise_dag)
            self.assertEqual(reuse_counts["NSX.dup"], 1)
            self.assertEqual(reuse_counts["NSY.dup"], 1)


class DottedReferenceGuardTests(unittest.TestCase):
    def test_dotted_access_is_not_a_bare_reference(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_dotted_reference_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["usesListMap"], set())
            self.assertEqual(module.compute_reuse_counts(premise_dag)["map"], 0)


class PrimeBoundaryTests(unittest.TestCase):
    def test_prime_suffixed_name_does_not_leak_into_shorter_prefix(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_prime_boundary_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            self.assertEqual(set(declarations.keys()), {"foo'", "usesFooAux", "foo'_aux"})
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["usesFooAux"], {"foo'_aux"})
            self.assertEqual(module.compute_reuse_counts(premise_dag)["foo'"], 0)


class UnicodeIdentifierTests(unittest.TestCase):
    def test_unicode_subscript_identifier_is_captured_in_full_and_resolved_correctly(self) -> None:
        module = load_experiment()
        with tempfile.TemporaryDirectory() as tmp:
            corpus = _build_unicode_subscript_corpus(Path(tmp))
            declarations = module.parse_declarations(module.find_lean_files([corpus]))
            self.assertIn("eval₂", declarations)
            self.assertIn("eval", declarations)
            premise_dag = module.build_premise_dag(declarations)
            self.assertEqual(premise_dag["usesEvalSubscript"], {"eval₂"})
            reuse_counts = module.compute_reuse_counts(premise_dag)
            self.assertEqual(reuse_counts["eval₂"], 1)
            self.assertEqual(reuse_counts["eval"], 0)


class PerformanceTests(unittest.TestCase):
    def test_build_premise_dag_scales_subquadratically(self) -> None:
        """Pins the tokenize-once design (v1.1 change set item 5) against an
        O(declaration_count^2) regression: with ~3000 declarations, a
        per-name-pair rescan would take tens of seconds to minutes, while a
        tokenize-once pass finishes in well under a second. The budget below
        (20s) is deliberately generous -- this is a regression guard, not a
        tight perf benchmark -- so normal runtime stays a few seconds at
        most while remaining reliable in CI.
        """
        module = load_experiment()
        n = 3000
        filler = " ".join(f"tok{i}" for i in range(20))
        declarations: dict[str, module.Declaration] = {}
        for i in range(n):
            name = f"decl{i}"
            refs = " ".join(f"decl{(i + k) % n}" for k in range(1, 4))
            body = f"theorem {name} : True := by\n  have h := {refs}\n  {filler}\n"
            declarations[name] = module.Declaration(
                name=name,
                kind="theorem",
                file="synthetic",
                body=body,
                namespace="",
                qualified_name=name,
            )

        start = time.perf_counter()
        premise_dag = module.build_premise_dag(declarations)
        elapsed = time.perf_counter() - start

        self.assertEqual(len(premise_dag), n)
        self.assertLess(
            elapsed,
            20.0,
            f"build_premise_dag took {elapsed:.2f}s for n={n}; expected a "
            "tokenize-once, near-linear pass, not an O(n^2) per-name rescan.",
        )


class VerdictRoundingTests(unittest.TestCase):
    def test_top_k_uses_round_half_up_not_bankers_rounding(self) -> None:
        module = load_experiment()
        reuse_counts = {f"decl{i}": 0 for i in range(65)}
        reuse_counts["decl0"] = 1
        verdict_info = module.compute_verdict(65, reuse_counts, total_edges=1)
        # 65 * TOP_FRACTION(0.10) = 6.5 -> round-half-up = 7. Python's
        # built-in round() uses banker's rounding (round-half-to-even) and
        # would give 6 here, which is the bug this test pins.
        self.assertEqual(verdict_info["topK"], 7)


if __name__ == "__main__":
    unittest.main()
